// hunter.js — the hunters: BFS pathing, LOS senses, FSM, meshes (DESIGN.md §6, DESIGN-v2 §2–3).
// Owns ctx.hunters. Listens: zoneEnter, zoneExit, hubEnter, lantern, lanternRemoved, gateOpened, npcCaught.
// Emits: hunterState {id, state, prev}, hunterCatch {x, z, hunterId, target}.
// Targets: the player, or the NPC follower (via ctx.npc.stimulus(): "walking, lamp off" 2.5 u, lit 12 u + LOS when
// within 3 u of the lit player); a hunter chases the nearest stimulated one. Follower catches are detected by npc.js
// (npcCaught) — the hunter then goes INVESTIGATE at its own position for catchBusyT and ignores the follower meanwhile.
// BFS runs only on repath ticks (chase: every HUNTER.repath s; wander/investigate: once per leg), never per frame.
import { HUNTER as H, HUNTER_PROFILES } from './config.js';
import * as models from './models.js';
import { T, idx, inBounds, cellType, isSolid, isBlocked, toCell, center, dist2d, los, bfsField, pathTo, nearestReachable } from './maps.js';

export { los };
let ctx = null;

export function init(c) {
  ctx = c;
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
    if (h.state !== 'STAGGERED') investigate(h, h.x, h.z);
  });
  ctx.actions.spawnHunter = spawnHunter;
}

/* ---------- mesh: models.hunter() — hunched 11-box figure with per-mesh eye materials ---------- */
function buildMesh(h) {
  const g = models.hunter({ profile: h.profile, eyeColor: h.prof.eyeColor, scaleY: h.prof.scaleY });
  g.name = `hunter:${h.id}`;
  h.eyes.push(...(g.userData.eyes || []));
  g.visible = false; ctx.scene.add(g); h.group = g;
}
function disposeMesh(h) {
  if (!h.group) return;
  models.disposeModel(h.group);   // removes from the scene, disposes the cloned materials, keeps cached geometry
  h.group = null; h.eyes.length = 0;
}

function makeHunter(id, profile) {
  const h = {
    id, profile, prof: HUNTER_PROFILES[profile] || HUNTER_PROFILES.base,
    active: false, state: 'WANDER', x: 0, z: 0, yaw: 0, path: [],
    tickT: 0, repathT: 0, noStimT: 0, idleT: 1, waitT: 0, staggerT: 0, dazeT: 0, unreachT: 0, busyT: 0,
    unreachable: false, wanderFar: false, stim: false, lastKnown: { x: 0, z: 0 }, target: 'player', targetId: null,
    group: null, eyes: [], extra: false,
  };
  buildMesh(h);
  return h;
}
// spawnHunter(cx, cz, profile) → a new active hunter record at that cell (endgame's deeper-lap reinforcements).
// It gets a matching map.hunterSpawns entry so the NaN-reset and the next spawnAll treat it like any other.
export function spawnHunter(cx, cz, profile = 'base') {
  const m = ctx.zone.map; if (!m || !inBounds(m, cx, cz) || isSolid(m, cx, cz)) return null;
  const h = makeHunter(ctx.hunters.length, HUNTER_PROFILES[profile] ? profile : 'base');
  h.extra = true;
  ctx.hunters.push(h);
  const p = center(m, cx, cz);
  m.hunterSpawns.push({ cx, cz, idx: idx(m, cx, cz), x: p.x, z: p.z, extra: true });
  reset(h, { cx, cz });
  h.idleT = 0.5;
  return h;
}

// (Re)build ctx.hunters to match the zone's H cells (one per spawn, profile from zone meta) and activate them.
export function spawnAll(zone) {
  const map = zone.map, spawns = map.hunterSpawns, profiles = (zone.meta && zone.meta.hunters) || [];
  while (ctx.hunters.length > spawns.length) disposeMesh(ctx.hunters.pop());
  spawns.forEach((s, i) => {
    const profile = profiles[i] || profiles[profiles.length - 1] || 'base';
    let h = ctx.hunters[i];
    if (!h || h.profile !== profile) { if (h) disposeMesh(h); h = makeHunter(i, profile); ctx.hunters[i] = h; }
    reset(h, s);
  });
}
function reset(h, s) {
  const p = center(ctx.zone.map, s.cx, s.cz);
  h.x = p.x; h.z = p.z; h.yaw = 0; h.path = [];
  h.state = 'WANDER'; h.active = true; h.group.visible = true;
  h.tickT = 0; h.repathT = 0; h.noStimT = 0; h.idleT = 1; h.waitT = 0; h.busyT = 0;
  h.staggerT = 0; h.dazeT = 0; h.unreachT = 0; h.unreachable = false; h.wanderFar = false; h.target = 'player'; h.targetId = null;
  h.lastKnown.x = h.x; h.lastKnown.z = h.z;
  syncMesh(h);
}
// Deactivate every hunter (kept in the list so tests can still read hunters[0]).
export function clear() {
  for (const h of ctx.hunters) { h.active = false; if (h.group) h.group.visible = false; }
}

