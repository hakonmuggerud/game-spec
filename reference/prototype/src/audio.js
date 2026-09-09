// audio.js — procedural WebAudio (DESIGN-v2 §1). No assets: oscillators, one looped 2 s white-noise
// buffer, biquad filters and gain envelopes. Graph: source → per-sound gain → master → destination.
//
// The AudioContext is created (and resume()d) on the first pointerdown/keydown/click on the document;
// every call before that is a no-op, and nothing here throws when WebAudio is unavailable (state.failed).
// One-shots come from the event bus (init registers listeners only); continuous layers (ambience drone,
// lamp crackle, footsteps, hunter presence, hub flame, water, drips) poll ctx in update().
// `audio` (the object with the `muted` getter) is what main.js puts on ctx.audio / window.__game.audio.
import { AUDIO } from './config.js';

let ctx = null;
const state = {
  ready: false, failed: false, vol: AUDIO.vol, muted: false,
  ac: null, master: null, noise: null, hasPanner: false,
  acc: 0,                       // seconds since the last parameter tick (params are pushed at ~30 Hz)
  popT: 0, hubPopT: 0, dripT: 3, // timers for lamp pops, hub flame pops, drips
  stepAcc: 0, lastX: null, lastZ: null, steps: 0,
  lastSting: -1e9, playedCount: 0, played: [], lastMode: null,
};
// Continuous voices (built once with the context): {g: GainNode, target: number, ...}
const V = { drone: null, lamp: null, hub: null, water: null, presence: [] };

// Local tuning (numbers from DESIGN-v2 §1; master/presence numbers come from config.AUDIO).
const TUNE = {
  tick: 1 / 30, tau: 0.1, droneTau: 0.5,               // droneTau 0.5 ≈ the 1.5 s crossfade
  droneSource: 0.025, droneWater: 0.025,
  lampCrackle: 0.012, lampLowMul: 3, lampLowOil: 15, popMin: 0.08, popMax: 0.3, popGain: 0.05,
  stepWalk: 0.55, stepSprint: 0.42, stepTeleport: 3,
  hubCrackle: 0.02, hubCrackleStep: 0.02, hubSine: 0.02, hubRange: 12,
  waterWash: 0.02,
  dripMin: 2, dripMax: 7, dripCisternMin: 0.6, dripCisternMax: 2,
  stateMul: { WANDER: 0.35, INVESTIGATE: 0.7, CHASE: 1, STAGGERED: 0.1 },
  duckMenu: 0.4, duckDead: 0.6,
};

/* ============================================================
   Context + helpers
   ============================================================ */
function ensure() {
  if (state.ready || state.failed) return state.ready;
  try {
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) { state.failed = true; return false; }
    const ac = new AC();
    state.ac = ac;
    state.master = ac.createGain();
    state.master.gain.value = state.muted ? 0 : state.vol;
    state.master.connect(ac.destination);
    state.hasPanner = typeof ac.createStereoPanner === 'function';
    // 2 s white noise, looped by every noise source
    const n = Math.floor(ac.sampleRate * 2), buf = ac.createBuffer(1, n, ac.sampleRate), d = buf.getChannelData(0);
    for (let i = 0; i < n; i++) d[i] = Math.random() * 2 - 1;
    state.noise = buf;
    buildVoices();
    state.ready = true;
    resume();
  } catch (e) {
    state.failed = true; state.ready = false;
    try { console.warn('[audio] unavailable:', e && e.message); } catch (e2) { /* ignore */ }
  }
  return state.ready;
}
function resume() {
  const ac = state.ac;
  if (!ac || ac.state !== 'suspended' || typeof ac.resume !== 'function') return;
  try { const p = ac.resume(); if (p && typeof p.catch === 'function') p.catch(() => {}); } catch (e) { /* ignore */ }
}
const now = () => state.ac.currentTime;
function gain(v) { const g = state.ac.createGain(); g.gain.value = v; return g; }
function osc(type, f) { const o = state.ac.createOscillator(); o.type = type; o.frequency.value = f; return o; }
function filt(type, f, q) { const b = state.ac.createBiquadFilter(); b.type = type; b.frequency.value = f; if (q !== undefined) b.Q.value = q; return b; }
function noiseSrc(loop = true) { const s = state.ac.createBufferSource(); s.buffer = state.noise; s.loop = loop; return s; }
function panner(v = 0) {
  if (!state.hasPanner) return null;
  const p = state.ac.createStereoPanner(); p.pan.value = Math.max(-1, Math.min(1, v)); return p;
}
// Per-sound output gain → (panner) → dest (master by default); returned node is what sources connect to.
function out(peak, pan, dest) {
  const g = gain(peak), target = dest || state.master;
  const p = pan !== undefined ? panner(pan) : null;
  if (p) { g.connect(p); p.connect(target); g.__pan = p; } else g.connect(target);
  return g;
}
function setTarget(param, v, t0, tau) { param.setTargetAtTime(v, t0, tau); }
// Attack/decay envelope on a gain param starting at t0; returns the end time.
function env(param, t0, peak, a, d) {
  param.cancelScheduledValues(t0);
  param.setValueAtTime(0.0001, t0);
  param.linearRampToValueAtTime(Math.max(0.0002, peak), t0 + a);
  param.exponentialRampToValueAtTime(0.0001, t0 + a + d);
  return t0 + a + d;
}
function stopAt(src, t, ...cleanup) {
  src.stop(t);
  src.onended = () => {
    try { src.disconnect(); for (const n of cleanup) { n.disconnect(); if (n.__pan) n.__pan.disconnect(); } } catch (e) { /* ignore */ }
  };
}
// Tone one-shot: type, f0→f1 (exponential) over `sweep` s, gain envelope peak/a/d, optional pan.
function note(type, f0, f1, t0, peak, a, d, { sweep = 0, pan, dest } = {}) {
  const o = osc(type, f0), g = out(0, pan, dest);
  o.frequency.setValueAtTime(f0, t0);
  if (f1 && f1 !== f0) o.frequency.exponentialRampToValueAtTime(f1, t0 + (sweep || a + d));
  const end = env(g.gain, t0, peak, a, d);
  o.connect(g); o.start(t0); stopAt(o, end + 0.05, g);
  return o;
}
// Noise one-shot through a filter: filter f0→f1 over `sweep` s, gain envelope.
function burst(t0, peak, a, d, { type = 'lowpass', f0 = 1000, f1 = 0, q = 1, sweep = 0, pan, dest } = {}) {
  const s = noiseSrc(true), b = filt(type, f0, q), g = out(0, pan, dest);
  b.frequency.setValueAtTime(f0, t0);
  if (f1 && f1 !== f0) b.frequency.exponentialRampToValueAtTime(f1, t0 + (sweep || a + d));
  const end = env(g.gain, t0, peak, a, d);
  s.connect(b); b.connect(g); s.start(t0); stopAt(s, end + 0.05, b, g);
  return s;
}
const clamp01 = (v) => Math.max(0, Math.min(1, v));
const rnd = (a, b) => a + Math.random() * (b - a);

