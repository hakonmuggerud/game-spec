//! LANE: economy. Captive NPCs and the follower AI — `npc.js` (DESIGN-v2 §3, DESIGN.md §7), minus the meshes.
//!
//! [`Npcs`] owns the NPC records (`npc.js` `recs` + `ctx.npcs` + `ctx.player.follower`): zone captives, the one
//! follower, and the hub residents. CAPTIVE (E within `interactR`) → FOLLOW → rescued on bank ≤ `saveR` from the
//! player, or CAUGHT (a hunter within `catchR`, not in a pool) → sinks `caughtT` → CAPTIVE again in its cell
//! (never lost). Follow AI every `tick` s: BFS toward the player (pools not blocked), `speed` (`fastSpeed`
//! beyond `fastDist`), stops at `stopDist`, snaps next to the player beyond `teleportDist`, wall-sliding with
//! `radius`. Hub residents stand at their building anchor + 1 x and turn toward the player within `hubLookR`.
//!
//! Every state change returns the events `npc.js` emitted (`npcFreed`, `npcCaught` + `npcLost`, `npcRescued`,
//! `npcTalk`, `uiClick`, `uiError`, `toast`); the shell owns meshes, menus and `ctx.state`.

use crate::contracts::Offer;
use crate::events::SimEvent;
use crate::grid::{self, center, dist2d, in_bounds, is_solid, los, to_cell};
use crate::player::{FollowerView, PlayerView};
use crate::save::SaveData;
use std::collections::BTreeMap;
use undercroft_data::config::{Cfg, NpcCfg};
use undercroft_data::tables::NpcDef;
use undercroft_data::zone::EntryKind;
use undercroft_data::{GameData, ParsedMap, ZoneDef};

/// `npc.js` record `state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcState {
    Idle,
    Captive,
    Follow,
    Caught,
    Rescued,
    Hub,
}

/// `npc.js` record `where`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Zone,
    Hub,
}

/// One NPC (`npc.js:record(id)`): position, state and the follower's path.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcRecord {
    pub id: String,
    pub name: String,
    pub short: String,
    pub state: NpcState,
    pub place: Option<Place>,
    pub x: f32,
    pub z: f32,
    /// Bob / sink height (`r.y`).
    pub y: f32,
    pub yaw: f32,
    /// The captive's cell in the zone.
    pub cell: Option<(i32, i32)>,
    /// World waypoints from the last repath (`grid::path_to`).
    pub path: Vec<(f32, f32)>,
    pub path_t: f32,
    pub sink_t: f32,
    pub lit: bool,
    pub moving: bool,
    pub in_pool: bool,
    /// The hunter that caught it last.
    pub hunter_id: Option<u32>,
    /// Idle animation phase (`Math.random() * 6.28` in the JS; deterministic here).
    pub phase: f32,
    /// Hub residents: the yaw they return to (facing the flame).
    pub home_yaw: f32,
}

/// What the follower needs to know about a hunter to be caught by it (`npc.js:checkCatch`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HunterTouch {
    pub id: u32,
    pub x: f32,
    pub z: f32,
    pub active: bool,
    /// `h.state === 'STAGGERED'` — a staggered hunter cannot catch.
    pub staggered: bool,
}

/// `npc.js:interactTarget` — the NPC in reach and what E would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcInteract {
    pub id: String,
    /// `"[E] Free Wick"` / `"[E] Talk to Wick"`.
    pub label: String,
    /// True in a zone (free), false in the hub (talk).
    pub free: bool,
}

/// `npc.js:talk(id)` — the dialogue box contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialogue {
    pub title: String,
    pub lines: Vec<String>,
    pub foot: String,
    pub npc: String,
    /// The contract id accepted with 1 / Enter, if one is offered.
    pub offer: Option<String>,
}

/// `npc.js:talk(id)`'s panel text, without the state change: the NPC's line, then either the contract
/// offer (`contracts::available`) or the titles of this NPC's active contracts. Pure, so the UI lane can
/// redraw an already-open dialogue from the same inputs; [`Npcs::talk`] builds its result with it.
pub fn dialogue(def: &NpcDef, offer: Option<&Offer>, active_titles: &[String]) -> Dialogue {
    let mut lines = vec![format!("\"{}\"", def.line)];
    if let Some(o) = offer {
        lines.push(String::new());
        lines.push(format!("  \"{}\"", o.text));
        lines.push(format!("  {}", o.objective));
        lines.push(format!("  Reward: {}", o.reward_text));
        lines.push(String::new());
        lines.push(format!("[1] Accept contract — {}", o.title));
    } else if !active_titles.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "  \"Come back when it's done.\" — {}",
            active_titles.join(", ")
        ));
    }
    Dialogue {
        title: def.name.clone(),
        lines,
        foot: if offer.is_some() {
            "1 / Enter to accept · Esc to close".to_string()
        } else {
            "Esc to close".to_string()
        },
        npc: def.id.clone(),
        offer: offer.map(|o| o.id.clone()),
    }
}

/// `npc.js:HUB_FALLBACK` — hub stand spots when the hub map has no building anchors (the v1 17×9 hub).
const HUB_FALLBACK: [(&str, (i32, i32)); 4] = [
    ("lamplighter", (6, 2)),
    ("keeper", (10, 2)),
    ("cartographer", (6, 4)),
    ("deacon", (10, 4)),
];

/// Everything `npc.js` keeps between frames.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Npcs {
    recs: BTreeMap<String, NpcRecord>,
    /// `ctx.npcs` — the records currently present, in spawn order.
    present: Vec<String>,
    /// `ctx.player.follower`.
    follower: Option<String>,
    /// Hunter id → time until which that hunter ignores the follower (after a catch).
    busy_until: BTreeMap<u32, f32>,
    /// The HUD line after a catch and when it expires.
    lost_note: (String, f32),
    /// The contract offered in the open dialogue.
    pending_offer: Option<String>,
}

fn face_toward(r: &NpcRecord, tx: f32, tz: f32) -> f32 {
    (-(tx - r.x)).atan2(-(tz - r.z))
}

/// `npc.js:turnToward` — rotate `yaw` toward `target` by at most `rate·dt`.
pub fn turn_toward(yaw: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let mut d = target - yaw;
    while d > std::f32::consts::PI {
        d -= 2.0 * std::f32::consts::PI;
    }
    while d < -std::f32::consts::PI {
        d += 2.0 * std::f32::consts::PI;
    }
    let step = rate * dt;
    if d.abs() <= step {
        yaw + d
    } else {
        yaw + d.signum() * step
    }
}

