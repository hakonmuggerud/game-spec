// hunter.js v3 — every creature in the ruin: BFS pathing, LOS senses, a shared FSM driver and a per-profile behaviour
// table (DESIGN.md §5 + §5.8, DESIGN-v2 §2–3). Owns ctx.hunters: one list, one record shape, one
// update/syncMesh/catch path; what differs per creature is `PROFILES[profile]` (senses, movement predicate, FSM states,
// flash/catch reactions, animation hooks).
// Listens: zoneEnter, zoneExit, hubEnter, lantern, lanternRemoved, gateOpened, npcCaught.
// Emits: hunterState {id, state, prev}, hunterCatch {x, z, hunterId, target} · lampSnuffed {hunterId, oil, lockout, x, z} ·
//        lanternSmashed {hunterId, x, z} · wardenAlert {hunterId, x, z} · wardenReturn {hunterId} · drownerSurge {hunterId, x, z} ·
//        drownerSink {hunterId} · falseLightPounce {hunterId, x, z} · falseLightReveal {hunterId, x, z} ·
//        flashResisted {hunterId, profile} · creatureStep {hunterId, profile, x, z, d} · toast {msg} (creature HUD overrides).
// Targets: the player, or the NPC follower (via ctx.npc.stimulus()) for profiles whose senses.follower is true; a hunter
// chases the nearest stimulated one. Follower catches are detected by npc.js (npcCaught).
// BFS runs only on repath ticks (chase-like states: every HUNTER.repath s; wander/investigate/drift: once per leg; the
// false light's rest-spot search twice per retreat), never per frame. The per-frame path allocates nothing.
import * as THREE from 'three';
import { HUNTER as H, HUNTER_PROFILES, CREATURE as CR } from './config.js';
import * as models from './models.js';
import { T, idx, inBounds, cellType, isSolid, isBlocked, toCell, center, dist2d, los, bfsField, pathTo, nearestReachable } from './maps.js';

export { los };
let ctx = null;
const DIRS4 = [[1, 0], [-1, 0], [0, 1], [0, -1]];
const TAU = Math.PI * 2;
const DEG = Math.PI / 180;
let ringGeo = null;   // the drowner's ripple ring (one shared geometry; the planted-lantern ring lives in world.js)

export function init(c) {
  ctx = c;
  buildProfiles();   // the table reads ctx.cfg (water multiplier), so it is built here, before any record exists
  const ev = ctx.events;
  ev.on('zoneEnter', () => { spawnAll(ctx.zone); recomputePools(); });
  ev.on('zoneExit', () => clear());
  ev.on('hubEnter', () => clear());
  ev.on('lantern', () => recomputePools());
  ev.on('lanternRemoved', () => recomputePools());
  ev.on('gateOpened', () => { for (const h of ctx.hunters) h.path = []; });
  // a follower was taken (npc.js): that hunter stands over the spot for catchBusyT, then wanders on
  ev.on('npcCaught', ({ hunterId }) => {
    const h = hunterId != null ? ctx.hunters[hunterId] : null;
    if (!h || !h.active) return;
    h.busyT = H.catchBusyT; h.target = 'player'; h.stim = false;
    if (h.prof.fsm.INVESTIGATE && h.state !== 'STAGGERED') investigate(h, h.x, h.z);
  });
  ctx.actions.spawnHunter = spawnHunter;
  ctx.actions.spawnCreature = spawnCreature;
  // ctx.hunter: the module surface other modules/tests may read (main mirrors it on window.__game.hunterApi)
  ctx.hunter = { spawnAll: () => spawnAll(ctx.zone), clear, stagger, investigate, recomputePools, onFlash, hint, info, spawnCreature, spawnHunter,
    profiles: PROFILES, pickWander, syncMesh };
}

/* ============================================================
   Records + meshes
   ============================================================ */
// makeHunter(id, profile) → a record with every field any profile uses (so the shape is stable for tests/audio).
function makeHunter(id, profile) {
  const prof = PROFILES[profile] || PROFILES.base;
  const h = {
    id, profile, prof, opts: {},
    active: false, state: prof.initial, x: 0, z: 0, y: 0, yaw: 0, yawVis: 0, path: [],
    tickT: 0, repathT: 0, noStimT: 0, idleT: 1, waitT: 0, staggerT: 0, dazeT: 0, unreachT: 0, busyT: 0, t: 0,
    unreachable: false, wanderFar: false, stim: false, lastKnown: { x: 0, z: 0 }, target: 'player', targetId: null,
    home: { cx: 0, cz: 0, x: 0, z: 0 }, phase: Math.random() * TAU,
    group: null, eyes: [], extra: false,
    // creature fields (unused by base/fast): see each profile's onReset
    prevState: null, snuffed: false, post: { x: 0, z: 0 }, postYaw: 0, sweep: 0, sweepDir: 1, light: null, lightTarget: null,
    body: null, homing: false, lungeX: 0, lungeZ: 0, lungeDone: false, restIdx: -1, leash: null, stepT: 0, moved: false,
    embers: null, debris: null, ring: null, ringOwn: false, wakeT: 0, emberK: 0, stepPhase: 0, posedDark: null, trapped: false,
  };
  h.blocked = (m, cx, cz) => !h.prof.canEnter(m, cx, cz, h);   // bound once: bfsField predicate (no per-repath closures)
  h.loose = (m, cx, cz) => !h.prof.canEnterLoose(m, cx, cz, h); // the same rules minus the pool one: used only to walk out of a pool
  buildMesh(h);
  return h;
}
// The factory is looked up by name at build time (models.js is written concurrently); a missing factory gives a coloured
// box with two eyes so the FSM/tests never depend on the model.
function factoryFor(name) {
  const f = models[name] || (models.MODELS && models.MODELS[name]);
  return typeof f === 'function' ? f : null;
}
function fallbackModel(h) {
  const p = h.prof, c = p.fallback || 0x333333, w = p.fallbackSize ? p.fallbackSize[0] : 0.5, ht = p.fallbackSize ? p.fallbackSize[1] : 1.8;
  const g = models.build([
    models.box(0, 0, 0, w, ht, w * 0.7, c, 'body'),
    models.emissive(models.box(-w * 0.2, ht * 0.85, -w * 0.36, 0.08, 0.05, 0.05, 0x000000, 'eyeL'), p.eyeColor, 0.4),
    models.emissive(models.box(w * 0.2, ht * 0.85, -w * 0.36, 0.08, 0.05, 0.05, 0x000000, 'eyeR'), p.eyeColor, 0.4),
  ], { jitter: 0 });
  Object.assign(g.userData, { model: h.profile, boxes: 3, height: ht, eyes: [models.named(g, 'eyeL'), models.named(g, 'eyeR')], fallback: true });
  return g;
}
function buildMesh(h) {
  const p = h.prof;
  let g = null;
  const f = factoryFor(p.model);
  if (f) { try { g = f({ profile: h.profile, eyeColor: p.eyeColor, scaleY: p.scaleY }); } catch (e) { console.warn(`hunter: models.${p.model} failed, using a box`, e); g = null; } }
  if (!g) g = fallbackModel(h);
  g.name = `hunter:${h.id}`;
  h.eyes.push(...(g.userData.eyes || []));
  g.visible = false; ctx.scene.add(g); h.group = g;
  if (p.onBuild) p.onBuild(h);
}
function disposeMesh(h) {
  if (!h.group) return;
  if (h.prof.onDispose) h.prof.onDispose(h);
  models.disposeModel(h.group);   // removes from the scene, disposes the cloned materials, keeps cached geometry
  h.group = null; h.eyes.length = 0;
}