/* ============================================================
   Continuous voices
   ============================================================ */
function buildVoices() {
  // Ambience drone: sine 38 + sine 57 (+3 cents) → lowpass 200; plus a water noise layer (Cistern).
  {
    const g = gain(0), lp = filt('lowpass', 200, 0.7);
    const o1 = osc('sine', 38), o2 = osc('sine', 57); o2.detune.value = 3;
    o1.connect(lp); o2.connect(lp); lp.connect(g); g.connect(state.master);
    const wg = gain(0), bp = filt('bandpass', 1200, 0.7), n = noiseSrc();
    n.connect(bp); bp.connect(wg); wg.connect(state.master);
    o1.start(); o2.start(); n.start();
    V.drone = { g, wg, o1, o2, target: 0, waterTarget: 0, base: 38 };
  }
  // Lamp crackle: noise → bandpass 3000 Q2 → gain (0.012, ×3 below 15 oil).
  {
    const g = gain(0), bp = filt('bandpass', 3000, 2), n = noiseSrc();
    n.connect(bp); bp.connect(g); g.connect(state.master); n.start();
    V.lamp = { g, target: 0 };
  }
  // Hub flame: crackle (noise → bandpass 1500) + sine 65 hum, scaled by tier and distance.
  {
    const g = gain(0), bp = filt('bandpass', 1500, 1.2), n = noiseSrc();
    n.connect(bp); bp.connect(g); g.connect(state.master); n.start();
    const hg = gain(0), o = osc('sine', 65); o.connect(hg); hg.connect(state.master); o.start();
    V.hub = { g, hg, target: 0, humTarget: 0 };
  }
  // Water wash while standing in a W cell: noise → lowpass 400.
  {
    const g = gain(0), lp = filt('lowpass', 400, 0.5), n = noiseSrc();
    n.connect(lp); lp.connect(g); g.connect(state.master); n.start();
    V.water = { g, target: 0 };
  }
}
// Hunter presence voice: saw 42 + sine 30 → lowpass (120 + 500·k) → tremolo (LFO 0.6 Hz ±30 %) → gain 0.22·k → pan.
function presenceVoice() {
  const saw = osc('sawtooth', 42), sin = osc('sine', 30), lp = filt('lowpass', 120, 1);
  const trem = gain(1), lfo = osc('sine', 0.6), lfoG = gain(0.3);
  lfo.connect(lfoG); lfoG.connect(trem.gain);
  const g = gain(0), pan = panner(0);
  saw.connect(lp); sin.connect(lp); lp.connect(trem); trem.connect(g);
  if (pan) { g.connect(pan); pan.connect(state.master); } else g.connect(state.master);
  saw.start(); sin.start(); lfo.start();
  return { g, lp, pan, target: 0, k: 0, nodes: [saw, sin, lfo] };
}

/* ============================================================
   One-shot table (name → scheduler). play(name, opts) looks these up.
   ============================================================ */
