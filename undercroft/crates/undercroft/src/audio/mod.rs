//! Audio lane: the port of `prototype/src/audio.js`.
//!
//! The prototype had no sound assets — every noise was a Web Audio graph built at play time. The
//! port keeps that split:
//!
//! - [`synth`] is `buildVoices` + the presence voice builders written as a `bevy_audio`
//!   [`Decodable`](bevy::audio::Decodable) source: oscillators, one looped 2 s noise buffer,
//!   biquads and `setTargetAtTime` gain smoothing, rendered on the audio thread.
//! - [`voices`] is `audio.js:tick` + `presenceTick`: a 30 Hz system that reads the game state and
//!   pushes [`synth::Params`] into the shared block, and queues the timed one-shots (lamp pops,
//!   drips, footsteps, plops, scrabbling, Warden treads).
//! - [`oneshots`] is the `SOUNDS` table. Those are baked to WAV once by
//!   `tools/export/audio/render.mjs` (the same schedulers, run through an `OfflineAudioContext`)
//!   and played back as clips, with `volume = peak × master` restoring the JS loudness.
//!
//! Master volume and mute live in `save.audio` exactly as `audio.js` kept them on `ctx.save.audio`
//! ([`AudioSettings`]), and `KeyM` / `BracketLeft` / `BracketRight` are handled here, off the
//! `key` event, as `main.js:575` did.
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs the asset server and the
//! `AudioPlugin` from `DefaultPlugins`, and must stay out of the headless harness
//! (`crate::headless`).

pub mod oneshots;
pub mod synth;
pub mod voices;

use bevy::audio::AddAudioSource;
use bevy::prelude::*;
use std::sync::{Arc, Mutex};

use undercroft_sim::creature::{HState, ProfileKind};
use undercroft_sim::SimRng;

use crate::messages::SimMessage;
use crate::resources::SaveRes;
use crate::tick::SimSet;
use synth::{Params, SharedParams, Synth, VoiceKind};

/* ============================================================
`audio.js:TUNE` — the local tuning table (master/presence numbers live in `config.ron` AUDIO)
============================================================ */

/// `TUNE.tick` — parameters are pushed at 30 Hz.
pub const TUNE_TICK: f32 = 1.0 / 30.0;
/// `TUNE.tau` — the default `setTargetAtTime` time constant.
pub const TUNE_TAU: f32 = 0.1;
/// `TUNE.droneTau` — 0.5 ≈ the 1.5 s ambience crossfade (`AUDIO.crossfade`).
pub const TUNE_DRONE_TAU: f32 = 0.5;
/// `TUNE.droneSource` — extra drone gain per Source lap.
pub const TUNE_DRONE_SOURCE: f32 = 0.025;
/// `TUNE.droneWater` — the Cistern's water layer.
pub const TUNE_DRONE_WATER: f32 = 0.025;
/// `TUNE.lampCrackle`.
pub const TUNE_LAMP_CRACKLE: f32 = 0.012;
/// `TUNE.lampLowMul` — the crackle is louder below `TUNE_LAMP_LOW_OIL`.
pub const TUNE_LAMP_LOW_MUL: f32 = 3.0;
/// `TUNE.lampLowOil`.
pub const TUNE_LAMP_LOW_OIL: f32 = 15.0;
/// `TUNE.popMin` / `TUNE.popMax` — seconds between lamp pops.
pub const TUNE_POP_MIN: f32 = 0.08;
/// See [`TUNE_POP_MIN`].
pub const TUNE_POP_MAX: f32 = 0.3;
/// `TUNE.popGain`.
pub const TUNE_POP_GAIN: f32 = 0.05;
/// `TUNE.stepWalk` — metres of travel per footstep.
pub const TUNE_STEP_WALK: f32 = 0.55;
/// `TUNE.stepSprint`.
pub const TUNE_STEP_SPRINT: f32 = 0.42;
/// `TUNE.stepTeleport` — a jump this far in one tick is a teleport, not walking.
pub const TUNE_STEP_TELEPORT: f32 = 3.0;
/// `TUNE.hubCrackle` / `TUNE.hubCrackleStep` — hub flame crackle at tier 1, and per extra tier.
pub const TUNE_HUB_CRACKLE: f32 = 0.02;
/// See [`TUNE_HUB_CRACKLE`].
pub const TUNE_HUB_CRACKLE_STEP: f32 = 0.02;
/// `TUNE.hubSine` — the 65 Hz flame hum, per tier.
pub const TUNE_HUB_SINE: f32 = 0.02;
/// `TUNE.hubRange` — the flame fades out over this distance.
pub const TUNE_HUB_RANGE: f32 = 12.0;
/// `TUNE.waterWash`.
pub const TUNE_WATER_WASH: f32 = 0.02;
/// `TUNE.dripMin` / `dripMax` — seconds between drips outside the Cistern.
pub const TUNE_DRIP_MIN: f32 = 2.0;
/// See [`TUNE_DRIP_MIN`].
pub const TUNE_DRIP_MAX: f32 = 7.0;
/// `TUNE.dripCisternMin` / `dripCisternMax`.
pub const TUNE_DRIP_CISTERN_MIN: f32 = 0.6;
/// See [`TUNE_DRIP_CISTERN_MIN`].
pub const TUNE_DRIP_CISTERN_MAX: f32 = 2.0;
/// `TUNE.duckMenu` — the master gain multiplier while a menu is open.
pub const TUNE_DUCK_MENU: f32 = 0.4;
/// `TUNE.duckDead` — … and on the death screen.
pub const TUNE_DUCK_DEAD: f32 = 0.6;

