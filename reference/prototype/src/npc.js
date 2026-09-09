// npc.js — captive NPCs, follow AI, hub placement and dialogue (DESIGN-v2 §3).
// Owns ctx.npcs (the NPC records currently present: zone captives/follower, or hub residents) and
// ctx.player.follower (the record of the NPC following the player, or null).
// Listens: zoneEnter, zoneExit, hubEnter, bank, hunterCatch (target 'npc'), death, gateOpened, shortcutOpened, key, menuClose, flameTier.
// Emits: npcFreed {id, x, z}, npcCaught + npcLost {id, x, z, hunterId}, npcRescued {id, zoneId}, npcTalk {id}, uiClick, uiError.
// Only config.js / maps.js / models.js may be imported; everything else is reached through ctx (feature-detected).
// Debug surface: ctx.npc (mirrored onto window.__game.npc) = {free, talk, follower, stimulus, caught, atHub, captives, get, NPCS, NPC_CFG}.
import { CFG } from './config.js';
import { idx, inBounds, isSolid, toCell, center, dist2d, bfsField, pathTo, los } from './maps.js';

let ctx = null;

// Tuning (DESIGN-v2 §3/§10). Kept here rather than config.js so the numbers travel with the module.
export const NPC_CFG = {
  tick: 0.25,          // follower repath period (s)
  speed: 3.5,          // follower walk speed (u/s)
  fastSpeed: 5.0,      // follower speed when > fastDist from the player
  fastDist: 6,
  stopDist: 1.5,       // follower stops this close to the player
  teleportDist: 14,    // stuck safety: snap to the player's cell beyond this
  saveR: 4,            // follower must be within this of the player when banking → rescued
  litR: 3,             // follower counts as lit within this of a lit player
  catchR: 0.8,         // hunter touch radius on a follower
  caughtT: 1.0,        // sink-into-the-dark time before returning to the cell
  hunterBusyT: 2,      // h.busyT set on the hunter that caught a follower (hunter.js may honour it)
  hubLookR: 5,         // hub residents turn toward the player within this
  interactR: CFG.interactR,
  radius: 0.25,        // follower body radius for wall sliding
};

// `cell` is only the LAST-RESORT fallback: zoneCellFor() prefers the zone file's `META.npcs` (DESIGN.md §3.7), which
// is what every zone supplies. Kept in step with the authored 62/64-cell maps so the fallback can never place a
// captive inside a wall.
export const NPCS = {
  lamplighter: { name: 'Wick the Lamplighter', short: 'Wick', pronoun: 'him', zone: 'undercroft', cell: [4, 47], unlocks: 'workshop', anchor: '1',
    coat: 0xb8862a, hat: 0x2a2a2a, line: 'Every lamp I ever lit is out. Let\'s fix that.' },
  cartographer: { name: 'Ines the Cartographer', short: 'Ines', pronoun: 'her', zone: 'cistern', cell: [60, 52], unlocks: 'cart', anchor: '3',
    coat: 0x6a8ab0, hat: 0x3a5a7a, line: 'I mapped every one of these halls. Then they moved.' },
  keeper: { name: 'Oren the Oil-press Keeper', short: 'Oren', pronoun: 'him', zone: 'ossuary', cell: [5, 7], unlocks: 'press', anchor: '2',
    coat: 0xc8b070, hat: 0x7a3a2a, line: 'Relics burn better than they pray.' },
  deacon: { name: 'Deacon Maud', short: 'Maud', pronoun: 'her', zone: 'undercroft', cell: [4, 5], unlocks: 'shrine', anchor: '4',
    coat: 0xe0d8c0, hat: 0x4a3a6a, line: 'The Source can be fed, or freed. Both are prayers.' },
};
// Hub stand spots when the hub map has no building anchors (v1 17×9 hub): cells around the flame.
const HUB_FALLBACK = { lamplighter: [6, 2], keeper: [10, 2], cartographer: [6, 4], deacon: [10, 4] };

/* ============================================================
   Records + meshes
   ============================================================ */