function chime(t0, freqs, { gap = 0.09, decay = 1.2, peak = 0.25, type = 'triangle' } = {}) {
  freqs.forEach((f, i) => note(type, f, 0, t0 + i * gap, peak, 0.01, decay));
}
const SOUNDS = {
  // Drips: sine sweep 1800→600 over 0.06, g0.15, decay 0.25, random pan ±0.8
  drip(t0) { note('sine', 1800, 600, t0, 0.15, 0.005, 0.25, { sweep: 0.06, pan: rnd(-0.8, 0.8) }); },
  // Lamp pop: 3 ms noise click
  pop(t0, { peak = TUNE.popGain } = {}) { burst(t0, peak, 0.001, 0.003, { type: 'bandpass', f0: 2500, q: 1 }); },
  hubPop(t0, { peak = 0.04 } = {}) { burst(t0, peak, 0.001, 0.008, { type: 'bandpass', f0: 1200, q: 1 }); },
  // Footsteps: noise 40 ms → lowpass 500 g0.12 (walk) / 900 g0.2 (sprint); water adds a slosh
  step(t0, { sprint = false, water = false } = {}) {
    burst(t0, sprint ? 0.2 : 0.12, 0.004, 0.036, { type: 'lowpass', f0: sprint ? 900 : 500, q: 0.7 });
    if (water) play('slosh');
  },
  slosh(t0) {
    note('sine', 300, 120, t0, 0.15, 0.01, 0.12, { sweep: 0.12 });
    burst(t0, 0.08, 0.01, 0.18, { type: 'bandpass', f0: 900, f1: 300, q: 0.8, sweep: 0.18 });
  },
  // Flash whoosh: noise → bandpass sweep 400→4000 over 0.18, g0.35, decay 0.25
  flash(t0) { burst(t0, 0.35, 0.02, 0.25, { type: 'bandpass', f0: 400, f1: 4000, q: 1.2, sweep: 0.18 }); },
  // Lantern plant: sine 220 0.2 s + noise hiss 0.4 s, then a 660 Hz blip
  lantern(t0) {
    note('sine', 220, 0, t0, 0.2, 0.01, 0.2);
    burst(t0, 0.1, 0.02, 0.4, { type: 'highpass', f0: 2000, q: 0.5 });
    note('sine', 660, 0, t0 + 0.42, 0.15, 0.005, 0.05);
  },
  // Pickup: triangle 880→1320 (2 × 90 ms); rich relic 3 notes (+1760); bundle low 330; quest 660/990
  pickup(t0, { kind = 'oil' } = {}) {
    if (kind === 'bundle') chime(t0, [330, 440], { gap: 0.12, decay: 0.4, peak: 0.22 });
    else if (kind === 'rich') chime(t0, [880, 1320, 1760], { gap: 0.09, decay: 0.3, peak: 0.2 });
    else if (kind === 'quest') chime(t0, [660, 990], { gap: 0.09, decay: 0.35, peak: 0.2 });
    else chime(t0, [880, 1320], { gap: 0.09, decay: 0.25, peak: 0.2 });
  },
  // Bank chime: triangle 523/659/784 staggered 90 ms, decay 1.2, g0.25
  bank(t0) { chime(t0, [523, 659, 784]); },
  // Chase sting: saw 110→55 over 0.5 + noise swell, g0.4
  sting(t0) {
    note('sawtooth', 110, 55, t0, 0.4, 0.02, 0.6, { sweep: 0.5 });
    burst(t0, 0.25, 0.3, 0.4, { type: 'lowpass', f0: 600, f1: 2500, q: 1, sweep: 0.5 });
  },
  // Death hit: noise 0.1 s g0.6 + square 60 Hz decay 1.2 through a lowpass sweep 4000→100
  death(t0) {
    const lp = filt('lowpass', 4000, 1); lp.frequency.setValueAtTime(4000, t0); lp.frequency.exponentialRampToValueAtTime(100, t0 + 1.2);
    const g = gain(1); lp.connect(g); g.connect(state.master);
    burst(t0, 0.6, 0.005, 0.1, { type: 'lowpass', f0: 6000, q: 0.5, dest: lp });
    note('square', 60, 0, t0, 0.5, 0.01, 1.2, { dest: lp });
    setTimeout(() => { try { lp.disconnect(); g.disconnect(); } catch (e) { /* ignore */ } }, 1600);
  },
  // NPCs
  npcFreed(t0) { note('triangle', 440, 880, t0, 0.22, 0.02, 0.35, { sweep: 0.3 }); },
  npcCaught(t0) { note('triangle', 440, 110, t0, 0.25, 0.02, 0.45, { sweep: 0.4 }); },
  npcRescued(t0) { chime(t0, [1046, 1318, 1568]); },
  // UI
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
  // Extras (not in the §1 table): small cues for lamp/gate/top-up/tier
  lampOn(t0) { note('square', 900, 0, t0, 0.04, 0.002, 0.012); burst(t0 + 0.01, 0.06, 0.01, 0.12, { type: 'bandpass', f0: 3000, q: 1.5 }); },
  lampOff(t0) { note('square', 500, 0, t0, 0.04, 0.002, 0.02); },
  topUp(t0) { note('sine', 200, 90, t0, 0.15, 0.02, 0.25, { sweep: 0.25 }); note('sine', 260, 140, t0 + 0.12, 0.1, 0.02, 0.2, { sweep: 0.2 }); },
  gate(t0) {
    note('square', 180, 120, t0, 0.18, 0.005, 0.5, { sweep: 0.4 });
    burst(t0, 0.25, 0.003, 0.25, { type: 'bandpass', f0: 2200, f1: 600, q: 2, sweep: 0.25 });
  },
  // shortcut (DESIGN.md §3.6): the gate cue pitched down, a chain rattle and a stone boom under it.
  // PLACEHOLDER MIX — the audio agent owns the final cue; keep the name `shortcut`.
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
  // Source descent cue (endgame `lap`): two-voice sine swell a semitone per lap, sweeping up a fifth over 2.2 s
  descend(t0, { lap = 1 } = {}) {
    const f0 = 55 * Math.pow(2, (lap | 0) / 12), dur = 2.2;
    for (const mul of [1, 1.498]) note('sine', f0 * mul, f0 * mul * 1.5, t0, 0.1, dur * 0.45, dur * 0.55, { sweep: dur });
    burst(t0, 0.05, dur * 0.5, dur * 0.5, { type: 'lowpass', f0: 200 + 40 * lap, f1: 600 + 80 * lap, q: 0.7, sweep: dur });
  },
  // Ending: held sine chord 6 s (cage 262/330/392; dawn 392/494/587; night 55+58 beating)
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
};

/* ============================================================
   Public API
   ============================================================ */
export function init(c) {
  ctx = c;
  const sa = (ctx.save && ctx.save.audio) || {};
  state.vol = Number.isFinite(sa.vol) ? Math.max(0, Math.min(1, sa.vol)) : AUDIO.vol;
  state.muted = !!sa.muted;
  // browser gesture rule: create/resume the context on the first user input
  const gesture = () => { if (ensure()) resume(); };
  for (const evn of ['pointerdown', 'keydown', 'click', 'touchstart']) document.addEventListener(evn, gesture, { passive: true });
  const ev = ctx.events;
  const on = (name, fn) => ev.on(name, fn);
  on('zoneEnter', ({ zoneId }) => { setDroneBase(zoneId === 'ossuary' ? 31 : 38); state.dripT = rnd(0.5, 2); state.stepAcc = 0; state.lastX = null; });
  on('hubEnter', () => { setDroneBase(38); state.lastX = null; });
  on('zoneExit', () => { state.lastX = null; });
  on('flash', () => play('flash'));
  on('lantern', () => play('lantern'));
  on('pickup', ({ kind }) => play('pickup', { kind }));
  on('bank', ({ pts }) => { if (pts > 0) play('bank'); else play('uiClick'); });
  on('hunterState', ({ id, state: s, prev } = {}) => {
    if (s !== 'CHASE' || prev === 'CHASE' || kindOf(profileOf(id)) !== 'growl') return;   // the roster has its own cues
    const t = ctx.state.time;
    if (t - state.lastSting >= AUDIO.stingGap) { state.lastSting = t; play('sting'); }
  });
  on('death', ({ hunterId } = {}) => { if (!DEATH_BY[profileOf(hunterId)]) play('death'); });
  on('npcFreed', () => play('npcFreed'));
  on('npcCaught', () => play('npcCaught'));
  on('npcRescued', () => play('npcRescued'));
  on('contractAccepted', () => play('contractAccepted'));
  on('contractComplete', () => play('contractComplete'));
  on('toolGained', () => play('toolGained'));
  on('build', () => play('build'));
  on('lightTech', () => play('lightTech'));
  on('ending', ({ id, choice }) => play('ending', { id: id || choice }));
  on('uiClick', () => play('uiClick'));
  on('uiError', () => play('uiError'));
  on('menuOpen', () => play('uiClick'));
  on('menuClose', () => play('uiClick'));
  on('lampToggle', ({ on: lit }) => play(lit ? 'lampOn' : 'lampOff'));
  on('topUp', () => play('topUp'));
  on('gateOpened', () => play('gate'));
  on('shortcutOpened', () => play('shortcut'));
  on('gateLocked', () => play('uiError'));
  on('waterEnter', () => play('slosh'));
  on('flameTier', ({ initial }) => { if (!initial) play('flameTier'); });
}
function setDroneBase(f) {
  if (!state.ready || !V.drone) return;
  V.drone.base = f;
  const t0 = now();
  V.drone.o1.frequency.setTargetAtTime(f, t0, TUNE.droneTau);
  V.drone.o2.frequency.setTargetAtTime(f * 1.5, t0, TUNE.droneTau);
}

