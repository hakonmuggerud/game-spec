//! The `main.js` lifecycle: every mode change, the fade, the hub/zone/bank/death loop, the menus,
//! the Source run and the endings — plus the per-tick sim calls whose results are *not* the player's
//! (`hunter.js`, `npc.js`, `contracts.js`, `endgame.js`).
//!
//! Shape of the port. `main.js` ran its state changes through a synchronous event bus: `emit('x')`
//! called the listeners `world.js` / `hunter.js` / `contracts.js` / `npc.js` / `hub.js` /
//! `endgame.js` had registered, in registration order, and those listeners emitted more events.
//! Here every lifecycle function builds one `Vec<SimEvent>` through [`push_events`], which calls
//! [`react`] — the port of that listener table — for each event and splices the follow-up events in
//! right after it. The batch is written to the bus once, so [`crate::messages::EventLog`] sees the
//! prototype's order (`begin` before `hubEnter`, `zoneExit` before `zoneEnter`, `npcCaught` before
//! `npcLost`).
//!
//! Nothing here re-implements game logic: every number comes from `undercroft_sim`. What this module
//! owns is ordering, the Bevy state machine and the side effects the sim deliberately returns as
//! data (HANDOFF §6): lantern smashes, death bundles, quest items, extra Source hunters, Source
//! dormancy, and the `hunterCatch → die` / `lampSnuffed` / `npcCaught` listeners.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::collections::BTreeMap;

use undercroft_data::tables::ItemKind;
use undercroft_data::ParsedMap;
use undercroft_sim::collision::BlockMask;
use undercroft_sim::contracts::{self, ContractEnv, QuestWorld};
use undercroft_sim::creature::{self, Ctx as CreatureCtx, Env as CreatureEnv, HState, ProfileKind};
use undercroft_sim::economy::{self, EndingScreen, Screen};
use undercroft_sim::events::CatchTarget;
use undercroft_sim::follower::HunterTouch;
use undercroft_sim::player::Carried;
use undercroft_sim::save::{Rescued, SaveData};
use undercroft_sim::world as sim_world;
use undercroft_sim::{grid, Pool, SimEvent};

use crate::debug::{DebugCommand, DebugQueue, DebugSet};
use crate::messages::{emit, SimMessage};
use crate::resources::{
    Fade, FadeMode, Game, HubMap, HubMapRes, HubRes, LampRes, Npcs, PendingTransition, Player,
    PlayerViewRes, RngRes, SaveRes, SaveStoreRes, Spawns, WorldItem, Zone, ZoneRes,
};
use crate::state::{self, GameMode, MenuKind, PrevMode};
use crate::tick::{Clock, SimSet};

/* ============================================================
Run-lane resources
============================================================ */

/// `endgame.js` `S.screen` / `S.current` — which altar overlay is up. The UI lane draws it; only this
/// module writes it.
#[derive(Resource, Debug, Clone, Default)]
pub struct EndingScreenRes(pub EndingScreen);

/// `world.js`'s module-level `bundles` — the one death bundle remembered per zone and respawned
/// whenever that zone is loaded (`world.js:loadZone` / `spawnItem` / `removeItem`).
#[derive(Resource, Debug, Clone, Default)]
pub struct BundleStore(pub BTreeMap<String, (Carried, f32, f32)>);

/// `save.js:save()` — the debounced writer. The prototype saved on *every* emitted event, at most
/// once per [`SAVE_DEBOUNCE`] seconds; so does this.
#[derive(Resource, Debug, Clone, Default)]
pub struct SaveWriter {
    dirty: bool,
    timer: f32,
}

/// `save.js:DEBOUNCE_MS` (250 ms).
pub const SAVE_DEBOUNCE: f32 = 0.25;

/// Has `main.js`'s init block run? (Module scope in the JS, which has no asynchronous data.)
#[derive(Resource, Debug, Default)]
pub struct Booted(pub bool);

/// `endgame.js:rideUp` armed a fade whose callback still has to run [`economy::ride_up`].
#[derive(Resource, Debug, Default)]
pub struct RideUpPending(pub bool);

/* ============================================================
System params
============================================================ */

/// `ctx.state.mode` / `prevMode` / `menuKind`. [`ModeParam::get`] is [`state::effective`] — the
/// pending [`NextState`] wins over the applied [`State`], so a mode set earlier in the same tick is
/// visible to everything that runs after it, as `main.js`'s synchronous `state.mode = m` was. The
/// read-only half of this is [`state::Mode`], which `player.rs` uses.
#[derive(SystemParam)]
pub struct ModeParam<'w> {
    state: Res<'w, State<GameMode>>,
    next: ResMut<'w, NextState<GameMode>>,
    /// `main.js:53 state.prevMode`.
    pub prev: ResMut<'w, PrevMode>,
    /// `main.js:53 state.menuKind`.
    pub kind: ResMut<'w, MenuKind>,
}

impl ModeParam<'_> {
    /// `ctx.state.mode`, as `main.js` would read it within the same frame.
    pub fn get(&self) -> GameMode {
        state::effective(&self.state, &self.next)
    }

    /// `state.mode = m`.
    pub fn set(&mut self, m: GameMode) {
        self.next.set(m);
    }
}

/// The read-only per-tick snapshot the sim calls take.
#[derive(SystemParam)]
pub struct Snapshot<'w> {
    /// Written by `player.rs`; stale (or default) until that lane lands, which nothing here trips on.
    pub view: Res<'w, PlayerViewRes>,
    /// `ctx.state.time`.
    pub clock: Res<'w, Clock>,
}

/// The resources only this module owns.
#[derive(SystemParam)]
pub struct RunLocal<'w> {
    pub ending: ResMut<'w, EndingScreenRes>,
    pub bundles: ResMut<'w, BundleStore>,
    pub ride_up: ResMut<'w, RideUpPending>,
}

/// Everything a lifecycle function touches (`ctx` in `main.js`), grouped into nested
/// [`SystemParam`]s to stay inside Bevy's 16-element tuple limit.
#[derive(SystemParam)]
pub struct RunCtx<'w> {
    pub game: Game<'w>,
    pub save: ResMut<'w, SaveRes>,
    pub hub: ResMut<'w, HubRes>,
    pub lamp: ResMut<'w, LampRes>,
    pub zone: ResMut<'w, ZoneRes>,
    pub hub_map: ResMut<'w, HubMapRes>,
    pub npcs: ResMut<'w, Npcs>,
    pub player: ResMut<'w, Player>,
    pub fade: ResMut<'w, Fade>,
    pub spawns: ResMut<'w, Spawns>,
    pub rng: ResMut<'w, RngRes>,
    pub store: Res<'w, SaveStoreRes>,
    pub mode: ModeParam<'w>,
    pub snap: Snapshot<'w>,
    pub local: RunLocal<'w>,
}

/* ============================================================
Borrow-splitting macros
============================================================ */

/// `contracts.js`'s view of the world, built from disjoint fields of [`RunCtx`] so the caller can
/// still hand the sim `&mut save` / `&mut hub` in the same expression.
macro_rules! contract_env {
    ($ctx:expr, $in_zone:expr) => {
        ContractEnv {
            data: $ctx.game.data(),
            zone: $ctx.zone.id(),
            map: $ctx.zone.get().map(|z| &z.map),
            in_zone_mode: $in_zone,
        }
    };
}

/// Run a creature-lane call over the loaded zone: builds the frame's [`CreatureEnv`], binds the
/// [`CreatureCtx`] and the record list, and evaluates to `Option<(result, events)>` — `None` when no
/// zone is loaded. A macro rather than a closure because `CreatureCtx<'a>` borrows the `Env`, the
/// tuning, the RNG and the event buffer at once, which no higher-ranked closure signature expresses.
macro_rules! creature_scope {
    ($ctx:expr, $in_zone:expr, |$c:ident, $hunters:ident| $body:expr) => {{
        let ctx_ref: &mut RunCtx = $ctx;
        let tuning_ref = ctx_ref.game.tuning();
        let player_ref = &ctx_ref.snap.view.0;
        let time_now = ctx_ref.snap.clock.time;
        let in_zone_now = $in_zone;
        match ctx_ref.zone.0.as_mut() {
            None => None,
            Some(zone_ref) => {
                let lantern_pos: Vec<(f32, f32)> =
                    zone_ref.lanterns.iter().map(|l| (l.x, l.z)).collect();
                let item_pos: Vec<(f32, f32)> = zone_ref.items.iter().map(|i| (i.x, i.z)).collect();
                let Zone {
                    map: map_ref,
                    pool: pool_ref,
                    hunters: $hunters,
                    ..
                } = zone_ref;
                let env_now = CreatureEnv {
                    map: &*map_ref,
                    pool: &*pool_ref,
                    player: player_ref,
                    lanterns: &lantern_pos,
                    items: &item_pos,
                    in_zone: in_zone_now,
                    time: time_now,
                };
                let mut ev_buf: Vec<SimEvent> = Vec::new();
                let mut cctx =
                    CreatureCtx::new(&env_now, tuning_ref, &mut ctx_ref.rng.0, &mut ev_buf);
                let $c = &mut cctx;
                let result = $body;
                Some((result, ev_buf))
            }
        }
    }};
}

/* ============================================================
Small helpers
============================================================ */

/// `ctx.zone.id`, or `""` before the first `loadZoneInactive`.
fn zone_id(ctx: &RunCtx) -> String {
    ctx.zone.id().unwrap_or_default().to_string()
}

/// `endgame.js:109 inSource(c)` — [`economy::in_source`] for the loaded zone. An id the data does not
/// know (only reachable through a hand-built [`ZoneRes`]) still falls back to the two map-side tests.
fn in_source(ctx: &RunCtx) -> bool {
    let Some(z) = ctx.zone.get() else {
        return false;
    };
    match ctx.game.data().zone(&z.id) {
        Some(def) => economy::in_source(def, &z.map),
        None => z.id == "source" || z.map.altar.is_some(),
    }
}

