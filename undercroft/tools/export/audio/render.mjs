// render.mjs — renders every one-shot in `reference/prototype/src/audio.js`'s SOUNDS table to a mono
// 16-bit 44.1 kHz WAV under `assets/audio/`, plus `assets/audio/manifest.ron` (name → file,
// peak, duration, samples).
//
// Why this exists: `audio.js` builds its one-shots out of Web Audio nodes at play time. The Bevy
// port keeps the *continuous* layers procedural (`src/audio/synth.rs`) but plays the one-shots as
// clips, so they have to be baked once. `node-web-audio-api` is a Rust-backed Web Audio
// implementation with a real `OfflineAudioContext`, so the schedulers below are copied from
// `audio.js` verbatim (only `state.master`/`state.noise`/`state.hasPanner` are re-pointed at the
// offline context, and `Math.random` is seeded so the render is byte-for-byte reproducible).
//
//   cd undercroft/tools/export/audio && npm install && node render.mjs
//
// Every file is mono 16-bit, peak-normalised to 1.0 and the original peak is written to the manifest, so the
// player restores the JS loudness with `volume = peak * master` (`audio.js:out(peak, pan)`).
// Panning is *not* baked: `panner()` returns null here (the `state.hasPanner === false` path of
// the prototype), and `src/audio/oneshots.rs` pans at play time instead.

import { OfflineAudioContext } from 'node-web-audio-api';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, '..', '..', '..', 'assets', 'audio');
const SR = 44100;
const SILENCE = 3e-4;   // 3x the 1e-4 floor `env()` ramps down to
const TAIL = 0.01;      // seconds of silence kept after the last audible sample