// play(name, opts): fire a one-shot from the table. Returns true if scheduled (false before the first
// gesture, while muted, or for an unknown name).
export function play(name, opts) {
  if (!state.ready || state.muted || !SOUNDS[name]) return false;
  if (state.ac.state !== 'running') { resume(); return false; }
  try { SOUNDS[name](now(), opts || {}); } catch (e) { return false; }
  state.playedCount += 1;
  state.played.push(name); if (state.played.length > 24) state.played.shift();
  return true;
}
export function setVolume(v) {
  state.vol = Math.max(0, Math.min(1, Math.round(v * 100) / 100));
  if (ctx) {
    if (ctx.save && ctx.save.audio) ctx.save.audio.vol = state.vol;
    ctx.events.emit('toast', { msg: `Volume ${Math.round(state.vol * 100)}%` });
  }
  return state.vol;
}
export function setMute(m) {
  state.muted = !!m;
  if (ctx) {
    if (ctx.save && ctx.save.audio) ctx.save.audio.muted = state.muted;
    ctx.events.emit('toast', { msg: state.muted ? 'Sound off' : 'Sound on' });
  }
  if (state.ready) setTarget(state.master.gain, state.muted ? 0 : state.vol, now(), 0.02);
  return state.muted;
}
export function toggleMute() { return setMute(!state.muted); }
export const isMuted = () => state.muted;

/* ============================================================
   update(ctx, dt): continuous layers, polled from ctx (~30 Hz parameter pushes)
   ============================================================ */
export function update(c, dt) {
  if (!state.ready) return;
  state.acc += dt;
  if (state.acc < TUNE.tick) return;
  const step = state.acc; state.acc = 0;
  try { tick(c, step); } catch (e) { /* audio must never break the frame */ }
}
function tick(c, dt) {
  const ac = state.ac, t0 = ac.currentTime, tau = TUNE.tau;
  const st = c.state, p = c.player, mode = st.mode;
  const inZone = mode === 'ZONE' || mode === 'DYING' || (mode === 'MENU' && st.prevMode === 'ZONE') || (mode === 'ENDING');
  const inHub = mode === 'HUB' || mode === 'TITLE' || (mode === 'MENU' && st.prevMode === 'HUB');
  const live = (mode === 'ZONE' || mode === 'HUB') && !st.paused;
  const zoneId = c.zone && c.zone.id;

  // master: volume, mute, ducking in menus / while paused / on the death screen
  let duck = 1;
  if (mode === 'MENU' || st.paused) duck = TUNE.duckMenu;
  else if (mode === 'DEAD') duck = TUNE.duckDead;
  setTarget(state.master.gain, state.muted ? 0 : state.vol * duck, t0, 0.05);
  if (state.muted) return; // everything else may idle at its last target; one-shots are skipped in play()

  // ambience drone
  {
    let g = 0;
    if (inZone) g = AUDIO.droneZone + (zoneId === 'source' ? TUNE.droneSource * (p.lap | 0) : 0);
    else if (inHub || mode === 'DEAD') g = AUDIO.droneHub;
    const wg = inZone && zoneId === 'cistern' ? TUNE.droneWater : 0;
    if (g !== V.drone.target) { V.drone.target = g; setTarget(V.drone.g.gain, g, t0, TUNE.droneTau); }
    if (wg !== V.drone.waterTarget) { V.drone.waterTarget = wg; setTarget(V.drone.wg.gain, wg, t0, TUNE.droneTau); }
  }

  // lamp crackle + pops
  {
    const lit = p.lampOn && (mode === 'ZONE' || mode === 'DYING' || mode === 'MENU');
    const low = p.oil < TUNE.lampLowOil;
    const g = lit ? TUNE.lampCrackle * (low ? TUNE.lampLowMul : 1) : 0;
    if (g !== V.lamp.target) { V.lamp.target = g; setTarget(V.lamp.g.gain, g, t0, tau); }
    if (lit && mode === 'ZONE' && !st.paused) {
      state.popT -= dt;
      if (state.popT <= 0) { state.popT = rnd(TUNE.popMin, TUNE.popMax); play('pop', { peak: TUNE.popGain * (low ? 1.6 : 1) }); }
    }
  }

  // footsteps by distance walked (position deltas; teleports ignored)
  {
    if (live && state.lastX !== null) {
      const d = Math.hypot(p.x - state.lastX, p.z - state.lastZ);
      if (d < TUNE.stepTeleport && p.moving) {
        state.stepAcc += d;
        const len = p.sprinting ? TUNE.stepSprint : TUNE.stepWalk;
        if (state.stepAcc >= len) {
          state.stepAcc -= len; state.steps += 1;
          play('step', { sprint: !!p.sprinting, water: !!p.inWater });
        }
      } else if (d >= TUNE.stepTeleport) state.stepAcc = 0;
    }
    state.lastX = p.x; state.lastZ = p.z;
    const wg = live && p.inWater ? TUNE.waterWash : 0;
    if (wg !== V.water.target) { V.water.target = wg; setTarget(V.water.g.gain, wg, t0, tau); }
  }

  // creature presence: one voice per hunter index, kind by profile (see the roster section below)
  presenceTick(c, dt, t0);

  // hub flame: crackle + hum by tier, faded by distance
  {
    const flame = c.hub && c.hub.flame;
    let g = 0, hum = 0, tier = 1, prox = 0;
    if (flame && inHub) {
      tier = flame.tier | 0 || 1;
      let fx0 = 0, fz0 = 0;
      if (flame.group && flame.group.position) { fx0 = flame.group.position.x; fz0 = flame.group.position.z; }
      else if (c.hub.map && c.hub.map.flame && c.maps && c.maps.center) { const q = c.maps.center(c.hub.map, c.hub.map.flame.cx, c.hub.map.flame.cz); fx0 = q.x; fz0 = q.z; }
      prox = clamp01(1 - Math.hypot(p.x - fx0, p.z - fz0) / TUNE.hubRange);
      g = (TUNE.hubCrackle + TUNE.hubCrackleStep * (tier - 1)) * prox;
      hum = TUNE.hubSine * tier * prox;
    }
    if (Math.abs(g - V.hub.target) > 0.0005 || (g === 0 && V.hub.target !== 0)) { V.hub.target = g; setTarget(V.hub.g.gain, g, t0, tau); }
    if (Math.abs(hum - V.hub.humTarget) > 0.0005 || (hum === 0 && V.hub.humTarget !== 0)) { V.hub.humTarget = hum; setTarget(V.hub.hg.gain, hum, t0, tau); }
    if (g > 0.002 && mode === 'HUB' && !st.paused) {
      state.hubPopT -= dt;
      if (state.hubPopT <= 0) { state.hubPopT = rnd(0.15, 0.5) / tier; play('hubPop', { peak: 0.05 * prox }); }
    }
  }

  // drips (zone timer)
  if (mode === 'ZONE' && !st.paused) {
    state.dripT -= dt;
    if (state.dripT <= 0) {
      state.dripT = zoneId === 'cistern' ? rnd(TUNE.dripCisternMin, TUNE.dripCisternMax) : rnd(TUNE.dripMin, TUNE.dripMax);
      play('drip');
    }
  }
  state.lastMode = mode;
}