const recs = new Map();
let pendingOffer = null;   // contract id offered in the open dialogue (accepted with 1/Enter)
const busyUntil = new Map(); // hunter id → state.time until which that hunter ignores the follower (after a catch)
let lostNote = { text: '', until: 0 };   // HUD line after a catch ("Wick was taken")
let hudEl = null;          // #npcline, created under the HUD's top-right block when it exists

function record(id) {
  let r = recs.get(id);
  if (r) return r;
  const def = NPCS[id];
  r = { id, name: def.name, short: def.short, state: 'IDLE', where: null, x: 0, z: 0, y: 0, yaw: 0, cell: null,
    path: [], pathT: 0, sinkT: 0, lit: false, moving: false, inPool: false, hunterId: null, phase: Math.random() * 6.28, group: null };
  recs.set(id, r);
  return r;
}
function ensureMesh(r) {
  if (r.group) return r.group;
  const THREE = ctx.THREE, def = NPCS[r.id];
  let g = null;
  try { if (ctx.models && typeof ctx.models.npc === 'function') g = ctx.models.npc(r.id); } catch (e) { console.warn('[npc] models.npc failed, using box', e); g = null; }
  if (!g) {
    g = new THREE.Group();
    const body = new THREE.Mesh(new THREE.BoxGeometry(0.44, 0.9, 0.3), new THREE.MeshLambertMaterial({ color: def.coat }));
    body.position.y = 0.45; g.add(body);
    const head = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.3, 0.3), new THREE.MeshLambertMaterial({ color: 0xd9b08c }));
    head.position.y = 1.05; g.add(head);
    const hat = new THREE.Mesh(new THREE.BoxGeometry(0.34, 0.12, 0.34), new THREE.MeshLambertMaterial({ color: def.hat }));
    hat.position.y = 1.26; g.add(hat);
  }
  g.name = `npc:${r.id}`;
  g.visible = false;
  ctx.scene.add(g);
  r.group = g;
  return g;
}
function show(r, on) { ensureMesh(r).visible = !!on; }
function sync(r) {
  const g = ensureMesh(r);
  g.position.set(r.x, r.y, r.z);
  g.rotation.y = r.yaw;
}
function hideAll() {
  for (const r of recs.values()) { show(r, false); r.where = null; r.state = 'IDLE'; r.path = []; r.sinkT = 0; r.y = 0; }
  ctx.npcs.length = 0;
  ctx.player.follower = null;
}
const faceToward = (r, tx, tz) => Math.atan2(-(tx - r.x), -(tz - r.z));
function turnToward(r, target, rate, dt) {
  let d = target - r.yaw;
  while (d > Math.PI) d -= 2 * Math.PI;
  while (d < -Math.PI) d += 2 * Math.PI;
  const step = rate * dt;
  r.yaw += Math.abs(d) <= step ? d : Math.sign(d) * step;
}
const toast = (msg) => { if (ctx.ui && typeof ctx.ui.toast === 'function') ctx.ui.toast(msg); else ctx.events.emit('toast', { msg }); };
const exitName = () => (ctx.zone.map && ctx.zone.map.stairs && ctx.zone.map.stairs.kind === 'elevator' ? 'cage' : 'stairs');

/* ============================================================
   Zone: captives + follower
   ============================================================ */
