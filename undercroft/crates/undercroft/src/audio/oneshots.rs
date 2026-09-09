//! The `audio.js` `SOUNDS` table: the ~60 one-shots, their event wiring (`audio.js:init` and the
//! two `audio.init` wrappers) and their playback.
//!
//! The prototype scheduled each one-shot as a fresh Web Audio graph. Here they are baked once by
//! `tools/export/audio/render.mjs` — the same schedulers run through an `OfflineAudioContext` —
//! into peak-normalised mono WAVs under `assets/audio/`, described by `assets/audio/manifest.ron`.
//! Playback restores the prototype's loudness with `volume = peak × master` (`audio.js:out`) and
//! pans with an equal-power balance in [`Clip`], the small `Decodable` wrapper below, because
//! `bevy_audio` has no stereo-balance knob of its own.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::asset::Asset;
use bevy::audio::{
    AudioPlayer, AudioSource, ChannelCount, Decodable, PlaybackSettings, Sample, SampleRate,
    Source, Volume,
};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use serde::Deserialize;

use undercroft_data::ItemKind;
use undercroft_sim::creature::ProfileKind;
use undercroft_sim::SimEvent;

use super::synth::equal_power_pan;
use super::voices::PresenceStates;
use super::*;
use crate::messages::SimMessage;
use crate::resources::{Game, Player, ZoneRes};
use crate::state::{GameMode, Mode};
use crate::tick::Clock;

/// `assets/audio/manifest.ron`, baked in so the table is a compile-time fact and no `std::fs`
/// creeps into the wasm build.
const MANIFEST_RON: &str = include_str!("../../../../assets/audio/manifest.ron");

/// One row of `manifest.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct SoundEntry {
    /// The `SOUNDS` key (or the key plus its option, e.g. `pickupRich`).
    pub name: String,
    /// File name under `assets/audio/`.
    pub file: String,
    /// The peak amplitude the prototype's graph produced — the playback gain.
    pub peak: f32,
    pub samples: usize,
    pub sample_rate: u32,
    pub duration: f32,
}

/// `assets/audio/manifest.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The rate most of the files are rendered at (each file's header is authoritative).
    pub sample_rate: u32,
    pub sounds: Vec<SoundEntry>,
}

/// Parse the baked manifest. Panics only if the generated file is malformed, which a build would
/// catch immediately.
pub fn manifest() -> Manifest {
    ron::from_str::<Manifest>(MANIFEST_RON).expect("assets/audio/manifest.ron parses")
}

/// Every sound name this lane can play. The event map below and
/// [`super::voices`] together reach all of them, except `menuMove` / `menuSelect`, which the ui
/// lane fires through [`OneShotQueue::play`] (the prototype's `menuMove` / `menuSelect` events have
/// no `SimEvent` yet — see the lane report).
pub const PLAYABLE: [&str; 59] = [
    "drip",
    "pop",
    "hubPop",
    "step",
    "stepSprint",
    "slosh",
    "flash",
    "lantern",
    "pickupOil",
    "pickupRich",
    "pickupBundle",
    "pickupQuest",
    "bank",
    "sting",
    "death",
    "npcFreed",
    "npcCaught",
    "npcRescued",
    "uiClick",
    "uiError",
    "build",
    "contractComplete",
    "contractAccepted",
    "toolGained",
    "lightTech",
    "lampOn",
    "lampOff",
    "topUp",
    "gate",
    "shortcut",
    "flameTier",
    "descend",
    "endingCage",
    "endingDawn",
    "endingNight",
    "menuMove",
    "menuSelect",
    "menuBack",
    "menuOpen",
    "lampwightSigh",
    "lampSnuff",
    "wardenClick",
    "wardenAlert",
    "wardenStep",
    "wardenReturn",
    "wardenDeath",
    "plop",
    "drownerSurge",
    "drownerSink",
    "drownerDeath",
    "falseLightPounce",
    "scrabble",
    "falseLightReveal",
    "falseLightDeath",
    "bruteRoar",
    "bruteStep",
    "lanternCrunch",
    "snort",
    "bruteDeath",
];

