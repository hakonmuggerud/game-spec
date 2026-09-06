// hub.js — the Last Lantern: great flame + tiers (v1), buildings/ghosts, services, departure board, menus,
// minimap + explored bitsets, shrine blessing (DESIGN-v2 §7).
// Owns ctx.hub.{flame, buildings (= save.buildings), anchors, meshes, api}. Only config.js / maps.js / models are
// imported; everything else is reached through ctx (feature-detected).
// Listens: hubEnter, bank, npcRescued, flameTier, zoneEnter, zoneExit, death, pickup, key, menuClose, toolGained,
//          contractAccepted, contractComplete.
// Emits: flameTier {tier, prev, initial}, build {id, cost}, lightTech {tier, prev}, zoneSelected {zoneId},
//        service {id, action, ...}, blessing {on}, blessingKept {kept, pts, zoneId}, minimap {on}, uiClick, uiError, toast.
import * as THREE from 'three';
import { CFG, TIERS, LIGHT_TECH, BUILD_COSTS, KEYS, POINTS, EMBERS, HUB_BLOCK } from './config.js';
import { ZONES, ZONE_ORDER, zoneLocked, center, toCell, inBounds, idx, isSolid, dist2d, los, T } from './maps.js';
import * as models from './models.js';

let ctx = null;

/* ============================================================
   Tables
   ============================================================ */
export const HUB_CFG = {
  interactR: 1.8,        // anchor ±1 cell (DESIGN-v2 §7)
  ghostColor: 0x3a3020,
  minimapPx: 5, exploreTick: 0.25, exploreMaxR: 16, exploreDarkR: 2, minimapHz: 8, exploredSaveT: 1.0,
  padGlow: 0.22,         // emissive floor pad under a built building (no PointLight: forward Lambert cost per light)
};
// Buildings (DESIGN.md §8 table). `anchor` = digit in the hub rows (the E-interaction point, ±HUB_CFG.interactR);
// `model` = models.js factory; `rot`/`dx`/`dz` place the model for the v2 map so it stands against its wall and
// leaves anchor +1 x (where the rescued NPC stands) and the cells west of the anchor free.
export const BUILDINGS = {
  board:    { name: 'Departure Board', anchor: '5', model: 'board', always: true, rot: Math.PI / 2, dx: 0.3, dz: -0.35,
    desc: 'Where the next descent goes: the stairs, the tram and the elevator all follow it.' },
  workshop: { name: 'Workshop', anchor: '1', npc: 'lamplighter', model: 'workshop', rot: Math.PI, dz: 0.12,
    desc: 'Light-tech: a brighter, thriftier handlamp — reach up, burn down, cheaper flashes and lanterns.' },
  press:    { name: 'Oil Press', anchor: '2', npc: 'keeper', model: 'oilPress', rot: Math.PI, dz: 0.1,
    desc: 'Presses relics into lamp oil, and deepens the reservoir your lamp starts each run with.' },
  cart:     { name: "Cartographer's Table", anchor: '3', npc: 'cartographer', model: 'cartTable', rot: 0, dz: -0.25,
    desc: 'A map of what you have seen below: explored halls, items, contract spots, the way out. Tab shows it.' },
  shrine:   { name: 'Shrine', anchor: '4', npc: 'deacon', model: 'shrine', rot: 0,
    desc: 'A blessing bought with oil: when the dark takes you, half of what you carry finds its own way up.' },
  tram:     { name: 'Tram dock', anchor: '6', tier: 2, model: 'tram', rot: Math.PI / 2,
    desc: 'Rails into the west wall. Opens The Cistern.' },
  elevator: { name: 'Elevator', anchor: '7', tier: 3, model: 'elevator', rot: 0, dx: 0.5, dz: -0.5,
    desc: 'A cage that drops past the Cistern. Opens The Ossuary, and the way to The Source.' },
};
export const BUILD_ORDER = ['board', 'workshop', 'press', 'cart', 'shrine', 'tram', 'elevator'];
// Anchors for the v1 17×9 hub (no digits in its rows): alcoves W/E, board by the stairs. Written onto
// ctx.hub.map.anchors so npc.js places the residents beside their building (anchor +1 x).
const V1_ANCHORS = {
  '1': { cx: 2, cz: 1, rot: Math.PI },  '2': { cx: 13, cz: 1, rot: Math.PI },
  '3': { cx: 2, cz: 5, rot: 0, dz: -0.25 }, '4': { cx: 13, cz: 5, rot: 0 },
  '5': { cx: 10, cz: 6, rot: Math.PI / 4, dx: -0.3 },   // front north-west: lit by the flame, still visible from the stairs
  '6': { cx: 2, cz: 3, rot: Math.PI / 2 }, '7': { cx: 14, cz: 3, rot: 0 },
};
const ROMAN = ['none', 'I', 'II', 'III', 'IV'];
const NPC_NAME = { lamplighter: 'Wick the Lamplighter', cartographer: 'Ines the Cartographer', keeper: 'Oren the Oil-press Keeper', deacon: 'Deacon Maud' };
const ZONE_KEY = { cistern: 'tram', ossuary: 'elevator', source: 'elevator' };

/* ============================================================
   Module state
   ============================================================ */
const meshes = {};            // id → {state:'none'|'ghost'|'built', group, pad}
let padGeo = null;
let ghostMat = null;
let menu = null;              // open hub menu: {kind, id, pick(n), confirm()}
let hudRes = null;            // #hubres element (created here)
const explored = {};          // zoneId → Uint8Array bitset (mirrored to save.explored as base64)
const walkable = {};          // zoneId → number of non-wall cells
const parsed = {};            // zoneId → parsed map (for zones that are not loaded)
let exploreT = 0, exploredDirty = false, exploredSaveT = 0 /* wall-clock s of first dirty */, minimapT = 0;
const run = { blessed: false };
const cache = { res: '', lamp: null };
const embers = { points: null, pos: null, life: null, vel: null, alive: 0 };   // great-flame ember cloud (one Points)
let colliderT = 0;

const emit = (n, p) => ctx.events.emit(n, p || {});
const toast = (msg) => emit('toast', { msg });
const err = (msg) => { if (msg) toast(msg); emit('uiError', {}); return false; };
const save = () => ctx.save;
const tierNow = () => (ctx.hub.flame ? ctx.hub.flame.tier : 1);
const zoneName = (id) => (ZONES[id] ? ZONES[id].name : id);
const describe = (c) => (ctx.actions && ctx.actions.describe ? ctx.actions.describe(c) : JSON.stringify(c));

/* ============================================================
   init
   ============================================================ */