// spawnHunter(cx, cz, profile) → a new active base/fast hunter at that cell (endgame's deeper-lap reinforcements).
// It gets a matching map.hunterSpawns entry so the next spawnAll treats it like any other H.
export function spawnHunter(cx, cz, profile = 'base') {
  if (PROFILES[profile] && PROFILES[profile].creature) return spawnCreature(profile, cx, cz);
  const m = ctx.zone.map; if (!m || !inBounds(m, cx, cz) || isSolid(m, cx, cz)) return null;
  const h = makeHunter(ctx.hunters.length, PROFILES[profile] ? profile : 'base');
  h.extra = true;
  ctx.hunters.push(h);
  const p = center(m, cx, cz);
  m.hunterSpawns.push({ cx, cz, idx: idx(m, cx, cz), x: p.x, z: p.z, extra: true });
  reset(h, { cx, cz });
  h.idleT = 0.5;
  return h;
}
// spawnCreature(profile, cx, cz, opts) → a new active creature record at that cell (debug/tests; any profile). It is
// appended to map.creatures (not hunterSpawns, which endgame counts) so a later spawnAll rebuilds it too.
export function spawnCreature(profile, cx, cz, opts = {}) {
  const m = ctx.zone.map; if (!m || !PROFILES[profile] || !inBounds(m, cx, cz) || isSolid(m, cx, cz)) return null;
  if (!PROFILES[profile].creature) return spawnHunter(cx, cz, profile);
  const h = makeHunter(ctx.hunters.length, profile);
  h.extra = true; h.opts = opts || {};
  ctx.hunters.push(h);
  const p = center(m, cx, cz);
  m.creatures.push({ kind: profile, cx, cz, idx: idx(m, cx, cz), x: p.x, z: p.z, extra: true });
  reset(h, { cx, cz });
  if (ctx.state.mode !== 'ZONE') { h.active = false; h.group.visible = false; }
  return h;
}

// (Re)build ctx.hunters to match the zone: H cells (profile from meta.hunters) then creature cells (profile = kind,
// options from meta.creatures, same row-major order) — and activate them.
export function spawnAll(zone) {
  const map = zone.map, meta = zone.meta || {}, profiles = meta.hunters || [], copts = meta.creatures || [];
  const specs = [];
  map.hunterSpawns.forEach((s, i) => specs.push({ s, profile: profiles[i] || profiles[profiles.length - 1] || 'base', opts: {} }));
  (map.creatures || []).forEach((c, i) => specs.push({ s: c, profile: PROFILES[c.kind] ? c.kind : 'base', opts: copts[i] || {} }));
  while (ctx.hunters.length > specs.length) disposeMesh(ctx.hunters.pop());
  specs.forEach((sp, i) => {
    let h = ctx.hunters[i];
    if (!h || h.profile !== sp.profile) { if (h) disposeMesh(h); h = makeHunter(i, sp.profile); ctx.hunters[i] = h; }
    h.opts = sp.opts; h.extra = !!sp.s.extra;
    reset(h, sp.s);
  });
}
function reset(h, s) {
  const m = ctx.zone.map, p = center(m, s.cx, s.cz);
  h.home.cx = s.cx; h.home.cz = s.cz; h.home.x = p.x; h.home.z = p.z;
  h.x = p.x; h.z = p.z; h.y = 0; h.yaw = 0; h.yawVis = 0; h.path = [];
  h.state = h.prof.initial; h.active = true; h.group.visible = true;
  h.tickT = 0; h.repathT = 0; h.noStimT = 0; h.idleT = 1; h.waitT = 0; h.busyT = 0; h.t = 0;
  h.staggerT = 0; h.dazeT = 0; h.unreachT = 0; h.unreachable = false; h.wanderFar = false; h.target = 'player'; h.targetId = null;
  h.lastKnown.x = h.x; h.lastKnown.z = h.z;
  h.prevState = null; h.snuffed = false; h.trapped = false; h.homing = false; h.lungeDone = false; h.restIdx = -1; h.stepT = 0; h.moved = false; h.wakeT = 0; h.emberK = 0;
  if (h.prof.onReset) h.prof.onReset(h);
  syncMesh(h);
}
// Deactivate every hunter (kept in the list so tests can still read hunters[0]).
export function clear() {
  for (const h of ctx.hunters) {
    h.active = false;
    if (h.group) h.group.visible = false;
    if (h.light) h.light.visible = false;
    // one-shot bursts live in the scene, not under the group: a smash on the last frame of a run must not follow the
    // player into the hub (they also stop stepping the moment the run ends, so they would hang there mid-flight)
    for (const b of [h.embers, h.debris]) { const g = b && (b.group || b.points); if (g) { g.visible = false; b.t = 0; } }
  }
}

/* ============================================================
   Pools
   ============================================================ */
export function recomputePools() {
  const m = ctx.zone.map; if (!m) return;
  m.pool.fill(0);
  for (const l of ctx.lanterns) {
    const c = toCell(m, l.x, l.z);
    for (let dz = -3; dz <= 3; dz++) for (let dx = -3; dx <= 3; dx++) {
      const cx = c.cx + dx, cz = c.cz + dz;
      if (!inBounds(m, cx, cz)) continue;
      const p = center(m, cx, cz);
      if (dist2d(p.x, p.z, l.x, l.z) <= ctx.cfg.poolR) m.pool[idx(m, cx, cz)] = 1;
    }
  }
}

/* ============================================================
   Shared helpers (senses, paths, wander)
   ============================================================ */
function setState(h, s) {
  const prev = h.state;
  h.state = s;
  const st = h.prof.fsm[s];
  if (prev !== s) {
    if (st && st.enter) st.enter(h, prev);
    ctx.events.emit('hunterState', { id: h.id, state: s, prev });
  }
}
const hunterCell = (h) => toCell(ctx.zone.map, h.x, h.z);
const fieldFrom = (h) => { const c = hunterCell(h); return bfsField(ctx.zone.map, c.cx, c.cz, h.blocked); };
// escapeIfTrapped(h): a creature can end up standing where its own rules forbid — a lantern planted on top of it puts a
// pool under its feet, and a strict BFS from a blocked cell has no exits at all, which would freeze it there for good.
// While that is the case it walks (at its state speed) to the nearest cell it may occupy, with pools ignored *only* for
// the way out (`canEnterLoose`); the state tick and the chase repath are skipped so nothing overwrites that path.
// One extra BFS per 0.2 s tick, and only while trapped.
function escapeIfTrapped(h) {
  const m = ctx.zone.map, c = hunterCell(h);
  if (!inBounds(m, c.cx, c.cz) || h.prof.canEnter(m, c.cx, c.cz, h)) return false;
  const field = bfsField(m, c.cx, c.cz, h.loose);
  let best = -1, bd = Infinity;
  for (let i = 0; i < field.dist.length; i++) {
    const d = field.dist[i];
    if (d <= 0 || d >= bd) continue;
    if (!h.prof.canEnter(m, i % m.w, (i / m.w) | 0, h)) continue;
    bd = d; best = i;
  }
  h.path = best >= 0 ? pathTo(m, field, best) : [];
  return h.path.length > 0;
}
// pathToPoint(h, field, x, z) → sets h.path to that cell, or to the nearest reachable one; returns whether it was reachable.
function pathToPoint(h, field, x, z) {
  const m = ctx.zone.map, tc = toCell(m, x, z);
  const reachable = inBounds(m, tc.cx, tc.cz) && field.dist[idx(m, tc.cx, tc.cz)] >= 0;
  const ti = reachable ? idx(m, tc.cx, tc.cz) : nearestReachable(m, field, x, z);
  h.path = ti >= 0 ? pathTo(m, field, ti) : [];
  return reachable;
}
const playerLit = (p) => p.lampOn || p.flashT > 0;
// stimAt: does something at (x, z) with these flags register from this hunter under ranges R? → distance or -1.
function stimAt(h, x, z, lit, sprint, water, moving, still, R) {
  const m = ctx.zone.map, d = dist2d(x, z, h.x, h.z);
  if (lit && R.lamp > 0 && d <= R.lamp && los(m, h.x, h.z, x, z)) return d;
  if (sprint && R.sprint > 0 && d <= R.sprint) return d;
  if (water && R.water > 0 && d <= R.water) return d;
  if (moving && R.walk > 0 && d <= R.walk) return d;
  if (still && R.still > 0 && d <= R.still) return d;
  return -1;
}
const followerStim = () => (ctx.npc && typeof ctx.npc.stimulus === 'function' ? ctx.npc.stimulus() : null);
// genericSense(h) → the nearest stimulated target {kind:'player'|'npc', id, x, z, d} or null (ranges from prof.senses).
function genericSense(h) {
  if (h.dazeT > 0) return null; // still dazzled after a flash: walks its far-wander leg blind
  const p = ctx.player, R = h.prof.senses;
  let best = null;
  const dp = stimAt(h, p.x, p.z, playerLit(p), p.sprinting, p.inWater && p.moving, p.moving, true, R);
  if (dp >= 0) best = { kind: 'player', id: null, x: p.x, z: p.z, d: dp };
  const f = R.follower && h.busyT <= 0 ? followerStim() : null;
  if (f && !f.inPool) {   // DESIGN-v2 §3: always "walking, lamp off"; lit when close to the lit player
    const df = stimAt(h, f.x, f.z, !!f.lit, false, false, true, false, R);
    if (df >= 0 && (!best || df < best.d)) best = { kind: 'npc', id: f.id, x: f.x, z: f.z, d: df };
  }
  return best;
}
// Where the hunter's current target is (the follower falls back to the player when it is gone).
function targetPos(h) {
  if (h.target === 'npc') { const f = followerStim(); if (f) return { x: f.x, z: f.z, inPool: !!f.inPool }; h.target = 'player'; }
  const p = ctx.player; return { x: p.x, z: p.z, inPool: !!p.inPool };
}
// pickWander(h, far, minCells): a random reachable cell — near (≤ wanderCells BFS), far (≥ farCells from the player),
// or ≥ minCells from the player (the sated Lampwight). A leashed creature (h.leash: BFS-from-home distances) only
// picks cells within prof.leash of home, and walks back toward the leash when it is outside it.
export function pickWander(h, far, minCells = 0) {
  const m = ctx.zone.map, p = ctx.player;
  const field = fieldFrom(h), pc = toCell(m, p.x, p.z);
  const leash = h.leash, leashN = h.prof.leash || 0;
  const cands = [];
  let farthest = -1, fd = -1, back = -1, bd = Infinity;
  for (let i = 0; i < field.dist.length; i++) {
    const d = field.dist[i]; if (d <= 0) continue;
    if (leash) { if (leash[i] < 0 || leash[i] > leashN) continue; if (leash[i] < bd) { bd = leash[i]; back = i; } }
    const cx = i % m.w, cz = (i / m.w) | 0, pd = Math.hypot(cx - pc.cx, cz - pc.cz);
    if (minCells > 0) { if (pd >= minCells) cands.push(i); if (pd > fd) { fd = pd; farthest = i; } }
    else if (far) { if (pd >= H.farCells) cands.push(i); if (pd > fd) { fd = pd; farthest = i; } }
    else if (d <= H.wanderCells) cands.push(i);
  }
  if (!cands.length && leash && back >= 0) cands.push(back);   // outside the leash: head for the nearest leash cell
  if (!cands.length && farthest >= 0) cands.push(farthest);
  if (!cands.length) { h.idleT = 1; return; }
  h.path = pathTo(m, field, cands[(Math.random() * cands.length) | 0]);
  h.idleT = 1 + Math.random() * 2;
}
// enterChase(h, state): the chase-like states share their bookkeeping (CHASE / DRAWN / SURGE / Warden CHASE).
function enterChase(h, state = 'CHASE') {
  setState(h, state);
  h.noStimT = 0; h.unreachT = 0; h.unreachable = false; h.repathT = H.repath;
}
// chasePath(h): BFS toward the live target (base/fast: always; creatures: only while stimulated, else lastKnown).
function chasePath(h) {
  const st = h.prof.fsm[h.state];
  let tx, tz;
  if (st && st.lastKnownWhenBlind && !h.stim) { tx = h.lastKnown.x; tz = h.lastKnown.z; }
  else { const p = targetPos(h); tx = p.x; tz = p.z; }
  h.unreachable = !pathToPoint(h, fieldFrom(h), tx, tz);
}
export function investigate(h, x, z) {
  setState(h, 'INVESTIGATE');
  h.waitT = H.waitT;
  h.wanderFar = !pathToPoint(h, fieldFrom(h), x, z);
}
// stagger(h): the flash reaction (base/fast: STAGGERED; creatures: their own onFlash rule).
export function stagger(h) {
  if (h.prof.creature) return onFlash(h);
  setState(h, 'STAGGERED');
  h.staggerT = H.staggerT; h.dazeT = 0; h.path = [];
  return 'stagger';
}
// onFlash(h): main.flash() calls this for every active hunter inside the flash cone with LOS. Returns what happened.
export function onFlash(h) {
  if (!h.active) return 'none';
  return h.prof.onFlash(h);
}