/// `TUNE.stateMul` — how loud a `growl` presence voice is per FSM state (0.35 for anything else).
pub fn growl_state_mul(state: HState) -> f32 {
    match state {
        HState::Wander => 0.35,
        HState::Investigate => 0.7,
        HState::Chase => 1.0,
        HState::Staggered => 0.1,
        _ => 0.35,
    }
}

/* ============================================================
`audio.js:CTUNE` / `KIND_OF` — the creature roster (DESIGN.md §5)
============================================================ */

/// `CTUNE.whistle` — the Lampwight.
pub mod whistle {
    use undercroft_sim::creature::HState;
    /// `CTUNE.whistle.peak`.
    pub const PEAK: f32 = 0.10;
    /// `CTUNE.whistle.range`.
    pub const RANGE: f32 = 20.0;
    /// `CTUNE.whistle.mul` (0.5 for an unlisted state).
    pub fn mul(state: HState) -> f32 {
        match state {
            HState::Drift | HState::Wander => 0.5,
            HState::Drawn | HState::Chase | HState::Snuff => 1.0,
            HState::Sated => 0.4,
            HState::Staggered => 0.1,
            _ => 0.5,
        }
    }
}

/// `CTUNE.grind` — the Warden.
pub mod grind {
    /// `CTUNE.grind.peak`.
    pub const PEAK: f32 = 0.06;
    /// `CTUNE.grind.range`.
    pub const RANGE: f32 = 14.0;
    /// `CTUNE.grind.sweep` — the yaw rate (rad/s) the grind is full at.
    pub const SWEEP: f32 = 0.35;
    /// `CTUNE.grind.stepChase` / `stepReturn` — tread interval per state.
    pub const STEP_CHASE: f32 = 0.45;
    /// See [`STEP_CHASE`].
    pub const STEP_RETURN: f32 = 0.6;
    /// `CTUNE.grind.stepRange`.
    pub const STEP_RANGE: f32 = 14.0;
    /// `wardenStep` gain at zero distance.
    pub const STEP_GAIN: f32 = 0.25;
    /// `wardenClick` gain at zero distance (the sweep reversal click).
    pub const CLICK_GAIN: f32 = 0.05;
    /// Seconds a `creatureStep` event suppresses the internal tread timer.
    pub const EXT_STEP_HOLD: f32 = 1.5;
}

