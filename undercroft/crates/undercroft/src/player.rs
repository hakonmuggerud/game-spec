//! The player: movement, the handlamp, flash, planted lanterns, pickups, the interaction target and the
//! in-run key handler. Port of `main.js:updatePlayer` / `updateLamp` / `toggleLamp` / `topUp` / `flash` /
//! `plantLantern` / `pickup` / `gateTarget` / `shortcutTarget` / `interactTarget` / `interact` and its
//! `keydown` listener (`main.js:551–640`), plus `world.js`'s water-boundary events and `hub.js:exploreTick`.
//!
//! Everything that *decides* is in `undercroft_sim`; this module is the shell that feeds it the frame's
//! inputs, writes the results back into the shared resources ([`Player`], [`LampRes`], [`ZoneRes`], …) and
//! turns the returned `Vec<SimEvent>` into [`SimMessage`]s.
//!
//! Frame order inside [`SimSet::Player`] (the JS ran the key handler between frames, then `updatePlayer`,
//! then `updateLamp`):
//!
//! 1. [`player_actions`] — the queued one-shot actions (E, F, Q, R, T and the raw keys).
//! 2. [`player_movement`] — [`MoveIntent`] → look, `collision::move_player`, the cell flags, `waterEnter` /
//!    `waterExit`.
//! 3. [`player_lamp`] — `economy::update_lamp`, `hub.js:exploreTick`, and the tick's [`PlayerViewRes`].
//!
//! What this module never does: change [`GameMode`], open a menu, run the fade or bank. Those are `run.rs`'s;
//! where the JS `interact()` called `actions.bank()` / `actions.descend()` this pushes the matching
//! [`DebugCommand`] into [`DebugQueue`] (PHASE2_SKELETON §6). The push happens in [`SimSet::Player`], after
//! [`DebugSet::Drain`] has already run, so the command waits in the queue for `run.rs` on the *next* tick.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::collections::VecDeque;

use undercroft_data::{CellKind, GameData, ParsedMap};
use undercroft_sim::collision;
use undercroft_sim::contracts::{self, ContractEnv};
use undercroft_sim::creature::{self, Ctx, Env};
use undercroft_sim::economy::{self, HubInteract};
use undercroft_sim::events::InteractTarget;
use undercroft_sim::grid;
use undercroft_sim::player::{Carried, PlayerView};
use undercroft_sim::pool::{self, Lantern};
use undercroft_sim::world;
use undercroft_sim::SimEvent;

use crate::debug::{DebugCommand, DebugQueue, DebugSet};
use crate::messages::{emit, SimMessage};
use crate::resources::{
    Game, HubMapRes, HubRes, LampRes, MoveIntent, Npcs, Player, PlayerViewRes, RngRes, SaveRes,
    Zone, ZoneRes,
};
use crate::state::{self, GameMode};
use crate::tick::{Clock, SimSet};

/* ============================================================
Interaction targets (main.js:interactTarget)
============================================================ */

/// What `E` would do right now — the resolved `main.js:interactTarget()` result, in the JS's own priority
/// order (endgame → NPC → hub → items → gates → shortcuts → stairs). `run()` is not a closure here: the
/// variant carries what [`do_interact`] needs, and the parts `run.rs` owns are pushed as [`DebugCommand`]s.
#[derive(Debug, Clone, PartialEq)]
pub enum Interaction {
    /// `{type:'item'}` — the index into [`Zone::items`].
    Item { index: usize },
    /// `{type:'bank'}` — the zone's extraction marker; `empty` mirrors the JS hint flag.
    Bank { empty: bool },
    /// `{type:'descend'}` — the hub stairs. `lock` is `MAPS.zoneLocked(...)`, non-`None` when refused.
    Descend {
        zone_id: String,
        lock: Option<String>,
    },
    /// `{type:'gate'}` — a closed `X` cell in reach and in front.
    Gate { cx: i32, cz: i32, locked: bool },
    /// `{type:'shortcut'}` — a barred `=` cell; `barred` is the wrong-side case (`uiError`, no toast).
    Shortcut {
        cx: i32,
        cz: i32,
        name: Option<String>,
        barred: bool,
    },
    /// `{type:'npc'}` — free a captive (zone) or talk to a resident (hub).
    Npc { id: String, free: bool },
    /// `{type:'building'}` / `{type:'build'}` — a hub building, built or a ghost.
    Building { id: String, built: bool },
    /// `{type:'altar'}` — kneel at the Source (`endgame.js:openChoice`).
    Altar,
    /// `{type:'rideUp'}` — the Source elevator (`endgame.js:rideUp`).
    Elevator,
}

impl Interaction {
    /// The `interact.target.type` the JS puts on the event.
    pub fn kind(&self) -> InteractTarget {
        match self {
            Interaction::Item { .. } => InteractTarget::Item,
            Interaction::Bank { .. } => InteractTarget::Bank,
            Interaction::Descend { .. } => InteractTarget::Descend,
            Interaction::Gate { .. } => InteractTarget::Gate,
            Interaction::Shortcut { .. } => InteractTarget::Shortcut,
            Interaction::Npc { .. } => InteractTarget::Npc,
            Interaction::Building { .. } => InteractTarget::Building,
            Interaction::Altar => InteractTarget::Altar,
            Interaction::Elevator => InteractTarget::Elevator,
        }
    }
}

/* ============================================================
Queued one-shot actions
============================================================ */

/// One player action to run this tick. Filled by [`handle_debug`] (and, later, by the world lane's real
/// input in [`SimSet::Input`]); drained by [`player_actions`].
///
/// They are not run inside [`DebugSet::Handle`] on purpose: `interact` pushes commands `run.rs` owns, and the
/// foundation's [`DebugSet::Drain`] sweep would throw those away in the same tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerAction {
    /// `actions.toggleLamp()` / `KEYS.lamp`.
    ToggleLamp,
    /// `actions.flash()` / `KEYS.flash`.
    Flash,
    /// `actions.plantLantern()` / `KEYS.lantern`.
    PlantLantern,
    /// `actions.topUp()` / `KEYS.topUp`.
    TopUp,
    /// `actions.interact()` / `KEYS.interact`.
    Interact,
    /// A raw `KeyboardEvent.code` through `main.js`'s `keydown` handler.
    Key(String),
}

/// Actions waiting for [`SimSet::Player`], oldest first.
#[derive(Resource, Debug, Default)]
pub struct PlayerActions(pub VecDeque<PlayerAction>);

impl PlayerActions {
    /// Queue one action.
    pub fn push(&mut self, a: PlayerAction) {
        self.0.push_back(a);
    }
}

/// `hub.js` `exploreT` — the 0.25 s minimap-exploration cadence. Reset outside `ZONE`, exactly as
/// `hub.js:onDescend` does (`exploreT = 0`).
#[derive(Resource, Debug, Default)]
pub struct ExploreTimer(pub f32);

/* ============================================================
Shared system parameters
============================================================ */