/* ============================================================
   Base FSM (base / fast; the Brute reuses it without STAGGERED)
   ============================================================ */
function wanderTick(h, t) {
  if (t) return enterChase(h);
  if (!h.path.length) { h.idleT -= H.tick; if (h.idleT <= 0) pickWander(h, false); }
}
const BASE_FSM = {
  WANDER: { move: 'path', catch: true, tick: wanderTick },
  INVESTIGATE: { move: 'path', catch: true, sweep: true, tick(h, t) {
    if (t) return enterChase(h);
    if (!h.path.length) {
      h.waitT -= H.tick;
      if (h.waitT <= 0) { setState(h, 'WANDER'); pickWander(h, h.wanderFar); h.wanderFar = false; }
    }
  } },
  CHASE: { move: 'path', repath: true, close: true, catch: true, tick(h, t) {
    if (!t) {
      h.noStimT += H.tick;
      if (h.noStimT >= h.prof.loseT) return investigate(h, h.lastKnown.x, h.lastKnown.z);
    }
    if (h.unreachable) {
      h.unreachT += H.tick;
      if (h.unreachT >= H.unreachT) { setState(h, 'WANDER'); pickWander(h, true); }
    } else h.unreachT = 0;
  } },
  STAGGERED: { move: 'still', tick(h) {
    h.staggerT -= H.tick;
    if (h.staggerT <= 0) { setState(h, 'WANDER'); h.dazeT = H.dazeT; pickWander(h, true); }
  } },
};
const baseSenses = { lamp: H.lampR, sprint: H.sprintR, walk: H.walkR, still: H.stillR, water: H.waterR, follower: true };
const notBlocked = (m, cx, cz) => !isBlocked(m, cx, cz);
// notSolid: every profile's `canEnterLoose` default — the movement rules minus the pool one (escapeIfTrapped only).
// A Warden trapped in a pool may cross its own territory line on the way out; the strict rule picks the target cell.
const notSolid = (m, cx, cz) => !isSolid(m, cx, cz);
function baseFlash(h) { if (h.state === 'STAGGERED') return 'none'; stagger(h); return 'stagger'; }

/* ============================================================
   Lampwight — the light-drinker (DESIGN.md §5.1)
   ============================================================ */
const LW = CR.lampwight;
const LAMPWIGHT_FSM = {
  DRIFT: { move: 'path', tick(h, t) {
    if (t) return enterChase(h, 'DRAWN');
    if (!h.path.length) { h.idleT -= H.tick; if (h.idleT <= 0) pickWander(h, false); }
  } },
  DRAWN: { move: 'path', repath: true, close: true, catch: true, lastKnownWhenBlind: true, tick(h, t) {
    if (!t) {
      h.noStimT += H.tick;
      if (h.noStimT >= h.prof.loseT) { setState(h, 'DRIFT'); h.idleT = 1 + Math.random(); return; }   // keeps its path to lastKnown
    }
    if (h.unreachable) { h.unreachT += H.tick; if (h.unreachT >= H.unreachT) { setState(h, 'DRIFT'); pickWander(h, true); } }
    else h.unreachT = 0;
  } },
  SNUFF: { move: 'still', ignoreStim: true, tick(h) {
    h.t -= H.tick;
    if (!h.snuffed && LW.snuffT - h.t >= LW.snuffAt - 1e-6) {
      h.snuffed = true; h.emberK = 1.5;
      ctx.events.emit('lampSnuffed', { hunterId: h.id, oil: LW.oil, lockout: LW.lockout, x: h.x, z: h.z });
    }
    if (h.t <= 0) { setState(h, 'SATED'); h.t = LW.satedT; pickWander(h, false, LW.satedCells); }
  } },
  SATED: { move: 'path', ignoreStim: true, tick(h) {
    h.t -= H.tick;
    if (!h.path.length) { h.idleT -= H.tick; if (h.idleT <= 0) pickWander(h, false, LW.satedCells); }
    if (h.t <= 0) { setState(h, 'DRIFT'); h.idleT = 1; }
  } },
  STAGGERED: { move: 'still', ignoreStim: true, tick(h) {
    h.staggerT -= H.tick;
    if (h.staggerT <= 0) { setState(h, 'DRIFT'); h.dazeT = LW.dazeT; pickWander(h, true); }
  } },
};

/* ============================================================
   Warden — the sentinel (DESIGN.md §5.2)
   ============================================================ */
