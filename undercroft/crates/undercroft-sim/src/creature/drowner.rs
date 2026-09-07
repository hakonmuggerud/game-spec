//! The Drowner — the thing under the surface (`hunter.js` ~487–610 `floodWater`, `nearBody`, `drownerSense`,
//! `driftPick`, `sink`, `DROWNER_FSM`, `drownerFlash`, `drownerReset`, `drownerGuard`, `drownerAnim`;
//! DESIGN.md §5.3). Water cells of its own body only; lit or loud near the water = surge.

use super::driver::{enter_chase, field_from, hunter_cell, path_to_point, set_state};
use super::profiles::{Fsm, Move, StateRow};
use super::record::{HState, Hunter, Target};
use super::{Ctx, Env, FlashResult, Stim, Tuning};
use crate::events::SimEvent;
use crate::grid::{cell_type, dist2d, idx, in_bounds, los, path_to, DIRS4};
use undercroft_data::{CellKind, ParsedMap};

/// `hunter.js:floodWater` — the connected water body containing a cell (all zero when the cell is not water).
pub fn flood_water(m: &ParsedMap, cx: i32, cz: i32) -> Vec<u8> {
    let mut body = vec![0u8; m.len()];
    if cell_type(m, cx, cz) != CellKind::Water {
        return body;
    }
    let mut q = vec![idx(m, cx, cz)];
    body[q[0]] = 1;
    while let Some(i) = q.pop() {
        let (x, z) = m.cell_of(i);
        for (dx, dz) in DIRS4 {
            let (nx, nz) = (x + dx, z + dz);
            if !in_bounds(m, nx, nz) || cell_type(m, nx, nz) != CellKind::Water {
                continue;
            }
            let j = idx(m, nx, nz);
            if body[j] != 0 {
                continue;
            }
            body[j] = 1;
            q.push(j);
        }
    }
    body
}

/// `hunter.js:nearBody` — the point is in, or 4-adjacent to, a water cell of this drowner's body.
pub fn near_body(m: &ParsedMap, h: &Hunter, x: f32, z: f32) -> bool {
    let (cx, cz) = crate::grid::to_cell(m, x, z);
    if !in_bounds(m, cx, cz) || h.body.is_empty() {
        return false;
    }
    if h.body[idx(m, cx, cz)] != 0 {
        return true;
    }
    DIRS4.iter().any(|(dx, dz)| {
        let (nx, nz) = (cx + dx, cz + dz);
        in_bounds(m, nx, nz) && h.body[idx(m, nx, nz)] != 0
    })
}

/// `hunter.js:drownerSense` — `trigger` (near its body within `trigger` u, lit-with-LOS / sprinting / wading),
/// `keep` (lit ≤ lamp, wading ≤ water, sprinting ≤ sprint) and `home` (lit ≤ `home` u while not dazed);
/// `stim` = trigger || keep.
pub fn sense(ctx: &Ctx, h: &Hunter) -> Option<Stim> {
    let env = ctx.env;
    let p = env.player;
    let m = env.map;
    let dr = &ctx.tuning.creature.drowner;
    let r = &ctx.prof(h.profile).senses;
    let d = dist2d(p.x, p.z, h.x, h.z);
    let lit = p.lit() && los(m, h.x, h.z, p.x, p.z);
    let wading = p.in_water && p.moving;
    let sprint = p.sprinting;
    let trigger = h.daze_t <= 0.0
        && d <= dr.trigger
        && near_body(m, h, p.x, p.z)
        && (lit || sprint || wading);
    let keep = (lit && d <= r.lamp) || (wading && d <= r.water) || (sprint && d <= r.sprint);
    let home = h.daze_t <= 0.0 && lit && d <= dr.home;
    if !trigger && !keep && !home {
        return None;
    }
    Some(Stim {
        kind: Target::Player,
        id: None,
        x: p.x,
        z: p.z,
        d,
        trigger,
        keep,
        home,
        stim: trigger || keep,
    })
}