// Cell of an NPC in the current zone: zone meta first, then the NPCS table, checked against the map's N cells.
function zoneCellFor(id) {
  const z = ctx.zone, meta = z.meta || {}, m = z.map;
  const c = (meta.npcs && meta.npcs[id]) || NPCS[id].cell;
  const n = m && m.npcCells && m.npcCells.find(k => k.cx === c[0] && k.cz === c[1]);
  if (!n && m && m.npcCells && m.npcCells.length) console.warn(`[npc] ${id}: no N cell at (${c[0]},${c[1]}) in ${z.id}`);
  return { cx: c[0], cz: c[1] };
}
function placeCaptive(r) {
  const m = ctx.zone.map, p = center(m, r.cell.cx, r.cell.cz);
  r.x = p.x; r.z = p.z; r.y = 0; r.yaw = 0; r.state = 'CAPTIVE'; r.where = 'zone';
  r.path = []; r.pathT = 0; r.sinkT = 0; r.lit = false; r.moving = false; r.hunterId = null;
  show(r, true); sync(r);
}
function spawnZone() {
  hideAll();
  const z = ctx.zone; if (!z.map) return;
  const ids = z.meta && z.meta.npcs ? Object.keys(z.meta.npcs) : Object.keys(NPCS).filter(id => NPCS[id].zone === z.id);
  for (const id of ids) {
    if (!NPCS[id] || (ctx.save.rescued && ctx.save.rescued[id])) continue;   // rescued NPCs no longer spawn in zones
    const r = record(id);
    r.cell = zoneCellFor(id);
    placeCaptive(r);
    ctx.npcs.push(r);
  }
}

// free(id): CAPTIVE → FOLLOW (E on a captive, or the test hook actions.freeNpc). One follower at a time.
export function free(id) {
  const r = recs.get(id);
  if (!r || r.where !== 'zone' || r.state !== 'CAPTIVE' || ctx.state.mode !== 'ZONE') return false;
  const f = ctx.player.follower;
  if (f && f !== r && f.state === 'FOLLOW') { toast('You cannot shepherd two'); ctx.events.emit('uiError', {}); return false; }
  r.state = 'FOLLOW'; r.path = []; r.pathT = 0; r.hunterId = null;
  ctx.player.follower = r;
  ctx.events.emit('npcFreed', { id, x: r.x, z: r.z });
  toast(`${r.short} follows you. Bring ${NPCS[id].pronoun} to the ${exitName()}.`);
  return true;
}
function rescue(r, zoneId) {
  ctx.save.rescued[r.id] = true;
  ctx.save.stats.rescues = (ctx.save.stats.rescues | 0) + 1;
  r.state = 'RESCUED';
  ctx.player.follower = null;
  ctx.events.emit('npcRescued', { id: r.id, zoneId });
  toast(`${r.name} is safe at the Lantern.`);
}
// A hunter touched the follower: it sinks into the dark for caughtT, then stands in its cell again (never lost).
function catchFollower(r, h) {
  if (!r || r.state !== 'FOLLOW') return false;
  r.state = 'CAUGHT'; r.sinkT = NPC_CFG.caughtT; r.path = []; r.moving = false; r.lit = false;
  r.hunterId = h ? h.id : null;
  ctx.player.follower = null;
  if (h) {
    // hunter.js owns h.busyT (DESIGN-v2 §3: INVESTIGATE(own pos) 2 s); we keep our own timer so the follower
    // is not re-caught the instant it re-appears even if that field is never decremented.
    busyUntil.set(h.id, ctx.state.time + NPC_CFG.hunterBusyT);
    h.busyT = Math.max(+h.busyT || 0, NPC_CFG.hunterBusyT); h.path = [];
  }
  const payload = { id: r.id, x: r.x, z: r.z, hunterId: r.hunterId };
  ctx.events.emit('npcCaught', payload);
  ctx.events.emit('npcLost', payload);
  lostNote = { text: `${r.short} was taken — back in ${NPCS[r.id].pronoun === 'her' ? 'her' : 'his'} cell`, until: ctx.state.time + 4 };
  toast(`${r.short} was dragged back into the dark.`);
  return true;
}

