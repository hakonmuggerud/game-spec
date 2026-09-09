//! The real-time synth behind the continuous layers of `audio.js` (`buildVoices`, the presence
//! voice builders and the `setTargetAtTime` parameter pushes in `tick`/`presenceTick`).
//!
//! `audio.js` builds a Web Audio graph — oscillators, one looped 2 s white-noise buffer, biquads,
//! gains — and pushes parameters at 30 Hz. This module is the same graph written out by hand as a
//! [`rodio::Source`] so it can be a `bevy_audio` [`Decodable`] asset: [`Synth`] is the asset,
//! [`SynthSource`] the decoder that runs on the audio thread, and [`Params`] the block of numbers
//! the game thread pushes into the shared [`Mutex`] at 30 Hz ([`super::voices`]).
//!
//! Everything here is pure DSP: no threads, no `Instant`, no allocation on the audio thread after
//! the first block, so it compiles and runs on `wasm32-unknown-unknown` as well as natively.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Mutex};

use bevy::asset::Asset;
use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source};
use bevy::reflect::TypePath;

/// Output sample rate. 44.1 kHz, like the rendered one-shots (originally rendered by `reference/tools/export/audio/render.mjs`).
pub const SR: u32 = 44_100;

/// One sample in seconds.
pub const DT: f32 = 1.0 / SR as f32;

/// `audio.js:ensure` — "2 s white noise, looped by every noise source".
pub const NOISE_SECONDS: f32 = 2.0;

/// How many presence voices the synth renders. `audio.js` builds one per hunter index with no
/// limit; the audio thread needs a fixed budget, so [`super::voices`] sorts by gain and keeps the
/// loudest this many (see the module deviations note).
pub const MAX_PRESENCE: usize = 24;

/// Frames between [`Params`] refreshes on the audio thread (~5.8 ms at 44.1 kHz — well under the
/// 33 ms parameter push interval, and far cheaper than locking per sample).
const REFRESH_FRAMES: u32 = 256;

/* ============================================================
Primitives
============================================================ */

/// The four `OscillatorNode` types `audio.js` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave {
    #[default]
    Sine,
    Triangle,
    Square,
    Sawtooth,
}

impl Wave {
    /// The waveform at a phase in `0..1`, matching the Web Audio starting phase (sine and triangle
    /// start at 0 rising, square at +1, sawtooth at −1).
    pub fn at(self, p: f32) -> f32 {
        match self {
            Wave::Sine => (p * TAU).sin(),
            Wave::Triangle => {
                let t = p * 4.0;
                if t < 1.0 {
                    t
                } else if t < 3.0 {
                    2.0 - t
                } else {
                    t - 4.0
                }
            }
            Wave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Sawtooth => 2.0 * p - 1.0,
        }
    }
}

/// A phase-accumulating oscillator (`audio.js:osc`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Osc {
    pub wave: Wave,
    phase: f32,
}

impl Osc {
    /// A new oscillator of this shape, phase 0.
    pub fn new(wave: Wave) -> Osc {
        Osc { wave, phase: 0.0 }
    }

    /// A new oscillator started part-way through its cycle, so voices built at the same moment do
    /// not phase-lock (`hunter.js` gives every creature a random `phase` for the same reason).
    pub fn with_phase(wave: Wave, phase: f32) -> Osc {
        Osc {
            wave,
            phase: phase.rem_euclid(1.0),
        }
    }

    /// Advance one sample at `freq` Hz and return the sample.
    #[inline]
    pub fn next(&mut self, freq: f32, dt: f32) -> f32 {
        let v = self.wave.at(self.phase);
        self.phase += freq * dt;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        v
    }
}

/// `Math.random() * 2 - 1` over `NOISE_SECONDS`, seeded so every build is identical
/// (`audio.js:ensure` filled the buffer from `Math.random()`).
pub fn noise_buffer(seed: u64, len: usize) -> Vec<f32> {
    // xorshift64*, the same shape as `SimRng`, kept local so the audio thread owns no game state.
    let mut s = seed | 1;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        let bits = (s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as u32; // 24 bits
        out.push((bits as f32 / 16_777_216.0) * 2.0 - 1.0);
    }
    out
}

/// One `createBufferSource()` reading the shared looping noise buffer (`audio.js:noiseSrc`).
#[derive(Debug, Clone)]
pub struct NoiseCursor {
    buf: Arc<[f32]>,
    idx: usize,
}

