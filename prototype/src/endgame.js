// endgame.js — The Source and the three endings (DESIGN-v2 §5).
// Owns: the Source's descent feel (per-lap fog/ambient tint, lap toasts, a rising audio cue on top of
// audio.js's lap-scaled drone), staged hunter pressure as the player descends, "[E] Ride up" at V (no
// banking in the Source), the altar → ENDING mode (pointer released, hunters frozen because hunter.update
// only runs in ZONE), the 3-choice overlay (1/2/3 or click), the end screen (title, 4 lines, stats,
// continue), `save.endings`, the hub HUD "Endings: ✦✧✧" line, a persistent hub mark per ending seen, and
// the `night` visit (flame visually re-tiered to 1 until the player leaves the hub).
// Listens: zoneEnter, zoneExit, hubEnter, begin, key, flameTier, death.
// Emits: altar {x, z}, ending {id, choice, title, tier, rescued, first}, endingContinue {id},
//        lap {lap, prev, zoneId}, hunterWoken {id, lap}, hunterSpawned {id, x, z, profile, lap},
//        sourceAbandoned {carried}, uiClick, uiError, toast.
// Talks to other modules only through ctx + events (feature-detected where a hook may not exist yet).
import * as CONFIG from './config.js';
import { ZONES, T, lapOf, toCell, center, cellType, dist2d, bfsField, inBounds } from './maps.js';

const { CFG, TIERS, HUNTER_PROFILES, KEYS } = CONFIG;
// Numbers (config.ENDGAME when the config agent appends it; these defaults otherwise).
export const ENDGAME = Object.assign({
  altarR: 1.6,            // E range at the altar (DESIGN-v2 §5)
  rideConfirmT: 3,        // seconds the "press E again" confirmation stays armed at V while carrying loot
  dawnRescued: 3, dawnTier: 4,
  fogPerLap: 0.10,        // fog density ×(1 + fogPerLap·lap)
  ambientPerLap: 0.13,    // ambient colour ×(1 − ambientPerLap·lap), floor 0.25
  tintLerp: 2.0,          // visual lap follows the real lap with this rate (1/s)
  wakeLapAhead: 1,        // a hunter spawned at lap L wakes when the player reaches lap L − wakeLapAhead
  extraLaps: [3, 4],      // laps that each add one extra fast hunter
  maxHunters: 4,
  spawnMinCells: 12, spawnMaxCells: 34,   // BFS distance window (from the player) for extra spawns
  cueGain: 0.10, cueDur: 2.2,             // fallback lap cue (own WebAudio nodes) gain × master vol, seconds
  nightInt: TIERS[0].int, nightDist: TIERS[0].dist,
}, CONFIG.ENDGAME || {});

let ctx = null;
const NPC_NAMES = { lamplighter: 'Wick the Lamplighter', cartographer: 'Ines the Cartographer', keeper: 'Oren the Oil-press Keeper', deacon: 'Deacon Maud' };
const LAP_LINES = [
  null,
  'First lap. The stone drinks the lamplight.',
  'Second lap. Something below has noticed the light.',
  'Third lap. The walls lean in. Your lamp burns faster here.',
  'Fourth lap. The dark is thick enough to touch.',
  'The chamber. It has been waiting a long time.',
];

/* ============================================================
   Endings (DESIGN-v2 §5) — availability, titles and the four lines each, varied by tier/rescued
   ============================================================ */
const list = (names) => names.length <= 1 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
export const ENDINGS = {
  cage: {
    id: 'cage', title: 'A Brighter Cage', choice: 'Feed the great flame', key: 'Digit1', mark: '✦',
    available: () => ({ ok: true }),
    lines: ({ tier, names }) => [
      'You pour everything you carried into the bowl, and the Source takes it the way the great flame always has: greedily, and without thanks.',
      tier >= 4
        ? 'Far above, the Last Lantern blazes white. Every alcove of the vault is lit; the dark backs off to the edges of the map and waits there.'
        : 'Far above, the Last Lantern flares — brighter than it has been in years, if not as bright as it could have been. And it will need feeding again.',
      names.length
        ? `Those you brought up keep the watch with you: ${list(names)}. Lamps trimmed, doors barred, eyes on the stairs.`
        : 'No one keeps the watch with you. The vault is lit, and it is empty.',
      'The dark is held. It is not healed. That was never on offer.',
    ],
  },
  dawn: {
    id: 'dawn', title: 'The Lantern Eternal', choice: 'Kindle a new flame', key: 'Digit2', mark: '✦',
    available: ({ tier, rescued }) => {
      const need = [];
      if (tier < ENDGAME.dawnTier) need.push(`flame tier ${ENDGAME.dawnTier} (now ${tier})`);
      if (rescued < ENDGAME.dawnRescued) need.push(`${ENDGAME.dawnRescued} rescued (now ${rescued})`);
      return need.length ? { ok: false, why: `Needs ${need.join(' and ')}` } : { ok: true };
    },
    lines: ({ names }) => [
      `Deacon Maud's ritual, spoken by ${names.length} voices where one would not do: ${list(names)} carry the Source up the spiral, lap by lap, and the dark parts before it.`,
      'The old flame and the new one meet in the vault. What burns there afterward is neither — it is the Lantern Eternal, and it does not need oil.',
      'The alcoves fill with people. Then the halls below them. The Undercroft becomes a town with a very good cellar.',
      'You hang your handlamp on a hook by the door. You do not think you will need it again.',
    ],
  },
  night: {
    id: 'night', title: 'The Long Night', choice: 'Free the dark', key: 'Digit3', mark: '✦',
    available: () => ({ ok: true }),
    lines: ({ names }) => [
      'You reach into the bowl and pinch the Source out like a wick.',
      'The spiral goes black. Then the vault above it: the great flame gutters, flares once, and is gone. Far away, something enormous exhales.',
      names.length
        ? `You walk out through the long night with ${list(names)} at your side. They know the way; they always did.`
        : 'You walk out through the long night alone. There is no one left to walk it with you.',
      'The dark is free. It was never the enemy — only hungry, like everything else.',
    ],
  },
};
export const ENDING_ORDER = ['cage', 'dawn', 'night'];