/// `main.js:spawnAt(m, yaw)` — the player stands on the map's stairs / elevator marker.
fn spawn_at(player: &mut Player, m: &ParsedMap, yaw: f32) {
    match &m.stairs {
        Some(s) => player.reset_at(s.marker.x, s.marker.z, yaw),
        None => player.reset_at(m.ox as f32 + m.w as f32 * 0.5, m.h as f32 * 0.5, yaw),
    }
}

/// What `contracts.js:spawnQuestsFor` has to know about the zone's items.
fn quest_world(ctx: &RunCtx) -> QuestWorld {
    let (mut quest_items, mut bundle_holds_quest) = (Vec::new(), false);
    if let Some(z) = ctx.zone.get() {
        for it in &z.items {
            match it.kind {
                ItemKind::Quest => {
                    if let Some(id) = &it.quest_id {
                        quest_items.push(id.clone());
                    }
                }
                ItemKind::Bundle if it.contents.map(|c| c.quest > 0) == Some(true) => {
                    bundle_holds_quest = true;
                }
                _ => {}
            }
        }
    }
    QuestWorld {
        quest_items,
        bundle_holds_quest,
        carried_quest: ctx.player.carried.quest,
    }
}

/// `npc.js:placeHub`.
fn place_hub(ctx: &mut RunCtx) {
    let Some(hm) = ctx.hub_map.0.as_ref() else {
        return;
    };
    ctx.npcs.0.place_hub(ctx.game.data(), &hm.map, &ctx.save.0);
}

/// True where `npc.js` re-places the hub residents: the hub is showing (or the title looks over it).
fn hub_showing(ctx: &RunCtx) -> bool {
    match ctx.mode.get() {
        GameMode::Hub | GameMode::Title => true,
        GameMode::Menu => ctx.mode.prev.0 == Some(GameMode::Hub),
        _ => false,
    }
}

/// `save.js:flush()` — write the save through the platform store now.
fn store_now(ctx: &RunCtx, writer: &mut SaveWriter) {
    writer.dirty = false;
    writer.timer = 0.0;
    ctx.store.0.store(&ctx.save.0.to_json());
}

/* ============================================================
The event bus: emit + the listener table
============================================================ */

/// `events.emit(name, payload)` for a whole batch: append each event to `out`, then splice in
/// whatever [`react`] (the listeners) produced, exactly as the synchronous JS bus did.
fn push_events(ctx: &mut RunCtx, out: &mut Vec<SimEvent>, evs: Vec<SimEvent>) {
    for e in evs {
        let mut follow = Vec::new();
        react(ctx, &e, &mut follow);
        out.push(e);
        if !follow.is_empty() {
            push_events(ctx, out, follow);
        }
    }
}

/// The listener table of `main.js`'s init block, in registration order — `world.js`, `hunter.js`,
/// `contracts.js`, `npc.js`, `hub.js`, `endgame.js`, plus `main.js`'s own `hunterCatch` and
/// `lampSnuffed` listeners. Purely presentational listeners (meshes, fog, HUD, audio, camera shake)
/// belong to the rendering lanes.
fn react(ctx: &mut RunCtx, e: &SimEvent, out: &mut Vec<SimEvent>) {
    match e {
        // main.js:747 — `hunterCatch {target:'player'}` ends the run.
        SimEvent::HunterCatch {
            hunter_id, target, ..
        } => match target {
            CatchTarget::Player => {
                die(ctx, Some(*hunter_id), out);
            }
            CatchTarget::Npc(id) => {
                let time = ctx.snap.clock.time;
                let evs = ctx.npcs.0.on_hunter_catch(
                    ctx.game.data(),
                    &ctx.game.config().npc_cfg,
                    Some(id),
                    Some(*hunter_id),
                    time,
                );
                out.extend(evs);
            }
        },
        // main.js:749 — the Lampwight's touch; `economy` owns the numbers, the shell applies them.
        SimEvent::LampSnuffed { oil, lockout, .. } => {
            let in_zone = ctx.mode.get() == GameMode::Zone;
            economy::on_lamp_snuffed(
                &mut ctx.lamp.0,
                ctx.game.config(),
                Some(*oil),
                Some(*lockout),
                in_zone,
            );
        }
        // hunter.js:38 — the catcher stands over the spot for `catchBusyT`.
        SimEvent::NpcCaught {
            hunter_id: Some(id),
            ..
        } => creature_on_npc_caught(ctx, *id),
        SimEvent::ZoneEnter { zone_id } => on_zone_enter(ctx, zone_id, out),
        SimEvent::ZoneExit { zone_id } => on_zone_exit(ctx, zone_id, out),
        SimEvent::HubEnter => {
            // hunter.js:32 clear · npc.js:381 placeHub · endgame.js:559 drop the overlay
            if let Some(z) = ctx.zone.get_mut() {
                creature::clear(&mut z.hunters);
            }
            place_hub(ctx);
            ctx.local.ending.0 = EndingScreen::default();
        }
        // contracts.js:349 · npc.js:391 · endgame.js:561
        SimEvent::Death { .. } => {
            let evs = {
                let env = contract_env!(ctx, false);
                contracts::on_death(&env, &mut ctx.save.0)
            };
            out.extend(evs);
            ctx.npcs.0.on_death();
            if let Some(run) = ctx.zone.get_mut().and_then(|z| z.source.as_mut()) {
                run.ride_confirm_t = 0.0;
            }
        }
        // hub.js:108 (the ledger, already inside economy::bank) · contracts.js:348 · npc.js:382
        SimEvent::Bank {
            carried, zone_id, ..
        } => {
            let evs = {
                let env = contract_env!(ctx, true);
                contracts::on_bank(&env, &mut ctx.save.0, &mut ctx.hub.0, carried, zone_id)
            };
            out.extend(evs);
            let pos = (ctx.player.x, ctx.player.z);
            let evs = ctx
                .npcs
                .0
                .on_bank(&ctx.game.config().npc_cfg, &mut ctx.save.0, pos, zone_id);
            out.extend(evs);
        }
        // contracts.js:352 · npc.js:393 · hub.js:113
        SimEvent::NpcRescued { id, .. } => {
            out.extend(contracts::on_rescued(ctx.game.data(), &ctx.save.0, id));
            ctx.npcs.0.on_rescued(id);
            if hub_showing(ctx) {
                place_hub(ctx);
            }
            out.extend(economy::on_npc_rescued_hub(
                ctx.game.data(),
                &ctx.save.0,
                id,
            ));
        }
        // hub.js:118 refreshBuildings(initial) · npc.js:407 · endgame.js:564
        SimEvent::FlameTier {
            tier,
            prev,
            initial,
        } => {
            if *initial {
                let m = ctx.mode.get();
                if m != GameMode::Zone && m != GameMode::Dying {
                    place_hub(ctx);
                }
            } else {
                out.extend(economy::on_flame_tier_hub(
                    ctx.game.data(),
                    &ctx.save.0,
                    *prev,
                    *tier,
                ));
            }
        }
        // npc.js:405 — a closed menu drops the pending contract offer.
        SimEvent::MenuClose { .. } => ctx.npcs.0.on_menu_close(),
        _ => {}
    }
}

/// The `zoneEnter` listeners in registration order: `hunter.js` (spawnAll + pools), `contracts.js`
/// (reset the run's progress, place quest items), `npc.js` (captives), `hub.js` (charge the
/// blessing), `endgame.js` (the Source run and its dormant creatures).
fn on_zone_enter(ctx: &mut RunCtx, zone: &str, out: &mut Vec<SimEvent>) {
    spawn_all(ctx);
    let evs = {
        let env = contract_env!(ctx, true);
        contracts::on_zone_enter(&env, &mut ctx.save.0, zone)
    };
    out.extend(evs);
    let quests = {
        let world = quest_world(ctx);
        let env = contract_env!(ctx, true);
        contracts::quests_to_spawn(&env, &ctx.save.0, zone, &world)
    };
    ctx.spawns.pending_quest_items.extend(quests);
    if let (Some(z), Some(def)) = (ctx.zone.0.as_ref(), ctx.game.data().zone(zone)) {
        ctx.npcs
            .0
            .spawn_zone(ctx.game.data(), def, &z.map, &ctx.save.0);
    }
    let evs = economy::charge_blessing(ctx.game.config(), &mut ctx.save.0, &mut ctx.hub.0);
    out.extend(evs);
    start_source_run(ctx);
}

/// The `zoneExit` listeners: `hunter.js` (clear), `contracts.js`, `npc.js` (hide), `hub.js` (the
/// blessing is spent), `endgame.js` (end the Source run).
fn on_zone_exit(ctx: &mut RunCtx, zone: &str, out: &mut Vec<SimEvent>) {
    if let Some(z) = ctx.zone.get_mut() {
        creature::clear(&mut z.hunters);
    }
    let evs = {
        let env = contract_env!(ctx, false);
        contracts::on_zone_exit(&env, &mut ctx.save.0, zone)
    };
    out.extend(evs);
    ctx.npcs.0.on_zone_exit();
    ctx.hub.0.blessed = false;
    if let Some(z) = ctx.zone.get_mut() {
        z.source = None;
    }
}

/* ============================================================
Creatures: spawn, wake, side effects
============================================================ */

/// `hunter.js`'s `zoneEnter` listener — `spawnAll(ctx.zone)` then `recomputePools()`.
fn spawn_all(ctx: &mut RunCtx) {
    let Some(id) = ctx.zone.id().map(str::to_string) else {
        return;
    };
    let Some(def) = ctx.game.data().zone(&id).cloned() else {
        return;
    };
    let pool_r = ctx.game.config().cfg.pool_r;
    let in_zone = ctx.mode.get() == GameMode::Zone;
    if let Some((fresh, _)) = creature_scope!(ctx, in_zone, |c, _hs| creature::spawn_all(c, &def)) {
        if let Some(z) = ctx.zone.get_mut() {
            z.hunters = fresh;
            let Zone {
                map,
                pool,
                lanterns,
                ..
            } = z;
            pool.recompute(map, lanterns, pool_r);
        }
    }
}