/// `CTUNE.wash` — the Drowner.
pub mod wash {
    use undercroft_sim::creature::HState;
    /// `CTUNE.wash.peak`.
    pub const PEAK: f32 = 0.08;
    /// `CTUNE.wash.range`.
    pub const RANGE: f32 = 12.0;
    /// `CTUNE.wash.speed` — the swim speed the wash saturates at.
    pub const SPEED: f32 = 5.0;
    /// `CTUNE.wash.plopMin` / `plopMax`.
    pub const PLOP_MIN: f32 = 2.0;
    /// See [`PLOP_MIN`].
    pub const PLOP_MAX: f32 = 5.0;
    /// `CTUNE.wash.plopRange`.
    pub const PLOP_RANGE: f32 = 12.0;
    /// `plop` gain at zero distance.
    pub const PLOP_GAIN: f32 = 0.14;
    /// `CTUNE.wash.mul` — **0 for an unlisted state** (a submerged Drowner is silent).
    pub fn mul(state: HState) -> f32 {
        match state {
            HState::Surge => 1.0,
            HState::Lurk => 0.5,
            HState::Surfacing => 0.6,
            HState::Sink => 0.4,
            _ => 0.0,
        }
    }
}

/// `CTUNE.lure` — the False Light. It has *no* presence voice; the lure layer is the whole sound.
pub mod lure {
    /// `CTUNE.lure.range`.
    pub const RANGE: f32 = 6.0;
    /// `CTUNE.lure.crackle` — the fake lamp crackle, as a fraction of `TUNE.lampCrackle`.
    pub const CRACKLE: f32 = 0.6;
    /// `CTUNE.lure.chime`.
    pub const CHIME: f32 = 0.012;
    /// `CTUNE.lure.tau` — the lure cuts out fast when the glass dies.
    pub const TAU: f32 = 0.02;
    /// `CTUNE.lure.scrabbleHz`.
    pub const SCRABBLE_HZ: f32 = 12.0;
    /// `CTUNE.lure.scrabbleRange`.
    pub const SCRABBLE_RANGE: f32 = 12.0;
    /// `scrabble` gain at zero distance.
    pub const SCRABBLE_GAIN: f32 = 0.08;
}

/// `CTUNE.breath` — the Brute.
pub mod breath {
    use undercroft_sim::creature::HState;
    /// `CTUNE.breath.peak`.
    pub const PEAK: f32 = 0.12;
    /// `CTUNE.breath.range`.
    pub const RANGE: f32 = 20.0;
    /// `CTUNE.breath.stepRange` — how far its stride thuds carry.
    pub const STEP_RANGE: f32 = 26.0;
    /// `CTUNE.breath.stepGain`.
    pub const STEP_GAIN: f32 = 0.35;
    /// `CTUNE.breath.mul` (0.6 for an unlisted state).
    pub fn mul(state: HState) -> f32 {
        match state {
            HState::Wander => 0.6,
            HState::Investigate => 0.8,
            HState::Chase => 1.0,
            _ => 0.6,
        }
    }
}

/// `audio.js:KIND_OF` — which presence voice a profile gets.
pub fn kind_of(profile: ProfileKind) -> VoiceKind {
    match profile {
        ProfileKind::Base | ProfileKind::Fast => VoiceKind::Growl,
        ProfileKind::Lampwight => VoiceKind::Whistle,
        ProfileKind::Warden => VoiceKind::Grind,
        ProfileKind::Drowner => VoiceKind::Wash,
        ProfileKind::FalseLight => VoiceKind::Lure,
        ProfileKind::Brute => VoiceKind::Breath,
    }
}

/// `audio.js:falloff(d, range)` — linear 1 → 0 over `range`.
pub fn falloff(d: f32, range: f32) -> f32 {
    (1.0 - d / range).clamp(0.0, 1.0)
}

/// `audio.js:panAt(x, z)` — where a world point sits across the player's facing, −1 … +1, scaled
/// by 0.8 like the JS hunter voices.
pub fn pan_at(px: f32, pz: f32, yaw: f32, x: f32, z: f32) -> f32 {
    let (dx, dz) = (x - px, z - pz);
    let d = dx.hypot(dz);
    if d < 0.01 {
        return 0.0;
    }
    let (rx, rz) = (yaw.cos(), -yaw.sin());
    ((dx * rx + dz * rz) / d).clamp(-1.0, 1.0) * 0.8
}

/* ============================================================
Resources
============================================================ */

