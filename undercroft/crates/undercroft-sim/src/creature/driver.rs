//! The shared FSM driver (`hunter.js` "Shared helpers" ~196–317 and "Driver" ~845–950) plus the flash
//! targeting of `main.js:flash` (~173–189).
//!
//! Per frame: [`update`] walks every active record → [`update_one`]: the 0.2 s sense/FSM [`tick`], the 0.3 s
//! chase repath, path following with the `close` fallback, the lunge, footsteps, the near-lantern hook, the
//! catch test and the anim hook. BFS runs only on repath ticks and wander legs, never per frame.

use super::profiles::{Move, StateRow};
use super::record::{HState, Hunter, ProfileKind};
use super::senses::target_pos;
use super::spawn::reset;
use super::{Ctx, Env, FlashResult, Stim};
use crate::events::{CatchTarget, SimEvent};
use crate::grid::{
    bfs_field, cell_type, dist2d, idx, in_bounds, los, nearest_reachable, path_to, to_cell, Field,
};
use undercroft_data::CellKind;

/// `hunter.js:setState` — change state, emitting `hunterState {id, state, prev}` on a real transition. (No
/// state row in the prototype defines an `enter` hook, so there is none here.)
pub fn set_state(ctx: &mut Ctx, h: &mut Hunter, s: HState) {
    let prev = h.state;
    h.state = s;
    if prev != s {
        ctx.emit(SimEvent::HunterState {
            id: h.id,
            state: s.js_name().to_string(),
            prev: prev.js_name().to_string(),
        });
    }
}

/// `hunter.js:hunterCell`.
#[inline]
pub fn hunter_cell(env: &Env, h: &Hunter) -> (i32, i32) {
    to_cell(env.map, h.x, h.z)
}

/// An all-unreachable field (a hunter standing off the grid has nowhere to go).
fn empty_field(env: &Env) -> Field {
    let n = env.map.len();
    Field {
        dist: vec![-1; n],
        parent: vec![-1; n],
    }
}

/// `hunter.js:fieldFrom` — BFS from the hunter's cell over the profile's movement rule (`h.blocked`).
pub fn field_from(ctx: &Ctx, h: &Hunter) -> Field {
    let env = ctx.env;
    let (cx, cz) = hunter_cell(env, h);
    if !in_bounds(env.map, cx, cz) {
        return empty_field(env);
    }
    let can_enter = ctx.prof(h.profile).hooks.can_enter;
    bfs_field(env.map, cx, cz, |x, z| !can_enter(env, h, x, z))
}

/// `hunter.js:escapeIfTrapped` — standing where its own rules forbid (a lantern planted on top of it): walk to
/// the nearest cell it may occupy, with pools ignored only for the way out (`canEnterLoose`). Returns whether
/// it is walking out (the state tick and the chase repath are then skipped).
pub fn escape_if_trapped(ctx: &mut Ctx, h: &mut Hunter) -> bool {
    let env = ctx.env;
    let m = env.map;
    let (cx, cz) = hunter_cell(env, h);
    let hooks = ctx.prof(h.profile).hooks;
    if !in_bounds(m, cx, cz) || (hooks.can_enter)(env, h, cx, cz) {
        return false;
    }
    let field = bfs_field(m, cx, cz, |x, z| !(hooks.can_enter_loose)(env, h, x, z));
    let mut best: Option<usize> = None;
    let mut bd = i16::MAX;
    for (i, &d) in field.dist.iter().enumerate() {
        if d <= 0 || d >= bd {
            continue;
        }
        let (x, z) = m.cell_of(i);
        if !(hooks.can_enter)(env, h, x, z) {
            continue;
        }
        bd = d;
        best = Some(i);
    }
    h.path = best.map(|i| path_to(m, &field, i)).unwrap_or_default();
    !h.path.is_empty()
}