// stats(): snapshot for tests/debugging (window.__game.audio.stats()).
export function stats() {
  const r = {
    ready: state.ready, failed: state.failed, ctxState: state.ac ? state.ac.state : null, muted: state.muted, vol: state.vol,
    steps: state.steps, playedCount: state.playedCount, played: state.played.slice(-12),
    master: state.ready ? +state.master.gain.value.toFixed(3) : 0,
    drone: V.drone ? V.drone.target : 0, droneWater: V.drone ? V.drone.waterTarget : 0, droneBase: V.drone ? V.drone.base : 0,
    lamp: V.lamp ? V.lamp.target : 0, water: V.water ? V.water.target : 0,
    hub: V.hub ? +V.hub.target.toFixed(4) : 0, hubHum: V.hub ? +V.hub.humTarget.toFixed(4) : 0,
    presence: V.presence.map(v => +v.target.toFixed(4)), presenceK: V.presence.map(v => +v.k.toFixed(3)),
    presenceKind: V.presence.map(v => v.kind || 'growl'), lure: V.presence.map(v => +(v.lureTarget || 0).toFixed(4)),
    names: Object.keys(SOUNDS),
  };
  return r;
}
// ensureContext(): test hook — create the context now (the same thing the gesture listener does).
export const ensureContext = () => ensure();

export const audio = {
  init, update, play, setVolume, setMute, toggleMute, stats, ensureContext,
  get muted() { return state.muted; },
  get vol() { return state.vol; },
  get ready() { return state.ready; },
  get failed() { return state.failed; },
  get context() { return state.ac; },
};

/* ============================================================
   Menu cues (appended): a soft tick when the selection moves, a click on select, a lower tick going back.
   main.js emits menuMove / menuSelect / menuBack from the main and pause menus (uiClick/uiError are reused for
   the rest). Registered by wrapping audio.init so the table above stays untouched.
   ============================================================ */
SOUNDS.menuMove = (t0) => { note('square', 720, 0, t0, 0.05, 0.002, 0.012); };
SOUNDS.menuSelect = (t0) => { note('square', 1200, 0, t0, 0.08, 0.002, 0.015); note('triangle', 1600, 0, t0 + 0.03, 0.06, 0.002, 0.05); };
SOUNDS.menuBack = (t0) => { note('square', 420, 0, t0, 0.06, 0.002, 0.03); };
SOUNDS.menuOpen = (t0) => { note('triangle', 330, 0, t0, 0.1, 0.005, 0.12); note('triangle', 495, 0, t0 + 0.06, 0.08, 0.005, 0.15); };
{
  const baseInit = audio.init;
  audio.init = function (c) {
    baseInit(c);
    c.events.on('menuMove', () => play('menuMove'));
    c.events.on('menuSelect', () => play('menuSelect'));
    c.events.on('menuBack', () => play('menuBack'));
    c.events.on('pauseOpen', () => play('menuOpen'));
    c.events.on('title', () => play('menuBack'));
  };
}

/* ============================================================
   Creature roster (DESIGN.md §5) — per-profile presence voices + the new one-shots.
   Presence voices (V.presence[i], one per hunter index, rebuilt when the profile at that index changes):
     base/fast → growl (as before) · lampwight → whistle ("breath through a keyhole") · warden → stone grind that
     scales with its sweep speed, a click at each reversal, treads in CHASE/RETURN · drowner → surge wash (the
     Cistern water noise ×4, by its speed) + plops on a timer · falseLight → NO presence (silence is the lure); a
     separate lure layer (fake crackle 0.6 × the lamp's + a faint glass chime) runs only while it is LIT and stops
     the instant it goes dark; scrabbling at 12 Hz in POUNCE · brute → breathing drone; stride thuds come from
     `creatureStep` events (g 0.35 × falloff ≤ 26 u).
   One-shots hang off: hunterState (lampwight DRAWN sigh, brute CHASE roar, base/fast CHASE sting), lampSnuffed,
   lanternSmashed, wardenAlert, wardenReturn, drownerSurge, drownerSink, falseLightPounce, falseLightReveal,
   flashResisted, creatureStep, and death (per-killer variants). Everything is a no-op before the AudioContext exists.
   ============================================================ */