// follower body: axis-separated wall sliding on the grid (pools never block followers)
function slide(r, m, dx, dz) {
  const rr = NPC_CFG.radius;
  const blocked = (x, z) => {
    for (const [ox, oz] of [[-rr, -rr], [rr, -rr], [-rr, rr], [rr, rr]]) { const c = toCell(m, x + ox, z + oz); if (isSolid(m, c.cx, c.cz)) return true; }
    return false;
  };
  if (dx) { const nx = r.x + dx; if (!blocked(nx, r.z)) r.x = nx; }
  if (dz) { const nz = r.z + dz; if (!blocked(r.x, nz)) r.z = nz; }
}
function stepToward(r, m, tx, tz, speed, dt) {
  const dx = tx - r.x, dz = tz - r.z, d = Math.hypot(dx, dz);
  if (d < 1e-4) return 0;
  const step = Math.min(speed * dt, d);
  slide(r, m, dx / d * step, dz / d * step);
  r.yaw = Math.atan2(-dx, -dz);
  return d - step;
}
function repath(r, m, p) {
  const fc = toCell(m, r.x, r.z), pc = toCell(m, p.x, p.z);
  r.path = [];
  if (!inBounds(m, fc.cx, fc.cz) || !inBounds(m, pc.cx, pc.cz)) return;
  const bfs = (ctx.maps && ctx.maps.bfsField) || bfsField, toPath = (ctx.maps && ctx.maps.pathTo) || pathTo;
  const field = bfs(m, fc.cx, fc.cz, true);          // pools are not blocked for followers
  const ti = idx(m, pc.cx, pc.cz);
  if (field.dist[ti] >= 0) r.path = toPath(m, field, ti);
}
// A free cell next to the player's, preferring the one behind them, else the player's own cell.
function snapCell(m, p) {
  const pc = toCell(m, p.x, p.z);
  const bx = Math.round(Math.sin(p.yaw)), bz = Math.round(Math.cos(p.yaw));   // behind = -(forward)
  const cands = [[bx, bz], [1, 0], [-1, 0], [0, 1], [0, -1]];
  for (const [dx, dz] of cands) {
    if (!dx && !dz) continue;
    const cx = pc.cx + dx, cz = pc.cz + dz;
    if (inBounds(m, cx, cz) && !isSolid(m, cx, cz)) return center(m, cx, cz);
  }
  return center(m, pc.cx, pc.cz);
}
function updateFollower(r, dt) {
  const p = ctx.player, m = ctx.zone.map;
  const d = dist2d(r.x, r.z, p.x, p.z);
  r.lit = p.lampOn && d <= NPC_CFG.litR;
  r.inPool = ctx.lanterns.some(l => dist2d(l.x, l.z, r.x, r.z) <= CFG.poolR);
  if (d > NPC_CFG.teleportDist) {                     // stuck safety: snap next to the player (behind, if free)
    const c = snapCell(m, p);
    r.x = c.x; r.z = c.z; r.path = []; r.pathT = 0; r.moving = false;
    return;
  }
  if (d <= NPC_CFG.stopDist) { r.moving = false; r.path = []; turnToward(r, faceToward(r, p.x, p.z), 4, dt); return; }
  r.pathT -= dt;
  if (r.pathT <= 0) { r.pathT = NPC_CFG.tick; repath(r, m, p); }
  const speed = d > NPC_CFG.fastDist ? NPC_CFG.fastSpeed : NPC_CFG.speed;
  r.moving = true;
  if (d <= 4 && los(m, r.x, r.z, p.x, p.z)) { stepToward(r, m, p.x, p.z, speed, dt); return; }   // close and visible: straight
  if (r.path.length) {
    const n = r.path[0];
    if (stepToward(r, m, n.x, n.z, speed, dt) < 0.15) r.path.shift();
  } else stepToward(r, m, p.x, p.z, speed, dt);       // no path (unreachable / helper missing): straight with wall sliding
}
function checkCatch(r) {
  if (r.state !== 'FOLLOW' || r.inPool) return;
  const t = ctx.state.time;
  for (const h of ctx.hunters) {
    if (!h.active || h.state === 'STAGGERED' || (busyUntil.get(h.id) || 0) > t) continue;
    if (dist2d(h.x, h.z, r.x, r.z) <= NPC_CFG.catchR) { catchFollower(r, h); return; }
  }
}
function updateZone(dt) {
  const p = ctx.player, t = ctx.state.time;
  for (const r of ctx.npcs) {
    if (r.where !== 'zone') continue;
    if (r.state === 'CAPTIVE') {
      const d = dist2d(r.x, r.z, p.x, p.z);
      if (d <= 8) turnToward(r, faceToward(r, p.x, p.z), 2.5, dt);
      r.y = 0.015 * Math.sin(t * 2 + r.phase);
    } else if (r.state === 'FOLLOW') {
      updateFollower(r, dt);
      r.y = r.moving ? 0.04 * Math.abs(Math.sin(t * 9 + r.phase)) : 0.015 * Math.sin(t * 2 + r.phase);
      checkCatch(r);
    } else if (r.state === 'CAUGHT') {
      r.sinkT -= dt;
      r.y = -1.6 * (1 - Math.max(0, r.sinkT) / NPC_CFG.caughtT);
      if (r.sinkT <= 0) placeCaptive(r);
    }
    if (!Number.isFinite(r.x) || !Number.isFinite(r.z)) placeCaptive(r);
    sync(r);
  }
}