impl NoiseCursor {
    /// Start reading at `offset` samples so two noise voices do not correlate.
    pub fn new(buf: Arc<[f32]>, offset: usize) -> NoiseCursor {
        let idx = if buf.is_empty() {
            0
        } else {
            offset % buf.len()
        };
        NoiseCursor { buf, idx }
    }

    /// The next sample, wrapping (`s.loop = true`).
    #[inline]
    pub fn sample(&mut self) -> f32 {
        if self.buf.is_empty() {
            return 0.0;
        }
        let v = self.buf[self.idx];
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        v
    }
}

/// The `BiquadFilterNode` types `audio.js` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Lowpass,
    Highpass,
    Bandpass,
}

/// A direct-form-1 biquad with the RBJ cookbook coefficients the Web Audio spec prescribes
/// (`audio.js:filt`). Web Audio reads `Q` in decibels for lowpass/highpass and as a plain quality
/// factor for bandpass, which is what [`Biquad::set`] does.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// A filter with its coefficients already set.
    pub fn new(kind: FilterKind, sr: f32, freq: f32, q: f32) -> Biquad {
        let mut b = Biquad::default();
        b.set(kind, sr, freq, q);
        b
    }

    /// Recompute the coefficients (state is kept, as Web Audio does when a param moves).
    pub fn set(&mut self, kind: FilterKind, sr: f32, freq: f32, q: f32) {
        let f = freq.clamp(1.0, sr * 0.49);
        let w0 = TAU * f / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        // Web Audio: Q is in dB for lowpass/highpass, a plain quality factor for bandpass.
        let q_lin = match kind {
            FilterKind::Lowpass | FilterKind::Highpass => 10f32.powf(q / 20.0),
            FilterKind::Bandpass => q.max(1e-4),
        };
        let alpha = sin_w0 / (2.0 * q_lin);
        let (b0, b1, b2) = match kind {
            FilterKind::Lowpass => {
                let x = (1.0 - cos_w0) / 2.0;
                (x, 1.0 - cos_w0, x)
            }
            FilterKind::Highpass => {
                let x = (1.0 + cos_w0) / 2.0;
                (x, -(1.0 + cos_w0), x)
            }
            FilterKind::Bandpass => (alpha, 0.0, -alpha),
        };
        let a0 = 1.0 + alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// Filter one sample.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// `AudioParam.setTargetAtTime(target, t0, tau)` — an exponential approach to `target`
/// (`audio.js:setTarget`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Smoothed {
    value: f32,
    target: f32,
}

impl Smoothed {
    /// Start at (and target) `v`.
    pub fn new(v: f32) -> Smoothed {
        Smoothed {
            value: v,
            target: v,
        }
    }

    /// `setTargetAtTime(v, …)`.
    #[inline]
    pub fn set_target(&mut self, v: f32) {
        self.target = v;
    }

    /// The value right now.
    #[inline]
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Jump straight to `v` (`cancelScheduledValues` + `setValueAtTime`, used by `lampSnuff`).
    #[inline]
    pub fn jump(&mut self, v: f32) {
        self.value = v;
        self.target = v;
    }

    /// The per-sample coefficient for a time constant: `1 - exp(-dt/tau)`.
    #[inline]
    pub fn coeff(dt: f32, tau: f32) -> f32 {
        if tau <= 0.0 {
            1.0
        } else {
            1.0 - (-dt / tau).exp()
        }
    }

    /// Advance one sample with a precomputed [`Smoothed::coeff`] and return the new value.
    #[inline]
    pub fn advance(&mut self, coeff: f32) -> f32 {
        self.value += (self.target - self.value) * coeff;
        self.value
    }
}