/* ---------- determinism: one seeded PRNG for the noise buffer and every rnd() ---------- */
function mulberry32(a) {
  return function () {
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
let rng = mulberry32(0x1D0C0FF);
Math.random = () => rng();

/* ============================================================
   audio.js helpers, verbatim (see the header)
   ============================================================ */
const TUNE = { popGain: 0.05 };
const state = { ac: null, master: null, noise: null, hasPanner: false };

const now = () => state.ac.currentTime;
function gain(v) { const g = state.ac.createGain(); g.gain.value = v; return g; }
function osc(type, f) { const o = state.ac.createOscillator(); o.type = type; o.frequency.value = f; return o; }
function filt(type, f, q) { const b = state.ac.createBiquadFilter(); b.type = type; b.frequency.value = f; if (q !== undefined) b.Q.value = q; return b; }
function noiseSrc(loop = true) { const s = state.ac.createBufferSource(); s.buffer = state.noise; s.loop = loop; return s; }
function panner(v = 0) {
  if (!state.hasPanner) return null;
  const p = state.ac.createStereoPanner(); p.pan.value = Math.max(-1, Math.min(1, v)); return p;
}
function out(peak, pan, dest) {
  const g = gain(peak), target = dest || state.master;
  const p = pan !== undefined ? panner(pan) : null;
  if (p) { g.connect(p); p.connect(target); g.__pan = p; } else g.connect(target);
  return g;
}
function env(param, t0, peak, a, d) {
  param.cancelScheduledValues(t0);
  param.setValueAtTime(0.0001, t0);
  param.linearRampToValueAtTime(Math.max(0.0002, peak), t0 + a);
  param.exponentialRampToValueAtTime(0.0001, t0 + a + d);
  return t0 + a + d;
}
function stopAt(src, t) { src.stop(t); }
function note(type, f0, f1, t0, peak, a, d, { sweep = 0, pan, dest } = {}) {
  const o = osc(type, f0), g = out(0, pan, dest);
  o.frequency.setValueAtTime(f0, t0);
  if (f1 && f1 !== f0) o.frequency.exponentialRampToValueAtTime(f1, t0 + (sweep || a + d));
  const end = env(g.gain, t0, peak, a, d);
  o.connect(g); o.start(t0); stopAt(o, end + 0.05, g);
  return o;
}
function burst(t0, peak, a, d, { type = 'lowpass', f0 = 1000, f1 = 0, q = 1, sweep = 0, pan, dest } = {}) {
  const s = noiseSrc(true), b = filt(type, f0, q), g = out(0, pan, dest);
  b.frequency.setValueAtTime(f0, t0);
  if (f1 && f1 !== f0) b.frequency.exponentialRampToValueAtTime(f1, t0 + (sweep || a + d));
  const end = env(g.gain, t0, peak, a, d);
  s.connect(b); b.connect(g); s.start(t0); stopAt(s, end + 0.05, b, g);
  return s;
}
const rnd = (a, b) => a + Math.random() * (b - a);
function chime(t0, freqs, { gap = 0.09, decay = 1.2, peak = 0.25, type = 'triangle' } = {}) {
  freqs.forEach((f, i) => note(type, f, 0, t0 + i * gap, peak, 0.01, decay));
}
function chitter(t0, { peak = 0.12, pan } = {}) { for (let i = 0; i < 8; i++) note('square', 900 + (i % 2) * 60, 0, t0 + i * 0.044, peak, 0.002, 0.02, { pan }); }
function bell(t0, f, decay, peak) { note('sine', f, 0, t0, peak, 0.005, decay); note('sine', f * 2.76, 0, t0, peak * 0.35, 0.005, decay * 0.5); }

/* ============================================================
   The SOUNDS table (audio.js base + menu cues + creature roster)
   ============================================================ */
const SOUNDS = {
  drip(t0) { note('sine', 1800, 600, t0, 0.15, 0.005, 0.25, { sweep: 0.06, pan: rnd(-0.8, 0.8) }); },
  pop(t0, { peak = TUNE.popGain } = {}) { burst(t0, peak, 0.001, 0.003, { type: 'bandpass', f0: 2500, q: 1 }); },
  hubPop(t0, { peak = 0.04 } = {}) { burst(t0, peak, 0.001, 0.008, { type: 'bandpass', f0: 1200, q: 1 }); },
  step(t0, { sprint = false } = {}) {
    burst(t0, sprint ? 0.2 : 0.12, 0.004, 0.036, { type: 'lowpass', f0: sprint ? 900 : 500, q: 0.7 });
  },
  slosh(t0) {
    note('sine', 300, 120, t0, 0.15, 0.01, 0.12, { sweep: 0.12 });
    burst(t0, 0.08, 0.01, 0.18, { type: 'bandpass', f0: 900, f1: 300, q: 0.8, sweep: 0.18 });
  },
  flash(t0) { burst(t0, 0.35, 0.02, 0.25, { type: 'bandpass', f0: 400, f1: 4000, q: 1.2, sweep: 0.18 }); },
  lantern(t0) {
    note('sine', 220, 0, t0, 0.2, 0.01, 0.2);
    burst(t0, 0.1, 0.02, 0.4, { type: 'highpass', f0: 2000, q: 0.5 });
    note('sine', 660, 0, t0 + 0.42, 0.15, 0.005, 0.05);
  },
  pickup(t0, { kind = 'oil' } = {}) {
    if (kind === 'bundle') chime(t0, [330, 440], { gap: 0.12, decay: 0.4, peak: 0.22 });
    else if (kind === 'rich') chime(t0, [880, 1320, 1760], { gap: 0.09, decay: 0.3, peak: 0.2 });
    else if (kind === 'quest') chime(t0, [660, 990], { gap: 0.09, decay: 0.35, peak: 0.2 });
    else chime(t0, [880, 1320], { gap: 0.09, decay: 0.25, peak: 0.2 });
  },
  bank(t0) { chime(t0, [523, 659, 784]); },
  sting(t0) {
    note('sawtooth', 110, 55, t0, 0.4, 0.02, 0.6, { sweep: 0.5 });
    burst(t0, 0.25, 0.3, 0.4, { type: 'lowpass', f0: 600, f1: 2500, q: 1, sweep: 0.5 });
  },
  death(t0) {
    const lp = filt('lowpass', 4000, 1); lp.frequency.setValueAtTime(4000, t0); lp.frequency.exponentialRampToValueAtTime(100, t0 + 1.2);
    const g = gain(1); lp.connect(g); g.connect(state.master);
    burst(t0, 0.6, 0.005, 0.1, { type: 'lowpass', f0: 6000, q: 0.5, dest: lp });
    note('square', 60, 0, t0, 0.5, 0.01, 1.2, { dest: lp });
  },
  npcFreed(t0) { note('triangle', 440, 880, t0, 0.22, 0.02, 0.35, { sweep: 0.3 }); },
  npcCaught(t0) { note('triangle', 440, 110, t0, 0.25, 0.02, 0.45, { sweep: 0.4 }); },
  npcRescued(t0) { chime(t0, [1046, 1318, 1568]); },
  uiClick(t0) { note('square', 1200, 0, t0, 0.08, 0.002, 0.015); },
  uiError(t0) { note('square', 160, 0, t0, 0.1, 0.005, 0.12); },
  build(t0) {
    chime(t0, [523, 659, 784]);
    for (let i = 0; i < 3; i++) burst(t0 + i * 0.18, 0.3, 0.003, 0.06, { type: 'lowpass', f0: 800, q: 0.7 });
  },
  contractComplete(t0) { chime(t0, [660, 990], { gap: 0.12, decay: 0.8, peak: 0.22 }); },
  contractAccepted(t0) { chime(t0, [660, 880], { gap: 0.08, decay: 0.3, peak: 0.15 }); },
  toolGained(t0) { chime(t0, [784, 988, 1175], { gap: 0.1, decay: 0.9, peak: 0.22 }); },
  lightTech(t0) { chime(t0, [659, 784, 988], { gap: 0.1, decay: 0.9, peak: 0.22 }); },
  lampOn(t0) { note('square', 900, 0, t0, 0.04, 0.002, 0.012); burst(t0 + 0.01, 0.06, 0.01, 0.12, { type: 'bandpass', f0: 3000, q: 1.5 }); },
  lampOff(t0) { note('square', 500, 0, t0, 0.04, 0.002, 0.02); },
  topUp(t0) { note('sine', 200, 90, t0, 0.15, 0.02, 0.25, { sweep: 0.25 }); note('sine', 260, 140, t0 + 0.12, 0.1, 0.02, 0.2, { sweep: 0.2 }); },
  gate(t0) {
    note('square', 180, 120, t0, 0.18, 0.005, 0.5, { sweep: 0.4 });
    burst(t0, 0.25, 0.003, 0.25, { type: 'bandpass', f0: 2200, f1: 600, q: 2, sweep: 0.25 });
  },
  shortcut(t0) {
    note('square', 110, 70, t0, 0.16, 0.005, 0.7, { sweep: 0.5 });
    for (let i = 0; i < 5; i++) burst(t0 + 0.02 * i, 0.12, 0.002, 0.09, { type: 'bandpass', f0: 2600 - 120 * i, q: 3, pan: rnd(-0.3, 0.3) });
    note('sine', 55, 38, t0 + 0.05, 0.22, 0.02, 1.1, { sweep: 0.9 });
    burst(t0 + 0.05, 0.18, 0.01, 0.6, { type: 'lowpass', f0: 260, f1: 90, q: 0.7, sweep: 0.5 });
  },
  flameTier(t0) {
    note('sine', 65, 130, t0, 0.2, 0.3, 1.2, { sweep: 1.0 });
    burst(t0, 0.15, 0.4, 0.8, { type: 'lowpass', f0: 300, f1: 1800, q: 0.7, sweep: 0.6 });
  },
  descend(t0, { lap = 1 } = {}) {
    const f0 = 55 * Math.pow(2, (lap | 0) / 12), dur = 2.2;
    for (const mul of [1, 1.498]) note('sine', f0 * mul, f0 * mul * 1.5, t0, 0.1, dur * 0.45, dur * 0.55, { sweep: dur });
    burst(t0, 0.05, dur * 0.5, dur * 0.5, { type: 'lowpass', f0: 200 + 40 * lap, f1: 600 + 80 * lap, q: 0.7, sweep: dur });
  },
  ending(t0, { id = 'cage' } = {}) {
    const chords = { cage: [262, 330, 392], feed: [262, 330, 392], dawn: [392, 494, 587], kindle: [392, 494, 587], night: [55, 58], dark: [55, 58] };
    const fs = chords[id] || chords.cage;
    for (const f of fs) {
      const o = osc('sine', f), g = out(0);
      g.gain.setValueAtTime(0.0001, t0);
      g.gain.linearRampToValueAtTime(0.12, t0 + 1);
      g.gain.setValueAtTime(0.12, t0 + 4);
      g.gain.linearRampToValueAtTime(0.0001, t0 + 6);
      o.connect(g); o.start(t0); stopAt(o, t0 + 6.1, g);
    }
  },
  // menu cues
  menuMove(t0) { note('square', 720, 0, t0, 0.05, 0.002, 0.012); },
  menuSelect(t0) { note('square', 1200, 0, t0, 0.08, 0.002, 0.015); note('triangle', 1600, 0, t0 + 0.03, 0.06, 0.002, 0.05); },
  menuBack(t0) { note('square', 420, 0, t0, 0.06, 0.002, 0.03); },
  menuOpen(t0) { note('triangle', 330, 0, t0, 0.1, 0.005, 0.12); note('triangle', 495, 0, t0 + 0.06, 0.08, 0.005, 0.15); },
  // creature roster
  lampwightSigh(t0) { burst(t0, 0.2, 0.5, 0.35, { type: 'bandpass', f0: 300, f1: 1200, q: 1.5, sweep: 0.8 }); },
  lampSnuff(t0) {
    burst(t0, 0.4, 0.01, 0.14, { type: 'highpass', f0: 1500, q: 0.5 });
    burst(t0 + 0.02, 0.2, 0.05, 0.3, { type: 'bandpass', f0: 600, f1: 200, q: 1, sweep: 0.3 });
    note('sine', 55, 0, t0 + 0.05, 0.3, 0.01, 0.45);
  },
  wardenClick(t0, { gain: g = 0.05, pan } = {}) { burst(t0, g, 0.001, 0.01, { type: 'bandpass', f0: 1800, q: 2, pan }); },
  wardenAlert(t0) {
    note('square', 220, 110, t0, 0.3, 0.005, 0.3, { sweep: 0.3 });
    burst(t0, 0.3, 0.003, 0.08, { type: 'bandpass', f0: 3000, q: 1 });
    note('sine', 55, 82, t0 + 0.25, 0.3, 0.2, 0.9, { sweep: 1.0 });
  },
  wardenStep(t0, { gain: g = 0.25, pan } = {}) { burst(t0, g, 0.004, 0.09, { type: 'lowpass', f0: 200, q: 0.8, pan }); burst(t0 + 0.01, g * 0.4, 0.002, 0.04, { type: 'bandpass', f0: 900, q: 1, pan }); },
  wardenReturn(t0) { note('sine', 82, 55, t0, 0.15, 0.05, 0.6, { sweep: 0.5 }); },
  wardenDeath(t0) { SOUNDS.death(t0); bell(t0 + 0.1, 165, 1.5, 0.3); },
  plop(t0, { gain: g = 0.14, pan } = {}) { note('sine', 180, 90, t0, g, 0.005, 0.12, { sweep: 0.12, pan }); },
  drownerSurge(t0) {
    burst(t0, 0.5, 0.03, 0.6, { type: 'bandpass', f0: 400, f1: 2000, q: 0.8, sweep: 0.6 });
    note('sawtooth', 48, 36, t0 + 0.1, 0.35, 0.1, 1.1, { sweep: 1.2 });
    burst(t0 + 0.1, 0.12, 0.2, 1.0, { type: 'lowpass', f0: 250, q: 1.5 });
  },
  drownerSink(t0) { burst(t0, 0.35, 0.3, 0.1, { type: 'bandpass', f0: 2000, f1: 400, q: 0.8, sweep: 0.4 }); note('sine', 120, 60, t0, 0.12, 0.2, 0.2, { sweep: 0.4 }); },
  drownerDeath(t0) {
    const lp = filt('lowpass', 2000, 1); lp.frequency.setValueAtTime(2000, t0); lp.frequency.exponentialRampToValueAtTime(80, t0 + 1.2);
    const g = gain(1); lp.connect(g); g.connect(state.master);
    burst(t0, 0.5, 0.02, 0.5, { type: 'bandpass', f0: 500, f1: 2500, q: 0.8, sweep: 0.4, dest: lp });
    burst(t0 + 0.05, 0.6, 0.005, 0.1, { type: 'lowpass', f0: 6000, q: 0.5, dest: lp });
    note('square', 60, 0, t0 + 0.05, 0.5, 0.01, 1.2, { dest: lp });
  },
  falseLightPounce(t0) { burst(t0, 0.3, 0.001, 0.012, { type: 'bandpass', f0: 2500, q: 1.5 }); chitter(t0 + 0.05); },
  scrabble(t0, { gain: g = 0.08, pan } = {}) { burst(t0, g, 0.002, 0.012, { type: 'lowpass', f0: 1200, q: 0.7, pan }); },
  falseLightReveal(t0) { note('sawtooth', 1200, 2400, t0, 0.35, 0.02, 0.38, { sweep: 0.4 }); burst(t0, 0.12, 0.02, 0.3, { type: 'highpass', f0: 2500, q: 0.7 }); },
  falseLightDeath(t0) { SOUNDS.death(t0); chitter(t0 + 0.15, { peak: 0.16 }); },
  bruteRoar(t0) {
    note('sawtooth', 70, 45, t0, 0.4, 0.05, 0.95, { sweep: 1.0 });
    burst(t0, 0.45, 0.35, 0.65, { type: 'lowpass', f0: 300, f1: 1800, q: 1, sweep: 0.6 });
  },
  bruteStep(t0, { gain: g = 0.35, pan } = {}) { burst(t0, g, 0.003, 0.16, { type: 'lowpass', f0: 90, q: 1, pan }); if (g > 0.15) burst(t0 + 0.01, g * 0.25, 0.002, 0.05, { type: 'bandpass', f0: 500, q: 0.8, pan }); },
  lanternCrunch(t0) {
    burst(t0, 0.5, 0.005, 0.15, { type: 'lowpass', f0: 600, q: 0.8 });
    for (let i = 0; i < 3; i++) note('square', 180, 0, t0 + 0.02 + i * 0.06, 0.2, 0.002, 0.03);
    burst(t0 + 0.05, 0.15, 0.01, 0.35, { type: 'highpass', f0: 3000, q: 0.5 });
  },
  snort(t0) { burst(t0, 0.3, 0.01, 0.2, { type: 'bandpass', f0: 250, q: 1.2 }); },
  bruteDeath(t0) {
    const lp = filt('lowpass', 4000, 1); lp.frequency.setValueAtTime(4000, t0); lp.frequency.exponentialRampToValueAtTime(100, t0 + 1.2);
    const g = gain(1.3); lp.connect(g); g.connect(state.master);
    burst(t0, 0.6, 0.005, 0.1, { type: 'lowpass', f0: 6000, q: 0.5, dest: lp });
    note('square', 60, 0, t0, 0.5, 0.01, 1.2, { dest: lp });
    note('square', 40, 0, t0 + 0.05, 0.4, 0.01, 0.35);
  },
};

/* ============================================================
   What to render: file name → (SOUNDS key, options)
   ============================================================ */
const TARGETS = [
  ['drip', 'drip'], ['pop', 'pop'], ['hubPop', 'hubPop'],
  ['step', 'step'], ['stepSprint', 'step', { sprint: true }], ['slosh', 'slosh'],
  ['flash', 'flash'], ['lantern', 'lantern'],
  ['pickupOil', 'pickup', { kind: 'oil' }], ['pickupRich', 'pickup', { kind: 'rich' }],
  ['pickupBundle', 'pickup', { kind: 'bundle' }], ['pickupQuest', 'pickup', { kind: 'quest' }],
  ['bank', 'bank'], ['sting', 'sting'], ['death', 'death'],
  ['npcFreed', 'npcFreed'], ['npcCaught', 'npcCaught'], ['npcRescued', 'npcRescued'],
  ['uiClick', 'uiClick'], ['uiError', 'uiError'], ['build', 'build'],
  ['contractComplete', 'contractComplete'], ['contractAccepted', 'contractAccepted'],
  ['toolGained', 'toolGained'], ['lightTech', 'lightTech'],
  ['lampOn', 'lampOn'], ['lampOff', 'lampOff'], ['topUp', 'topUp'],
  ['gate', 'gate'], ['shortcut', 'shortcut'], ['flameTier', 'flameTier'],
  // one descend cue; the player pitches it by 2^(lap/12) with PlaybackSettings::speed
  ['descend', 'descend', { lap: 0 }, 3.0, 11025],
  // the ending chords are three pure sines under 600 Hz: 11 kHz is transparent and saves 1.5 MB
  ['endingCage', 'ending', { id: 'cage' }, 7.0, 11025],
  ['endingDawn', 'ending', { id: 'dawn' }, 7.0, 11025],
  ['endingNight', 'ending', { id: 'night' }, 7.0, 11025],
  ['menuMove', 'menuMove'], ['menuSelect', 'menuSelect'], ['menuBack', 'menuBack'], ['menuOpen', 'menuOpen'],
  ['lampwightSigh', 'lampwightSigh'], ['lampSnuff', 'lampSnuff'],
  ['wardenClick', 'wardenClick'], ['wardenAlert', 'wardenAlert'], ['wardenStep', 'wardenStep'],
  ['wardenReturn', 'wardenReturn'], ['wardenDeath', 'wardenDeath'],
  ['plop', 'plop'], ['drownerSurge', 'drownerSurge'], ['drownerSink', 'drownerSink'],
  ['drownerDeath', 'drownerDeath'],
  ['falseLightPounce', 'falseLightPounce'], ['scrabble', 'scrabble'],
  ['falseLightReveal', 'falseLightReveal'], ['falseLightDeath', 'falseLightDeath'],
  ['bruteRoar', 'bruteRoar'], ['bruteStep', 'bruteStep'], ['lanternCrunch', 'lanternCrunch'],
  ['snort', 'snort'], ['bruteDeath', 'bruteDeath'],
];

/* ============================================================
   Render + WAV writing
   ============================================================ */
function wav16(samples, sr) {
  const n = samples.length, buf = Buffer.alloc(44 + n * 2);
  buf.write('RIFF', 0); buf.writeUInt32LE(36 + n * 2, 4); buf.write('WAVE', 8);
  buf.write('fmt ', 12); buf.writeUInt32LE(16, 16); buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(1, 22); buf.writeUInt32LE(sr, 24); buf.writeUInt32LE(sr * 2, 28);
  buf.writeUInt16LE(2, 32); buf.writeUInt16LE(16, 34);
  buf.write('data', 36); buf.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) {
    const v = Math.max(-1, Math.min(1, samples[i]));
    buf.writeInt16LE(Math.round(v * 32767), 44 + i * 2);
  }
  return buf;
}

async function renderOne(key, opts, dur, sr) {
  const ac = new OfflineAudioContext(1, Math.ceil(sr * dur), sr);
  state.ac = ac;
  state.hasPanner = false;
  state.master = ac.createGain(); state.master.gain.value = 1; state.master.connect(ac.destination);
  const n = Math.floor(sr * 2), nb = ac.createBuffer(1, n, sr), d = nb.getChannelData(0);
  for (let i = 0; i < n; i++) d[i] = Math.random() * 2 - 1;
  state.noise = nb;
  SOUNDS[key](0, opts || {});
  const rendered = await ac.startRendering();
  return rendered.getChannelData(0);
}

mkdirSync(OUT, { recursive: true });
const manifest = [];
let total = 0;
for (const [name, key, opts, dur, rate] of TARGETS) {
  const sr = rate || SR;
  rng = mulberry32(0x1D0C0FF);   // same noise buffer and rnd() stream for every sound
  const raw = await renderOne(key, opts, dur || 3.0, sr);
  let last = -1, peak = 0;
  for (let i = 0; i < raw.length; i++) {
    const a = Math.abs(raw[i]);
    if (a > peak) peak = a;
    if (a > SILENCE) last = i;
  }
  if (last < 0) throw new Error(`${name}: rendered silence`);
  const end = Math.min(raw.length, last + 1 + Math.ceil(TAIL * sr));
  const cut = raw.subarray(0, end);
  const norm = peak > 0 ? 1 / peak : 1;
  const out = new Float32Array(cut.length);
  for (let i = 0; i < cut.length; i++) out[i] = cut[i] * norm;
  const buf = wav16(out, sr);
  writeFileSync(join(OUT, `${name}.wav`), buf);
  total += buf.length;
  manifest.push({ name, file: `${name}.wav`, peak, samples: cut.length, sr, dur: cut.length / sr, bytes: buf.length });
  console.log(`${name.padEnd(18)} peak ${peak.toFixed(4)}  ${(cut.length / sr).toFixed(3)}s  ${sr} Hz  ${(buf.length / 1024).toFixed(1)} KiB`);
}

const ron = [
  '// Generated by tools/export/audio/render.mjs — do not edit by hand.',
  '// Every WAV is mono 16-bit PCM (44.1 kHz, or 11.025 kHz for the long low cues; each file\'s',
  '// header carries its own rate). Files are peak-normalised to 1.0 and `peak` is the amplitude',
  '// the prototype\'s Web Audio graph produced, so the player uses `volume = peak * master`.',
  '(',
  `  sample_rate: ${SR},`,   // the rate all but the long, low cues are rendered at
  '  sounds: [',
  ...manifest.map((m) => `    (name: "${m.name}", file: "${m.file}", peak: ${m.peak.toFixed(6)}, samples: ${m.samples}, sample_rate: ${m.sr}, duration: ${m.dur.toFixed(4)}),`),
  '  ],',
  ')',
  '',
].join('\n');
writeFileSync(join(OUT, 'manifest.ron'), ron);
console.log(`\n${manifest.length} sounds, ${(total / 1024).toFixed(1)} KiB of WAV + manifest.ron`);