// HUD: one line under the contracts block — "◆ Wick follows" / "◆ Wick was taken — back in his cell".
function ensureHud() {
  if (hudEl || typeof document === 'undefined') return hudEl;
  const host = (ctx.dom && (ctx.dom.tr || ctx.dom.contracts && ctx.dom.contracts.parentElement)) || document.getElementById('tr');
  if (!host) return null;
  hudEl = document.createElement('div'); hudEl.id = 'npcline';
  hudEl.style.cssText = 'color:#d8b070;font-size:12px;white-space:pre-line';
  host.appendChild(hudEl);
  if (ctx.dom) ctx.dom.npcline = hudEl;
  return hudEl;
}
let hudText = '';
function updateHud() {
  const el = ensureHud(); if (!el) return;
  const f = ctx.player.follower, t = ctx.state.time;
  let s = '';
  if (ctx.state.mode === 'ZONE' || ctx.state.mode === 'MENU') {
    if (f && f.state === 'FOLLOW') s = `◆ ${f.short} follows${f.inPool ? ' (safe)' : f.lit ? ' (lit)' : ''}`;
    else if (lostNote.until > t) s = `◆ ${lostNote.text}`;
  }
  if (s !== hudText) { hudText = s; el.textContent = s; }
}

/* ============================================================
   Hub: rescued NPCs at their building spot
   ============================================================ */