/// Equal-power stereo balance for a mono source, as `StereoPannerNode` defines it for mono input
/// (`audio.js:panner`): `pan` −1 = hard left, +1 = hard right.
#[inline]
pub fn equal_power_pan(pan: f32) -> (f32, f32) {
    let x = (pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (x.cos(), x.sin())
}

/// `audio.js:env(param, t0, peak, a, d)` as a pure function of the time since the note started:
/// a linear ramp from 0.0001 to `peak` over `a`, then an exponential ramp back to 0.0001 over `d`.
/// Used by the tests and by anything that wants the JS envelope shape without a graph.
pub fn env_gain(t: f32, peak: f32, a: f32, d: f32) -> f32 {
    const FLOOR: f32 = 0.0001;
    let peak = peak.max(0.0002);
    if t <= 0.0 {
        return FLOOR;
    }
    if t < a {
        // linearRampToValueAtTime
        return FLOOR + (peak - FLOOR) * (t / a.max(1e-9));
    }
    let td = t - a;
    if td >= d {
        return FLOOR;
    }
    // exponentialRampToValueAtTime
    peak * (FLOOR / peak).powf(td / d.max(1e-9))
}

/* ============================================================
Parameters pushed from the game thread
============================================================ */

/// Which presence voice a hunter gets (`audio.js:KIND_OF`, DESIGN.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceKind {
    /// `base` / `fast` — saw 42 + sine 30 under a moving lowpass.
    #[default]
    Growl,
    /// `lampwight` — "breath through a keyhole".
    Whistle,
    /// `warden` — stone grind that scales with its sweep speed.
    Grind,
    /// `drowner` — surge wash.
    Wash,
    /// `falseLight` — no presence; a fake lamp crackle + glass chime while LIT.
    Lure,
    /// `brute` — breathing drone.
    Breath,
}

/// One presence voice's per-tick parameters (`audio.js:presenceTick` writes these onto the nodes).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VoiceParams {
    pub kind: VoiceKind,
    /// The gain target (`setGain(v, g, …)`).
    pub gain: f32,
    /// −1 … +1 (`panAt`).
    pub pan: f32,
    /// Growl only: the lowpass cutoff `120 + 500·k`.
    pub cutoff: f32,
}

/// Everything the 30 Hz tick pushes into the synth (`audio.js:tick` + `presenceTick`).
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// `state.master.gain` — `vol × duck`, or 0 while muted.
    pub master: f32,
    /// `V.drone.target`.
    pub drone: f32,
    /// `V.drone.waterTarget` — the Cistern water layer.
    pub drone_water: f32,
    /// `V.drone.base` — 38, or 31 in the Ossuary (`setDroneBase`).
    pub drone_base: f32,
    /// `V.lamp.target`.
    pub lamp: f32,
    /// Bumped by `SOUNDS.lampSnuff`, which cuts the lamp crackle to zero instantly rather than
    /// letting it ramp (`cancelScheduledValues` + `setValueAtTime(0)`).
    pub lamp_cut: u32,
    /// `V.hub.target` — flame crackle.
    pub hub: f32,
    /// `V.hub.humTarget` — the 65 Hz flame hum.
    pub hub_hum: f32,
    /// `V.water.target` — the wash while standing in a `W` cell.
    pub water: f32,
    /// `V.presence`, at most [`MAX_PRESENCE`] entries.
    pub presence: Vec<VoiceParams>,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            master: 0.0,
            drone: 0.0,
            drone_water: 0.0,
            drone_base: 38.0,
            lamp: 0.0,
            lamp_cut: 0,
            hub: 0.0,
            hub_hum: 0.0,
            water: 0.0,
            presence: Vec::new(),
        }
    }
}

/// The parameter block shared between the game thread and the audio thread. Cheap to clone (it is
/// one `Arc`), so both the [`Synth`] asset and the pusher system hold one.
pub type SharedParams = Arc<Mutex<Params>>;

/* ============================================================
The voices
============================================================ */

/// `audio.js:buildVoices` — ambience drone (two sines under a lowpass) plus the Cistern water
/// layer (noise under a bandpass).
struct Drone {
    o1: Osc,
    o2: Osc,
    lp: Biquad,
    g: Smoothed,
    base: Smoothed,
    noise: NoiseCursor,
    bp: Biquad,
    wg: Smoothed,
}

/// A noise layer under one filter and one gain: lamp crackle, hub crackle and the water wash all
/// have this shape in `buildVoices`.
struct NoiseLayer {
    noise: NoiseCursor,
    filt: Biquad,
    g: Smoothed,
}

impl NoiseLayer {
    fn new(noise: NoiseCursor, kind: FilterKind, freq: f32, q: f32) -> NoiseLayer {
        NoiseLayer {
            noise,
            filt: Biquad::new(kind, SR as f32, freq, q),
            g: Smoothed::new(0.0),
        }
    }

    #[inline]
    fn next(&mut self, coeff: f32) -> f32 {
        let n = self.noise.sample();
        self.filt.process(n) * self.g.advance(coeff)
    }
}

/// One hunter presence voice. Every kind is built from the same parts (`whistleVoice`,
/// `grindVoice`, `washVoice`, `lureVoice`, `breathVoice`, `growlVoice`); the unused ones idle.
struct Presence {
    kind: VoiceKind,
    a: Osc,
    b: Osc,
    lfo: Osc,
    filt: Biquad,
    noise: NoiseCursor,
    g: Smoothed,
    cutoff: Smoothed,
    pan: Smoothed,
}