const WD = CR.warden;
const FACING_YAW = { N: 0, E: -Math.PI / 2, S: Math.PI, W: Math.PI / 2 };
const angDiff = (a, b) => { let d = a - b; while (d > Math.PI) d -= TAU; while (d < -Math.PI) d += TAU; return Math.abs(d); };
const wardenTerritory = (h) => (h.opts.territory != null ? h.opts.territory : WD.territory);
const inTerritory = (h, x, z) => dist2d(x, z, h.post.x, h.post.z) <= wardenTerritory(h);
function wardenSense(h) {
  const p = ctx.player, m = ctx.zone.map, lit = playerLit(p), d = dist2d(p.x, p.z, h.x, h.z);
  const reach = h.opts.reach != null ? h.opts.reach : WD.reach, cone = (h.opts.cone != null ? h.opts.cone : WD.cone) * DEG;
  let hit = false;
  if (h.state === 'SENTRY') {
    if (lit && d <= WD.near) hit = true;   // it feels the heat
    else if (lit && d <= reach && angDiff(Math.atan2(-(p.x - h.x), -(p.z - h.z)), h.yaw) <= cone && los(m, h.x, h.z, p.x, p.z)) hit = true;
  } else {
    if (lit && inTerritory(h, p.x, p.z) && los(m, h.x, h.z, p.x, p.z)) hit = true;
    else if (p.sprinting && d <= h.prof.senses.sprint) hit = true;
  }
  return hit ? { kind: 'player', id: null, x: p.x, z: p.z, d } : null;
}
function enterReturn(h) {
  setState(h, 'RETURN');
  h.noStimT = 0; h.unreachable = false; h.unreachT = 0;
  pathToPoint(h, fieldFrom(h), h.post.x, h.post.z);
  ctx.events.emit('wardenReturn', { hunterId: h.id });
}
const WARDEN_FSM = {
  SENTRY: { move: 'still', tick(h, t) {
    if (t) { setState(h, 'ALERT'); h.t = WD.alertT; ctx.events.emit('wardenAlert', { hunterId: h.id, x: h.x, z: h.z }); ctx.events.emit('toast', { msg: 'The Warden has seen your light' }); }
  } },
  ALERT: { move: 'still', tick(h) { h.t -= H.tick; if (h.t <= 0) enterChase(h); } },
  CHASE: { move: 'path', repath: true, close: true, catch: true, lastKnownWhenBlind: true, tick(h, t) {
    const tp = targetPos(h);
    if (!inTerritory(h, tp.x, tp.z)) return enterReturn(h);
    if (!t) { h.noStimT += H.tick; if (h.noStimT >= h.prof.loseT) return enterReturn(h); }
    if (h.unreachable) { h.unreachT += H.tick; if (h.unreachT >= WD.giveUpT) return enterReturn(h); }
    else h.unreachT = 0;
  } },
  RETURN: { move: 'path', catch: true, tick(h, t) {
    if (t && inTerritory(h, t.x, t.z)) return enterChase(h);
    if (dist2d(h.x, h.z, h.post.x, h.post.z) <= WD.atPost) { h.x = h.post.x; h.z = h.post.z; h.path = []; setState(h, 'SENTRY'); return; }
    if (!h.path.length) {
      pathToPoint(h, fieldFrom(h), h.post.x, h.post.z);
      // no way home (moved outside its ground by a debug teleport): it is simply back at the post next tick
      if (!h.path.length) { h.x = h.post.x; h.z = h.post.z; }
    }
  } },
  FLINCH: { move: 'still', ignoreStim: true, tick(h) {
    h.t -= H.tick;
    if (h.t <= 0) { const s = h.prevState && h.prevState !== 'FLINCH' ? h.prevState : 'SENTRY'; setState(h, s); }
  } },
};
function wardenFlash(h) {
  if (h.state !== 'FLINCH') h.prevState = h.state;
  setState(h, 'FLINCH'); h.t = WD.flinchT;
  return 'flinch';
}
function wardenReset(h) {
  h.post.x = h.home.x; h.post.z = h.home.z;
  h.postYaw = FACING_YAW[h.opts.facing] != null ? FACING_YAW[h.opts.facing] : 0;
  h.yaw = h.postYaw; h.yawVis = h.yaw; h.sweep = 0; h.sweepDir = 1;
  if (h.light) h.light.visible = true;
}
function wardenBuild(h) {
  const L = WD.light;
  const light = new THREE.SpotLight(L.color, L.int.SENTRY, L.dist, L.angle * DEG, L.penumbra, L.decay);
  light.position.set(0, h.group.userData.spotY || 2.3, 0); light.name = 'wardenLight';
  const target = new THREE.Object3D(); target.position.set(0, 1.0, -6); target.name = 'wardenLightTarget';
  h.group.add(target); light.target = target; h.group.add(light);
  h.light = light; h.lightTarget = target;
}
function wardenAnim(h, dt) {
  const sweep = (h.opts.sweep != null ? h.opts.sweep : WD.sweep) * DEG;
  if (h.state === 'SENTRY') {
    h.sweep += h.sweepDir * WD.sweepRate * dt;
    if (h.sweep > sweep) { h.sweep = sweep; h.sweepDir = -1; } else if (h.sweep < -sweep) { h.sweep = -sweep; h.sweepDir = 1; }
    h.yaw = h.postYaw + h.sweep;
  }
  if (h.light) {
    const k = WD.light.int[h.state] != null ? WD.light.int[h.state] : 0;
    h.light.intensity = k; h.light.visible = k > 0 && h.active;
    const visor = h.group.userData.visor;
    if (visor && visor.material) visor.material.emissiveIntensity = k / 2;
  }
  const u = h.group.userData, legs = u.legs;
  if (legs && legs.length === 2) {
    const a = h.moved ? 0.3 * Math.sin(h.stepPhase += dt * 9) : 0;
    legs[0].rotation.x = a; legs[1].rotation.x = -a;
  }
  // the plinth is the post's stone, not the Warden's: it shows only while it stands on it
  if (u.plinth) u.plinth.visible = dist2d(h.x, h.z, h.post.x, h.post.z) <= WD.atPost + 0.05;
}

/* ============================================================
   Drowner — the thing under the surface (DESIGN.md §5.3)
   ============================================================ */
