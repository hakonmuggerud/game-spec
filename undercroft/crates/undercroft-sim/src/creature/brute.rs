//! The Brute — the wall that walks (`hunter.js` ~699–797 `BRUTE_FSM`, `bruteFlash`, `bruteReset`,
//! `bruteNearLantern`, `bruteAnim`; DESIGN.md §5.5). The base FSM without STAGGERED, leashed wander, pools
//! allowed at half speed, smashes lanterns, ignores the flash.

use super::base::{chase_tick, investigate_tick, wander_tick};
use super::profiles::{Fsm, Move, StateRow};
use super::record::{HState, Hunter};
use super::{Ctx, Env, FlashResult, Stim, Tuning};
use crate::events::SimEvent;
use crate::grid::{bfs_solid, dist2d};
use std::f32::consts::{PI, TAU};

/// `BRUTE_FSM` rows — WANDER without `catch`, INVESTIGATE / CHASE as the base.
pub fn row(s: HState) -> Option<StateRow> {
    Some(match s {
        HState::Wander => StateRow::new(Move::Path),
        HState::Investigate => StateRow::new(Move::Path).catch().sweep(),
        HState::Chase => StateRow::new(Move::Path).repath().close().catch(),
        _ => return None,
    })
}

fn tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    match h.state {
        HState::Wander => wander_tick(ctx, h, t),
        HState::Investigate => investigate_tick(ctx, h, t),
        HState::Chase => {
            let u = ctx.tuning.creature.brute.unreach_t;
            chase_tick(ctx, h, t, u)
        }
        _ => {}
    }
}

/// `hunter.js:BRUTE_FSM`.
pub const BRUTE_FSM: Fsm = Fsm { row, tick };

/// `hunter.js:bruteFlash` — no state change: `flashResisted` and the HUD line.
pub fn on_flash(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    ctx.emit(SimEvent::FlashResisted {
        hunter_id: h.id,
        profile: h.profile.js_name().to_string(),
    });
    ctx.emit(SimEvent::toast("It does not flinch"));
    FlashResult::None
}

/// `hunter.js:bruteReset` — the leash is BFS over static solids only (it never moves with lanterns).
pub fn on_reset(ctx: &mut Ctx, h: &mut Hunter) {
    h.leash = bfs_solid(ctx.map(), h.home.cx, h.home.cz).dist;
    h.step_t = 0.0;
}

/// `hunter.js:bruteNearLantern` — smash any planted lantern within `smashR`: `lanternRemoved` (the shell
/// removes it and recomputes the pools) then `lanternSmashed`, the toast and the ember burst. One smash per
/// call; a lantern already in `smashed` this frame is skipped.
pub fn near_lantern(ctx: &mut Ctx, h: &mut Hunter, smashed: &mut Vec<usize>) {
    let smash_r = ctx.tuning.creature.brute.smash_r;
    let life = ctx.tuning.creature.brute.embers.life;
    for (i, &(lx, lz)) in ctx.env.lanterns.iter().enumerate() {
        if smashed.contains(&i) || dist2d(lx, lz, h.x, h.z) > smash_r {
            continue;
        }
        smashed.push(i);
        ctx.emit(SimEvent::LanternRemoved { x: lx, z: lz });
        ctx.emit(SimEvent::LanternSmashed {
            hunter_id: h.id,
            x: lx,
            z: lz,
        });
        ctx.emit(SimEvent::toast("It smashed your lantern"));
        h.anim.burst_t = life;
        h.anim.burst_x = lx;
        h.anim.burst_z = lz;
        h.anim.burst_fired = true;
        return; // one smash per tick
    }
}

/// `hunter.js:bruteAnim` — the turn-rate limit on the rendered yaw (it corners badly), the stride sway and
/// leg swing, and the ember burst timer.
pub fn anim(_env: &Env, tuning: &Tuning, h: &mut Hunter, dt: f32) {
    let br = &tuning.creature.brute;
    let mut d = h.yaw - h.yaw_vis;
    while d > PI {
        d -= TAU;
    }
    while d < -PI {
        d += TAU;
    }
    let max_t = br.turn_rate * dt;
    h.yaw_vis += d.clamp(-max_t, max_t);
    if h.moved {
        h.step_phase += dt * if h.state == HState::Chase { 14.0 } else { 10.5 };
    }
    let sw = if h.moved { h.step_phase.sin() } else { 0.0 };
    h.anim.sway = 0.06 * sw;
    h.anim.leg_phase = h.step_phase;
    h.anim.leg_swing = if h.moved { 0.35 } else { 0.0 };
    if h.anim.burst_t > 0.0 {
        h.anim.burst_t = (h.anim.burst_t - dt).max(0.0);
    }
}