impl Presence {
    fn new(kind: VoiceKind, noise: Arc<[f32]>, slot: usize) -> Presence {
        // Offset each voice into the noise buffer and each LFO in phase so voices do not lock.
        let offset = slot.wrapping_mul(7919) % noise.len().max(1);
        let phase = (slot as f32 * 0.137).fract();
        let mut v = Presence {
            kind,
            a: Osc::with_phase(Wave::Sine, phase),
            b: Osc::with_phase(Wave::Sine, phase * 0.5),
            lfo: Osc::with_phase(Wave::Sine, phase),
            filt: Biquad::default(),
            noise: NoiseCursor::new(noise, offset),
            g: Smoothed::new(0.0),
            cutoff: Smoothed::new(120.0),
            pan: Smoothed::new(0.0),
        };
        v.rebuild(kind);
        v
    }

    /// `voiceFor(i, kind)` — swap the voice when the profile at this index changed.
    fn rebuild(&mut self, kind: VoiceKind) {
        self.kind = kind;
        self.g.jump(0.0);
        let sr = SR as f32;
        match kind {
            VoiceKind::Growl => {
                self.a.wave = Wave::Sawtooth;
                self.b.wave = Wave::Sine;
                self.filt.set(FilterKind::Lowpass, sr, 120.0, 1.0);
                self.cutoff.jump(120.0);
            }
            VoiceKind::Whistle => {
                self.a.wave = Wave::Sine;
                self.b.wave = Wave::Sine;
                self.filt.set(FilterKind::Bandpass, sr, 660.0, 6.0);
            }
            VoiceKind::Grind => {
                self.a.wave = Wave::Sawtooth;
                self.filt.set(FilterKind::Lowpass, sr, 90.0, 1.2);
            }
            VoiceKind::Wash => {
                self.filt.set(FilterKind::Lowpass, sr, 400.0, 0.5);
            }
            VoiceKind::Lure => {
                self.a.wave = Wave::Sine;
                self.b.wave = Wave::Sine;
                self.filt.set(FilterKind::Bandpass, sr, 3000.0, 2.0);
            }
            VoiceKind::Breath => {
                self.a.wave = Wave::Sine;
                self.filt.set(FilterKind::Lowpass, sr, 120.0, 0.8);
            }
        }
    }

    /// One sample, before panning.
    #[inline]
    fn mono(&mut self, coeff: f32) -> f32 {
        match self.kind {
            // saw 42 + sine 30 → lowpass (120 + 500·k) → tremolo (LFO 0.6 Hz ±30 %)
            VoiceKind::Growl => {
                let cut = self.cutoff.advance(coeff);
                self.filt.set(FilterKind::Lowpass, SR as f32, cut, 1.0);
                let x = self.a.next(42.0, DT) + self.b.next(30.0, DT);
                let trem = 1.0 + 0.3 * self.lfo.next(0.6, DT);
                self.filt.process(x) * trem
            }
            // sine 660 with 4 Hz / 12 Hz vibrato → bandpass 660 Q6 → tremolo 0.2 Hz ±25 %
            VoiceKind::Whistle => {
                let vib = self.b.next(4.0, DT) * 12.0;
                let x = self.a.next(660.0 + vib, DT);
                let trem = 0.75 + 0.25 * self.lfo.next(0.2, DT);
                self.filt.process(x) * trem
            }
            // sawtooth 28 → lowpass 90 Q1.2
            VoiceKind::Grind => {
                let x = self.a.next(28.0, DT);
                self.filt.process(x)
            }
            // the Cistern water noise again: noise → lowpass 400
            VoiceKind::Wash => {
                let n = self.noise.sample();
                self.filt.process(n)
            }
            // fake lamp crackle (noise → bandpass 3000 Q2) + a faint detuned glass chime
            VoiceKind::Lure => {
                let n = self.noise.sample();
                let crackle = self.filt.process(n);
                let trem = 0.7 + 0.3 * self.lfo.next(0.9, DT);
                // 1320 + 1980 (+5 cents), tremolo, at 0.6 of the crackle bus
                let chime = (self.a.next(1320.0, DT) + self.b.next(1980.0 * 1.002_89, DT)) * trem;
                crackle + chime * 0.6
            }
            // sine 38 + noise → lowpass 120 → tremolo 0.35 Hz ±40 %
            VoiceKind::Breath => {
                let n = self.noise.sample();
                let x = self.a.next(38.0, DT) + self.filt.process(n);
                let trem = 0.7 + 0.4 * self.lfo.next(0.35, DT);
                x * trem
            }
        }
    }
}