export function init(c) {
  ctx = c;
  ctx.hub.flame = { group: null, fire: null, light: null, base: null, rim: null, boxes: [], tier: 1, baseInt: 2, model: null };
  Object.defineProperty(ctx.hub, 'buildings', { get: () => ctx.save.buildings, configurable: true, enumerable: true });
  ctx.hub.meshes = meshes;
  ensureAnchors(ctx.hub.map);
  buildFlame(ctx.hub.map);
  ghostMat = new THREE.MeshBasicMaterial({ color: HUB_CFG.ghostColor, wireframe: true });
  ghostMat._shared = true;
  padGeo = new THREE.BoxGeometry(1.8, 0.02, 1.8);
  applyLightTechCfg();
  ensureHud();
  ctx.hub.api = { BUILDINGS, BUILD_ORDER, HUB_CFG, buildStatus, canBuild, build, costText, openBoard, openBuilding, closeMenu, select, selected,
    lightTech, startOil, tier, tierFor, checkTier, exploredBits, exploredPct, drawMinimap, blessing: () => ({ on: !!save().blessing, lit: run.blessed }),
    applyBlessing, refresh, anchorOf, colliders, rebuildColliders, embers: () => ({ alive: embers.alive, visible: !!(embers.points && embers.points.visible) }),
    HUB_BLOCK, get menu() { return menu; } };
  rebuildColliders();
  if (typeof window !== 'undefined') setTimeout(() => { if (window.__game && !window.__game.hub) window.__game.hub = ctx.hub.api; }, 0);

  const ev = ctx.events;
  ev.on('hubEnter', () => { checkTier(true); refreshBuildings(true); rebuildColliders(); if (embers.points) embers.points.visible = true; });
  ev.on('npcRescued', () => rebuildColliders());
  ev.on('zoneEnter', () => { if (embers.points) embers.points.visible = false; });
  ev.on('bank', ({ carried }) => {
    // v2 resource ledger (DESIGN-v2 §7): banking credits oil/relics/rich alongside flame points
    const s = save();
    s.oil += (carried.oil | 0) * CFG.flaskOil; s.relics += carried.relic | 0; s.rich += carried.rich | 0;
  });
  ev.on('npcRescued', ({ id }) => {
    refreshBuildings();
    const b = BUILD_ORDER.find(k => BUILDINGS[k].npc === id);
    if (b && !save().buildings[b]) toast(`${NPC_NAME[id] || id} could raise a ${BUILDINGS[b].name} at the Lantern.`);
  });
  ev.on('flameTier', ({ initial }) => { refreshBuildings(initial); syncBoardPapers(); });
  ev.on('lightTech', () => syncBoardPapers());
  ev.on('build', () => syncBoardPapers());
  ev.on('zoneEnter', ({ zoneId }) => onZoneEnter(zoneId));
  ev.on('zoneExit', () => { flushExplored(true); run.blessed = false; });
  ev.on('key', ({ code, mode }) => onKey(code, mode));
  ev.on('menuClose', () => { menu = null; });
  ev.on('toolGained', () => { if (menu && menu.kind === 'board') openBoard(); });
  ev.on('contractAccepted', () => { if (menu && menu.kind === 'board') openBoard(); });
  ev.on('contractComplete', () => { if (menu && menu.kind === 'board') openBoard(); });
  if (ctx.dom && ctx.dom.menubody) ctx.dom.menubody.addEventListener('click', onMenuClick);

  const t = tierFor(save().points);
  applyTier(t);
  refreshBuildings(true);
  ctx.events.emit('flameTier', { tier: t, prev: 0, initial: true });
}

/* ============================================================
   Anchors
   ============================================================ */
// The v2 hub rows carry digit anchors; the v1 hub gets the V1_ANCHORS table written onto map.anchors.
function ensureAnchors(m) {
  if (!m) return;
  if (!m.anchors) m.anchors = {};
  const rots = {};
  if (!Object.keys(m.anchors).length) {
    for (const [ch, a] of Object.entries(V1_ANCHORS)) {
      if (!inBounds(m, a.cx, a.cz) || isSolid(m, a.cx, a.cz)) continue;
      const p = center(m, a.cx, a.cz);
      m.anchors[ch] = { cx: a.cx, cz: a.cz, idx: idx(m, a.cx, a.cz), x: p.x, z: p.z };
      rots[ch] = { rot: a.rot || 0, dx: a.dx || 0, dz: a.dz || 0 };
    }
  }
  const anchors = {};
  for (const id of BUILD_ORDER) {
    const b = BUILDINGS[id], a = m.anchors[b.anchor];
    if (!a) continue;
    const r = rots[b.anchor] || { rot: b.rot || 0, dx: b.dx || 0, dz: b.dz || 0 };
    anchors[id] = { id, cx: a.cx, cz: a.cz, x: a.x, z: a.z, rot: r.rot, dx: r.dx, dz: r.dz };
  }
  ctx.hub.anchors = anchors;
}
export function anchorOf(id) { return (ctx && ctx.hub.anchors && ctx.hub.anchors[id]) || null; }

/* ============================================================
   The great flame — models.flameBase() (same brazier/tongue maths as v1; handles kept on ctx.hub.flame)
   ============================================================ */
function buildFlame(m) {
  const flame = ctx.hub.flame;
  const p = center(m, m.flame.cx, m.flame.cz);
  const g = models.flameBase({ tier: 1 }); g.position.set(p.x, 0, p.z); g.name = 'flame';
  const ud = g.userData;
  flame.base = ud.base; flame.rim = ud.rim; flame.fire = ud.fire; flame.boxes = ud.tongues; flame.model = ud;
  const light = new THREE.PointLight(0xffa040, 2, 7, 2); light.position.y = ud.lightY || 1.2; g.add(light);
  ctx.scene.add(g);
  flame.group = g; flame.light = light;
  buildEmbers(g);
}
// Embers: EMBERS.max points born in the coal bed, rising and drifting, fading out over their life; perTier[tier−1] alive.
function buildEmbers(parent) {
  const n = EMBERS.max;
  embers.pos = new Float32Array(n * 3); embers.life = new Float32Array(n); embers.vel = new Float32Array(n * 3);
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.Float32BufferAttribute(embers.pos, 3));
  geo.attributes.position.setUsage(THREE.DynamicDrawUsage);
  const mat = new THREE.PointsMaterial({ color: 0xffa040, size: EMBERS.size, sizeAttenuation: true, transparent: true, opacity: 0.9, depthWrite: false, blending: THREE.AdditiveBlending });
  const pts = new THREE.Points(geo, mat); pts.name = 'flame:embers'; pts.frustumCulled = false;
  for (let i = 0; i < n; i++) { embers.life[i] = -Math.random() * 2; embers.pos[i * 3 + 1] = -5; }
  parent.add(pts); embers.points = pts;
}
function spawnEmber(i) {
  const a = Math.random() * Math.PI * 2, r = Math.random() * 0.28;
  embers.pos[i * 3] = Math.cos(a) * r; embers.pos[i * 3 + 1] = 0.55 + Math.random() * 0.3 * ctx.hub.flame.tier; embers.pos[i * 3 + 2] = Math.sin(a) * r;
  embers.vel[i * 3] = (Math.random() - 0.5) * EMBERS.drift; embers.vel[i * 3 + 1] = EMBERS.rise[0] + Math.random() * (EMBERS.rise[1] - EMBERS.rise[0]); embers.vel[i * 3 + 2] = (Math.random() - 0.5) * EMBERS.drift;
  embers.life[i] = EMBERS.life[0] + Math.random() * (EMBERS.life[1] - EMBERS.life[0]);
}
function updateEmbers(dt, time) {
  const pts = embers.points; if (!pts || !pts.visible || dt <= 0) return;
  const want = EMBERS.perTier[Math.max(0, Math.min(3, ctx.hub.flame.tier - 1))];
  let alive = 0;
  for (let i = 0; i < EMBERS.max; i++) {
    if (embers.life[i] > 0) {
      embers.life[i] -= dt;
      const k = i * 3;
      embers.pos[k] += (embers.vel[k] + 0.15 * Math.sin(time * 3 + i)) * dt; embers.pos[k + 1] += embers.vel[k + 1] * dt; embers.pos[k + 2] += (embers.vel[k + 2] + 0.15 * Math.cos(time * 2.6 + i)) * dt;
      if (embers.life[i] <= 0 || embers.pos[k + 1] > 2.9) { embers.life[i] = -0.2 - Math.random() * 0.6; embers.pos[k + 1] = -5; }
      else alive++;
    } else if (alive < want && i < want) { embers.life[i] += dt; if (embers.life[i] >= 0) { spawnEmber(i); alive++; } }
  }
  embers.alive = alive;
  pts.geometry.attributes.position.needsUpdate = true;
  pts.material.opacity = 0.7 + 0.2 * Math.sin(time * 7);
}