/// `hunter.js:init`'s `npcCaught` listener, for the catching record.
fn creature_on_npc_caught(ctx: &mut RunCtx, hunter_id: u32) {
    let in_zone = ctx.mode.get() == GameMode::Zone;
    creature_scope!(ctx, in_zone, |c, hs| {
        if let Some(i) = hs.iter().position(|h| h.id == hunter_id) {
            creature::on_npc_caught(c, &mut hs[i]);
        }
    });
}

/// `endgame.js:startRun()` — in the Source the hunters that spawned deep go dormant. HANDOFF §6:
/// `creature::spawn_all` activates everything and the shell deactivates the sleepers.
fn start_source_run(ctx: &mut RunCtx) {
    if !in_source(ctx) || ctx.mode.get() != GameMode::Zone {
        if let Some(z) = ctx.zone.get_mut() {
            z.source = None;
        }
        return;
    }
    let cfg = ctx.game.config();
    let Some(z) = ctx.zone.0.as_mut() else { return };
    let live: Vec<(u32, f32, f32, &str)> = z
        .hunters
        .iter()
        .filter(|h| h.active)
        .map(|h| (h.id, h.x, h.z, h.profile.js_name()))
        .collect();
    let run = economy::start_source_run(cfg, &z.map, &live);
    for d in &run.dormant {
        if let Some(h) = z.hunters.iter_mut().find(|h| h.id == d.id) {
            h.active = false;
        }
    }
    z.source = Some(run);
}

/// `endgame.js:onDeeper` — a woken hunter comes back in its profile's initial state. The JS assigns
/// the fields directly, so no `hunterState` is emitted.
fn wake_hunter(ctx: &mut RunCtx, id: u32) {
    let tuning = ctx.game.tuning();
    let Some(z) = ctx.zone.0.as_mut() else { return };
    let Some(h) = z.hunters.iter_mut().find(|h| h.id == id) else {
        return;
    };
    h.active = true;
    h.state = tuning.prof(h.profile).initial;
    h.path.clear();
    h.idle_t = 0.5;
}

/* ============================================================
Zone loading and the run
============================================================ */

/// `main.js:loadZoneInactive(id)` — `world.loadZone` (parse the map, apply the save's gate and
/// shortcut openings, respawn this zone's remembered death bundle) then `spawnAll` + `clear`, so
/// `ctx.hunters[0]` is always a valid record.
fn load_zone_inactive(ctx: &mut RunCtx, id: &str) {
    let Some(def) = ctx.game.data().zone(id).cloned() else {
        warn!("loadZone: unknown zone {id}");
        return;
    };
    let mut map = match ctx.game.data().parse_zone(id) {
        Some(Ok(m)) => m,
        Some(Err(e)) => {
            error!("loadZone {id}: {e}");
            return;
        }
        None => return,
    };
    let doors = sim_world::apply_saved_openings(&mut map, id, &ctx.save.0);
    let pool = Pool::empty(map.len());
    let mut items = Vec::new();
    if let Some((c, x, z)) = ctx.local.bundles.0.get(id).copied() {
        items.push(WorldItem {
            kind: ItemKind::Bundle,
            x,
            z,
            contents: Some(c),
            quest_id: None,
            label: None,
        });
    }
    ctx.zone.0 = Some(Zone {
        id: id.to_string(),
        map,
        doors,
        pool,
        lanterns: Vec::new(),
        hunters: Vec::new(),
        items,
        source: None,
    });
    let in_zone = ctx.mode.get() == GameMode::Zone;
    if let Some((fresh, _)) = creature_scope!(ctx, in_zone, |c, _hs| creature::spawn_all(c, &def)) {
        if let Some(z) = ctx.zone.get_mut() {
            z.hunters = fresh;
            creature::clear(&mut z.hunters);
        }
    }
}

/// `world.js:resetItems()` — every non-bundle item comes back at its map cell.
fn reset_items(ctx: &mut RunCtx) {
    let Some(z) = ctx.zone.get_mut() else { return };
    z.items.retain(|i| i.kind == ItemKind::Bundle);
    let spawns: Vec<WorldItem> = z
        .map
        .items
        .iter()
        .map(|d| {
            let (x, zz) = grid::center(&z.map, d.cx, d.cz);
            WorldItem::new(d.kind, x, zz)
        })
        .collect();
    z.items.extend(spawns);
}

/// `main.js:startRun()`.
fn start_run(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) {
    let Some(id) = ctx.zone.id().map(str::to_string) else {
        return;
    };
    ctx.mode.set(GameMode::Zone);
    if let Some(z) = ctx.zone.0.as_ref() {
        spawn_at(&mut ctx.player, &z.map, 0.0);
    }
    reset_items(ctx);
    let pool_r = ctx.game.config().cfg.pool_r;
    if let Some(z) = ctx.zone.get_mut() {
        z.lanterns.clear();
        let Zone {
            map,
            pool,
            lanterns,
            ..
        } = z;
        pool.recompute(map, lanterns, pool_r);
    }
    let evs = economy::start_run(
        ctx.game.config(),
        &mut ctx.save.0,
        &ctx.hub.0,
        &mut ctx.lamp.0,
        &id,
    );
    push_events(ctx, out, evs);
}

/// `main.js:enterHub()`.
fn enter_hub(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) {
    ctx.mode.set(GameMode::Hub);
    if let Some(hm) = ctx.hub_map.0.as_ref() {
        spawn_at(&mut ctx.player, &hm.map, 0.0);
    }
    let evs = economy::enter_hub(
        ctx.game.config(),
        &ctx.save.0,
        &mut ctx.hub.0,
        &mut ctx.lamp.0,
    );
    push_events(ctx, out, evs);
}

/* ============================================================
Lifecycle
============================================================ */

/// `main.js`'s init block (`saveMod.load()` … `world.init` … `npc.init` … `hub.init`), once. The JS
/// comment there — "listeners must exist before hub.init emits the initial flameTier" — is why the
/// tier announcement comes last, after the hub map, the selected zone and the hub residents.
fn boot_now(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) {
    // save.js:load() — the v2 record, migrated from v2 → v3 by the sim.
    let json = ctx.store.0.load();
    ctx.save.0 = SaveData::load(json.as_deref(), None);
    // world.js:init — the hub map and its collision mask (the hub lane adds prop footprints later).
    match ctx.game.data().parse_hub() {
        Ok(map) => {
            let mask = BlockMask::for_map(&map);
            ctx.hub_map.0 = Some(HubMap { map, mask });
        }
        Err(e) => error!("hub map: {e}"),
    }
    let selected = economy::selected(ctx.game.data(), &ctx.save.0).to_string();
    load_zone_inactive(ctx, &selected);
    place_hub(ctx);
    // hub.js:init — applyTier + `flameTier {tier, prev: 0, initial: true}`.
    let evs = economy::init_tier(ctx.game.config(), &ctx.save.0, &mut ctx.hub.0);
    push_events(ctx, out, evs);
    // main.js: spawnAt(ctx.hub.map, 0) — the main menu looks over the hub.
    if let Some(hm) = ctx.hub_map.0.as_ref() {
        spawn_at(&mut ctx.player, &hm.map, 0.0);
    }
}

/// `main.js:begin()` — Continue / New Game. The screen is already black, so it fades *in* while the
/// hub is entered; there is no fade-out.
fn begin(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Title {
        return false;
    }
    ctx.fade.a = 1.0;
    ctx.fade.mode = FadeMode::In;
    ctx.fade.pending = None;
    push_events(ctx, out, vec![SimEvent::Begin]);
    enter_hub(ctx, out);
    true
}

/// `main.js:descend()` — stairs / tram / elevator all go to `save.zoneSelected`.
fn descend(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Hub {
        return false;
    }
    let loaded = ctx.zone.id().map(str::to_string);
    let r = economy::descend(ctx.game.data(), &ctx.save.0, &ctx.hub.0, loaded.as_deref());
    match r {
        Ok(id) => ctx.fade.start(PendingTransition::StartRun { zone_id: id }),
        Err(evs) => {
            push_events(ctx, out, evs);
            false
        }
    }
}

/// `main.js:bank()` — the fade-out; the banking itself is the callback.
fn bank(ctx: &mut RunCtx) -> bool {
    if ctx.mode.get() != GameMode::Zone {
        return false;
    }
    ctx.fade.start(PendingTransition::Bank)
}

/// The `bank()` fade callback.
fn do_bank(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) {
    let id = zone_id(ctx);
    let evs = economy::bank(
        ctx.game.config(),
        &mut ctx.save.0,
        &mut ctx.player.carried,
        &id,
    );
    push_events(ctx, out, evs);
    enter_hub(ctx, out);
}

/// `main.js:die(h)` — DYING mode, the blessing, the death bundle, `death`. The death camera is the
/// world lane's; neither the player nor the creatures tick while dying (`main.js:385`).
fn die(ctx: &mut RunCtx, hunter_id: Option<u32>, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Zone {
        return false;
    }
    ctx.mode.set(GameMode::Dying);
    ctx.player.dying_t = ctx.game.config().cfg.dying_t;
    let id = zone_id(ctx);
    let (x, z) = (ctx.player.x, ctx.player.z);
    let outcome = economy::die(
        ctx.game.config(),
        &mut ctx.save.0,
        &mut ctx.hub.0,
        &mut ctx.player.carried,
        &id,
        x,
        z,
        hunter_id,
    );
    if let Some(bundle) = outcome.bundle {
        ctx.spawns.pending_bundles.push((bundle, x, z));
    }
    push_events(ctx, out, outcome.events);
    true
}

/// `main.js:returnToHub()` — the death screen's "Return to the Lantern".
fn return_to_hub(ctx: &mut RunCtx) -> bool {
    if ctx.mode.get() != GameMode::Dead || ctx.fade.mode == FadeMode::Out {
        return false;
    }
    ctx.local.ride_up.0 = false;
    ctx.fade.start(PendingTransition::EnterHub)
}