/* ============================================================
The source
============================================================ */

/// The `Decodable` asset: a handle to it plus [`bevy::audio::AudioPlayer`] starts the synth.
#[derive(Asset, TypePath, Clone)]
pub struct Synth {
    /// The block the 30 Hz tick writes and the audio thread reads.
    pub shared: SharedParams,
}

impl Synth {
    /// A synth sharing this parameter block.
    pub fn new(shared: SharedParams) -> Synth {
        Synth { shared }
    }
}

impl Decodable for Synth {
    type Decoder = SynthSource;

    fn decoder(&self) -> SynthSource {
        SynthSource::new(self.shared.clone())
    }
}

/// The audio-thread half: an endless interleaved stereo [`Source`] mixing every continuous layer.
pub struct SynthSource {
    shared: SharedParams,
    params: Params,
    /// Frames until the next [`Params`] refresh.
    countdown: u32,
    /// The pending right-channel sample (`next()` is called once per channel).
    pending_right: Option<f32>,
    master: Smoothed,
    drone: Drone,
    lamp: NoiseLayer,
    hub: NoiseLayer,
    hub_hum: Osc,
    hub_hum_g: Smoothed,
    water: NoiseLayer,
    presence: Vec<Presence>,
    /// The last `Params::lamp_cut` seen, so a bump cuts the crackle exactly once.
    lamp_cut: u32,
    noise: Arc<[f32]>,
    /// `1 - exp(-dt/tau)` for `TUNE.tau`, the drone's `TUNE.droneTau` and the master's 0.05.
    c_tau: f32,
    c_drone: f32,
    c_master: f32,
}

impl SynthSource {
    /// Build the whole graph (`buildVoices`) around a shared parameter block.
    pub fn new(shared: SharedParams) -> SynthSource {
        let noise: Arc<[f32]> =
            noise_buffer(0x00DE_2C0F, (SR as f32 * NOISE_SECONDS) as usize).into();
        let sr = SR as f32;
        let drone = Drone {
            o1: Osc::new(Wave::Sine),
            o2: Osc::with_phase(Wave::Sine, 0.25),
            lp: Biquad::new(FilterKind::Lowpass, sr, 200.0, 0.7),
            g: Smoothed::new(0.0),
            base: Smoothed::new(38.0),
            noise: NoiseCursor::new(noise.clone(), 0),
            bp: Biquad::new(FilterKind::Bandpass, sr, 1200.0, 0.7),
            wg: Smoothed::new(0.0),
        };
        let n = noise.len();
        SynthSource {
            shared,
            params: Params::default(),
            countdown: 0,
            pending_right: None,
            master: Smoothed::new(0.0),
            drone,
            lamp: NoiseLayer::new(
                NoiseCursor::new(noise.clone(), n / 5),
                FilterKind::Bandpass,
                3000.0,
                2.0,
            ),
            hub: NoiseLayer::new(
                NoiseCursor::new(noise.clone(), n / 3),
                FilterKind::Bandpass,
                1500.0,
                1.2,
            ),
            hub_hum: Osc::new(Wave::Sine),
            hub_hum_g: Smoothed::new(0.0),
            water: NoiseLayer::new(
                NoiseCursor::new(noise.clone(), n / 2),
                FilterKind::Lowpass,
                400.0,
                0.5,
            ),
            presence: Vec::with_capacity(MAX_PRESENCE),
            lamp_cut: 0,
            noise,
            c_tau: Smoothed::coeff(DT, super::TUNE_TAU),
            c_drone: Smoothed::coeff(DT, super::TUNE_DRONE_TAU),
            c_master: Smoothed::coeff(DT, 0.05),
        }
    }