/* ============================================================
   Module state
   ============================================================ */
const S = {
  screen: null,            // null | 'choice' | 'end' (while state.mode === 'ENDING')
  current: null,           // ending id shown on the end screen
  run: null,               // per Source run: {maxLap, vLap, dormant:[{h, wakeLap}], extras, spawnedLaps:Set}
  rideConfirmT: 0,
  night: null,             // snapshot of flame values while a `night` visit dims the hub flame
  marks: null,             // THREE.Group of hub marks (one per ending seen)
  hudEl: null, hudText: '',
  dom: null,
  cue: null, cues: 0,      // fallback WebAudio nodes for the lap cue, and how many cues fired (any path)
  tint: null,              // cached THREE.Color for the per-lap fog tint (no per-frame allocation)
};

const inSource = (c = ctx) => !!(c && c.zone && c.zone.meta && (c.zone.meta.noBank || c.zone.id === 'source' || c.zone.altar));
const rescuedIds = () => { const r = (ctx.save && ctx.save.rescued) || {}; return Object.keys(r).filter(k => r[k]); };
const rescuedNames = () => rescuedIds().map(id => (ctx.npc && ctx.npc.NPCS && ctx.npc.NPCS[id] && ctx.npc.NPCS[id].name) || NPC_NAMES[id] || id);
const flameTier = (c = ctx) => (c.hub && c.hub.flame ? c.hub.flame.tier | 0 : 1);
const toast = (msg) => ctx.events.emit('toast', { msg });   // ui queues it; observable by tests/audio
const carriedTotal = (c) => (c.oil | 0) + (c.relic | 0) + (c.rich | 0) + (c.quest | 0);
const emptyCarried = () => ({ oil: 0, relic: 0, rich: 0, quest: 0 });

// isSourceUnlocked(ctx): the departure-board gate — flame tier 4 and Deacon Maud rescued (ZONES.source.requires).
export function isSourceUnlocked(c = ctx) {
  const req = (ZONES.source && ZONES.source.requires) || { tier: 4, rescued: 'deacon' };
  const rescued = (c.save && c.save.rescued) || {};
  return flameTier(c) >= (req.tier || 4) && !!rescued[req.rescued || 'deacon'];
}
// sourceLockReason(ctx): null when open, else the board text.
export function sourceLockReason(c = ctx) {
  if (isSourceUnlocked(c)) return null;
  const missing = [];
  if (flameTier(c) < 4) missing.push('flame tier 4');
  if (!((c.save && c.save.rescued) || {}).deacon) missing.push('Deacon Maud');
  return `Needs ${missing.join(' and ')}`;
}
// info(): what the choice/end screens are computed from.
export function info() {
  const s = ctx.save, st = (s && s.stats) || {};
  return { tier: flameTier(), rescued: rescuedIds().length, names: rescuedNames(), total: Object.keys((s && s.rescued) || {}).length || 4,
    runs: st.runs | 0, deaths: st.deaths | 0, points: (s && s.points) | 0, seen: { ...((s && s.endings) || {}) } };
}
// available(id) → {ok, why?} for the current save.
export function available(id) { const e = ENDINGS[id]; return e ? e.available(info()) : { ok: false, why: 'Unknown ending' }; }

/* ============================================================
   Overlay DOM (created here so no other file has to change; reuses #ending if index.html has one)
   ============================================================ */
