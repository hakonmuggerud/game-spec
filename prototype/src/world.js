// world.js — scene geometry: instanced blocks, hub, zone loading, per-zone palette/fog, items, planted
// lanterns, sconces, gates (X), shortcuts (=), water (W), elevators (V), collision (DESIGN.md §3–4, DESIGN.md §3.6).
// Talks to other modules only via ctx + events. Listens: flameTier, hubEnter, zoneEnter, zoneExit, toolGained.
// Emits: gateOpened {zoneId, id, cx, cz, idx, tool}, gateLocked {zoneId, cx, cz, tool}, zoneIntro {zoneId, text},
//        shortcutOpened {zoneId, id, name, cx, cz, idx},
//        waterEnter / waterExit {x, z} (player crossing a W boundary — audio "slosh", hunters use player.inWater).
import * as THREE from 'three';
import { CFG, SCONCE_INT, TOOLS, HUB_OX, HUB_WARMTH, HUB_BLOCK } from './config.js';
import * as MAPS from './maps.js';
import * as models from './models.js';

const { T, idx, inBounds, cellType, isSolid, isBlocked, isWater, toCell, center, dist2d, los, bfsField, pathTo, nearestReachable } = MAPS;
export { isSolid, isBlocked, isWater, cellType, toCell, center, los, bfsField, pathTo, nearestReachable, idx, inBounds, dist2d, T };

let ctx = null;
const sconces = [];            // hub alcove sconces {cup, light, phase}
let alcoveTier = 1;
const hubProps = { group: null, list: [], lanterns: [] };   // merged camp props + per-tier hanging-lantern groups
let ambient = null;            // the scene's AmbientLight (colour follows the zone palette)
const water = { mesh: null, cells: [], t: 0 };
const bundles = {};            // zoneId → {x, z, contents}: a death bundle survives zone switches
const atmos = { current: 'hub' };
let wasInWater = false;

// Items are models.js factories (feet-origin): kind → the height they hover at (the bob adds ±0.08).
const ITEM_Y = { oil: 0.12, relic: 0.14, rich: 0.14, bundle: 0.02, quest: 0.1 };
let ringGeo = null;
// Hub alcove sconces (DESIGN-v2 §7): tier 2 lights the W/E alcoves, tier 3 the N corner rooms, tier 4 everything.
// `side` = which way the lamp faces off the wall; positions are for the 25×13 hub (a v1 17×9 map gets the old pair).
const SCONCE_SPOTS_V2 = [
  { cx: 1, cz: 7, side: 1, minTier: 2 }, { cx: 23, cz: 7, side: -1, minTier: 2 },
  { cx: 1, cz: 2, side: 1, minTier: 3 }, { cx: 23, cz: 2, side: -1, minTier: 3 },
];
const SCONCE_SPOTS_V1 = [{ cx: 1, cz: 3, side: 1, minTier: 2 }, { cx: 15, cz: 3, side: -1, minTier: 2 }];
// Camp props for the 25×13 hub (DESIGN.md §3 hub): cell + offset + yaw per models.PROP_BOXES entry. Everything is
// merged into one solid + one glow mesh (two draw calls); tall props become collision cells (map.blockMask).
const faceFlame = (cx, cz, m) => Math.atan2(-(m.flame.x - (m.ox + cx + 0.5)), -(m.flame.z - (cz + 0.5)));
const HUB_PROPS_V2 = [
  // nave: hearth rug, two benches turned to the fire, firewood and a stew pot beside the brazier, bedrolls along the walls
  { model: 'rug', cx: 11, cz: 3, opts: { w: 4.6, d: 2.6 } },
  { model: 'bench', cx: 8, cz: 3, ry: faceFlame }, { model: 'bench', cx: 14, cz: 3, ry: faceFlame },
  { model: 'logPile', cx: 9, cz: 1, dz: -0.12 }, { model: 'cookpot', cx: 13, cz: 1, dz: -0.05 },
  { model: 'bedroll', cx: 7, cz: 1, dx: -0.1, dz: 0.5 }, { model: 'bedroll', cx: 15, cz: 1, dx: 0.1, dz: 0.5, opts: { color: 0x3a5a6a } },
  { model: 'crateStack', cx: 7, cz: 9, dx: -0.05, dz: 0.05 }, { model: 'barrel', cx: 7, cz: 8, dx: -0.15 },
  { model: 'barrel', cx: 6, cz: 11, dx: -0.15, dz: 0.15 }, { model: 'crate', cx: 14, cz: 11, dx: 0.1, dz: 0.1, ry: 0.3 },
  { model: 'candleCluster', cx: 12, cz: 2, dx: 0.35, dz: -0.3 },
  // NW room: the Workshop and the Cartographer's Table — a crate by the bench, bookshelves along the east wall
  { model: 'crate', cx: 5, cz: 1, dx: 0.1, dz: -0.1 },
  { model: 'bookshelf', cx: 5, cz: 3, ry: Math.PI / 2, dx: 0.3 }, { model: 'bookshelf', cx: 5, cz: 4, ry: Math.PI / 2, dx: 0.3 },
  // NE room: the Oil Press and the Shrine — herbs and bottles hung over the press, candles before the shrine, the keeper's bedroll
  { model: 'herbRail', cx: 19, cz: 1, dz: 0.55 }, { model: 'candleCluster', cx: 21, cz: 3, dz: 0.32 },
  { model: 'bedroll', cx: 17, cz: 3, dx: -0.1, dz: 0.5, opts: { color: 0x6a5a30 } }, { model: 'crate', cx: 17, cz: 1, dx: -0.1, dz: -0.1, ry: -0.2 },
  // W alcove (tram): crates at the rail end. E alcove (elevator): stores in the south-west corner
  { model: 'crate', cx: 5, cz: 8, dx: 0.1 }, { model: 'barrel', cx: 5, cz: 9, dx: 0.15, dz: 0.1 },
  { model: 'crateStack', cx: 17, cz: 9, dx: -0.05, dz: 0.05 }, { model: 'barrel', cx: 18, cz: 9, dz: 0.1 },
];
// Hanging lanterns lit from the given flame tier: the stairs/board first, then the alcove mouths, then the corner-room
// doors and the hearth, then the whole camp. `light` ones carry a real PointLight (HUB_WARMTH.lanternLight; five in all,
// culled with the rest of the hub while a zone runs); the others are emissive glass under the flame's own light.
const HUB_LANTERNS_V2 = [
  { cx: 13, cz: 8, tier: 1, light: true },
  { cx: 6, cz: 7, tier: 2, light: true }, { cx: 16, cz: 6, tier: 2, light: true },
  { cx: 1, cz: 5, tier: 3, light: true }, { cx: 23, cz: 5, tier: 3, light: true }, { cx: 8, cz: 5, tier: 3 }, { cx: 14, cz: 5, tier: 3 },
  { cx: 3, cz: 7, tier: 4 }, { cx: 20, cz: 7, tier: 4 }, { cx: 3, cz: 2, tier: 4 }, { cx: 20, cz: 2, tier: 4 }, { cx: 9, cz: 8, tier: 4 },
];