/// `state.vol` / `state.muted`, mirrored into `save.audio` — the master volume and mute flag.
///
/// `audio.js:setVolume` / `setMute` owned `ctx.save.audio`, so this lane owns `SaveRes.0.audio`
/// too: the sound panel (ui lane) should call [`AudioSettings::set_volume`] /
/// [`AudioSettings::toggle_mute`] rather than writing the save itself.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct AudioSettings {
    /// 0 … 1, rounded to 2 decimals like `setVolume`.
    pub vol: f32,
    pub muted: bool,
    /// Set for one frame after a change, so the toast is emitted once.
    changed: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings {
            vol: undercroft_sim::save::DEFAULT_AUDIO_VOL,
            muted: false,
            changed: false,
        }
    }
}

impl AudioSettings {
    /// `audio.js:setVolume(v)` — clamp, round to 2 decimals, remember to toast.
    pub fn set_volume(&mut self, v: f32) -> f32 {
        let v = (v.clamp(0.0, 1.0) * 100.0).round() / 100.0;
        if v != self.vol {
            self.vol = v;
            self.changed = true;
        }
        self.vol
    }

    /// `audio.js:setMute(m)`.
    pub fn set_mute(&mut self, m: bool) -> bool {
        if m != self.muted {
            self.muted = m;
            self.changed = true;
        }
        self.muted
    }

    /// `audio.js:toggleMute()`.
    pub fn toggle_mute(&mut self) -> bool {
        self.set_mute(!self.muted)
    }

    /// The `state.master.gain` target for a mode's ducking (`audio.js:tick`).
    pub fn master(&self, duck: f32) -> f32 {
        if self.muted {
            0.0
        } else {
            self.vol * duck
        }
    }
}

/// The parameter block shared with the audio thread (`synth::SharedParams`).
#[derive(Resource, Clone)]
pub struct AudioParams(pub SharedParams);

impl Default for AudioParams {
    fn default() -> Self {
        AudioParams(Arc::new(Mutex::new(Params::default())))
    }
}

/// The one entity playing [`Synth`], so the plugin only ever spawns it once.
#[derive(Resource, Debug)]
pub struct SynthEntity(pub Entity);

/// Everything `audio.js` kept in its module-level `state` that is not volume: the 30 Hz
/// accumulator, the pop/drip/footstep timers and the sting rate limiter.
#[derive(Resource, Debug)]
pub struct AudioRuntime {
    /// `state.acc`.
    pub acc: f32,
    /// `state.popT` / `state.hubPopT` / `state.dripT`.
    pub pop_t: f32,
    pub hub_pop_t: f32,
    pub drip_t: f32,
    /// `state.stepAcc` / `state.lastX` / `state.lastZ` / `state.steps`.
    pub step_acc: f32,
    pub last_pos: Option<(f32, f32)>,
    pub steps: u32,
    /// `state.lastSting` — game time of the last chase sting (`AUDIO.stingGap` apart).
    pub last_sting: f32,
    /// `V.drone.base` — 38, or 31 in the Ossuary (`setDroneBase`, set from `zoneEnter`).
    pub drone_base: f32,
    /// `state.playedCount` / `state.played` — for tests and the debug overlay.
    pub played_count: u32,
    pub played: Vec<&'static str>,
    /// This lane's own randomness; the sim's `RngRes` belongs to the sim (HANDOFF §8).
    pub rng: SimRng,
}

impl Default for AudioRuntime {
    fn default() -> Self {
        AudioRuntime {
            acc: 0.0,
            pop_t: 0.0,
            hub_pop_t: 0.0,
            drip_t: 3.0,
            step_acc: 0.0,
            last_pos: None,
            steps: 0,
            last_sting: -1e9,
            drone_base: 38.0,
            played_count: 0,
            played: Vec::new(),
            rng: SimRng::from_entropy(),
        }
    }
}

impl AudioRuntime {
    /// `audio.js:rnd(a, b)`.
    pub fn rnd(&mut self, a: f32, b: f32) -> f32 {
        self.rng.range(a, b)
    }

    /// Remember a played sound (`state.played`, capped at 24 like the JS).
    pub fn note_played(&mut self, name: &'static str) {
        self.played_count += 1;
        self.played.push(name);
        if self.played.len() > 24 {
            self.played.remove(0);
        }
    }
}

/* ============================================================
Volume / mute
============================================================ */