/// Everything the player systems read and write. Grouped so the systems stay inside Bevy's parameter limit;
/// the fields are disjoint resources, so borrowing two of them at once is fine.
#[derive(SystemParam)]
pub struct Sim<'w> {
    pub game: Game<'w>,
    /// `ctx.state.mode`, pending change included — the same notion of "now" `run.rs`'s `ModeParam`
    /// has, so a mode `run.rs` set earlier in this very tick (`start_run` → `Zone`) is already
    /// visible here (see [`state::Mode`]).
    pub mode: state::Mode<'w>,
    pub player: ResMut<'w, Player>,
    pub lamp: ResMut<'w, LampRes>,
    pub zone: ResMut<'w, ZoneRes>,
    pub hub_map: Res<'w, HubMapRes>,
    pub npcs: ResMut<'w, Npcs>,
    pub save: ResMut<'w, SaveRes>,
    pub hub: ResMut<'w, HubRes>,
    pub rng: ResMut<'w, RngRes>,
    pub clock: Res<'w, Clock>,
}

impl Sim<'_> {
    /// The data asset has finished loading. Every system bails out while `GameMode::Loading` is still
    /// waiting for `assets/data/game.gamedata.ron`: there is no `CFG` to read yet.
    fn ready(&self) -> bool {
        self.game.get().is_some()
    }

    /// `ctx.state.mode`.
    fn mode(&self) -> GameMode {
        self.mode.get()
    }

    /// `main.js:zoneMul()` — the loaded zone's burn / lamp multipliers (the JS reads `ctx.zone.meta` even in
    /// the hub, where `onDeep` is false so only the zone and light-tech factors apply).
    fn zone_mul(&self) -> economy::ZoneMul {
        let data = self.game.data();
        let zone = self.zone.id().and_then(|id| data.zone(id));
        let tech = economy::light_tech(&data.config, &self.save.0);
        economy::zone_mul(
            &data.config,
            zone,
            tech,
            self.player.on_deep,
            self.player.lap,
        )
    }

    /// `CFG.lampDist × zoneMul().dist` — the handlamp light's `distance` this tick.
    fn lamp_reach(&self) -> f32 {
        self.game.config().cfg.lamp_dist * self.zone_mul().dist
    }

    /// The snapshot every sim call takes, built from the *current* player and lamp (not the one
    /// [`PlayerViewRes`] holds, which is last tick's).
    fn view(&self) -> PlayerView {
        self.player
            .view(&self.lamp.0, self.lamp_reach(), self.npcs.0.stimulus())
    }
}

/* ============================================================
Debug commands (DebugSet::Handle)
============================================================ */

/// `window.__game.actions` — take the seven commands the player owns. `teleport` is applied here (it only
/// moves the player); everything else becomes a [`PlayerAction`] so the pushes it makes survive
/// [`DebugSet::Drain`].
fn handle_debug(
    mut queue: ResMut<DebugQueue>,
    mut acts: ResMut<PlayerActions>,
    mut player: ResMut<Player>,
) {
    let taken = queue.take(|c| {
        matches!(
            c,
            DebugCommand::Flash
                | DebugCommand::PlantLantern
                | DebugCommand::Interact
                | DebugCommand::TopUp
                | DebugCommand::ToggleLamp
                | DebugCommand::Teleport { .. }
                | DebugCommand::Key(_)
        )
    });
    for cmd in taken {
        match cmd {
            DebugCommand::Flash => acts.push(PlayerAction::Flash),
            DebugCommand::PlantLantern => acts.push(PlayerAction::PlantLantern),
            DebugCommand::Interact => acts.push(PlayerAction::Interact),
            DebugCommand::TopUp => acts.push(PlayerAction::TopUp),
            DebugCommand::ToggleLamp => acts.push(PlayerAction::ToggleLamp),
            DebugCommand::Key(code) => acts.push(PlayerAction::Key(code)),
            // `actions.teleport(x, z, yaw?)` — move the player within the current map (tests).
            DebugCommand::Teleport { x, z, yaw } => {
                player.x = x;
                player.z = z;
                if let Some(y) = yaw {
                    player.yaw = y;
                }
            }
            _ => unreachable!("handle_debug took a command it does not own"),
        }
    }
}

/* ============================================================
Actions (SimSet::Player)
============================================================ */

/// Run the queued [`PlayerAction`]s: `toggleLamp`, `flash`, `plantLantern`, `topUp`, `interact` and the raw
/// key handler, in the order they arrived.
fn player_actions(
    mut w: Sim,
    mut acts: ResMut<PlayerActions>,
    mut queue: ResMut<DebugQueue>,
    menu: Res<crate::state::MenuKind>,
    fade: Res<crate::resources::Fade>,
    mut ev: MessageWriter<SimMessage>,
) {
    if !w.ready() {
        return;
    }
    let fade_out = fade.mode == crate::resources::FadeMode::Out;
    let pending: Vec<PlayerAction> = acts.0.drain(..).collect();
    for a in pending {
        match a {
            PlayerAction::ToggleLamp => {
                toggle_lamp(&mut w, &mut ev);
            }
            PlayerAction::Flash => {
                flash(&mut w, &mut ev);
            }
            PlayerAction::PlantLantern => {
                plant_lantern(&mut w, &mut ev);
            }
            PlayerAction::TopUp => {
                top_up(&mut w, &mut ev);
            }
            PlayerAction::Interact => {
                interact(&mut w, &mut queue, fade_out, &mut ev);
            }
            PlayerAction::Key(code) => {
                on_key(&mut w, &mut queue, &menu, fade_out, &code, &mut ev);
            }
        }
    }
}

/// `main.js:toggleLamp()` — F.
fn toggle_lamp(w: &mut Sim, ev: &mut MessageWriter<SimMessage>) -> bool {
    let in_zone = w.mode() == GameMode::Zone;
    let (ok, evs) = economy::toggle_lamp(&mut w.lamp.0, in_zone);
    emit(ev, evs);
    ok
}

/// `main.js:topUp()` — T: a carried flask into the handlamp.
fn top_up(w: &mut Sim, ev: &mut MessageWriter<SimMessage>) -> bool {
    let in_zone = w.mode() == GameMode::Zone;
    // `game`, `lamp` and `player` are disjoint fields of `Sim`, so these three borrows coexist.
    let (ok, evs) = economy::top_up(
        &mut w.lamp.0,
        w.game.config(),
        &mut w.player.carried,
        in_zone,
    );
    emit(ev, evs);
    ok
}