/// `npc.js:exitName` — "cage" for an elevator zone, else "stairs".
fn exit_name(map: &ParsedMap) -> &'static str {
    match map.stairs.as_ref() {
        Some(s) if s.kind == EntryKind::Elevator => "cage",
        _ => "stairs",
    }
}

/// `npc.js:slide` — axis-separated wall sliding of a body of radius `rr` (pools never block followers).
pub fn slide(m: &ParsedMap, rr: f32, x: &mut f32, z: &mut f32, dx: f32, dz: f32) {
    let blocked = |px: f32, pz: f32| {
        for (ox, oz) in [(-rr, -rr), (rr, -rr), (-rr, rr), (rr, rr)] {
            let (cx, cz) = to_cell(m, px + ox, pz + oz);
            if is_solid(m, cx, cz) {
                return true;
            }
        }
        false
    };
    if dx != 0.0 {
        let nx = *x + dx;
        if !blocked(nx, *z) {
            *x = nx;
        }
    }
    if dz != 0.0 {
        let nz = *z + dz;
        if !blocked(*x, nz) {
            *z = nz;
        }
    }
}

/// `npc.js:stepToward` — move up to `speed·dt` toward the target with wall sliding, face it; returns the
/// distance left.
fn step_toward(
    r: &mut NpcRecord,
    m: &ParsedMap,
    rr: f32,
    tx: f32,
    tz: f32,
    speed: f32,
    dt: f32,
) -> f32 {
    let dx = tx - r.x;
    let dz = tz - r.z;
    let d = dx.hypot(dz);
    if d < 1e-4 {
        return 0.0;
    }
    let step = (speed * dt).min(d);
    slide(m, rr, &mut r.x, &mut r.z, dx / d * step, dz / d * step);
    r.yaw = (-dx).atan2(-dz);
    d - step
}

/// `npc.js:repath` — BFS from the follower's cell to the player's over solid cells only.
fn repath(r: &mut NpcRecord, m: &ParsedMap, px: f32, pz: f32) {
    let (fcx, fcz) = to_cell(m, r.x, r.z);
    let (pcx, pcz) = to_cell(m, px, pz);
    r.path.clear();
    if !in_bounds(m, fcx, fcz) || !in_bounds(m, pcx, pcz) {
        return;
    }
    let field = grid::bfs_solid(m, fcx, fcz);
    let ti = grid::idx(m, pcx, pcz);
    if field.reachable(ti) {
        r.path = grid::path_to(m, &field, ti);
    }
}

/// `npc.js:snapCell` — a free cell next to the player's, preferring the one behind them, else their own cell.
pub fn snap_cell(m: &ParsedMap, px: f32, pz: f32, yaw: f32) -> (f32, f32) {
    let (pcx, pcz) = to_cell(m, px, pz);
    let bx = yaw.sin().round() as i32;
    let bz = yaw.cos().round() as i32;
    for (dx, dz) in [(bx, bz), (1, 0), (-1, 0), (0, 1), (0, -1)] {
        if dx == 0 && dz == 0 {
            continue;
        }
        let cx = pcx + dx;
        let cz = pcz + dz;
        if in_bounds(m, cx, cz) && !is_solid(m, cx, cz) {
            return center(m, cx, cz);
        }
    }
    center(m, pcx, pcz)
}

/// `npc.js:hubSpot(id)` — the resident's stand spot: its building anchor + 1 x when that cell is free, else the
/// anchor; without anchors (the v1 hub) `HUB_FALLBACK`, else two cells south of the flame.
pub fn hub_spot(data: &GameData, m: &ParsedMap, id: &str) -> (f32, f32) {
    let def = data.npcs.npcs.get(id);
    let anchor = def
        .and_then(|d| d.anchor.parse::<u8>().ok())
        .and_then(|digit| m.anchors.get(&digit));
    if let Some(a) = anchor {
        let cx = if in_bounds(m, a.cx + 1, a.cz) && !is_solid(m, a.cx + 1, a.cz) {
            a.cx + 1
        } else {
            a.cx
        };
        return center(m, cx, a.cz);
    }
    let fb = HUB_FALLBACK
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, c)| *c)
        .or_else(|| m.flame.map(|f| (f.cx, f.cz + 2)))
        .unwrap_or((0, 0));
    center(m, fb.0, fb.1)
}

impl Npcs {
    /// No NPCs anywhere.
    pub fn new() -> Npcs {
        Npcs::default()
    }

    /// `npc.js:record(id)` — the record, created idle at the origin on first use.
    fn record(&mut self, data: &GameData, id: &str) -> Option<&mut NpcRecord> {
        if !self.recs.contains_key(id) {
            let def = data.npcs.npcs.get(id)?;
            let phase = (self.recs.len() as f32) * 1.7 % std::f32::consts::TAU;
            self.recs.insert(
                id.to_string(),
                NpcRecord {
                    id: id.to_string(),
                    name: def.name.clone(),
                    short: def.short.clone(),
                    state: NpcState::Idle,
                    place: None,
                    x: 0.0,
                    z: 0.0,
                    y: 0.0,
                    yaw: 0.0,
                    cell: None,
                    path: Vec::new(),
                    path_t: 0.0,
                    sink_t: 0.0,
                    lit: false,
                    moving: false,
                    in_pool: false,
                    hunter_id: None,
                    phase,
                    home_yaw: 0.0,
                },
            );
        }
        self.recs.get_mut(id)
    }

    /// `npc.js:get(id)`.
    pub fn get(&self, id: &str) -> Option<&NpcRecord> {
        self.recs.get(id)
    }

    /// `ctx.npcs` — the records present (zone captives / follower, or hub residents), in spawn order.
    pub fn present(&self) -> Vec<&NpcRecord> {
        self.present
            .iter()
            .filter_map(|id| self.recs.get(id))
            .collect()
    }

    /// `npc.js:hideAll` — everything idle, nothing present, no follower.
    pub fn hide_all(&mut self) {
        for r in self.recs.values_mut() {
            r.place = None;
            r.state = NpcState::Idle;
            r.path.clear();
            r.sink_t = 0.0;
            r.y = 0.0;
        }
        self.present.clear();
        self.follower = None;
    }

    /// `npc.js:placeCaptive` — stand in the cell, CAPTIVE.
    fn place_captive(r: &mut NpcRecord, m: &ParsedMap) {
        let (cx, cz) = r.cell.unwrap_or((0, 0));
        let (x, z) = center(m, cx, cz);
        r.x = x;
        r.z = z;
        r.y = 0.0;
        r.yaw = 0.0;
        r.state = NpcState::Captive;
        r.place = Some(Place::Zone);
        r.path.clear();
        r.path_t = 0.0;
        r.sink_t = 0.0;
        r.lit = false;
        r.moving = false;
        r.hunter_id = None;
    }