/* ============================================================
   Collision mask: buildings (ghost or built), the brazier and the standing residents (world.js adds the props)
   ============================================================ */
const bbTmp = new THREE.Box3();
function markObject(m, obj, bit) {
  const mark = ctx.world && ctx.world.markBoxCells; if (!mark) return;
  obj.updateMatrixWorld(true);
  obj.traverse(o => { if (!o.isMesh || o.isInstancedMesh || o.isPoints) return; bbTmp.setFromObject(o); mark(m, bbTmp, bit); });
}
// rebuildColliders(): recompute the building / flame / NPC bits of ctx.hub.map.blockMask (prop bits are kept).
export function rebuildColliders() {
  const m = ctx && ctx.hub.map; if (!m || !m.blockMask) return null;
  const keep = HUB_BLOCK.PROP;
  for (let i = 0; i < m.blockMask.length; i++) m.blockMask[i] &= keep;
  for (const id of BUILD_ORDER) { const r = meshes[id]; if (r && r.group && r.state !== 'none') markObject(m, r.group, HUB_BLOCK.BUILDING); }
  if (ctx.hub.flame && ctx.hub.flame.group) markObject(m, ctx.hub.flame.group, HUB_BLOCK.FLAME);
  for (const r of ctx.npcs || []) {
    if (r.where !== 'hub') continue;
    const c = toCell(m, r.x, r.z);
    if (inBounds(m, c.cx, c.cz) && !isSolid(m, c.cx, c.cz)) m.blockMask[idx(m, c.cx, c.cz)] |= HUB_BLOCK.NPC;
  }
  colliderT = 0;
  return m.blockMask;
}
// colliders() → [{cx, cz, bits, kinds[]}] for every blocked hub cell (debug / hub-audit).
export function colliders() {
  const m = ctx && ctx.hub.map; if (!m || !m.blockMask) return [];
  const out = [];
  for (let i = 0; i < m.blockMask.length; i++) {
    const b = m.blockMask[i]; if (!b) continue;
    const kinds = Object.keys(HUB_BLOCK).filter(k => typeof HUB_BLOCK[k] === 'number' && k === k.toUpperCase() && (b & HUB_BLOCK[k]));
    out.push({ cx: i % m.w, cz: (i / m.w) | 0, bits: b, kinds });
  }
  return out;
}
function applyTier(tier) {
  const flame = ctx.hub.flame, t = TIERS[tier - 1];
  flame.tier = tier; flame.baseInt = t.int; flame.light.distance = t.dist;
  flame.model.setTier(tier);   // fire scale, tongue gradient (dark red → yellow-white tip), brazier/rim glow
}
export function tierFor(points) { let t = 1; for (let i = 0; i < TIERS.length; i++) if (points >= TIERS[i].pts) t = i + 1; return t; }
// Recompute the tier from save.points; on change apply it and emit flameTier (ui toasts the message).
export function checkTier(announce = true) {
  const flame = ctx.hub.flame, t = tierFor(save().points);
  if (t !== flame.tier || !announce) {
    const prev = flame.tier;
    applyTier(t);
    ctx.events.emit('flameTier', { tier: t, prev, initial: !announce });
  }
  return flame.tier;
}
export const tier = () => ctx.hub.flame.tier;

/* ============================================================
   Buildings: ghosts, meshes, status, build
   ============================================================ */
function makeModel(id) {
  const b = BUILDINGS[id], f = ctx.models && ctx.models[b.model];
  let g = null;
  if (typeof f === 'function') { try { g = f(); } catch (e) { console.warn(`[hub] models.${b.model} failed:`, e); g = null; } }
  if (!g || !g.isObject3D) {
    g = new THREE.Group();
    const m = new THREE.Mesh(new THREE.BoxGeometry(1.2, 1.0, 0.8), new THREE.MeshLambertMaterial({ color: 0x7a5a3a }));
    m.position.y = 0.5; g.add(m);
  }
  return g;
}
function placeGroup(g, a) {
  g.position.set(a.x + (a.dx || 0), 0, a.z + (a.dz || 0));
  g.rotation.y = a.rot || 0;
}
function removeMesh(id) {
  const r = meshes[id]; if (!r) return;
  if (r.group) ctx.scene.remove(r.group);
  if (r.pad) { ctx.scene.remove(r.pad); r.pad.material.dispose(); }
  delete meshes[id];
}
function setMesh(id, state) {
  removeMesh(id);
  const a = anchorOf(id); if (!a || state === 'none') { meshes[id] = { state: 'none', group: null, light: null }; return; }
  const g = makeModel(id);
  g.name = `${state}:${id}`;
  placeGroup(g, a);
  let pad = null;
  if (state === 'ghost') {
    g.traverse(o => { if (o.isMesh) o.material = ghostMat; });
  } else {
    // candle/lamp light on the flagstones: an emissive pad instead of a PointLight (cheap at any frame rate)
    pad = new THREE.Mesh(padGeo, new THREE.MeshLambertMaterial({ color: 0x000000, emissive: 0xffc070, emissiveIntensity: HUB_CFG.padGlow, transparent: true, opacity: 0.55 }));
    pad.position.set(a.x + (a.dx || 0), 0.011, a.z + (a.dz || 0));
    ctx.scene.add(pad);
  }
  ctx.scene.add(g);
  meshes[id] = { state, group: g, pad, phase: Math.random() * 6 };
  if (id === 'board' && state === 'built') {
    // own paper materials (models may hand out cached ones), a candle on the cap so the notices read in any light
    for (const pm of (g.userData && g.userData.papers) || []) if (pm && pm.material) { pm.material = pm.material.clone(); pm.material._shared = false; }
    const candle = new THREE.Mesh(new THREE.BoxGeometry(0.08, 0.16, 0.08), new THREE.MeshLambertMaterial({ color: 0x000000, emissive: 0xffd080, emissiveIntensity: 1 }));
    candle.position.set(0.55, 2.14, 0.1); candle.name = 'candle'; g.add(candle);
    syncBoardPapers();
  }
}
// unlocked(id): may the ghost show? NPC buildings need their NPC rescued; tram/elevator need the flame tier.
function unlocked(id) {
  const b = BUILDINGS[id]; if (b.always) return true;
  if (b.npc) return !!(save().rescued && save().rescued[b.npc]);
  if (b.tier) return tierNow() >= b.tier;
  return true;
}
const isBuilt = (id) => !!(BUILDINGS[id].always || (save().buildings && save().buildings[id]));
// buildStatus(id) → {id, built, unlocked, affordable, reason, cost, have}
export function buildStatus(id) {
  const b = BUILDINGS[id]; if (!b) return null;
  const cost = costOf(id), s = save();
  const have = { oil: s.oil | 0, relics: s.relics | 0, rich: s.rich | 0 };
  const built = isBuilt(id), unl = unlocked(id);
  let reason = null;
  if (built) reason = 'Already built';
  else if (!unl) reason = b.npc ? `Needs ${NPC_NAME[b.npc] || b.npc} rescued` : `Needs the flame at tier ${b.tier}`;
  else {
    const short = [];
    if ((cost.oil || 0) > have.oil) short.push(`${cost.oil - have.oil} more oil`);
    if ((cost.relics || 0) > have.relics) short.push(`${cost.relics - have.relics} more relics`);
    if ((cost.rich || 0) > have.rich) short.push(`${cost.rich - have.rich} more rich relics`);
    if (short.length) reason = `Needs ${short.join(', ')}`;
  }
  return { id, built, unlocked: unl, affordable: !reason, reason, cost, have };
}
export function canBuild(id) { const st = buildStatus(id); return !!(st && !st.built && st.unlocked && st.affordable); }
function costOf(id) { const c = BUILD_COSTS[id] || {}; return { oil: c.oil | 0, relics: c.relics | 0, rich: c.rich | 0 }; }
export function costText(cost) {
  if (typeof cost === 'string') cost = costOf(cost);
  const parts = [];
  if (cost.oil) parts.push(`${cost.oil} oil`);
  if (cost.relics) parts.push(`${cost.relics} relic${cost.relics === 1 ? '' : 's'}`);
  if (cost.rich) parts.push(`${cost.rich} rich`);
  return parts.length ? parts.join(' + ') : 'free';
}
function spend(cost) { const s = save(); s.oil -= cost.oil | 0; s.relics -= cost.relics | 0; s.rich -= cost.rich | 0; }
// build(id, {free}) → true when built. `free` skips cost and requirements (test hook).
export function build(id, { free = false } = {}) {
  const b = BUILDINGS[id]; if (!b) return false;
  const st = buildStatus(id);
  if (st.built) return false;
  if (!free && !st.unlocked) return err(st.reason);
  if (!free && !st.affordable) return err(st.reason);
  if (!free) spend(st.cost);
  save().buildings[id] = true;
  refreshBuildings();
  emit('build', { id, cost: free ? { oil: 0, relics: 0, rich: 0 } : st.cost });
  emit('uiClick', {});
  toast(`${b.name} built.`);
  for (const z of ZONE_ORDER) if (ZONE_KEY[z] === id && !zoneLocked(z, save(), tierNow())) toast(`${zoneName(z)} is open. Choose it at the Departure Board.`);
  if (menu && menu.kind === 'build' && menu.id === id) openBuilding(id);
  return true;
}
// refreshBuildings(quiet): make every building's mesh match {none, ghost, built}; announce new ghosts unless quiet.
function refreshBuildings(quiet = false) {
  if (!ctx.hub.anchors) return;
  for (const id of BUILD_ORDER) {
    const state = isBuilt(id) ? 'built' : unlocked(id) ? 'ghost' : 'none';
    const cur = meshes[id] ? meshes[id].state : null;
    if (cur === state) continue;
    setMesh(id, state);
    if (!quiet && state === 'ghost' && cur === 'none' && BUILDINGS[id].tier) toast(`The flame is strong enough for a ${BUILDINGS[id].name} (${costText(id)}).`);
  }
  rebuildColliders();
}
function syncBoardPapers() {
  const r = meshes.board; if (!r || !r.group || !r.group.userData) return;
  const ud = r.group.userData, papers = ud.papers || [];
  ZONE_ORDER.forEach((z, i) => {
    const locked = !!zoneLocked(z, save(), tierNow());
    if (typeof ud.setLocked === 'function') ud.setLocked(i, locked);
    const pm = papers[i];
    if (pm && pm.material && pm.material.emissive) { pm.material.emissive.set(locked ? 0x555555 : 0xe0d0a0); pm.material.emissiveIntensity = locked ? 0.08 : 0.3; }
  });
}

