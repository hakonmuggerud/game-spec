//! `audio.js:tick` and `presenceTick` — the 30 Hz parameter push for the continuous layers.
//!
//! One [`FixedUpdate`] system reads the game state exactly as the prototype's `update(ctx, dt)`
//! did (mode flags, `ctx.player`, `ctx.zone`, `ctx.hunters`, `ctx.hub.flame`), fills a
//! [`Params`](super::synth::Params) block for the audio thread and queues the timed one-shots
//! (lamp pops, hub flame pops, drips, footsteps, plops, scrabbling, Warden treads and reversal
//! clicks) into [`OneShotQueue`](super::oneshots::OneShotQueue).

use bevy::prelude::*;
use std::f32::consts::PI;

use undercroft_sim::creature::{HState, Hunter};

use super::oneshots::{OneShot, OneShotQueue};
use super::synth::{VoiceKind, VoiceParams, MAX_PRESENCE};
use super::*;
use crate::resources::{Game, HubMapRes, LampRes, Player, ZoneRes};
use crate::state::{GameMode, Mode, PrevMode};

/// The per-voice state `audio.js` kept on the voice objects between ticks (`lastYaw`, `lastDir`,
/// `stepT`, `extStepT`, `lastX`/`lastZ`/`speed`, `plopT`, `scrabT`).
#[derive(Debug, Clone)]
pub struct PresenceState {
    pub kind: VoiceKind,
    /// Warden: last rendered facing and sweep direction, and the tread timer.
    pub last_yaw: Option<f32>,
    pub last_dir: f32,
    pub step_t: f32,
    /// Game time of the last `creatureStep` event for this Warden (treads defer to it for 1.5 s).
    pub ext_step_t: f32,
    /// Drowner: last position and the smoothed swim speed, plus the plop timer.
    pub last_pos: Option<(f32, f32)>,
    pub speed: f32,
    pub plop_t: f32,
    /// False Light: the scrabbling timer.
    pub scrab_t: f32,
}

impl Default for PresenceState {
    fn default() -> Self {
        PresenceState {
            kind: VoiceKind::Growl,
            last_yaw: None,
            last_dir: 0.0,
            step_t: 0.0,
            ext_step_t: -1e9,
            last_pos: None,
            speed: 0.0,
            plop_t: 2.0,
            scrab_t: 0.0,
        }
    }
}

/// `V.presence` — one entry per hunter index, rebuilt when the profile at that index changes.
#[derive(Resource, Debug, Default)]
pub struct PresenceStates(pub Vec<PresenceState>);

impl PresenceStates {
    /// Grow to `n` entries.
    fn ensure(&mut self, n: usize) {
        while self.0.len() < n {
            self.0.push(PresenceState::default());
        }
    }

    /// The state of the voice a hunter id maps to, if the zone still has it.
    pub fn by_index(&mut self, i: usize) -> Option<&mut PresenceState> {
        self.0.get_mut(i)
    }
}