const CTUNE = {
  whistle: { peak: 0.10, range: 20, mul: { DRIFT: 0.5, WANDER: 0.5, DRAWN: 1, CHASE: 1, SNUFF: 1, SATED: 0.4, STAGGERED: 0.1 } },
  grind: { peak: 0.06, range: 14, sweep: 0.35, stepChase: 0.45, stepReturn: 0.6, stepRange: 14 },
  wash: { peak: 0.08, range: 12, speed: 5, mul: { SURGE: 1, LURK: 0.5, SURFACING: 0.6, SINK: 0.4 }, plopMin: 2, plopMax: 5, plopRange: 12 },
  lure: { range: 6, crackle: 0.6, chime: 0.012, tau: 0.02, scrabbleHz: 12, scrabbleRange: 12 },
  breath: { peak: 0.12, range: 20, mul: { WANDER: 0.6, INVESTIGATE: 0.8, CHASE: 1 }, stepRange: 26, stepGain: 0.35 },
  growlMul: TUNE.stateMul,
};
const KIND_OF = { base: 'growl', fast: 'growl', lampwight: 'whistle', warden: 'grind', drowner: 'wash', falseLight: 'lure', brute: 'breath' };
const kindOf = (profile) => KIND_OF[profile] || 'growl';
const profileOf = (id) => { const h = ctx && ctx.hunters && id != null ? ctx.hunters[id] : null; return h ? (h.profile || 'base') : 'base'; };
const falloff = (d, range) => clamp01(1 - d / range);
// pan of a world position relative to the player's facing (−1 left … +1 right), scaled 0.8 like the hunter voices
function panAt(x, z) {
  if (!ctx) return 0;
  const p = ctx.player, dx = x - p.x, dz = z - p.z, d = Math.hypot(dx, dz);
  if (d < 0.01) return 0;
  const rx = Math.cos(p.yaw), rz = -Math.sin(p.yaw);
  return Math.max(-1, Math.min(1, (dx * rx + dz * rz) / d)) * 0.8;
}
const distTo = (x, z) => ctx ? Math.hypot(x - ctx.player.x, z - ctx.player.z) : 0;

/* ---------- voice builders: {kind, g, pan, target, k, nodes[], ...} ---------- */
function voiceOut() {
  const g = gain(0), pan = panner(0);
  if (pan) { g.connect(pan); pan.connect(state.master); } else g.connect(state.master);
  return { g, pan };
}
function whistleVoice() {
  const { g, pan } = voiceOut();
  const o = osc('sine', 660), vib = osc('sine', 4), vibG = gain(12), trem = gain(0.75), tremLfo = osc('sine', 0.2), tremG = gain(0.25);
  vib.connect(vibG); vibG.connect(o.frequency);
  tremLfo.connect(tremG); tremG.connect(trem.gain);
  const bp = filt('bandpass', 660, 6);
  o.connect(bp); bp.connect(trem); trem.connect(g);
  o.start(); vib.start(); tremLfo.start();
  return { kind: 'whistle', g, pan, target: 0, k: 0, nodes: [o, vib, tremLfo] };
}
function grindVoice() {
  const { g, pan } = voiceOut();
  const saw = osc('sawtooth', 28), lp = filt('lowpass', 90, 1.2);
  saw.connect(lp); lp.connect(g); saw.start();
  return { kind: 'grind', g, pan, target: 0, k: 0, nodes: [saw], lastYaw: null, lastDir: 0, stepT: 0, extStepT: -1e9 };
}
function washVoice() {
  const { g, pan } = voiceOut();
  const n = noiseSrc(), lp = filt('lowpass', 400, 0.5);
  n.connect(lp); lp.connect(g); n.start();
  return { kind: 'wash', g, pan, target: 0, k: 0, nodes: [n], lastX: null, lastZ: null, speed: 0, plopT: rnd(1, 3) };
}
function lureVoice() {
  const { g, pan } = voiceOut();
  const n = noiseSrc(), bp = filt('bandpass', 3000, 2), cg = gain(1);
  n.connect(bp); bp.connect(cg); cg.connect(g); n.start();
  const chime = gain(0.6), trem = gain(0.7), lfo = osc('sine', 0.9), lfoG = gain(0.3);
  lfo.connect(lfoG); lfoG.connect(trem.gain);
  const o1 = osc('sine', 1320), o2 = osc('sine', 1980); o2.detune.value = 5;
  o1.connect(trem); o2.connect(trem); trem.connect(chime); chime.connect(g);
  o1.start(); o2.start(); lfo.start();
  return { kind: 'lure', g, pan, target: 0, k: 0, lureTarget: 0, nodes: [n, o1, o2, lfo], scrabT: 0 };
}
function breathVoice() {
  const { g, pan } = voiceOut();
  const o = osc('sine', 38), n = noiseSrc(), lp = filt('lowpass', 120, 0.8), lfo = osc('sine', 0.35), lfoG = gain(0.4), trem = gain(0.7);
  lfo.connect(lfoG); lfoG.connect(trem.gain);
  o.connect(trem); n.connect(lp); lp.connect(trem); trem.connect(g);
  o.start(); n.start(); lfo.start();
  return { kind: 'breath', g, pan, target: 0, k: 0, nodes: [o, n, lfo] };
}
function growlVoice() { const v = presenceVoice(); v.kind = 'growl'; v.nodes = []; return v; }
const VOICE_BUILDERS = { growl: growlVoice, whistle: whistleVoice, grind: grindVoice, wash: washVoice, lure: lureVoice, breath: breathVoice };
function stopVoice(v) {
  try {
    v.g.gain.cancelScheduledValues(0); v.g.gain.value = 0;
    for (const n of v.nodes || []) { try { n.stop(); } catch (e) { /* ignore */ } }
    setTimeout(() => { try { v.g.disconnect(); if (v.pan) v.pan.disconnect(); } catch (e) { /* ignore */ } }, 200);
  } catch (e) { /* ignore */ }
}
// voiceFor(i, kind): the voice at index i, rebuilt when the kind changed.
function voiceFor(i, kind) {
  let v = V.presence[i];
  if (v && v.kind !== kind) { stopVoice(v); v = null; }
  if (!v) { v = VOICE_BUILDERS[kind](); v.kind = kind; }
  V.presence[i] = v;
  return v;
}
const setGain = (v, g, t0, tau) => {
  if (Math.abs(g - v.target) > 0.0005 || (g === 0 && v.target !== 0)) { v.target = g; setTarget(v.g.gain, g, t0, tau); }
};