/* ============================================================
   Services
   ============================================================ */
// Light-tech multipliers: {distMul, burnMul, flashCost, lanternCost} for the current save.lightTech.
export function lightTech() { return LIGHT_TECH[Math.max(0, Math.min(LIGHT_TECH.length - 1, (ctx ? save().lightTech : 0) | 0))]; }
// Mirror the tier's flash/lantern costs onto CFG so the HUD cooldown dots (ui.js reads CFG.*Cost) agree with main.js.
function applyLightTechCfg() { const t = lightTech(); CFG.flashCost = t.flashCost; CFG.lanternCost = t.lanternCost; }
// Oil the lamp starts with on descend: tier start value + reservoir upgrades.
export function startOil() { return CFG.startOil[ctx.hub.flame.tier - 1] + BUILD_COSTS.reservoirOil * (save().reservoir | 0); }
export const selected = () => (ctx && ZONES[save().zoneSelected] ? save().zoneSelected : 'undercroft');

function upgradeLightTech() {
  const s = save(), cur = s.lightTech | 0, next = cur + 1;
  if (next >= LIGHT_TECH.length) return err('Nothing more to learn here.');
  const cost = LIGHT_TECH[next].cost, need = [];
  if ((cost.relics | 0) > (s.relics | 0)) need.push(`${cost.relics - (s.relics | 0)} more relics`);
  if ((cost.rich | 0) > (s.rich | 0)) need.push(`${cost.rich - (s.rich | 0)} more rich relics`);
  if (need.length) return err(`Needs ${need.join(', ')}`);
  s.relics -= cost.relics | 0; s.rich -= cost.rich | 0; s.lightTech = next;
  applyLightTechCfg();
  emit('lightTech', { tier: next, prev: cur });
  emit('service', { id: 'workshop', action: 'lightTech', tier: next });
  emit('uiClick', {});
  toast(`Light-tech ${ROMAN[next]}: reach ×${LIGHT_TECH[next].distMul}, burn ×${LIGHT_TECH[next].burnMul}.`);
  for (const z of ZONE_ORDER) if (ZONES[z].requires && ZONES[z].requires.lightTech === next && !zoneLocked(z, s, tierNow())) toast(`${zoneName(z)} is open. Choose it at the Departure Board.`);
  return true;
}
function pressRelics(n) {
  const s = save(); n = Math.min(n | 0, s.relics | 0);
  if (n <= 0) return err('No relics to press.');
  const oil = n * BUILD_COSTS.pressRelicOil;
  s.relics -= n; s.oil += oil;
  emit('service', { id: 'press', action: 'press', n, oil });
  emit('uiClick', {});
  toast(`Pressed ${n} relic${n === 1 ? '' : 's'} into ${oil} oil.`);
  return true;
}
function deepenReservoir() {
  const s = save(), lvl = s.reservoir | 0, costs = BUILD_COSTS.reservoir;
  if (lvl >= costs.length) return err('The reservoir is as deep as it goes.');
  const cost = costs[lvl];
  if ((s.relics | 0) < cost) return err(`Needs ${cost - (s.relics | 0)} more relics`);
  s.relics -= cost; s.reservoir = lvl + 1;
  if (ctx.state.mode === 'HUB' || ctx.state.mode === 'MENU') ctx.player.oil = startOil();
  emit('service', { id: 'press', action: 'reservoir', level: s.reservoir, startOil: startOil() });
  emit('uiClick', {});
  toast(`Deeper reservoir: the lamp now starts with ${startOil()} oil.`);
  return true;
}
function toggleBlessing() {
  const s = save(); s.blessing = !s.blessing;
  emit('blessing', { on: s.blessing });
  emit('uiClick', {});
  toast(s.blessing ? `The blessing is lit — ${BUILD_COSTS.blessingOil} oil at each descent.` : 'The blessing is snuffed.');
  return true;
}
// applyBlessing(carried) → what goes into the death bundle. In a blessed run ⌊half⌋ of each carried kind (oil/relic/
// rich) is banked on the spot (both ledgers) and only the rest is dropped; main.die() calls this before spawning
// the bundle. Unblessed: the carried loot comes back unchanged.
export function applyBlessing(carried) {
  const c = { oil: 0, relic: 0, rich: 0, quest: 0, ...(carried || {}) };
  if (!run.blessed) return c;
  run.blessed = false;
  const kept = {}; let pts = 0, any = false;
  for (const k of ['oil', 'relic', 'rich']) {
    const n = c[k] | 0, keep = Math.floor(n / 2);
    if (keep <= 0) continue;
    c[k] = n - keep; kept[k] = keep; any = true;
    pts += keep * (POINTS[k] | 0);
  }
  if (!any) return c;
  const s = save();
  s.oil += (kept.oil | 0) * CFG.flaskOil; s.relics += kept.relic | 0; s.rich += kept.rich | 0;
  s.points += pts; s.stats.banked += pts;
  emit('blessingKept', { kept, pts, zoneId: ctx.zone.id, x: ctx.player.x, z: ctx.player.z });
  toast(`The blessing kept ${describe({ oil: kept.oil | 0, relic: kept.relic | 0, rich: kept.rich | 0, quest: 0 })} (+${pts}).`);
  return c;
}
// Charge the blessing at zoneEnter; a blessed run keeps ⌊half⌋ of each carried kind on death (applyBlessing).
function onZoneEnter(zoneId) {
  run.blessed = false;
  const s = save();
  if (s.blessing && s.buildings && s.buildings.shrine) {
    if ((s.oil | 0) >= BUILD_COSTS.blessingOil) { s.oil -= BUILD_COSTS.blessingOil; run.blessed = true; toast(`The blessing holds (−${BUILD_COSTS.blessingOil} oil).`); }
    else toast('No oil for the blessing; the shrine candles stay dark.');
    emit('service', { id: 'shrine', action: 'charge', lit: run.blessed });
  }
  exploreT = 0; minimapT = 0;
}
// refresh(): re-derive everything from the save (debug actions that rewrite save fields call this).
export function refresh() { applyLightTechCfg(); refreshBuildings(true); syncBoardPapers(); }