/// `audio.js:update(c, dt)` — accumulate to `TUNE.tick` and run one parameter push.
#[allow(clippy::too_many_arguments)]
pub fn tick(
    time: Res<Time>,
    mode: Mode,
    prev: Res<PrevMode>,
    clock: Res<crate::tick::Clock>,
    game: Game,
    player: Res<Player>,
    lamp: Res<LampRes>,
    zone: Res<ZoneRes>,
    hub_map: Res<HubMapRes>,
    hub: Res<crate::resources::HubRes>,
    settings: Res<AudioSettings>,
    params: Res<AudioParams>,
    mut rt: ResMut<AudioRuntime>,
    mut states: ResMut<PresenceStates>,
    mut queue: ResMut<OneShotQueue>,
) {
    rt.acc += time.delta_secs();
    if rt.acc < TUNE_TICK {
        return;
    }
    let dt = rt.acc;
    rt.acc = 0.0;
    let Some(asset) = game.get() else {
        return;
    };
    let audio_cfg = &asset.data.config.audio;

    let m = mode.get();
    let prev_mode = prev.0;
    // `audio.js:tick` — `MENU` counts as whichever mode it was opened from; `paused` is a mode
    // here, so `live` is simply "the world is running".
    let in_zone = matches!(m, GameMode::Zone | GameMode::Dying | GameMode::Ending)
        || (m == GameMode::Menu && prev_mode == Some(GameMode::Zone));
    let in_hub = matches!(m, GameMode::Hub | GameMode::Title)
        || (m == GameMode::Menu && prev_mode == Some(GameMode::Hub));
    let live = matches!(m, GameMode::Zone | GameMode::Hub);
    let zone_id = zone.id().unwrap_or("");

    let Ok(mut p) = params.0.lock() else {
        return;
    };

    // master: volume, mute, ducking in menus / on the death screen
    let duck = if m == GameMode::Menu {
        TUNE_DUCK_MENU
    } else if m == GameMode::Dead {
        TUNE_DUCK_DEAD
    } else {
        1.0
    };
    p.master = settings.master(duck);
    if settings.muted {
        // everything else may idle at its last target; one-shots are skipped in `play_queue`
        return;
    }

    // ---- ambience drone ----
    p.drone_base = rt.drone_base;
    p.drone = if in_zone {
        audio_cfg.drone_zone
            + if zone_id == "source" {
                TUNE_DRONE_SOURCE * player.lap.max(0) as f32
            } else {
                0.0
            }
    } else if in_hub || m == GameMode::Dead {
        audio_cfg.drone_hub
    } else {
        0.0
    };
    p.drone_water = if in_zone && zone_id == "cistern" {
        TUNE_DRONE_WATER
    } else {
        0.0
    };

    // ---- lamp crackle + pops ----
    {
        let lit = lamp.0.lamp_on && matches!(m, GameMode::Zone | GameMode::Dying | GameMode::Menu);
        let low = lamp.0.oil < TUNE_LAMP_LOW_OIL;
        p.lamp = if lit {
            TUNE_LAMP_CRACKLE * if low { TUNE_LAMP_LOW_MUL } else { 1.0 }
        } else {
            0.0
        };
        if lit && m == GameMode::Zone {
            rt.pop_t -= dt;
            if rt.pop_t <= 0.0 {
                rt.pop_t = rt.rnd(TUNE_POP_MIN, TUNE_POP_MAX);
                queue.push(OneShot::new("pop").gain(if low { 1.6 } else { 1.0 }));
            }
        }
    }

    // ---- footsteps by distance walked (position deltas; teleports ignored) ----
    {
        if live {
            if let Some((lx, lz)) = rt.last_pos {
                let d = (player.x - lx).hypot(player.z - lz);
                if d < TUNE_STEP_TELEPORT && player.moving {
                    rt.step_acc += d;
                    let len = if player.sprinting {
                        TUNE_STEP_SPRINT
                    } else {
                        TUNE_STEP_WALK
                    };
                    if rt.step_acc >= len {
                        rt.step_acc -= len;
                        rt.steps += 1;
                        queue.push(OneShot::new(if player.sprinting {
                            "stepSprint"
                        } else {
                            "step"
                        }));
                        if player.in_water {
                            queue.push(OneShot::new("slosh"));
                        }
                    }
                } else if d >= TUNE_STEP_TELEPORT {
                    rt.step_acc = 0.0;
                }
            }
        }
        rt.last_pos = Some((player.x, player.z));
        p.water = if live && player.in_water {
            TUNE_WATER_WASH
        } else {
            0.0
        };
    }

    // ---- creature presence ----
    presence_tick(
        &mut p,
        &mut rt,
        &mut states,
        &mut queue,
        zone.get().map(|z| z.hunters.as_slice()).unwrap_or(&[]),
        &player,
        m,
        live,
        audio_cfg.presence_max,
        audio_cfg.presence_range,
        clock.time,
        dt,
    );

    // ---- hub flame: crackle + hum by tier, faded by distance ----
    {
        let flame = hub_map
            .0
            .as_ref()
            .and_then(|h| h.map.flame.as_ref())
            .filter(|_| in_hub);
        let (mut g, mut hum, mut tier, mut prox) = (0.0, 0.0, 1u32, 0.0);
        if let Some(f) = flame {
            tier = hub.0.tier.max(1);
            prox = (1.0 - (player.x - f.x).hypot(player.z - f.z) / TUNE_HUB_RANGE).clamp(0.0, 1.0);
            g = (TUNE_HUB_CRACKLE + TUNE_HUB_CRACKLE_STEP * (tier - 1) as f32) * prox;
            hum = TUNE_HUB_SINE * tier as f32 * prox;
        }
        p.hub = g;
        p.hub_hum = hum;
        if g > 0.002 && m == GameMode::Hub {
            rt.hub_pop_t -= dt;
            if rt.hub_pop_t <= 0.0 {
                rt.hub_pop_t = rt.rnd(0.15, 0.5) / tier as f32;
                // `play('hubPop', {peak: 0.05 * prox})` against the table's default peak of 0.04
                queue.push(OneShot::new("hubPop").gain(0.05 * prox / 0.04));
            }
        }
    }

    // ---- drips (zone timer) ----
    if m == GameMode::Zone {
        rt.drip_t -= dt;
        if rt.drip_t <= 0.0 {
            rt.drip_t = if zone_id == "cistern" {
                rt.rnd(TUNE_DRIP_CISTERN_MIN, TUNE_DRIP_CISTERN_MAX)
            } else {
                rt.rnd(TUNE_DRIP_MIN, TUNE_DRIP_MAX)
            };
            // `drip` pans itself randomly (`pan: rnd(-0.8, 0.8)`)
            let pan = rt.rnd(-0.8, 0.8);
            queue.push(OneShot::new("drip").pan(pan));
        }
    }
}