export function init(c) {
  ctx = c;
  const { scene } = ctx;
  scene.background = new THREE.Color(0x000000);
  scene.fog = new THREE.FogExp2(0x000000, 0.11);
  ambient = new THREE.AmbientLight(0x0b0a14, 1);
  scene.add(ambient);
  ringGeo = new THREE.RingGeometry(CFG.poolR - 0.15, CFG.poolR, 40);
  ringGeo.rotateX(-Math.PI / 2);
  buildHub();
  applyAtmosphere('hub');
  const ev = ctx.events;
  ev.on('flameTier', ({ tier }) => setAlcoveTier(tier));
  ev.on('hubEnter', () => { applyAtmosphere('hub'); wasInWater = false; setHubLights(true); });
  ev.on('zoneEnter', ({ zoneId }) => {
    applyAtmosphere(zoneId || (ctx.zone && ctx.zone.id) || 'hub');
    wasInWater = false;
    setHubLights(false);
    const meta = MAPS.ZONES[zoneId];
    if (meta && meta.intro) { ctx.events.emit('zoneIntro', { zoneId, text: meta.intro }); ctx.events.emit('toast', { msg: meta.intro }); }
  });
  ev.on('zoneExit', () => { wasInWater = false; });
  // ctx.world: debug/integration surface (also mirrored onto window.__game.world once main has created it)
  ctx.world = {
    validate: MAPS.validateAll, validateZone: MAPS.validateZone, palettes: MAPS.PALETTES, zoneLocked: MAPS.zoneLocked,
    openGate, gateAt, gateStatus, openShortcut, shortcutAt, shortcutStatus, shortcutSide,
    shortcuts: () => ((ctx.zone && ctx.zone.shortcuts) || []).map(s => ({ id: s.id, name: s.name, cx: s.cx, cz: s.cz, idx: s.idx, openFrom: s.openFrom, open: !!s.open, mesh: !!s.mesh })),
    isWater: (wx, wz) => { const m = ctx.player.map; if (!m) return false; const c = toCell(m, wx, wz); return isWater(m, c.cx, c.cz); },
    waterCells: () => water.cells.length, atmosphere: () => atmos.current, applyAtmosphere,
    spawnItem, removeItem, spawnLantern, removeLantern, setAlcoveTier, alcoveTier: () => alcoveTier, loadZone, unloadZone,
    setHubLights, culledLights: () => culledLights.length,
    // hub camp (DESIGN.md §3 hub): placed props, lantern groups, and the collision mask hub.js/world.js write
    hubProps: () => hubProps.list.map(p => ({ ...p })), hubLanterns: () => hubProps.lanterns.map(l => ({ tier: l.tier, cx: l.cx, cz: l.cz, visible: l.group.visible, light: l.light ? +l.light.intensity.toFixed(2) : null })),
    hubBlocked: (cx, cz) => { const m = ctx.hub.map; return !!(m && m.blockMask && inBounds(m, cx, cz) && m.blockMask[idx(m, cx, cz)]); },
    markBoxCells, hubWarmth: () => ({ ambient: ambient.color.getHex(), fog: ctx.scene.fog.color.getHex(), density: ctx.scene.fog.density }),
  };
  setTimeout(() => { if (window.__game && !window.__game.world) window.__game.world = ctx.world; }, 0);
}

