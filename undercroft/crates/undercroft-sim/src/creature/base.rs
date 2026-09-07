//! The base FSM (`hunter.js:BASE_FSM`, ~322–355): WANDER / INVESTIGATE / CHASE / STAGGERED for `base` and
//! `fast`; the Brute reuses WANDER / INVESTIGATE without STAGGERED (`brute.rs`). Also the default movement
//! predicates `notBlocked` / `notSolid` and `baseFlash`.

use super::driver::{enter_chase, investigate, pick_wander, set_state, stagger};
use super::profiles::{Fsm, Move, StateRow};
use super::record::{HState, Hunter};
use super::{Ctx, Env, FlashResult, Stim};
use crate::grid::{is_blocked, is_solid};

/// `notBlocked` — every profile's default `canEnter`: not solid, not a pool cell.
pub fn not_blocked(env: &Env, _h: &Hunter, cx: i32, cz: i32) -> bool {
    !is_blocked(env.map, env.pool, cx, cz)
}

/// `notSolid` — every profile's default `canEnterLoose` (the rules minus the pool one, `escapeIfTrapped`
/// only) and the Brute's `canEnter`.
pub fn not_solid(env: &Env, _h: &Hunter, cx: i32, cz: i32) -> bool {
    !is_solid(env.map, cx, cz)
}

/// `hunter.js:baseFlash`.
pub fn base_flash(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    if h.state == HState::Staggered {
        return FlashResult::None;
    }
    stagger(ctx, h);
    FlashResult::Stagger
}

/// `BASE_FSM` rows.
pub fn base_row(s: HState) -> Option<StateRow> {
    Some(match s {
        HState::Wander => StateRow::new(Move::Path).catch(),
        HState::Investigate => StateRow::new(Move::Path).catch().sweep(),
        HState::Chase => StateRow::new(Move::Path).repath().close().catch(),
        HState::Staggered => StateRow::new(Move::Still),
        _ => return None,
    })
}

/// `hunter.js:wanderTick` — chase on a stimulus, else idle then pick a near wander cell.
pub fn wander_tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    if t.is_some() {
        return enter_chase(ctx, h, HState::Chase);
    }
    if h.path.is_empty() {
        h.idle_t -= ctx.tuning.hunter.tick;
        if h.idle_t <= 0.0 {
            pick_wander(ctx, h, false, 0);
        }
    }
}

/// `BASE_FSM.INVESTIGATE.tick` — wait `waitT` at the spot, then wander (far when the spot was unreachable).
pub fn investigate_tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    if t.is_some() {
        return enter_chase(ctx, h, HState::Chase);
    }
    if h.path.is_empty() {
        h.wait_t -= ctx.tuning.hunter.tick;
        if h.wait_t <= 0.0 {
            set_state(ctx, h, HState::Wander);
            let far = h.wander_far;
            pick_wander(ctx, h, far, 0);
            h.wander_far = false;
        }
    }
}

/// `BASE_FSM.CHASE.tick` with the give-up timer `unreach_t` (`HUNTER.unreachT` for base, `CREATURE.brute.unreachT`
/// for the Brute).
pub fn chase_tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>, unreach_t: f32) {
    let tick = ctx.tuning.hunter.tick;
    if t.is_none() {
        h.no_stim_t += tick;
        if h.no_stim_t >= ctx.prof(h.profile).lose_t {
            let (x, z) = h.last_known;
            return investigate(ctx, h, x, z);
        }
    }
    if h.unreachable {
        h.unreach_t += tick;
        if h.unreach_t >= unreach_t {
            set_state(ctx, h, HState::Wander);
            pick_wander(ctx, h, true, 0);
        }
    } else {
        h.unreach_t = 0.0;
    }
}

/// `BASE_FSM.STAGGERED.tick` — frozen `staggerT`, then WANDER far, dazed `dazeT`.
pub fn staggered_tick(ctx: &mut Ctx, h: &mut Hunter) {
    h.stagger_t -= ctx.tuning.hunter.tick;
    if h.stagger_t <= 0.0 {
        set_state(ctx, h, HState::Wander);
        h.daze_t = ctx.tuning.hunter.daze_t;
        pick_wander(ctx, h, true, 0);
    }
}

fn base_tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    match h.state {
        HState::Wander => wander_tick(ctx, h, t),
        HState::Investigate => investigate_tick(ctx, h, t),
        HState::Chase => {
            let u = ctx.tuning.hunter.unreach_t;
            chase_tick(ctx, h, t, u)
        }
        HState::Staggered => staggered_tick(ctx, h),
        _ => {}
    }
}

/// `hunter.js:BASE_FSM`.
pub const BASE_FSM: Fsm = Fsm {
    row: base_row,
    tick: base_tick,
};