/// `main.js:flash()` — Q: the burst (`economy::flash` pays for it) and what it does to the creatures in the
/// cone (`creature::flash`). The JS emits the creature reactions first and `flash {x, z}` last.
fn flash(w: &mut Sim, ev: &mut MessageWriter<SimMessage>) -> bool {
    let in_zone = w.mode() == GameMode::Zone;
    let (x, z) = (w.player.x, w.player.z);
    let time = w.clock.time;
    let view = w.view();
    let (ok, evs) = {
        let cfg = w.game.config();
        let tech = economy::light_tech(cfg, &w.save.0);
        economy::flash(&mut w.lamp.0, cfg, tech, in_zone, x, z)
    };
    if !ok {
        emit(ev, evs);
        return false;
    }
    let tuning = w.game.tuning();
    let mut hits = Vec::new();
    if let Some(zone) = w.zone.get_mut() {
        let Zone {
            map,
            pool,
            lanterns,
            hunters,
            items,
            ..
        } = zone;
        let lpos: Vec<(f32, f32)> = lanterns.iter().map(|l| (l.x, l.z)).collect();
        let ipos: Vec<(f32, f32)> = items.iter().map(|i| (i.x, i.z)).collect();
        let env = Env {
            map,
            pool,
            player: &view,
            lanterns: &lpos,
            items: &ipos,
            in_zone: true,
            time,
        };
        let mut ctx = Ctx::new(&env, tuning, &mut w.rng.0, &mut hits);
        creature::flash(&mut ctx, hunters);
    }
    emit(ev, hits);
    emit(ev, evs);
    true
}

/// `main.js:plantLantern()` — R: pay, recycle the oldest lantern at `CFG.lanternMax`, plant, recompute the
/// pool bitmap (`hunter.js`'s `lantern` listener) and let the contracts see it (`contracts.js:onLantern`).
fn plant_lantern(w: &mut Sim, ev: &mut MessageWriter<SimMessage>) -> bool {
    let in_zone = w.mode() == GameMode::Zone;
    let (x, z) = (w.player.x, w.player.z);
    let time = w.clock.time;
    let (pool_r, lantern_max) = {
        let c = &w.game.config().cfg;
        (c.pool_r, c.lantern_max)
    };
    let (count, oldest) = match w.zone.get() {
        Some(z) => (
            z.lanterns.len() as u32,
            z.lanterns.first().map(|l| (l.x, l.z)),
        ),
        None => (0, None),
    };
    let (ok, evs) = {
        let cfg = w.game.config();
        let tech = economy::light_tech(cfg, &w.save.0);
        economy::plant_lantern(&mut w.lamp.0, cfg, tech, in_zone, count, oldest, x, z)
    };
    if !ok {
        emit(ev, evs);
        return false;
    }
    if let Some(zone) = w.zone.get_mut() {
        if zone.lanterns.len() as u32 >= lantern_max && !zone.lanterns.is_empty() {
            zone.lanterns.remove(0);
        }
        zone.lanterns.push(Lantern::new(x, z, time));
        let Zone {
            map,
            pool,
            lanterns,
            ..
        } = zone;
        pool.recompute(map, lanterns, pool_r);
    }
    emit(ev, evs);
    // `contracts.js`'s `lantern` listener — a `plant` contract completes where the lantern lands
    if let Some(zone) = w.zone.0.as_ref() {
        let env = ContractEnv::zone(w.game.data(), &zone.id, &zone.map);
        let evs = contracts::on_lantern(&env, &mut w.save.0, &mut w.hub.0, x, z);
        emit(ev, evs);
    }
    true
}

/* ============================================================
Interaction (main.js:interactTarget / interact)
============================================================ */

/// `main.js:gateTarget()` — the closed gate in reach (`CFG.interactR + 0.5`) that the player faces.
fn gate_target(
    map: &ParsedMap,
    doors: &world::ZoneDoors,
    p: &Player,
    r: f32,
) -> Option<(i32, i32)> {
    let (fx, fz) = (-p.yaw.sin(), -p.yaw.cos());
    for (i, g) in map.gates.iter().enumerate() {
        if doors.gate_open(i) {
            continue;
        }
        let (px, pz) = grid::center(map, g.marker.cx, g.marker.cz);
        let (dx, dz) = (px - p.x, pz - p.z);
        if dx.hypot(dz) <= r + 0.5 && dx * fx + dz * fz > 0.0 {
            return Some((g.marker.cx, g.marker.cz));
        }
    }
    None
}

/// `main.js:shortcutTarget()` — the barred door in reach that the player faces, with the side test
/// (`world::shortcut_status`): from the barred side E only beeps.
fn shortcut_target(
    map: &ParsedMap,
    doors: &world::ZoneDoors,
    p: &Player,
    r: f32,
) -> Option<Interaction> {
    let (fx, fz) = (-p.yaw.sin(), -p.yaw.cos());
    for (i, s) in map.shortcuts.iter().enumerate() {
        if doors.shortcut_open(i) {
            continue;
        }
        let (px, pz) = grid::center(map, s.marker.cx, s.marker.cz);
        let (dx, dz) = (px - p.x, pz - p.z);
        if dx.hypot(dz) > r + 0.5 || dx * fx + dz * fz <= 0.0 {
            continue;
        }
        let st = world::shortcut_status(map, doors, Some(i), p.x, p.z);
        return Some(Interaction::Shortcut {
            cx: s.marker.cx,
            cz: s.marker.cz,
            name: st.name,
            barred: !st.can_open,
        });
    }
    None
}

/// `main.js:interactTarget()` — what E would do right now, in the JS's order: endgame (altar, elevator) →
/// NPC → hub building / stairs → zone items → gates → shortcuts → the zone's extraction marker.
fn resolve_interact(w: &Sim, fade_out: bool) -> Option<Interaction> {
    let mode = w.mode();
    if fade_out || !matches!(mode, GameMode::Zone | GameMode::Hub) {
        return None;
    }
    let data = w.game.data();
    let cfg = &data.config;
    let r = cfg.cfg.interact_r;
    let p = &*w.player;
    let view = w.view();

    // endgame.js:interactTarget — Source only
    if mode == GameMode::Zone {
        if let Some(zone) = w.zone.get() {
            if let Some(def) = data.zone(&zone.id) {
                if economy::in_source(def, &zone.map) {
                    if economy::altar_in_reach(cfg, &zone.map, p.x, p.z) {
                        return Some(Interaction::Altar);
                    }
                    if let Some(st) = zone.map.stairs {
                        if grid::dist2d(st.marker.x, st.marker.z, p.x, p.z) <= r {
                            return Some(Interaction::Elevator);
                        }
                    }
                }
            }
        }
    }

    // npc.js:interactTarget
    if let Some(n) = w.npcs.0.interact_target(
        &data.npcs.cfg,
        mode == GameMode::Zone,
        mode == GameMode::Hub,
        &view,
    ) {
        return Some(Interaction::Npc {
            id: n.id,
            free: n.free,
        });
    }

    // hub.js:interactTarget
    if mode == GameMode::Hub {
        if let Some(hub) = w.hub_map.0.as_ref() {
            match economy::hub_interact_target(data, &w.save.0, &w.hub.0, &hub.map, p.x, p.z) {
                Some(HubInteract::Build { id, .. }) => {
                    return Some(Interaction::Building { id, built: false })
                }
                Some(HubInteract::Building { id, .. }) => {
                    return Some(Interaction::Building { id, built: true })
                }
                Some(HubInteract::Descend { zone_id, lock, .. }) => {
                    return Some(Interaction::Descend { zone_id, lock })
                }
                None => {}
            }
        }
        return None;
    }

    let zone = w.zone.get()?;
    // items in reach and in front (`CFG.interactR`, nearest wins)
    let (fx, fz) = (-p.yaw.sin(), -p.yaw.cos());
    let mut best: Option<usize> = None;
    let mut bd = r;
    for (i, it) in zone.items.iter().enumerate() {
        let (dx, dz) = (it.x - p.x, it.z - p.z);
        let d = dx.hypot(dz);
        if d < bd && dx * fx + dz * fz > 0.0 {
            best = Some(i);
            bd = d;
        }
    }
    if let Some(index) = best {
        return Some(Interaction::Item { index });
    }
    if let Some((cx, cz)) = gate_target(&zone.map, &zone.doors, p, r) {
        let def = data.zone(&zone.id);
        let locked = def
            .map(|d| world::gate_status(d, &w.save.0, &cfg.tools, Some(false)).locked)
            .unwrap_or(false);
        return Some(Interaction::Gate { cx, cz, locked });
    }
    if let Some(sc) = shortcut_target(&zone.map, &zone.doors, p, r) {
        return Some(sc);
    }
    // the extraction marker: bank in a zone
    if let Some(st) = zone.map.stairs {
        if grid::dist2d(st.marker.x, st.marker.z, p.x, p.z) <= r {
            return Some(Interaction::Bank {
                empty: p.carried.is_empty(),
            });
        }
    }
    None
}

