//! The Warden — the sentinel (`hunter.js` ~393–485 `wardenSense`, `WARDEN_FSM`, `wardenFlash`, `wardenReset`,
//! `wardenAnim`; DESIGN.md §5.2). A sweeping cone at a post; it never leaves its territory.

use super::driver::{enter_chase, field_from, path_to_point, set_state};
use super::profiles::{Fsm, Move, StateRow};
use super::record::{HState, Hunter, Target};
use super::{Ctx, Env, FlashResult, Stim, Tuning};
use crate::events::SimEvent;
use crate::grid::{dist2d, is_blocked, los};
use std::f32::consts::{PI, TAU};

const DEG: f32 = PI / 180.0;

/// `hunter.js:angDiff` — the absolute wrapped difference of two angles.
pub fn ang_diff(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > PI {
        d -= TAU;
    }
    while d < -PI {
        d += TAU;
    }
    d.abs()
}

/// `hunter.js:wardenSense` — SENTRY: a lit lamp within `near` at any angle, or within `reach` inside the cone
/// with LOS. ALERT / CHASE / RETURN: a lit lamp with LOS anywhere in the territory, or sprinting within
/// `senses.sprint`. Never the follower.
pub fn sense(ctx: &Ctx, h: &Hunter) -> Option<Stim> {
    let env = ctx.env;
    let p = env.player;
    let m = env.map;
    let wd = &ctx.tuning.creature.warden;
    let lit = p.lit();
    let d = dist2d(p.x, p.z, h.x, h.z);
    let reach = h.opts.reach.unwrap_or(wd.reach);
    let cone = h.opts.cone.unwrap_or(wd.cone) * DEG;
    let hit = if h.state == HState::Sentry {
        (lit && d <= wd.near) // it feels the heat
            || (lit
                && d <= reach
                && ang_diff((-(p.x - h.x)).atan2(-(p.z - h.z)), h.yaw) <= cone
                && los(m, h.x, h.z, p.x, p.z))
    } else {
        (lit && h.in_territory(p.x, p.z) && los(m, h.x, h.z, p.x, p.z))
            || (p.sprinting && d <= ctx.prof(h.profile).senses.sprint)
    };
    hit.then(|| Stim::at(Target::Player, None, p.x, p.z, d))
}

/// `hunter.js:enterReturn` — walk back to the post, `wardenReturn`.
fn enter_return(ctx: &mut Ctx, h: &mut Hunter) {
    set_state(ctx, h, HState::Return);
    h.no_stim_t = 0.0;
    h.unreachable = false;
    h.unreach_t = 0.0;
    let field = field_from(ctx, h);
    let (px, pz) = h.post;
    path_to_point(ctx.env, h, &field, px, pz);
    ctx.emit(SimEvent::WardenReturn { hunter_id: h.id });
}

/// `WARDEN_FSM` rows.
pub fn row(s: HState) -> Option<StateRow> {
    Some(match s {
        HState::Sentry => StateRow::new(Move::Still),
        HState::Alert => StateRow::new(Move::Still),
        HState::Chase => StateRow::new(Move::Path)
            .repath()
            .close()
            .catch()
            .last_known_when_blind(),
        HState::Return => StateRow::new(Move::Path).catch(),
        HState::Flinch => StateRow::new(Move::Still).ignore_stim(),
        _ => return None,
    })
}