    /// `npc.js` `zoneEnter` listener + `spawnZone`: the zone's captives (`ZoneDef::npcs`, else the NPC table's
    /// `zone`/`cell`) that are not yet rescued stand in their cells; catch timers and the lost note clear.
    pub fn spawn_zone(&mut self, data: &GameData, zone: &ZoneDef, m: &ParsedMap, save: &SaveData) {
        self.busy_until.clear();
        self.lost_note.1 = 0.0;
        self.hide_all();
        let ids: Vec<(String, (i32, i32))> = if !zone.npcs.is_empty() {
            zone.npcs
                .iter()
                .map(|(id, c)| (id.clone(), (c[0], c[1])))
                .collect()
        } else {
            data.npcs
                .order
                .iter()
                .filter_map(|id| data.npcs.npcs.get(id))
                .filter(|d| d.zone == zone.id)
                .map(|d| (d.id.clone(), (d.cell[0], d.cell[1])))
                .collect()
        };
        for (id, cell) in ids {
            if !data.npcs.npcs.contains_key(&id) || save.rescued.get(&id) {
                continue;
            }
            if let Some(r) = self.record(data, &id) {
                r.cell = Some(cell);
                Npcs::place_captive(r, m);
                self.present.push(id);
            }
        }
    }

    /// `npc.js:free(id)` — CAPTIVE → FOLLOW (E on a captive). One follower at a time ("You cannot shepherd two").
    pub fn free(
        &mut self,
        data: &GameData,
        m: &ParsedMap,
        id: &str,
        in_zone_mode: bool,
    ) -> (bool, Vec<SimEvent>) {
        let ok = match self.recs.get(id) {
            Some(r) => r.place == Some(Place::Zone) && r.state == NpcState::Captive && in_zone_mode,
            None => false,
        };
        if !ok {
            return (false, vec![]);
        }
        if let Some(f) = self.follower.as_deref() {
            if f != id && self.recs.get(f).map(|r| r.state) == Some(NpcState::Follow) {
                return (
                    false,
                    vec![
                        SimEvent::toast("You cannot shepherd two"),
                        SimEvent::ui_error(),
                    ],
                );
            }
        }
        let pronoun = data
            .npcs
            .npcs
            .get(id)
            .map(|d| d.pronoun.clone())
            .unwrap_or_else(|| "them".into());
        let exit = exit_name(m);
        let r = match self.recs.get_mut(id) {
            Some(r) => r,
            None => return (false, vec![]),
        };
        r.state = NpcState::Follow;
        r.path.clear();
        r.path_t = 0.0;
        r.hunter_id = None;
        let (x, z, short) = (r.x, r.z, r.short.clone());
        self.follower = Some(id.to_string());
        (
            true,
            vec![
                SimEvent::NpcFreed {
                    id: id.to_string(),
                    x,
                    z,
                },
                SimEvent::toast(format!(
                    "{short} follows you. Bring {pronoun} to the {exit}."
                )),
            ],
        )
    }

    /// `npc.js:rescue` — `save.rescued[id] = true`, `stats.rescues++`, RESCUED, `npcRescued {id, zoneId}`.
    fn rescue(&mut self, save: &mut SaveData, id: &str, zone_id: &str) -> Vec<SimEvent> {
        save.rescued.set(id, true);
        save.stats.rescues += 1;
        let name = match self.recs.get_mut(id) {
            Some(r) => {
                r.state = NpcState::Rescued;
                r.name.clone()
            }
            None => id.to_string(),
        };
        self.follower = None;
        vec![
            SimEvent::NpcRescued {
                id: id.to_string(),
                zone_id: zone_id.to_string(),
                debug: false,
            },
            SimEvent::toast(format!("{name} is safe at the Lantern.")),
        ]
    }

    /// `npc.js` `bank` listener — a FOLLOWing NPC within `saveR` of the player is rescued.
    pub fn on_bank(
        &mut self,
        cfg: &NpcCfg,
        save: &mut SaveData,
        player: (f32, f32),
        zone_id: &str,
    ) -> Vec<SimEvent> {
        let id = match self.follower.clone() {
            Some(id) => id,
            None => return vec![],
        };
        let near = match self.recs.get(&id) {
            Some(r) => {
                r.state == NpcState::Follow && dist2d(r.x, r.z, player.0, player.1) <= cfg.save_r
            }
            None => false,
        };
        if near {
            self.rescue(save, &id, zone_id)
        } else {
            vec![]
        }
    }

    /// `main.js:actions.rescueNpc(id)` — the debug rescue: marks the save, emits `npcRescued {debug: true}`.
    pub fn rescue_debug(&mut self, save: &mut SaveData, id: &str, zone_id: &str) -> Vec<SimEvent> {
        let mut ev = self.rescue(save, id, zone_id);
        if let Some(SimEvent::NpcRescued { debug, .. }) = ev.first_mut() {
            *debug = true;
        }
        self.on_rescued(id);
        ev
    }

    /// `npc.js` `npcRescued` listener — drop the zone record (a rescue by any path). The shell re-places the hub
    /// residents ([`Npcs::place_hub`]) when the hub is showing.
    pub fn on_rescued(&mut self, id: &str) {
        if let Some(r) = self.recs.get_mut(id) {
            if r.place == Some(Place::Zone) && r.state != NpcState::Rescued {
                r.state = NpcState::Rescued;
                self.present.retain(|p| p != id);
                if self.follower.as_deref() == Some(id) {
                    self.follower = None;
                }
            }
        }
    }