const DR = CR.drowner;
function floodWater(m, cx, cz) {
  const body = new Uint8Array(m.w * m.h), q = [idx(m, cx, cz)];
  if (cellType(m, cx, cz) !== T.WATER) return body;
  body[q[0]] = 1;
  while (q.length) {
    const i = q.pop(), x = i % m.w, z = (i / m.w) | 0;
    for (const [dx, dz] of DIRS4) {
      const nx = x + dx, nz = z + dz;
      if (!inBounds(m, nx, nz) || cellType(m, nx, nz) !== T.WATER) continue;
      const j = idx(m, nx, nz); if (body[j]) continue;
      body[j] = 1; q.push(j);
    }
  }
  return body;
}
// The player is in, or 4-adjacent to, a water cell of this drowner's body.
function nearBody(h, x, z) {
  const m = ctx.zone.map, c = toCell(m, x, z);
  if (!inBounds(m, c.cx, c.cz) || !h.body) return false;
  if (h.body[idx(m, c.cx, c.cz)]) return true;
  for (const [dx, dz] of DIRS4) { const nx = c.cx + dx, nz = c.cz + dz; if (inBounds(m, nx, nz) && h.body[idx(m, nx, nz)]) return true; }
  return false;
}
function drownerSense(h) {
  const p = ctx.player, m = ctx.zone.map, d = dist2d(p.x, p.z, h.x, h.z), R = h.prof.senses;
  const lit = playerLit(p) && los(m, h.x, h.z, p.x, p.z), wading = p.inWater && p.moving, sprint = p.sprinting;
  const trigger = h.dazeT <= 0 && d <= DR.trigger && nearBody(h, p.x, p.z) && (lit || sprint || wading);
  const keep = (lit && d <= R.lamp) || (wading && d <= R.water) || (sprint && d <= R.sprint);
  const home = h.dazeT <= 0 && lit && d <= DR.home;
  if (!trigger && !keep && !home) return null;
  return { kind: 'player', id: null, x: p.x, z: p.z, d, trigger, keep, home, stim: trigger || keep };
}
// a random water cell of its body ≤ driftCells BFS away
function driftPick(h) {
  const m = ctx.zone.map, field = fieldFrom(h), cands = [];
  for (let i = 0; i < field.dist.length; i++) { const d = field.dist[i]; if (d > 0 && d <= DR.driftCells) cands.push(i); }
  h.idleT = DR.driftT[0] + Math.random() * (DR.driftT[1] - DR.driftT[0]);
  if (cands.length) h.path = pathTo(m, field, cands[(Math.random() * cands.length) | 0]);
}
function sink(h) { setState(h, 'SINK'); h.t = DR.sinkT; h.path = []; ctx.events.emit('drownerSink', { hunterId: h.id }); }
const DROWNER_FSM = {
  SUBMERGED: { move: 'path', tick(h, t) {
    if (t && t.trigger) {
      setState(h, 'SURFACING'); h.t = DR.surfaceT; h.path = []; h.homing = false;
      ctx.events.emit('drownerSurge', { hunterId: h.id, x: h.x, z: h.z });
      return;
    }
    if (t && t.home) {
      h.homing = true; h.repathT += H.tick;
      if (h.repathT >= 1.0) { h.repathT = 0; pathToPoint(h, fieldFrom(h), t.x, t.z); }
      h.lastKnown.x = t.x; h.lastKnown.z = t.z;
    } else {
      h.homing = false;
      if (!h.path.length) { h.idleT -= H.tick; if (h.idleT <= 0) driftPick(h); }
    }
  }, speed: (h) => (h.homing ? DR.homeSpeed : h.prof.speed.SUBMERGED) },
  SURFACING: { move: 'still', ignoreStim: true, tick(h) { h.t -= H.tick; if (h.t <= 0) { enterChase(h, 'SURGE'); h.wakeT = 0.5; } } },
  SURGE: { move: 'path', repath: true, close: true, catch: true, lastKnownWhenBlind: true, tick(h, t) {
    if (t && t.keep) h.noStimT = 0; else h.noStimT += H.tick;
    if (h.noStimT >= h.prof.loseT) { setState(h, 'LURK'); h.t = DR.lurkT; pathToPoint(h, fieldFrom(h), h.lastKnown.x, h.lastKnown.z); }
  } },
  LURK: { move: 'path', tick(h, t) {
    if (t && t.keep) return enterChase(h, 'SURGE');
    h.t -= H.tick;
    if (h.t <= 0) return sink(h);
    if (!h.path.length) { const m = ctx.zone.map, field = fieldFrom(h), cands = []; for (let i = 0; i < field.dist.length; i++) if (field.dist[i] > 0 && field.dist[i] <= 4) cands.push(i); if (cands.length) h.path = pathTo(m, field, cands[(Math.random() * cands.length) | 0]); }
  } },
  SINK: { move: 'still', ignoreStim: true, tick(h) { h.t -= H.tick; if (h.t <= 0) { setState(h, 'SUBMERGED'); h.dazeT = DR.dazeT; h.idleT = 1; } } },
};
function drownerFlash(h) {
  if (h.state === 'SURFACING' || h.state === 'SURGE' || h.state === 'LURK') { sink(h); return 'sink'; }
  return 'none';
}
function drownerReset(h) {
  const m = ctx.zone.map;
  h.body = floodWater(m, h.home.cx, h.home.cz);
  h.y = DR.ySub; h.idleT = 1;
  if (h.ring) h.ring.visible = true;
}
function drownerBuild(h) {
  const u = h.group.userData;
  if (u.ripple) { h.ring = u.ripple; h.ringOwn = false; return; }   // the model brought its own ripple ring
  h.ringOwn = true;
  if (!ringGeo) { ringGeo = new THREE.RingGeometry(0.5, 0.65, 24); ringGeo.rotateX(-Math.PI / 2); }
  const ring = new THREE.Mesh(ringGeo, new THREE.MeshLambertMaterial({ color: 0x000000, emissive: DR.ripple.color, emissiveIntensity: DR.ripple.k, side: THREE.DoubleSide }));
  ring.name = 'ripple'; ring.position.y = 0.02;
  h.group.add(ring); h.ring = ring;
}
function drownerDispose(h) { if (h.ring && h.ringOwn) h.ring.material.dispose(); h.ring = null; }
// Every 0.2 s: a drowner outside its own water body (debug teleport, parkHunters) snaps back to its spawn.
function drownerGuard(h) {
  const m = ctx.zone.map, c = hunterCell(h);
  if (!inBounds(m, c.cx, c.cz) || !h.body[idx(m, c.cx, c.cz)]) { h.x = h.home.x; h.z = h.home.z; h.path = []; }
}
function drownerAnim(h, dt, time) {
  const s = h.state;
  const jaw = h.group.userData.jaw;   // it gapes as it comes up and while it surges (negative x = the jaw drops)
  if (jaw) jaw.rotation.x = s === 'SURGE' ? -0.45 : s === 'SURFACING' ? -0.2 : s === 'LURK' ? -0.12 : 0;
  let ty = DR.ySub;
  if (s === 'SURFACING') ty = DR.ySub + (DR.ySurf - DR.ySub) * (1 - Math.max(0, h.t) / DR.surfaceT);
  else if (s === 'SURGE' || s === 'LURK') ty = DR.ySurf;
  else if (s === 'SINK') ty = DR.ySurf + (DR.ySub - DR.ySurf) * (1 - Math.max(0, h.t) / DR.sinkT);
  h.y = ty;
  const shown = s !== 'SUBMERGED';
  for (const c of h.group.children) if (c !== h.ring) c.visible = shown;
  if (h.ring) {
    h.ring.position.y = 0.02 - h.y - 0.1;   // stays on the water surface (y −0.1) whatever the body does
    const R = DR.ripple, u = h.group.userData;
    if (h.wakeT > 0) h.wakeT -= dt;
    if (!h.ringOwn && typeof u.update === 'function') { if (u.wake) u.wake(Math.max(0, h.wakeT / 0.5)); u.update(time + h.phase); }
    else {
      let sc;
      if (s === 'SUBMERGED') sc = R.min + (R.max - R.min) * ((time + h.phase) % R.period) / R.period;
      else if (h.wakeT > 0) sc = R.max * (1 + 0.5 * (1 - h.wakeT / 0.5));
      else sc = 1.0;
      h.ring.scale.set(sc, 1, sc);
      h.ring.material.emissiveIntensity = s === 'SUBMERGED' ? R.k : R.k * 0.5;
    }
  }
}

/* ============================================================
   False light — the lantern that isn't (DESIGN.md §5.4)
   ============================================================ */