function hubSpot(id) {
  const m = ctx.hub.map, def = NPCS[id];
  const a = m && m.anchors && m.anchors[def.anchor];
  if (a) {
    const cx = inBounds(m, a.cx + 1, a.cz) && !isSolid(m, a.cx + 1, a.cz) ? a.cx + 1 : a.cx;   // anchor +1 x (DESIGN-v2 §7)
    return center(m, cx, a.cz);
  }
  const f = HUB_FALLBACK[id] || [m.flame.cx, m.flame.cz + 2];
  return center(m, f[0], f[1]);
}
function placeHub() {
  hideAll();
  const m = ctx.hub.map; if (!m) return;
  const rescued = ctx.save.rescued || {};
  const fl = m.flame ? center(m, m.flame.cx, m.flame.cz) : null;
  for (const id of Object.keys(NPCS)) {
    if (!rescued[id]) continue;
    const r = record(id), s = hubSpot(id);
    r.x = s.x; r.z = s.z; r.y = 0; r.state = 'HUB'; r.where = 'hub'; r.path = []; r.moving = false; r.lit = false;
    r.yaw = fl ? faceToward(r, fl.x, fl.z) : 0; r.homeYaw = r.yaw;
    show(r, true); sync(r);
    ctx.npcs.push(r);
  }
}
function updateHub(dt) {
  const p = ctx.player, t = ctx.state.time;
  for (const r of ctx.npcs) {
    if (r.where !== 'hub') continue;
    const d = dist2d(r.x, r.z, p.x, p.z);
    turnToward(r, d <= NPC_CFG.hubLookR ? faceToward(r, p.x, p.z) : r.homeYaw, 2.5, dt);
    r.y = 0.02 * Math.sin(t * 1.8 + r.phase);
    sync(r);
  }
}
// talk(id): one-line dialogue (+ contract offer when contracts.available is reachable through ctx).
export function talk(id) {
  const r = recs.get(id), def = NPCS[id];
  if (!r || r.where !== 'hub') return false;
  const lines = [`"${def.line}"`];
  pendingOffer = null;
  const con = ctx.contracts;
  const offer = con && typeof con.available === 'function' ? con.available(id) : null;
  if (offer) {
    const oid = typeof offer === 'string' ? offer : offer.id;
    const title = (typeof offer === 'object' && (offer.title || offer.name)) || oid;
    pendingOffer = oid;
    lines.push('');
    if (typeof offer === 'object') {   // the contract offer: flavour, objective, reward (DESIGN-v2 §3/§4)
      if (offer.text) lines.push(`  "${offer.text}"`);
      if (offer.objective) lines.push(`  ${offer.objective}`);
      if (offer.rewardText) lines.push(`  Reward: ${offer.rewardText}`);
      lines.push('');
    }
    lines.push(`[1] Accept contract — ${title}`);
  } else {
    const A = ctx.contracts, act = A && typeof A.active === 'function' ? A.active().filter(cid => A.get && A.get(cid) && A.get(cid).poster === id) : [];
    if (act.length) lines.push('', `  "Come back when it's done." — ${act.map(cid => A.get(cid).title).join(', ')}`);
  }
  ctx.events.emit('npcTalk', { id });
  ctx.events.emit('uiClick', {});
  const opts = { title: def.name, lines, foot: pendingOffer ? '1 / Enter to accept · Esc to close' : 'Esc to close', npc: id, line: def.line, offer: pendingOffer };
  if (ctx.actions && typeof ctx.actions.openMenu === 'function') {
    const ok = ctx.actions.openMenu('dialog', opts) !== false;
    // ui.showDialog may lay the dialogue out differently from the generic menu; redraw with it when it is its own function
    if (ok && ctx.ui && typeof ctx.ui.showDialog === 'function' && ctx.ui.showDialog !== ctx.ui.showMenu) { try { ctx.ui.showDialog(opts); } catch (e) { /* keep the generic menu */ } }
    return ok;
  }
  toast(`${def.short}: ${def.line}`);
  return true;
}
function onKey({ code, mode }) {
  if (mode !== 'MENU' || ctx.state.menuKind !== 'dialog' || !pendingOffer) return;
  if (code !== 'Digit1' && code !== 'Enter' && code !== 'Space') return;
  const id = pendingOffer; pendingOffer = null;
  const ok = ctx.actions && typeof ctx.actions.accept === 'function' ? ctx.actions.accept(id) : false;
  if (ctx.actions && typeof ctx.actions.closeMenu === 'function') ctx.actions.closeMenu();
  if (ok === false) { toast('You already carry enough contracts.'); ctx.events.emit('uiError', {}); }
}

/* ============================================================
   Module API
   ============================================================ */