/// The `returnToHub` / `rideUp` fade callback: `zoneExit`, then the hub.
fn do_enter_hub_transition(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) {
    let id = zone_id(ctx);
    if ctx.local.ride_up.0 {
        // endgame.js:rideUp's `go()` — what is carried stays below, then `zoneExit`.
        ctx.local.ride_up.0 = false;
        let mut carried = ctx.player.carried;
        let evs = match ctx.zone.0.as_mut().and_then(|z| z.source.as_mut()) {
            Some(run) => {
                let (_, evs) =
                    economy::ride_up(&ctx.game.asset().data.config, run, &mut carried, true, &id);
                evs
            }
            None => Vec::new(),
        };
        ctx.player.carried = carried;
        push_events(ctx, out, evs);
    } else {
        push_events(ctx, out, vec![SimEvent::ZoneExit { zone_id: id }]);
    }
    enter_hub(ctx, out);
}

/// `main.js:toMainMenu()` — the pause menu's "Main menu". From a zone the run is abandoned first:
/// carried loot is lost, never banked, and the hub is untouched.
fn to_main_menu(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() == GameMode::Menu {
        let kind = ctx.mode.kind.0.clone().unwrap_or_default();
        let back = ctx.mode.prev.0.unwrap_or(GameMode::Hub);
        ctx.mode.set(back);
        ctx.mode.prev.0 = None;
        ctx.mode.kind.0 = None;
        push_events(ctx, out, vec![SimEvent::MenuClose { kind }]);
    }
    // `main.js:470 state.lostBelow` — held back until after `title`, because the ui lane flushes the
    // run's toasts on `title` and this one has to survive that flush (`main.js:481-484`).
    let mut lost_below = String::new();
    if ctx.mode.get() == GameMode::Zone {
        let id = zone_id(ctx);
        let lost = economy::describe(&ctx.player.carried);
        ctx.player.carried = Carried::default();
        push_events(
            ctx,
            out,
            vec![
                SimEvent::RunAbandoned {
                    zone_id: id.clone(),
                    lost: lost.clone(),
                },
                SimEvent::ZoneExit { zone_id: id },
            ],
        );
        enter_hub(ctx, out);
        if lost != "nothing" {
            lost_below = lost;
        }
    }
    if ctx.mode.get() != GameMode::Hub {
        return false;
    }
    ctx.mode.set(GameMode::Title);
    ctx.fade.pending = None;
    ctx.fade.mode = FadeMode::In;
    if let Some(hm) = ctx.hub_map.0.as_ref() {
        spawn_at(&mut ctx.player, &hm.map, 0.0);
    }
    push_events(ctx, out, vec![SimEvent::Title]);
    if !lost_below.is_empty() {
        push_events(
            ctx,
            out,
            vec![SimEvent::toast(format!("Left below: {lost_below}."))],
        );
    }
    true
}

/// `main.js:openMenu(kind)`.
fn open_menu(ctx: &mut RunCtx, kind: &str, out: &mut Vec<SimEvent>) -> bool {
    if !state::can_open_menu(ctx.mode.get()) {
        return false;
    }
    ctx.mode.prev.0 = Some(ctx.mode.get());
    ctx.mode.kind.0 = Some(kind.to_string());
    ctx.mode.set(GameMode::Menu);
    push_events(
        ctx,
        out,
        vec![SimEvent::MenuOpen {
            kind: kind.to_string(),
        }],
    );
    true
}

/// `main.js:closeMenu()`.
fn close_menu(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Menu {
        return false;
    }
    let kind = ctx.mode.kind.0.clone().unwrap_or_default();
    let back = ctx.mode.prev.0.unwrap_or(GameMode::Hub);
    ctx.mode.set(back);
    ctx.mode.prev.0 = None;
    ctx.mode.kind.0 = None;
    push_events(ctx, out, vec![SimEvent::MenuClose { kind }]);
    true
}

/// `main.js:openPause()`.
fn open_pause(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if !state::can_open_menu(ctx.mode.get()) || ctx.fade.mode == FadeMode::Out {
        return false;
    }
    if !open_menu(ctx, "pause", out) {
        return false;
    }
    let in_zone = ctx.mode.prev.0 == Some(GameMode::Zone);
    push_events(ctx, out, vec![SimEvent::PauseOpen { in_zone }]);
    true
}

/// `main.js:closePause()`.
fn close_pause(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Menu || ctx.mode.kind.0.as_deref() != Some("pause") {
        return false;
    }
    close_menu(ctx, out)
}

/// `main.js:resetRuntime()` — `save.reset()` plus every derived runtime state, without a reload.
/// Sound settings are preferences, not progress, and survive.
fn reset_runtime(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    ctx.save.0.reset(true);
    // hub.checkTier(false) — always re-emits `flameTier {initial: true}`.
    let evs = economy::check_tier(ctx.game.config(), &ctx.save.0, &mut ctx.hub.0, false);
    push_events(ctx, out, evs);
    let m = ctx.mode.get();
    if m == GameMode::Title
        || m == GameMode::Hub
        || (m == GameMode::Menu && ctx.mode.prev.0 == Some(GameMode::Hub))
    {
        let selected = economy::selected(ctx.game.data(), &ctx.save.0).to_string();
        load_zone_inactive(ctx, &selected);
    }
    if m == GameMode::Hub || m == GameMode::Title {
        ctx.lamp.0.oil =
            economy::start_oil(ctx.game.config(), ctx.hub.0.tier, ctx.save.0.reservoir);
    }
    push_events(ctx, out, vec![SimEvent::SaveReset]);
    true
}

/// `main.js:clearSave()` — abandon any run, wipe, back to the main menu.
fn clear_save(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    let m = ctx.mode.get();
    if matches!(m, GameMode::Menu | GameMode::Zone | GameMode::Hub) && !to_main_menu(ctx, out) {
        return false;
    }
    if ctx.mode.get() != GameMode::Title {
        return false;
    }
    reset_runtime(ctx, out);
    push_events(ctx, out, vec![SimEvent::toast("Save wiped.")]);
    true
}

/// `main.js:newGame(force)`. The debug command carries no `force` and the skeleton has no confirm
/// panel, so it behaves as `newGame(true)`.
fn new_game(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Title {
        return false;
    }
    if ctx.save.0.has_progress() {
        reset_runtime(ctx, out);
    }
    begin(ctx, out)
}

/// `main.js:actions.gotoZone(id)` — start a run in that zone right now, from the title, the hub, a
/// zone or the death screen: no fade, no lock check.
fn goto_zone(ctx: &mut RunCtx, id: &str, out: &mut Vec<SimEvent>) -> bool {
    if ctx.game.data().zone(id).is_none() {
        return false;
    }
    if ctx.mode.get() == GameMode::Title {
        begin(ctx, out);
    }
    if ctx.mode.get() == GameMode::Menu {
        close_menu(ctx, out);
    }
    if ctx.mode.get() == GameMode::Ending && !cancel_choice(ctx, out) {
        return false;
    }
    ctx.fade.cut_in(0.6);
    if matches!(
        ctx.mode.get(),
        GameMode::Zone | GameMode::Dying | GameMode::Dead
    ) {
        let zid = zone_id(ctx);
        push_events(ctx, out, vec![SimEvent::ZoneExit { zone_id: zid }]);
    }
    let (_, evs) = economy::select_zone(ctx.game.data(), &mut ctx.save.0, &ctx.hub.0, id, false);
    push_events(ctx, out, evs);
    load_zone_inactive(ctx, id);
    start_run(ctx, out);
    true
}

/// `main.js:actions.loadZone(id)` — v2 semantics: in the hub it only loads the zone inactive.
fn load_zone_action(ctx: &mut RunCtx, id: &str, out: &mut Vec<SimEvent>) -> bool {
    if ctx.game.data().zone(id).is_none() {
        return false;
    }
    let (_, evs) = economy::select_zone(ctx.game.data(), &mut ctx.save.0, &ctx.hub.0, id, false);
    push_events(ctx, out, evs);
    if matches!(ctx.mode.get(), GameMode::Zone | GameMode::Dying) {
        let zid = zone_id(ctx);
        push_events(ctx, out, vec![SimEvent::ZoneExit { zone_id: zid }]);
        load_zone_inactive(ctx, id);
        start_run(ctx, out);
    } else if ctx.zone.id() != Some(id) {
        load_zone_inactive(ctx, id);
    }
    true
}

/// `main.js:actions.unlockAll()` — every NPC rescued, tool owned, building built, light-tech III,
/// tier 4, plenty of resources. `checkTier(false)` re-emits `flameTier {initial: true}` so every
/// module re-derives from the save.
fn unlock_all(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    let top = ctx
        .game
        .config()
        .tiers
        .last()
        .map(|t| t.pts)
        .unwrap_or_default();
    {
        let s = &mut ctx.save.0;
        for id in Rescued::IDS {
            s.rescued.set(id, true);
        }
        for id in ["prybar", "sluice", "censer"] {
            s.tools.set(id, true);
        }
        for id in ["workshop", "press", "cart", "shrine", "tram", "elevator"] {
            s.buildings.set(id, true);
        }
        s.points = s.points.max(top);
        s.oil = s.oil.max(999);
        s.relics = s.relics.max(99);
        s.rich = s.rich.max(20);
    }
    let prev_tech = ctx.save.0.light_tech;
    ctx.save.0.light_tech = 3;
    let evs = economy::check_tier(ctx.game.config(), &ctx.save.0, &mut ctx.hub.0, false);
    push_events(ctx, out, evs);
    if prev_tech != 3 {
        push_events(
            ctx,
            out,
            vec![SimEvent::LightTech {
                tier: 3,
                prev: prev_tech,
            }],
        );
    }
    true
}

/* ============================================================
Endgame
============================================================ */

