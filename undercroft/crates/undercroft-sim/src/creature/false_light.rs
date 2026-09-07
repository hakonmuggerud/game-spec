//! The false light — the lantern that isn't (`hunter.js` ~612–697 `falseLightSense`, `pickRest`, `enterRetreat`,
//! `FALSELIGHT_FSM`, `falseLightFlash`, `falseLightAnim`; DESIGN.md §5.4). A proximity trap: LIT → DARK →
//! POUNCE (a straight lunge) → RETREAT to a scored rest spot → RELIGHT → LIT.

use super::driver::{field_from, set_state};
use super::profiles::{Fsm, Move, StateRow};
use super::record::{HState, Hunter, Target};
use super::{Ctx, Env, FlashResult, Stim, Tuning};
use crate::events::SimEvent;
use crate::grid::{
    bfs_blocked, cell_type, center, dist2d, in_bounds, is_solid, los, path_to, to_cell, DIRS4,
};
use undercroft_data::CellKind;

/// `hunter.js:falseLightSense` — while LIT: the player within `proximity` with LOS, lamp on or off.
pub fn sense(ctx: &Ctx, h: &Hunter) -> Option<Stim> {
    if h.state != HState::Lit {
        return None;
    }
    let env = ctx.env;
    let p = env.player;
    let d = dist2d(p.x, p.z, h.x, h.z);
    let prox = ctx.tuning.creature.false_light.proximity;
    (d <= prox && los(env.map, h.x, h.z, p.x, p.z))
        .then(|| Stim::at(Target::Player, None, p.x, p.z, d))
}

/// `hunter.js:pickRest` — a reachable non-solid, non-pool cell `restMin`–`restMax` BFS from the player without
/// LOS from them; nooks (+2 per solid 4-neighbour), loot within 2 cells (+3) and visibility from a floor cell
/// within 6 u (+1) score higher; one of the top `restTop` at random.
pub fn pick_rest(ctx: &mut Ctx, h: &Hunter) -> Option<usize> {
    let env = ctx.env;
    let m = env.map;
    let p = env.player;
    let tuning = ctx.tuning;
    let fl = &tuning.creature.false_light;
    let (pcx, pcz) = to_cell(m, p.x, p.z);
    if !in_bounds(m, pcx, pcz) {
        return None;
    }
    let from_player = bfs_blocked(m, env.pool, pcx, pcz); // player-side distances over solid ∪ pool
    let mine = field_from(ctx, h);
    let mut scored: Vec<(usize, f64)> = Vec::new();
    for (i, &md) in mine.dist.iter().enumerate() {
        if md < 0 {
            continue;
        }
        let d = from_player.dist[i] as i32;
        if d < fl.rest_min || d > fl.rest_max {
            continue;
        }
        let (cx, cz) = m.cell_of(i);
        let (cxw, czw) = center(m, cx, cz);
        if los(m, p.x, p.z, cxw, czw) {
            continue;
        }
        let mut score = 0.0f64;
        for (dx, dz) in DIRS4 {
            if is_solid(m, cx + dx, cz + dz) {
                score += 2.0;
            }
        }
        for &(ix, iz) in env.items {
            let (icx, icz) = to_cell(m, ix, iz);
            if (icx - cx).abs() <= 2 && (icz - cz).abs() <= 2 {
                score += 3.0;
                break;
            }
        }
        let mut seen = false;
        'outer: for z in (cz - 6).max(0)..=(cz + 6).min(m.h - 1) {
            for x in (cx - 6).max(0)..=(cx + 6).min(m.w - 1) {
                if cell_type(m, x, z) != CellKind::Floor
                    || ((x - cx) as f64).hypot((z - cz) as f64) > 6.0
                {
                    continue;
                }
                if los(m, (x + m.ox) as f32 + 0.5, z as f32 + 0.5, cxw, czw) {
                    seen = true;
                    break 'outer;
                }
            }
        }
        if seen {
            score += 1.0;
        }
        scored.push((i, score + ctx.rng.unit() * 0.01));
    }
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let top = (fl.rest_top.max(1) as usize).min(scored.len());
    Some(scored[ctx.rand_index(top)].0)
}

/// `hunter.js:enterRetreat` — pick a rest spot and path there (RELIGHT on the spot when none exists).
pub fn enter_retreat(ctx: &mut Ctx, h: &mut Hunter) {
    h.rest_idx = pick_rest(ctx, h);
    set_state(ctx, h, HState::Retreat);
    match h.rest_idx {
        Some(i) => {
            let field = field_from(ctx, h);
            h.path = path_to(ctx.map(), &field, i);
        }
        None => {
            h.path.clear();
            set_state(ctx, h, HState::Relight);
            h.t = ctx.tuning.creature.false_light.relight_t;
        }
    }
}