/* ============================================================
   Interaction + menus
   ============================================================ */
// interactTarget(ctx): nearest visible building within HUB_CFG.interactR → {type:'build'|'building', id, label, run()};
// otherwise the hub stairs → {type:'descend', zoneId, label} naming the selected zone (main runs descend()).
export function interactTarget(c) {
  if (c.state.mode !== 'HUB' || !c.hub.anchors) return null;
  const p = c.player;
  let best = null, bd = HUB_CFG.interactR;
  for (const id of BUILD_ORDER) {
    const r = meshes[id]; if (!r || r.state === 'none') continue;
    const a = anchorOf(id); if (!a) continue;
    const d = dist2d(a.x, a.z, p.x, p.z);
    if (d < bd) { best = id; bd = d; }
  }
  if (best) {
    const b = BUILDINGS[best];
    if (isBuilt(best)) return { type: 'building', id: best, label: `[E] ${b.name}`, run: () => openBuilding(best) };
    return { type: 'build', id: best, label: `[E] Build ${b.name} — ${costText(best)}`, run: () => openBuilding(best) };
  }
  const m = c.hub.map;
  if (m && m.stairs) {
    const s = center(m, m.stairs.cx, m.stairs.cz);
    if (dist2d(s.x, s.z, p.x, p.z) <= CFG.interactR) {
      const zid = selected(), lock = zoneLocked(zid, save(), tierNow());
      return { type: 'descend', zoneId: zid, label: `[E] Descend — ${zoneName(zid)}${lock ? ' (locked: ' + lock + ')' : ''}` };
    }
  }
  return null;
}
// showMenu(kind, opts, handlers): open (or redraw, when already open) the generic ui menu in MENU mode.
function showMenu(kind, opts, handlers) {
  const A = ctx.actions, U = ctx.ui;
  if (ctx.state.mode === 'MENU') {
    if (!menu || !U || typeof U.showMenu !== 'function') return false;
    U.showMenu(opts);
  } else {
    if (!A || typeof A.openMenu !== 'function') return false;
    if (A.openMenu(kind, opts) === false) return false;
  }
  menu = { kind, opts, ...handlers };
  const special = U && (kind === 'board' ? U.showBoard : U.showBuild);
  if (typeof special === 'function' && special !== U.showMenu) { try { special(opts); } catch (e) { /* keep the generic menu */ } }
  decorateMenu();
  return true;
}
export function closeMenu() { if (menu && ctx.actions && typeof ctx.actions.closeMenu === 'function') return ctx.actions.closeMenu(); return false; }
// Lines beginning with "[n]" are clickable: mark them.
function decorateMenu() {
  const body = ctx.dom && ctx.dom.menubody; if (!body) return;
  for (const d of body.children) { const m = /^\s*\[(\d)\]/.exec(d.textContent || ''); d.style.cursor = m ? 'pointer' : ''; }
}
function onMenuClick(e) {
  if (!menu || ctx.state.mode !== 'MENU') return;
  const d = e.target && e.target.closest ? e.target.closest('#menubody > div') : null;
  const m = d && /^\s*\[(\d)\]/.exec(d.textContent || '');
  if (m) handleMenuKey(`Digit${m[1]}`);
}
function onKey(code, mode) {
  if (mode === 'MENU') { handleMenuKey(code); return; }
  if ((mode === 'HUB' || mode === 'ZONE') && code === KEYS.minimap) onMinimapToggle();
}
function handleMenuKey(code) {
  if (!menu || ctx.state.mode !== 'MENU') return;
  const n = /^(?:Digit|Numpad)(\d)$/.exec(code);
  if (n && typeof menu.pick === 'function') { menu.pick(parseInt(n[1], 10)); return; }
  if (KEYS.confirm.includes(code) || code === KEYS.interact) { if (typeof menu.confirm === 'function') menu.confirm(); else closeMenu(); }
}
const haveLine = () => { const s = save(); return `  You have ${s.oil | 0} oil, ${s.relics | 0} relic${s.relics === 1 ? '' : 's'}, ${s.rich | 0} rich.`; };