    /// Pull the latest parameters, apply them to every voice's targets, and rebuild presence
    /// voices whose kind changed (`voiceFor`).
    fn refresh(&mut self) {
        if let Ok(p) = self.shared.try_lock() {
            self.params.clone_from(&p);
        }
        let p = &self.params;
        self.master.set_target(p.master);
        self.drone.g.set_target(p.drone);
        self.drone.wg.set_target(p.drone_water);
        self.drone.base.set_target(p.drone_base);
        self.lamp.g.set_target(p.lamp);
        if p.lamp_cut != self.lamp_cut {
            self.lamp_cut = p.lamp_cut;
            self.lamp.g.jump(0.0);
        }
        self.hub.g.set_target(p.hub);
        self.hub_hum_g.set_target(p.hub_hum);
        self.water.g.set_target(p.water);

        let want = p.presence.len().min(MAX_PRESENCE);
        while self.presence.len() < want {
            let slot = self.presence.len();
            let kind = p.presence[slot].kind;
            self.presence
                .push(Presence::new(kind, self.noise.clone(), slot));
        }
        for (i, v) in self.presence.iter_mut().enumerate() {
            if i >= want {
                v.g.set_target(0.0);
                continue;
            }
            let vp = p.presence[i];
            if v.kind != vp.kind {
                v.rebuild(vp.kind);
            }
            v.g.set_target(vp.gain);
            v.pan.set_target(vp.pan);
            if vp.kind == VoiceKind::Growl {
                v.cutoff.set_target(vp.cutoff);
            }
        }
    }

    /// Render one stereo frame.
    fn frame(&mut self) -> (f32, f32) {
        if self.countdown == 0 {
            self.refresh();
            self.countdown = REFRESH_FRAMES;
        }
        self.countdown -= 1;

        let (ct, cd, cm) = (self.c_tau, self.c_drone, self.c_master);

        // ambience drone: sine base + sine base·1.5 (+3 cents) → lowpass 200, plus the water layer
        let base = self.drone.base.advance(cd);
        let d1 = self.drone.o1.next(base, DT);
        let d2 = self.drone.o2.next(base * 1.5 * 1.001_734, DT); // +3 cents
        let mut mono = self.drone.lp.process(d1 + d2) * self.drone.g.advance(cd);
        let wn = self.drone.noise.sample();
        mono += self.drone.bp.process(wn) * self.drone.wg.advance(cd);

        // lamp crackle, hub flame crackle + hum, water wash
        mono += self.lamp.next(ct);
        mono += self.hub.next(ct);
        mono += self.hub_hum.next(65.0, DT) * self.hub_hum_g.advance(ct);
        mono += self.water.next(ct);

        let (mut l, mut r) = (mono, mono);
        for v in &mut self.presence {
            let g = v.g.advance(ct);
            let pan = v.pan.advance(ct);
            let s = v.mono(ct) * g;
            let (gl, gr) = equal_power_pan(pan);
            l += s * gl;
            r += s * gr;
        }

        let m = self.master.advance(cm);
        // A soft limiter: the JS relied on the browser's output clamping; keep peaks in range so a
        // pile-up of presence voices cannot clip hard.
        (limit(l * m), limit(r * m))
    }
}

/// `tanh`-ish soft clip, transparent below ±0.5.
#[inline]
fn limit(x: f32) -> f32 {
    if x > -0.5 && x < 0.5 {
        x
    } else {
        x.clamp(-4.0, 4.0).tanh() * 0.9
    }
}

impl Iterator for SynthSource {
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if let Some(r) = self.pending_right.take() {
            return Some(r);
        }
        let (l, r) = self.frame();
        self.pending_right = Some(r);
        Some(l)
    }
}