/// The static name for a runtime string, so a queued shot never allocates.
fn intern(name: &str) -> Option<&'static str> {
    PLAYABLE.iter().copied().find(|n| *n == name)
}

/* ============================================================
The clip asset
============================================================ */

/// Decoded PCM plus the pan it is played at. One asset is created per shot and freed when the
/// playing entity despawns, which is what lets a mono clip be panned per event
/// (`audio.js:out(peak, pan)`).
#[derive(Asset, TypePath, Clone)]
pub struct Clip {
    /// Mono samples, −1 … 1.
    pub samples: Arc<[f32]>,
    pub sample_rate: u32,
    /// −1 = hard left … +1 = hard right.
    pub pan: f32,
}

impl Decodable for Clip {
    type Decoder = ClipSource;

    fn decoder(&self) -> ClipSource {
        let (l, r) = equal_power_pan(self.pan);
        ClipSource {
            samples: self.samples.clone(),
            sample_rate: self.sample_rate,
            i: 0,
            gains: (l, r),
            pending_right: None,
        }
    }
}

/// The audio-thread side of [`Clip`]: a mono buffer widened to equal-power stereo.
pub struct ClipSource {
    samples: Arc<[f32]>,
    sample_rate: u32,
    i: usize,
    gains: (f32, f32),
    pending_right: Option<f32>,
}

impl Iterator for ClipSource {
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if let Some(r) = self.pending_right.take() {
            return Some(r);
        }
        let s = *self.samples.get(self.i)?;
        self.i += 1;
        self.pending_right = Some(s * self.gains.1);
        Some(s * self.gains.0)
    }
}

impl Source for ClipSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).expect("stereo")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(self.sample_rate.max(1)).expect("nonzero sample rate")
    }

    fn total_duration(&self) -> Option<core::time::Duration> {
        Some(core::time::Duration::from_secs_f32(
            self.samples.len() as f32 / self.sample_rate.max(1) as f32,
        ))
    }
}

/// One decoded one-shot.
#[derive(Debug, Clone)]
pub struct ClipData {
    pub samples: Arc<[f32]>,
    pub sample_rate: u32,
    /// `manifest.ron` `peak` — the JS `out(peak, …)` gain.
    pub peak: f32,
}

/// Every one-shot: the asset handles while they load, then the decoded PCM.
#[derive(Resource, Default)]
pub struct Clips {
    /// Handles kept alive so the `AudioSource` bytes stay loaded until decoded.
    loading: Vec<(&'static str, Handle<AudioSource>, f32)>,
    /// name → decoded clip.
    pub decoded: BTreeMap<&'static str, ClipData>,
}

impl Clips {
    /// The decoded clip, if it has finished loading.
    pub fn get(&self, name: &str) -> Option<&ClipData> {
        self.decoded.get(name)
    }