/// `endgame.js:rideUp()` — the first press while carrying only arms the confirmation (no fade); the
/// second leaves, and the leaving half runs in the fade callback as the JS's `go()` did.
fn ride_up(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Zone || !in_source(ctx) {
        return false;
    }
    let Some(armed) = ctx
        .zone
        .get()
        .and_then(|z| z.source.as_ref())
        .map(|r| r.ride_confirm_t > 0.0)
    else {
        return false;
    };
    if ctx.player.carried.total() > 0 && !armed {
        // The sim arms `rideConfirmT` and returns the toast; nothing else happens yet.
        let id = zone_id(ctx);
        let mut carried = ctx.player.carried;
        let evs = match ctx.zone.0.as_mut().and_then(|z| z.source.as_mut()) {
            Some(run) => {
                economy::ride_up(&ctx.game.asset().data.config, run, &mut carried, true, &id).1
            }
            None => Vec::new(),
        };
        ctx.player.carried = carried;
        push_events(ctx, out, evs);
        return true;
    }
    ctx.local.ride_up.0 = true;
    if !ctx.fade.start(PendingTransition::EnterHub) {
        ctx.local.ride_up.0 = false;
        return false;
    }
    true
}

/// `endgame.js:openChoice()` — E at the altar: ENDING mode, hunters frozen.
fn open_choice(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Zone {
        return false;
    }
    let id = zone_id(ctx);
    let (x, z) = (ctx.player.x, ctx.player.z);
    let (ok, evs) = economy::open_choice(&mut ctx.local.ending.0, true, x, z, &id);
    if ok {
        ctx.mode.prev.0 = Some(GameMode::Zone);
        ctx.mode.kind.0 = Some("ending".to_string());
        ctx.mode.set(GameMode::Ending);
    }
    push_events(ctx, out, evs);
    ok
}

/// `endgame.js:cancel()` — Esc on the choice screen, back to the run.
fn cancel_choice(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Ending {
        return false;
    }
    let (ok, evs) = economy::cancel_choice(&mut ctx.local.ending.0);
    if !ok {
        return false;
    }
    let back = ctx.mode.prev.0.unwrap_or(GameMode::Zone);
    ctx.mode.set(back);
    ctx.mode.prev.0 = None;
    ctx.mode.kind.0 = None;
    push_events(ctx, out, evs);
    true
}

/// `endgame.js:choose(id)`.
fn choose_ending(ctx: &mut RunCtx, id: &str, out: &mut Vec<SimEvent>) -> bool {
    if ctx.mode.get() != GameMode::Ending {
        return false;
    }
    let tier = ctx.hub.0.tier;
    let (ok, evs) = economy::choose_ending(
        ctx.game.data(),
        &mut ctx.save.0,
        &mut ctx.local.ending.0,
        tier,
        id,
    );
    push_events(ctx, out, evs);
    ok
}

/// `endgame.js:continueToHub()` — "Click to continue".
fn continue_ending(ctx: &mut RunCtx) -> bool {
    if ctx.mode.get() != GameMode::Ending || ctx.local.ending.0.screen != Some(Screen::End) {
        return false;
    }
    let id = ctx.local.ending.0.current.clone().unwrap_or_default();
    // `endgame.js:312` emits `uiClick` exactly once; `economy::continue_to_hub` already returns it at
    // the tail of its batch, so nothing is pushed here (pushing one too would double the click).
    ctx.fade.start(PendingTransition::EndingContinue { id })
}

/// The `continueToHub` fade callback. The sim returns `zoneExit`, the `night` cue and
/// `endingContinue` in one batch; `enterHub()` belongs between the first and the rest.
fn do_ending_continue(ctx: &mut RunCtx, out: &mut Vec<SimEvent>) {
    let id = zone_id(ctx);
    let mut carried = ctx.player.carried;
    let (_, mut evs) = economy::continue_to_hub(&mut ctx.local.ending.0, &mut carried, &id);
    ctx.player.carried = carried;
    ctx.mode.prev.0 = None;
    ctx.mode.kind.0 = None;
    let tail = if evs.is_empty() {
        Vec::new()
    } else {
        evs.split_off(1)
    };
    push_events(ctx, out, evs);
    enter_hub(ctx, out);
    push_events(ctx, out, tail);
}

/* ============================================================
Systems
============================================================ */

/// The game data has finished loading — every system below reads `CFG` unconditionally, and in the
/// real app `GameMode::Loading` lasts until the asset server is done.
fn data_ready(game: Game) -> bool {
    game.get().is_some()
}

/// `main.js`'s init block, once, as soon as the data is loaded.
fn boot(mut ctx: RunCtx, mut booted: ResMut<Booted>, mut w: MessageWriter<SimMessage>) {
    if booted.0 || ctx.mode.get() == GameMode::Loading || ctx.game.get().is_none() {
        return;
    }
    booted.0 = true;
    let mut out = Vec::new();
    boot_now(&mut ctx, &mut out);
    emit(&mut w, out);
}

/// The `window.__game.actions` this module owns (`main.js:660–740`).
fn owns(cmd: &DebugCommand) -> bool {
    matches!(
        cmd,
        DebugCommand::Begin
            | DebugCommand::Descend
            | DebugCommand::Bank
            | DebugCommand::Die
            | DebugCommand::EnterHub
            | DebugCommand::ReturnToHub
            | DebugCommand::OpenMenu(_)
            | DebugCommand::CloseMenu
            | DebugCommand::OpenMainMenu
            | DebugCommand::OpenPause
            | DebugCommand::ClosePause
            | DebugCommand::ClearSave
            | DebugCommand::NewGame
            | DebugCommand::LoadZone(_)
            | DebugCommand::SelectZone(_)
            | DebugCommand::FreeNpc(_)
            | DebugCommand::Accept(_)
            | DebugCommand::Build { .. }
            | DebugCommand::Choose(_)
            | DebugCommand::GiveTool(_)
            | DebugCommand::SetPoints(_)
            | DebugCommand::SetResources { .. }
            | DebugCommand::Rescue(_)
            | DebugCommand::UnlockAll
            | DebugCommand::GotoZone(_)
            | DebugCommand::SpawnHunter { .. }
            | DebugCommand::SpawnCreature { .. }
            | DebugCommand::RideUp
            | DebugCommand::OpenChoice
            | DebugCommand::ContinueEnding
            | DebugCommand::Reset
    )
}

/// Drain and run this module's debug commands ([`DebugSet::Handle`]).
fn handle_debug(
    mut ctx: RunCtx,
    mut queue: ResMut<DebugQueue>,
    mut writer: ResMut<SaveWriter>,
    mut w: MessageWriter<SimMessage>,
) {
    let cmds = queue.take(owns);
    if cmds.is_empty() {
        return;
    }
    let mut out = Vec::new();
    for cmd in &cmds {
        run_command(&mut ctx, cmd, &mut writer, &mut out);
    }
    emit(&mut w, out);
}

fn run_command(
    ctx: &mut RunCtx,
    cmd: &DebugCommand,
    writer: &mut SaveWriter,
    out: &mut Vec<SimEvent>,
) {
    match cmd {
        DebugCommand::Begin => {
            begin(ctx, out);
        }
        DebugCommand::Descend => {
            descend(ctx, out);
        }
        DebugCommand::Bank => {
            bank(ctx);
        }
        DebugCommand::Die => {
            let h = ctx
                .zone
                .get()
                .and_then(|z| z.hunters.iter().find(|h| h.active).map(|h| h.id));
            if die(ctx, h, out) {
                store_now(ctx, writer);
            }
        }
        DebugCommand::EnterHub => enter_hub(ctx, out),
        DebugCommand::ReturnToHub => {
            return_to_hub(ctx);
        }
        DebugCommand::OpenMenu(kind) => {
            open_menu(ctx, kind, out);
        }
        DebugCommand::CloseMenu => {
            close_menu(ctx, out);
        }
        DebugCommand::OpenMainMenu => {
            to_main_menu(ctx, out);
        }
        DebugCommand::OpenPause => {
            open_pause(ctx, out);
        }
        DebugCommand::ClosePause => {
            close_pause(ctx, out);
        }
        DebugCommand::ClearSave => {
            clear_save(ctx, out);
            store_now(ctx, writer);
        }
        DebugCommand::NewGame => {
            new_game(ctx, out);
        }
        DebugCommand::LoadZone(id) => {
            load_zone_action(ctx, id, out);
        }
        DebugCommand::SelectZone(id) => {
            let (_, evs) =
                economy::select_zone(ctx.game.data(), &mut ctx.save.0, &ctx.hub.0, id, true);
            push_events(ctx, out, evs);
        }
        DebugCommand::FreeNpc(id) => {
            let in_zone = ctx.mode.get() == GameMode::Zone;
            let evs = match ctx.zone.0.as_ref() {
                Some(z) => ctx.npcs.0.free(ctx.game.data(), &z.map, id, in_zone).1,
                None => Vec::new(),
            };
            push_events(ctx, out, evs);
        }
        DebugCommand::Accept(id) => {
            let in_zone = ctx.mode.get() == GameMode::Zone;
            let evs = {
                let env = contract_env!(ctx, in_zone);
                contracts::accept(&env, &mut ctx.save.0, id, false).1
            };
            push_events(ctx, out, evs);
        }
        DebugCommand::Build { id, free } => {
            let (_, evs) = economy::build(ctx.game.data(), &mut ctx.save.0, &ctx.hub.0, id, *free);
            push_events(ctx, out, evs);
        }
        DebugCommand::Choose(id) => {
            choose_ending(ctx, id, out);
        }
        DebugCommand::GiveTool(id) => {
            let (_, evs) = economy::give_tool(&mut ctx.save.0, id);
            push_events(ctx, out, evs);
        }
        DebugCommand::SetPoints(n) => {
            let evs = economy::set_points(ctx.game.config(), &mut ctx.save.0, &mut ctx.hub.0, *n);
            push_events(ctx, out, evs);
            store_now(ctx, writer);
        }
        DebugCommand::SetResources { oil, relics, rich } => {
            if let Some(v) = oil {
                ctx.save.0.oil = *v;
            }
            if let Some(v) = relics {
                ctx.save.0.relics = *v;
            }
            if let Some(v) = rich {
                ctx.save.0.rich = *v;
            }
            store_now(ctx, writer);
        }
        DebugCommand::Rescue(id) => {
            if ctx.save.0.rescued.get(id) {
                return;
            }
            let zid = zone_id(ctx);
            let evs = ctx.npcs.0.rescue_debug(&mut ctx.save.0, id, &zid);
            push_events(ctx, out, evs);
        }
        DebugCommand::UnlockAll => {
            unlock_all(ctx, out);
        }
        DebugCommand::GotoZone(id) => {
            goto_zone(ctx, id, out);
        }
        DebugCommand::SpawnHunter { cx, cz, profile } => {
            let kind = ProfileKind::from_js_name_or_base(profile);
            let in_zone = ctx.mode.get() == GameMode::Zone;
            let (cx, cz) = (*cx, *cz);
            if let Some((_, evs)) = creature_scope!(ctx, in_zone, |c, hs| creature::spawn_hunter(
                c, hs, cx, cz, kind
            )) {
                push_events(ctx, out, evs);
            }
        }
        DebugCommand::SpawnCreature {
            profile,
            cx,
            cz,
            opts,
        } => {
            let kind = ProfileKind::from_js_name_or_base(profile);
            let in_zone = ctx.mode.get() == GameMode::Zone;
            let (cx, cz, opts) = (*cx, *cz, *opts);
            if let Some((_, evs)) = creature_scope!(ctx, in_zone, |c, hs| {
                creature::spawn_creature(c, hs, kind, cx, cz, opts)
            }) {
                push_events(ctx, out, evs);
            }
        }
        DebugCommand::RideUp => {
            ride_up(ctx, out);
        }
        DebugCommand::OpenChoice => {
            open_choice(ctx, out);
        }
        DebugCommand::ContinueEnding => {
            continue_ending(ctx);
        }
        DebugCommand::Reset => {
            reset_runtime(ctx, out);
            store_now(ctx, writer);
        }
        _ => {}
    }
}