const CSS = `
#ending { cursor: default; }
#ending .box { min-width: 420px; max-width: 620px; }
#endingbody p { margin: 8px 0; }
#endingchoices button { display: block; width: 100%; box-sizing: border-box; text-align: left; margin: 6px 0; padding: 8px 12px;
  font: inherit; font-size: 13px; color: #d8cfc0; background: #151007; border: 1px solid #5a4a30; cursor: pointer; }
#endingchoices button:hover, #endingchoices button:focus { border-color: #ffb265; outline: none; }
#endingchoices button[disabled] { color: #5a5048; cursor: not-allowed; border-color: #2e2820; }
#endingchoices .ekey { color: #ffb265; margin-right: 10px; }
#endingchoices button[disabled] .ekey { color: #5a5048; }
#endingchoices .ewhy { display: block; font-size: 11px; color: #8a7a5a; margin-top: 2px; }
#endingstats { margin-top: 14px; }
#endingshud { color: #ffb265; font-size: 12px; }
`;
function buildDom() {
  if (typeof document === 'undefined') return null;
  let root = document.getElementById('ending');
  if (!root) {
    root = document.createElement('div'); root.id = 'ending'; root.className = 'screen'; root.hidden = true;
    root.innerHTML = '<div class="box"><h1 id="endingtitle"></h1><div id="endingbody"></div><div id="endingchoices"></div>' +
      '<div id="endingstats" class="small"></div><div id="endingfoot" class="cta"></div></div>';
    document.body.appendChild(root);
  }
  if (!document.getElementById('endingcss')) { const st = document.createElement('style'); st.id = 'endingcss'; st.textContent = CSS; document.head.appendChild(st); }
  const q = (id) => document.getElementById(id);
  const dom = { root, title: q('endingtitle'), body: q('endingbody'), choices: q('endingchoices'), stats: q('endingstats'), foot: q('endingfoot') };
  root.addEventListener('click', (e) => {
    if (S.screen === 'end') { continueToHub(); return; }
    if (S.screen === 'choice') {
      const b = e.target.closest && e.target.closest('button[data-ending]');
      if (b) choose(b.dataset.ending);
    }
  });
  // HUD line "Endings: ✦✧✧" under the flame/contract lines (top-right block when present)
  const host = q('tr') || q('hud');
  if (host && !q('endingshud')) { const d = document.createElement('div'); d.id = 'endingshud'; host.appendChild(d); }
  S.hudEl = q('endingshud');
  if (ctx && ctx.dom) Object.assign(ctx.dom, { ending: root, endingtitle: dom.title, endingbody: dom.body, endingchoices: dom.choices, endingstats: dom.stats, endingfoot: dom.foot, endingshud: S.hudEl });
  return dom;
}
const statsLine = (i) => `Runs ${i.runs} · Deaths ${i.deaths} · Rescued ${i.rescued}/${i.total} · Points ${i.points}`;
function renderChoice() {
  const d = S.dom; if (!d) return;
  const i = info();
  d.title.textContent = 'THE SOURCE';
  d.body.replaceChildren(...[
    'A bowl of stone, and in it a light that is not fire. It is older than the Lantern; the Lantern was lit from it.',
    'It can be fed. It can be carried. It can be put out.',
  ].map(t => { const p = document.createElement('p'); p.textContent = t; return p; }));
  d.choices.replaceChildren(...ENDING_ORDER.map((id, n) => {
    const e = ENDINGS[id], a = e.available(i);
    const b = document.createElement('button'); b.type = 'button'; b.dataset.ending = id;
    const k = document.createElement('span'); k.className = 'ekey'; k.textContent = `[${n + 1}]`;
    b.append(k, document.createTextNode(e.choice + (i.seen[id] ? '  (seen)' : '')));
    if (!a.ok) { b.disabled = true; const w = document.createElement('span'); w.className = 'ewhy'; w.textContent = a.why; b.appendChild(w); }
    return b;
  }));
  d.stats.textContent = `Flame tier ${i.tier} · Rescued ${i.rescued}/${i.total}`;
  d.foot.textContent = 'Press 1, 2 or 3 — Esc to step back';
  d.root.hidden = false;
}
function renderEnd(id) {
  const d = S.dom; if (!d) return;
  const e = ENDINGS[id], i = info();
  d.title.textContent = e.title.toUpperCase();
  d.body.replaceChildren(...e.lines(i).map(t => { const p = document.createElement('p'); p.textContent = t; return p; }));
  d.choices.replaceChildren();
  d.stats.textContent = statsLine(i);
  d.foot.textContent = 'Click to continue';
  d.root.hidden = false;
}
function hideOverlay() { if (S.dom) S.dom.root.hidden = true; }
function setHud(hidden) { if (ctx.dom && ctx.dom.hud) ctx.dom.hud.hidden = hidden; }
function releaseLock() { try { if (document.pointerLockElement && document.exitPointerLock) document.exitPointerLock(); } catch (e) { /* ignore */ } }
function requestLock() { try { const r = ctx.canvas && ctx.canvas.requestPointerLock && ctx.canvas.requestPointerLock(); if (r && r.catch) r.catch(() => {}); } catch (e) { /* ignore */ } }