/// `hunter.js:pathToPoint` — set `h.path` to that cell, or to the nearest reachable one; returns whether the
/// cell itself was reachable.
pub fn path_to_point(env: &Env, h: &mut Hunter, field: &Field, x: f32, z: f32) -> bool {
    let m = env.map;
    let (tx, tz) = to_cell(m, x, z);
    let reachable = in_bounds(m, tx, tz) && field.dist[idx(m, tx, tz)] >= 0;
    let ti = if reachable {
        Some(idx(m, tx, tz))
    } else {
        nearest_reachable(m, field, x, z)
    };
    h.path = ti.map(|i| path_to(m, field, i)).unwrap_or_default();
    reachable
}

/// `hunter.js:pickWander(h, far, minCells)` — a random reachable cell: near (≤ `wanderCells` BFS), far
/// (≥ `farCells` from the player) or ≥ `min_cells` from the player (the sated Lampwight). A leashed creature
/// only picks cells within its leash of home and heads back toward the leash when outside it.
pub fn pick_wander(ctx: &mut Ctx, h: &mut Hunter, far: bool, min_cells: i32) {
    let env = ctx.env;
    let m = env.map;
    let p = env.player;
    let field = field_from(ctx, h);
    let (pcx, pcz) = to_cell(m, p.x, p.z);
    let hcfg = &ctx.tuning.hunter;
    let leash_n = ctx.prof(h.profile).leash.unwrap_or(0);
    let leashed = !h.leash.is_empty();
    let mut cands: Vec<usize> = Vec::new();
    let mut farthest: Option<usize> = None;
    let mut fd = -1.0f64;
    let mut back: Option<usize> = None;
    let mut bd = i16::MAX;
    for (i, &d) in field.dist.iter().enumerate() {
        if d <= 0 {
            continue;
        }
        if leashed {
            let l = h.leash[i];
            if l < 0 || l as i32 > leash_n {
                continue;
            }
            if l < bd {
                bd = l;
                back = Some(i);
            }
        }
        let (cx, cz) = m.cell_of(i);
        let pd = ((cx - pcx) as f64).hypot((cz - pcz) as f64);
        if min_cells > 0 {
            if pd >= min_cells as f64 {
                cands.push(i);
            }
            if pd > fd {
                fd = pd;
                farthest = Some(i);
            }
        } else if far {
            if pd >= hcfg.far_cells as f64 {
                cands.push(i);
            }
            if pd > fd {
                fd = pd;
                farthest = Some(i);
            }
        } else if d as i32 <= hcfg.wander_cells {
            cands.push(i);
        }
    }
    if cands.is_empty() && leashed {
        if let Some(b) = back {
            cands.push(b); // outside the leash: head for the nearest leash cell
        }
    }
    if cands.is_empty() {
        if let Some(f) = farthest {
            cands.push(f);
        }
    }
    if cands.is_empty() {
        h.idle_t = 1.0;
        return;
    }
    let pick = cands[ctx.rand_index(cands.len())];
    h.path = path_to(m, &field, pick);
    h.idle_t = 1.0 + ctx.rng.unit() as f32 * 2.0;
}

/// `hunter.js:enterChase` — the chase-like states share their bookkeeping (CHASE / DRAWN / SURGE / Warden CHASE).
pub fn enter_chase(ctx: &mut Ctx, h: &mut Hunter, state: HState) {
    set_state(ctx, h, state);
    h.no_stim_t = 0.0;
    h.unreach_t = 0.0;
    h.unreachable = false;
    h.repath_t = ctx.tuning.hunter.repath;
}

/// `hunter.js:chasePath` — BFS toward the live target (base/fast: always; creatures: only while stimulated,
/// else `lastKnown`).
pub fn chase_path(ctx: &mut Ctx, h: &mut Hunter) {
    let row = ctx.prof(h.profile).row(h.state);
    let (tx, tz) = if row.is_some_and(|r| r.last_known_when_blind) && !h.stim {
        h.last_known
    } else {
        let (x, z, _) = target_pos(ctx.env, h);
        (x, z)
    };
    let field = field_from(ctx, h);
    h.unreachable = !path_to_point(ctx.env, h, &field, tx, tz);
}