// openBuilding(id): the build menu for a ghost, the service menu for a built building.
export function openBuilding(id) {
  const b = BUILDINGS[id]; if (!b || !ctx) return false;
  if (!isBuilt(id)) return openBuildMenu(id);
  switch (id) {
    case 'board': return openBoard();
    case 'workshop': return openWorkshop();
    case 'press': return openPress();
    case 'cart': return openCart();
    case 'shrine': return openShrine();
    case 'tram': return openRide('tram');
    case 'elevator': return openRide('elevator');
    default: return false;
  }
}
function openBuildMenu(id) {
  const b = BUILDINGS[id], st = buildStatus(id);
  const lines = [b.desc, '', `Cost: ${costText(st.cost)}`, haveLine(), ''];
  if (b.npc) lines.push(`  ${NPC_NAME[b.npc]} will keep it.`);
  lines.push(st.affordable ? `[1] Build the ${b.name}` : `  [1] Build the ${b.name} — ${st.reason}`);
  return showMenu('build', { title: `${b.name} (unbuilt)`, lines, foot: '1 to build · Esc to close' },
    { id, pick: (n) => { if (n === 1) build(id); }, confirm: () => { if (canBuild(id)) build(id); else closeMenu(); } });
}
function techLine(t, label) {
  const x = LIGHT_TECH[t];
  return `${label}: lamp reach ×${x.distMul}, burn ×${x.burnMul}, flash ${x.flashCost} oil, lantern ${x.lanternCost} oil`;
}
function openWorkshop() {
  const s = save(), cur = s.lightTech | 0, next = cur + 1, lines = [];
  lines.push(`"${'Every lamp I ever lit is out. Let\'s fix that.'}"`, '');
  lines.push(cur > 0 ? techLine(cur, `Light-tech ${ROMAN[cur]} (yours)`) : 'Your handlamp is plain: reach ×1, burn ×1, flash 15 oil, lantern 20 oil.');
  lines.push('');
  if (next < LIGHT_TECH.length) {
    const cost = LIGHT_TECH[next].cost, ok = (s.relics | 0) >= (cost.relics | 0) && (s.rich | 0) >= (cost.rich | 0);
    lines.push(`${ok ? '' : '  '}[1] Light-tech ${ROMAN[next]} — ${costText(cost)}`);
    lines.push(`  ${techLine(next, '   gives')}`);
  } else lines.push('  Nothing more to learn here.');
  lines.push(haveLine());
  return showMenu('service', { title: 'Workshop', lines, foot: '1 to upgrade · Esc to close' },
    { id: 'workshop', pick: (n) => { if (n === 1 && upgradeLightTech()) openWorkshop(); }, confirm: () => closeMenu() });
}
function openPress() {
  const s = save(), lvl = s.reservoir | 0, costs = BUILD_COSTS.reservoir, lines = [];
  lines.push('"Relics burn better than they pray."', '');
  lines.push(`${(s.relics | 0) > 0 ? '' : '  '}[1] Press one relic → ${BUILD_COSTS.pressRelicOil} oil`);
  lines.push(`${(s.relics | 0) > 0 ? '' : '  '}[2] Press every relic (${s.relics | 0} → ${(s.relics | 0) * BUILD_COSTS.pressRelicOil} oil)`);
  if (lvl < costs.length) lines.push(`${(s.relics | 0) >= costs[lvl] ? '' : '  '}[3] Deeper reservoir (${lvl}/${costs.length}) — ${costs[lvl]} relics: the lamp starts with +${BUILD_COSTS.reservoirOil} oil`);
  else lines.push(`  Reservoir ${lvl}/${costs.length}: the lamp starts with ${startOil()} oil.`);
  lines.push(haveLine());
  return showMenu('service', { title: 'Oil Press', lines, foot: '1–3 choose · Esc to close' },
    { id: 'press', pick: (n) => { const ok = n === 1 ? pressRelics(1) : n === 2 ? pressRelics(save().relics | 0) : n === 3 ? deepenReservoir() : false; if (ok) openPress(); }, confirm: () => closeMenu() });
}
function openCart() {
  const lines = ['"I mapped every one of these halls. Then they moved."', '', 'Tab shows the map while you are below: explored halls, items you have seen, contract spots, the way out.', ''];
  for (const z of ZONE_ORDER) lines.push(`  ${zoneName(z)} — ${exploredPct(z)}% charted`);
  lines.push('', '[1] Show or hide the map now');
  return showMenu('service', { title: "Cartographer's Table", lines, foot: '1 toggles · Esc to close' },
    { id: 'cart', pick: (n) => { if (n === 1) toggleMinimap(); }, confirm: () => closeMenu() });
}
function openShrine() {
  const s = save(), on = !!s.blessing;
  const lines = ['"The Source can be fed, or freed. Both are prayers."', '',
    `Blessing: ${on ? 'LIT' : 'unlit'}. ${BUILD_COSTS.blessingOil} oil at each descent; when the dark takes you, half of each kind you carry is banked anyway.`, '',
    `[1] ${on ? 'Snuff the blessing' : 'Light the blessing'}`, haveLine()];
  return showMenu('service', { title: 'Shrine', lines, foot: '1 toggles · Esc to close' },
    { id: 'shrine', pick: (n) => { if (n === 1 && toggleBlessing()) openShrine(); }, confirm: () => closeMenu() });
}
function openRide(id) {
  const zones = id === 'tram' ? ['cistern'] : ['ossuary', 'source'];
  const lines = [BUILDINGS[id].desc, ''];
  zones.forEach((z, i) => { const lock = zoneLocked(z, save(), tierNow()); lines.push(lock ? `  [${i + 1}] ${zoneName(z)} — locked: ${lock}` : `[${i + 1}] Ride to ${zoneName(z)}`); });
  return showMenu('service', { title: BUILDINGS[id].name, lines, foot: '1–2 ride · Esc to close' },
    { id, pick: (n) => { const z = zones[n - 1]; if (!z) return; if (select(z, { check: true })) { closeMenu(); if (ctx.actions && ctx.actions.descend) ctx.actions.descend(); } },
      confirm: () => closeMenu() });
}

/* ============================================================
   Departure board
   ============================================================ */
// openBoard(): zone list with lock reasons + active contract targets; 1–4 select, Enter/E confirm (close).
export function openBoard() {
  if (!ctx) return false;
  const s = save(), t = tierNow(), sel = selected(), lines = [];
  const targets = ctx.contracts && typeof ctx.contracts.targets === 'function' ? ctx.contracts.targets() : [];
  ZONE_ORDER.forEach((z, i) => {
    const lock = zoneLocked(z, s, t), meta = ZONES[z];
    if (lock) lines.push(`  [${i + 1}] ${meta.name} — locked: ${lock}`);
    else lines.push(`[${i + 1}] ${meta.name}${z === sel ? '   ◆ next descent' : ''}`);
    for (const c of targets) if (c.zone === z) {
      const spot = c.cell && meta.spots ? meta.spots.find(sp => sp.cell[0] === c.cell[0] && sp.cell[1] === c.cell[1]) : null;
      lines.push(`      ◇ ${c.title}${spot ? ' — ' + spot.label : ''}${c.goal > 1 ? ` (${c.progress | 0}/${c.goal})` : ''}`);
    }
  });
  if (!targets.length) lines.push('', '  No contracts posted. The rescued will have work for you.');
  return showMenu('board', { title: 'Departure Board', lines, foot: '1–4 choose · Enter / E confirm · Esc close' },
    { id: 'board', pick: (n) => { const z = ZONE_ORDER[n - 1]; if (z && select(z, { check: true })) openBoard(); }, confirm: () => closeMenu() });
}
// select(zoneId, {check}) → record save.zoneSelected and emit zoneSelected. With `check`, a locked zone is refused
// (uiError + reason); without it any known zone is accepted (actions.loadZone / test hook).
export function select(zoneId, { check = false } = {}) {
  if (!ZONES[zoneId]) return false;
  if (check) { const lock = zoneLocked(zoneId, save(), tierNow()); if (lock) return err(`${zoneName(zoneId)}: ${lock}`); }
  const changed = save().zoneSelected !== zoneId;
  save().zoneSelected = zoneId;
  ctx.events.emit('zoneSelected', { zoneId });
  if (check && changed) { emit('uiClick', {}); toast(`Next descent: ${zoneName(zoneId)}.`); }
  return true;
}

/* ============================================================
   Explored bitsets + minimap
   ============================================================ */
