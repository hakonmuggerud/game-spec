//! The shared senses (`hunter.js:stimAt`, `genericSense`, `followerStim`, `targetPos`, `playerLit`). The
//! creature-specific senses live in their modules (`warden::sense`, `drowner::sense`, `false_light::sense`).

use super::record::{Hunter, Target};
use super::{Ctx, Env, Stim};
use crate::grid::{dist2d, los};
use undercroft_data::config::Senses;

/// `hunter.js:stimAt` — does something at `(x, z)` with these flags register from this hunter under ranges
/// `r`? Returns the distance, or `None`. Order matters: lamp (with LOS), sprint, water, walk, still.
#[allow(clippy::too_many_arguments)]
pub fn stim_at(
    env: &Env,
    h: &Hunter,
    x: f32,
    z: f32,
    lit: bool,
    sprint: bool,
    water: bool,
    moving: bool,
    still: bool,
    r: &Senses,
) -> Option<f32> {
    let d = dist2d(x, z, h.x, h.z);
    if lit && r.lamp > 0.0 && d <= r.lamp && los(env.map, h.x, h.z, x, z) {
        return Some(d);
    }
    if sprint && r.sprint > 0.0 && d <= r.sprint {
        return Some(d);
    }
    if water && r.water > 0.0 && d <= r.water {
        return Some(d);
    }
    if moving && r.walk > 0.0 && d <= r.walk {
        return Some(d);
    }
    if still && r.still > 0.0 && d <= r.still {
        return Some(d);
    }
    None
}

/// `hunter.js:genericSense` — the nearest stimulated target (the player, or the follower for profiles whose
/// `senses.follower` is true and that are not busy), `None` while dazed or when nothing registers.
pub fn generic_sense(ctx: &Ctx, h: &Hunter) -> Option<Stim> {
    if h.daze_t > 0.0 {
        return None; // still dazzled after a flash: walks its far-wander leg blind
    }
    let env = ctx.env;
    let p = env.player;
    let r = &ctx.prof(h.profile).senses;
    let mut best: Option<Stim> = None;
    if let Some(dp) = stim_at(
        env,
        h,
        p.x,
        p.z,
        p.lit(),
        p.sprinting,
        p.in_water && p.moving,
        p.moving,
        true,
        r,
    ) {
        best = Some(Stim::at(Target::Player, None, p.x, p.z, dp));
    }
    if r.follower && h.busy_t <= 0.0 {
        if let Some(f) = p.follower.as_ref().filter(|f| !f.in_pool) {
            // DESIGN-v2 §3: always "walking, lamp off"; lit when close to the lit player
            if let Some(df) = stim_at(env, h, f.x, f.z, f.lit, false, false, true, false, r) {
                if best.as_ref().is_none_or(|b| df < b.d) {
                    best = Some(Stim::at(Target::Npc, Some(f.id.clone()), f.x, f.z, df));
                }
            }
        }
    }
    best
}

/// Where the hunter's current target is (`hunter.js:targetPos`): `(x, z, in_pool)`. The follower falls back to
/// the player when it is gone (and the record's target is reset).
pub fn target_pos(env: &Env, h: &mut Hunter) -> (f32, f32, bool) {
    if h.target == Target::Npc {
        if let Some(f) = env.player.follower.as_ref() {
            return (f.x, f.z, f.in_pool);
        }
        h.target = Target::Player;
    }
    let p = env.player;
    (p.x, p.z, p.in_pool)
}