/* ---------- pools ---------- */
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

/* ---------- FSM ---------- */
function setState(h, s) {
  const prev = h.state;
  h.state = s;
  if (prev !== s) ctx.events.emit('hunterState', { id: h.id, state: s, prev });
}
const hunterCell = (h) => toCell(ctx.zone.map, h.x, h.z);

// stimAt: does something at (x, z) with these flags register from this hunter? → distance, or -1.
function stimAt(h, x, z, { lit, sprint, water, moving, still }) {
  const m = ctx.zone.map, d = dist2d(x, z, h.x, h.z);
  if (lit && d <= H.lampR && los(m, h.x, h.z, x, z)) return d;
  if (sprint && d <= H.sprintR) return d;
  if (water && d <= H.waterR) return d;
  if (moving && d <= H.walkR) return d;
  if (still && d <= H.stillR) return d;
  return -1;
}
const followerStim = () => (ctx.npc && typeof ctx.npc.stimulus === 'function' ? ctx.npc.stimulus() : null);
// sense(h) → the nearest stimulated target {kind:'player'|'npc', id, x, z, d} or null.
function sense(h) {
  if (h.dazeT > 0) return null; // still dazzled after a flash: walks its far-wander leg blind
  const p = ctx.player;
  let best = null;
  const dp = stimAt(h, p.x, p.z, { lit: p.lampOn || p.flashT > 0, sprint: p.sprinting, water: p.inWater && p.moving, moving: p.moving, still: true });
  if (dp >= 0) best = { kind: 'player', id: null, x: p.x, z: p.z, d: dp };
  const f = h.busyT > 0 ? null : followerStim();
  if (f && !f.inPool) {   // DESIGN-v2 §3: always "walking, lamp off"; lit when close to the lit player
    const df = stimAt(h, f.x, f.z, { lit: !!f.lit, sprint: false, water: false, moving: true, still: false });
    if (df >= 0 && (!best || df < best.d)) best = { kind: 'npc', id: f.id, x: f.x, z: f.z, d: df };
  }
  return best;
}
// Where the hunter's current target is (the follower falls back to the player when it is gone).
function targetPos(h) {
  if (h.target === 'npc') { const f = followerStim(); if (f) return { x: f.x, z: f.z, inPool: !!f.inPool }; h.target = 'player'; }
  const p = ctx.player; return { x: p.x, z: p.z, inPool: !!p.inPool };
}
export function pickWander(h, far) {
  const m = ctx.zone.map, p = ctx.player;
  const hc = hunterCell(h), field = bfsField(m, hc.cx, hc.cz);
  const pc = toCell(m, p.x, p.z);
  const cands = [];
  let farthest = -1, fd = -1;
  for (let i = 0; i < field.dist.length; i++) {
    const d = field.dist[i]; if (d <= 0) continue;
    const cx = i % m.w, cz = (i / m.w) | 0, pd = Math.hypot(cx - pc.cx, cz - pc.cz);
    if (far) { if (pd >= H.farCells) cands.push(i); if (pd > fd) { fd = pd; farthest = i; } }
    else if (d <= H.wanderCells) cands.push(i);
  }
  if (!cands.length && farthest >= 0) cands.push(farthest);
  if (!cands.length) { h.idleT = 1; return; }
  h.path = pathTo(m, field, cands[(Math.random() * cands.length) | 0]);
  h.idleT = 1 + Math.random() * 2;
}
function enterChase(h) {
  setState(h, 'CHASE');
  h.noStimT = 0; h.unreachT = 0; h.unreachable = false; h.repathT = H.repath;
}
function chasePath(h) {
  const m = ctx.zone.map, p = targetPos(h);
  const hc = hunterCell(h), field = bfsField(m, hc.cx, hc.cz);
  const pc = toCell(m, p.x, p.z);
  if (inBounds(m, pc.cx, pc.cz) && field.dist[idx(m, pc.cx, pc.cz)] >= 0) {
    h.path = pathTo(m, field, idx(m, pc.cx, pc.cz)); h.unreachable = false;
  } else {
    h.unreachable = true;
    const best = nearestReachable(m, field, p.x, p.z);
    h.path = best >= 0 ? pathTo(m, field, best) : [];
  }
}
export function investigate(h, x, z) {
  const m = ctx.zone.map;
  setState(h, 'INVESTIGATE');
  h.waitT = H.waitT;
  const hc = hunterCell(h), field = bfsField(m, hc.cx, hc.cz);
  const tc = toCell(m, x, z);
  const reachable = inBounds(m, tc.cx, tc.cz) && field.dist[idx(m, tc.cx, tc.cz)] >= 0;
  const ti = reachable ? idx(m, tc.cx, tc.cz) : nearestReachable(m, field, x, z);
  h.path = ti >= 0 ? pathTo(m, field, ti) : [];
  h.wanderFar = !reachable;
}
export function stagger(h) {
  setState(h, 'STAGGERED');
  h.staggerT = H.staggerT; h.dazeT = 0; h.path = [];
}
function tick(h) {
  h.dazeT = Math.max(0, h.dazeT - H.tick);
  h.busyT = Math.max(0, h.busyT - H.tick);
  const t = sense(h), stim = !!t; h.stim = stim;
  if (t) { h.lastKnown.x = t.x; h.lastKnown.z = t.z; h.noStimT = 0; h.target = t.kind; h.targetId = t.id; }
  const s = h.state;
  if (s === 'STAGGERED') {
    h.staggerT -= H.tick;
    if (h.staggerT <= 0) { setState(h, 'WANDER'); h.dazeT = H.dazeT; pickWander(h, true); }
    return;
  }
  if (stim && s !== 'CHASE') { enterChase(h); return; }
  if (s === 'WANDER') {
    if (!h.path.length) { h.idleT -= H.tick; if (h.idleT <= 0) pickWander(h, false); }
  } else if (s === 'INVESTIGATE') {
    if (!h.path.length) {
      h.waitT -= H.tick;
      if (h.waitT <= 0) { setState(h, 'WANDER'); pickWander(h, h.wanderFar); h.wanderFar = false; }
    }
  } else if (s === 'CHASE') {
    if (!stim) {
      h.noStimT += H.tick;
      if (h.noStimT >= h.prof.loseT) { investigate(h, h.lastKnown.x, h.lastKnown.z); return; }
    }
    if (h.unreachable) {
      h.unreachT += H.tick;
      if (h.unreachT >= H.unreachT) { setState(h, 'WANDER'); pickWander(h, true); }
    } else h.unreachT = 0;
  }
}
function moveToward(h, tx, tz, speed, dt) {
  const dx = tx - h.x, dz = tz - h.z, d = Math.hypot(dx, dz);
  if (d < 1e-4) return d;
  const step = Math.min(speed * dt, d);
  h.x += dx / d * step; h.z += dz / d * step;
  h.yaw = Math.atan2(-dx, -dz);
  return d - step;
}
function updateOne(h, dt) {
  const m = ctx.zone.map, p = ctx.player;
  h.tickT += dt;
  if (h.tickT >= H.tick) { h.tickT -= H.tick; tick(h); }
  const s = h.state;
  if (s === 'CHASE') {
    h.repathT += dt;
    if (h.repathT >= H.repath) { h.repathT = 0; chasePath(h); }
  }
  if (s !== 'STAGGERED') {
    const hc = hunterCell(h);
    const speed = h.prof.speed[s] * (cellType(m, hc.cx, hc.cz) === T.WATER ? ctx.cfg.hunterWaterMul : 1);
    if (h.path.length) {
      const n = h.path[0];
      if (moveToward(h, n.x, n.z, speed, dt) < 0.1) h.path.shift();
    } else if (s === 'CHASE') {
      // path exhausted: close the last stretch directly, never into a pool
      const tp = targetPos(h), pc = toCell(m, tp.x, tp.z);
      if (!h.unreachable && !tp.inPool && !isBlocked(m, pc.cx, pc.cz)) moveToward(h, tp.x, tp.z, speed, dt);
      else h.yaw = Math.atan2(-(tp.x - h.x), -(tp.z - h.z));
    } else if (s === 'INVESTIGATE') {
      h.yaw += 0.7 * dt; // slow sweep while waiting
    }
  }
  if (!Number.isFinite(h.x) || !Number.isFinite(h.z)) reset(h, m.hunterSpawns[h.id] || m.hunterSpawns[0]);
  // catch (the player; follower catches are npc.js's — it emits npcCaught, handled in init)
  if (ctx.state.mode === 'ZONE' && s !== 'STAGGERED' && !p.inPool &&
      dist2d(h.x, h.z, p.x, p.z) <= h.prof.catchR) {
    ctx.events.emit('hunterCatch', { x: p.x, z: p.z, hunterId: h.id, target: 'player' });
  }
  syncMesh(h);
}
export function syncMesh(h) {
  if (!h.group) return;
  h.group.position.set(h.x, 0, h.z);
  h.group.rotation.y = h.yaw;
  const ei = h.prof.eye[h.state];
  for (const e of h.eyes) e.material.emissiveIntensity = ei;
}

export function update(c, dt) {
  if (c.state.mode !== 'ZONE' || c.state.paused) return;
  for (const h of c.hunters) {
    if (!h.active) continue;
    updateOne(h, dt);
    if (c.state.mode !== 'ZONE') break; // a catch this frame ended the run
  }
}