/* ============================================================
   Interaction: altar (choice) and V (ride up) — main asks endgame first (DESIGN-v2 §9)
   ============================================================ */
export function interactTarget(c) {
  if (!c || c.state.mode !== 'ZONE' || !inSource(c) || !c.zone.map) return null;
  const p = c.player, m = c.zone.map;
  if (c.zone.altar) {
    const a = center(m, c.zone.altar.cx, c.zone.altar.cz);
    if (dist2d(a.x, a.z, p.x, p.z) <= ENDGAME.altarR) return { type: 'altar', label: '[E] Kneel at the Source', run: () => openChoice() };
  }
  if (m.stairs) {
    const s = center(m, m.stairs.cx, m.stairs.cz);
    if (dist2d(s.x, s.z, p.x, p.z) <= CFG.interactR) {
      const any = carriedTotal(p.carried) > 0;
      const label = !any ? '[E] Ride up — abandon the descent' : S.rideConfirmT > 0 ? '[E] Ride up now — what you carry stays below' : '[E] Ride up — abandon the descent (loot is lost)';
      return { type: 'rideUp', label, armed: S.rideConfirmT > 0, run: () => rideUp() };
    }
  }
  return null;
}
// rideUp(): leave the Source from V. No banking here: carried loot is discarded (a second press confirms when carrying).
export function rideUp() {
  if (!ctx || ctx.state.mode !== 'ZONE' || !inSource()) return false;
  const p = ctx.player;
  if (carriedTotal(p.carried) > 0 && S.rideConfirmT <= 0) {
    S.rideConfirmT = ENDGAME.rideConfirmT;
    toast('There is no banking here. Ride up now and what you carry stays below — press E again to leave.');
    ctx.events.emit('uiError', {});
    return true;
  }
  const go = () => {
    const zoneId = ctx.zone.id, carried = { ...emptyCarried(), ...p.carried };
    p.carried = emptyCarried(); S.rideConfirmT = 0;
    ctx.events.emit('sourceAbandoned', { carried, zoneId });
    toast(carriedTotal(carried) > 0 ? `You ride up out of the Source. ${ctx.actions.describe ? ctx.actions.describe(carried) : 'What you carried'} stays below.` : 'You ride up out of the Source. The altar keeps its light.');
    ctx.events.emit('zoneExit', { zoneId });
    ctx.actions.enterHub();
  };
  return ctx.actions.transition ? ctx.actions.transition(go) !== false : (go(), true);
}
// openChoice(): E at the altar → ENDING mode, pointer released, hunters frozen (hunter.update runs only in ZONE).
export function openChoice() {
  if (!ctx || ctx.state.mode !== 'ZONE') return false;
  const st = ctx.state, p = ctx.player;
  st.prevMode = 'ZONE'; st.mode = 'ENDING'; st.menuKind = 'ending';
  S.screen = 'choice'; S.current = null;
  ctx.keys.clear(); releaseLock(); setHud(true);
  renderChoice();
  ctx.events.emit('altar', { x: p.x, z: p.z, zoneId: ctx.zone.id });
  ctx.events.emit('uiClick', {});
  return true;
}
// cancel(): Esc on the choice screen — back to the run.
export function cancel() {
  if (!ctx || ctx.state.mode !== 'ENDING' || S.screen !== 'choice') return false;
  hideOverlay(); S.screen = null;
  ctx.state.mode = ctx.state.prevMode || 'ZONE'; ctx.state.prevMode = null; ctx.state.menuKind = null;
  setHud(false); requestLock();
  ctx.events.emit('uiClick', {});
  return true;
}
// choose(id): pick an ending ('cage' | 'dawn' | 'night') on the choice screen. Emits `ending`.
export function choose(id) {
  if (!ctx || ctx.state.mode !== 'ENDING' || S.screen !== 'choice') return false;
  const e = ENDINGS[id]; if (!e) return false;
  const a = e.available(info());
  if (!a.ok) { toast(`${e.choice}: ${a.why}`); ctx.events.emit('uiError', {}); return false; }
  const i = info(), first = !(ctx.save.endings && ctx.save.endings[id]);
  if (!ctx.save.endings) ctx.save.endings = { cage: false, dawn: false, night: false };
  ctx.save.endings[id] = true;
  S.screen = 'end'; S.current = id;
  renderEnd(id);
  rebuildMarks();
  ctx.events.emit('ending', { id, choice: e.choice, title: e.title, tier: i.tier, rescued: i.rescued, first });
  return true;
}
// continueToHub(): "Click to continue" → fade → hub (carried loot discarded; `night` dims the flame for the visit).
export function continueToHub() {
  if (!ctx || ctx.state.mode !== 'ENDING' || S.screen !== 'end') return false;
  const id = S.current;
  const go = () => {
    const zoneId = ctx.zone.id;
    hideOverlay(); S.screen = null; S.current = null;
    ctx.player.carried = emptyCarried();
    ctx.state.prevMode = null; ctx.state.menuKind = null;
    setHud(false);
    ctx.events.emit('zoneExit', { zoneId });
    ctx.actions.enterHub();
    if (id === 'night') beginNightVisit();
    ctx.events.emit('endingContinue', { id });
  };
  requestLock();
  if (ctx.actions.transition) { if (ctx.actions.transition(go) === false) return false; } else go();
  ctx.events.emit('uiClick', {});
  return true;
}
function onKey({ code, mode }) {
  if (mode !== 'ENDING' || !S.screen) return;
  if (S.screen === 'choice') {
    const n = (KEYS.menuPick || ['Digit1', 'Digit2', 'Digit3']).indexOf(code);
    if (n >= 0 && n < ENDING_ORDER.length) choose(ENDING_ORDER[n]);
    else if (code === KEYS.menuClose) cancel();
  } else if (S.screen === 'end') {
    if ((KEYS.confirm || []).includes(code) || code === KEYS.interact) continueToHub();
  }
}

