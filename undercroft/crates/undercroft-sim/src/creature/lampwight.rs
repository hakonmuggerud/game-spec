//! The Lampwight — the light-drinker (`hunter.js` ~357–391 `LAMPWIGHT_FSM` and the `lampwight` profile entry,
//! DESIGN.md §5.1). Senses only a lit lamp (24 u + LOS); it snuffs the flame instead of killing.

use super::driver::{enter_chase, pick_wander, set_state};
use super::profiles::{Fsm, Move, StateRow};
use super::record::{HState, Hunter};
use super::{Ctx, Env, FlashResult, Stim, Tuning};
use crate::events::SimEvent;
use crate::player::PlayerView;

/// `LAMPWIGHT_FSM` rows.
pub fn row(s: HState) -> Option<StateRow> {
    Some(match s {
        HState::Drift => StateRow::new(Move::Path),
        HState::Drawn => StateRow::new(Move::Path)
            .repath()
            .close()
            .catch()
            .last_known_when_blind(),
        HState::Snuff => StateRow::new(Move::Still).ignore_stim(),
        HState::Sated => StateRow::new(Move::Path).ignore_stim(),
        HState::Staggered => StateRow::new(Move::Still).ignore_stim(),
        _ => return None,
    })
}

fn tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    let tick = ctx.tuning.hunter.tick;
    let tuning = ctx.tuning;
    let lw = &tuning.creature.lampwight;
    match h.state {
        HState::Drift => {
            if t.is_some() {
                return enter_chase(ctx, h, HState::Drawn);
            }
            if h.path.is_empty() {
                h.idle_t -= tick;
                if h.idle_t <= 0.0 {
                    pick_wander(ctx, h, false, 0);
                }
            }
        }
        HState::Drawn => {
            if t.is_none() {
                h.no_stim_t += tick;
                if h.no_stim_t >= ctx.prof(h.profile).lose_t {
                    // keeps its path to lastKnown
                    set_state(ctx, h, HState::Drift);
                    h.idle_t = 1.0 + ctx.rng.unit() as f32;
                    return;
                }
            }
            if h.unreachable {
                h.unreach_t += tick;
                if h.unreach_t >= ctx.tuning.hunter.unreach_t {
                    set_state(ctx, h, HState::Drift);
                    pick_wander(ctx, h, true, 0);
                }
            } else {
                h.unreach_t = 0.0;
            }
        }
        HState::Snuff => {
            h.t -= tick;
            if !h.snuffed && lw.snuff_t - h.t >= lw.snuff_at - 1e-6 {
                h.snuffed = true;
                h.ember_k = 1.5;
                ctx.emit(SimEvent::LampSnuffed {
                    hunter_id: h.id,
                    oil: lw.oil,
                    lockout: lw.lockout,
                    x: h.x,
                    z: h.z,
                });
            }
            if h.t <= 0.0 {
                set_state(ctx, h, HState::Sated);
                h.t = lw.sated_t;
                pick_wander(ctx, h, false, lw.sated_cells);
            }
        }
        HState::Sated => {
            h.t -= tick;
            if h.path.is_empty() {
                h.idle_t -= tick;
                if h.idle_t <= 0.0 {
                    pick_wander(ctx, h, false, lw.sated_cells);
                }
            }
            if h.t <= 0.0 {
                set_state(ctx, h, HState::Drift);
                h.idle_t = 1.0;
            }
        }
        HState::Staggered => {
            h.stagger_t -= tick;
            if h.stagger_t <= 0.0 {
                set_state(ctx, h, HState::Drift);
                h.daze_t = lw.daze_t;
                pick_wander(ctx, h, true, 0);
            }
        }
        _ => {}
    }
}

/// `hunter.js:LAMPWIGHT_FSM`.
pub const LAMPWIGHT_FSM: Fsm = Fsm { row, tick };

/// The `lampwight` profile's `onFlash`: STAGGERED for `CREATURE.lampwight.staggerT`.
pub fn on_flash(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    if h.state == HState::Staggered {
        return FlashResult::None;
    }
    set_state(ctx, h, HState::Staggered);
    h.stagger_t = ctx.tuning.creature.lampwight.stagger_t;
    h.daze_t = 0.0;
    h.path.clear();
    FlashResult::Stagger
}

/// `catchIf: (h, p) => p.lampOn` — only a lit lamp can be snuffed.
pub fn catch_if(_h: &Hunter, p: &PlayerView) -> bool {
    p.lamp_on
}

/// `onCatch` — the touch: no death, it drinks the flame (SNUFF for `snuffT`).
pub fn on_catch(ctx: &mut Ctx, h: &mut Hunter) {
    set_state(ctx, h, HState::Snuff);
    h.t = ctx.tuning.creature.lampwight.snuff_t;
    h.snuffed = false;
    h.path.clear();
}

/// `anim` — it bobs 0.05 u at 0.7 Hz; the chest ember decays from 1.5 after the snuff.
pub fn anim(env: &Env, _t: &Tuning, h: &mut Hunter, dt: f32) {
    h.y = 0.05 * (env.time * std::f32::consts::TAU * 0.7 + h.phase).sin();
    if h.state != HState::Snuff && h.ember_k > 0.0 {
        h.ember_k = (h.ember_k - dt * 0.3).max(0.0);
    }
    h.anim.ember_k = h.ember_k;
}