const FL = CR.falseLight;
let falseLights = 0;   // lights alive in the scene (capped at FL.maxLights per zone)
function falseLightSense(h) {
  if (h.state !== 'LIT') return null;
  const p = ctx.player, d = dist2d(p.x, p.z, h.x, h.z);
  if (d <= FL.proximity && los(ctx.zone.map, h.x, h.z, p.x, p.z)) return { kind: 'player', id: null, x: p.x, z: p.z, d };
  return null;
}
// pickRest(h): a reachable non-solid, non-pool cell 8–30 BFS from the player without LOS from them; nooks, loot and
// visibility from a corridor score higher; one of the top 6 at random. Returns a cell index or -1.
function pickRest(h) {
  const m = ctx.zone.map, p = ctx.player, pc = toCell(m, p.x, p.z);
  if (!inBounds(m, pc.cx, pc.cz)) return -1;
  const fromPlayer = bfsField(m, pc.cx, pc.cz), mine = fieldFrom(h);   // player-side distances over solid ∪ pool
  const scored = [];
  for (let i = 0; i < mine.dist.length; i++) {
    if (mine.dist[i] < 0) continue;
    const d = fromPlayer.dist[i]; if (d < FL.restMin || d > FL.restMax) continue;
    const cx = i % m.w, cz = (i / m.w) | 0, c = center(m, cx, cz);
    if (los(m, p.x, p.z, c.x, c.z)) continue;
    let score = 0;
    for (const [dx, dz] of DIRS4) if (isSolid(m, cx + dx, cz + dz)) score += 2;
    for (const it of ctx.items) { const ic = toCell(m, it.x, it.z); if (Math.abs(ic.cx - cx) <= 2 && Math.abs(ic.cz - cz) <= 2) { score += 3; break; } }
    let seen = false;
    for (let z = Math.max(0, cz - 6); z <= Math.min(m.h - 1, cz + 6) && !seen; z++) for (let x = Math.max(0, cx - 6); x <= Math.min(m.w - 1, cx + 6) && !seen; x++) {
      if (cellType(m, x, z) !== T.FLOOR || Math.hypot(x - cx, z - cz) > 6) continue;
      if (los(m, x + 0.5 + m.ox, z + 0.5, c.x, c.z)) seen = true;
    }
    if (seen) score += 1;
    scored.push({ i, score: score + Math.random() * 0.01 });
  }
  if (!scored.length) return -1;
  scored.sort((a, b) => b.score - a.score);
  return scored[(Math.random() * Math.min(FL.restTop, scored.length)) | 0].i;
}
function enterRetreat(h) {
  const m = ctx.zone.map;
  h.restIdx = pickRest(h);
  setState(h, 'RETREAT');
  if (h.restIdx >= 0) { const field = fieldFrom(h); h.path = pathTo(m, field, h.restIdx); }
  else { h.path = []; setState(h, 'RELIGHT'); h.t = FL.relightT; }
}
const FALSELIGHT_FSM = {
  LIT: { move: 'still', tick(h, t) {
    if (t) { setState(h, 'DARK'); h.t = FL.darkT; ctx.events.emit('falseLightPounce', { hunterId: h.id, x: h.x, z: h.z }); }
  } },
  DARK: { move: 'still', ignoreStim: true, tick(h) {
    h.t -= H.tick;
    if (h.t <= 0) { const p = ctx.player; h.lungeX = p.x; h.lungeZ = p.z; h.lungeDone = false; setState(h, 'POUNCE'); h.t = FL.pounceT; }
  } },
  POUNCE: { move: 'lunge', catch: true, ignoreStim: true, tick(h) { h.t -= H.tick; if (h.t <= 0 || h.lungeDone) enterRetreat(h); } },
  RETREAT: { move: 'path', ignoreStim: true, tick(h) { if (!h.path.length) { setState(h, 'RELIGHT'); h.t = FL.relightT; } } },
  RELIGHT: { move: 'still', ignoreStim: true, tick(h) { h.t -= H.tick; if (h.t <= 0) setState(h, 'LIT'); } },
  REVEALED: { move: 'still', ignoreStim: true, tick(h) { h.t -= H.tick; if (h.t <= 0) enterRetreat(h); } },
  STAGGERED: { move: 'still', ignoreStim: true, tick(h) { h.staggerT -= H.tick; if (h.staggerT <= 0) enterRetreat(h); } },
};
function falseLightFlash(h) {
  if (h.state === 'LIT') {
    setState(h, 'REVEALED'); h.t = FL.revealT;
    ctx.events.emit('falseLightReveal', { hunterId: h.id, x: h.x, z: h.z }); ctx.events.emit('toast', { msg: 'It was never a lantern' });
    return 'reveal';
  }
  if (h.state === 'DARK' || h.state === 'POUNCE') { setState(h, 'STAGGERED'); h.staggerT = FL.staggerT; h.path = []; return 'abort'; }
  return 'none';
}
function falseLightBuild(h) {
  if (falseLights >= FL.maxLights) return;   // cap: the glass still glows, only the PointLight is skipped
  const L = FL.light, light = new THREE.PointLight(L.color, L.int, L.dist, L.decay);
  light.position.y = h.group.userData.lightY || L.y; light.name = 'falseLight';
  h.group.add(light); h.light = light; falseLights++;
}
function falseLightDispose(h) { if (h.light) { h.light = null; falseLights = Math.max(0, falseLights - 1); } }
function falseLightReset(h) { if (h.light) h.light.visible = true; }
function falseLightAnim(h, dt, time) {
  const lit = h.state === 'LIT', f = 0.93 + 0.07 * Math.sin(time * 13 + h.phase), u = h.group.userData;
  // the tell (red eyes + dropped jaw) sits on its front face: face the player the moment it goes dark, or the
  // reveal/pounce wind-up reads as an empty patch of floor (models/audio note 1)
  if (h.state === 'DARK' || h.state === 'REVEALED') { const p = ctx.player; h.yaw = h.yawVis = Math.atan2(-(p.x - h.x), -(p.z - h.z)); }
  if (h.light) { h.light.intensity = lit ? FL.light.int * f : 0; h.light.visible = lit && h.active; }
  if (h.posedDark !== !lit && typeof u.setDark === 'function') { u.setDark(!lit); h.posedDark = !lit; }   // legs splay, jaw drops, glass dies
  const glass = u.glass;
  if (glass && glass.material) glass.material.emissiveIntensity = lit ? f : 0;
}

/* ============================================================
   Brute — the wall that walks (DESIGN.md §5.5)
   ============================================================ */
const BR = CR.brute;
const BRUTE_FSM = {
  WANDER: { move: 'path', tick: wanderTick },
  INVESTIGATE: BASE_FSM.INVESTIGATE,
  CHASE: { move: 'path', repath: true, close: true, catch: true, tick(h, t) {
    if (!t) { h.noStimT += H.tick; if (h.noStimT >= h.prof.loseT) return investigate(h, h.lastKnown.x, h.lastKnown.z); }
    if (h.unreachable) { h.unreachT += H.tick; if (h.unreachT >= BR.unreachT) { setState(h, 'WANDER'); pickWander(h, true); } }
    else h.unreachT = 0;
  } },
};
function bruteFlash(h) { ctx.events.emit('flashResisted', { hunterId: h.id, profile: h.profile }); ctx.events.emit('toast', { msg: 'It does not flinch' }); return 'none'; }
function bruteReset(h) {
  const m = ctx.zone.map;
  h.leash = bfsField(m, h.home.cx, h.home.cz, true).dist;   // static solids only: the leash never moves with lanterns
  h.stepT = 0;
}
// models.js one-shot particle groups (emberBurst / lanternDebris): userData.{reset(), step(dt) → alive, life, dispose?}.
// makeBurst parks one invisible in the scene; fireBurst restarts it at a point; stepBurst runs it and hides it when done.
function makeBurst(name, args) {
  const f = factoryFor(name);
  if (!f) return null;
  let g = null;
  try { g = f(args); } catch (e) { console.warn(`hunter: models.${name} failed`, e); return null; }
  const u = g && g.userData;
  if (!u || typeof u.reset !== 'function' || typeof u.step !== 'function') return null;
  g.visible = false; ctx.scene.add(g);
  return { group: g, t: 0, life: u.life || BR.embers.life, model: true };
}
function fireBurst(b, x, z) { if (!b) return; b.group.position.set(x, 0, z); b.group.userData.reset(); b.group.visible = true; b.t = b.life; }
function stepBurst(b, dt) { if (!b || b.t <= 0) return; b.t -= dt; if (!b.group.userData.step(dt)) { b.t = 0; b.group.visible = false; } }
function disposeBurst(b) { if (!b) return; const d = b.group.userData.dispose; if (typeof d === 'function') d(); else ctx.scene.remove(b.group); }
function bruteBuild(h) {
  h.debris = makeBurst('lanternDebris', {});    // wood/iron/glass shards: the lantern itself coming apart
  const burst = makeBurst('emberBurst', { count: BR.embers.n, color: BR.embers.color, life: BR.embers.life });
  if (burst) {   // parked invisible in the scene until a smash
    h.embers = burst;
    return;
  }
  const n = BR.embers.n, pos = new Float32Array(n * 3), geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  const mat = new THREE.PointsMaterial({ color: BR.embers.color, size: 0.09, transparent: true, opacity: 0, blending: THREE.AdditiveBlending, depthWrite: false });
  const points = new THREE.Points(geo, mat); points.name = 'embers'; points.visible = false; points.frustumCulled = false;
  ctx.scene.add(points);
  h.embers = { points, vel: new Float32Array(n * 3), t: 0 };
}
function bruteDispose(h) {
  disposeBurst(h.debris); h.debris = null;
  if (!h.embers) return;
  if (h.embers.model) disposeBurst(h.embers);
  else { ctx.scene.remove(h.embers.points); h.embers.points.geometry.dispose(); h.embers.points.material.dispose(); }
  h.embers = null;
}
// Smash any planted lantern within smashR: the existing world API removes it, lanternRemoved recomputes the pools.
function bruteNearLantern(h) {
  for (const l of ctx.lanterns) {
    if (dist2d(l.x, l.z, h.x, h.z) > BR.smashR) continue;
    const x = l.x, z = l.z;
    ctx.world.removeLantern(l);
    ctx.events.emit('lanternRemoved', { x, z });
    ctx.events.emit('lanternSmashed', { hunterId: h.id, x, z });
    ctx.events.emit('toast', { msg: 'It smashed your lantern' });
    fireBurst(h.debris, x, z);
    if (h.embers && h.embers.model) fireBurst(h.embers, x, z);
    else if (h.embers) {
      const e = h.embers, p = e.points.geometry.attributes.position.array;
      for (let i = 0; i < BR.embers.n; i++) {
        p[i * 3] = x + (Math.random() - 0.5) * 0.4; p[i * 3 + 1] = 0.8 + Math.random() * 0.5; p[i * 3 + 2] = z + (Math.random() - 0.5) * 0.4;
        e.vel[i * 3] = (Math.random() - 0.5) * 1.2; e.vel[i * 3 + 1] = BR.embers.rise * (0.6 + Math.random() * 0.8); e.vel[i * 3 + 2] = (Math.random() - 0.5) * 1.2;
      }
      e.t = BR.embers.life; e.points.visible = true; e.points.geometry.attributes.position.needsUpdate = true;
    }
    return;   // one smash per tick
  }
}
function bruteAnim(h, dt, time) {
  // turn-rate limit (visual): it corners badly
  let d = h.yaw - h.yawVis; while (d > Math.PI) d -= TAU; while (d < -Math.PI) d += TAU;
  const maxT = BR.turnRate * dt;
  h.yawVis += Math.max(-maxT, Math.min(maxT, d));
  const u = h.group.userData;
  if (h.moved) h.stepPhase += dt * (h.state === 'CHASE' ? 14 : 10.5);
  const sw = h.moved ? Math.sin(h.stepPhase) : 0;
  if (u.body) u.body.position.x = 0.06 * sw; else h.group.rotation.z = 0.03 * sw;
  if (u.legs && u.legs.length === 2) { u.legs[0].rotation.x = 0.35 * sw; u.legs[1].rotation.x = -0.35 * sw; }
  stepBurst(h.debris, dt);
  const e = h.embers;
  if (e && e.model) stepBurst(e, dt);
  else if (e && e.t > 0) {
    e.t -= dt;
    const p = e.points.geometry.attributes.position.array;
    for (let i = 0; i < BR.embers.n * 3; i++) p[i] += e.vel[i] * dt;
    e.points.geometry.attributes.position.needsUpdate = true;
    e.points.material.opacity = Math.max(0, e.t / BR.embers.life);
    if (e.t <= 0) e.points.visible = false;
  }
}