/// `main.js:interact()` — resolve the target, run it, then emit `interact {target, handled, x, z}`.
fn interact(
    w: &mut Sim,
    queue: &mut DebugQueue,
    fade_out: bool,
    ev: &mut MessageWriter<SimMessage>,
) -> bool {
    let target = resolve_interact(w, fade_out);
    let handled = match &target {
        Some(t) => do_interact(w, queue, t, ev),
        None => false,
    };
    let (x, z) = (w.player.x, w.player.z);
    emit(
        ev,
        vec![SimEvent::Interact {
            target: target.as_ref().map(Interaction::kind),
            handled,
            x,
            z,
        }],
    );
    handled
}

/// The `run()` of each `main.js:interactTarget` result. Returns the JS's `handled`.
fn do_interact(
    w: &mut Sim,
    queue: &mut DebugQueue,
    t: &Interaction,
    ev: &mut MessageWriter<SimMessage>,
) -> bool {
    match t {
        Interaction::Item { index } => pickup(w, *index, ev),
        // `bank()` / `descend()` live in run.rs; the JS `interact` called them through `ctx.actions` too.
        Interaction::Bank { .. } => {
            queue.push(DebugCommand::Bank);
            true
        }
        Interaction::Descend { zone_id, lock } => {
            if let Some(reason) = lock {
                let name = economy::zone_name(w.game.data(), zone_id).to_string();
                emit(
                    ev,
                    vec![
                        SimEvent::toast(format!("{name}: {reason}")),
                        SimEvent::ui_error(),
                    ],
                );
                false
            } else {
                queue.push(DebugCommand::Descend);
                true
            }
        }
        Interaction::Gate { cx, cz, locked } => open_gate(w, *cx, *cz, *locked, ev),
        Interaction::Shortcut {
            cx,
            cz,
            name,
            barred,
        } => open_shortcut(w, *cx, *cz, name.as_deref(), *barred, ev),
        Interaction::Npc { id, free } => {
            if *free {
                free_npc(w, id, ev)
            } else {
                talk_npc(w, queue, id, ev)
            }
        }
        Interaction::Building { id, built } => {
            // `hub.js:openBuilding` — the board has its own panel, every other built one is a `service`
            // menu, a ghost is the `build` menu. run.rs owns the mode switch.
            let kind = match (built, id.as_str()) {
                (false, _) => "build",
                (true, "board") => "board",
                (true, _) => "service",
            };
            queue.push(DebugCommand::OpenMenu(kind.to_string()));
            true
        }
        Interaction::Altar => {
            queue.push(DebugCommand::OpenChoice);
            true
        }
        Interaction::Elevator => {
            queue.push(DebugCommand::RideUp);
            true
        }
    }
}

/// `main.js:pickup(it)` — add it to the carried loot, take it out of the world, tell the contracts.
fn pickup(w: &mut Sim, index: usize, ev: &mut MessageWriter<SimMessage>) -> bool {
    let Some(zone) = w.zone.get() else {
        return false;
    };
    let Some(item) = zone.items.get(index).cloned() else {
        return false;
    };
    let zone_id = zone.id.clone();
    let evs = economy::pickup(
        &mut w.player.carried,
        item.kind,
        item.contents,
        item.x,
        item.z,
    );
    if let Some(zone) = w.zone.get_mut() {
        zone.items.remove(index);
    }
    emit(ev, evs);
    // `contracts.js:onPickup` — a quest item or a bundle holding one advances its contract
    if let Some(zone) = w.zone.0.as_ref() {
        let env = ContractEnv::zone(w.game.data(), &zone_id, &zone.map);
        let holds_quest = item.contents.map(|c: Carried| c.quest > 0).unwrap_or(false);
        let evs = contracts::on_pickup(
            &env,
            &mut w.save.0,
            item.kind,
            item.quest_id.as_deref(),
            holds_quest,
        );
        emit(ev, evs);
    }
    true
}

/// `main.js:gateTarget().run()` — `world::open_gate` plus the JS's "Locked — needs …" toast.
fn open_gate(
    w: &mut Sim,
    cx: i32,
    cz: i32,
    locked: bool,
    ev: &mut MessageWriter<SimMessage>,
) -> bool {
    let Some(zone_id) = w.zone.id().map(str::to_string) else {
        return false;
    };
    let data = w.game.data();
    let Some(def) = data.zone(&zone_id) else {
        return false;
    };
    // `TOOLS[tool] || tool` — the display name for the JS's "Locked — needs …" toast
    let tool_name = def
        .gate
        .as_ref()
        .map(|g| {
            data.config
                .tools
                .get(&g.tool)
                .cloned()
                .unwrap_or_else(|| g.tool.clone())
        })
        .unwrap_or_default();
    let Some(zone) = w.zone.0.as_mut() else {
        return false;
    };
    let (ok, evs) = world::open_gate(
        &mut zone.map,
        &mut zone.doors,
        &mut w.save.0,
        def,
        &data.config.tools,
        cx,
        cz,
        false,
    );
    emit(ev, evs);
    if !ok && locked {
        emit(
            ev,
            vec![SimEvent::toast(format!("Locked — needs {tool_name}"))],
        );
    }
    ok
}