/// `main.js:575` — `KeyM` toggles mute, `BracketLeft` / `BracketRight` step the volume by
/// `AUDIO.volStep`. The keys arrive as `SimEvent::Key` (the world lane forwards every press) and
/// the toast goes back out on the bus, exactly as `setVolume` / `setMute` emitted it.
///
/// `saveReset` is handled here too: `main.js:resetRuntime` keeps the sound settings across a wipe
/// ("preferences, not progress"), so this lane writes them straight back into the fresh save.
fn volume_keys(
    mut reader: MessageReader<SimMessage>,
    mut settings: ResMut<AudioSettings>,
    mut save: ResMut<SaveRes>,
    game: crate::resources::Game,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let cfg = &asset.data.config;
    let step = if cfg.audio.vol_step > 0.0 {
        cfg.audio.vol_step
    } else {
        0.1
    };
    let bound = |action: &str, code: &str| {
        cfg.keys
            .get(action)
            .map(|list| list.iter().any(|c| c == code))
            .unwrap_or(false)
    };
    let mut restore = false;
    for m in reader.read() {
        match &m.0 {
            undercroft_sim::SimEvent::Key { code, .. } => {
                if bound("mute", code) {
                    settings.toggle_mute();
                } else if bound("volDown", code) {
                    let v = settings.vol;
                    settings.set_volume(v - step);
                } else if bound("volUp", code) {
                    let v = settings.vol;
                    settings.set_volume(v + step);
                }
            }
            undercroft_sim::SimEvent::SaveReset => restore = true,
            _ => {}
        }
    }
    if restore {
        save.0.audio.vol = settings.vol;
        save.0.audio.muted = settings.muted;
    }
}

/// `audio.js:setVolume` / `setMute` each emitted a toast; a separate system does it because one
/// system may not both read and write `SimMessage`.
fn volume_toast(mut settings: ResMut<AudioSettings>, mut out: MessageWriter<SimMessage>) {
    if !settings.changed {
        return;
    }
    settings.changed = false;
    let msg = if settings.muted {
        "Sound off".to_string()
    } else if settings.vol == 0.0 {
        "Volume 0%".to_string()
    } else {
        format!("Volume {}%", (settings.vol * 100.0).round() as i32)
    };
    out.write(SimMessage(undercroft_sim::SimEvent::Toast { msg }));
}

/// Keep `save.audio` and [`AudioSettings`] in step in both directions: the save wins when it is
/// (re)loaded, this lane wins when a key or the sound panel changed it (`audio.js:init` read
/// `ctx.save.audio` once and wrote it back on every change).
fn sync_save(mut settings: ResMut<AudioSettings>, mut save: ResMut<SaveRes>) {
    let saved = save.0.audio;
    let same = saved.vol == settings.vol && saved.muted == settings.muted;
    if same {
        return;
    }
    if settings.is_changed() && !settings.is_added() {
        save.0.audio.vol = settings.vol;
        save.0.audio.muted = settings.muted;
    } else {
        settings.vol = saved.vol.clamp(0.0, 1.0);
        settings.muted = saved.muted;
    }
}

/* ============================================================
Plugin
============================================================ */

/// Register the synth asset, spawn the one entity that plays it, load the one-shot clips and add
/// the 30 Hz parameter push and the event → sound mapping.
pub fn plugin(app: &mut App) {
    // `DefaultPlugins` brings `AudioPlugin`; a bare `App` (the asset-loader test in
    // `crate::headless`, or any future tool that adds `UndercroftPlugin` without a renderer) does
    // not, and registering audio sources or loading `AudioSource`s without it panics. Stay inert.
    if !app.is_plugin_added::<bevy::audio::AudioPlugin>() {
        info!("audio: no AudioPlugin, the audio lane stays inert");
        return;
    }
    app.add_audio_source::<Synth>()
        .add_audio_source::<oneshots::Clip>()
        .init_resource::<AudioSettings>()
        .init_resource::<AudioParams>()
        .init_resource::<AudioRuntime>()
        .init_resource::<oneshots::Clips>()
        .init_resource::<oneshots::OneShotQueue>()
        .init_resource::<voices::PresenceStates>()
        .add_systems(Startup, (start_synth, oneshots::load_clips))
        .add_systems(
            Update,
            (
                volume_keys,
                volume_toast,
                sync_save,
                oneshots::decode_clips,
                oneshots::map_events,
                oneshots::play_queue,
                oneshots::reap_shots,
            )
                .chain(),
        )
        .add_systems(FixedUpdate, voices::tick.in_set(SimSet::Fanout));
}