/* ============================================================
   The descent: lap tint, lap toasts + audio cue, staged hunters
   ============================================================ */
function startRun() {
  const m = ctx.zone.map;
  S.run = { maxLap: 0, vLap: 0, dormant: [], extras: 0, spawnedLaps: new Set(), announced: new Set() };
  S.rideConfirmT = 0;
  // hunters that spawn deep start dormant and wake as the player closes in (hunter.js spawned them this event)
  for (const h of ctx.hunters) {
    if (!h.active) continue;
    const c = toCell(m, h.x, h.z), lap = lapOf(c.cx, c.cz);
    const wakeLap = lap - ENDGAME.wakeLapAhead;
    if (wakeLap > 0) { h.active = false; if (h.group) h.group.visible = false; S.run.dormant.push({ h, wakeLap }); }
  }
}
function endRun() {
  if (S.run) for (const d of S.run.dormant) { d.h.active = false; if (d.h.group) d.h.group.visible = false; }
  S.run = null; S.rideConfirmT = 0;
}
function ambientLight() { return ctx.scene.children.find(o => o.isAmbientLight) || null; }
function palette() { return (ctx.world && ctx.world.palettes && ctx.world.palettes.source) || ZONES.source.palette || { fog: { color: 0x04000c, density: 0.12 }, ambient: 0x0e0a1a }; }
function applyTint(vLap) {
  const pal = palette(), scene = ctx.scene, THREE = ctx.THREE;
  if (scene.fog) {
    if (!S.tint) S.tint = new THREE.Color(0x08001a);
    scene.fog.density = pal.fog.density * (1 + ENDGAME.fogPerLap * vLap);
    scene.fog.color.set(pal.fog.color).lerp(S.tint, Math.min(1, vLap / 5) * 0.6);
  }
  const amb = ambientLight();
  if (amb) amb.color.set(pal.ambient).multiplyScalar(Math.max(0.25, 1 - ENDGAME.ambientPerLap * vLap));
}
function lapCue(lap) {
  const a = ctx.audio; if (!a) return false;
  if (typeof a.play === 'function' && a.play('descend', { lap })) { S.cues += 1; return true; }   // audio.js may add a dedicated cue
  // fallback: a soft rising two-voice swell on the audio context (respects mute/volume)
  const ac = a.context; if (!ac || a.muted || !a.ready) return false;
  try {
    const t0 = ac.currentTime, dur = ENDGAME.cueDur, vol = (typeof a.vol === 'number' ? a.vol : 0.8) * ENDGAME.cueGain;
    const g = ac.createGain(); g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(Math.max(0.0002, vol), t0 + dur * 0.45);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
    const lp = ac.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = 320 + 60 * lap;
    g.connect(lp); lp.connect(ac.destination);
    const f0 = 55 * Math.pow(2, lap / 12);
    for (const mul of [1, 1.498]) {
      const o = ac.createOscillator(); o.type = 'sine';
      o.frequency.setValueAtTime(f0 * mul, t0); o.frequency.exponentialRampToValueAtTime(f0 * mul * 1.5, t0 + dur);
      o.connect(g); o.start(t0); o.stop(t0 + dur + 0.05);
    }
    S.cue = { g, lp, until: t0 + dur }; S.cues += 1;
    return true;
  } catch (e) { return false; }
}
function onDeeper(lap, prev) {
  const r = S.run;
  ctx.events.emit('lap', { lap, prev, zoneId: ctx.zone.id });
  if (LAP_LINES[lap] && !r.announced.has(lap)) { r.announced.add(lap); toast(LAP_LINES[lap]); }
  lapCue(lap);
  // wake dormant hunters whose lap the player is closing in on
  for (const d of r.dormant.slice()) {
    if (lap < d.wakeLap) continue;
    // a creature wakes in its own initial state (DRIFT / SENTRY / SUBMERGED / LIT), a hunter in WANDER
    d.h.active = true; if (d.h.group) d.h.group.visible = true;
    d.h.state = (d.h.prof && d.h.prof.initial) || 'WANDER'; d.h.path = []; d.h.idleT = 0.5;
    r.dormant.splice(r.dormant.indexOf(d), 1);
    ctx.events.emit('hunterWoken', { id: d.h.id, lap });
  }
  // extra hunters on the deeper laps (DESIGN-v2 §5 "hunters increase as you descend")
  for (const L of ENDGAME.extraLaps) {
    if (lap < L || r.spawnedLaps.has(L)) continue;
    r.spawnedLaps.add(L);
    // DESIGN.md §5.6: maxHunters counts the base/fast hunters only (creatures are placed, not reinforcements)
    if (ctx.hunters.filter(h => h.active && (h.profile === 'base' || h.profile === 'fast')).length >= ENDGAME.maxHunters) continue;
    const cell = pickSpawnCell(lap);
    if (cell) spawnExtraHunter(cell, 'fast', lap);
  }
}
// pickSpawnCell(lap): a reachable cell on this lap or the next, 12–34 BFS steps from the player, biased far.
function pickSpawnCell(lap) {
  const m = ctx.zone.map, p = ctx.player, pc = toCell(m, p.x, p.z);
  if (!inBounds(m, pc.cx, pc.cz)) return null;
  const f = bfsField(m, pc.cx, pc.cz, true), cands = [];
  for (let i = 0; i < f.dist.length; i++) {
    const d = f.dist[i]; if (d < ENDGAME.spawnMinCells || d > ENDGAME.spawnMaxCells) continue;
    const cx = i % m.w, cz = (i / m.w) | 0, t = m.cells[i];
    if (t !== T.FLOOR && t !== T.DEEP) continue;
    const l = lapOf(cx, cz); if (l !== lap && l !== lap + 1) continue;
    cands.push({ cx, cz, d });
  }
  if (!cands.length) return null;
  cands.sort((a, b) => b.d - a.d);
  return cands[(Math.random() * Math.min(6, cands.length)) | 0];
}
// A hunter record in the shape hunter.js owns (ctx.hunters contract); hunter.update drives it like any other.
// Uses ctx.actions.spawnHunter when the hunter agent provides one.
function spawnExtraHunter(cell, profile, lap) {
  const m = ctx.zone.map, pos = center(m, cell.cx, cell.cz);
  let h = null;
  if (typeof ctx.actions.spawnHunter === 'function') h = ctx.actions.spawnHunter(cell.cx, cell.cz, profile) || null;
  if (!h) {
    const THREE = ctx.THREE, prof = HUNTER_PROFILES[profile] || HUNTER_PROFILES.base;
    let group = null, eyes = [];
    if (ctx.models && typeof ctx.models.hunter === 'function') {
      try { group = ctx.models.hunter({ profile }); eyes = (group.userData && group.userData.eyes) || []; } catch (e) { group = null; }
    }
    if (!group) {
      group = new THREE.Group();
      const body = new THREE.Mesh(new THREE.BoxGeometry(0.6, 1.8, 0.6), new THREE.MeshLambertMaterial({ color: 0x050505 }));
      body.position.y = 0.9; group.add(body);
      for (const sx of [-0.12, 0.12]) {
        const e = new THREE.Mesh(new THREE.SphereGeometry(0.06, 8, 6), new THREE.MeshLambertMaterial({ color: 0x000000, emissive: prof.eyeColor, emissiveIntensity: 0.4 }));
        e.position.set(sx, 1.5, -0.31); group.add(e); eyes.push(e);
      }
      group.scale.y = prof.scaleY;
    }
    group.name = 'hunter:extra';
    ctx.scene.add(group);
    h = {
      id: ctx.hunters.length, profile, prof, active: true, state: 'WANDER', x: pos.x, z: pos.z, yaw: 0, path: [],
      tickT: 0, repathT: 0, noStimT: 0, idleT: 0.5, waitT: 0, staggerT: 0, dazeT: 0, unreachT: 0, busyT: 0,
      unreachable: false, wanderFar: false, stim: false, lastKnown: { x: pos.x, z: pos.z }, target: 'player',
      group, eyes, extra: true,
    };
    group.position.set(pos.x, 0, pos.z); group.visible = true;
    ctx.hunters.push(h);
    m.hunterSpawns.push({ cx: cell.cx, cz: cell.cz, idx: cell.cz * m.w + cell.cx, x: pos.x, z: pos.z, extra: true });
  }
  S.run.extras += 1;
  ctx.events.emit('hunterSpawned', { id: h.id, x: h.x, z: h.z, profile, lap });
  return h;
}