/// `FALSELIGHT_FSM` rows.
pub fn row(s: HState) -> Option<StateRow> {
    Some(match s {
        HState::Lit => StateRow::new(Move::Still),
        HState::Dark => StateRow::new(Move::Still).ignore_stim(),
        HState::Pounce => StateRow::new(Move::Lunge).catch().ignore_stim(),
        HState::Retreat => StateRow::new(Move::Path).ignore_stim(),
        HState::Relight => StateRow::new(Move::Still).ignore_stim(),
        HState::Revealed => StateRow::new(Move::Still).ignore_stim(),
        HState::Staggered => StateRow::new(Move::Still).ignore_stim(),
        _ => return None,
    })
}

fn tick(ctx: &mut Ctx, h: &mut Hunter, t: Option<&Stim>) {
    let tuning = ctx.tuning;
    let tick = tuning.hunter.tick;
    let fl = &tuning.creature.false_light;
    match h.state {
        HState::Lit => {
            if t.is_some() {
                set_state(ctx, h, HState::Dark);
                h.t = fl.dark_t;
                ctx.emit(SimEvent::FalseLightPounce {
                    hunter_id: h.id,
                    x: h.x,
                    z: h.z,
                });
            }
        }
        HState::Dark => {
            h.t -= tick;
            if h.t <= 0.0 {
                let p = ctx.env.player;
                h.lunge = (p.x, p.z);
                h.lunge_done = false;
                set_state(ctx, h, HState::Pounce);
                h.t = fl.pounce_t;
            }
        }
        HState::Pounce => {
            h.t -= tick;
            if h.t <= 0.0 || h.lunge_done {
                enter_retreat(ctx, h);
            }
        }
        HState::Retreat => {
            if h.path.is_empty() {
                set_state(ctx, h, HState::Relight);
                h.t = fl.relight_t;
            }
        }
        HState::Relight => {
            h.t -= tick;
            if h.t <= 0.0 {
                set_state(ctx, h, HState::Lit);
            }
        }
        HState::Revealed => {
            h.t -= tick;
            if h.t <= 0.0 {
                enter_retreat(ctx, h);
            }
        }
        HState::Staggered => {
            h.stagger_t -= tick;
            if h.stagger_t <= 0.0 {
                enter_retreat(ctx, h);
            }
        }
        _ => {}
    }
}

/// `hunter.js:FALSELIGHT_FSM`.
pub const FALSELIGHT_FSM: Fsm = Fsm { row, tick };

/// `hunter.js:falseLightFlash` — LIT: REVEALED (`falseLightReveal`); DARK / POUNCE: STAGGERED (abort); else nothing.
pub fn on_flash(ctx: &mut Ctx, h: &mut Hunter) -> FlashResult {
    let tuning = ctx.tuning;
    let fl = &tuning.creature.false_light;
    match h.state {
        HState::Lit => {
            let (reveal_t, x, z, id) = (fl.reveal_t, h.x, h.z, h.id);
            set_state(ctx, h, HState::Revealed);
            h.t = reveal_t;
            ctx.emit(SimEvent::FalseLightReveal {
                hunter_id: id,
                x,
                z,
            });
            ctx.emit(SimEvent::toast("It was never a lantern"));
            FlashResult::Reveal
        }
        HState::Dark | HState::Pounce => {
            let stagger_t = fl.stagger_t;
            set_state(ctx, h, HState::Staggered);
            h.stagger_t = stagger_t;
            h.path.clear();
            FlashResult::Abort
        }
        _ => FlashResult::None,
    }
}

/// `hunter.js:falseLightAnim` — the flicker, the light on only while LIT, facing the player in DARK / REVEALED
/// (the tell sits on its front face), the dark pose.
pub fn anim(env: &Env, tuning: &Tuning, h: &mut Hunter, _dt: f32) {
    let fl = &tuning.creature.false_light;
    let lit = h.state == HState::Lit;
    let f = 0.93 + 0.07 * (env.time * 13.0 + h.phase).sin();
    if matches!(h.state, HState::Dark | HState::Revealed) {
        let p = env.player;
        h.yaw = (-(p.x - h.x)).atan2(-(p.z - h.z));
        h.yaw_vis = h.yaw;
    }
    h.anim.light_k = if lit { fl.light.int * f } else { 0.0 };
    h.anim.light_on = lit && h.active;
    if h.posed_dark != Some(!lit) {
        h.posed_dark = Some(!lit); // legs splay, jaw drops, glass dies
    }
    h.anim.posed_dark = !lit;
    h.anim.glass_k = if lit { f } else { 0.0 };
}