/// `hunter.js:investigate(h, x, z)`.
pub fn investigate(ctx: &mut Ctx, h: &mut Hunter, x: f32, z: f32) {
    set_state(ctx, h, HState::Investigate);
    h.wait_t = ctx.tuning.hunter.wait_t;
    let field = field_from(ctx, h);
    h.wander_far = !path_to_point(ctx.env, h, &field, x, z);
}

/// `hunter.js:stagger(h)` — the flash reaction (base/fast: STAGGERED; creatures: their own `onFlash` rule).
pub fn stagger(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    if ctx.prof(h.profile).creature {
        return on_flash(ctx, h);
    }
    set_state(ctx, h, HState::Staggered);
    h.stagger_t = ctx.tuning.hunter.stagger_t;
    h.daze_t = 0.0;
    h.path.clear();
    FlashResult::Stagger
}

/// `hunter.js:onFlash(h)` — the profile's reaction for an active hunter inside the flash cone with LOS.
pub fn on_flash(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    if !h.active {
        return FlashResult::None;
    }
    (ctx.prof(h.profile).hooks.on_flash)(ctx, h)
}

/// `hunter.js:stateDef(h)` — the current state's row; a state the profile does not know (the endgame writing
/// `WANDER` onto a creature) resets it to the profile's initial state.
pub fn state_def(ctx: &mut Ctx, h: &mut Hunter) -> StateRow {
    let prof = ctx.prof(h.profile);
    if let Some(r) = prof.row(h.state) {
        return r;
    }
    let initial = prof.initial;
    set_state(ctx, h, initial);
    prof.row(initial).unwrap_or(StateRow::new(Move::Still))
}

/// `hunter.js:tick(h)` — the 0.2 s sense + FSM step.
pub fn tick(ctx: &mut Ctx, h: &mut Hunter) {
    let tick = ctx.tuning.hunter.tick;
    h.daze_t = (h.daze_t - tick).max(0.0);
    h.busy_t = (h.busy_t - tick).max(0.0);
    let prof = ctx.prof(h.profile);
    if let Some(f) = prof.hooks.on_tick {
        f(ctx, h);
    }
    let st = state_def(ctx, h);
    // only a state that walks can walk out; a still state (a Warden at its post, a LIT false light) just sits in
    // the pool — it cannot reach the player either, since its lunge/close refuse pool cells
    h.trapped = st.mv == Move::Path && escape_if_trapped(ctx, h);
    if h.trapped {
        return; // walking out: the state keeps its timers, nothing else runs this tick
    }
    let t: Option<Stim> = if st.ignore_stim {
        None
    } else {
        (prof.hooks.sense)(ctx, h)
    };
    h.stim = t.as_ref().is_some_and(|t| t.stim);
    if h.stim {
        if let Some(t) = t.as_ref() {
            h.last_known = (t.x, t.z);
            h.no_stim_t = 0.0;
            h.target = t.kind;
            h.target_id = t.id.clone();
        }
    }
    (prof.fsm.tick)(ctx, h, t.as_ref());
}

/// `hunter.js:moveToward` — step toward a point at `speed`; returns the distance left.
pub fn move_toward(h: &mut Hunter, tx: f32, tz: f32, speed: f32, dt: f32) -> f32 {
    let dx = tx - h.x;
    let dz = tz - h.z;
    let d = dx.hypot(dz);
    if d < 1e-4 {
        return d;
    }
    let step = (speed * dt).min(d);
    h.x += dx / d * step;
    h.z += dz / d * step;
    h.yaw = (-dx).atan2(-dz);
    h.moved = step > 0.0;
    d - step
}