/* ---------- the per-frame presence tick (called from tick()) ---------- */
function presenceTick(c, dt, t0) {
  const tau = TUNE.tau, p = c.player, mode = c.state.mode, hs = c.hunters || [], live = mode === 'ZONE' && !c.state.paused;
  for (let i = 0; i < Math.max(hs.length, V.presence.length); i++) {
    const h = hs[i];
    const kind = h ? kindOf(h.profile) : (V.presence[i] ? V.presence[i].kind : 'growl');
    const v = voiceFor(i, kind);
    const on = !!(h && h.active && mode === 'ZONE');
    const d = on ? Math.hypot(h.x - p.x, h.z - p.z) : 1e9;
    const pan = on ? panAt(h.x, h.z) : 0;
    let g = 0, k = 0;
    switch (kind) {
      case 'growl': {
        if (on) k = falloff(d, AUDIO.presenceRange) * (CTUNE.growlMul[h.state] !== undefined ? CTUNE.growlMul[h.state] : 0.35);
        g = AUDIO.presenceMax * k;
        if (Math.abs(g - v.target) > 0.0005 || (g === 0 && v.target !== 0)) { v.target = g; setTarget(v.g.gain, g, t0, tau); setTarget(v.lp.frequency, 120 + 500 * k, t0, tau); }
        break;
      }
      case 'whistle': {
        const T = CTUNE.whistle;
        if (on) k = falloff(d, T.range) * (T.mul[h.state] !== undefined ? T.mul[h.state] : 0.5);
        g = T.peak * k; setGain(v, g, t0, tau);
        break;
      }
      case 'grind': {
        const T = CTUNE.grind;
        let facing = on ? h.yaw : 0;
        if (on && h.group && h.group.userData && h.group.userData.head) facing += h.group.userData.head.rotation.y;
        let speed = 0;
        if (on && v.lastYaw !== null && dt > 0) {
          let dy = facing - v.lastYaw; while (dy > Math.PI) dy -= Math.PI * 2; while (dy < -Math.PI) dy += Math.PI * 2;
          speed = Math.abs(dy) / dt;
          const dir = Math.abs(dy) > 1e-4 ? Math.sign(dy) : 0;
          if (dir && v.lastDir && dir !== v.lastDir && d <= T.range) play('wardenClick', { gain: 0.05 * falloff(d, T.range), pan });   // reversal click
          if (dir) v.lastDir = dir;
        }
        v.lastYaw = on ? facing : null;
        if (on) k = falloff(d, T.range) * clamp01(speed / T.sweep);
        g = T.peak * k; setGain(v, g, t0, tau);
        // treads: CHASE every 0.45 s, RETURN every 0.6 s — unless creatureStep events are driving them
        if (on && live && (h.state === 'CHASE' || h.state === 'RETURN') && d <= T.stepRange && c.state.time - v.extStepT > 1.5) {
          v.stepT -= dt;
          if (v.stepT <= 0) { v.stepT = h.state === 'CHASE' ? T.stepChase : T.stepReturn; play('wardenStep', { gain: 0.25 * falloff(d, T.stepRange), pan }); }
        } else if (!on) v.stepT = 0;
        break;
      }
      case 'wash': {
        const T = CTUNE.wash;
        if (on && v.lastX !== null && dt > 0) { const s = Math.hypot(h.x - v.lastX, h.z - v.lastZ) / dt; v.speed = s < 40 ? s : v.speed; }
        v.lastX = on ? h.x : null; v.lastZ = on ? h.z : null;
        const mul = on ? (T.mul[h.state] !== undefined ? T.mul[h.state] : 0) : 0;
        if (on) k = falloff(d, T.range) * mul * clamp01(0.35 + v.speed / T.speed);
        g = T.peak * k; setGain(v, g, t0, tau);
        if (on && live && d <= T.plopRange) {
          v.plopT -= dt;
          if (v.plopT <= 0) { v.plopT = rnd(T.plopMin, T.plopMax); play('plop', { gain: 0.14 * falloff(d, T.plopRange), pan }); }
        }
        break;
      }
      case 'lure': {
        const T = CTUNE.lure;
        // presence stays 0 (stats().presence reports it): the lure layer is the only sound it makes
        const lit = on && h.state === 'LIT' && d <= T.range;
        const lg = lit ? (T.crackle * TUNE.lampCrackle + T.chime) * falloff(d, T.range) : 0;
        if (Math.abs(lg - v.lureTarget) > 0.0005 || (lg === 0 && v.lureTarget !== 0)) { v.lureTarget = lg; setTarget(v.g.gain, lg, t0, lg === 0 ? T.tau : tau); }
        v.target = 0; k = 0;
        if (on && live && h.state === 'POUNCE' && d <= T.scrabbleRange) {
          v.scrabT -= dt;
          if (v.scrabT <= 0) { v.scrabT = 1 / T.scrabbleHz; play('scrabble', { gain: 0.08 * falloff(d, T.scrabbleRange), pan }); }
        } else v.scrabT = 0;
        break;
      }
      case 'breath': {
        const T = CTUNE.breath;
        if (on) k = falloff(d, T.range) * (T.mul[h.state] !== undefined ? T.mul[h.state] : 0.6);
        g = T.peak * k; setGain(v, g, t0, tau);
        break;
      }
      default: break;
    }
    v.k = k;
    if (v.pan && (k > 0 || (v.lureTarget || 0) > 0)) setTarget(v.pan.pan, pan, t0, tau);
  }
}