const b64 = (u8) => { let s = ''; for (let i = 0; i < u8.length; i++) s += String.fromCharCode(u8[i]); return btoa(s); };
const unb64 = (s, n) => { const out = new Uint8Array(n); try { const d = atob(s); for (let i = 0; i < Math.min(n, d.length); i++) out[i] = d.charCodeAt(i); } catch (e) { /* invalid: fresh */ } return out; };
function zoneMap(id) {
  if (ctx.zone && ctx.zone.id === id && ctx.zone.map) return ctx.zone.map;
  if (!parsed[id] && ctx.maps && ctx.maps.parseZone && ZONES[id]) parsed[id] = ctx.maps.parseZone(id);
  return parsed[id] || null;
}
// exploredBits(zoneId) → Uint8Array bitset (idx = cz·w + cx), loaded from save.explored on first use.
export function exploredBits(id) {
  if (explored[id]) return explored[id];
  const z = ZONES[id]; if (!z) return null;
  const n = Math.ceil(z.rows.length * z.rows[0].length / 8);
  const s = save().explored && save().explored[id];
  explored[id] = typeof s === 'string' && s ? unb64(s, n) : new Uint8Array(n);
  return explored[id];
}
const bitSet = (bits, i) => !!(bits[i >> 3] & (1 << (i & 7)));
// exploredPct(zoneId) → % of non-wall cells seen.
export function exploredPct(id) {
  const bits = exploredBits(id); if (!bits) return 0;
  if (walkable[id] == null) { const m = zoneMap(id); let n = 0; if (m) for (let i = 0; i < m.cells.length; i++) if (m.cells[i] !== T.WALL) n++; walkable[id] = n; }
  const m = zoneMap(id); if (!m || !walkable[id]) return 0;
  let seen = 0;
  for (let i = 0; i < m.cells.length; i++) if (m.cells[i] !== T.WALL && bitSet(bits, i)) seen++;
  return Math.round(100 * seen / walkable[id]);
}
function flushExplored(force) {
  if (!exploredDirty && !force) return;
  const s = save(); if (!s.explored || typeof s.explored !== 'object') s.explored = {};
  for (const id of Object.keys(explored)) s.explored[id] = b64(explored[id]);
  exploredDirty = false; exploredSaveT = 0;   // wall-clock throttle (game time crawls at low frame rates)
}
function lampNode() {
  if (cache.lamp && cache.lamp.parent) return cache.lamp;
  cache.lamp = ctx.camera && ctx.camera.children ? ctx.camera.children.find(o => o.isPointLight) || null : null;
  return cache.lamp;
}
const N8 = [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [1, -1], [-1, 1], [-1, -1]];
// Mark every non-wall cell within the lamp's reach (2 u when dark) that the player has line of sight to, plus the
// walls bordering it.
function exploreTick() {
  const p = ctx.player, m = ctx.zone.map, id = ctx.zone.id; if (!m || !id) return;
  const bits = exploredBits(id); if (!bits) return;
  const lamp = lampNode();
  const r = p.lampOn && lamp ? Math.min(HUB_CFG.exploreMaxR, lamp.distance || 0) : HUB_CFG.exploreDarkR;
  const pc = toCell(m, p.x, p.z), rc = Math.ceil(r);
  const mark = (cx, cz) => { if (!inBounds(m, cx, cz)) return; const i = idx(m, cx, cz), b = 1 << (i & 7); if (!(bits[i >> 3] & b)) { bits[i >> 3] |= b; exploredDirty = true; } };
  for (let cz = pc.cz - rc; cz <= pc.cz + rc; cz++) for (let cx = pc.cx - rc; cx <= pc.cx + rc; cx++) {
    if (!inBounds(m, cx, cz) || isSolid(m, cx, cz)) continue;
    const c = center(m, cx, cz);
    if (dist2d(c.x, c.z, p.x, p.z) > r) continue;
    if (!(cx === pc.cx && cz === pc.cz) && !los(m, p.x, p.z, c.x, c.z)) continue;
    mark(cx, cz);
    for (const [dx, dz] of N8) if (isSolid(m, cx + dx, cz + dz)) mark(cx + dx, cz + dz);
  }
}
const hasCart = () => !!(save().buildings && save().buildings.cart);
function onMinimapToggle() {
  const cv = ctx.dom && ctx.dom.minimap; if (!cv) return;
  // main.js has already toggled #minimap (ui.toggleMinimap); without the table there is no map to show
  if (!hasCart()) { if (!cv.hidden) { cv.hidden = true; toast("You have no map of this place. Ines could draw one — Cartographer's Table."); emit('uiError', {}); } return; }
  emit('minimap', { on: !cv.hidden });
  if (!cv.hidden) drawMinimap();
}
function toggleMinimap() {
  const cv = ctx.dom && ctx.dom.minimap; if (!cv) return false;
  if (ctx.ui && typeof ctx.ui.toggleMinimap === 'function') ctx.ui.toggleMinimap(); else cv.hidden = !cv.hidden;
  emit('minimap', { on: !cv.hidden });
  if (!cv.hidden) drawMinimap();
  return !cv.hidden;
}
const CELL_COLOR = { [T.FLOOR]: '#2a2a33', [T.DEEP]: '#16161f', [T.WATER]: '#1a2a3a', [T.WALL]: '#555555', [T.PILLAR]: '#666666',
  [T.GATE]: '#8a6a3a', [T.STAIRS]: '#2a2a33', [T.ELEVATOR]: '#2a2a33', [T.ALTAR]: '#3a2a4a' };
const ITEM_COLOR = { oil: '#ffa030', relic: '#60e0ff', rich: '#c070ff', bundle: '#c8c8c8', quest: '#e0d0a0' };
// drawMinimap(): the current map (zone: explored cells only; hub: everything) into #minimap.
export function drawMinimap() {
  const cv = ctx.dom && ctx.dom.minimap; if (!cv || cv.hidden) return false;
  const g = cv.getContext && cv.getContext('2d', { willReadFrequently: true }); if (!g) return false;
  const st = ctx.state, mode = st.mode === 'MENU' ? st.prevMode : st.mode;
  const hubMode = mode === 'HUB' || mode === 'TITLE';
  const m = hubMode ? ctx.hub.map : ctx.zone.map; if (!m) return false;
  const W = cv.width, H = cv.height;
  const px = Math.max(2, Math.min(HUB_CFG.minimapPx, Math.floor(Math.min(W / m.w, H / m.h))));
  const ox = Math.floor((W - m.w * px) / 2), oz = Math.floor((H - m.h * px) / 2);
  const bits = hubMode ? null : exploredBits(ctx.zone.id);
  const seen = (cx, cz) => hubMode || (bits && bitSet(bits, idx(m, cx, cz)));
  const X = (wx) => ox + (wx - m.ox) * px, Z = (wz) => oz + wz * px;
  g.clearRect(0, 0, W, H);
  for (let cz = 0; cz < m.h; cz++) for (let cx = 0; cx < m.w; cx++) {
    if (!seen(cx, cz)) continue;
    const t = m.cells[idx(m, cx, cz)];
    g.fillStyle = CELL_COLOR[t] || '#2a2a33';
    g.fillRect(ox + cx * px, oz + cz * px, px, px);
  }
  const dot = (wx, wz, color, r = px * 0.35) => { g.fillStyle = color; g.beginPath(); g.arc(X(wx), Z(wz), r, 0, Math.PI * 2); g.fill(); };
  const diamond = (wx, wz, color, r = px * 0.6) => { const x = X(wx), z = Z(wz); g.strokeStyle = color; g.lineWidth = 1; g.beginPath(); g.moveTo(x, z - r); g.lineTo(x + r, z); g.lineTo(x, z + r); g.lineTo(x - r, z); g.closePath(); g.stroke(); };
  const tri = (wx, wz, color, ang, r = px * 0.7) => { const x = X(wx), z = Z(wz); g.fillStyle = color; g.beginPath();
    g.moveTo(x + Math.sin(ang) * r, z + Math.cos(ang) * r); g.lineTo(x + Math.sin(ang + 2.5) * r, z + Math.cos(ang + 2.5) * r); g.lineTo(x + Math.sin(ang - 2.5) * r, z + Math.cos(ang - 2.5) * r); g.closePath(); g.fill(); };
  // ▲ the way out (always shown: you came in that way)
  if (m.stairs) tri(m.stairs.x, m.stairs.z, '#6a8aff', Math.PI);
  if (m.altar && seen(m.altar.cx, m.altar.cz)) diamond(m.altar.x, m.altar.z, '#c070ff');
  if (m.flame) dot(m.flame.x, m.flame.z, '#ffb265', px * 0.6);
  if (hubMode) {
    for (const id of BUILD_ORDER) { const r = meshes[id], a = anchorOf(id); if (!r || r.state === 'none' || !a) continue; g.fillStyle = r.state === 'built' ? '#ffd080' : '#5a5040'; g.fillRect(X(a.x) - px * 0.4, Z(a.z) - px * 0.4, px * 0.8, px * 0.8); }
  } else {
    const zid = ctx.zone.id;
    // contract spots (active targets in this zone) + explored spots
    const targets = ctx.contracts && typeof ctx.contracts.targets === 'function' ? ctx.contracts.targets(zid) : [];
    for (const c of targets) if (c.x != null) diamond(c.x + m.ox, c.z, '#ffd080');
    for (const sp of m.spots || []) if (seen(sp.cx, sp.cz) && !targets.some(c => c.cell && c.cell[0] === sp.cx && c.cell[1] === sp.cz)) diamond(sp.x, sp.z, '#8a7a5a');
    for (const it of ctx.items) { const c = toCell(m, it.x, it.z); if (seen(c.cx, c.cz)) dot(it.x, it.z, ITEM_COLOR[it.kind] || '#fff'); }
    for (const l of ctx.lanterns) dot(l.x, l.z, '#ffc070', px * 0.45);
    for (const n of ctx.npcs || []) if (n.where === 'zone' && n.x != null) { const c = toCell(m, n.x, n.z); if (n.state === 'FOLLOW' || seen(c.cx, c.cz)) dot(n.x, n.z, '#b8862a', px * 0.4); }
  }
  const p = ctx.player;
  if (p && p.map === m) tri(p.x, p.z, '#ffffff', p.yaw + Math.PI, px * 0.9);
  return true;
}