/// `hunter.js:speedFor` — the state speed with the water and pool multipliers of the cell it stands on.
pub fn speed_for(ctx: &Ctx, h: &Hunter, st: &StateRow) -> f32 {
    let env = ctx.env;
    let m = env.map;
    let prof = ctx.prof(h.profile);
    let (cx, cz) = hunter_cell(env, h);
    let mut s = match st.speed {
        Some(f) => f(h, ctx.tuning),
        None => prof.speed_of(h.state),
    };
    if cell_type(m, cx, cz) == CellKind::Water {
        s *= prof.water_mul;
    }
    if prof.pool_mul != 1.0 && in_bounds(m, cx, cz) && env.pool.is_pool(idx(m, cx, cz)) {
        s *= prof.pool_mul;
    }
    s
}

/// `hunter.js:syncMesh` — the render-side values derived from the record each frame.
fn sync(ctx: &Ctx, h: &mut Hunter) {
    if h.profile != ProfileKind::Brute {
        h.yaw_vis = h.yaw;
    }
    h.anim.eye_k = ctx.prof(h.profile).eye_of(h.state);
}

/// `hunter.js:updateOne(h, dt)`. `smashed` collects lanterns removed this frame (indices into `Env::lanterns`)
/// so two creatures cannot smash the same one. Returns `true` when this hunter emitted `hunterCatch` (the run
/// ended: `update` stops the loop, as the JS did on `mode !== 'ZONE'`).
pub fn update_one(ctx: &mut Ctx, h: &mut Hunter, dt: f32, smashed: &mut Vec<usize>) -> bool {
    let env = ctx.env;
    let m = env.map;
    let p = env.player;
    let tuning = ctx.tuning;
    let hcfg = &tuning.hunter;
    h.tick_t += dt;
    if h.tick_t >= hcfg.tick {
        h.tick_t -= hcfg.tick;
        tick(ctx, h);
    }
    let st = state_def(ctx, h);
    let prof = ctx.prof(h.profile);
    h.moved = false;
    h.anim.burst_fired = false;
    if st.repath && !h.trapped {
        h.repath_t += dt;
        if h.repath_t >= hcfg.repath {
            h.repath_t = 0.0;
            chase_path(ctx, h);
        }
    }
    match st.mv {
        Move::Path => {
            let speed = speed_for(ctx, h, &st);
            if let Some(&(nx, nz)) = h.path.first() {
                let last = h.path.len() == 1;
                if move_toward(h, nx, nz, speed, dt) < (if last { 0.02 } else { 0.1 }) {
                    h.path.remove(0);
                }
            } else if st.close && !h.trapped && (h.stim || !st.last_known_when_blind) {
                // path exhausted: close the last stretch directly — never into a pool (unless it wades in),
                // never off its ground
                let (tx, tz, in_pool) = target_pos(env, h);
                let (pcx, pcz) = to_cell(m, tx, tz);
                if !h.unreachable
                    && (!in_pool || prof.catch_in_pool)
                    && (prof.hooks.can_enter)(env, h, pcx, pcz)
                {
                    move_toward(h, tx, tz, speed, dt);
                } else {
                    h.yaw = (-(tx - h.x)).atan2(-(tz - h.z));
                }
            } else if st.sweep {
                h.yaw += 0.7 * dt; // slow sweep while waiting
            }
        }
        Move::Lunge if !h.lunge_done => {
            // straight line at the position the lunge started with; stops at walls and pool edges
            let speed = speed_for(ctx, h, &st);
            let dx = h.lunge.0 - h.x;
            let dz = h.lunge.1 - h.z;
            let d = dx.hypot(dz);
            if d < 0.05 {
                h.lunge_done = true;
            } else {
                let step = (speed * dt).min(d);
                let nx = h.x + dx / d * step;
                let nz = h.z + dz / d * step;
                let (ncx, ncz) = to_cell(m, nx, nz);
                if (prof.hooks.can_enter)(env, h, ncx, ncz) {
                    h.x = nx;
                    h.z = nz;
                    h.yaw = (-dx).atan2(-dz);
                    h.moved = true;
                } else {
                    h.lunge_done = true;
                }
            }
        }
        _ => {}
    }
    if !h.x.is_finite() || !h.z.is_finite() {
        let (cx, cz) = (h.home.cx, h.home.cz);
        reset(ctx, h, cx, cz);
    }
    // footsteps (creatureStep) for profiles that stride
    if prof.step.is_some() && h.moved {
        h.step_t += dt;
        let iv = prof.step_of(h.state);
        if h.step_t >= iv {
            h.step_t -= iv;
            ctx.emit(SimEvent::CreatureStep {
                hunter_id: h.id,
                profile: h.profile.js_name().to_string(),
                x: h.x,
                z: h.z,
                d: dist2d(h.x, h.z, p.x, p.z),
            });
        }
    }
    // a lantern within reach is smashed before any catch is checked: the pool buys the seconds the smash takes
    if let Some(f) = prof.hooks.on_near_lantern {
        if !env.lanterns.is_empty() {
            f(ctx, h, smashed);
        }
    }
    // catch (the player; follower catches are the follower module's — it emits npcCaught)
    let mut caught = false;
    if env.in_zone
        && st.catch
        && (!p.in_pool || prof.catch_in_pool)
        && dist2d(h.x, h.z, p.x, p.z) <= prof.catch_r
        && prof.hooks.catch_if.is_none_or(|f| f(h, p))
    {
        if prof.kills {
            ctx.emit(SimEvent::HunterCatch {
                x: p.x,
                z: p.z,
                hunter_id: h.id,
                target: CatchTarget::Player,
            });
            caught = true;
        } else if let Some(f) = prof.hooks.on_catch {
            f(ctx, h);
        }
    }
    if let Some(f) = prof.hooks.anim {
        f(env, ctx.tuning, h, dt);
    }
    sync(ctx, h);
    caught
}