/// `main.js:shortcutTarget().run()` — lift the bars from the far side, or beep from the barred one.
fn open_shortcut(
    w: &mut Sim,
    cx: i32,
    cz: i32,
    name: Option<&str>,
    barred: bool,
    ev: &mut MessageWriter<SimMessage>,
) -> bool {
    if barred {
        let id = w
            .zone
            .get()
            .and_then(|z| world::shortcut_at(&z.map, cx, cz))
            .and_then(|i| {
                w.zone
                    .get()
                    .and_then(|z| z.map.shortcuts.get(i))
                    .and_then(|s| s.id().map(str::to_string))
            });
        emit(
            ev,
            vec![SimEvent::UiError {
                reason: Some("shortcutBarred".to_string()),
                id,
            }],
        );
        return false;
    }
    let (x, z) = (w.player.x, w.player.z);
    let Some(zone_id) = w.zone.id().map(str::to_string) else {
        return false;
    };
    let Some(zone) = w.zone.get_mut() else {
        return false;
    };
    let (ok, evs) = world::open_shortcut(
        &mut zone.map,
        &mut zone.doors,
        &mut w.save.0,
        &zone_id,
        cx,
        cz,
        (x, z),
        false,
    );
    emit(ev, evs);
    if ok {
        let name = name.unwrap_or_default();
        emit(
            ev,
            vec![SimEvent::toast(format!(
                "The bars fall. {name} is open for good."
            ))],
        );
    }
    ok
}

/// `npc.js:free(id)` — E on a captive.
fn free_npc(w: &mut Sim, id: &str, ev: &mut MessageWriter<SimMessage>) -> bool {
    let in_zone = w.mode() == GameMode::Zone;
    let Some(zone) = w.zone.0.as_ref() else {
        return false;
    };
    let (ok, evs) = w.npcs.0.free(w.game.data(), &zone.map, id, in_zone);
    emit(ev, evs);
    ok
}

/// `npc.js:talk(id)` — E on a hub resident: the dialogue, then `openMenu('dialog')` (run.rs's).
fn talk_npc(
    w: &mut Sim,
    queue: &mut DebugQueue,
    id: &str,
    ev: &mut MessageWriter<SimMessage>,
) -> bool {
    let data = w.game.data();
    let Some(def) = data.npcs.npcs.get(id) else {
        return false;
    };
    let offer = contracts::available(data, &w.save.0, id);
    let titles: Vec<String> = w
        .save
        .0
        .contracts
        .active
        .iter()
        .filter_map(|cid| data.contracts.contracts.get(cid))
        .filter(|c| c.poster == id)
        .map(|c| c.title.clone())
        .collect();
    match w.npcs.0.talk(def, offer.as_ref(), &titles) {
        Some((_dlg, evs)) => {
            emit(ev, evs);
            queue.push(DebugCommand::OpenMenu("dialog".to_string()));
            true
        }
        None => false,
    }
}

/* ============================================================
Keys (main.js:551–640)
============================================================ */

/// Is `code` one of `CFG.KEYS[action]`?
fn is_key(data: &GameData, action: &str, code: &str) -> bool {
    data.config
        .keys
        .get(action)
        .is_some_and(|ks| ks.iter().any(|k| k == code))
}

/// `main.js`'s `keydown` listener. Every branch ends in `key {code, mode}`, exactly as the JS does.
///
/// Title, death-screen and menu *navigation* are the UI lane's; what reaches `run.rs` is forwarded as a
/// [`DebugCommand`] (`ReturnToHub`, `OpenPause`, `ClosePause`, `CloseMenu`, `Accept`). Volume / mute and the
/// minimap toggle belong to the audio and UI lanes and are not handled here.
fn on_key(
    w: &mut Sim,
    queue: &mut DebugQueue,
    menu: &crate::state::MenuKind,
    fade_out: bool,
    code: &str,
    ev: &mut MessageWriter<SimMessage>,
) {
    let mode = w.mode();
    let key_event = |ev: &mut MessageWriter<SimMessage>| {
        emit(
            ev,
            vec![SimEvent::Key {
                code: code.to_string(),
                mode: mode.js_name().to_string(),
            }],
        );
    };
    // resolve the binding first: the action handlers need `&mut Sim`, so no borrow of the data may be alive
    let (confirm, menu_close, action) = {
        let data = w.game.data();
        let action = ["lamp", "flash", "lantern", "interact", "top_up"]
            .into_iter()
            .find(|a| is_key(data, a, code));
        (
            is_key(data, "confirm", code),
            is_key(data, "menu_close", code),
            action,
        )
    };

    match mode {
        // TITLE: the main-menu widget (ui lane) and the hidden Backspace wipe; nothing to forward yet.
        GameMode::Title | GameMode::Loading => {
            key_event(ev);
        }
        GameMode::Dead => {
            if confirm {
                queue.push(DebugCommand::ReturnToHub);
            }
            key_event(ev);
        }
        GameMode::Menu => {
            let is_pause = menu.0.as_deref() == Some("pause");
            if menu_close {
                queue.push(if is_pause {
                    DebugCommand::ClosePause
                } else {
                    DebugCommand::CloseMenu
                });
            } else if menu.0.as_deref() == Some("dialog") {
                // npc.js:onKey — 1 / Enter / Space accepts the offer, then the shell closes the menu
                if let Some(id) = w.npcs.0.on_key(code, true) {
                    queue.push(DebugCommand::Accept(id));
                    queue.push(DebugCommand::CloseMenu);
                }
            }
            key_event(ev);
        }
        GameMode::Hub | GameMode::Zone | GameMode::Dying | GameMode::Ending => {
            if menu_close && matches!(mode, GameMode::Hub | GameMode::Zone) {
                queue.push(DebugCommand::OpenPause);
                key_event(ev);
                return;
            }
            if fade_out || matches!(mode, GameMode::Ending | GameMode::Dying) {
                key_event(ev);
                return;
            }
            match action {
                Some("lamp") => {
                    toggle_lamp(w, ev);
                }
                Some("flash") => {
                    flash(w, ev);
                }
                Some("lantern") => {
                    plant_lantern(w, ev);
                }
                Some("interact") => {
                    interact(w, queue, fade_out, ev);
                }
                Some("top_up") => {
                    top_up(w, ev);
                }
                // `KEYS.minimap` (Tab), `mute`, `volUp` / `volDown`: the ui and audio lanes
                _ => {}
            }
            key_event(ev);
        }
    }
}

/* ============================================================
Movement (main.js:updatePlayer + world.js water boundaries)
============================================================ */