/* ---------- hub light culling ---------- */
// Forward Lambert evaluates every PointLight in the scene for every fragment, wherever it is. The hub sits 60 u
// away (beyond the camera's far plane) while a zone runs, so its flame/sconce/landing lights are switched off on
// zoneEnter and back on at hubEnter. Anything x ≥ HUB_OX − 2 counts as hub-side (hub group, flame, endgame marks).
const culledLights = [];
export function setHubLights(on) {
  if (on) { for (const l of culledLights) l.visible = true; culledLights.length = 0; return; }
  const v = new THREE.Vector3();
  ctx.scene.traverse(o => {
    if (!o.isPointLight || !o.visible) return;
    if (o.parent === ctx.camera) return;                       // the handlamp
    o.getWorldPosition(v);
    if (v.x >= HUB_OX - 2) { o.visible = false; culledLights.push(o); }
  });
}

/* ---------- per-zone atmosphere (fog / ambient / background) ---------- */
export function paletteFor(id) { return MAPS.PALETTES[id] || MAPS.PALETTES.hub; }
export function applyAtmosphere(id) {
  const p = paletteFor(id), scene = ctx.scene;
  atmos.current = MAPS.PALETTES[id] ? id : 'hub';
  scene.fog.color.set(p.fog.color); scene.fog.density = p.fog.density;
  scene.background.set(p.sky);
  ambient.color.set(p.ambient);
  if (atmos.current === 'hub') applyHubWarmth(alcoveTier);
  return atmos.current;
}
// The hub only: ambient and fog warm up with the flame tier (config.HUB_WARMTH); zones keep their palette exactly.
function applyHubWarmth(tier) {
  if (atmos.current !== 'hub') return;
  const i = Math.max(0, Math.min(3, (tier | 0) - 1)), w = HUB_WARMTH;
  ambient.color.set(w.ambient[i]);
  ctx.scene.fog.color.set(w.fog.color[i]); ctx.scene.fog.density = w.fog.density;
}

/* ---------- instanced blocks ---------- */
function buildBlocks(m, group, meta, { hole = false } = {}) {
  const box = (w, h, d) => new THREE.BoxGeometry(w, h, d);
  const pal = meta && meta.palette ? meta.palette : MAPS.PALETTES.hub;
  // blocks are 2% oversize so neighbouring instances overlap: separate boxes sharing an edge leave
  // pixel-sized rasterization cracks at distance (very visible at the 1/3-res pixelated render)
  const S = 1.02;
  const kinds = {
    floor:  { geo: box(S, 0.2, S),     y: -0.1,  color: pal.floor,  list: [] },
    deep:   { geo: box(S, 0.2, S),     y: -0.1,  color: pal.deep,   list: [] },
    water:  { geo: box(S, 0.2, S),     y: -0.25, color: pal.water,  list: [] },   // bed 0.15 below the floor
    wall:   { geo: box(S, 3.4, S),     y: 1.5,   color: pal.wall,   list: [] },
    pillar: { geo: box(0.8, 3.4, 0.8), y: 1.5,   color: pal.pillar, list: [] },
    ceil:   { geo: box(S, 0.2, S),     y: 3.1,   color: pal.ceil,   list: [] },
  };
  const bands = meta && meta.deepStyle === 'bands';
  const lapOf = (x, z) => (bands ? MAPS.lapOf(x, z, m) : 0);
  for (let z = 0; z < m.h; z++) for (let x = 0; x < m.w; x++) {
    const t = m.cells[idx(m, x, z)], p = center(m, x, z), lap = lapOf(x, z);
    if (t === T.WALL) { kinds.wall.list.push({ ...p, lap }); continue; }
    if (t === T.DEEP) {
      // Source bands: lap 0 reads as plain floor; darker per lap (DESIGN-v2 §2)
      if (bands) kinds[lap === 0 ? 'floor' : 'deep'].list.push({ ...p, lap });
      else kinds.deep.list.push(p);
    } else if (t === T.WATER) kinds.water.list.push(p);
    else if (!(hole && t === T.STAIRS)) kinds.floor.list.push({ ...p, lap });   // hub: open stairwell under the S cell
    kinds.ceil.list.push({ ...p, lap });
    if (t === T.PILLAR) kinds.pillar.list.push({ ...p, lap });
  }
  const dummy = new THREE.Object3D(), col = new THREE.Color();
  for (const [name, k] of Object.entries(kinds)) {
    if (!k.list.length) { k.geo.dispose(); continue; }
    const mesh = new THREE.InstancedMesh(k.geo, new THREE.MeshLambertMaterial({ color: 0xffffff }), k.list.length);
    mesh.name = `blocks:${name}`;
    k.list.forEach((p, i) => {
      dummy.position.set(p.x, k.y, p.z); dummy.updateMatrix();
      mesh.setMatrixAt(i, dummy.matrix);
      col.set(k.color).multiplyScalar(1 + (Math.random() * 2 - 1) * 0.06); // ±6% jitter
      // the Source spiral darkens per lap: floor/ceiling/walls lose 12 % per lap, deep pockets bottom out at 30 %
      if (p.lap) col.multiplyScalar(Math.max(0.3, 1 - 0.12 * p.lap));
      mesh.setColorAt(i, col);
    });
    mesh.instanceMatrix.needsUpdate = true;
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
    group.add(mesh);
  }
}