    /// `npc.js:catchFollower(r, h)` — a hunter touched the follower: CAUGHT, sinks for `caughtT`, then stands in
    /// its cell again. The hunter is busy for `hunterBusyT` ([`Npcs::hunter_busy`]; the creature lane should also
    /// set `h.busyT` and clear its path). Emits `npcCaught` + `npcLost` and the toast.
    pub fn caught(
        &mut self,
        data: &GameData,
        cfg: &NpcCfg,
        id: Option<&str>,
        hunter: Option<u32>,
        time: f32,
    ) -> (bool, Vec<SimEvent>) {
        let id = match id.map(String::from).or_else(|| self.follower.clone()) {
            Some(id) => id,
            None => return (false, vec![]),
        };
        let pronoun = data
            .npcs
            .npcs
            .get(&id)
            .map(|d| if d.pronoun == "her" { "her" } else { "his" })
            .unwrap_or("his");
        let r = match self.recs.get_mut(&id) {
            Some(r) if r.state == NpcState::Follow => r,
            _ => return (false, vec![]),
        };
        r.state = NpcState::Caught;
        r.sink_t = cfg.caught_t;
        r.path.clear();
        r.moving = false;
        r.lit = false;
        r.hunter_id = hunter;
        let (x, z, short) = (r.x, r.z, r.short.clone());
        self.follower = None;
        if let Some(h) = hunter {
            self.busy_until.insert(h, time + cfg.hunter_busy_t);
        }
        self.lost_note = (
            format!("{short} was taken — back in {pronoun} cell"),
            time + 4.0,
        );
        (
            true,
            vec![
                SimEvent::NpcCaught {
                    id: id.clone(),
                    x,
                    z,
                    hunter_id: hunter,
                },
                SimEvent::NpcLost {
                    id,
                    x,
                    z,
                    hunter_id: hunter,
                },
                SimEvent::toast(format!("{short} was dragged back into the dark.")),
            ],
        )
    }

    /// `npc.js` `hunterCatch` listener — for `target: 'npc'` events; `npc_id` when the payload names one.
    pub fn on_hunter_catch(
        &mut self,
        data: &GameData,
        cfg: &NpcCfg,
        npc_id: Option<&str>,
        hunter: Option<u32>,
        time: f32,
    ) -> Vec<SimEvent> {
        self.caught(data, cfg, npc_id, hunter, time).1
    }

    /// Is this hunter still ignoring the follower after a catch (`busyUntil`)?
    pub fn hunter_busy(&self, hunter: u32, time: f32) -> bool {
        self.busy_until.get(&hunter).copied().unwrap_or(0.0) > time
    }

    /// `npc.js` `death` listener — the follower stops.
    pub fn on_death(&mut self) {
        if let Some(r) = self.follower.as_ref().and_then(|id| self.recs.get_mut(id)) {
            r.moving = false;
            r.path.clear();
        }
    }

    /// `npc.js` `gateOpened` / `shortcutOpened` listeners — repath now.
    pub fn on_door_opened(&mut self) {
        if let Some(r) = self.follower.as_ref().and_then(|id| self.recs.get_mut(id)) {
            r.path.clear();
            r.path_t = 0.0;
        }
    }

    /// `npc.js` `zoneExit` listener.
    pub fn on_zone_exit(&mut self) {
        self.hide_all();
    }

    /// `npc.js:updateFollower` for one record.
    #[allow(clippy::too_many_arguments)]
    fn update_follower(
        r: &mut NpcRecord,
        cfg: &NpcCfg,
        pool_r: f32,
        dt: f32,
        p: &PlayerView,
        m: &ParsedMap,
        lanterns: &[(f32, f32)],
    ) {
        let d = dist2d(r.x, r.z, p.x, p.z);
        r.lit = p.lamp_on && d <= cfg.lit_r;
        r.in_pool = lanterns
            .iter()
            .any(|&(lx, lz)| dist2d(lx, lz, r.x, r.z) <= pool_r);
        if d > cfg.teleport_dist {
            let (x, z) = snap_cell(m, p.x, p.z, p.yaw);
            r.x = x;
            r.z = z;
            r.path.clear();
            r.path_t = 0.0;
            r.moving = false;
            return;
        }
        if d <= cfg.stop_dist {
            r.moving = false;
            r.path.clear();
            r.yaw = turn_toward(r.yaw, face_toward(r, p.x, p.z), 4.0, dt);
            return;
        }
        r.path_t -= dt;
        if r.path_t <= 0.0 {
            r.path_t = cfg.tick;
            repath(r, m, p.x, p.z);
        }
        let speed = if d > cfg.fast_dist {
            cfg.fast_speed
        } else {
            cfg.speed
        };
        r.moving = true;
        if d <= 4.0 && los(m, r.x, r.z, p.x, p.z) {
            step_toward(r, m, cfg.radius, p.x, p.z, speed, dt);
            return;
        }
        if let Some(&(nx, nz)) = r.path.first() {
            if step_toward(r, m, cfg.radius, nx, nz, speed, dt) < 0.15 {
                r.path.remove(0);
            }
        } else {
            step_toward(r, m, cfg.radius, p.x, p.z, speed, dt);
        }
    }

    /// `npc.js:checkCatch` — the first active, unstaggered, not-busy hunter within `catchR` catches the
    /// follower (never while it stands in a lantern pool).
    fn check_catch(
        &mut self,
        data: &GameData,
        cfg: &NpcCfg,
        id: &str,
        hunters: &[HunterTouch],
        time: f32,
    ) -> Vec<SimEvent> {
        let (x, z) = match self.recs.get(id) {
            Some(r) if r.state == NpcState::Follow && !r.in_pool => (r.x, r.z),
            _ => return vec![],
        };
        for h in hunters {
            if !h.active || h.staggered || self.hunter_busy(h.id, time) {
                continue;
            }
            if dist2d(h.x, h.z, x, z) <= cfg.catch_r {
                return self.caught(data, cfg, Some(id), Some(h.id), time).1;
            }
        }
        vec![]
    }

    /// `npc.js:updateZone(dt)` — captives turn toward a near player and bob; the follower follows and may be
    /// caught; a caught NPC sinks and re-appears in its cell. `lanterns` = planted lantern positions,
    /// `hunters` = the hunters as [`HunterTouch`]. Returns the catch events.
    #[allow(clippy::too_many_arguments)]
    pub fn update_zone(
        &mut self,
        data: &GameData,
        cfg: &NpcCfg,
        player_cfg: &Cfg,
        dt: f32,
        time: f32,
        p: &PlayerView,
        m: &ParsedMap,
        lanterns: &[(f32, f32)],
        hunters: &[HunterTouch],
    ) -> Vec<SimEvent> {
        let mut out = Vec::new();
        for id in self.present.clone() {
            let r = match self.recs.get_mut(&id) {
                Some(r) if r.place == Some(Place::Zone) => r,
                _ => continue,
            };
            match r.state {
                NpcState::Captive => {
                    let d = dist2d(r.x, r.z, p.x, p.z);
                    if d <= 8.0 {
                        r.yaw = turn_toward(r.yaw, face_toward(r, p.x, p.z), 2.5, dt);
                    }
                    r.y = 0.015 * (time * 2.0 + r.phase).sin();
                }
                NpcState::Follow => {
                    Npcs::update_follower(r, cfg, player_cfg.pool_r, dt, p, m, lanterns);
                    r.y = if r.moving {
                        0.04 * (time * 9.0 + r.phase).sin().abs()
                    } else {
                        0.015 * (time * 2.0 + r.phase).sin()
                    };
                    out.extend(self.check_catch(data, cfg, &id, hunters, time));
                }
                NpcState::Caught => {
                    r.sink_t -= dt;
                    r.y = -1.6 * (1.0 - r.sink_t.max(0.0) / cfg.caught_t);
                    if r.sink_t <= 0.0 {
                        Npcs::place_captive(r, m);
                    }
                }
                _ => {}
            }
            if let Some(r) = self.recs.get_mut(&id) {
                if !r.x.is_finite() || !r.z.is_finite() {
                    Npcs::place_captive(r, m);
                }
            }
        }
        out
    }