/// `audio.js:presenceTick` — one voice per hunter index, its kind chosen by profile.
#[allow(clippy::too_many_arguments)]
fn presence_tick(
    p: &mut synth::Params,
    rt: &mut AudioRuntime,
    states: &mut PresenceStates,
    queue: &mut OneShotQueue,
    hunters: &[Hunter],
    player: &Player,
    mode: GameMode,
    live: bool,
    presence_max: f32,
    presence_range: f32,
    now: f32,
    dt: f32,
) {
    let n = hunters.len().max(states.0.len());
    states.ensure(n);
    p.presence.clear();
    for i in 0..n {
        let h = hunters.get(i);
        let kind = match h {
            Some(h) => kind_of(h.profile),
            None => states.0[i].kind,
        };
        if states.0[i].kind != kind {
            states.0[i] = PresenceState {
                kind,
                ..PresenceState::default()
            };
        }
        let on = h.map(|h| h.active).unwrap_or(false) && mode == GameMode::Zone;
        let (d, pan) = match h.filter(|_| on) {
            Some(h) => (
                (h.x - player.x).hypot(h.z - player.z),
                pan_at(player.x, player.z, player.yaw, h.x, h.z),
            ),
            None => (1e9, 0.0),
        };
        let state = h.map(|h| h.state).unwrap_or(HState::Wander);
        let mut k = 0.0;
        let mut gain;
        let mut cutoff = 120.0;

        match kind {
            VoiceKind::Growl => {
                if on {
                    k = falloff(d, presence_range) * growl_state_mul(state);
                }
                gain = presence_max * k;
                cutoff = 120.0 + 500.0 * k;
            }
            VoiceKind::Whistle => {
                if on {
                    k = falloff(d, whistle::RANGE) * whistle::mul(state);
                }
                gain = whistle::PEAK * k;
            }
            VoiceKind::Grind => {
                let v = &mut states.0[i];
                let facing = if on {
                    h.map(|h| h.yaw).unwrap_or(0.0)
                } else {
                    0.0
                };
                let mut speed = 0.0;
                if on && dt > 0.0 {
                    if let Some(last) = v.last_yaw {
                        let mut dy = facing - last;
                        while dy > PI {
                            dy -= PI * 2.0;
                        }
                        while dy < -PI {
                            dy += PI * 2.0;
                        }
                        speed = dy.abs() / dt;
                        let dir = if dy.abs() > 1e-4 { dy.signum() } else { 0.0 };
                        if dir != 0.0 && v.last_dir != 0.0 && dir != v.last_dir && d <= grind::RANGE
                        {
                            // reversal click, `play('wardenClick', {gain: 0.05 * falloff(...)})`
                            queue.push(
                                OneShot::new("wardenClick")
                                    .gain(falloff(d, grind::RANGE))
                                    .pan(pan),
                            );
                        }
                        if dir != 0.0 {
                            v.last_dir = dir;
                        }
                    }
                }
                v.last_yaw = if on { Some(facing) } else { None };
                if on {
                    k = falloff(d, grind::RANGE) * (speed / grind::SWEEP).clamp(0.0, 1.0);
                }
                gain = grind::PEAK * k;
                // treads: CHASE every 0.45 s, RETURN every 0.6 s, unless `creatureStep` drives them
                let driven_externally = now - v.ext_step_t <= grind::EXT_STEP_HOLD;
                if on
                    && live
                    && matches!(state, HState::Chase | HState::Return)
                    && d <= grind::STEP_RANGE
                    && !driven_externally
                {
                    v.step_t -= dt;
                    if v.step_t <= 0.0 {
                        v.step_t = if state == HState::Chase {
                            grind::STEP_CHASE
                        } else {
                            grind::STEP_RETURN
                        };
                        queue.push(
                            OneShot::new("wardenStep")
                                .gain(falloff(d, grind::STEP_RANGE))
                                .pan(pan),
                        );
                    }
                } else if !on {
                    v.step_t = 0.0;
                }
            }
            VoiceKind::Wash => {
                let v = &mut states.0[i];
                if on && dt > 0.0 {
                    if let (Some((lx, lz)), Some(h)) = (v.last_pos, h) {
                        let s = (h.x - lx).hypot(h.z - lz) / dt;
                        if s < 40.0 {
                            v.speed = s;
                        }
                    }
                }
                v.last_pos = if on { h.map(|h| (h.x, h.z)) } else { None };
                let mul = if on { wash::mul(state) } else { 0.0 };
                if on {
                    k = falloff(d, wash::RANGE)
                        * mul
                        * (0.35 + v.speed / wash::SPEED).clamp(0.0, 1.0);
                }
                gain = wash::PEAK * k;
                if on && live && d <= wash::PLOP_RANGE {
                    v.plop_t -= dt;
                    if v.plop_t <= 0.0 {
                        v.plop_t = rt.rnd(wash::PLOP_MIN, wash::PLOP_MAX);
                        queue.push(
                            OneShot::new("plop")
                                .gain(falloff(d, wash::PLOP_RANGE))
                                .pan(pan),
                        );
                    }
                }
            }
            VoiceKind::Lure => {
                let v = &mut states.0[i];
                // the presence stays 0: the lure layer is the only sound a False Light makes
                let lit = on && state == HState::Lit && d <= lure::RANGE;
                gain = if lit {
                    (lure::CRACKLE * TUNE_LAMP_CRACKLE + lure::CHIME) * falloff(d, lure::RANGE)
                } else {
                    0.0
                };
                if on && live && state == HState::Pounce && d <= lure::SCRABBLE_RANGE {
                    v.scrab_t -= dt;
                    if v.scrab_t <= 0.0 {
                        v.scrab_t = 1.0 / lure::SCRABBLE_HZ;
                        queue.push(
                            OneShot::new("scrabble")
                                .gain(falloff(d, lure::SCRABBLE_RANGE))
                                .pan(pan),
                        );
                    }
                } else {
                    v.scrab_t = 0.0;
                }
            }
            VoiceKind::Breath => {
                if on {
                    k = falloff(d, breath::RANGE) * breath::mul(state);
                }
                gain = breath::PEAK * k;
            }
        }
        if !on {
            gain = 0.0;
        }
        p.presence.push(VoiceParams {
            kind,
            gain,
            pan,
            cutoff,
        });
    }
    // The audio thread renders a bounded number of voices; keep the loudest (the JS had no cap).
    if p.presence.len() > MAX_PRESENCE {
        p.presence.sort_by(|a, b| b.gain.total_cmp(&a.gain));
        p.presence.truncate(MAX_PRESENCE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footstep_interval_is_shorter_when_sprinting() {
        const { assert!(TUNE_STEP_SPRINT < TUNE_STEP_WALK) };
        // walking 1.1 m makes two steps, sprinting the same distance makes two as well but with
        // 0.26 m left over instead of 0.0 — the accumulator, not a timer, drives them.
        let steps = |len: f32, dist: f32| (dist / len).floor() as u32;
        assert_eq!(steps(TUNE_STEP_WALK, 1.1), 2);
        assert_eq!(steps(TUNE_STEP_SPRINT, 1.1), 2);
        assert_eq!(steps(TUNE_STEP_WALK, 2.0), 3);
        assert_eq!(steps(TUNE_STEP_SPRINT, 2.0), 4);
    }
}