/* ============================================================
   Hub: marks per ending seen, HUD line, the `night` visit
   ============================================================ */
function freeOffset(m, cx, cz) {
  for (const [dx, dz] of [[0, -2], [0, 2], [2, 0], [-2, 0], [0, -3], [0, 3]]) {
    const t = cellType(m, cx + dx, cz + dz);
    if (t === T.FLOOR || t === T.DEEP) return { dx, dz };
  }
  return { dx: 0, dz: 2 };
}
export function rebuildMarks() {
  if (!ctx || !ctx.hub || !ctx.hub.map || !ctx.hub.map.flame) return;
  const THREE = ctx.THREE, m = ctx.hub.map, f = center(m, m.flame.cx, m.flame.cz), seen = (ctx.save && ctx.save.endings) || {};
  if (S.marks) {
    ctx.scene.remove(S.marks);
    S.marks.traverse(o => { if (o.isMesh) { o.geometry.dispose(); o.material.dispose(); } });
  }
  const g = new THREE.Group(); g.name = 'endgame:marks'; g.position.set(f.x, 0, f.z);
  const mesh = (w, h, d, color, emissive, k, x, y, z) => {
    const mm = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), new THREE.MeshLambertMaterial({ color, emissive: emissive || 0x000000, emissiveIntensity: k || 0 }));
    mm.position.set(x, y + h / 2, z); g.add(mm); return mm;
  };
  if (seen.cage) {
    // four brass watch-lanterns around the brazier (on floor cells: the v2 flame stands against the north wall)
    const spots = [];
    for (const [x, z] of [[2.1, 2.1], [-2.1, 2.1], [2.1, -2.1], [-2.1, -2.1], [3.4, 0.8], [-3.4, 0.8], [1.4, 3.4], [-1.4, 3.4]]) {
      const c = toCell(m, f.x + x, f.z + z), t = cellType(m, c.cx, c.cz);
      if ((t === T.FLOOR || t === T.DEEP) && spots.length < 4) spots.push([x, z]);
    }
    for (const [x, z] of spots) {
      mesh(0.08, 1.3, 0.08, 0x6a5a30, 0, 0, x, 0, z);
      mesh(0.22, 0.26, 0.22, 0x302010, 0xffc070, 0.8, x, 1.3, z);
      mesh(0.3, 0.05, 0.3, 0x3a2a1a, 0, 0, x, 1.56, z);
    }
  }
  if (seen.dawn) {
    // a pale beacon column beside the brazier: the flame that does not need oil
    const o = freeOffset(m, m.flame.cx, m.flame.cz);
    mesh(0.7, 0.35, 0.7, 0x8a8a9a, 0, 0, o.dx, 0, o.dz);
    mesh(0.3, 2.4, 0.3, 0xd0d0e0, 0x8090ff, 0.35, o.dx, 0.35, o.dz);
    mesh(0.4, 0.4, 0.4, 0xffffff, 0xe0f0ff, 1.0, o.dx, 2.75, o.dz);
    const light = new THREE.PointLight(0xc0d0ff, 1.4, 8, 2); light.position.set(o.dx, 2.9, o.dz); g.add(light);
  }
  if (seen.night) {
    // a ring of dead candle stubs and ash around the brazier; one ember left
    for (let i = 0; i < 6; i++) {
      const a = i / 6 * Math.PI * 2 + 0.3, x = Math.cos(a) * 1.6, z = Math.sin(a) * 1.6;
      mesh(0.12, 0.18 + 0.06 * (i % 3), 0.12, 0x2a2a30, 0, 0, x, 0, z);
      mesh(0.05, 0.04, 0.05, 0x111111, i === 0 ? 0x802010 : 0x000000, i === 0 ? 0.6 : 0, x, 0.18 + 0.06 * (i % 3), z);
    }
    for (let i = 0; i < 5; i++) { const a = i * 1.7, r = 1.0 + 0.3 * (i % 2); mesh(0.3, 0.04, 0.22, 0x141418, 0, 0, Math.cos(a) * r, 0, Math.sin(a) * r); }
  }
  ctx.scene.add(g); S.marks = g;
  return g;
}
function hudLine() {
  const seen = (ctx.save && ctx.save.endings) || {};
  const any = ENDING_ORDER.some(id => seen[id]);
  if (!any || ctx.state.mode !== 'HUB') return '';
  return `Endings: ${ENDING_ORDER.map(id => seen[id] ? '✦' : '✧').join('')}`;
}
function updateHud() {
  if (!S.hudEl) return;
  const t = hudLine();
  if (t !== S.hudText) { S.hudText = t; S.hudEl.textContent = t; }
}
// `night`: for this hub visit the great flame reads as tier 1 (values restored when the player leaves or the tier changes)
function beginNightVisit() {
  const fl = ctx.hub && ctx.hub.flame; if (!fl || !fl.light) return;
  const THREE = ctx.THREE;
  S.night = { baseInt: fl.baseInt, dist: fl.light.distance, tier: fl.tier,
    boxes: (fl.boxes || []).map(b => b.material.emissive.clone()),
    base: fl.base ? fl.base.material.emissiveIntensity : 0, rim: fl.rim ? fl.rim.material.emissiveIntensity : 0 };
  fl.baseInt = ENDGAME.nightInt; fl.light.distance = ENDGAME.nightDist;
  const c = new THREE.Color(0xff6020);
  (fl.boxes || []).forEach((b, i) => b.material.emissive.copy(c).lerp(new THREE.Color(0xfff0c0), i * 0.22).multiplyScalar(0.7 + 0.1 * i));
  if (fl.base) fl.base.material.emissiveIntensity = 0.08;
  if (fl.rim) fl.rim.material.emissiveIntensity = 0.35;
  if (ctx.world && typeof ctx.world.setAlcoveTier === 'function') ctx.world.setAlcoveTier(1);
  toast('The Lantern gutters. Only embers remain.');
}
function endNightVisit() {
  const n = S.night; if (!n) return;
  S.night = null;
  const fl = ctx.hub && ctx.hub.flame; if (!fl || !fl.light) return;
  fl.baseInt = n.baseInt; fl.light.distance = n.dist;
  (fl.boxes || []).forEach((b, i) => { if (n.boxes[i]) b.material.emissive.copy(n.boxes[i]); });
  if (fl.base) fl.base.material.emissiveIntensity = n.base;
  if (fl.rim) fl.rim.material.emissiveIntensity = n.rim;
  if (ctx.world && typeof ctx.world.setAlcoveTier === 'function') ctx.world.setAlcoveTier(fl.tier);
}