    /// `npc.js:updateHud` — "◆ Wick follows (lit)" / "◆ Wick was taken — back in his cell" / "" (only in ZONE or
    /// MENU mode).
    pub fn hud_line(&self, time: f32, zone_or_menu: bool) -> String {
        if !zone_or_menu {
            return String::new();
        }
        if let Some(f) = self.follower() {
            let tag = if f.in_pool {
                " (safe)"
            } else if f.lit {
                " (lit)"
            } else {
                ""
            };
            return format!("◆ {} follows{tag}", f.short);
        }
        if self.lost_note.1 > time {
            return format!("◆ {}", self.lost_note.0);
        }
        String::new()
    }

    /* ---------------- Hub ---------------- */

    /// `npc.js:placeHub` — every rescued NPC stands at its hub spot facing the flame.
    pub fn place_hub(&mut self, data: &GameData, m: &ParsedMap, save: &SaveData) {
        self.hide_all();
        let flame = m.flame.map(|f| center(m, f.cx, f.cz));
        for id in data.npcs.order.clone() {
            if !save.rescued.get(&id) {
                continue;
            }
            let (sx, sz) = hub_spot(data, m, &id);
            if let Some(r) = self.record(data, &id) {
                r.x = sx;
                r.z = sz;
                r.y = 0.0;
                r.state = NpcState::Hub;
                r.place = Some(Place::Hub);
                r.path.clear();
                r.moving = false;
                r.lit = false;
                r.yaw = flame.map(|(fx, fz)| face_toward(r, fx, fz)).unwrap_or(0.0);
                r.home_yaw = r.yaw;
                self.present.push(id);
            }
        }
    }

    /// `npc.js:updateHub(dt)` — residents turn toward the player within `hubLookR`, else home; bob.
    pub fn update_hub(&mut self, cfg: &NpcCfg, dt: f32, time: f32, p: &PlayerView) {
        for id in self.present.clone() {
            if let Some(r) = self.recs.get_mut(&id) {
                if r.place != Some(Place::Hub) {
                    continue;
                }
                let d = dist2d(r.x, r.z, p.x, p.z);
                let target = if d <= cfg.hub_look_r {
                    face_toward(r, p.x, p.z)
                } else {
                    r.home_yaw
                };
                r.yaw = turn_toward(r.yaw, target, 2.5, dt);
                r.y = 0.02 * (time * 1.8 + r.phase).sin();
            }
        }
    }

    /// `npc.js:talk(id)` — the one-line dialogue plus the contract offer (`offer` = `contracts::available`)
    /// or, with none, the titles of this NPC's active contracts ("Come back when it's done."). Emits `npcTalk`
    /// and `uiClick`; `None` when the NPC is not standing in the hub.
    pub fn talk(
        &mut self,
        def: &NpcDef,
        offer: Option<&Offer>,
        active_titles: &[String],
    ) -> Option<(Dialogue, Vec<SimEvent>)> {
        let r = self.recs.get(&def.id)?;
        if r.place != Some(Place::Hub) {
            return None;
        }
        let dlg = dialogue(def, offer, active_titles);
        self.pending_offer = dlg.offer.clone();
        Some((
            dlg,
            vec![SimEvent::NpcTalk { id: def.id.clone() }, SimEvent::UiClick],
        ))
    }

    /// `npc.js:onKey` — in the dialogue, 1 / Enter / Space accepts the pending offer: returns the contract id
    /// to pass to `contracts::accept` (the shell then closes the menu; a refused accept toasts "You already
    /// carry enough contracts." + `uiError`).
    pub fn on_key(&mut self, code: &str, in_dialog_menu: bool) -> Option<String> {
        if !in_dialog_menu {
            return None;
        }
        if !matches!(code, "Digit1" | "Enter" | "Space") {
            return None;
        }
        self.pending_offer.take()
    }

    /// `npc.js` `menuClose` listener.
    pub fn on_menu_close(&mut self) {
        self.pending_offer = None;
    }

    /// The contract offered in the open dialogue.
    pub fn pending_offer(&self) -> Option<&str> {
        self.pending_offer.as_deref()
    }

    /* ---------------- Queries ---------------- */

    /// `npc.js:follower()` — the NPC following the player, if it is in FOLLOW.
    pub fn follower(&self) -> Option<&NpcRecord> {
        self.follower
            .as_ref()
            .and_then(|id| self.recs.get(id))
            .filter(|r| r.state == NpcState::Follow)
    }

    /// `npc.js:stimulus()` — what hunters may sense of the follower.
    pub fn stimulus(&self) -> Option<FollowerView> {
        self.follower().map(|f| FollowerView {
            id: f.id.clone(),
            x: f.x,
            z: f.z,
            moving: f.moving,
            lit: f.lit,
            in_pool: f.in_pool,
        })
    }

    /// `npc.js:atHub()` — `(id, x, z)` of the residents standing in the hub.
    pub fn at_hub(&self) -> Vec<(String, f32, f32)> {
        self.present()
            .into_iter()
            .filter(|r| r.place == Some(Place::Hub))
            .map(|r| (r.id.clone(), r.x, r.z))
            .collect()
    }

    /// `npc.js:captives()` — ids standing captive in the current zone.
    pub fn captives(&self) -> Vec<String> {
        self.present()
            .into_iter()
            .filter(|r| r.place == Some(Place::Zone) && r.state == NpcState::Captive)
            .map(|r| r.id.clone())
            .collect()
    }