/// `hunter.js:driftPick` — a random water cell of its body ≤ `driftCells` BFS away, and a new idle timer.
fn drift_pick(ctx: &mut Ctx, h: &mut Hunter) {
    let tuning = ctx.tuning;
    let dr = &tuning.creature.drowner;
    let field = field_from(ctx, h);
    let cands: Vec<usize> = field
        .dist
        .iter()
        .enumerate()
        .filter(|(_, &d)| d > 0 && d as i32 <= dr.drift_cells)
        .map(|(i, _)| i)
        .collect();
    h.idle_t = dr.drift_t[0] + ctx.rng.unit() as f32 * (dr.drift_t[1] - dr.drift_t[0]);
    if !cands.is_empty() {
        let pick = cands[ctx.rand_index(cands.len())];
        h.path = path_to(ctx.map(), &field, pick);
    }
}

/// `hunter.js:sink` — SINK for `sinkT`, `drownerSink`.
pub fn sink(ctx: &mut Ctx, h: &mut Hunter) {
    set_state(ctx, h, HState::Sink);
    h.t = ctx.tuning.creature.drowner.sink_t;
    h.path.clear();
    ctx.emit(SimEvent::DrownerSink { hunter_id: h.id });
}

/// SUBMERGED speed: `homeSpeed` while homing on a lamp, else the state speed.
fn submerged_speed(h: &Hunter, t: &Tuning) -> f32 {
    if h.homing {
        t.creature.drowner.home_speed
    } else {
        t.prof(h.profile).speed_of(HState::Submerged)
    }
}

/// `DROWNER_FSM` rows.
pub fn row(s: HState) -> Option<StateRow> {
    Some(match s {
        HState::Submerged => StateRow::new(Move::Path).with_speed(submerged_speed),
        HState::Surfacing => StateRow::new(Move::Still).ignore_stim(),
        HState::Surge => StateRow::new(Move::Path)
            .repath()
            .close()
            .catch()
            .last_known_when_blind(),
        HState::Lurk => StateRow::new(Move::Path),
        HState::Sink => StateRow::new(Move::Still).ignore_stim(),
        _ => return None,
    })
}

fn tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    let tuning = ctx.tuning;
    let tick = tuning.hunter.tick;
    let dr = &tuning.creature.drowner;
    match h.state {
        HState::Submerged => {
            if t.is_some_and(|t| t.trigger) {
                set_state(ctx, h, HState::Surfacing);
                h.t = dr.surface_t;
                h.path.clear();
                h.homing = false;
                ctx.emit(SimEvent::DrownerSurge {
                    hunter_id: h.id,
                    x: h.x,
                    z: h.z,
                });
                return;
            }
            match t {
                Some(t) if t.home => {
                    h.homing = true;
                    h.repath_t += tick;
                    if h.repath_t >= 1.0 {
                        h.repath_t = 0.0;
                        let field = field_from(ctx, h);
                        path_to_point(ctx.env, h, &field, t.x, t.z);
                    }
                    h.last_known = (t.x, t.z);
                }
                _ => {
                    h.homing = false;
                    if h.path.is_empty() {
                        h.idle_t -= tick;
                        if h.idle_t <= 0.0 {
                            drift_pick(ctx, h);
                        }
                    }
                }
            }
        }
        HState::Surfacing => {
            h.t -= tick;
            if h.t <= 0.0 {
                enter_chase(ctx, h, HState::Surge);
                h.wake_t = 0.5;
            }
        }
        HState::Surge => {
            if t.is_some_and(|t| t.keep) {
                h.no_stim_t = 0.0;
            } else {
                h.no_stim_t += tick;
            }
            if h.no_stim_t >= ctx.prof(h.profile).lose_t {
                set_state(ctx, h, HState::Lurk);
                h.t = dr.lurk_t;
                let field = field_from(ctx, h);
                let (lx, lz) = h.last_known;
                path_to_point(ctx.env, h, &field, lx, lz);
            }
        }
        HState::Lurk => {
            if t.is_some_and(|t| t.keep) {
                return enter_chase(ctx, h, HState::Surge);
            }
            h.t -= tick;
            if h.t <= 0.0 {
                return sink(ctx, h);
            }
            if h.path.is_empty() {
                let field = field_from(ctx, h);
                let cands: Vec<usize> = field
                    .dist
                    .iter()
                    .enumerate()
                    .filter(|(_, &d)| d > 0 && d <= 4)
                    .map(|(i, _)| i)
                    .collect();
                if !cands.is_empty() {
                    let pick = cands[ctx.rand_index(cands.len())];
                    h.path = path_to(ctx.map(), &field, pick);
                }
            }
        }
        HState::Sink => {
            h.t -= tick;
            if h.t <= 0.0 {
                set_state(ctx, h, HState::Submerged);
                h.daze_t = dr.daze_t;
                h.idle_t = 1.0;
            }
        }
        _ => {}
    }
}