/// `main.js:210 updateFade(dt)` — the black overlay and its pending callback.
fn drive_fade(mut ctx: RunCtx, time: Res<Time>, mut w: MessageWriter<SimMessage>) {
    let dt = time.delta_secs();
    let fade_t = ctx.game.config().cfg.fade_t;
    let mut out = Vec::new();
    match ctx.fade.mode {
        FadeMode::Out => {
            ctx.fade.a = (ctx.fade.a + dt / fade_t).min(1.0);
            if ctx.fade.a >= 1.0 {
                let cb = ctx.fade.pending.take();
                ctx.fade.mode = FadeMode::In;
                match cb {
                    Some(PendingTransition::StartRun { zone_id }) => {
                        if ctx.zone.id() != Some(zone_id.as_str()) {
                            load_zone_inactive(&mut ctx, &zone_id);
                        }
                        start_run(&mut ctx, &mut out);
                    }
                    Some(PendingTransition::Bank) => do_bank(&mut ctx, &mut out),
                    Some(PendingTransition::EnterHub) => {
                        do_enter_hub_transition(&mut ctx, &mut out)
                    }
                    Some(PendingTransition::Dead) => ctx.mode.set(GameMode::Dead),
                    Some(PendingTransition::Title) => {
                        to_main_menu(&mut ctx, &mut out);
                    }
                    Some(PendingTransition::EndingContinue { .. }) => {
                        do_ending_continue(&mut ctx, &mut out)
                    }
                    None => {}
                }
            }
        }
        FadeMode::In => {
            ctx.fade.a = (ctx.fade.a - dt / fade_t).max(0.0);
            if ctx.fade.a <= 0.0 {
                ctx.fade.mode = FadeMode::Idle;
            }
        }
        FadeMode::Idle => {}
    }
    emit(&mut w, out);
}

/// `main.js:update` — the DYING countdown, then `showDeathScreen()`.
fn tick_dying(
    time: Res<Time>,
    mut player: ResMut<Player>,
    state: Res<State<GameMode>>,
    mut next: ResMut<NextState<GameMode>>,
) {
    if *state.get() != GameMode::Dying || !matches!(*next, NextState::Unchanged) {
        return;
    }
    player.dying_t -= time.delta_secs();
    if player.dying_t <= 0.0 {
        player.dying_t = 0.0;
        next.set(GameMode::Dead);
    }
}

/// `hunter.js:update(ctx, dt)` plus `main.js`'s listeners for what it returns (HANDOFF §6).
fn tick_creatures(mut ctx: RunCtx, time: Res<Time>, mut w: MessageWriter<SimMessage>) {
    if !state::sim_runs(ctx.mode.get()) || ctx.zone.get().is_none() {
        return;
    }
    let dt = time.delta_secs();
    let Some((_, evs)) = creature_scope!(&mut ctx, true, |c, hs| creature::update(c, hs, dt))
    else {
        return;
    };
    // The Brute's smash: `lanternRemoved` + `lanternSmashed` are requests — the shell removes the
    // lantern and recomputes the pool (HANDOFF §6).
    let pool_r = ctx.game.config().cfg.pool_r;
    let mut removed = false;
    for e in &evs {
        if let SimEvent::LanternRemoved { x, z } = e {
            if let Some(zn) = ctx.zone.get_mut() {
                if let Some(i) = zn
                    .lanterns
                    .iter()
                    .position(|l| (l.x - x).abs() < 1e-3 && (l.z - z).abs() < 1e-3)
                {
                    zn.lanterns.remove(i);
                    removed = true;
                }
            }
        }
    }
    if removed {
        if let Some(zn) = ctx.zone.get_mut() {
            let Zone {
                map,
                pool,
                lanterns,
                ..
            } = zn;
            pool.recompute(map, lanterns, pool_r);
        }
    }
    let mut out = Vec::new();
    push_events(&mut ctx, &mut out, evs);
    emit(&mut w, out);
}

/// `npc.js:update(ctx, dt)`.
fn tick_follower(mut ctx: RunCtx, time: Res<Time>, mut w: MessageWriter<SimMessage>) {
    let dt = time.delta_secs();
    let time_s = ctx.snap.clock.time;
    let mut out = Vec::new();
    match ctx.mode.get() {
        GameMode::Zone => {
            let Some(z) = ctx.zone.0.as_ref() else { return };
            let lanterns: Vec<(f32, f32)> = z.lanterns.iter().map(|l| (l.x, l.z)).collect();
            let touches: Vec<HunterTouch> = z
                .hunters
                .iter()
                .map(|h| HunterTouch {
                    id: h.id,
                    x: h.x,
                    z: h.z,
                    active: h.active,
                    staggered: h.state == HState::Staggered,
                })
                .collect();
            let cfg = ctx.game.config();
            let evs = ctx.npcs.0.update_zone(
                ctx.game.data(),
                &cfg.npc_cfg,
                &cfg.cfg,
                dt,
                time_s,
                &ctx.snap.view.0,
                &z.map,
                &lanterns,
                &touches,
            );
            push_events(&mut ctx, &mut out, evs);
        }
        GameMode::Hub | GameMode::Title => {
            ctx.npcs
                .0
                .update_hub(&ctx.game.config().npc_cfg, dt, time_s, &ctx.snap.view.0);
        }
        _ => {}
    }
    emit(&mut w, out);
}

/// `contracts.js:update(ctx, dt)`.
fn tick_contracts(mut ctx: RunCtx, time: Res<Time>, mut w: MessageWriter<SimMessage>) {
    if ctx.mode.get() != GameMode::Zone || ctx.zone.get().is_none() {
        return;
    }
    let dt = time.delta_secs();
    let evs = {
        let env = contract_env!(ctx, true);
        contracts::tick(
            &env,
            &mut ctx.save.0,
            &mut ctx.hub.0,
            dt,
            &ctx.snap.view.0,
            false,
        )
    };
    let mut out = Vec::new();
    push_events(&mut ctx, &mut out, evs);
    emit(&mut w, out);
}

/// `endgame.js:update(ctx, dt)` — the lap tracker and its staged pressure.
fn tick_source(mut ctx: RunCtx, time: Res<Time>, mut w: MessageWriter<SimMessage>) {
    let mode = ctx.mode.get();
    if !matches!(
        mode,
        GameMode::Zone | GameMode::Ending | GameMode::Menu | GameMode::Dying
    ) || ctx.zone.get().and_then(|z| z.source.as_ref()).is_none()
    {
        return;
    }
    let dt = time.delta_secs();
    let in_zone = mode == GameMode::Zone;
    let player_lap = ctx.player.lap;
    let id = zone_id(&ctx);
    let player_pos = (ctx.player.x, ctx.player.z);
    let crossed = {
        let cfg = ctx.game.config();
        let run = ctx
            .zone
            .0
            .as_mut()
            .and_then(|z| z.source.as_mut())
            .expect("checked above");
        economy::update_source_run(run, cfg, dt, in_zone, player_lap)
    };
    let Some((lap, prev)) = crossed else {
        return;
    };
    let outcome = {
        let data = ctx.game.data();
        let Some(z) = ctx.zone.0.as_mut() else { return };
        let active_base_fast = z
            .hunters
            .iter()
            .filter(|h| h.active && matches!(h.profile, ProfileKind::Base | ProfileKind::Fast))
            .count() as u32;
        let Zone { map, source, .. } = z;
        let run = source.as_mut().expect("checked above");
        economy::on_deeper(
            data,
            run,
            lap,
            prev,
            &id,
            map,
            player_pos,
            active_base_fast,
            &mut ctx.rng.0,
        )
    };
    for id in outcome.woken {
        wake_hunter(&mut ctx, id);
    }
    ctx.spawns.pending_extra_hunters.extend(outcome.spawn);
    let mut out = Vec::new();
    push_events(&mut ctx, &mut out, outcome.events);
    emit(&mut w, out);
}