// S / V marker: a flat glowing sill (kept: it is what the player sees under their feet at spawn) plus the
// models.js staircase (zones: rising north = the way up; hub: turned round, low end toward the flame) or the
// elevator cage. Both models are decorative — no collision — and scaled to stay inside their cell.
function buildStairsMarker(m, group, { hub = false } = {}) {
  const p = center(m, m.stairs.cx, m.stairs.cz);
  if (hub) {
    // the way DOWN: a stairwell sunk into the floor (buildBlocks leaves the S cell open), steps dropping south,
    // away from the flame, into a black passage; a cool landing lamp so a tier-1 arrival is not pitch black
    const st = models.stairsDown(); st.position.set(p.x, 0, p.z); st.name = 'stairs'; group.add(st);
    const light = new THREE.PointLight(0x9ab0ff, 0.9, 4.5, 2); light.position.set(p.x, 1.2, p.z - 0.4); group.add(light);
    return;
  }
  const marker = new THREE.Mesh(new THREE.BoxGeometry(0.9, 0.06, 0.9),
    new THREE.MeshLambertMaterial({ color: 0x1a2230, emissive: 0x2a4a9a, emissiveIntensity: 0.55 }));
  marker.position.set(p.x, 0.03, p.z);
  group.add(marker);
  if (m.stairs.kind === 'elevator') {
    // V: the elevator cage (spawn + extraction, same role as S) — DESIGN-v2 §2
    const e = models.elevator(); e.position.set(p.x, 0, p.z); e.name = 'elevator'; group.add(e);
    const light = new THREE.PointLight(0xffc070, 1.2, 5, 2); light.position.set(p.x, e.userData.lightY || 2.4, p.z); group.add(light);
  } else {
    // zones: the way UP, rising north toward the surface
    const st = models.stairs(); st.scale.setScalar(0.8); st.position.set(p.x, 0, p.z); st.name = 'stairs';
    group.add(st);
  }
}