fn tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    let tuning = ctx.tuning;
    let tick = tuning.hunter.tick;
    let wd = &tuning.creature.warden;
    match h.state {
        HState::Sentry => {
            if t.is_some() {
                set_state(ctx, h, HState::Alert);
                h.t = wd.alert_t;
                ctx.emit(SimEvent::WardenAlert {
                    hunter_id: h.id,
                    x: h.x,
                    z: h.z,
                });
                ctx.emit(SimEvent::toast("The Warden has seen your light"));
            }
        }
        HState::Alert => {
            h.t -= tick;
            if h.t <= 0.0 {
                enter_chase(ctx, h, HState::Chase);
            }
        }
        HState::Chase => {
            let (tx, tz, _) = super::senses::target_pos(ctx.env, h);
            if !h.in_territory(tx, tz) {
                return enter_return(ctx, h);
            }
            if t.is_none() {
                h.no_stim_t += tick;
                if h.no_stim_t >= ctx.prof(h.profile).lose_t {
                    return enter_return(ctx, h);
                }
            }
            if h.unreachable {
                h.unreach_t += tick;
                if h.unreach_t >= wd.give_up_t {
                    enter_return(ctx, h);
                }
            } else {
                h.unreach_t = 0.0;
            }
        }
        HState::Return => {
            if let Some(t) = t {
                if h.in_territory(t.x, t.z) {
                    return enter_chase(ctx, h, HState::Chase);
                }
            }
            if dist2d(h.x, h.z, h.post.0, h.post.1) <= wd.at_post {
                h.x = h.post.0;
                h.z = h.post.1;
                h.path.clear();
                set_state(ctx, h, HState::Sentry);
                return;
            }
            if h.path.is_empty() {
                let field = field_from(ctx, h);
                let (px, pz) = h.post;
                path_to_point(ctx.env, h, &field, px, pz);
                // no way home (moved outside its ground by a debug teleport): it is simply back at the post
                if h.path.is_empty() {
                    h.x = px;
                    h.z = pz;
                }
            }
        }
        HState::Flinch => {
            h.t -= tick;
            if h.t <= 0.0 {
                let s = match h.prev_state {
                    Some(s) if s != HState::Flinch => s,
                    _ => HState::Sentry,
                };
                set_state(ctx, h, s);
            }
        }
        _ => {}
    }
}

/// `hunter.js:WARDEN_FSM`.
pub const WARDEN_FSM: Fsm = Fsm { row, tick };

/// `hunter.js:wardenFlash` — FLINCH for `flinchT`, then the previous state resumes (path kept).
pub fn on_flash(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    if h.state != HState::Flinch {
        h.prev_state = Some(h.state);
    }
    set_state(ctx, h, HState::Flinch);
    h.t = ctx.tuning.creature.warden.flinch_t;
    FlashResult::Flinch
}

/// The `warden` profile's `canEnter`: not blocked and within the territory of the post.
pub fn can_enter(env: &Env, h: &Hunter, cx: i32, cz: i32) -> bool {
    !is_blocked(env.map, env.pool, cx, cz)
        && dist2d(
            (env.map.ox + cx) as f32 + 0.5,
            cz as f32 + 0.5,
            h.post.0,
            h.post.1,
        ) <= h.territory()
}

/// `hunter.js:wardenReset` — post at home, facing from `opts.facing`.
pub fn on_reset(ctx: &mut Ctx, h: &mut Hunter) {
    h.post = (h.home.x, h.home.z);
    h.post_yaw = h.opts.facing.map(|f| f.yaw()).unwrap_or(0.0);
    h.yaw = h.post_yaw;
    h.yaw_vis = h.yaw;
    h.sweep = 0.0;
    h.sweep_dir = 1.0;
    h.territory_default = ctx.tuning.creature.warden.territory;
    h.anim.light_on = true;
}

/// `hunter.js:wardenAnim` — the SENTRY sweep (a triangle wave of `±sweep` at `sweepRate`), the cone light
/// intensity per state, leg swing and the plinth.
pub fn anim(_env: &Env, tuning: &Tuning, h: &mut Hunter, dt: f32) {
    let wd = &tuning.creature.warden;
    let sweep = h.opts.sweep.unwrap_or(wd.sweep) * DEG;
    if h.state == HState::Sentry {
        h.sweep += h.sweep_dir * wd.sweep_rate * dt;
        if h.sweep > sweep {
            h.sweep = sweep;
            h.sweep_dir = -1.0;
        } else if h.sweep < -sweep {
            h.sweep = -sweep;
            h.sweep_dir = 1.0;
        }
        h.yaw = h.post_yaw + h.sweep;
    }
    let k = wd.light.int.get(h.state.js_name()).copied().unwrap_or(0.0);
    h.anim.light_k = k;
    h.anim.light_on = k > 0.0 && h.active;
    h.anim.glass_k = k / 2.0;
    if h.moved {
        h.step_phase += dt * 9.0;
    }
    h.anim.leg_phase = h.step_phase;
    h.anim.leg_swing = if h.moved { 0.3 } else { 0.0 };
    // the plinth is the post's stone, not the Warden's: it shows only while it stands on it
    h.anim.plinth = dist2d(h.x, h.z, h.post.0, h.post.1) <= wd.at_post + 0.05;
}