/// `hunter.js:DROWNER_FSM`.
pub const DROWNER_FSM: Fsm = Fsm { row, tick };

/// `hunter.js:drownerFlash` — a forced dive while surfaced, nothing while submerged.
pub fn on_flash(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    if matches!(h.state, HState::Surfacing | HState::Surge | HState::Lurk) {
        sink(ctx, h);
        return FlashResult::Sink;
    }
    FlashResult::None
}

/// `canEnter`: a water cell of its body, not a pool cell.
pub fn can_enter(env: &Env, h: &Hunter, cx: i32, cz: i32) -> bool {
    let m = env.map;
    cell_type(m, cx, cz) == CellKind::Water
        && h.body.get(idx(m, cx, cz)).copied() == Some(1)
        && !env.pool.is_pool(idx(m, cx, cz))
}

/// `canEnterLoose`: a water cell of its body — it never leaves the water.
pub fn can_enter_loose(env: &Env, h: &Hunter, cx: i32, cz: i32) -> bool {
    let m = env.map;
    cell_type(m, cx, cz) == CellKind::Water && h.body.get(idx(m, cx, cz)).copied() == Some(1)
}

/// `hunter.js:drownerReset` — flood its body from the spawn, start submerged.
pub fn on_reset(ctx: &mut Ctx, h: &mut Hunter) {
    h.body = flood_water(ctx.map(), h.home.cx, h.home.cz);
    h.y = ctx.tuning.creature.drowner.y_sub;
    h.idle_t = 1.0;
    h.anim.ripple_k = ctx.tuning.creature.drowner.ripple.k;
}

/// `hunter.js:drownerGuard` — every 0.2 s: a drowner outside its own water body (debug teleport) snaps back to
/// its spawn.
pub fn guard(ctx: &mut Ctx, h: &mut Hunter) {
    let m = ctx.map();
    let (cx, cz) = hunter_cell(ctx.env, h);
    if !in_bounds(m, cx, cz) || h.body.get(idx(m, cx, cz)).copied() != Some(1) {
        h.x = h.home.x;
        h.z = h.home.z;
        h.path.clear();
    }
}

/// `hunter.js:drownerAnim` — jaw, body height per state, the ripple ring's scale / glow and the surge wake.
pub fn anim(env: &Env, tuning: &Tuning, h: &mut Hunter, dt: f32) {
    let dr = &tuning.creature.drowner;
    let s = h.state;
    h.anim.jaw = match s {
        HState::Surge => -0.45,
        HState::Surfacing => -0.2,
        HState::Lurk => -0.12,
        _ => 0.0,
    };
    h.y = match s {
        HState::Surfacing => {
            dr.y_sub + (dr.y_surf - dr.y_sub) * (1.0 - h.t.max(0.0) / dr.surface_t)
        }
        HState::Surge | HState::Lurk => dr.y_surf,
        HState::Sink => dr.y_surf + (dr.y_sub - dr.y_surf) * (1.0 - h.t.max(0.0) / dr.sink_t),
        _ => dr.y_sub,
    };
    h.anim.body_shown = s != HState::Submerged;
    h.anim.ring_y = 0.02 - h.y - 0.1; // stays on the water surface (y −0.1) whatever the body does
    let r = &dr.ripple;
    if h.wake_t > 0.0 {
        h.wake_t -= dt;
    }
    h.anim.ripple_scale = if s == HState::Submerged {
        r.min + (r.max - r.min) * ((env.time + h.phase) % r.period) / r.period
    } else if h.wake_t > 0.0 {
        r.max * (1.0 + 0.5 * (1.0 - h.wake_t / 0.5))
    } else {
        1.0
    };
    h.anim.ripple_k = if s == HState::Submerged {
        r.k
    } else {
        r.k * 0.5
    };
}