export function init(c) {
  ctx = c;
  if (!Array.isArray(ctx.npcs)) ctx.npcs = [];
  const ev = ctx.events;
  ev.on('zoneEnter', () => { busyUntil.clear(); lostNote.until = 0; spawnZone(); });
  ev.on('zoneExit', () => hideAll());
  ev.on('hubEnter', () => placeHub());
  ev.on('bank', ({ zoneId }) => {
    const f = ctx.player.follower;
    if (f && f.state === 'FOLLOW' && dist2d(f.x, f.z, ctx.player.x, ctx.player.z) <= NPC_CFG.saveR) rescue(f, zoneId);
  });
  ev.on('hunterCatch', ({ target, hunterId, npcId }) => {
    if (target !== 'npc' && target !== 'follower') return;
    const f = (npcId && recs.get(npcId)) || ctx.player.follower;
    catchFollower(f, ctx.hunters[hunterId] || null);
  });
  ev.on('death', () => { const f = ctx.player.follower; if (f) { f.moving = false; f.path = []; } });
  // a rescue by any path (bank, or the debug action): drop the zone record, and re-place the hub residents when there
  ev.on('npcRescued', ({ id }) => {
    const r = recs.get(id);
    if (r && r.where === 'zone' && r.state !== 'RESCUED') {
      r.state = 'RESCUED'; show(r, false);
      const i = ctx.npcs.indexOf(r); if (i >= 0) ctx.npcs.splice(i, 1);
      if (ctx.player.follower === r) ctx.player.follower = null;
    }
    if ((ctx.state.mode === 'HUB' || ctx.state.mode === 'TITLE' || (ctx.state.mode === 'MENU' && ctx.state.prevMode === 'HUB')) && ctx.hub.map) placeHub();
  });
  ev.on('gateOpened', () => { const f = ctx.player.follower; if (f) { f.path = []; f.pathT = 0; } });
  ev.on('shortcutOpened', () => { const f = ctx.player.follower; if (f) { f.path = []; f.pathT = 0; } });
  ev.on('key', onKey);
  ev.on('menuClose', () => { pendingOffer = null; });
  // save reset re-emits the initial flameTier: re-place the hub residents from the (now empty) rescued set
  ev.on('flameTier', ({ initial }) => { if (initial && ctx.state.mode !== 'ZONE' && ctx.state.mode !== 'DYING' && ctx.hub.map) placeHub(); });
  if (ctx.hub.map) placeHub();   // the title screen looks over the hub
  // debug/integration surface (mirrored onto window.__game.npc once main has created it)
  ctx.npc = { free, talk, follower, stimulus, caught, atHub, captives, get, NPCS, NPC_CFG };
  if (typeof window !== 'undefined') setTimeout(() => { if (window.__game && !window.__game.npc) window.__game.npc = ctx.npc; }, 0);
}
export function update(c, dt) {
  const mode = c.state.mode;
  updateHud();
  if (c.state.paused) return;
  if (mode === 'ZONE') updateZone(dt);
  else if (mode === 'HUB' || mode === 'TITLE') updateHub(dt);
}
// follower(): the NPC currently following the player in the zone, or null.
export function follower() { const f = ctx ? ctx.player.follower : null; return f && f.state === 'FOLLOW' ? f : null; }
// stimulus(): what hunters may sense of the follower — {id, x, z, moving, lit, inPool} or null (DESIGN-v2 §3:
// always "walking, lamp off" (2.5 u); `lit` = within litR of a lit player (12 u + LOS)).
export function stimulus() { const f = follower(); return f ? { id: f.id, x: f.x, z: f.z, moving: f.moving, lit: f.lit, inPool: f.inPool } : null; }
// caught(id?, hunter?): external hook (hunter.js) — a hunter touched the follower.
export function caught(id, h) { return catchFollower(id ? recs.get(id) : ctx.player.follower, h || null); }
// atHub(): [{id, x, z}] of rescued NPCs standing in the hub.
export function atHub() { return ctx ? ctx.npcs.filter(r => r.where === 'hub').map(r => ({ id: r.id, x: r.x, z: r.z })) : []; }
// captives(): ids of NPCs standing captive in the current zone.
export function captives() { return ctx ? ctx.npcs.filter(r => r.where === 'zone' && r.state === 'CAPTIVE').map(r => r.id) : []; }
export function get(id) { return recs.get(id) || null; }
// interactTarget(ctx): {type:'npc', id, label, run()} when an NPC is within reach, else null.
export function interactTarget(c) {
  const st = c.state, p = c.player;
  if (st.mode !== 'ZONE' && st.mode !== 'HUB') return null;
  let best = null, bd = NPC_CFG.interactR;
  for (const r of c.npcs) {
    if (!((st.mode === 'ZONE' && r.where === 'zone' && r.state === 'CAPTIVE') || (st.mode === 'HUB' && r.where === 'hub'))) continue;
    const d = dist2d(r.x, r.z, p.x, p.z);
    if (d <= bd) { best = r; bd = d; }
  }
  if (!best) return null;
  if (st.mode === 'ZONE') return { type: 'npc', id: best.id, label: `[E] Free ${best.short}`, run: () => free(best.id) };
  return { type: 'npc', id: best.id, label: `[E] Talk to ${best.short}`, run: () => talk(best.id) };
}