/// `hunter.js:update(c, dt)` — step every active record; stops after a `hunterCatch` (the run ended). Does
/// nothing outside a zone (`mode !== 'ZONE'`); pausing is the caller's (it simply does not call this).
pub fn update(ctx: &mut Ctx, hunters: &mut [Hunter], dt: f32) {
    if !ctx.env.in_zone {
        return;
    }
    let mut smashed: Vec<usize> = Vec::new();
    for h in hunters.iter_mut() {
        if !h.active {
            continue;
        }
        if update_one(ctx, h, dt, &mut smashed) {
            break; // a catch this frame ended the run
        }
    }
}

/// `main.js:flash` targeting (~178–185): the indices of the active hunters inside the flash cone
/// (`0 < d ≤ CFG.flashRange`, `dot(forward, toHunter) > CFG.flashDot`) with LOS from the player. What the
/// flash does to each is the profile's ([`on_flash`]).
pub fn flash_targets(env: &Env, tuning: &super::Tuning, hunters: &[Hunter]) -> Vec<usize> {
    let p = env.player;
    let (fx, fz) = (-p.yaw.sin(), -p.yaw.cos());
    let mut out = Vec::new();
    for (i, h) in hunters.iter().enumerate() {
        if !h.active {
            continue;
        }
        let dx = h.x - p.x;
        let dz = h.z - p.z;
        let d = dx.hypot(dz);
        if d > 0.0 && d <= tuning.flash_range {
            let dot = (dx * fx + dz * fz) / d;
            if dot > tuning.flash_dot && los(env.map, h.x, h.z, p.x, p.z) {
                out.push(i);
            }
        }
    }
    out
}

/// `main.js:flash` applied to the creatures: every hunter [`flash_targets`] picks gets its `onFlash`, in list
/// order. Returns `(id, reaction)` per hit. The oil cost, cooldown, `flashT` and the `flash` event are the
/// player's side (the shell / economy lane).
pub fn flash(ctx: &mut Ctx, hunters: &mut [Hunter]) -> Vec<(u32, FlashResult)> {
    let hits = flash_targets(ctx.env, ctx.tuning, hunters);
    hits.into_iter()
        .map(|i| {
            let h = &mut hunters[i];
            let r = on_flash(ctx, h);
            (h.id, r)
        })
        .collect()
}