/// Drain the spawn requests the sim returned as data (HANDOFF §6): quest items, death bundles and
/// the Source's extra hunters.
fn drain_spawns(mut ctx: RunCtx, mut w: MessageWriter<SimMessage>) {
    if ctx.spawns.is_empty() {
        return;
    }
    let mut out = Vec::new();
    let quests = std::mem::take(&mut ctx.spawns.pending_quest_items);
    let bundles = std::mem::take(&mut ctx.spawns.pending_bundles);
    let extras = std::mem::take(&mut ctx.spawns.pending_extra_hunters);
    let zid = zone_id(&ctx);
    for q in quests {
        if let Some(z) = ctx.zone.get_mut() {
            z.items.push(WorldItem {
                kind: ItemKind::Quest,
                x: q.x,
                z: q.z,
                contents: None,
                quest_id: Some(q.contract_id),
                label: Some(q.label),
            });
        }
    }
    for (c, x, zz) in bundles {
        if let Some(z) = ctx.zone.get_mut() {
            // main.js:die — one bundle per zone; the older one is removed first.
            z.items.retain(|i| i.kind != ItemKind::Bundle);
            z.items.push(WorldItem {
                kind: ItemKind::Bundle,
                x,
                z: zz,
                contents: Some(c),
                quest_id: None,
                label: None,
            });
        }
        ctx.local.bundles.0.insert(zid.clone(), (c, x, zz));
    }
    for ex in extras {
        let kind = ProfileKind::from_js_name_or_base(&ex.profile);
        let (cx, cz, lap) = (ex.cx, ex.cz, ex.lap);
        let Some((idx, evs)) = creature_scope!(&mut ctx, true, |c, hs| creature::spawn_hunter(
            c, hs, cx, cz, kind
        )) else {
            continue;
        };
        push_events(&mut ctx, &mut out, evs);
        let spawned = idx
            .and_then(|i| ctx.zone.get().and_then(|z| z.hunters.get(i)))
            .map(|h| SimEvent::HunterSpawned {
                id: h.id,
                x: h.x,
                z: h.z,
                profile: ex.profile.clone(),
                lap,
            });
        if let Some(ev) = spawned {
            push_events(&mut ctx, &mut out, vec![ev]);
        }
    }
    emit(&mut w, out);
}

/// The listeners for events this module never produces itself: `gateOpened` / `shortcutOpened` drop
/// every path (`hunter.js:35`, `npc.js:402`), and a picked-up bundle is forgotten
/// (`world.js:removeItem`).
fn react_external(
    mut r: MessageReader<SimMessage>,
    mut zone: ResMut<ZoneRes>,
    mut npcs: ResMut<Npcs>,
    mut bundles: ResMut<BundleStore>,
) {
    for m in r.read() {
        match &m.0 {
            SimEvent::GateOpened { .. } | SimEvent::ShortcutOpened { .. } => {
                if let Some(z) = zone.get_mut() {
                    creature::clear_paths(&mut z.hunters);
                }
                npcs.0.on_door_opened();
            }
            SimEvent::Pickup {
                kind: ItemKind::Bundle,
                ..
            } => {
                if let Some(id) = zone.id().map(str::to_string) {
                    bundles.0.remove(&id);
                }
            }
            _ => {}
        }
    }
}

/// `save.js` — one write per [`SAVE_DEBOUNCE`] seconds, whenever anything was emitted.
fn persist_save(
    mut r: MessageReader<SimMessage>,
    time: Res<Time>,
    mut writer: ResMut<SaveWriter>,
    save: Res<SaveRes>,
    store: Res<SaveStoreRes>,
) {
    if r.read().count() > 0 {
        writer.dirty = true;
        if writer.timer <= 0.0 {
            writer.timer = SAVE_DEBOUNCE;
        }
    }
    if writer.timer > 0.0 {
        writer.timer -= time.delta_secs();
        if writer.timer <= 0.0 {
            writer.timer = 0.0;
            if writer.dirty {
                writer.dirty = false;
                store.0.store(&save.0.to_json());
            }
        }
    }
}