    /// Every clip has been decoded.
    pub fn ready(&self) -> bool {
        self.loading.is_empty()
    }
}

/// Kick off the asset loads listed in `manifest.ron`.
pub fn load_clips(mut clips: ResMut<Clips>, server: Res<AssetServer>) {
    for entry in manifest().sounds {
        let Some(name) = intern(&entry.name) else {
            warn!("audio: manifest lists unknown sound {}", entry.name);
            continue;
        };
        let handle = server.load::<AudioSource>(format!("audio/{}", entry.file));
        clips.loading.push((name, handle, entry.peak));
    }
    info!("audio: loading {} one-shots", clips.loading.len());
}

/// Turn each loaded WAV into PCM once. `bevy_audio` would decode it again per playback; decoding
/// here instead is what allows the per-shot pan (and it is a few hundred KB in total).
pub fn decode_clips(mut clips: ResMut<Clips>, sources: Res<Assets<AudioSource>>) {
    if clips.loading.is_empty() {
        return;
    }
    let mut still = Vec::new();
    let mut done = Vec::new();
    for (name, handle, peak) in std::mem::take(&mut clips.loading) {
        match sources.get(&handle) {
            Some(src) => match decode_wav(&src.bytes) {
                Some((samples, rate)) => done.push((
                    name,
                    ClipData {
                        samples: samples.into(),
                        sample_rate: rate,
                        peak,
                    },
                )),
                None => error!("audio: {name}.wav is not 16-bit PCM"),
            },
            None => still.push((name, handle, peak)),
        }
    }
    for (name, data) in done {
        clips.decoded.insert(name, data);
    }
    clips.loading = still;
    if clips.loading.is_empty() {
        info!("audio: {} one-shots ready", clips.decoded.len());
    }
}

/// Minimal RIFF/WAVE reader for the files `render.mjs` writes: mono 16-bit PCM. Returns the
/// samples and the file's own sample rate.
pub fn decode_wav(bytes: &[u8]) -> Option<(Vec<f32>, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u16at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32at(pos + 4) as usize;
        let body = pos + 8;
        if body + len > bytes.len() {
            return None;
        }
        if id == b"fmt " && len >= 16 {
            channels = u16at(body + 2);
            rate = u32at(body + 4);
            bits = u16at(body + 14);
        } else if id == b"data" {
            if channels != 1 || bits != 16 || rate == 0 {
                return None;
            }
            let mut out = Vec::with_capacity(len / 2);
            for i in (body..body + len - 1).step_by(2) {
                out.push(i16::from_le_bytes([bytes[i], bytes[i + 1]]) as f32 / 32768.0);
            }
            return Some((out, rate));
        }
        pos = body + len + (len & 1); // chunks are word-aligned
    }
    None
}

/* ============================================================
The queue
============================================================ */

/// One scheduled one-shot (`play(name, opts)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OneShot {
    pub name: &'static str,
    /// Multiplier on the table's own peak — the JS `{gain}` / `{peak}` options.
    pub gain: f32,
    /// −1 … +1 (`{pan}`).
    pub pan: f32,
    /// Playback rate; only `descend` uses it (a semitone per Source lap).
    pub speed: f32,
}

impl OneShot {
    /// A shot at the table's own peak, centred.
    pub fn new(name: &'static str) -> OneShot {
        OneShot {
            name,
            gain: 1.0,
            pan: 0.0,
            speed: 1.0,
        }
    }

    /// Scale the table's peak (`{gain: …}` in the JS).
    pub fn gain(mut self, g: f32) -> OneShot {
        self.gain = g;
        self
    }

    /// Pan it (`{pan: …}`).
    pub fn pan(mut self, p: f32) -> OneShot {
        self.pan = p.clamp(-1.0, 1.0);
        self
    }

    /// Pitch it (`descend`'s per-lap semitone).
    pub fn speed(mut self, s: f32) -> OneShot {
        self.speed = s;
        self
    }
}

/// One-shots scheduled this frame, drained by [`play_queue`]. The ui lane can push menu cues here
/// with [`OneShotQueue::play`].
#[derive(Resource, Debug, Default)]
pub struct OneShotQueue(pub Vec<OneShot>);

impl OneShotQueue {
    /// Queue a shot.
    pub fn push(&mut self, shot: OneShot) {
        self.0.push(shot);
    }

    /// Queue a sound by name; `false` for a name that is not in [`PLAYABLE`].
    pub fn play(&mut self, name: &str) -> bool {
        match intern(name) {
            Some(n) => {
                self.0.push(OneShot::new(n));
                true
            }
            None => false,
        }
    }
}

/* ============================================================
Event wiring (`audio.js:init` and the two wrappers)
============================================================ */

/// `SOUNDS.pickup` — the chime depends on what was picked up.
fn pickup_sound(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Bundle => "pickupBundle",
        ItemKind::Rich => "pickupRich",
        ItemKind::Quest => "pickupQuest",
        // the JS default covers `oil` and `relic`
        ItemKind::Oil | ItemKind::Relic => "pickupOil",
    }
}