/* ---------- one-shots ---------- */
function chitter(t0, { peak = 0.12, pan } = {}) { for (let i = 0; i < 8; i++) note('square', 900 + (i % 2) * 60, 0, t0 + i * 0.044, peak, 0.002, 0.02, { pan }); }
function bell(t0, f, decay, peak) { note('sine', f, 0, t0, peak, 0.005, decay); note('sine', f * 2.76, 0, t0, peak * 0.35, 0.005, decay * 0.5); }
Object.assign(SOUNDS, {
  // Lampwight: DRAWN → a rising sigh; snuff → hard exhale + the lamp crackle cut + a 55 Hz thud
  lampwightSigh(t0) { burst(t0, 0.2, 0.5, 0.35, { type: 'bandpass', f0: 300, f1: 1200, q: 1.5, sweep: 0.8 }); },
  lampSnuff(t0) {
    burst(t0, 0.4, 0.01, 0.14, { type: 'highpass', f0: 1500, q: 0.5 });
    burst(t0 + 0.02, 0.2, 0.05, 0.3, { type: 'bandpass', f0: 600, f1: 200, q: 1, sweep: 0.3 });
    note('sine', 55, 0, t0 + 0.05, 0.3, 0.01, 0.45);
    if (V.lamp) { V.lamp.target = 0; V.lamp.g.gain.cancelScheduledValues(t0); V.lamp.g.gain.setValueAtTime(0, t0); }
  },
  // Warden: reversal click, alert clang + horn, treads, "walks home", bell on a kill
  wardenClick(t0, { gain: g = 0.05, pan } = {}) { burst(t0, g, 0.001, 0.01, { type: 'bandpass', f0: 1800, q: 2, pan }); },
  wardenAlert(t0) {
    note('square', 220, 110, t0, 0.3, 0.005, 0.3, { sweep: 0.3 });
    burst(t0, 0.3, 0.003, 0.08, { type: 'bandpass', f0: 3000, q: 1 });
    note('sine', 55, 82, t0 + 0.25, 0.3, 0.2, 0.9, { sweep: 1.0 });
  },
  wardenStep(t0, { gain: g = 0.25, pan } = {}) { burst(t0, g, 0.004, 0.09, { type: 'lowpass', f0: 200, q: 0.8, pan }); burst(t0 + 0.01, g * 0.4, 0.002, 0.04, { type: 'bandpass', f0: 900, q: 1, pan }); },
  wardenReturn(t0) { note('sine', 82, 55, t0, 0.15, 0.05, 0.6, { sweep: 0.5 }); },
  wardenDeath(t0) { SOUNDS.death(t0); bell(t0 + 0.1, 165, 1.5, 0.3); },
  // Drowner: plops, surge splash + gurgling roar, reverse splash on the sink, "pulled under" on a kill
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
    setTimeout(() => { try { lp.disconnect(); g.disconnect(); } catch (e) { /* ignore */ } }, 1600);
  },
  // False light: snap-click as the glass dies then a chitter; scrabbling; the reveal shriek; a kill
  falseLightPounce(t0) { burst(t0, 0.3, 0.001, 0.012, { type: 'bandpass', f0: 2500, q: 1.5 }); chitter(t0 + 0.05); },
  scrabble(t0, { gain: g = 0.08, pan } = {}) { burst(t0, g, 0.002, 0.012, { type: 'lowpass', f0: 1200, q: 0.7, pan }); },
  falseLightReveal(t0) { note('sawtooth', 1200, 2400, t0, 0.35, 0.02, 0.38, { sweep: 0.4 }); burst(t0, 0.12, 0.02, 0.3, { type: 'highpass', f0: 2500, q: 0.7 }); },
  falseLightDeath(t0) { SOUNDS.death(t0); chitter(t0 + 0.15, { peak: 0.16 }); },
  // Brute: roar on CHASE, stride thuds, the lantern crunch, a snort at the flash, a heavier kill
  bruteRoar(t0) {
    note('sawtooth', 70, 45, t0, 0.4, 0.05, 0.95, { sweep: 1.0 });
    burst(t0, 0.45, 0.35, 0.65, { type: 'lowpass', f0: 300, f1: 1800, q: 1, sweep: 0.6 });
  },
  bruteStep(t0, { gain: g = 0.35, pan } = {}) { burst(t0, g, 0.003, 0.16, { type: 'lowpass', f0: 90, q: 1, pan }); if (g > 0.15) burst(t0 + 0.01, g * 0.25, 0.002, 0.05, { type: 'bandpass', f0: 500, q: 0.8, pan }); },
  lanternCrunch(t0) {
    burst(t0, 0.5, 0.005, 0.15, { type: 'lowpass', f0: 600, q: 0.8 });
    for (let i = 0; i < 3; i++) note('square', 180, 0, t0 + 0.02 + i * 0.06, 0.2, 0.002, 0.03);
    burst(t0 + 0.05, 0.15, 0.01, 0.35, { type: 'highpass', f0: 3000, q: 0.5 });   // glass
  },
  snort(t0) { burst(t0, 0.3, 0.01, 0.2, { type: 'bandpass', f0: 250, q: 1.2 }); },
  bruteDeath(t0) {
    const lp = filt('lowpass', 4000, 1); lp.frequency.setValueAtTime(4000, t0); lp.frequency.exponentialRampToValueAtTime(100, t0 + 1.2);
    const g = gain(1.3); lp.connect(g); g.connect(state.master);
    burst(t0, 0.6, 0.005, 0.1, { type: 'lowpass', f0: 6000, q: 0.5, dest: lp });
    note('square', 60, 0, t0, 0.5, 0.01, 1.2, { dest: lp });
    note('square', 40, 0, t0 + 0.05, 0.4, 0.01, 0.35);
    setTimeout(() => { try { lp.disconnect(); g.disconnect(); } catch (e) { /* ignore */ } }, 1600);
  },
});
const DEATH_BY = { warden: 'wardenDeath', drowner: 'drownerDeath', falseLight: 'falseLightDeath', brute: 'bruteDeath' };

/* ---------- event wiring (wraps audio.init; the base table above stays untouched) ---------- */
{
  const baseInit = audio.init;
  audio.init = function (c) {
    baseInit(c);
    const on = (name, fn) => c.events.on(name, fn);
    on('hunterState', ({ id, state: s, prev } = {}) => {
      const profile = profileOf(id);
      if (profile === 'lampwight' && s === 'DRAWN' && prev !== 'DRAWN') play('lampwightSigh');
      if (profile === 'brute' && s === 'CHASE' && prev !== 'CHASE') {
        const t = c.state.time;
        if (t - state.lastSting >= AUDIO.stingGap) { state.lastSting = t; play('bruteRoar'); }
      }
    });
    on('lampSnuffed', () => play('lampSnuff'));
    on('lanternSmashed', () => play('lanternCrunch'));
    on('wardenAlert', () => play('wardenAlert'));
    on('wardenReturn', () => play('wardenReturn'));
    on('drownerSurge', () => play('drownerSurge'));
    on('drownerSink', () => play('drownerSink'));
    on('falseLightPounce', () => play('falseLightPounce'));
    on('falseLightReveal', () => play('falseLightReveal'));
    on('flashResisted', () => play('snort'));
    on('creatureStep', ({ hunterId, profile, x, z, d } = {}) => {
      const prof = profile || profileOf(hunterId);
      const dist = Number.isFinite(d) ? d : (Number.isFinite(x) && Number.isFinite(z) ? distTo(x, z) : 0);
      const pan = Number.isFinite(x) && Number.isFinite(z) ? panAt(x, z) : 0;
      if (prof === 'brute') { const T = CTUNE.breath; if (dist <= T.stepRange) play('bruteStep', { gain: T.stepGain * falloff(dist, T.stepRange), pan }); }
      else if (prof === 'warden') {
        const T = CTUNE.grind, v = hunterId != null ? V.presence[hunterId] : null;
        if (v && v.kind === 'grind') v.extStepT = c.state.time;
        if (dist <= T.stepRange) play('wardenStep', { gain: 0.25 * falloff(dist, T.stepRange), pan });
      } else if (dist <= 18) play('step', { sprint: false, water: false });
    });
    on('death', ({ hunterId } = {}) => { const s = DEATH_BY[profileOf(hunterId)]; if (s) play(s); });
  };
}