impl Source for SynthSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).expect("stereo")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SR).expect("nonzero sample rate")
    }

    fn total_duration(&self) -> Option<core::time::Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Root mean square of a block of samples.
    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|v| v * v).sum::<f32>() / xs.len() as f32).sqrt()
    }

    #[test]
    fn sine_block_rms_is_amplitude_over_root_two() {
        let mut o = Osc::new(Wave::Sine);
        let amp = 0.5;
        let xs: Vec<f32> = (0..SR as usize).map(|_| o.next(440.0, DT) * amp).collect();
        let want = amp * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (rms(&xs) - want).abs() < 1e-3,
            "rms {} want {want}",
            rms(&xs)
        );
    }

    #[test]
    fn waveforms_have_the_web_audio_starting_phase() {
        assert!(Wave::Sine.at(0.0).abs() < 1e-6);
        assert!(Wave::Triangle.at(0.0).abs() < 1e-6);
        assert_eq!(Wave::Triangle.at(0.25), 1.0);
        assert_eq!(Wave::Square.at(0.0), 1.0);
        assert_eq!(Wave::Square.at(0.75), -1.0);
        assert_eq!(Wave::Sawtooth.at(0.0), -1.0);
        assert!((Wave::Sawtooth.at(1.0 - 1e-6) - 1.0).abs() < 1e-5);
    }

    /// The lowpass in the ambience drone must pass 100 Hz and kill 4 kHz.
    #[test]
    fn lowpass_attenuates_high_frequencies() {
        let level = |freq: f32| {
            let mut f = Biquad::new(FilterKind::Lowpass, SR as f32, 200.0, 0.7);
            let mut o = Osc::new(Wave::Sine);
            let mut out = Vec::with_capacity(SR as usize / 2);
            for i in 0..SR as usize / 2 {
                let y = f.process(o.next(freq, DT));
                if i > SR as usize / 10 {
                    out.push(y); // skip the transient
                }
            }
            rms(&out)
        };
        let low = level(100.0);
        let high = level(4000.0);
        assert!(low > 0.6, "100 Hz passed at {low}");
        assert!(high < low / 100.0, "4 kHz {high} vs 100 Hz {low}");
    }

    #[test]
    fn highpass_and_bandpass_shape_as_expected() {
        let level = |kind: FilterKind, f0: f32, q: f32, freq: f32| {
            let mut f = Biquad::new(kind, SR as f32, f0, q);
            let mut o = Osc::new(Wave::Sine);
            let mut out = Vec::new();
            for i in 0..SR as usize / 2 {
                let y = f.process(o.next(freq, DT));
                if i > SR as usize / 10 {
                    out.push(y);
                }
            }
            rms(&out)
        };
        // highpass 2000: 4 kHz survives, 100 Hz does not
        assert!(level(FilterKind::Highpass, 2000.0, 0.5, 4000.0) > 0.5);
        assert!(level(FilterKind::Highpass, 2000.0, 0.5, 100.0) < 0.01);
        // bandpass 3000 Q2 peaks at its centre
        let mid = level(FilterKind::Bandpass, 3000.0, 2.0, 3000.0);
        assert!(mid > 0.6, "centre {mid}");
        assert!(level(FilterKind::Bandpass, 3000.0, 2.0, 300.0) < mid / 4.0);
    }

    #[test]
    fn smoothing_converges_on_its_target() {
        let mut s = Smoothed::new(0.0);
        s.set_target(1.0);
        let c = Smoothed::coeff(DT, 0.1);
        // after one time constant, ~63 % of the way there
        for _ in 0..(SR as f32 * 0.1) as usize {
            s.advance(c);
        }
        assert!((s.value() - 0.632).abs() < 0.01, "{}", s.value());
        for _ in 0..(SR as f32 * 0.9) as usize {
            s.advance(c);
        }
        assert!((s.value() - 1.0).abs() < 1e-3, "{}", s.value());
    }

    #[test]
    fn noise_buffer_is_seeded_and_centred() {
        let a = noise_buffer(0x00DE_2C0F, 4096);
        let b = noise_buffer(0x00DE_2C0F, 4096);
        assert_eq!(a, b, "the noise buffer must be deterministic");
        let mean = a.iter().sum::<f32>() / a.len() as f32;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!(a.iter().all(|v| (-1.0..=1.0).contains(v)));
        assert!((rms(&a) - 0.577).abs() < 0.05, "rms {}", rms(&a));
    }

    #[test]
    fn equal_power_pan_holds_its_power() {
        for pan in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
            let (l, r) = equal_power_pan(pan);
            assert!((l * l + r * r - 1.0).abs() < 1e-5, "pan {pan}");
        }
        let (l, r) = equal_power_pan(-1.0);
        assert!(l > 0.999 && r < 1e-6);
        let (l, r) = equal_power_pan(1.0);
        assert!(r > 0.999 && l < 1e-6);
    }

    #[test]
    fn env_gain_matches_the_js_shape() {
        // note('sine', …, peak 0.2, a 0.01, d 0.25)
        assert!(env_gain(-1.0, 0.2, 0.01, 0.25) <= 0.0001);
        assert!((env_gain(0.01, 0.2, 0.01, 0.25) - 0.2).abs() < 1e-6);
        assert!((env_gain(0.005, 0.2, 0.01, 0.25) - 0.1).abs() < 1e-3);
        // exponential decay: half-way through the decay is the geometric mean
        let mid = env_gain(0.01 + 0.125, 0.2, 0.01, 0.25);
        assert!((mid - (0.2f32 * 0.0001).sqrt()).abs() < 1e-4, "{mid}");
        assert!(env_gain(0.5, 0.2, 0.01, 0.25) <= 0.0001);
    }

    #[test]
    fn the_source_renders_stereo_frames_without_a_device() {
        let shared: SharedParams = Arc::new(Mutex::new(Params {
            master: 1.0,
            drone: 0.08,
            drone_base: 38.0,
            ..Params::default()
        }));
        let mut src = SynthSource::new(shared);
        assert_eq!(src.channels().get(), 2);
        assert_eq!(src.sample_rate().get(), SR);
        assert!(src.total_duration().is_none());
        let xs: Vec<f32> = (&mut src).take(SR as usize * 2).collect();
        assert_eq!(xs.len(), SR as usize * 2);
        assert!(xs.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        // the drone has settled well above silence by the end of the second
        let tail = &xs[SR as usize..];
        assert!(rms(tail) > 0.005, "drone rms {}", rms(tail));
    }
}