/* ---------- hub ---------- */
export function buildHub() {
  const map = MAPS.parseHub();
  const group = new THREE.Group(); group.name = 'hub';
  buildBlocks(map, group, { palette: MAPS.PALETTES.hub }, { hole: true });
  buildStairsMarker(map, group, { hub: true });
  buildSconces(map, group);
  map.blockMask = new Uint8Array(map.w * map.h);   // HUB_BLOCK bits: props here, buildings/NPCs/flame from hub.js
  buildProps(map, group);
  ctx.scene.add(group);
  ctx.hub.map = map; ctx.hub.group = group;
  return map;
}
/* ---------- hub camp props (merged) + hanging lanterns per tier ---------- */
// markBoxCells(map, bb, bit): set `bit` on every cell the world-space Box3 overlaps once shrunk by HUB_BLOCK.shrink
// per side (a box thinner than that marks its centre cell). Boxes below minTop or above maxBottom are walkable.
export function markBoxCells(m, bb, bit) {
  if (!m || !m.blockMask) return 0;
  if (bb.max.y < HUB_BLOCK.minTop || bb.min.y > HUB_BLOCK.maxBottom) return 0;
  if (bb.max.x - bb.min.x < HUB_BLOCK.minSize && bb.max.z - bb.min.z < HUB_BLOCK.minSize) return 0;   // a post, a leg, a candle
  const sh = HUB_BLOCK.shrink;
  let x0 = bb.min.x + sh, x1 = bb.max.x - sh, z0 = bb.min.z + sh, z1 = bb.max.z - sh;
  if (x1 < x0) x0 = x1 = (bb.min.x + bb.max.x) / 2;
  if (z1 < z0) z0 = z1 = (bb.min.z + bb.max.z) / 2;
  let n = 0;
  for (let cz = Math.floor(z0); cz <= Math.floor(z1 - 1e-6); cz++) for (let cx = Math.floor(x0 - m.ox); cx <= Math.floor(x1 - m.ox - 1e-6); cx++) {
    if (!inBounds(m, cx, cz) || isSolid(m, cx, cz)) continue;
    m.blockMask[idx(m, cx, cz)] |= bit; n++;
  }
  return n;
}
function propBoxesWorld(m, p) {
  const f = models.PROP_BOXES[p.model]; if (!f) return [];
  const x = m.ox + p.cx + 0.5 + (p.dx || 0), z = p.cz + 0.5 + (p.dz || 0);
  const ry = typeof p.ry === 'function' ? p.ry(p.cx, p.cz, m) : (p.ry || 0);
  return models.placeBoxes(f(p.opts || {}), x, z, ry);
}
function buildProps(m, group) {
  if (m.w < 25) return;   // the v1 17×9 hub has no camp
  const bb = new THREE.Box3();
  const cellsOf = (boxes) => {
    const cells = new Set();
    for (const b of boxes) {
      // AABB of the yawed box
      const c = Math.abs(Math.cos(b.ry || 0)), s = Math.abs(Math.sin(b.ry || 0)), hw = (b.w * c + b.d * s) / 2, hd = (b.w * s + b.d * c) / 2;
      bb.min.set(b.x - hw, b.y, b.z - hd); bb.max.set(b.x + hw, b.y + b.h, b.z + hd);
      const before = m.blockMask.slice();
      if (markBoxCells(m, bb, HUB_BLOCK.PROP)) for (let i = 0; i < before.length; i++) if (before[i] !== m.blockMask[i]) cells.add(i);
    }
    return [...cells].map(i => [i % m.w, (i / m.w) | 0]);
  };
  const all = [];
  hubProps.list = [];
  for (const p of HUB_PROPS_V2) {
    if (!inBounds(m, p.cx, p.cz) || isSolid(m, p.cx, p.cz)) continue;
    const boxes = propBoxesWorld(m, p);
    hubProps.list.push({ model: p.model, cx: p.cx, cz: p.cz, boxes: boxes.length, cells: cellsOf(boxes) });
    all.push(...boxes);
  }
  const g = models.mergeBoxes(all, { jitter: 0.05 }); g.name = 'hub:props'; group.add(g); hubProps.group = g;
  // hanging lanterns: one merged group per tier (glass flickers through the group's glow material)
  hubProps.lanterns = [];
  const byTier = {};
  for (const l of HUB_LANTERNS_V2) {
    if (!inBounds(m, l.cx, l.cz) || isSolid(m, l.cx, l.cz)) continue;
    (byTier[l.tier] = byTier[l.tier] || []).push(l);
  }
  for (const [tier, list] of Object.entries(byTier)) {
    const boxes = [];
    for (const l of list) boxes.push(...models.placeBoxes(models.hangLanternBoxes(), m.ox + l.cx + 0.5, l.cz + 0.5, 0));
    const lg = models.mergeBoxes(boxes, { jitter: 0 }); lg.name = `hub:lanterns:${tier}`; lg.visible = false; group.add(lg);
    const glow = lg.getObjectByName('glow');
    for (const l of list) {
      let light = null;
      if (l.light) {
        const L = HUB_WARMTH.lanternLight;
        light = new THREE.PointLight(L.color, 0, L.dist, 2); light.position.set(m.ox + l.cx + 0.5, 2.2, l.cz + 0.5); light.name = `hub:lanternLight:${l.cx},${l.cz}`;
        group.add(light);   // always `visible` (intensity 0 below its tier) so setHubLights culls it like the sconces
      }
      hubProps.lanterns.push({ tier: +tier, cx: l.cx, cz: l.cz, group: lg, glow, light, phase: Math.random() * 6 });
    }
  }
}
// The great flame alone cannot light the side alcoves: 8 u away, inverse-square + Lambert leaves their
// walls at ~6/255, i.e. black. So each alcove holds a wall sconce that catches from the flame as it grows:
// a banked ember at tier 1 (alcoves barely readable, never pitch black), lit progressively from tier 2 (DESIGN §3:
// alcoves emerge = progress).
function buildSconces(m, group) {
  const spots = (m.w >= 25 ? SCONCE_SPOTS_V2 : SCONCE_SPOTS_V1).filter(s => inBounds(m, s.cx, s.cz) && !isSolid(m, s.cx, s.cz));
  for (const { cx, cz, side, minTier } of spots) {
    const p = center(m, cx, cz);
    const g = new THREE.Group(); g.position.set(p.x - side * 0.35, 0, p.z); // against the far wall
    const cup = new THREE.Mesh(new THREE.BoxGeometry(0.22, 0.22, 0.22),
      new THREE.MeshLambertMaterial({ color: 0x1a1210, emissive: 0xff7a30, emissiveIntensity: 0 }));
    cup.position.y = 1.7; g.add(cup);
    const light = new THREE.PointLight(0xffa040, 0, 7, 2); light.position.set(side * 0.3, 1.9, 0); g.add(light);
    group.add(g);
    sconces.push({ cup, light, phase: Math.random() * 6, minTier, int: 0 });
  }
}
// setAlcoveTier(tier): sconce brightness per tier; a sconce stays dark below its alcove's minTier.
export function setAlcoveTier(tier) {
  alcoveTier = tier;
  applyHubWarmth(tier);
  for (const l of hubProps.lanterns) { l.group.visible = tier >= l.tier; if (l.light) l.light.intensity = tier >= l.tier ? HUB_WARMTH.lanternLight.int : 0; }
  for (const s of sconces) {
    const on = tier >= s.minTier;
    s.int = on ? SCONCE_INT[tier - 1] : SCONCE_INT[0];
    s.light.intensity = s.int;
    s.cup.material.emissiveIntensity = on ? 0.3 + 0.2 * (tier - 2) : 0.08;
  }
}