    /// `npc.js:interactTarget` — the nearest captive (ZONE) or resident (HUB) within `interactR`.
    pub fn interact_target(
        &self,
        cfg: &NpcCfg,
        in_zone: bool,
        in_hub: bool,
        p: &PlayerView,
    ) -> Option<NpcInteract> {
        if !in_zone && !in_hub {
            return None;
        }
        let mut best: Option<&NpcRecord> = None;
        let mut bd = cfg.interact_r;
        for r in self.present() {
            let ok = (in_zone && r.place == Some(Place::Zone) && r.state == NpcState::Captive)
                || (in_hub && r.place == Some(Place::Hub));
            if !ok {
                continue;
            }
            let d = dist2d(r.x, r.z, p.x, p.z);
            if d <= bd {
                best = Some(r);
                bd = d;
            }
        }
        let b = best?;
        Some(if in_zone {
            NpcInteract {
                id: b.id.clone(),
                label: format!("[E] Free {}", b.short),
                free: true,
            }
        } else {
            NpcInteract {
                id: b.id.clone(),
                label: format!("[E] Talk to {}", b.short),
                free: false,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    fn names(ev: &[SimEvent]) -> Vec<&'static str> {
        ev.iter().map(SimEvent::name).collect()
    }

    fn undercroft(d: &GameData) -> (ZoneDef, ParsedMap) {
        let z = d.zone("undercroft").unwrap().clone();
        let m = d.parse_zone("undercroft").unwrap().unwrap();
        (z, m)
    }

    #[test]
    fn captives_stand_in_their_cells() {
        let d = data();
        let (z, m) = undercroft(&d);
        let mut n = Npcs::new();
        let save = SaveData::default();
        n.spawn_zone(&d, &z, &m, &save);
        let mut caps = n.captives();
        caps.sort();
        assert_eq!(caps, vec!["deacon", "lamplighter"]);
        let w = n.get("lamplighter").unwrap();
        assert_eq!(w.cell, Some((4, 47)));
        assert_eq!((w.x, w.z), (4.5, 47.5));
        assert_eq!(w.state, NpcState::Captive);
        // every N cell of the map holds a captive
        for c in &m.npc_cells {
            assert!(
                n.present().iter().any(|r| r.cell == Some((c.cx, c.cz))),
                "N at ({}, {})",
                c.cx,
                c.cz
            );
        }
        // rescued NPCs no longer spawn
        let mut s2 = SaveData::default();
        s2.rescued.deacon = true;
        n.spawn_zone(&d, &z, &m, &s2);
        assert_eq!(n.captives(), vec!["lamplighter"]);
        // interact target
        let cfg = &d.config.npc_cfg;
        let p = PlayerView {
            x: 5.5,
            z: 47.5,
            ..Default::default()
        };
        let t = n.interact_target(cfg, true, false, &p).expect("in reach");
        assert_eq!(
            t,
            NpcInteract {
                id: "lamplighter".into(),
                label: "[E] Free Wick".into(),
                free: true
            }
        );
        let far = PlayerView {
            x: 7.5,
            z: 47.5,
            ..Default::default()
        };
        assert!(n.interact_target(cfg, true, false, &far).is_none());
        assert!(n.interact_target(cfg, false, false, &p).is_none());
    }

    #[test]
    fn free_follow_rescue() {
        let d = data();
        let (z, m) = undercroft(&d);
        let cfg = &d.config.npc_cfg;
        let mut n = Npcs::new();
        let mut save = SaveData::default();
        n.spawn_zone(&d, &z, &m, &save);
        assert!(!n.free(&d, &m, "lamplighter", false).0, "not in ZONE mode");
        assert!(!n.free(&d, &m, "keeper", true).0, "not here");
        let (ok, ev) = n.free(&d, &m, "lamplighter", true);
        assert!(ok);
        assert_eq!(
            ev,
            vec![
                SimEvent::NpcFreed {
                    id: "lamplighter".into(),
                    x: 4.5,
                    z: 47.5
                },
                SimEvent::toast("Wick follows you. Bring him to the stairs."),
            ]
        );
        assert_eq!(n.follower().map(|f| f.id.as_str()), Some("lamplighter"));
        let (ok, ev) = n.free(&d, &m, "deacon", true);
        assert!(!ok);
        assert_eq!(
            ev,
            vec![
                SimEvent::toast("You cannot shepherd two"),
                SimEvent::ui_error()
            ]
        );
        assert!(!n.free(&d, &m, "lamplighter", true).0, "already following");
        // stimulus: walking, lamp off, lit within litR of the lit player
        let mut p = PlayerView {
            x: 6.5,
            z: 47.5,
            lamp_on: true,
            ..Default::default()
        };
        let ev = n.update_zone(&d, cfg, &d.config.cfg, 0.1, 1.0, &p, &m, &[], &[]);
        assert!(ev.is_empty());
        let s = n.stimulus().unwrap();
        assert!(s.lit && s.moving && !s.in_pool);
        assert_eq!(n.hud_line(1.0, true), "◆ Wick follows (lit)");
        // a lantern pool makes it safe
        let ev = n.update_zone(
            &d,
            cfg,
            &d.config.cfg,
            0.1,
            1.1,
            &p,
            &m,
            &[(4.5, 47.5)],
            &[],
        );
        assert!(ev.is_empty());
        assert!(n.stimulus().unwrap().in_pool);
        assert_eq!(n.hud_line(1.1, true), "◆ Wick follows (safe)");
        assert_eq!(n.hud_line(1.1, false), "");
        // walks toward the player over a second
        for i in 0..20 {
            n.update_zone(
                &d,
                cfg,
                &d.config.cfg,
                0.05,
                1.2 + i as f32 * 0.05,
                &p,
                &m,
                &[],
                &[],
            );
        }
        let f = n.follower().unwrap();
        assert!(
            dist2d(f.x, f.z, p.x, p.z) <= cfg.stop_dist + 0.05,
            "stopped near the player: {} {}",
            f.x,
            f.z
        );
        assert!(!f.moving);
        // bank too far: no rescue; bank near: rescued
        let far = (p.x + 10.0, p.z);
        assert!(n.on_bank(cfg, &mut save, far, "undercroft").is_empty());
        let ev = n.on_bank(cfg, &mut save, (p.x, p.z), "undercroft");
        assert_eq!(
            ev,
            vec![
                SimEvent::NpcRescued {
                    id: "lamplighter".into(),
                    zone_id: "undercroft".into(),
                    debug: false
                },
                SimEvent::toast("Wick the Lamplighter is safe at the Lantern."),
            ]
        );
        assert!(save.rescued.lamplighter);
        assert_eq!(save.stats.rescues, 1);
        assert!(n.follower().is_none());
        // as in the JS, the RESCUED record stays listed until zoneExit clears it
        n.on_rescued("lamplighter");
        assert_eq!(n.get("lamplighter").unwrap().state, NpcState::Rescued);
        n.on_zone_exit();
        assert!(n.present().is_empty());
        // a rescue by the debug path drops a zone captive at once
        n.spawn_zone(&d, &z, &m, &save);
        assert_eq!(n.captives(), vec!["deacon"]);
        n.on_rescued("deacon");
        assert!(n.captives().is_empty());
        // the elevator zones say "cage"
        let oz = d.zone("ossuary").unwrap();
        let om = d.parse_zone("ossuary").unwrap().unwrap();
        n.spawn_zone(&d, oz, &om, &save);
        let (_, ev) = n.free(&d, &om, "keeper", true);
        assert_eq!(
            ev[1],
            SimEvent::toast("Oren follows you. Bring him to the cage.")
        );
        p.x = 0.0;
        p.z = 0.0;
        assert!(
            n.on_bank(cfg, &mut save, (0.0, 0.0), "ossuary").is_empty(),
            "keeper is far from (0,0)"
        );
    }

    #[test]
    fn caught_sinks_and_returns_to_the_cell() {
        let d = data();
        let (z, m) = undercroft(&d);
        let cfg = &d.config.npc_cfg;
        let mut n = Npcs::new();
        let save = SaveData::default();
        n.spawn_zone(&d, &z, &m, &save);
        n.free(&d, &m, "deacon", true);
        let p = PlayerView {
            x: 6.5,
            z: 5.5,
            lamp_on: false,
            ..Default::default()
        };
        let f = n.follower().unwrap();
        let hunters = [
            HunterTouch {
                id: 0,
                x: f.x + 0.5,
                z: f.z,
                active: true,
                staggered: true,
            },
            HunterTouch {
                id: 1,
                x: f.x + 0.5,
                z: f.z,
                active: false,
                staggered: false,
            },
            HunterTouch {
                id: 2,
                x: f.x + 0.5,
                z: f.z,
                active: true,
                staggered: false,
            },
        ];
        let ev = n.update_zone(&d, cfg, &d.config.cfg, 0.05, 10.0, &p, &m, &[], &hunters);
        assert_eq!(names(&ev), vec!["npcCaught", "npcLost", "toast"]);
        assert!(
            matches!(&ev[0], SimEvent::NpcCaught { id, hunter_id: Some(2), .. } if id == "deacon")
        );
        assert_eq!(
            ev[2],
            SimEvent::toast("Maud was dragged back into the dark.")
        );
        assert!(n.follower().is_none());
        assert!(n.hunter_busy(2, 11.0));
        assert!(!n.hunter_busy(2, 12.5));
        assert!(!n.hunter_busy(0, 10.0));
        assert_eq!(
            n.hud_line(12.0, true),
            "◆ Maud was taken — back in her cell"
        );
        assert_eq!(n.hud_line(15.0, true), "");
        let r = n.get("deacon").unwrap();
        assert_eq!(r.state, NpcState::Caught);
        assert_eq!(r.sink_t, cfg.caught_t);
        // sinks over caughtT, then stands in the cell again
        n.update_zone(&d, cfg, &d.config.cfg, 0.5, 10.5, &p, &m, &[], &hunters);
        let r = n.get("deacon").unwrap();
        assert!((r.y + 0.8).abs() < 1e-5);
        n.update_zone(&d, cfg, &d.config.cfg, 0.6, 11.1, &p, &m, &[], &hunters);
        let r = n.get("deacon").unwrap();
        assert_eq!(r.state, NpcState::Captive);
        assert_eq!((r.x, r.z), (4.5, 5.5));
        assert_eq!(r.y, 0.0);
        // in a pool it cannot be caught; a busy hunter cannot catch either
        n.free(&d, &m, "deacon", true);
        let ev = n.update_zone(
            &d,
            cfg,
            &d.config.cfg,
            0.05,
            20.0,
            &p,
            &m,
            &[(4.5, 5.5)],
            &hunters[2..],
        );
        assert!(ev.is_empty());
        let ev = n.update_zone(
            &d,
            cfg,
            &d.config.cfg,
            0.05,
            20.1,
            &p,
            &m,
            &[],
            &hunters[2..],
        );
        assert_eq!(names(&ev), vec!["npcCaught", "npcLost", "toast"]);
        // the external hook: only a FOLLOWing NPC can be caught
        assert!(!n.caught(&d, cfg, Some("deacon"), None, 21.0).0);
        assert!(!n.caught(&d, cfg, None, None, 21.0).0);
        n.on_death();
        n.on_door_opened();
        n.on_zone_exit();
        assert!(n.present().is_empty());
    }

    #[test]
    fn teleport_when_stuck_and_wall_slide() {
        let d = data();
        let (z, m) = undercroft(&d);
        let cfg = &d.config.npc_cfg;
        let mut n = Npcs::new();
        n.spawn_zone(&d, &z, &m, &SaveData::default());
        n.free(&d, &m, "lamplighter", true);
        // far beyond teleportDist: snaps next to the player (behind, if free)
        let entry = z.anchor("entry").unwrap();
        let (ex, ez) = center(&m, entry[0], entry[1]);
        let p = PlayerView {
            x: ex,
            z: ez,
            yaw: 0.0,
            ..Default::default()
        };
        n.update_zone(&d, cfg, &d.config.cfg, 0.05, 0.0, &p, &m, &[], &[]);
        let f = n.follower().unwrap();
        assert!(dist2d(f.x, f.z, ex, ez) <= 1.01, "snapped: {} {}", f.x, f.z);
        assert_eq!((f.x, f.z), snap_cell(&m, ex, ez, 0.0));
        let (cx, cz) = to_cell(&m, f.x, f.z);
        assert!(!is_solid(&m, cx, cz));
        // sliding: a step into a wall is refused per axis, the free axis still moves
        let (wx, wz) = center(&m, 1, 47); // the west wing's west wall is at x = 0
        assert!(is_solid(&m, 0, 47) && !is_solid(&m, 1, 47));
        let (mut x, mut z) = (wx, wz);
        slide(&m, cfg.radius, &mut x, &mut z, -1.0, 0.0);
        assert_eq!((x, z), (wx, wz));
        let free_z = if is_solid(&m, 1, 48) { -0.2 } else { 0.2 };
        let (mut x, mut z) = (wx, wz);
        slide(&m, cfg.radius, &mut x, &mut z, -1.0, free_z);
        assert_eq!(x, wx);
        assert!((z - (wz + free_z)).abs() < 1e-6);
        // turning
        assert!((turn_toward(0.0, 1.0, 10.0, 0.05) - 0.5).abs() < 1e-6);
        assert!((turn_toward(0.0, 0.2, 10.0, 0.05) - 0.2).abs() < 1e-6);
        assert!(turn_toward(3.0, -3.0, 1.0, 0.1) > 3.0, "shortest way round");
    }

    #[test]
    fn hub_residents_and_dialogue() {
        let d = data();
        let hub = d.parse_hub().unwrap();
        let cfg = &d.config.npc_cfg;
        let mut n = Npcs::new();
        let mut save = SaveData::default();
        n.place_hub(&d, &hub, &save);
        assert!(n.at_hub().is_empty());
        save.rescued.lamplighter = true;
        save.rescued.deacon = true;
        n.place_hub(&d, &hub, &save);
        let at = n.at_hub();
        assert_eq!(at.len(), 2);
        assert_eq!(at[0].0, "lamplighter");
        // anchor + 1 x (DESIGN-v2 §7), on a free cell
        for (id, x, z) in &at {
            let def = &d.npcs.npcs[id];
            let a = hub.anchors[&def.anchor.parse::<u8>().unwrap()];
            let (cx, cz) = to_cell(&hub, *x, *z);
            assert_eq!(cz, a.cz);
            assert!(cx == a.cx + 1 || cx == a.cx);
            assert!(!is_solid(&hub, cx, cz));
        }
        assert_eq!(hub_spot(&d, &hub, "lamplighter"), (at[0].1, at[0].2));
        let w = n.get("lamplighter").unwrap();
        assert_eq!(w.state, NpcState::Hub);
        let fl = hub.flame.unwrap();
        let expect = (-(fl.x - w.x)).atan2(-(fl.z - w.z));
        assert!((w.home_yaw - expect).abs() < 1e-6, "faces the flame");
        // turns toward a near player
        let p = PlayerView {
            x: w.x + 1.0,
            z: w.z + 1.0,
            ..Default::default()
        };
        n.update_hub(cfg, 1.0, 0.0, &p);
        let w = n.get("lamplighter").unwrap();
        assert!((w.yaw - (-1.0f32).atan2(-1.0)).abs() < 1e-5);
        let t = n.interact_target(cfg, false, true, &p).unwrap();
        assert_eq!(
            t,
            NpcInteract {
                id: "lamplighter".into(),
                label: "[E] Talk to Wick".into(),
                free: false
            }
        );
        // v1 hub fallback spots
        let v1 = crate::fixtures::load_map_fixture("hub_v1");
        let v1m = undercroft_data::parse_hub(v1.rows.as_ref().unwrap(), d.config.hub_ox).unwrap();
        assert!(v1m.anchors.is_empty());
        assert_eq!(hub_spot(&d, &v1m, "keeper"), center(&v1m, 10, 2));
        // dialogue with an offer, then accept through the key
        let def = &d.npcs.npcs["lamplighter"];
        let offer = contracts::available(&d, &save, "lamplighter").unwrap();
        let (dlg, ev) = n.talk(def, Some(&offer), &[]).unwrap();
        assert_eq!(names(&ev), vec!["npcTalk", "uiClick"]);
        assert_eq!(dlg.title, "Wick the Lamplighter");
        assert_eq!(
            dlg.lines[0],
            "\"Every lamp I ever lit is out. Let's fix that.\""
        );
        assert_eq!(dlg.lines[2], "  \"Plant a lantern in the great hall, dead centre. Let it see the dark can be pushed.\"");
        assert_eq!(
            dlg.lines[3],
            "  Plant a lantern in the great hall of The Undercroft."
        );
        assert_eq!(dlg.lines[4], "  Reward: Pry Bar, 4 flame points");
        assert_eq!(dlg.lines[6], "[1] Accept contract — Relight the Great Hall");
        assert_eq!(dlg.foot, "1 / Enter to accept · Esc to close");
        assert_eq!(dlg.offer.as_deref(), Some("c_relight"));
        assert_eq!(n.pending_offer(), Some("c_relight"));
        assert!(n.on_key("KeyE", true).is_none());
        assert!(n.on_key("Digit1", false).is_none());
        assert_eq!(n.on_key("Digit1", true).as_deref(), Some("c_relight"));
        assert!(n.on_key("Enter", true).is_none(), "consumed");
        let env = contracts::ContractEnv::hub(&d);
        assert!(contracts::accept(&env, &mut save, "c_relight", false).0);
        // now the NPC has one active: "come back when it's done"
        let active: Vec<String> = save
            .contracts
            .active
            .iter()
            .map(|id| d.contracts.contracts[id].title.clone())
            .collect();
        let (dlg, _) = n
            .talk(
                def,
                contracts::available(&d, &save, "lamplighter").as_ref(),
                &active,
            )
            .unwrap();
        assert_eq!(
            dlg.lines,
            vec![
                "\"Every lamp I ever lit is out. Let's fix that.\"".to_string(),
                String::new(),
                "  \"Come back when it's done.\" — Relight the Great Hall".to_string(),
            ]
        );
        assert_eq!(dlg.foot, "Esc to close");
        assert!(dlg.offer.is_none());
        n.on_menu_close();
        assert!(n.pending_offer().is_none());
        // not in the hub: no dialogue
        assert!(n.talk(&d.npcs.npcs["keeper"], None, &[]).is_none());
        // a debug rescue re-places the residents
        let ev = n.rescue_debug(&mut save, "keeper", "ossuary");
        assert!(matches!(&ev[0], SimEvent::NpcRescued { debug: true, .. }));
        n.place_hub(&d, &hub, &save);
        assert_eq!(n.at_hub().len(), 3);
    }

    /// The pure [`dialogue`] builder produces exactly what [`Npcs::talk`] returns, offer or not, so the UI
    /// lane can redraw an open dialogue without touching `Npcs`.
    #[test]
    fn dialogue_matches_talk() {
        let d = data();
        let hub = d.parse_hub().unwrap();
        let mut save = SaveData::defaults(0.6);
        save.rescued.set("lamplighter", true);
        let mut n = Npcs::default();
        n.place_hub(&d, &hub, &save);
        let def = &d.npcs.npcs["lamplighter"];
        let offer = contracts::available(&d, &save, "lamplighter");
        let pure = dialogue(def, offer.as_ref(), &[]);
        let (via_talk, _) = n.talk(def, offer.as_ref(), &[]).unwrap();
        assert_eq!(pure, via_talk);
        assert_eq!(pure.offer.as_deref(), Some("c_relight"));
        assert_eq!(pure.foot, "1 / Enter to accept · Esc to close");
        // with no offer and no active contract the line stands alone
        let bare = dialogue(def, None, &[]);
        assert_eq!(bare.lines.len(), 1);
        assert_eq!(bare.foot, "Esc to close");
        assert!(bare.offer.is_none());
    }
}