/// Renders audible proof that the synth and the baked clips work, for a lane that cannot take a
/// screenshot (PHASE2_LANES §2 "audio", point 4). Ignored by default because it writes files:
///
/// ```sh
/// cargo test -p undercroft --features dev -- --ignored render_wavs_for_listening --nocapture
/// ```
///
/// Output goes to `$UNDERCROFT_AUDIO_OUT` (default: the session scratchpad).
#[cfg(test)]
mod render_check {
    use super::*;
    use crate::audio::oneshots::{decode_wav, Clip};
    use crate::audio::TUNE_LAMP_CRACKLE;
    use bevy::audio::Decodable;

    fn out_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("UNDERCROFT_AUDIO_OUT").unwrap_or_else(|_| {
            "/tmp/claude-1000/-home-agent-repos-game-spec/\
             eda02054-8575-4010-bd55-bc92ae54e57a/scratchpad"
                .to_string()
        }))
    }

    /// Interleaved stereo f32 → a 16-bit WAV, the same layout `render.mjs` writes.
    fn write_wav(path: &std::path::Path, stereo: &[f32], sr: u32) -> std::io::Result<usize> {
        let n = stereo.len();
        let mut b = Vec::with_capacity(44 + n * 2);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n as u32 * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&sr.to_le_bytes());
        b.extend_from_slice(&(sr * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n as u32 * 2).to_le_bytes());
        for s in stereo {
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(path, &b)?;
        Ok(b.len())
    }

    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|v| v * v).sum::<f32>() / xs.len() as f32).sqrt()
    }

    #[test]
    #[ignore = "writes WAVs; run with --ignored"]
    fn render_wavs_for_listening() {
        let dir = out_dir();
        std::fs::create_dir_all(&dir).expect("scratchpad");

        // 2 s of the zone ambience drone with one Brute breath voice close by.
        let shared: SharedParams = Arc::new(Mutex::new(Params {
            master: 1.0,
            drone: 0.08,
            drone_base: 38.0,
            lamp: TUNE_LAMP_CRACKLE,
            presence: vec![VoiceParams {
                kind: VoiceKind::Breath,
                gain: 0.12,
                pan: -0.5,
                cutoff: 120.0,
            }],
            ..Params::default()
        }));
        let mut src = SynthSource::new(shared);
        let drone: Vec<f32> = (&mut src).take(SR as usize * 2 * 2).collect();
        let p = dir.join("audio_drone.wav");
        let bytes = write_wav(&p, &drone, SR).expect("write");
        println!(
            "{}: {bytes} bytes, {:.3} s, rms {:.5}, peak {:.5}",
            p.display(),
            drone.len() as f32 / 2.0 / SR as f32,
            rms(&drone),
            drone.iter().fold(0.0f32, |a, v| a.max(v.abs()))
        );
        assert!(rms(&drone) > 0.001, "the drone is silent");

        // One baked one-shot through the clip source, panned right like a Warden tread.
        let wav = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/audio/bank.wav"
        ))
        .expect("bank.wav");
        let (samples, rate) = decode_wav(&wav).expect("decode");
        let clip = Clip {
            samples: samples.into(),
            sample_rate: rate,
            pan: 0.6,
        };
        let shot: Vec<f32> = clip.decoder().collect();
        let p = dir.join("audio_oneshot_bank.wav");
        let bytes = write_wav(&p, &shot, rate).expect("write");
        println!(
            "{}: {bytes} bytes, {:.3} s, rms {:.5}, peak {:.5}",
            p.display(),
            shot.len() as f32 / 2.0 / rate as f32,
            rms(&shot),
            shot.iter().fold(0.0f32, |a, v| a.max(v.abs()))
        );
        assert!(rms(&shot) > 0.01, "the one-shot is silent");
    }
}