/* ---------- zones ---------- */
export function loadZone(id) {
  const meta = MAPS.ZONES[id];
  if (!meta) throw new Error(`unknown zone "${id}"`);
  unloadZone();
  const map = MAPS.parseZone(id);
  const group = new THREE.Group(); group.name = `zone:${id}`;
  const zone = { id, meta, map, group, spots: map.spots, gates: map.gates, shortcuts: map.shortcuts, npcCells: map.npcCells, npcCell: map.npcCells[0] || null,
    altar: map.altar, exit: map.stairs, palette: meta.palette };
  ctx.zone = zone; ctx.state.zoneId = id;
  // gates already opened in this save stay open (save.gatesOpened[zoneId] holds stable string ids — never cell
  // indices, because the grids change size; a legacy numeric entry is ignored, costing one E press, DESIGN.md §3.6)
  const opened = (ctx.save.gatesOpened && ctx.save.gatesOpened[id]) || [];
  for (const g of map.gates) {
    if (opened.includes(g.id)) { g.open = true; map.cells[g.idx] = T.FLOOR; continue; }
    const p = center(map, g.cx, g.cz);
    g.mesh = models.gate(); g.mesh.position.set(p.x, 0, p.z); g.mesh.name = `gate:${g.cx},${g.cz}`;
    // bars face across the narrower opening: vertical if the wall runs north-south
    if (isSolid(map, g.cx, g.cz - 1) && isSolid(map, g.cx, g.cz + 1)) g.mesh.rotation.y = Math.PI / 2;
    group.add(g.mesh);
  }
  // shortcuts (=): opened ones (save.shortcuts[zoneId] = [id, …]) become floor with the bars raised into the lintel;
  // the rest stay T.SHORTCUT — solid and sight-blocking — behind the barred model (DESIGN.md §3.6)
  const scOpen = (ctx.save.shortcuts && ctx.save.shortcuts[id]) || [];
  for (const s of map.shortcuts) {
    const isOpen = !!s.id && scOpen.includes(s.id);
    if (isOpen) { s.open = true; map.cells[s.idx] = T.FLOOR; }
    addShortcutMesh(map, group, s);
  }
  buildBlocks(map, group, meta);
  if (map.stairs) buildStairsMarker(map, group);
  if (map.altar) { const a = models.altar(); const p = center(map, map.altar.cx, map.altar.cz); a.position.set(p.x, 0, p.z); a.name = 'altar'; group.add(a); }
  buildWater(map, group, meta);
  ctx.scene.add(group);
  if (bundles[id]) { const b = bundles[id]; spawnItem('bundle', b.x, b.z, b.contents); }
  return zone;
}
export function unloadZone() {
  const z = ctx.zone;
  if (!z || !z.group) return;
  clearLanterns();
  for (const it of ctx.items.slice()) disposeItem(it);   // meshes only: a bundle is respawned when the zone reloads
  ctx.scene.remove(z.group);
  z.group.traverse(o => { if (o.isMesh) { if (o.geometry) o.geometry.dispose(); if (o.material && !o.material._shared) o.material.dispose(); } });
  water.mesh = null; water.cells = [];
  ctx.zone = { id: null, meta: null, map: null, group: null, spots: [], gates: [], shortcuts: [], npcCells: [], npcCell: null, altar: null, exit: null, palette: null };
  ctx.state.zoneId = null;
}

/* ---------- water (W): translucent animated surface over a sunken bed ---------- */
// One merged sheet with shared vertices (not per-cell instances): neighbouring tiles neither overlap nor
// crack, so the surface reads as a continuous rippling skin. Vertices bob ±0.02 by sin(t·2 + x + z).
function buildWater(map, group, meta) {
  const cells = [];
  for (let z = 0; z < map.h; z++) for (let x = 0; x < map.w; x++) if (map.cells[idx(map, x, z)] === T.WATER) cells.push({ ...center(map, x, z), cx: x, cz: z });
  if (!cells.length) return;
  const pal = (meta && meta.palette) || MAPS.PALETTES.hub;
  const { mat } = models.water();
  mat.color.set(pal.waterSurface); mat.emissive.set(pal.waterGlow);
  mat.transparent = true; mat.opacity = 0.72; mat.depthWrite = false; mat.side = THREE.DoubleSide;
  const vmap = new Map(), pos = [], index = [];
  const vid = (x, z) => { const k = `${x},${z}`; let i = vmap.get(k); if (i === undefined) { i = pos.length / 3; vmap.set(k, i); pos.push(x, -0.1, z); } return i; };
  for (const c of cells) {
    const x0 = c.x - 0.5, z0 = c.z - 0.5;
    const a = vid(x0, z0), b = vid(x0 + 1, z0), d = vid(x0 + 1, z0 + 1), e = vid(x0, z0 + 1);
    index.push(a, e, d, a, d, b);
  }
  const geo = new THREE.BufferGeometry();
  const attr = new THREE.Float32BufferAttribute(pos, 3); attr.setUsage(THREE.DynamicDrawUsage);
  geo.setAttribute('position', attr); geo.setIndex(index); geo.computeVertexNormals();
  const mesh = new THREE.Mesh(geo, mat); mesh.name = 'water'; mesh.frustumCulled = false;
  group.add(mesh);
  water.mesh = mesh; water.cells = cells; water.t = 0;
}
function updateWater(time) {
  const mesh = water.mesh; if (!mesh) return;
  mesh.material.emissiveIntensity = 0.25 + 0.1 * Math.sin(time * 1.7);
  const a = mesh.geometry.attributes.position, p = a.array;
  for (let i = 0; i < p.length; i += 3) p[i + 1] = -0.1 + 0.02 * Math.sin(time * 2 + p[i] + p[i + 2]);
  a.needsUpdate = true;
}

/* ---------- gates (X) ---------- */
// gateAt(cx, cz) → the gate record at that cell (open or not) or null.
export function gateAt(cx, cz) { const z = ctx.zone; return (z && z.gates && z.gates.find(g => g.cx === cx && g.cz === cz)) || null; }
// gateStatus(g) → {locked, tool, toolName, open}: locked = closed and the save lacks the zone's gate tool.
export function gateStatus(g) {
  const z = ctx.zone, tool = z && z.meta && z.meta.gate ? z.meta.gate.tool : null;
  const has = !!(tool && ctx.save.tools && ctx.save.tools[tool]);
  return { open: !!(g && g.open), locked: !!g && !g.open && !has, tool, toolName: tool ? (TOOLS[tool] || tool) : null };
}
// openGate(cx, cz, {force}) → true when it opened. Without `force` the save must hold the zone's gate tool
// (a refusal emits gateLocked). Opening removes the bars for the run, turns the cell to floor and records it
// in save.gatesOpened[zoneId] so it stays open; emits gateOpened (hunter.js drops its paths).
export function openGate(cx, cz, { force = false } = {}) {
  const z = ctx.zone; if (!z || !z.map) return false;
  const g = z.gates.find(g => g.cx === cx && g.cz === cz && !g.open);
  if (!g) return false;
  const st = gateStatus(g);
  if (st.locked && !force) { ctx.events.emit('gateLocked', { zoneId: z.id, cx, cz, tool: st.tool, toolName: st.toolName }); return false; }
  g.open = true; z.map.cells[g.idx] = T.FLOOR;
  if (g.mesh) { z.group.remove(g.mesh); g.mesh = null; }
  const so = ctx.save.gatesOpened || (ctx.save.gatesOpened = {});
  const list = so[z.id] || (so[z.id] = []);
  const gid = g.id || st.tool || 'gate';
  if (!list.includes(gid)) list.push(gid);
  ctx.events.emit('gateOpened', { zoneId: z.id, id: gid, cx, cz, idx: g.idx, tool: st.tool });
  return true;
}