/// `main.js:updatePlayer(dt)` — mouse look, WASD against the collision grid, then the cell flags and the
/// water-boundary events `world.js:update` emits. Movement only runs where the JS ran it: `HUB` and `ZONE`.
fn player_movement(
    mut w: Sim,
    time: Res<Time>,
    mut intent: ResMut<MoveIntent>,
    mut ev: MessageWriter<SimMessage>,
) {
    if !w.ready() {
        return;
    }
    let dt = time.delta_secs();
    let it = *intent;
    *intent = MoveIntent::default();
    let mode = w.mode();
    // `Cfg` is scalars only, so this copy is free; cloning the whole `Config` every tick would not be
    let cfg = w.game.config().cfg.clone();
    let bands = w
        .zone
        .id()
        .and_then(|id| w.game.data().zone(id))
        .is_some_and(|z| z.deep_style == undercroft_data::zone::DeepStyle::Bands);

    // Mouse look (`main.js` `mousemove`: HUB / ZONE / DYING while pointer-locked). `look_dx` / `look_dy` are
    // *raw* pointer deltas — `CFG.mouseSens` is applied here, exactly where the JS applies it — and the
    // keyboard look of `updatePlayer` (`CFG.lookKeys × dt` on the arrow keys) folds into the same two fields.
    if matches!(mode, GameMode::Hub | GameMode::Zone | GameMode::Dying) {
        w.player.yaw -= it.look_dx * cfg.mouse_sens;
        w.player.pitch = (w.player.pitch - it.look_dy * cfg.mouse_sens).clamp(-1.5, 1.5);
    }
    if !matches!(mode, GameMode::Hub | GameMode::Zone) {
        return;
    }

    let mut mx = it.strafe;
    let mut mz = -it.forward;
    let moving = mx != 0.0 || mz != 0.0;
    w.player.sprinting = moving && it.sprint;

    // borrow the map (and, in the hub, its block mask) in place; they live in fields `w.player` does not
    let (map, mask): (Option<&ParsedMap>, Option<&collision::BlockMask>) = match mode {
        GameMode::Hub => match w.hub_map.0.as_ref() {
            Some(h) => (Some(&h.map), Some(&h.mask)),
            None => (None, None),
        },
        _ => (w.zone.0.as_ref().map(|z| &z.map), None),
    };
    let Some(map) = map else {
        w.player.moving = moving;
        return; // run.rs has not filled the map yet: no collision, no cell flags
    };

    if moving {
        let len = mx.hypot(mz);
        mx /= len;
        mz /= len;
        let (s, c) = (w.player.yaw.sin(), w.player.yaw.cos());
        // forward = (-sin yaw, -cos yaw)
        let vx = mx * c + mz * s;
        let vz = -mx * s + mz * c;
        let sprinting = w.player.sprinting;
        let (px, pz) = (w.player.x, w.player.z);
        let speed = collision::player_speed_at(map, &cfg, px, pz, sprinting);
        let (nx, nz) =
            collision::move_player(map, mask, &cfg, px, pz, vx * speed * dt, vz * speed * dt);
        w.player.x = nx;
        w.player.z = nz;
    }
    w.player.moving = moving;
    if !w.player.x.is_finite() || !w.player.z.is_finite() {
        // `main.js`: a non-finite position drops the player back at the entry
        if let Some(st) = map.stairs {
            w.player.x = st.marker.x;
            w.player.z = st.marker.z;
        } else {
            w.player.x = 0.0;
            w.player.z = 0.0;
        }
    }

    // cell flags
    let (cx, cz) = grid::to_cell(map, w.player.x, w.player.z);
    let t = grid::cell_type(map, cx, cz);
    let was_water = w.player.in_water;
    w.player.on_deep = t == CellKind::Deep;
    w.player.in_water = t == CellKind::Water;
    w.player.lap = if bands && mode == GameMode::Zone {
        grid::lap_of_map(map, cx, cz)
    } else {
        0
    };
    let (px, pz) = (w.player.x, w.player.z);
    w.player.in_pool = mode == GameMode::Zone
        && w.zone
            .0
            .as_ref()
            .is_some_and(|z| pool::in_pool(&z.lanterns, px, pz, cfg.pool_r));

    // `world.js:update` — the W boundary, ZONE only
    if mode == GameMode::Zone && w.player.in_water != was_water {
        let e = if w.player.in_water {
            SimEvent::WaterEnter { x: px, z: pz }
        } else {
            SimEvent::WaterExit { x: px, z: pz }
        };
        emit(&mut ev, vec![e]);
    }
}

/* ============================================================
Lamp, exploration and the tick's PlayerView
============================================================ */

/// `main.js:updateLamp(dt, time)` plus `hub.js:exploreTick` and the [`PlayerViewRes`] every other lane reads.
/// Runs in every mode: the JS calls `updateLamp(0, time)` outside HUB / ZONE / DYING so the forced-off rule
/// and the flicker still apply.
fn player_lamp(
    mut w: Sim,
    time: Res<Time>,
    mut explore: ResMut<ExploreTimer>,
    mut view: ResMut<PlayerViewRes>,
) {
    if !w.ready() {
        return;
    }
    let mode = w.mode();
    let dt = if matches!(mode, GameMode::Hub | GameMode::Zone | GameMode::Dying) {
        time.delta_secs()
    } else {
        0.0
    };
    let mul = w.zone_mul();
    let reach = economy::update_lamp(
        &mut w.lamp.0,
        w.game.config(),
        dt,
        mul,
        state::lamp_allowed(mode),
    );

    // hub.js: exploration ticks in ZONE only; the timer is reset everywhere else (`onDescend`).
    if mode == GameMode::Zone {
        explore.0 -= dt;
        if explore.0 <= 0.0 {
            explore.0 = w.game.config().hub_cfg.explore_tick;
            let (x, z) = (w.player.x, w.player.z);
            let lamp_on = w.lamp.0.lamp_on;
            if let Some(zone) = w.zone.0.as_ref() {
                explore_tick(w.game.config(), zone, &mut w.save.0, x, z, lamp_on, reach);
            }
        }
    } else {
        explore.0 = 0.0;
    }

    view.0 = w.player.view(&w.lamp.0, reach, w.npcs.0.stimulus());
}

/// `hub.js:exploreTick()` — mark the lit, visible cells in the zone's explored bitset (`hub.js:flushExplored`
/// mirrors it into the save; here it is written straight back, and `run.rs` persists the save as the JS does).
fn explore_tick(
    cfg: &undercroft_data::Config,
    zone: &Zone,
    save: &mut undercroft_sim::save::SaveData,
    x: f32,
    z: f32,
    lamp_on: bool,
    reach: f32,
) {
    let mut bits = save.explored_bits(&zone.id, zone.map.len());
    if economy::explore_tick(cfg, &zone.map, &mut bits, x, z, lamp_on, reach) {
        save.set_explored_bits(&zone.id, &bits);
    }
}

/* ============================================================
Plugin
============================================================ */

/// The player systems: the debug handler in [`DebugSet::Handle`], the rest chained in [`SimSet::Player`].
pub fn plugin(app: &mut App) {
    app.init_resource::<PlayerActions>()
        .init_resource::<ExploreTimer>()
        .add_systems(FixedUpdate, handle_debug.in_set(DebugSet::Handle))
        .add_systems(
            FixedUpdate,
            (player_actions, player_movement, player_lamp)
                .chain()
                .in_set(SimSet::Player),
        );
}

