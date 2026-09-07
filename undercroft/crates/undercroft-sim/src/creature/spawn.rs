//! Records in and out (`hunter.js` ~108–174 `spawnHunter`, `spawnCreature`, `spawnAll`, `reset`, `clear`), the
//! event listeners of `hunter.js:init` (`gateOpened` / `shortcutOpened` → paths dropped, `npcCaught` → the
//! catcher stands over the spot) and the HUD line `hunter.js:hint`.

use super::driver::investigate;
use super::record::{HState, Hunter, ProfileKind, SpawnOpts, Target};
use super::Ctx;
use crate::grid::{center, dist2d, in_bounds, is_solid, los};
use std::f32::consts::TAU;
use undercroft_data::zone::ZoneDef;

/// `hunter.js:makeHunter(id, profile)` — a fresh inactive record with a random animation phase.
pub fn make_hunter(ctx: &mut Ctx, id: u32, profile: ProfileKind) -> Hunter {
    let initial = ctx.prof(profile).initial;
    let phase = ctx.rng.unit() as f32 * TAU;
    Hunter::new(id, profile, initial, phase)
}

/// `hunter.js:reset(h, s)` — place the record at a spawn cell, activate it, restore every timer and run the
/// profile's `onReset`.
pub fn reset(ctx: &mut Ctx, h: &mut Hunter, cx: i32, cz: i32) {
    let (x, z) = center(ctx.map(), cx, cz);
    h.home.cx = cx;
    h.home.cz = cz;
    h.home.x = x;
    h.home.z = z;
    h.x = x;
    h.z = z;
    h.y = 0.0;
    h.yaw = 0.0;
    h.yaw_vis = 0.0;
    h.path.clear();
    h.state = ctx.prof(h.profile).initial;
    h.active = true;
    h.tick_t = 0.0;
    h.repath_t = 0.0;
    h.no_stim_t = 0.0;
    h.idle_t = 1.0;
    h.wait_t = 0.0;
    h.busy_t = 0.0;
    h.t = 0.0;
    h.stagger_t = 0.0;
    h.daze_t = 0.0;
    h.unreach_t = 0.0;
    h.unreachable = false;
    h.wander_far = false;
    h.target = Target::Player;
    h.target_id = None;
    h.last_known = (h.x, h.z);
    h.prev_state = None;
    h.snuffed = false;
    h.trapped = false;
    h.homing = false;
    h.lunge_done = false;
    h.rest_idx = None;
    h.step_t = 0.0;
    h.moved = false;
    h.wake_t = 0.0;
    h.ember_k = 0.0;
    if let Some(f) = ctx.prof(h.profile).hooks.on_reset {
        f(ctx, h);
    }
    h.anim.eye_k = ctx.prof(h.profile).eye_of(h.state);
}

/// `hunter.js:spawnHunter(cx, cz, profile)` — a new active `base` / `fast` hunter at that cell (the endgame's
/// deeper-lap reinforcements); a creature profile is routed to [`spawn_creature`]. Returns its index, `None`
/// off-grid or on a solid. (The JS also appended a `map.hunterSpawns` entry so the next `spawnAll` rebuilt it;
/// the map is immutable here, so the caller keeps its own list of extras.)
pub fn spawn_hunter(
    ctx: &mut Ctx,
    hunters: &mut Vec<Hunter>,
    cx: i32,
    cz: i32,
    profile: ProfileKind,
) -> Option<usize> {
    if profile.is_creature() {
        return spawn_creature(ctx, hunters, profile, cx, cz, SpawnOpts::default());
    }
    let m = ctx.map();
    if !in_bounds(m, cx, cz) || is_solid(m, cx, cz) {
        return None;
    }
    let mut h = make_hunter(ctx, hunters.len() as u32, profile);
    h.extra = true;
    reset(ctx, &mut h, cx, cz);
    h.idle_t = 0.5;
    hunters.push(h);
    Some(hunters.len() - 1)
}

/// `hunter.js:spawnCreature(profile, cx, cz, opts)` — a new record of any profile at that cell (debug / tests).
/// Inactive when not in a zone.
pub fn spawn_creature(
    ctx: &mut Ctx,
    hunters: &mut Vec<Hunter>,
    profile: ProfileKind,
    cx: i32,
    cz: i32,
    opts: SpawnOpts,
) -> Option<usize> {
    let m = ctx.map();
    if !in_bounds(m, cx, cz) || is_solid(m, cx, cz) {
        return None;
    }
    if !profile.is_creature() {
        return spawn_hunter(ctx, hunters, cx, cz, profile);
    }
    let mut h = make_hunter(ctx, hunters.len() as u32, profile);
    h.extra = true;
    h.opts = opts;
    reset(ctx, &mut h, cx, cz);
    if !ctx.env.in_zone {
        h.active = false;
    }
    hunters.push(h);
    Some(hunters.len() - 1)
}