/* ============================================================
   The profile table (DESIGN.md §5.8)
   ============================================================ */
function profile(name, extra) {
  const cfg = HUNTER_PROFILES[name];
  return Object.assign({ name, speed: cfg.speed, eye: cfg.eye, catchR: cfg.catchR, loseT: cfg.loseT, scaleY: cfg.scaleY, eyeColor: cfg.eyeColor,
    kills: cfg.kills !== false, senses: cfg.senses || baseSenses, creature: true, model: name, initial: 'WANDER',
    canEnter: notBlocked, canEnterLoose: notSolid, poolMul: 1, waterMul: ctx.cfg.hunterWaterMul, catchInPool: false,
    sense: genericSense, onFlash: baseFlash, fsm: BASE_FSM }, extra);
}
// PROFILES[name] — filled by buildProfiles() in init (exported for tests/audio: ctx.hunter.profiles).
export const PROFILES = {};
function buildProfiles() {
  if (PROFILES.base) return;
  PROFILES.base = profile('base', { creature: false, model: 'hunter', fallback: 0x08080a });
  PROFILES.fast = profile('fast', { creature: false, model: 'hunter', fallback: 0x08080a });
  PROFILES.lampwight = profile('lampwight', { initial: 'DRIFT', fsm: LAMPWIGHT_FSM, fallback: 0x6a7488, fallbackSize: [0.4, 2.2],
    onFlash: (h) => { if (h.state === 'STAGGERED') return 'none'; setState(h, 'STAGGERED'); h.staggerT = LW.staggerT; h.dazeT = 0; h.path = []; return 'stagger'; },
    // the touch: no death — it drinks the flame (only a lit lamp can be snuffed)
    catchIf: (h, p) => p.lampOn, onCatch: (h) => { setState(h, 'SNUFF'); h.t = LW.snuffT; h.snuffed = false; h.path = []; },
    anim(h, dt, time) {
      const u = h.group.userData;
      if (typeof u.update === 'function') { u.update(time + h.phase); h.y = 0; }   // the model bobs its own body sub-group
      else h.y = 0.05 * Math.sin(time * TAU * 0.7 + h.phase);
      const ember = u.ember;
      if (h.state !== 'SNUFF' && h.emberK > 0) h.emberK = Math.max(0, h.emberK - dt * 0.3);
      if (ember && ember.material) ember.material.emissiveIntensity = h.emberK;
    } });
  PROFILES.warden = profile('warden', { initial: 'SENTRY', fsm: WARDEN_FSM, sense: wardenSense, onFlash: wardenFlash, fallback: 0x3a2f22, fallbackSize: [0.7, 2.5],
    canEnter: (m, cx, cz, h) => !isBlocked(m, cx, cz) && dist2d(m.ox + cx + 0.5, cz + 0.5, h.post.x, h.post.z) <= wardenTerritory(h),
    onReset: wardenReset, onBuild: wardenBuild, anim: wardenAnim, territory: WD.territory });
  PROFILES.drowner = profile('drowner', { initial: 'SUBMERGED', fsm: DROWNER_FSM, sense: drownerSense, onFlash: drownerFlash, waterMul: 1.0, fallback: 0x06080a, fallbackSize: [0.6, 0.55],
    canEnter: (m, cx, cz, h) => cellType(m, cx, cz) === T.WATER && !!(h.body && h.body[idx(m, cx, cz)]) && m.pool[idx(m, cx, cz)] !== 1,
    canEnterLoose: (m, cx, cz, h) => cellType(m, cx, cz) === T.WATER && !!(h.body && h.body[idx(m, cx, cz)]),   // never leaves the water

    onReset: drownerReset, onBuild: drownerBuild, onDispose: drownerDispose, onTick: drownerGuard, anim: drownerAnim });
  PROFILES.falseLight = profile('falseLight', { initial: 'LIT', fsm: FALSELIGHT_FSM, sense: falseLightSense, onFlash: falseLightFlash, fallback: 0x3a2a1a, fallbackSize: [0.3, 1.6],
    onReset: falseLightReset, onBuild: falseLightBuild, onDispose: falseLightDispose, anim: falseLightAnim });
  PROFILES.brute = profile('brute', { fsm: BRUTE_FSM, onFlash: bruteFlash, fallback: 0x141210, fallbackSize: [1.3, 2.6],
    canEnter: (m, cx, cz) => !isSolid(m, cx, cz), poolMul: BR.poolMul, catchInPool: true, leash: BR.leash, step: BR.step,
    onReset: bruteReset, onBuild: bruteBuild, onDispose: bruteDispose, onNearLantern: bruteNearLantern, anim: bruteAnim });
}

/* ============================================================
   Driver
   ============================================================ */