/* ============================================================
   HUD: banked resources line (#hubres, created here under #hud)
   ============================================================ */
function ensureHud() {
  if (typeof document === 'undefined') return;
  const hud = (ctx.dom && ctx.dom.hud) || document.getElementById('hud') || document.body;
  if (!document.getElementById('hubstyle')) {
    const st = document.createElement('style'); st.id = 'hubstyle';
    st.textContent = '#hubres{position:absolute;left:16px;bottom:16px;font-size:12px;line-height:1.7;color:#a89878;text-shadow:0 0 4px #000;white-space:pre-line}';
    document.head.appendChild(st);
  }
  hudRes = document.getElementById('hubres');
  if (!hudRes) { hudRes = document.createElement('div'); hudRes.id = 'hubres'; hud.appendChild(hudRes); }
  if (ctx.dom) ctx.dom.hubres = hudRes;
}
function updateHud() {
  if (!hudRes) return;
  const st = ctx.state, s = save(), mode = st.mode === 'MENU' ? st.prevMode : st.mode;
  let text = '';
  if (mode === 'HUB') {
    const parts = [`Banked: ${s.oil | 0} oil · ${s.relics | 0} relic${s.relics === 1 ? '' : 's'} · ${s.rich | 0} rich`];
    parts.push(`Light-tech ${ROMAN[s.lightTech | 0]}${(s.reservoir | 0) ? ` · reservoir +${(s.reservoir | 0) * BUILD_COSTS.reservoirOil}` : ''}${s.buildings && s.buildings.shrine ? ` · blessing ${s.blessing ? 'lit' : 'unlit'}` : ''}`);
    parts.push(`Next descent: ${zoneName(selected())}`);
    text = parts.join('\n');
  } else if (mode === 'ZONE' && run.blessed) text = 'Blessed';
  if (text !== cache.res) { cache.res = text; hudRes.textContent = text; }
}

/* ============================================================
   Per frame
   ============================================================ */
export function update(c, dt) {
  const flame = c.hub.flame, time = c.state.time;
  // flicker: two slow waves plus a fast shimmer (0.80 … 1.0 of the tier intensity)
  const f = 0.9 + 0.05 * Math.sin(time * 13) + 0.035 * Math.sin(time * 7.3 + 1.1) + 0.015 * Math.sin(time * 31);
  flame.light.intensity = flame.baseInt * f;
  flame.model.update(time);   // fire scale pulse + each tongue twists and sways on its own
  const hubMode = c.state.mode === 'HUB' || c.state.mode === 'TITLE' || (c.state.mode === 'MENU' && c.state.prevMode === 'HUB');
  if (hubMode) {
    updateEmbers(c.state.paused ? 0 : dt, time);
    // residents: a small idle — head turns, shoulders breathe, arms sway (npc.js owns the bob and the turn-to-player)
    for (const r of c.npcs) {
      if (r.where !== 'hub' || !r.group) continue;
      const ud = r.group.userData;
      if (!ud._idle) ud._idle = { head: ud.head || null, armL: models.named(r.group, 'armL'), armR: models.named(r.group, 'armR'), coat: models.named(r.group, 'coat') };
      const I = ud._idle, ph = r.phase || 0;
      if (I.head) I.head.rotation.y = 0.14 * Math.sin(time * 0.7 + ph) + 0.05 * Math.sin(time * 2.3 + ph);
      if (I.armL) I.armL.rotation.x = 0.06 * Math.sin(time * 1.1 + ph);
      if (I.armR) I.armR.rotation.x = -0.06 * Math.sin(time * 1.1 + ph + 0.4);
      if (I.coat) I.coat.scale.z = 1 + 0.03 * Math.sin(time * 1.6 + ph);
    }
    colliderT += dt; if (colliderT > 1.0) rebuildColliders();   // residents re-placed by npc.js, marks by endgame
  }
  for (const id of BUILD_ORDER) { const r = meshes[id]; if (r && r.pad) r.pad.material.emissiveIntensity = HUB_CFG.padGlow * (0.9 + 0.1 * Math.sin(time * 9 + r.phase)); }
  const cart = meshes.tram && meshes.tram.group && meshes.tram.group.userData && meshes.tram.group.userData.cart;
  if (cart) cart.position.z = 0.15 * Math.sin(time * 0.5);
  // exploration (ZONE, unpaused) + throttled save mirror
  if (c.state.mode === 'ZONE' && !c.state.paused) {
    exploreT -= dt;
    if (exploreT <= 0) { exploreT = HUB_CFG.exploreTick; exploreTick(); }
  }
  if (exploredDirty) { const now = (typeof performance !== 'undefined' ? performance.now() : Date.now()) / 1000; if (!exploredSaveT) exploredSaveT = now; else if (now - exploredSaveT >= HUB_CFG.exploredSaveT) flushExplored(false); }
  // minimap (throttled) + HUD line
  const cv = c.dom && c.dom.minimap;
  if (cv && !cv.hidden) { minimapT -= dt; if (minimapT <= 0) { minimapT = 1 / HUB_CFG.minimapHz; drawMinimap(); } }
  updateHud();
}