/* ============================================================
Tests
============================================================ */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::*;
    use crate::messages::EventLog;
    use crate::resources::MemoryStore;
    use undercroft_sim::economy::LampState;

    /// Start a real run through the same path `actions.gotoZone(id)` takes
    /// (`run.rs::goto_zone` — begin from `Title` if needed, `load_zone_inactive`, `start_run`),
    /// rather than poking `ZoneRes` by hand: `run.rs`'s `boot` system runs in the same
    /// `DebugSet::Handle` set and would otherwise clobber a hand-built zone with the inactive one
    /// it loads for the save's selected zone. Each test gets its own in-memory save store so
    /// nothing leaks between them.
    fn app_in_zone(id: &str) -> App {
        let mut app = headless_app_with_store(MemoryStore::new());
        send(&mut app, DebugCommand::GotoZone(id.into()));
        step(&mut app, 0.5);
        // `NextState` is applied by the `StateTransition` schedule of the *next* frame, so `mode`
        // can still lag the tick that queued the command by one; the extra half-second above is
        // normally enough, but one more tick makes the wait explicit rather than incidental.
        step(&mut app, 1.0 / 60.0);
        assert_eq!(
            mode(&app),
            GameMode::Zone,
            "gotoZone starts the run immediately"
        );
        app
    }

    fn teleport(app: &mut App, x: f32, z: f32, yaw: f32) {
        send(
            app,
            DebugCommand::Teleport {
                x,
                z,
                yaw: Some(yaw),
            },
        );
        step(app, 1.0 / 60.0);
    }

    fn player_of(app: &App) -> Player {
        app.world().resource::<Player>().clone()
    }

    fn lamp_of(app: &App) -> LampState {
        app.world().resource::<LampRes>().0
    }

    fn log_names(app: &App) -> Vec<&'static str> {
        app.world().resource::<EventLog>().names()
    }

    /// `main.js:startRun()` lights the handlamp (`economy::start_run` sets `lampOn = true`) and it
    /// stays lit: `player_lamp` runs later in the *same* fixed tick that `run.rs::start_run` queued
    /// `NextState(Zone)` in, and both lanes read the mode through [`state::Mode`] /
    /// `run.rs::ModeParam`, i.e. [`state::effective`] — the pending mode wins, so
    /// `state::lamp_allowed` is already true and `economy::update_lamp` does not force the lamp off.
    ///
    /// This was a real cross-lane defect: `Sim::mode()` used to read `Res<State<GameMode>>`, which
    /// Bevy only updates in the `StateTransition` schedule *between frames*, so every run started
    /// through the action queue — the only way a real build starts one — began with the handlamp
    /// permanently snuffed. `main.js` has no equivalent bug: `state.mode` is a plain assignment, so
    /// `updateLamp`, called later in the same synchronous `startRun()`, already sees `'ZONE'`.
    #[test]
    fn starting_a_run_leaves_the_handlamp_lit() {
        let app = app_in_zone("undercroft");
        assert!(
            lamp_of(&app).lamp_on,
            "economy::start_run turned the lamp on and update_lamp kept it on"
        );
    }

    /// The same, several fixed ticks deep: `update_lamp` runs every tick and must never revisit the
    /// decision, and a real frame can contain more than one fixed tick, in which case even the
    /// *next* tick still only sees the pending `NextState`.
    #[test]
    fn the_handlamp_stays_lit_for_the_whole_run() {
        let mut app = app_in_zone("undercroft");
        step(&mut app, 2.0);
        assert!(lamp_of(&app).lamp_on, "still lit two seconds in");
        assert!(
            lamp_of(&app).oil < 100.0,
            "and burning oil, so update_lamp really ran"
        );
    }

    #[test]
    fn toggle_lamp_twice_logs_off_then_on() {
        let mut app = app_in_zone("undercroft");
        assert!(lamp_of(&app).lamp_on);
        send(&mut app, DebugCommand::ToggleLamp);
        step(&mut app, 1.0 / 60.0);
        assert!(!lamp_of(&app).lamp_on);
        send(&mut app, DebugCommand::ToggleLamp);
        step(&mut app, 1.0 / 60.0);
        assert!(lamp_of(&app).lamp_on);
        let toggles: Vec<bool> = app
            .world()
            .resource::<EventLog>()
            .all("lampToggle")
            .iter()
            .filter_map(|e| match e {
                SimEvent::LampToggle { on } => Some(*on),
                _ => None,
            })
            .collect();
        assert_eq!(toggles, vec![false, true]);
    }

    #[test]
    fn flash_costs_the_light_tech_oil_and_logs_flash() {
        let mut app = app_in_zone("undercroft");
        let before = lamp_of(&app).oil;
        send(&mut app, DebugCommand::Flash);
        step(&mut app, 1.0 / 60.0);
        assert_eq!(app.world().resource::<EventLog>().count("flash"), 1);
        let after = lamp_of(&app);
        // CFG.flashCost 15 at light-tech 0, minus one tick of CFG.burn (0.5/s) while lit
        assert!(
            (before - after.oil - 15.0).abs() < 0.05,
            "oil {before} -> {}",
            after.oil
        );
        assert!(after.flash_t > 0.0 && after.flash_cd > 0.0);
    }

    #[test]
    fn planting_lanterns_fills_the_pool_and_recycles_the_oldest() {
        let mut app = app_in_zone("undercroft");
        // plenty of oil for five lanterns at 20 each
        app.world_mut().resource_mut::<LampRes>().0.oil = 100.0;
        app.world_mut().resource_mut::<LampRes>().0.lamp_on = false;
        let max = app
            .world()
            .resource::<Assets<crate::GameDataAsset>>()
            .iter()
            .next()
            .expect("asset")
            .1
            .data
            .config
            .cfg
            .lantern_max;
        assert_eq!(max, 4);
        for i in 0..max {
            let p = player_of(&app);
            teleport(&mut app, p.x + i as f32 * 0.0, p.z, 0.0);
            send(&mut app, DebugCommand::PlantLantern);
            step(&mut app, 1.05); // CFG.lanternCd
        }
        assert_eq!(app.world().resource::<EventLog>().count("lantern"), 4);
        assert_eq!(
            app.world().resource::<EventLog>().count("lanternRemoved"),
            0
        );
        {
            let z = app.world().resource::<ZoneRes>();
            let z = z.get().expect("zone");
            assert_eq!(z.lanterns.len(), 4);
            assert!(z.pool.count() > 0, "pool bitmap recomputed");
        }
        // the fifth recycles the oldest
        send(&mut app, DebugCommand::PlantLantern);
        step(&mut app, 1.0 / 60.0);
        assert_eq!(app.world().resource::<EventLog>().count("lantern"), 5);
        assert_eq!(
            app.world().resource::<EventLog>().count("lanternRemoved"),
            1
        );
        let z = app.world().resource::<ZoneRes>();
        assert_eq!(z.get().expect("zone").lanterns.len(), 4);
    }

    #[test]
    fn interact_picks_up_an_item_in_front_of_the_player() {
        let mut app = app_in_zone("undercroft");
        let (ix, iz, kind) = {
            let z = app.world().resource::<ZoneRes>();
            let it = z.get().expect("zone").items.first().expect("an item");
            (it.x, it.z, it.kind)
        };
        // stand half a unit south of it looking north: forward = (0, -1) at yaw 0
        teleport(&mut app, ix, iz + 0.5, 0.0);
        send(&mut app, DebugCommand::Interact);
        step(&mut app, 1.0 / 60.0);
        assert_eq!(app.world().resource::<EventLog>().count("pickup"), 1);
        let carried = player_of(&app).carried;
        assert_eq!(carried.get(kind), 1);
        let z = app.world().resource::<ZoneRes>();
        assert!(
            !z.get()
                .expect("zone")
                .items
                .iter()
                .any(|i| i.x == ix && i.z == iz),
            "the item left the world"
        );
        assert!(log_names(&app).contains(&"interact"));
    }

    #[test]
    fn interact_at_the_stairs_asks_run_rs_to_bank() {
        let mut app = app_in_zone("undercroft");
        let (sx, sz) = {
            let z = app.world().resource::<ZoneRes>();
            let m = &z.get().expect("zone").map;
            let s = m.stairs.expect("stairs");
            (s.marker.x, s.marker.z)
        };
        teleport(&mut app, sx, sz, 0.0);
        send(&mut app, DebugCommand::Interact);
        step(&mut app, 1.0 / 60.0);
        // pushed in SimSet::Player, i.e. after DebugSet::Drain — it waits for run.rs on the next tick
        assert!(
            app.world()
                .resource::<DebugQueue>()
                .0
                .contains(&DebugCommand::Bank),
            "queue: {:?}",
            app.world().resource::<DebugQueue>().0
        );
        let last = app
            .world()
            .resource::<EventLog>()
            .last("interact")
            .cloned()
            .expect("interact logged");
        match last {
            SimEvent::Interact {
                target, handled, ..
            } => {
                assert_eq!(target, Some(InteractTarget::Bank));
                assert!(handled);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// A test stand-in for the world lane: hold one [`MoveIntent`] for every fixed tick.
    fn hold_intent(app: &mut App, it: MoveIntent) {
        app.insert_resource(HeldIntent(it));
        app.add_systems(FixedUpdate, apply_held.in_set(SimSet::Input));
    }

    #[derive(Resource, Clone, Copy)]
    struct HeldIntent(MoveIntent);

    fn apply_held(held: Res<HeldIntent>, mut intent: ResMut<MoveIntent>) {
        *intent = held.0;
    }

    #[test]
    fn forward_moves_at_the_walk_speed_along_minus_sin_minus_cos() {
        let mut app = app_in_zone("undercroft");
        // an open cell with room to the north: start at the entry and walk one second
        let start = player_of(&app);
        hold_intent(
            &mut app,
            MoveIntent {
                forward: 1.0,
                ..Default::default()
            },
        );
        step(&mut app, 1.0);
        let end = player_of(&app);
        let walk = 2.6_f32; // CFG.walk
        let (dx, dz) = (end.x - start.x, end.z - start.z);
        let moved = dx.hypot(dz);
        assert!(
            moved > 0.5 * walk,
            "moved {moved} from ({}, {}) to ({}, {})",
            start.x,
            start.z,
            end.x,
            end.z
        );
        assert!(moved <= walk + 0.01, "never faster than CFG.walk: {moved}");
        // forward at yaw 0 is −z
        assert!(dz < 0.0 && dx.abs() < 1e-3, "d = ({dx}, {dz})");
        // and never inside a wall
        let z = app.world().resource::<ZoneRes>();
        let m = &z.get().expect("zone").map;
        assert!(!collision::box_blocked(m, None, end.x, end.z, 0.3));
        assert!(end.moving);
    }

    #[test]
    fn walking_into_water_logs_water_enter() {
        // the Cistern is the only zone with `W` cells
        let mut app = app_in_zone("cistern");
        let (nx, nz, yaw) = {
            let z = app.world().resource::<ZoneRes>();
            let m = &z.get().expect("zone").map;
            water_approach(m).expect("the cistern has a floor cell beside water")
        };
        teleport(&mut app, nx, nz, yaw);
        hold_intent(
            &mut app,
            MoveIntent {
                forward: 1.0,
                ..Default::default()
            },
        );
        step(&mut app, 1.0);
        assert!(player_of(&app).in_water, "standing in water");
        assert_eq!(app.world().resource::<EventLog>().count("waterEnter"), 1);
    }

    /// A floor cell next to a water cell, with the yaw that walks from one into the other.
    fn water_approach(m: &ParsedMap) -> Option<(f32, f32, f32)> {
        for cz in 0..m.h {
            for cx in 0..m.w {
                if m.cell_type(cx, cz) != CellKind::Floor {
                    continue;
                }
                for (dx, dz) in grid::DIRS4 {
                    if m.cell_type(cx + dx, cz + dz) != CellKind::Water {
                        continue;
                    }
                    let (x, z) = grid::center(m, cx, cz);
                    // forward = (-sin yaw, -cos yaw) must point at (dx, dz)
                    let yaw = (-(dx as f32)).atan2(-(dz as f32));
                    return Some((x, z, yaw));
                }
            }
        }
        None
    }

    #[test]
    fn a_lit_second_in_a_corridor_marks_explored_cells() {
        let mut app = app_in_zone("undercroft");
        assert!(lamp_of(&app).lamp_on);
        step(&mut app, 1.0);
        let z = app.world().resource::<ZoneRes>();
        let m = &z.get().expect("zone").map;
        let bits = app
            .world()
            .resource::<SaveRes>()
            .0
            .explored_bits("undercroft", m.len());
        let set = bits.iter().map(|b| b.count_ones()).sum::<u32>();
        assert!(set > 0, "explore_tick wrote the bitset");
        assert!(economy::explored_pct(m, &bits) > 0);
    }

    #[test]
    fn keys_are_logged_with_the_mode_and_drive_the_lamp() {
        let mut app = app_in_zone("undercroft");
        send(&mut app, DebugCommand::Key("KeyF".into()));
        step(&mut app, 1.0 / 60.0);
        assert!(!lamp_of(&app).lamp_on, "KEYS.lamp is F");
        let last = app
            .world()
            .resource::<EventLog>()
            .last("key")
            .cloned()
            .expect("key logged");
        assert_eq!(
            last,
            SimEvent::Key {
                code: "KeyF".into(),
                mode: "ZONE".into()
            }
        );
        // Escape asks run.rs for the pause menu
        send(&mut app, DebugCommand::Key("Escape".into()));
        step(&mut app, 1.0 / 60.0);
        assert!(app
            .world()
            .resource::<DebugQueue>()
            .0
            .contains(&DebugCommand::OpenPause));
    }

    #[test]
    fn the_player_view_is_written_every_tick_without_a_zone() {
        let mut app = headless_app();
        step(&mut app, 0.5);
        let v = app.world().resource::<PlayerViewRes>().0.clone();
        assert!(!v.lamp_on, "the lamp is forced off outside ZONE/DYING/MENU");
        assert_eq!(v.lamp_reach, 11.0, "CFG.lampDist with no zone loaded");
        assert_eq!(mode(&app), GameMode::Title);
    }
}