/// `hunter.js:spawnAll(zone)` — one record per `H` cell (profile from `zone.hunters`, the last one repeated,
/// `base` when the list is empty or the name unknown) then one per creature cell (profile = kind, options from
/// `zone.creatures` in the same row-major order), all active at their spawn cells. The map is `ctx.env.map`.
pub fn spawn_all(ctx: &mut Ctx, zone: &ZoneDef) -> Vec<Hunter> {
    let map = ctx.map();
    let mut specs: Vec<(i32, i32, ProfileKind, SpawnOpts)> = Vec::new();
    for (i, s) in map.hunter_spawns.iter().enumerate() {
        let name = zone.hunters.get(i).or(zone.hunters.last());
        let profile = name.map_or(ProfileKind::Base, |n| ProfileKind::from_js_name_or_base(n));
        specs.push((s.cx, s.cz, profile, SpawnOpts::default()));
    }
    for (i, c) in map.creatures.iter().enumerate() {
        let opts = zone
            .creatures
            .get(i)
            .map(SpawnOpts::from)
            .unwrap_or_default();
        specs.push((
            c.marker.cx,
            c.marker.cz,
            ProfileKind::from_creature(c.kind),
            opts,
        ));
    }
    let mut out = Vec::with_capacity(specs.len());
    for (i, (cx, cz, profile, opts)) in specs.into_iter().enumerate() {
        let mut h = make_hunter(ctx, i as u32, profile);
        h.opts = opts;
        h.extra = false;
        reset(ctx, &mut h, cx, cz);
        out.push(h);
    }
    out
}

/// `hunter.js:clear()` — deactivate every hunter (kept in the list so `hunters[0]` still reads).
pub fn clear(hunters: &mut [Hunter]) {
    for h in hunters {
        h.active = false;
        h.anim.light_on = false;
        h.anim.burst_t = 0.0;
    }
}

/// The `gateOpened` / `shortcutOpened` listeners — every path is dropped and recomputed on the next tick
/// (DESIGN.md §3.6).
pub fn clear_paths(hunters: &mut [Hunter]) {
    for h in hunters {
        h.path.clear();
    }
}

/// The `npcCaught` listener — the catcher stands over the spot for `catchBusyT`, then wanders on (profiles
/// with an INVESTIGATE state investigate their own position unless STAGGERED).
pub fn on_npc_caught(ctx: &mut Ctx, h: &mut Hunter) {
    if !h.active {
        return;
    }
    h.busy_t = ctx.tuning.hunter.catch_busy_t;
    h.target = Target::Player;
    h.stim = false;
    let has_investigate = ctx.prof(h.profile).row(HState::Investigate).is_some();
    if has_investigate && h.state != HState::Staggered {
        let (x, z) = (h.x, h.z);
        investigate(ctx, h, x, z);
    }
}

/// `hunter.js:hint()` — the creature line for the HUD (DESIGN.md §5.1–5 HUD rows), empty when none applies.
/// The 2 s overrides (alert / reveal / smash / resist) go out as `toast` events instead.
pub fn hint(ctx: &Ctx, hunters: &[Hunter]) -> String {
    let env = ctx.env;
    if !env.in_zone {
        return String::new();
    }
    let p = env.player;
    let m = env.map;
    if p.lamp_lock > 0.0 {
        return format!("Snuffed — [F] relight in {} s", p.lamp_lock.ceil() as i32);
    }
    for h in hunters {
        if !h.active {
            continue;
        }
        let d = dist2d(h.x, h.z, p.x, p.z);
        let s = h.state;
        let out = match h.profile {
            ProfileKind::Lampwight
                if s == HState::Drawn && d <= 12.0 && los(m, h.x, h.z, p.x, p.z) =>
            {
                "Something is drawn to your light"
            }
            ProfileKind::Warden if s == HState::Chase => "Leave its ground or go dark",
            ProfileKind::Drowner if s == HState::Surge => "It's in the water — get out",
            ProfileKind::Drowner
                if s == HState::Submerged
                    && d <= 8.0
                    && super::drowner::near_body(m, h, p.x, p.z) =>
            {
                "The water is moving"
            }
            ProfileKind::FalseLight if s == HState::Pounce => "It has seen you",
            ProfileKind::Brute if p.in_pool && d <= 6.0 => "Not safe — it will wade in",
            ProfileKind::Brute if s == HState::Chase => "It has seen you",
            _ => "",
        };
        if !out.is_empty() {
            return out.to_string();
        }
    }
    String::new()
}