/// `SOUNDS.ending` — three chords across the six ending ids.
fn ending_sound(id: &str) -> &'static str {
    match id {
        "dawn" | "kindle" => "endingDawn",
        "night" | "dark" => "endingNight",
        _ => "endingCage",
    }
}

/// `audio.js:DEATH_BY` — the per-killer death sting; `None` falls back to `death`.
fn death_sound(profile: Option<ProfileKind>) -> &'static str {
    match profile {
        Some(ProfileKind::Warden) => "wardenDeath",
        Some(ProfileKind::Drowner) => "drownerDeath",
        Some(ProfileKind::FalseLight) => "falseLightDeath",
        Some(ProfileKind::Brute) => "bruteDeath",
        _ => "death",
    }
}

/// Map this frame's [`SimEvent`]s onto one-shots, exactly as the prototype's listeners did.
#[allow(clippy::too_many_arguments)]
pub fn map_events(
    mut reader: MessageReader<SimMessage>,
    game: Game,
    zone: Res<ZoneRes>,
    player: Res<Player>,
    clock: Res<Clock>,
    params: Res<AudioParams>,
    mut rt: ResMut<AudioRuntime>,
    mut states: ResMut<PresenceStates>,
    mut queue: ResMut<OneShotQueue>,
) {
    let sting_gap = game
        .get()
        .map(|a| a.data.config.audio.sting_gap)
        .unwrap_or(6.0);
    let now = clock.time;
    let profile_of = |id: Option<u32>| -> Option<ProfileKind> {
        let id = id?;
        zone.get()?
            .hunters
            .iter()
            .find(|h| h.id == id)
            .map(|h| h.profile)
    };
    let index_of =
        |id: u32| -> Option<usize> { zone.get()?.hunters.iter().position(|h| h.id == id) };
    let pan_of = |x: f32, z: f32| pan_at(player.x, player.z, player.yaw, x, z);

    for m in reader.read() {
        match &m.0 {
            // ---- ambience ----
            SimEvent::ZoneEnter { zone_id } => {
                rt.drone_base = if zone_id == "ossuary" { 31.0 } else { 38.0 };
                rt.drip_t = rt.rnd(0.5, 2.0);
                rt.step_acc = 0.0;
                rt.last_pos = None;
            }
            SimEvent::HubEnter => {
                rt.drone_base = 38.0;
                rt.last_pos = None;
            }
            SimEvent::ZoneExit { .. } => rt.last_pos = None,

            // ---- player ----
            SimEvent::Flash { .. } => queue.push(OneShot::new("flash")),
            SimEvent::Lantern { .. } => queue.push(OneShot::new("lantern")),
            SimEvent::Pickup { kind, .. } => queue.push(OneShot::new(pickup_sound(*kind))),
            SimEvent::Bank { pts, .. } => {
                queue.push(OneShot::new(if *pts > 0 { "bank" } else { "uiClick" }))
            }
            SimEvent::LampToggle { on } => {
                queue.push(OneShot::new(if *on { "lampOn" } else { "lampOff" }))
            }
            SimEvent::TopUp { .. } => queue.push(OneShot::new("topUp")),
            SimEvent::WaterEnter { .. } => queue.push(OneShot::new("slosh")),

            // ---- world ----
            SimEvent::GateOpened { .. } => queue.push(OneShot::new("gate")),
            SimEvent::ShortcutOpened { .. } => queue.push(OneShot::new("shortcut")),
            SimEvent::GateLocked { .. } => queue.push(OneShot::new("uiError")),

            // ---- creatures ----
            SimEvent::HunterState { id, state, prev } => {
                let profile = profile_of(Some(*id));
                let entering = |s: &str| state == s && prev != s;
                match profile {
                    Some(ProfileKind::Lampwight) if entering("DRAWN") => {
                        queue.push(OneShot::new("lampwightSigh"))
                    }
                    Some(ProfileKind::Brute) if entering("CHASE") => {
                        if now - rt.last_sting >= sting_gap {
                            rt.last_sting = now;
                            queue.push(OneShot::new("bruteRoar"));
                        }
                    }
                    // the roster has its own cues; only `growl` profiles get the chase sting
                    Some(p)
                        if kind_of(p) == VoiceKind::Growl
                            && entering("CHASE")
                            && now - rt.last_sting >= sting_gap =>
                    {
                        rt.last_sting = now;
                        queue.push(OneShot::new("sting"));
                    }
                    _ => {}
                }
            }
            SimEvent::Death { hunter_id, .. } => {
                queue.push(OneShot::new(death_sound(profile_of(*hunter_id))))
            }
            SimEvent::LampSnuffed { .. } => {
                queue.push(OneShot::new("lampSnuff"));
                // `SOUNDS.lampSnuff` cuts the lamp crackle to zero on the spot
                if let Ok(mut p) = params.0.lock() {
                    p.lamp = 0.0;
                    p.lamp_cut = p.lamp_cut.wrapping_add(1);
                }
            }
            SimEvent::LanternSmashed { .. } => queue.push(OneShot::new("lanternCrunch")),
            SimEvent::WardenAlert { .. } => queue.push(OneShot::new("wardenAlert")),
            SimEvent::WardenReturn { .. } => queue.push(OneShot::new("wardenReturn")),
            SimEvent::DrownerSurge { .. } => queue.push(OneShot::new("drownerSurge")),
            SimEvent::DrownerSink { .. } => queue.push(OneShot::new("drownerSink")),
            SimEvent::FalseLightPounce { .. } => queue.push(OneShot::new("falseLightPounce")),
            SimEvent::FalseLightReveal { .. } => queue.push(OneShot::new("falseLightReveal")),
            SimEvent::FlashResisted { .. } => queue.push(OneShot::new("snort")),
            SimEvent::CreatureStep {
                hunter_id,
                profile,
                x,
                z,
                d,
            } => {
                let prof = ProfileKind::from_js_name(profile)
                    .or_else(|| profile_of(Some(*hunter_id)))
                    .unwrap_or(ProfileKind::Base);
                let pan = pan_of(*x, *z);
                match prof {
                    ProfileKind::Brute if *d <= breath::STEP_RANGE => queue.push(
                        OneShot::new("bruteStep")
                            .gain(falloff(*d, breath::STEP_RANGE))
                            .pan(pan),
                    ),
                    ProfileKind::Warden => {
                        if let Some(v) = index_of(*hunter_id).and_then(|i| states.by_index(i)) {
                            v.ext_step_t = now;
                        }
                        if *d <= grind::STEP_RANGE {
                            queue.push(
                                OneShot::new("wardenStep")
                                    .gain(falloff(*d, grind::STEP_RANGE))
                                    .pan(pan),
                            );
                        }
                    }
                    _ if *d <= 18.0 => queue.push(OneShot::new("step")),
                    _ => {}
                }
            }

            // ---- NPCs ----
            SimEvent::NpcFreed { .. } => queue.push(OneShot::new("npcFreed")),
            SimEvent::NpcCaught { .. } => queue.push(OneShot::new("npcCaught")),
            SimEvent::NpcRescued { .. } => queue.push(OneShot::new("npcRescued")),

            // ---- contracts / hub ----
            SimEvent::ContractAccepted { .. } => queue.push(OneShot::new("contractAccepted")),
            SimEvent::ContractComplete { .. } => queue.push(OneShot::new("contractComplete")),
            SimEvent::ToolGained { .. } => queue.push(OneShot::new("toolGained")),
            SimEvent::Build { .. } => queue.push(OneShot::new("build")),
            SimEvent::LightTech { .. } => queue.push(OneShot::new("lightTech")),
            SimEvent::FlameTier { initial, .. } => {
                if !*initial {
                    queue.push(OneShot::new("flameTier"));
                }
            }

            // ---- endgame ----
            SimEvent::Lap { lap, .. } => {
                // `SOUNDS.descend` scales its base by 2^(lap/12); the clip is rendered at lap 0.
                queue.push(OneShot::new("descend").speed(2f32.powf(*lap as f32 / 12.0)));
            }
            SimEvent::Ending { id, choice, .. } => {
                let which = if id.is_empty() { choice } else { id };
                queue.push(OneShot::new(ending_sound(which)));
            }

            // ---- UI ----
            SimEvent::UiClick => queue.push(OneShot::new("uiClick")),
            SimEvent::UiError { .. } => queue.push(OneShot::new("uiError")),
            SimEvent::MenuOpen { .. } | SimEvent::MenuClose { .. } => {
                queue.push(OneShot::new("uiClick"))
            }
            SimEvent::PauseOpen { .. } => queue.push(OneShot::new("menuOpen")),
            SimEvent::Title => queue.push(OneShot::new("menuBack")),
            _ => {}
        }
    }
}