/* ---------- shortcuts (=) — DESIGN.md §3.6 ---------- */
// A shortcut is one (or two adjacent) barred cells in a wall line. Barred = solid + sight-blocking (maps.isSolid);
// `E` from the `openFrom` side only lifts the bars for good. No tool, no corridor: it removes a detour.
const SHORTCUT_YAW = { N: 0, S: Math.PI, E: -Math.PI / 2, W: Math.PI / 2 };   // model front (-Z) faces the openFrom side
function addShortcutMesh(map, group, s) {
  const p = center(map, s.cx, s.cz);
  s.mesh = s.open ? models.shortcutOpen() : models.shortcutBarred();
  s.mesh.position.set(p.x, 0, p.z);
  s.mesh.rotation.y = SHORTCUT_YAW[s.openFrom] != null ? SHORTCUT_YAW[s.openFrom] : 0;
  s.mesh.name = `shortcut:${s.id || '?'}:${s.cx},${s.cz}`;
  group.add(s.mesh);
}
function disposeMesh(group, mesh) {
  if (!mesh) return;
  group.remove(mesh);
  mesh.traverse(o => { if (o.isMesh) { if (o.geometry) o.geometry.dispose(); if (o.material && !o.material._shared) o.material.dispose(); } });
}
// shortcutAt(cx, cz) → the shortcut record at that cell (open or not) or null.
export function shortcutAt(cx, cz) { const z = ctx.zone; return (z && z.shortcuts && z.shortcuts.find(s => s.cx === cx && s.cz === cz)) || null; }
// shortcutSide(s, wx, wz) → 'far' when (wx, wz) is on the `openFrom` side of the door, else 'near' (the barred side).
export function shortcutSide(s, wx, wz) {
  const m = ctx.zone.map; if (!m || !s) return 'near';
  const p = center(m, s.cx, s.cz);
  const d = SHORTCUT_YAW[s.openFrom] == null ? 0
    : s.openFrom === 'N' ? p.z - wz : s.openFrom === 'S' ? wz - p.z : s.openFrom === 'E' ? wx - p.x : p.x - wx;
  return d > 0 ? 'far' : 'near';
}
// shortcutStatus(s, wx, wz) → {open, id, name, openFrom, side, canOpen}.
export function shortcutStatus(s, wx, wz) {
  const side = s ? shortcutSide(s, wx, wz) : 'near';
  return { open: !!(s && s.open), id: s ? s.id : null, name: s ? s.name : null, openFrom: s ? s.openFrom : null, side, canOpen: !!s && !s.open && side === 'far' };
}
// openShortcut(cx, cz, {force}) → true when it opened. Refuses from the barred side unless `force`. Opening turns
// every cell of the door group to floor, swaps the barred model for the open one, records the id in
// save.shortcuts[zoneId] (so it stays open for good) and emits shortcutOpened (hunter/npc drop their paths).
export function openShortcut(cx, cz, { force = false, from = null } = {}) {
  const z = ctx.zone; if (!z || !z.map) return false;
  const s = z.shortcuts.find(s => s.cx === cx && s.cz === cz && !s.open);
  if (!s) return false;
  if (!force) {
    const p = from || ctx.player;
    if (shortcutSide(s, p.x, p.z) !== 'far') return false;
  }
  const group = s.id ? z.shortcuts.filter(o => o.id === s.id) : [s];
  for (const o of group) {
    o.open = true; z.map.cells[o.idx] = T.FLOOR;
    disposeMesh(z.group, o.mesh); o.mesh = null;
    addShortcutMesh(z.map, z.group, o);
  }
  if (s.id) {
    const so = ctx.save.shortcuts || (ctx.save.shortcuts = {});
    const list = so[z.id] || (so[z.id] = []);
    if (!list.includes(s.id)) list.push(s.id);
  }
  ctx.events.emit('shortcutOpened', { zoneId: z.id, id: s.id, name: s.name, cx: s.cx, cz: s.cz, idx: s.idx });
  return true;
}