/* ============================================================
   init / update
   ============================================================ */
export function init(c) {
  ctx = c;
  S.dom = buildDom();
  const ev = ctx.events;
  ev.on('zoneEnter', () => { if (inSource()) startRun(); else endRun(); endNightVisit(); });
  ev.on('zoneExit', () => endRun());
  ev.on('hubEnter', () => { if (S.screen) { hideOverlay(); S.screen = null; } });
  ev.on('begin', () => { endNightVisit(); rebuildMarks(); });
  ev.on('death', () => { S.rideConfirmT = 0; });
  ev.on('key', onKey);
  // a real tier change (or a save reset, which re-emits the initial tier) ends the night look and refreshes the marks
  ev.on('flameTier', ({ initial }) => { if (S.night && !initial) endNightVisit(); if (initial) { S.night = null; rebuildMarks(); } });
  rebuildMarks();
  ctx.endgame = { ENDINGS, ENDING_ORDER, ENDGAME, isSourceUnlocked, sourceLockReason, available, info, rideUp, openChoice, cancel, choose, continueToHub,
    rebuildMarks, get screen() { return S.screen; }, get current() { return S.current; }, get run() { return S.run; }, get night() { return !!S.night; }, get cues() { return S.cues; } };
  if (typeof window !== 'undefined') setTimeout(() => { if (window.__game && !window.__game.endgame) window.__game.endgame = ctx.endgame; }, 0);
}
export function update(c, dt) {
  if (!ctx) return;
  const st = c.state, mode = st.mode;
  S.rideConfirmT = Math.max(0, S.rideConfirmT - dt);
  updateHud();
  // the descent: track the lap, ease the tint after it
  if (S.run && (mode === 'ZONE' || mode === 'ENDING' || mode === 'MENU' || mode === 'DYING') && inSource(c)) {
    const lap = mode === 'ZONE' && !st.paused ? (c.player.lap | 0) : S.run.maxLap;
    if (mode === 'ZONE' && !st.paused && lap > S.run.maxLap) { const prev = S.run.maxLap; S.run.maxLap = lap; onDeeper(lap, prev); }
    const target = mode === 'ZONE' ? (c.player.lap | 0) : S.run.maxLap;
    S.run.vLap += (target - S.run.vLap) * Math.min(1, dt * ENDGAME.tintLerp);
    applyTint(S.run.vLap);
  }
  // `night` visit: hub.update sizes the fire from flame.tier every frame, so the tier-1 scale is re-applied here (endgame runs after hub)
  if (S.night && (mode === 'HUB' || mode === 'MENU' || mode === 'TITLE')) {
    const fl = c.hub.flame, s = 0.6;
    if (fl && fl.fire) fl.fire.scale.set(s, s * (0.95 + 0.08 * Math.sin(st.time * 9.7)), s);
  }
}