/// How long a one-shot entity is kept before it is reaped (its own length plus a margin).
///
/// `PlaybackMode::Despawn` only fires once a sink exists, and `bevy_audio` creates no sinks when
/// the machine has no output device — without this the queue would leak an entity per drip.
#[derive(Component, Debug)]
pub struct ShotLife(pub f32);

/// Despawn finished (or never-started) one-shots.
pub fn reap_shots(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut ShotLife)>) {
    let dt = time.delta_secs();
    for (e, mut life) in &mut q {
        life.0 -= dt;
        if life.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// Spawn one `AudioPlayer` per queued shot (`play(name, opts)`); muted or before the clips have
/// loaded, the queue is simply dropped, as `play()` returned `false` in the prototype.
pub fn play_queue(
    mut commands: Commands,
    mut queue: ResMut<OneShotQueue>,
    mut assets: ResMut<Assets<Clip>>,
    clips: Res<Clips>,
    settings: Res<AudioSettings>,
    mode: Mode,
    mut rt: ResMut<AudioRuntime>,
) {
    let shots = std::mem::take(&mut queue.0);
    if settings.muted {
        return;
    }
    // `audio.js:tick` ducks the master bus; one-shots go through it, so apply the same factor.
    let duck = match mode.get() {
        GameMode::Menu => TUNE_DUCK_MENU,
        GameMode::Dead => TUNE_DUCK_DEAD,
        _ => 1.0,
    };
    let master = settings.master(duck);
    if master <= 0.0 {
        return;
    }
    for shot in shots {
        let Some(clip) = clips.get(shot.name) else {
            continue;
        };
        let volume = clip.peak * shot.gain * master;
        if volume <= 0.0 {
            continue;
        }
        let handle = assets.add(Clip {
            samples: clip.samples.clone(),
            sample_rate: clip.sample_rate,
            pan: shot.pan,
        });
        debug!("audio: {} at {volume:.3} (pan {:.2})", shot.name, shot.pan);
        let secs = clip.samples.len() as f32 / clip.sample_rate.max(1) as f32;
        commands.spawn((
            AudioPlayer(handle),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_speed(shot.speed),
            ShotLife(secs / shot.speed.max(0.01) + 0.5),
        ));
        rt.note_played(shot.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_sim::creature::HState;

    #[test]
    fn the_manifest_parses_and_covers_every_playable_sound() {
        let m = manifest();
        assert_eq!(m.sample_rate, super::super::synth::SR);
        let mut names: Vec<&str> = m.sounds.iter().map(|s| s.name.as_str()).collect();
        names.sort_unstable();
        let mut want = PLAYABLE.to_vec();
        want.sort_unstable();
        assert_eq!(names, want, "manifest.ron and PLAYABLE must agree");
        for s in &m.sounds {
            assert!(
                s.peak > 0.0 && s.peak <= 1.01,
                "{}: peak {}",
                s.name,
                s.peak
            );
            assert!(s.samples > 0 && s.duration > 0.0, "{} is empty", s.name);
            assert!(s.file == format!("{}.wav", s.name), "{}", s.file);
        }
    }

    /// Every `SOUNDS` key the prototype's listeners can reach must be reachable here too.
    #[test]
    fn the_event_table_maps_the_js_sounds() {
        assert_eq!(pickup_sound(ItemKind::Oil), "pickupOil");
        assert_eq!(pickup_sound(ItemKind::Relic), "pickupOil");
        assert_eq!(pickup_sound(ItemKind::Rich), "pickupRich");
        assert_eq!(pickup_sound(ItemKind::Quest), "pickupQuest");
        assert_eq!(pickup_sound(ItemKind::Bundle), "pickupBundle");
        assert_eq!(ending_sound("cage"), "endingCage");
        assert_eq!(ending_sound("feed"), "endingCage");
        assert_eq!(ending_sound("dawn"), "endingDawn");
        assert_eq!(ending_sound("kindle"), "endingDawn");
        assert_eq!(ending_sound("night"), "endingNight");
        assert_eq!(ending_sound("dark"), "endingNight");
        // DEATH_BY: base/fast/lampwight fall through to the plain death hit
        assert_eq!(death_sound(None), "death");
        assert_eq!(death_sound(Some(ProfileKind::Base)), "death");
        assert_eq!(death_sound(Some(ProfileKind::Lampwight)), "death");
        assert_eq!(death_sound(Some(ProfileKind::Warden)), "wardenDeath");
        assert_eq!(death_sound(Some(ProfileKind::Drowner)), "drownerDeath");
        assert_eq!(
            death_sound(Some(ProfileKind::FalseLight)),
            "falseLightDeath"
        );
        assert_eq!(death_sound(Some(ProfileKind::Brute)), "bruteDeath");
    }

    #[test]
    fn only_growl_profiles_get_the_chase_sting() {
        // `audio.js:init` — `kindOf(profileOf(id)) !== 'growl'` bails out
        for p in ProfileKind::ALL {
            let growl = kind_of(p) == VoiceKind::Growl;
            assert_eq!(
                growl,
                matches!(p, ProfileKind::Base | ProfileKind::Fast),
                "{p:?}"
            );
        }
        // the Brute roars instead, on the same `lastSting` budget
        assert_eq!(kind_of(ProfileKind::Brute), VoiceKind::Breath);
        assert_ne!(HState::Chase.js_name(), HState::Wander.js_name());
    }

    #[test]
    fn one_shot_builders_clamp_and_default() {
        let s = OneShot::new("drip");
        assert_eq!((s.gain, s.pan, s.speed), (1.0, 0.0, 1.0));
        assert_eq!(OneShot::new("drip").pan(-4.0).pan, -1.0);
        assert_eq!(OneShot::new("drip").pan(4.0).pan, 1.0);
        assert!(intern("drip").is_some());
        assert!(intern("nope").is_none());
        let mut q = OneShotQueue::default();
        assert!(q.play("menuSelect"));
        assert!(!q.play("nope"));
        assert_eq!(q.0.len(), 1);
    }

    /// The WAV reader must round-trip what `render.mjs` writes.
    #[test]
    fn wav_reader_reads_the_rendered_files() {
        let mut bytes = Vec::new();
        let data: Vec<i16> = vec![0, 16384, -16384, 32767];
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36u32 + data.len() as u32 * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&44100u32.to_le_bytes());
        bytes.extend_from_slice(&88200u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32 * 2).to_le_bytes());
        for s in &data {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let (samples, rate) = decode_wav(&bytes).expect("parses");
        assert_eq!(rate, 44100);
        assert_eq!(samples.len(), 4);
        assert!((samples[1] - 0.5).abs() < 1e-6);
        assert!((samples[2] + 0.5).abs() < 1e-6);
        assert!(decode_wav(b"nope").is_none());
    }

    #[test]
    fn clip_source_pans_a_mono_buffer() {
        let clip = Clip {
            samples: vec![1.0, 1.0].into(),
            sample_rate: 44100,
            pan: 1.0,
        };
        let out: Vec<f32> = clip.decoder().collect();
        assert_eq!(out.len(), 4);
        assert!(out[0].abs() < 1e-6, "hard right must silence the left");
        assert!((out[1] - 1.0).abs() < 1e-5);
    }
}