/// Spawn the single looping [`Synth`] voice (`buildVoices` ran once when the context was created).
fn start_synth(
    mut commands: Commands,
    mut synths: ResMut<Assets<Synth>>,
    params: Res<AudioParams>,
) {
    let handle = synths.add(Synth::new(params.0.clone()));
    let e = commands
        .spawn((
            Name::new("audio: continuous voices"),
            bevy::audio::AudioPlayer(handle),
            // The source never ends, so `Once` is enough; the master gain inside the synth is what
            // silences it (`audio.js` kept every continuous voice running and moved its gain).
            bevy::audio::PlaybackSettings::ONCE,
        ))
        .id();
    commands.insert_resource(SynthEntity(e));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_match_the_js_table() {
        assert_eq!(kind_of(ProfileKind::Base), VoiceKind::Growl);
        assert_eq!(kind_of(ProfileKind::Fast), VoiceKind::Growl);
        assert_eq!(kind_of(ProfileKind::Lampwight), VoiceKind::Whistle);
        assert_eq!(kind_of(ProfileKind::Warden), VoiceKind::Grind);
        assert_eq!(kind_of(ProfileKind::Drowner), VoiceKind::Wash);
        assert_eq!(kind_of(ProfileKind::FalseLight), VoiceKind::Lure);
        assert_eq!(kind_of(ProfileKind::Brute), VoiceKind::Breath);
    }

    #[test]
    fn state_multipliers_match_the_js_tables() {
        // TUNE.stateMul, with 0.35 for anything unlisted
        assert_eq!(growl_state_mul(HState::Wander), 0.35);
        assert_eq!(growl_state_mul(HState::Investigate), 0.7);
        assert_eq!(growl_state_mul(HState::Chase), 1.0);
        assert_eq!(growl_state_mul(HState::Staggered), 0.1);
        assert_eq!(growl_state_mul(HState::Sentry), 0.35);
        // CTUNE.whistle.mul, default 0.5
        assert_eq!(whistle::mul(HState::Drawn), 1.0);
        assert_eq!(whistle::mul(HState::Sated), 0.4);
        assert_eq!(whistle::mul(HState::Lit), 0.5);
        // CTUNE.wash.mul, default 0 — a SUBMERGED Drowner makes no sound
        assert_eq!(wash::mul(HState::Surge), 1.0);
        assert_eq!(wash::mul(HState::Submerged), 0.0);
        assert_eq!(wash::mul(HState::Wander), 0.0);
        // CTUNE.breath.mul, default 0.6
        assert_eq!(breath::mul(HState::Chase), 1.0);
        assert_eq!(breath::mul(HState::Staggered), 0.6);
    }

    #[test]
    fn falloff_and_pan_match_the_js_helpers() {
        assert_eq!(falloff(0.0, 18.0), 1.0);
        assert_eq!(falloff(18.0, 18.0), 0.0);
        assert_eq!(falloff(36.0, 18.0), 0.0);
        assert!((falloff(9.0, 18.0) - 0.5).abs() < 1e-6);
        // facing +x (yaw 0 → right vector (1, 0)): a hunter due east is hard right
        assert!((pan_at(0.0, 0.0, 0.0, 5.0, 0.0) - 0.8).abs() < 1e-5);
        assert!((pan_at(0.0, 0.0, 0.0, -5.0, 0.0) + 0.8).abs() < 1e-5);
        assert!(pan_at(0.0, 0.0, 0.0, 0.0, 5.0).abs() < 1e-5);
        assert_eq!(pan_at(1.0, 1.0, 0.0, 1.0, 1.0), 0.0);
    }

    #[test]
    fn volume_rounds_and_clamps_like_set_volume() {
        let mut s = AudioSettings::default();
        assert_eq!(s.set_volume(0.834), 0.83);
        assert_eq!(s.set_volume(-1.0), 0.0);
        assert_eq!(s.set_volume(2.0), 1.0);
        assert_eq!(s.master(1.0), 1.0);
        assert_eq!(s.master(TUNE_DUCK_MENU), TUNE_DUCK_MENU);
        assert!(s.toggle_mute());
        assert_eq!(s.master(1.0), 0.0);
    }
}
