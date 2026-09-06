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
  return { g, lp, pan, target: 0, k: 0 };
}
function presenceFor(i) {
  while (V.presence.length <= i) V.presence.push(presenceVoice());
  return V.presence[i];
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
  on('hunterState', ({ state: s, prev }) => {
    if (s !== 'CHASE' || prev === 'CHASE') return;
    const t = ctx.state.time;
    if (t - state.lastSting >= AUDIO.stingGap) { state.lastSting = t; play('sting'); }
  });
  on('death', () => play('death'));
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

  // hunter presence (one voice per hunter index, created lazily)
  {
    const hs = c.hunters || [];
    const rx = Math.cos(p.yaw), rz = -Math.sin(p.yaw); // camera-right vector (forward = (-sin, -cos))
    for (let i = 0; i < Math.max(hs.length, V.presence.length); i++) {
      const h = hs[i], v = presenceFor(i);
      let k = 0, pan = 0;
      if (h && h.active && mode === 'ZONE') {
        const dx = h.x - p.x, dz = h.z - p.z, d = Math.hypot(dx, dz);
        k = clamp01(1 - d / AUDIO.presenceRange) * (TUNE.stateMul[h.state] !== undefined ? TUNE.stateMul[h.state] : 0.35);
        if (d > 0.01) pan = Math.max(-1, Math.min(1, (dx * rx + dz * rz) / d)) * 0.8;
      }
      const g = AUDIO.presenceMax * k;
      v.k = k;
      if (Math.abs(g - v.target) > 0.0005 || (g === 0 && v.target !== 0)) {
        v.target = g;
        setTarget(v.g.gain, g, t0, tau);
        setTarget(v.lp.frequency, 120 + 500 * k, t0, tau);
      }
      if (v.pan && k > 0) setTarget(v.pan.pan, pan, t0, tau);
    }
  }

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