// stateDef(h): the current state's table row; a state the profile does not know (tests/endgame writing 'WANDER' or
// 'CHASE' onto a creature) resets it to the profile's initial state.
function stateDef(h) {
  let st = h.prof.fsm[h.state];
  if (!st) { setState(h, h.prof.initial); st = h.prof.fsm[h.state]; }
  return st;
}
function tick(h) {
  h.dazeT = Math.max(0, h.dazeT - H.tick);
  h.busyT = Math.max(0, h.busyT - H.tick);
  if (h.prof.onTick) h.prof.onTick(h);
  const st = stateDef(h);
  // only a state that walks can walk out; a still state (a Warden at its post, a LIT false light) just sits in the
  // pool — it cannot reach the player either, since its lunge/close refuse pool cells
  h.trapped = st.move === 'path' && escapeIfTrapped(h);
  if (h.trapped) return;            // walking out: the state keeps its timers, nothing else runs this tick
  const t = st.ignoreStim ? null : h.prof.sense(h);
  h.stim = !!(t && t.stim !== false);
  if (h.stim) { h.lastKnown.x = t.x; h.lastKnown.z = t.z; h.noStimT = 0; h.target = t.kind; h.targetId = t.id; }
  st.tick(h, t);
}
function moveToward(h, tx, tz, speed, dt) {
  const dx = tx - h.x, dz = tz - h.z, d = Math.hypot(dx, dz);
  if (d < 1e-4) return d;
  const step = Math.min(speed * dt, d);
  h.x += dx / d * step; h.z += dz / d * step;
  h.yaw = Math.atan2(-dx, -dz);
  h.moved = step > 0;
  return d - step;
}
function speedFor(h, st) {
  const m = ctx.zone.map, hc = hunterCell(h);
  let s = st.speed ? st.speed(h) : (h.prof.speed[h.state] || 0);
  const t = cellType(m, hc.cx, hc.cz);
  if (t === T.WATER) s *= h.prof.waterMul;
  if (h.prof.poolMul !== 1 && inBounds(m, hc.cx, hc.cz) && m.pool[idx(m, hc.cx, hc.cz)] === 1) s *= h.prof.poolMul;
  return s;
}
function updateOne(h, dt) {
  const m = ctx.zone.map, p = ctx.player;
  h.tickT += dt;
  if (h.tickT >= H.tick) { h.tickT -= H.tick; tick(h); }
  const st = stateDef(h);
  h.moved = false;
  if (st.repath && !h.trapped) {
    h.repathT += dt;
    if (h.repathT >= H.repath) { h.repathT = 0; chasePath(h); }
  }
  if (st.move === 'path') {
    const speed = speedFor(h, st);
    if (h.path.length) {
      const n = h.path[0], last = h.path.length === 1;
      if (moveToward(h, n.x, n.z, speed, dt) < (last ? 0.02 : 0.1)) h.path.shift();
    } else if (st.close && !h.trapped && (h.stim || !st.lastKnownWhenBlind)) {
      // path exhausted: close the last stretch directly — never into a pool (unless it wades in), never off its ground
      const tp = targetPos(h), pc = toCell(m, tp.x, tp.z);
      if (!h.unreachable && (!tp.inPool || h.prof.catchInPool) && h.prof.canEnter(m, pc.cx, pc.cz, h)) moveToward(h, tp.x, tp.z, speed, dt);
      else h.yaw = Math.atan2(-(tp.x - h.x), -(tp.z - h.z));
    } else if (st.sweep) {
      h.yaw += 0.7 * dt; // slow sweep while waiting
    }
  } else if (st.move === 'lunge' && !h.lungeDone) {
    // straight line at the position the lunge started with; stops at walls and pool edges
    const speed = speedFor(h, st), dx = h.lungeX - h.x, dz = h.lungeZ - h.z, d = Math.hypot(dx, dz);
    if (d < 0.05) h.lungeDone = true;
    else {
      const step = Math.min(speed * dt, d), nx = h.x + dx / d * step, nz = h.z + dz / d * step, nc = toCell(m, nx, nz);
      if (h.prof.canEnter(m, nc.cx, nc.cz, h)) { h.x = nx; h.z = nz; h.yaw = Math.atan2(-dx, -dz); h.moved = true; }
      else h.lungeDone = true;
    }
  }
  if (!Number.isFinite(h.x) || !Number.isFinite(h.z)) reset(h, h.home);
  // footsteps (creatureStep) for profiles that stride
  if (h.prof.step && h.moved) {
    h.stepT += dt;
    const iv = h.prof.step[h.state] || 0.6;
    if (h.stepT >= iv) { h.stepT -= iv; ctx.events.emit('creatureStep', { hunterId: h.id, profile: h.profile, x: h.x, z: h.z, d: dist2d(h.x, h.z, p.x, p.z) }); }
  }
  // a lantern within reach is smashed before any catch is checked: the pool buys the seconds the smash takes (§5)
  if (h.prof.onNearLantern && ctx.lanterns.length) h.prof.onNearLantern(h);
  // catch (the player; follower catches are npc.js's — it emits npcCaught, handled in init)
  if (ctx.state.mode === 'ZONE' && st.catch && (!p.inPool || h.prof.catchInPool) &&
      dist2d(h.x, h.z, p.x, p.z) <= h.prof.catchR && (!h.prof.catchIf || h.prof.catchIf(h, p))) {
    if (h.prof.kills) ctx.events.emit('hunterCatch', { x: p.x, z: p.z, hunterId: h.id, target: 'player' });
    else if (h.prof.onCatch) h.prof.onCatch(h, p);
  }
  if (h.prof.anim) h.prof.anim(h, dt, ctx.state.time);
  syncMesh(h);
}
export function syncMesh(h) {
  if (!h.group) return;
  h.group.position.set(h.x, h.y, h.z);
  h.group.rotation.y = h.prof.name === 'brute' ? h.yawVis : h.yaw;
  const ei = h.prof.eye[h.state];
  const k = ei != null ? ei : 0.3;
  for (const e of h.eyes) e.material.emissiveIntensity = k;
}

export function update(c, dt) {
  if (c.state.mode !== 'ZONE' || c.state.paused) return;
  for (const h of c.hunters) {
    if (!h.active) continue;
    updateOne(h, dt);
    if (c.state.mode !== 'ZONE') break; // a catch this frame ended the run
  }
}

/* ============================================================
   HUD + debug surfaces
   ============================================================ */
// hint() → the creature line for the HUD (DESIGN.md §5.1–5 HUD rows), '' when none applies. ui.hintText may
// append it; the 2 s overrides (alert / reveal / smash / resist) go out as `toast` events instead.
export function hint() {
  if (!ctx || ctx.state.mode !== 'ZONE') return '';
  const p = ctx.player, m = ctx.zone.map;
  if (p.lampLock > 0) return `Snuffed — [F] relight in ${Math.ceil(p.lampLock)} s`;
  let out = '';
  for (const h of ctx.hunters) {
    if (!h.active) continue;
    const d = dist2d(h.x, h.z, p.x, p.z), s = h.state;
    switch (h.profile) {
      case 'lampwight': if (s === 'DRAWN' && d <= 12 && los(m, h.x, h.z, p.x, p.z)) out = 'Something is drawn to your light'; break;
      case 'warden': if (s === 'CHASE') out = 'Leave its ground or go dark'; break;
      case 'drowner':
        if (s === 'SURGE') out = "It's in the water — get out";
        else if (s === 'SUBMERGED' && d <= 8 && nearBody(h, p.x, p.z)) out = 'The water is moving';
        break;
      case 'falseLight': if (s === 'POUNCE') out = 'It has seen you'; break;
      case 'brute': if (p.inPool && d <= 6) out = 'Not safe — it will wade in'; else if (s === 'CHASE') out = 'It has seen you'; break;
      default: break;
    }
    if (out) return out;
  }
  return out;
}
// info(h) → a plain snapshot of the fields tests care about (per-creature debug fields).
export function info(h) {
  if (!h) return null;
  const m = ctx.zone.map, c = hunterCell(h);
  const o = { id: h.id, profile: h.profile, state: h.state, active: h.active, x: +h.x.toFixed(3), z: +h.z.toFixed(3), y: +h.y.toFixed(3), cx: c.cx, cz: c.cz,
    cell: inBounds(m, c.cx, c.cz) ? m.cells[idx(m, c.cx, c.cz)] : -1, pool: inBounds(m, c.cx, c.cz) ? m.pool[idx(m, c.cx, c.cz)] : -1,
    t: +h.t.toFixed(2), noStimT: +h.noStimT.toFixed(2), dazeT: +h.dazeT.toFixed(2), staggerT: +h.staggerT.toFixed(2), stim: h.stim, target: h.target, trapped: h.trapped,
    path: h.path.length, unreachable: h.unreachable, home: { ...h.home }, model: h.group ? h.group.userData.model : null, boxes: h.group ? h.group.userData.boxes : 0,
    fallbackModel: !!(h.group && h.group.userData.fallback) };
  if (h.profile === 'warden') Object.assign(o, { post: { ...h.post }, postYaw: h.postYaw, sweep: +h.sweep.toFixed(3), yaw: +h.yaw.toFixed(3), light: h.light ? +h.light.intensity.toFixed(2) : null, prevState: h.prevState, territory: wardenTerritory(h) });
  if (h.profile === 'drowner') Object.assign(o, { bodyCells: h.body ? h.body.reduce((a, b) => a + b, 0) : 0, inBody: !!(h.body && inBounds(m, c.cx, c.cz) && h.body[idx(m, c.cx, c.cz)]), homing: h.homing, ring: h.ring ? +h.ring.scale.x.toFixed(2) : null });
  if (h.profile === 'falseLight') Object.assign(o, { light: h.light ? +h.light.intensity.toFixed(2) : null, lightVisible: !!(h.light && h.light.visible), restIdx: h.restIdx, lunge: [+h.lungeX.toFixed(2), +h.lungeZ.toFixed(2)], lungeDone: h.lungeDone });
  if (h.profile === 'lampwight') Object.assign(o, { snuffed: h.snuffed, emberK: +h.emberK.toFixed(2) });
  if (h.profile === 'brute') Object.assign(o, { leashD: h.leash && inBounds(m, c.cx, c.cz) ? h.leash[idx(m, c.cx, c.cz)] : null, yawVis: +h.yawVis.toFixed(3), embers: h.embers ? +h.embers.t.toFixed(2) : null, emberVisible: !!(h.embers && (h.embers.group || h.embers.points).visible) });
  return o;
}