/// The `main.js` lifecycle: mode changes, the fade, and the per-tick sim calls that are not the
/// player's.
pub fn plugin(app: &mut App) {
    app.init_resource::<EndingScreenRes>()
        .init_resource::<BundleStore>()
        .init_resource::<SaveWriter>()
        .init_resource::<Booted>()
        .init_resource::<RideUpPending>()
        .add_systems(
            FixedUpdate,
            (boot, handle_debug)
                .chain()
                .in_set(DebugSet::Handle)
                .run_if(data_ready),
        )
        .add_systems(
            FixedUpdate,
            tick_creatures.in_set(SimSet::Creatures).run_if(data_ready),
        )
        .add_systems(
            FixedUpdate,
            tick_follower.in_set(SimSet::Follower).run_if(data_ready),
        )
        .add_systems(
            FixedUpdate,
            tick_contracts.in_set(SimSet::Contracts).run_if(data_ready),
        )
        .add_systems(
            FixedUpdate,
            (
                drive_fade,
                tick_dying,
                tick_source,
                drain_spawns,
                react_external,
                persist_save,
            )
                .chain()
                .in_set(SimSet::Economy)
                .run_if(data_ready),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::*;
    use crate::resources::MemoryStore;

    /// `headless_app()` falls back to `resources::default_store()`, which is a *file* natively; every
    /// test here keeps its own `localStorage` so nothing leaks between runs.
    fn app() -> App {
        headless_app_with_store(MemoryStore::new())
    }

    fn zone_res(app: &App) -> &ZoneRes {
        app.world().resource::<ZoneRes>()
    }

    fn save_of(app: &App) -> &SaveData {
        &app.world().resource::<SaveRes>().0
    }

    fn dying_t(app: &App) -> f32 {
        let handle = app.world().resource::<crate::GameDataHandle>().0.clone();
        let assets = app.world().resource::<Assets<crate::GameDataAsset>>();
        assets.get(&handle).expect("data").data.config.cfg.dying_t
    }

    /// The map's `H` + creature cells — what `hunter.js:spawnAll` creates.
    fn spawn_count(id: &str) -> usize {
        let asset = workspace_asset();
        let m = asset.data.parse_zone(id).expect("zone").expect("map");
        m.hunter_spawns.len() + m.creatures.len()
    }

    /// `main.js:begin()` — the title fades in on the hub; the initial `flameTier` comes from the
    /// init block (`hub.init`), before `begin` itself, exactly as in the JS.
    #[test]
    fn begin_enters_the_hub_in_js_order() {
        let mut app = app();
        send(&mut app, DebugCommand::Begin);
        step(&mut app, 2.0);
        assert_eq!(mode(&app), GameMode::Hub);
        let names = log(&app).names();
        let begin_at = names.iter().position(|n| *n == "begin").expect("begin");
        let hub_at = names
            .iter()
            .position(|n| *n == "hubEnter")
            .expect("hubEnter");
        assert!(begin_at < hub_at, "begin before hubEnter: {names:?}");
        assert!(
            log(&app)
                .all("flameTier")
                .iter()
                .any(|e| matches!(e, SimEvent::FlameTier { initial: true, .. })),
            "the init block announced the tier: {names:?}"
        );
        assert!(app.world().resource::<HubMapRes>().0.is_some());
    }

    /// `actions.gotoZone(id)` from the title: begin, then straight into the run.
    #[test]
    fn goto_zone_starts_a_run_with_every_spawn() {
        let mut app = app();
        send(&mut app, DebugCommand::GotoZone("undercroft".into()));
        step(&mut app, 0.5);
        assert_eq!(mode(&app), GameMode::Zone);
        let z = zone_res(&app).get().expect("zone loaded");
        assert_eq!(z.id, "undercroft");
        assert_eq!(z.hunters.len(), spawn_count("undercroft"));
        assert!(z.hunters.iter().all(|h| h.active), "spawnAll activates all");
        assert!(!z.items.is_empty(), "resetItems placed the map's loot");
        assert_eq!(log(&app).count("zoneEnter"), 1);
        assert_eq!(save_of(&app).stats.runs, 1);
    }

    /// `die()` → DYING for `CFG.dyingT`, then the death screen; `returnToHub()` fades back.
    #[test]
    fn dying_becomes_dead_then_returns_to_the_hub() {
        let mut app = app();
        send(&mut app, DebugCommand::GotoZone("undercroft".into()));
        step(&mut app, 0.5);
        let t = dying_t(&app);
        send(&mut app, DebugCommand::Die);
        step(&mut app, 1.0 / 30.0);
        assert_eq!(mode(&app), GameMode::Dying);
        assert_eq!(log(&app).count("death"), 1);
        // one extra tick: `NextState` is applied by the `StateTransition` schedule of the
        // *next* frame, so the mode the test reads lags the tick that set it.
        step(&mut app, t + 2.0 / 60.0);
        assert_eq!(mode(&app), GameMode::Dead);
        assert_eq!(save_of(&app).stats.deaths, 1);
        send(&mut app, DebugCommand::ReturnToHub);
        step(&mut app, 2.0);
        assert_eq!(mode(&app), GameMode::Hub);
        let names = log(&app).names();
        let exit = names
            .iter()
            .rposition(|n| *n == "zoneExit")
            .expect("zoneExit");
        let hub = names
            .iter()
            .rposition(|n| *n == "hubEnter")
            .expect("hubEnter");
        assert!(exit < hub, "zoneExit before hubEnter: {names:?}");
    }

    /// `openPause()` / `closePause()` (`main.js:426` + `state.prevMode`).
    #[test]
    fn the_pause_menu_round_trips_through_prev_mode() {
        let mut app = app();
        send(&mut app, DebugCommand::Begin);
        step(&mut app, 0.5);
        send(&mut app, DebugCommand::OpenPause);
        step(&mut app, 1.0 / 30.0);
        assert_eq!(mode(&app), GameMode::Menu);
        assert_eq!(app.world().resource::<PrevMode>().0, Some(GameMode::Hub));
        assert_eq!(
            app.world().resource::<MenuKind>().0.as_deref(),
            Some("pause")
        );
        assert_eq!(log(&app).count("pauseOpen"), 1);
        send(&mut app, DebugCommand::ClosePause);
        step(&mut app, 1.0 / 30.0);
        assert_eq!(mode(&app), GameMode::Hub);
        assert_eq!(app.world().resource::<PrevMode>().0, None);
        assert_eq!(log(&app).count("menuClose"), 1);
    }

    /// `toMainMenu()` from a zone abandons the run (`main.js:461`). The "Left below" toast is emitted
    /// *after* `title`, not before: `main.js:481-484` emits `title` first so the ui lane's flush of the
    /// run's toasts cannot swallow this one ("… so this one shows over the main menu").
    #[test]
    fn the_main_menu_abandons_a_run() {
        let mut app = app();
        send(&mut app, DebugCommand::GotoZone("undercroft".into()));
        step(&mut app, 0.5);
        app.world_mut().resource_mut::<Player>().carried = Carried {
            oil: 2,
            relic: 0,
            rich: 0,
            quest: 0,
        };
        send(&mut app, DebugCommand::OpenMainMenu);
        step(&mut app, 0.5);
        assert_eq!(mode(&app), GameMode::Title);
        assert_eq!(log(&app).count("runAbandoned"), 1);
        let names = log(&app).names();
        let ab = names.iter().position(|n| *n == "runAbandoned").expect("ab");
        let ti = names.iter().position(|n| *n == "title").expect("title");
        assert!(ab < ti, "runAbandoned before title: {names:?}");
        let left_below = log(&app)
            .entries()
            .iter()
            .position(
                |(_, e)| matches!(e, SimEvent::Toast { msg, .. } if msg.starts_with("Left below")),
            )
            .expect("the abandoned loot is announced");
        assert!(
            ti < left_below,
            "`title` before the \"Left below\" toast: {names:?}"
        );
    }

    /// `actions.giveTool(id)` — `toolGained` fires once (`main.js:686`).
    #[test]
    fn give_tool_is_idempotent() {
        let mut app = app();
        send(&mut app, DebugCommand::Begin);
        step(&mut app, 0.5);
        send(&mut app, DebugCommand::GiveTool("prybar".into()));
        send(&mut app, DebugCommand::GiveTool("prybar".into()));
        step(&mut app, 0.5);
        assert!(save_of(&app).tools.prybar);
        assert_eq!(log(&app).count("toolGained"), 1);
    }

    /// `endgame.js:startRun()` — HANDOFF §6 Source dormancy: `spawn_all` activates every record and
    /// the shell puts the deep `L Y B` creatures back to sleep.
    #[test]
    fn the_source_keeps_its_deep_creatures_dormant() {
        let mut app = app();
        send(&mut app, DebugCommand::UnlockAll);
        send(&mut app, DebugCommand::GotoZone("source".into()));
        step(&mut app, 0.5);
        assert_eq!(mode(&app), GameMode::Zone);
        let z = zone_res(&app).get().expect("source loaded");
        let run = z.source.as_ref().expect("endgame.js S.run");
        assert!(
            !run.dormant.is_empty(),
            "the Source spawns hunters deeper than lap 1"
        );
        for d in &run.dormant {
            let h = z.hunters.iter().find(|h| h.id == d.id).expect("record");
            assert!(!h.active, "dormant {} is asleep", h.profile.js_name());
        }
        assert!(
            z.hunters.iter().any(|h| h.active),
            "the shallow ones still walk"
        );
    }

    /// `save.js` — the store is the only channel between two sessions.
    #[test]
    fn the_save_survives_into_a_second_app() {
        let shared = MemoryStore::new();
        let mut a = headless_app_with_store(shared.clone());
        send(&mut a, DebugCommand::Begin);
        step(&mut a, 0.5);
        send(&mut a, DebugCommand::SetPoints(12));
        send(
            &mut a,
            DebugCommand::SetResources {
                oil: Some(7),
                relics: None,
                rich: None,
            },
        );
        step(&mut a, 0.5);
        assert_eq!(save_of(&a).points, 12);
        assert!(shared.peek().is_some(), "the store was written");

        let mut b = headless_app_with_store(shared.clone());
        step(&mut b, 0.5);
        assert_eq!(save_of(&b).points, 12);
        assert_eq!(save_of(&b).oil, 7);
    }

    /// `main.js:bank()` — the fade callback banks, credits the v2 ledger (`hub.js`'s `bank`
    /// listener, inside `economy::bank`) and enters the hub, in that order.
    #[test]
    fn banking_credits_the_ledger_and_returns_to_the_hub() {
        let mut app = app();
        send(&mut app, DebugCommand::GotoZone("undercroft".into()));
        step(&mut app, 0.5);
        app.world_mut().resource_mut::<Player>().carried = Carried {
            oil: 2,
            relic: 1,
            rich: 0,
            quest: 0,
        };
        send(&mut app, DebugCommand::Bank);
        step(&mut app, 2.0);
        assert_eq!(mode(&app), GameMode::Hub);
        assert_eq!(log(&app).count("bank"), 1);
        assert_eq!(save_of(&app).points, 5, "2 flasks + 1 relic");
        assert_eq!(save_of(&app).relics, 1);
        assert!(app.world().resource::<Player>().carried.is_empty());
        let names = log(&app).names();
        let bank = names.iter().position(|n| *n == "bank").expect("bank");
        let exit = names
            .iter()
            .rposition(|n| *n == "zoneExit")
            .expect("zoneExit");
        let hub = names
            .iter()
            .rposition(|n| *n == "hubEnter")
            .expect("hubEnter");
        assert!(bank < exit && exit < hub, "{names:?}");
    }

    /// `main.js:die` + HANDOFF §6: `economy::die` returns the bundle as data and the shell drops it
    /// in the zone, where `world.js` remembers it for the next visit.
    #[test]
    fn a_death_leaves_one_bundle_behind() {
        let mut app = app();
        send(&mut app, DebugCommand::GotoZone("undercroft".into()));
        step(&mut app, 0.5);
        app.world_mut().resource_mut::<Player>().carried = Carried {
            oil: 3,
            relic: 0,
            rich: 0,
            quest: 0,
        };
        send(&mut app, DebugCommand::Die);
        step(&mut app, 0.5);
        let z = zone_res(&app).get().expect("zone");
        let bundles: Vec<_> = z
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Bundle)
            .collect();
        assert_eq!(bundles.len(), 1, "one bundle per zone");
        assert_eq!(bundles[0].contents.map(|c| c.oil), Some(3));
        assert!(app
            .world()
            .resource::<BundleStore>()
            .0
            .contains_key("undercroft"));
        assert!(app.world().resource::<Player>().carried.is_empty());
    }

    /// `hub.select` + `descend()` — the full fade path, and `Zone.doors` comes from the save. The fade
    /// callback runs in `SimSet::Economy`, i.e. after `player_lamp` has already run this tick, so this
    /// also covers the other half of the shared-mode fix: the lamp `economy::start_run` lights must
    /// still be lit on the next tick.
    #[test]
    fn descend_fades_into_the_selected_zone() {
        let mut app = app();
        send(&mut app, DebugCommand::Begin);
        step(&mut app, 0.5);
        send(&mut app, DebugCommand::SelectZone("undercroft".into()));
        send(&mut app, DebugCommand::Descend);
        step(&mut app, 2.0);
        assert_eq!(mode(&app), GameMode::Zone);
        assert_eq!(zone_res(&app).id(), Some("undercroft"));
        assert_eq!(log(&app).count("zoneEnter"), 1);
        assert!(
            app.world().resource::<LampRes>().0.lamp_on,
            "descend leaves the handlamp lit"
        );
    }

    /// `endgame.js:296 continueToHub()` emits `uiClick` once (`endgame.js:312`); the fade callback then
    /// runs `zoneExit → enterHub → endingContinue`. Nothing may add a second click.
    #[test]
    fn the_ending_continue_clicks_once_and_returns_to_the_hub() {
        let mut app = app();
        send(&mut app, DebugCommand::UnlockAll);
        send(&mut app, DebugCommand::GotoZone("source".into()));
        step(&mut app, 0.5);
        let altar = {
            let z = zone_res(&app);
            z.get()
                .expect("source")
                .map
                .altar
                .expect("the Source altar")
        };
        send(
            &mut app,
            DebugCommand::Teleport {
                x: altar.x,
                z: altar.z,
                yaw: Some(0.0),
            },
        );
        step(&mut app, 1.0 / 60.0);
        send(&mut app, DebugCommand::OpenChoice);
        step(&mut app, 1.0 / 30.0);
        assert_eq!(mode(&app), GameMode::Ending, "{:?}", log(&app).names());
        send(&mut app, DebugCommand::Choose("cage".into()));
        step(&mut app, 1.0 / 30.0);
        assert_eq!(log(&app).count("ending"), 1, "{:?}", log(&app).names());

        let clicks = log(&app).count("uiClick");
        send(&mut app, DebugCommand::ContinueEnding);
        step(&mut app, 2.0);
        assert_eq!(mode(&app), GameMode::Hub);
        assert_eq!(log(&app).count("endingContinue"), 1);
        assert_eq!(
            log(&app).count("uiClick") - clicks,
            1,
            "one click, not two: {:?}",
            log(&app).names()
        );
        let names = log(&app).names();
        let exit = names.iter().rposition(|n| *n == "zoneExit").expect("exit");
        let hub = names.iter().rposition(|n| *n == "hubEnter").expect("hub");
        let cont = names
            .iter()
            .rposition(|n| *n == "endingContinue")
            .expect("continue");
        assert!(exit < hub && hub < cont, "{names:?}");
    }

    /// `main.js:actions.clearSave()` — abandon, wipe, back to the title.
    #[test]
    fn clear_save_wipes_and_returns_to_the_title() {
        let mut app = app();
        send(&mut app, DebugCommand::GotoZone("undercroft".into()));
        step(&mut app, 0.5);
        send(&mut app, DebugCommand::SetPoints(9));
        step(&mut app, 0.5);
        send(&mut app, DebugCommand::ClearSave);
        step(&mut app, 0.5);
        assert_eq!(mode(&app), GameMode::Title);
        assert_eq!(save_of(&app).points, 0);
        assert_eq!(log(&app).count("saveReset"), 1);
    }
}