/* ---------- items ---------- */
// spawnItem(kind, x, z, contents) → item record {kind, x, z, mesh, baseY, phase, contents}. kind ∈ oil | relic | rich |
// bundle | quest (models.js registry names; `oil` → flask, `rich` → richRelic). Meshes are feet-origin voxel groups.
export function spawnItem(kind, x, z, contents) {
  const mesh = models.makeModel(kind);
  const baseY = ITEM_Y[kind] != null ? ITEM_Y[kind] : 0.1;
  mesh.position.set(x, baseY, z);
  mesh.name = `item:${kind}`;
  ctx.scene.add(mesh);
  const it = { kind, x, z, mesh, baseY, phase: Math.random() * 6.28, contents: contents || null };
  ctx.items.push(it);
  if (kind === 'bundle' && ctx.zone.id) bundles[ctx.zone.id] = { x, z, contents: { ...contents } };
  return it;
}
function disposeItem(it) {
  if (it.mesh) { if (it.mesh.isGroup || it.mesh.userData.model) models.disposeModel(it.mesh); else { ctx.scene.remove(it.mesh); if (it.mesh.material) it.mesh.material.dispose(); } }
  const i = ctx.items.indexOf(it); if (i >= 0) ctx.items.splice(i, 1);
}
// removeItem: picked up / replaced — a removed bundle is also forgotten for this zone.
export function removeItem(it) {
  disposeItem(it);
  if (it.kind === 'bundle' && ctx.zone.id && bundles[ctx.zone.id] && bundles[ctx.zone.id].x === it.x && bundles[ctx.zone.id].z === it.z) delete bundles[ctx.zone.id];
}
export function resetItems() {
  for (const it of ctx.items.slice()) if (it.kind !== 'bundle') removeItem(it);
  const m = ctx.zone.map;
  for (const d of m.items) { const p = center(m, d.cx, d.cz); spawnItem(d.kind, p.x, p.z); }
}

/* ---------- planted lanterns (meshes only; cost/cooldown/events live in main.js) ---------- */
export function spawnLantern(x, z) {
  const g = models.lantern(); g.position.set(x, 0, z); g.name = 'lantern';
  const ring = new THREE.Mesh(ringGeo, new THREE.MeshLambertMaterial({
    color: 0x000000, emissive: 0xffc070, emissiveIntensity: 0.6, side: THREE.DoubleSide }));
  ring.position.y = 0.02; ring.name = 'poolRing'; g.add(ring);
  const light = new THREE.PointLight(0xffc070, 2.0, 6, 2); light.position.y = g.userData.lightY || 1.28; g.add(light);
  ctx.scene.add(g);
  const l = { x, z, group: g, light, glass: g.userData.glass || null, phase: Math.random() * 6 };
  ctx.lanterns.push(l);
  return l;
}
export function removeLantern(l) {
  models.disposeModel(l.group);   // cloned materials + the pool ring's own material; ringGeo is shared and kept
  const i = ctx.lanterns.indexOf(l); if (i >= 0) ctx.lanterns.splice(i, 1);
}
export function clearLanterns() { while (ctx.lanterns.length) removeLantern(ctx.lanterns[0]); }

/* ---------- collision ---------- */
// Hub camp cells (map.blockMask: props, buildings, NPCs, the brazier) are solid for the player too.
function boxBlocked(m, x, z) {
  const r = CFG.radius;
  for (const [dx, dz] of [[-r, -r], [r, -r], [-r, r], [r, r]]) {
    const c = toCell(m, x + dx, z + dz);
    if (isSolid(m, c.cx, c.cz)) return true;
    if (m.blockMask && m.blockMask[idx(m, c.cx, c.cz)]) return true;
  }
  return false;
}
// Move p (needs x, z, map) by dx then dz, cancelling any axis step that would overlap a solid cell.
// Water (W) and open gates are walkable; closed gates (X) are solid (isSolid), so no special case here.
export function moveWithCollision(p, dx, dz) {
  if (dx) { const nx = p.x + dx; if (!boxBlocked(p.map, nx, p.z)) p.x = nx; }
  if (dz) { const nz = p.z + dz; if (!boxBlocked(p.map, p.x, nz)) p.z = nz; }
  return p;
}

/* ---------- per-frame ---------- */
export function update(c, dt) {
  const time = c.state.time;
  for (const it of c.items) {
    it.mesh.position.y = it.baseY + 0.08 * Math.sin(time * 2 + it.phase);
    it.mesh.rotation.y += dt * 0.8;
  }
  if (c.state.mode === 'ZONE' && !c.state.paused) for (const l of c.lanterns) {
    const f = 0.93 + 0.07 * Math.sin(time * 13 + l.phase);
    l.light.intensity = 2.0 * f;
    if (l.glass) l.glass.material.emissiveIntensity = f;
  }
  for (const s of sconces) s.light.intensity = s.int * (0.93 + 0.07 * Math.sin(time * 11 + s.phase));
  for (const l of hubProps.lanterns) {
    if (!l.group.visible) continue;
    const f = 0.9 + 0.08 * Math.sin(time * 9 + l.phase) + 0.02 * Math.sin(time * 23 + l.phase);
    if (l.glow) l.glow.material.color.setScalar(HUB_WARMTH.lanternGlow * f);
    if (l.light && l.light.intensity > 0) l.light.intensity = HUB_WARMTH.lanternLight.int * f;
  }
  updateWater(time);
  // water boundary events (main sets player.inWater from the cell type each frame)
  if (c.state.mode === 'ZONE') {
    const inW = !!c.player.inWater;
    if (inW !== wasInWater) { c.events.emit(inW ? 'waterEnter' : 'waterExit', { x: c.player.x, z: c.player.z }); wasInWater = inW; }
  }
}
