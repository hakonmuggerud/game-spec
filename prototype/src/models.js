// models.js — voxel-box model factories (DESIGN-v2 §6). Imports only THREE; any module may import this file.
//
// Conventions (every factory):
//   • returns a THREE.Group whose origin is at the feet/base (y = 0 is the floor) and whose front faces -Z
//     (the same convention as the player and hunter yaw: forward = (-sin yaw, -cos yaw), rotation.y = yaw);
//   • is built from BoxGeometry boxes with flat MeshLambertMaterial colours, optional emissive parts;
//   • ≤ 30 boxes (see `boxCount(group)`), `group.userData` carries: `model` (name), `boxes` (count),
//     `height`, and any live handles a caller needs (`eyes[]`, `glass`, `fire`, `papers[]`, `update(t, dt)` …).
//
// Helpers: box() → plain description; emissive() marks glow; build() → Group (geometry cached by size,
// material by colour); pattern() → box list from ASCII layers. Registry: MODELS[name](opts) and
// makeModel(name, opts). Materials are cloned per mesh by default (build({shared:false})) so callers may
// tweak / dispose them freely; cached geometries survive dispose() (three re-uploads on next render).
import * as THREE from 'three';

const geoCache = new Map(), matCache = new Map();
function geom(w, h, d) {
  const k = `${w}|${h}|${d}`;
  let g = geoCache.get(k);
  if (!g) { g = new THREE.BoxGeometry(w, h, d); geoCache.set(k, g); }
  return g;
}
function material(color, emissive, k) {
  const key = `${color}|${emissive == null ? '' : emissive}|${k || 0}`;
  let m = matCache.get(key);
  if (!m) {
    m = emissive == null
      ? new THREE.MeshLambertMaterial({ color })
      : new THREE.MeshLambertMaterial({ color, emissive, emissiveIntensity: k });
    m._shared = true; // world.js skips disposing materials flagged _shared
    matCache.set(key, m);
  }
  return m;
}

// box(x, y, z, w, h, d, color): centred on x/z, y = bottom. Optional 8th arg: name (set on the mesh).
export const box = (x, y, z, w, h, d, color, name) => ({ x, y, z, w, h, d, color, emissive: null, k: 0, name: name || null, ry: 0 });
// emissive(box, color, k): mark a box as glowing (returns the same box). Emissive boxes get colour 0x000000
// unless the box colour was set explicitly to something other than the emissive colour.
export const emissive = (b, color, k = 1) => { b.emissive = color; b.k = k; return b; };
// rotated(box, ry): yaw the box (radians) around its own centre (returns the same box).
export const rotated = (b, ry) => { b.ry = ry; return b; };

// build(boxes, {scale=1, jitter=0.06, shared=false}) → THREE.Group. Materials come from the cache
// (jitter is applied as a per-mesh colour copy only when jitter > 0 and shared=false).
export function build(boxes, { scale = 1, jitter = 0.06, shared = false } = {}) {
  const g = new THREE.Group();
  for (const b of boxes) {
    let mat = material(b.color, b.emissive, b.k);
    if (!shared && jitter > 0 && b.emissive == null) {
      mat = mat.clone(); mat._shared = false; mat.color.multiplyScalar(1 + (Math.random() * 2 - 1) * jitter);
    } else if (!shared) { mat = mat.clone(); mat._shared = false; }
    const m = new THREE.Mesh(geom(b.w, b.h, b.d), mat);
    m.position.set(b.x, b.y + b.h / 2, b.z);
    if (b.ry) m.rotation.y = b.ry;
    if (b.name) m.name = b.name;
    g.add(m);
  }
  if (scale !== 1) g.scale.setScalar(scale);
  return g;
}

// pattern(layers, palette, cell): layers[y] is an array of strings (z rows, x chars); '.' = empty.
// Greedy x-run merge → box list. palette: char → colour (or {color, emissive, k}).
export function pattern(layers, palette, cell = 0.1) {
  const out = [];
  layers.forEach((rows, y) => {
    const h = rows.length, w = Math.max(...rows.map(r => r.length));
    rows.forEach((row, z) => {
      let x = 0;
      while (x < w) {
        const ch = row[x];
        if (!ch || ch === '.' || !(ch in palette)) { x++; continue; }
        let run = 1; while (row[x + run] === ch) run++;
        const p = palette[ch], color = typeof p === 'object' ? p.color : p;
        const b = box((x + run / 2 - w / 2) * cell, y * cell, (z + 0.5 - h / 2) * cell, run * cell, cell, cell, color);
        if (typeof p === 'object' && p.emissive != null) emissive(b, p.emissive, p.k == null ? 1 : p.k);
        out.push(b);
        x += run;
      }
    });
  });
  return out;
}

/* ---------- small utilities ---------- */
// boxCount(group): number of box meshes (instanced water tiles count as 1).
export function boxCount(group) { let n = 0; group.traverse(o => { if (o.isMesh) n++; }); return n; }
// named(group, name): first mesh with that name, or null.
export function named(group, name) { let r = null; group.traverse(o => { if (!r && o.name === name) r = o; }); return r; }
// disposeModel(group): dispose per-mesh (cloned) materials; cached geometries/materials are left alone.
export function disposeModel(group) {
  group.traverse(o => { if (o.isMesh && o.material && !o.material._shared) o.material.dispose(); });
  if (group.parent) group.parent.remove(group);
}
// finish(group, name, extra): tag userData (model, boxes, height) and merge live handles.
function finish(group, name, extra) {
  const bb = new THREE.Box3().setFromObject(group);
  Object.assign(group.userData, { model: name, boxes: boxCount(group), height: +((bb.max.y - bb.min.y) || 0).toFixed(3) }, extra || {});
  group.name = group.name || name;
  return group;
}
const glow = (x, y, z, w, h, d, color, k = 1, name) => emissive(box(x, y, z, w, h, d, 0x000000, name), color, k);

/* ============================================================
   Creatures
   ============================================================ */
// hunter({profile:'base'|'fast', eyeColor, scaleY}) — tall hunched figure, 11 boxes, height 1.8 (×1.15 fast).
// userData.eyes = [left, right] (each has its own material: set eyes[i].material.emissiveIntensity per state).
// The upper body is a sub-group pivoted at the hips and tilted −0.25 rad so the head leans forward (−Z).
export function hunter(opts = {}) {
  const profile = typeof opts === 'string' ? opts : (opts.profile || 'base');
  const fast = profile === 'fast';
  const eyeColor = opts.eyeColor != null ? opts.eyeColor : (fast ? 0xffa020 : 0xff3a20);
  const scaleY = opts.scaleY != null ? opts.scaleY : (fast ? 1.15 : 1.0);
  const eyeK = opts.eyeIntensity != null ? opts.eyeIntensity : 0.4;
  const skin = 0x08080a, bone = 0x16161c;
  const g = new THREE.Group();
  // legs (stay vertical)
  g.add(build([box(-0.13, 0, 0, 0.16, 0.6, 0.16, skin), box(0.13, 0, 0, 0.16, 0.6, 0.16, skin)], { jitter: 0 }));
  // upper body: pivot at the hips (y 0.6), tilted forward
  const up = new THREE.Group(); up.position.y = 0.6; up.rotation.x = -0.25;
  const ub = build([
    box(0, 0, 0, 0.5, 0.9, 0.35, skin, 'torso'),
    box(0, 0.55, -0.05, 0.58, 0.14, 0.4, bone, 'shoulders'),      // shoulder ridge: the hunch silhouette
    box(0, 0.85, -0.1, 0.35, 0.35, 0.35, skin, 'head'),
    box(-0.35, -0.3, 0, 0.14, 1.1, 0.14, skin, 'armL'), box(0.35, -0.3, 0, 0.14, 1.1, 0.14, skin, 'armR'),
    box(-0.35, -0.5, -0.02, 0.16, 0.2, 0.18, bone, 'handL'), box(0.35, -0.5, -0.02, 0.16, 0.2, 0.18, bone, 'handR'),
    glow(-0.08, 0.95, -0.29, 0.08, 0.05, 0.05, eyeColor, eyeK, 'eyeL'),
    glow(0.08, 0.95, -0.29, 0.08, 0.05, 0.05, eyeColor, eyeK, 'eyeR'),
  ], { jitter: 0 });
  up.add(ub); g.add(up);
  g.scale.y = scaleY;
  return finish(g, 'hunter', { eyes: [named(ub, 'eyeL'), named(ub, 'eyeR')], upper: up, profile });
}

// NPC looks (DESIGN-v2 §3): id → hat/coat colours.
export const NPC_LOOKS = {
  lamplighter:  { hat: 'stovepipe', hatColor: 0x2a2a2a, coat: 0xb8862a, lamp: true },
  cartographer: { hat: 'flat',      hatColor: 0x3a5a7a, coat: 0x6a8ab0 },
  keeper:       { hat: 'scarf',     hatColor: 0x7a3a2a, coat: 0x5a4a3a, apron: 0xc8b070 },
  deacon:       { hat: 'hood',      hatColor: 0x4a3a6a, coat: 0xe0d8c0 },
};
// npc(id | {id, coat, hatColor}) — 0.44 wide, ~1.3 tall (1.6 with the stovepipe); ≤ 13 boxes.
// userData.lamp = the glowing hand-lamp mesh (Wick only), userData.head = head mesh.
export function npc(opts = {}) {
  const id = typeof opts === 'string' ? opts : (opts.id || 'lamplighter');
  const L = Object.assign({}, NPC_LOOKS[id] || NPC_LOOKS.lamplighter, typeof opts === 'object' ? opts : {});
  const skin = 0xd9b08c, boot = 0x2a2018, coat = L.coat, dark = 0x1a1410;
  const b = [
    box(-0.11, 0, 0.02, 0.14, 0.16, 0.2, boot), box(0.11, 0, 0.02, 0.14, 0.16, 0.2, boot),
    box(0, 0.14, 0, 0.44, 0.78, 0.3, coat, 'coat'),
    box(0, 0.95, 0, 0.3, 0.3, 0.3, skin, 'head'),
    box(-0.05, 1.11, -0.155, 0.04, 0.04, 0.02, dark), box(0.05, 1.11, -0.155, 0.04, 0.04, 0.02, dark), // eyes
    box(-0.28, 0.3, 0, 0.12, 0.55, 0.12, coat, 'armL'), box(0.28, 0.3, 0, 0.12, 0.55, 0.12, coat, 'armR'),
    box(-0.28, 0.24, 0, 0.11, 0.08, 0.11, skin), box(0.28, 0.24, 0, 0.11, 0.08, 0.11, skin),        // hands
  ];
  const hc = L.hatColor;
  switch (L.hat) {
    case 'stovepipe': b.push(box(0, 1.25, 0, 0.42, 0.03, 0.42, hc, 'brim'), box(0, 1.28, 0, 0.28, 0.35, 0.28, hc, 'hat'), box(0, 1.3, 0, 0.3, 0.06, 0.3, 0xb8862a, 'band')); break;
    case 'flat': b.push(box(0, 1.25, 0, 0.5, 0.06, 0.5, hc, 'brim'), box(0, 1.31, 0, 0.28, 0.1, 0.28, hc, 'hat')); break;
    case 'scarf': b.push(box(0, 1.22, 0, 0.32, 0.12, 0.32, hc, 'hat'), box(0, 1.0, 0.17, 0.2, 0.28, 0.06, hc, 'tail')); break;
    case 'hood': b.push(box(0, 1.25, 0, 0.36, 0.1, 0.36, hc, 'hat'), box(0, 0.93, 0.08, 0.36, 0.34, 0.22, hc, 'hood'),
      box(-0.17, 0.93, -0.08, 0.03, 0.34, 0.14, hc), box(0.17, 0.93, -0.08, 0.03, 0.34, 0.14, hc)); break;
    default: b.push(box(0, 1.25, 0, 0.32, 0.08, 0.32, hc, 'hat'));
  }
  if (L.apron) b.push(box(0, 0.16, -0.16, 0.3, 0.62, 0.03, L.apron, 'apron'));
  if (L.lamp) b.push(glow(0.28, 0.1, -0.06, 0.1, 0.1, 0.1, 0xffb265, 1, 'lamp'));
  const g = build(b, { jitter: 0.03 });
  return finish(g, 'npc', { id, head: named(g, 'head'), lamp: named(g, 'lamp') });
}

/* ============================================================
   Items
   ============================================================ */
// flask() — 0.22 wide, 0.43 tall, 5 boxes; userData.center = 0.2 (bob around this y).
export function flask() {
  const g = build([
    emissive(box(0, 0, 0, 0.22, 0.28, 0.22, 0xffa030, 'body'), 0xffa030, 0.5),
    box(0, 0.12, 0, 0.25, 0.04, 0.25, 0x6a4a2a),                  // strap band
    emissive(box(0, 0.28, 0, 0.1, 0.1, 0.1, 0xffa030), 0xffa030, 0.5),
    box(0, 0.38, 0, 0.14, 0.05, 0.14, 0x6a4a2a, 'cork'),
    box(0.1, 0.16, 0, 0.06, 0.16, 0.04, 0x6a4a2a),                // handle
  ], { jitter: 0 });
  return finish(g, 'flask', { center: 0.2 });
}
// relic() — 3 stacked boxes 0.12/0.28/0.12 wide, cyan glow; 3 boxes, 0.4 tall.
export function relic() {
  const c = 0x60e0ff;
  const g = build([
    emissive(box(0, 0, 0, 0.12, 0.1, 0.12, c), c, 0.6),
    emissive(rotated(box(0, 0.1, 0, 0.28, 0.2, 0.28, c), Math.PI / 4), c, 0.6),
    emissive(box(0, 0.3, 0, 0.12, 0.1, 0.12, c), c, 0.6),
  ], { jitter: 0 });
  return finish(g, 'relic', { center: 0.2 });
}
// richRelic() — 0.16/0.4/0.16 stack, violet glow, plus 4 tiny corner boxes; 7 boxes, 0.5 tall.
export function richRelic() {
  const c = 0xc070ff, b = [
    emissive(box(0, 0, 0, 0.16, 0.14, 0.16, c), c, 0.9),
    emissive(rotated(box(0, 0.14, 0, 0.4, 0.22, 0.4, c), Math.PI / 4), c, 0.9),
    emissive(box(0, 0.36, 0, 0.16, 0.14, 0.16, c), c, 0.9),
  ];
  for (const [x, z] of [[-0.22, -0.22], [0.22, -0.22], [-0.22, 0.22], [0.22, 0.22]]) b.push(emissive(box(x, 0.22, z, 0.06, 0.06, 0.06, 0xf0d0ff), 0xf0d0ff, 1));
  return finish(build(b, { jitter: 0 }), 'richRelic', { center: 0.25 });
}
// quest() — a rolled chart / ledger (contract "recover" items): 4 boxes.
export function quest() {
  const c = 0xe0d0a0;
  const g = build([
    emissive(rotated(box(0, 0.05, 0, 0.44, 0.12, 0.12, c), 0.3), c, 0.4),
    box(-0.2, 0.03, 0.05, 0.05, 0.16, 0.16, 0x8a2a2a), box(0.2, 0.03, -0.07, 0.05, 0.16, 0.16, 0x8a2a2a),
    box(0, 0.0, 0, 0.3, 0.05, 0.3, 0x3a2a1a),
  ], { jitter: 0 });
  return finish(g, 'quest', { center: 0.1 });
}
// bundle() — the death bundle: grey sack with a tie and spilled glints; 6 boxes, 0.5 tall.
export function bundle() {
  const g = build([
    emissive(box(0, 0, 0, 0.45, 0.4, 0.45, 0x9a9a9a, 'sack'), 0x9a9a9a, 0.5),
    box(0, 0.4, 0, 0.2, 0.12, 0.2, 0x6a6a6a, 'neck'),
    box(0, 0.36, 0, 0.26, 0.05, 0.26, 0x4a3a2a, 'tie'),
    emissive(box(0.24, 0, 0.18, 0.1, 0.1, 0.1, 0x60e0ff), 0x60e0ff, 0.6),
    emissive(box(-0.26, 0, 0.1, 0.08, 0.12, 0.08, 0xffa030), 0xffa030, 0.5),
    emissive(box(0.05, 0, -0.28, 0.1, 0.08, 0.1, 0xc070ff), 0xc070ff, 0.9),
  ], { jitter: 0 });
  return finish(g, 'bundle', { center: 0.25 });
}
// lantern() — planted lantern: pole, foot, 4-post cage, glowing glass, cap, hook; 12 boxes, 1.6 tall.
// userData.glass = the emissive mesh (flicker its emissiveIntensity), userData.lightY = 1.28.
export function lantern() {
  const wood = 0x3a2a1a, iron = 0x2a2420, b = [
    box(0, 0, 0, 0.3, 0.05, 0.3, wood, 'foot'),
    box(0, 0.05, 0, 0.06, 1.05, 0.06, wood, 'pole'),
    box(0, 1.08, 0, 0.3, 0.04, 0.3, iron, 'tray'),
  ];
  for (const [x, z] of [[-0.12, -0.12], [0.12, -0.12], [-0.12, 0.12], [0.12, 0.12]]) b.push(box(x, 1.1, z, 0.04, 0.36, 0.04, iron));
  b.push(emissive(box(0, 1.14, 0, 0.22, 0.26, 0.22, 0xffc070, 'glass'), 0xffc070, 1.0),
    box(0, 1.46, 0, 0.3, 0.06, 0.3, iron, 'cap'), box(0, 1.52, 0, 0.12, 0.05, 0.12, iron),
    box(0, 1.57, 0, 0.04, 0.08, 0.04, iron, 'hook'), box(0.08, 0.85, 0, 0.14, 0.04, 0.04, iron));
  const g = build(b, { jitter: 0.03 });
  return finish(g, 'lantern', { glass: named(g, 'glass'), lightY: 1.28 });
}

/* ============================================================
   Zone furniture
   ============================================================ */
// stairs() — 5 steps rising toward −Z (the way up), side walls, a landing and a cold-blue rune; 10 boxes.
// 1.6 u wide × 1.7 u deep: decorative (no collision); the player stands on it at spawn.
export function stairs() {
  const stone = 0x3a3a44, wall = 0x30303a, b = [];
  for (let i = 0; i < 5; i++) b.push(box(0, 0, 0.4 - 0.2 * i, 1.6, 0.12 * (i + 1), 0.3, stone, `step${i}`));
  b.push(box(0, 0, -0.7, 1.6, 0.66, 0.3, stone, 'landing'));
  b.push(box(-0.85, 0, -0.1, 0.1, 0.9, 1.5, wall, 'wallL'), box(0.85, 0, -0.1, 0.1, 0.9, 1.5, wall, 'wallR'));
  b.push(glow(0, 0.7, -0.86, 0.4, 0.16, 0.03, 0x4060ff, 0.9, 'rune'));
  b.push(glow(0, 0.0, 0.62, 1.4, 0.02, 0.08, 0x2a4a9a, 0.55, 'sill'));
  const g = build(b, { jitter: 0.04 });
  return finish(g, 'stairs', { rune: named(g, 'rune') });
}
// stairsDown() — the hub's descent: a 1x1 stairwell sunk below the floor (the floor block under it is skipped),
// five steps dropping toward +Z, shaft walls, a black passage mouth at the bottom and a rune glow; 12 boxes.
// Origin at floor level in the cell centre; nothing rises above y = 0. Decorative — no collision.
export function stairsDown() {
  const stone = 0x4e4e5c, wall = 0x2a2a34, b = [];
  const bottom = -1.4;
  for (let i = 0; i < 5; i++) { const top = -0.24 * (i + 1); b.push(box(0, bottom, -0.4 + 0.2 * i, 0.9, top - bottom, 0.22, stone, `step${i}`)); }
  b.push(box(0, bottom - 0.06, 0, 1.0, 0.06, 1.0, 0x1a1a22, 'pitFloor'));
  b.push(box(-0.5, bottom, 0, 0.06, -bottom, 1.02, wall, 'wallL'), box(0.5, bottom, 0, 0.06, -bottom, 1.02, wall, 'wallR'));
  b.push(box(0, bottom, -0.5, 1.02, -bottom, 0.06, wall, 'wallN'));
  b.push(box(0, bottom, 0.5, 1.02, 0.2, 0.06, wall, 'lintel'));                       // low sill: the passage continues below it
  b.push(box(0, bottom, 0.49, 0.9, -bottom - 0.2, 0.02, 0x020204, 'void'));            // the black mouth of the way down
  b.push(glow(0, bottom + 0.55, 0.47, 0.4, 0.16, 0.03, 0x4060ff, 0.9, 'rune'));
  b.push(glow(0, -0.02, -0.48, 0.9, 0.02, 0.06, 0x2a4a9a, 0.55, 'sill'));
  const g = build(b, { jitter: 0.04 });
  return finish(g, 'stairsDown', { rune: named(g, 'rune') });
}
// elevator() — cage: floor, 4 posts, 8 rails (waist + top ring), roof, cable and a hanging lamp; 19 boxes, 3.5 tall.
// userData.lamp = the glowing lamp mesh, lightY = 2.4. Decorative: the player spawns inside the cage.
export function elevator() {
  const wood = 0x5a4a3a, iron = 0x4a4038, b = [box(0, 0, 0, 1.6, 0.1, 1.6, wood, 'floor')];
  for (const [x, z] of [[-0.76, -0.76], [0.76, -0.76], [-0.76, 0.76], [0.76, 0.76]]) b.push(box(x, 0.1, z, 0.08, 2.7, 0.08, iron));
  for (const y of [0.9, 2.5]) {
    b.push(box(0, y, -0.76, 1.52, 0.06, 0.06, iron), box(0, y, 0.76, 1.52, 0.06, 0.06, iron),
      box(-0.76, y, 0, 0.06, 0.06, 1.52, iron), box(0.76, y, 0, 0.06, 0.06, 1.52, iron));
  }
  b.push(box(0, 2.8, 0, 1.7, 0.08, 1.7, wood, 'roof'), box(0, 2.88, 0, 0.08, 0.62, 0.08, 0x2a2420, 'cable'));
  b.push(box(0, 2.62, 0, 0.04, 0.18, 0.04, 0x2a2420), glow(0, 2.44, 0, 0.16, 0.18, 0.16, 0xffc070, 1.0, 'lamp'));
  const g = build(b, { jitter: 0.04 });
  return finish(g, 'elevator', { lamp: named(g, 'lamp'), lightY: 2.4 });
}
// gate() — locked gate (X): 5 bars, 2 crossbars, hinge plates and a lock box; 10 boxes, 2.6 tall, spans 1 u in x.
export function gate() {
  const iron = 0x4a4a4a, b = [];
  for (let i = -2; i <= 2; i++) b.push(box(i * 0.2, 0, 0, 0.08, 2.6, 0.08, iron));
  b.push(box(0, 0.6, 0, 0.9, 0.08, 0.1, iron), box(0, 2.0, 0, 0.9, 0.08, 0.1, iron));
  b.push(box(-0.45, 0.5, 0, 0.06, 0.2, 0.14, 0x2a2a2a), box(-0.45, 1.9, 0, 0.06, 0.2, 0.14, 0x2a2a2a));
  b.push(box(0.3, 1.25, 0, 0.18, 0.22, 0.14, 0x6a5a30, 'lock'));
  return finish(build(b, { jitter: 0.03 }), 'gate');
}
// shortcutBarred() / shortcutOpen() — the `=` shortcut door (DESIGN.md §3.6).
// PLACEHOLDER ART (built by the maps groundwork, not by the art agent): a readable portcullis, not the final piece.
// The art agent owns the final look; keep the contract: origin at the cell centre on the floor, front faces -Z,
// spans ≤ 1 u in x, ≤ 3.4 tall, and the ONLY difference between the two must be that the bars are up.
// `opts.side` = +1 / −1 along the door's own -Z axis: which side carries the brass lift-bar and its amber glint,
// i.e. the `openFrom` side, so the player can read through the bars which side opens it. world.js yaws the group
// so -Z points at the openFrom flank, so `side` is +1 in practice; -1 is there for models.html.
export function shortcutBarred({ side = 1 } = {}) {
  const timber = 0x3a2a1a, iron = 0x2a2420, brass = 0x8a6a2a, b = [];
  b.push(box(0, 2.6, 0, 1.02, 0.4, 0.34, timber, 'lintel'));                       // stone/timber lintel
  b.push(box(-0.46, 0, 0, 0.1, 2.6, 0.28, timber, 'jambL'), box(0.46, 0, 0, 0.1, 2.6, 0.28, timber, 'jambR'));
  for (let i = -2; i <= 2; i++) b.push(box(i * 0.18, 0, 0, 0.09, 2.6, 0.09, iron, `bar${i + 2}`));   // 5 heavy bars
  b.push(box(0, 0.5, 0, 0.84, 0.08, 0.1, iron, 'crossLow'), box(0, 1.9, 0, 0.84, 0.08, 0.1, iron, 'crossHigh'));
  b.push(box(0.34, 2.18, 0, 0.2, 0.2, 0.2, iron, 'drum'), box(0.34, 1.2, 0, 0.05, 1.0, 0.05, iron, 'chain'));
  // the lift-bar and its glint sit on the far side only: that is the side E works from
  b.push(box(0, 1.15, side * -0.17, 0.7, 0.12, 0.06, brass, 'liftBar'));
  b.push(glow(0.26, 1.15, side * -0.21, 0.08, 0.08, 0.04, 0xffb040, 0.9, 'glint'));
  return finish(build(b, { jitter: 0.03 }), 'shortcutBarred', { side });
}
// Same frame with the bars raised into the lintel (y 2.2–3.0): walkable underneath, and still visible from far off,
// so the player can always see which shortcut they have opened.
export function shortcutOpen({ side = 1 } = {}) {
  const timber = 0x3a2a1a, iron = 0x2a2420, brass = 0x8a6a2a, b = [];
  b.push(box(0, 2.6, 0, 1.02, 0.4, 0.34, timber, 'lintel'));
  b.push(box(-0.46, 0, 0, 0.1, 2.6, 0.28, timber, 'jambL'), box(0.46, 0, 0, 0.1, 2.6, 0.28, timber, 'jambR'));
  for (let i = -2; i <= 2; i++) b.push(box(i * 0.18, 2.2, 0, 0.09, 0.8, 0.09, iron, `bar${i + 2}`));   // raised
  b.push(box(0, 2.2, 0, 0.84, 0.08, 0.1, iron, 'crossLow'), box(0, 2.98, 0, 0.84, 0.08, 0.1, iron, 'crossHigh'));
  b.push(box(0.34, 2.18, 0, 0.2, 0.2, 0.2, iron, 'drum'), box(0.34, 2.2, 0, 0.05, 0.8, 0.05, iron, 'chain'));
  b.push(box(0, 2.24, side * -0.17, 0.7, 0.12, 0.06, brass, 'liftBar'));
  b.push(glow(0.26, 2.24, side * -0.21, 0.08, 0.08, 0.04, 0xffb040, 0.5, 'glint'));
  return finish(build(b, { jitter: 0.03 }), 'shortcutOpen', { side });
}
// altar() — the Source: step, ring of 8 stones, bowl, 3-box flame (violet → orange); 13 boxes.
// userData.flame = [meshes], userData.update(t) sways the flame.
export function altar() {
  const b = [box(0, 0, 0, 1.6, 0.12, 1.6, 0x24242c, 'step')];
  for (let i = 0; i < 8; i++) { const a = i / 8 * Math.PI * 2; b.push(rotated(box(Math.cos(a) * 1.2, 0, Math.sin(a) * 1.2, 0.3, 0.4 + 0.1 * (i % 2), 0.3, 0x2a2a30), -a)); }
  b.push(box(0, 0.12, 0, 0.8, 0.5, 0.8, 0x3a3a44, 'bowl'));
  [[0.4, 0x8040ff], [0.28, 0xc050a0], [0.16, 0xff6020]].forEach(([s, c], i) => b.push(glow(0, 0.62 + i * 0.28, 0, s, 0.3, s, c, 1, `flame${i}`)));
  const g = build(b, { jitter: 0.03 });
  const flame = [0, 1, 2].map(i => named(g, `flame${i}`));
  const update = (t) => flame.forEach((m, i) => { m.rotation.y = i * 0.7 + 0.35 * Math.sin(t * (2.1 + i * 0.6) + i); m.position.x = 0.04 * i * Math.sin(t * 3.3 + i); m.position.z = 0.04 * i * Math.cos(t * 2.7 + i); });
  return finish(g, 'altar', { flame, update });
}
// water() — geometry + a FRESH material for world.js's merged/instanced water sheet (DESIGN-v2 §2).
export function water() {
  return { geo: geom(1.02, 0.05, 1.02),
    mat: new THREE.MeshLambertMaterial({ color: 0x1a2a3a, emissive: 0x06101a, emissiveIntensity: 0.3 }) };
}
// waterTile(n) — an n×n patch of animated water surface (InstancedMesh, counts as 1 box) on a dark bed,
// at y −0.1 like the zones. userData.update(t) scrolls the glow and bobs the instances ±0.02.
export function waterTile(opts = {}) {
  const n = (typeof opts === 'number' ? opts : opts.n) || 4;
  const g = new THREE.Group();
  const bed = new THREE.Mesh(geom(n, 0.06, n), material(0x0a0e14).clone()); bed.material._shared = false;
  bed.position.y = -0.22; g.add(bed);
  const { geo, mat } = water();
  mat.transparent = true; mat.opacity = 0.85;
  const mesh = new THREE.InstancedMesh(geo, mat, n * n); mesh.name = 'water';
  const d = new THREE.Object3D(), cells = [];
  let i = 0;
  for (let z = 0; z < n; z++) for (let x = 0; x < n; x++) { cells.push([x - n / 2 + 0.5, z - n / 2 + 0.5]); d.position.set(cells[i][0], -0.1, cells[i][1]); d.updateMatrix(); mesh.setMatrixAt(i++, d.matrix); }
  g.add(mesh);
  const update = (t) => {
    mat.emissiveIntensity = 0.25 + 0.1 * Math.sin(t * 1.7);
    cells.forEach(([x, z], k) => { d.position.set(x, -0.1 + 0.02 * Math.sin(t * 2 + x + z), z); d.updateMatrix(); mesh.setMatrixAt(k, d.matrix); });
    mesh.instanceMatrix.needsUpdate = true;
  };
  return finish(g, 'water', { mesh, update });
}

/* ============================================================
   Hub buildings (DESIGN-v2 §7) — anchor at the group origin, front toward −Z
   ============================================================ */
// tram() — mine cart on 6 u of rails (rails run along z, cart faces −Z); 20 boxes.
// userData.cart = the cart sub-group (slide it along z), userData.lamp = headlamp mesh.
export function tram() {
  const rail = 0x444444, tie = 0x3a2a1a, b = [];
  b.push(box(-0.5, 0, 0, 0.08, 0.08, 6, rail), box(0.5, 0, 0, 0.08, 0.08, 6, rail));
  for (let i = -2; i <= 2; i++) b.push(box(0, 0, i * 1.2, 1.3, 0.05, 0.16, tie));
  const rails = build(b, { jitter: 0.04 });
  const cb = [
    box(0, 0.3, 0, 1.4, 0.6, 0.9, 0x6a3a2a, 'body'),
    box(0, 0.9, -0.44, 1.46, 0.06, 0.06, 0x4a2a1a), box(0, 0.9, 0.44, 1.46, 0.06, 0.06, 0x4a2a1a), // rim
    box(-0.72, 0.9, 0, 0.06, 0.06, 0.94, 0x4a2a1a), box(0.72, 0.9, 0, 0.06, 0.06, 0.94, 0x4a2a1a),
    box(0, 0.18, 0, 1.0, 0.14, 0.6, 0x2a2420),                                                    // axle block
    box(-0.62, 0.62, 0, 0.16, 0.2, 0.3, 0x3a2a1a), box(0.62, 0.62, 0, 0.16, 0.2, 0.3, 0x3a2a1a),  // ore: relic-like glints in the cart
    emissive(box(-0.3, 0.9, 0.1, 0.22, 0.14, 0.22, 0x60e0ff), 0x60e0ff, 0.5), emissive(box(0.25, 0.9, -0.15, 0.18, 0.12, 0.18, 0xffa030), 0xffa030, 0.4),
    glow(0, 0.55, -0.47, 0.18, 0.18, 0.06, 0xffc070, 1.0, 'lamp'),
  ];
  for (const [x, z] of [[-0.55, -0.32], [0.55, -0.32], [-0.55, 0.32], [0.55, 0.32]]) cb.push(box(x, 0.05, z, 0.3, 0.3, 0.1, 0x2a2a2a));
  const cart = build(cb, { jitter: 0.04 });
  cart.rotation.y = Math.PI / 2;   // long axis along the rails (Z); wheels roll along Z
  const g = new THREE.Group(); g.add(rails); g.add(cart);
  return finish(g, 'tram', { cart, lamp: named(cart, 'lamp') });
}
// workshop() — bench with tools, tool wall behind (+Z), anvil on a stump, 3 hanging lamps; 22 boxes.
export function workshop() {
  const wood = 0x7a5a3a, dark = 0x3a2a1a, iron = 0x333338, b = [
    box(0, 0.8, 0, 1.2, 0.1, 0.6, wood, 'benchTop'),
    box(-0.5, 0, -0.2, 0.1, 0.8, 0.1, dark), box(0.5, 0, -0.2, 0.1, 0.8, 0.1, dark), box(-0.5, 0, 0.2, 0.1, 0.8, 0.1, dark), box(0.5, 0, 0.2, 0.1, 0.8, 0.1, dark),
    box(0, 0.3, 0, 1.1, 0.06, 0.5, dark),                                   // shelf
    box(-0.3, 0.9, 0, 0.3, 0.08, 0.16, iron), box(0.25, 0.9, -0.1, 0.16, 0.16, 0.16, 0x6a4a2a), // hammer head, oil pot
    box(0, 0.6, 0.55, 1.4, 1.2, 0.1, 0x5a4530, 'toolWall'),
    box(-0.45, 1.0, 0.48, 0.08, 0.5, 0.04, iron), box(-0.45, 1.5, 0.48, 0.24, 0.1, 0.05, iron),  // hammer on wall
    box(0.1, 0.9, 0.48, 0.06, 0.6, 0.04, iron), box(0.2, 0.9, 0.48, 0.06, 0.6, 0.04, iron),     // tongs
    box(0.5, 1.2, 0.48, 0.3, 0.3, 0.04, 0x8a6a4a),                                                // hanging coil
    box(0.95, 0, -0.5, 0.36, 0.35, 0.36, dark, 'stump'), box(0.95, 0.35, -0.5, 0.5, 0.28, 0.24, iron, 'anvil'), box(0.95, 0.5, -0.78, 0.2, 0.1, 0.2, iron),
  ];
  for (const x of [-0.45, 0, 0.45]) b.push(box(x, 1.95, -0.05, 0.03, 0.45, 0.03, iron), glow(x, 1.8, -0.05, 0.16, 0.16, 0.16, 0xffc070, 1.0, `lamp${x}`));
  const g = build(b, { jitter: 0.04 });
  return finish(g, 'workshop', { lamps: [-0.45, 0, 0.45].map(x => named(g, `lamp${x}`)) });
}
// oilPress() — vat, press plate, screw column with crossbar, frame, dripping spout, 2 hooped barrels (+x side); 18 boxes, 2.05 tall.
export function oilPress() {
  const iron = 0x555555, screw = 0x888888, wood = 0x5a4a3a, barrel = 0x6a4a2a, hoop = 0x2a2420, b = [
    box(0, 0, 0, 1.0, 0.8, 1.0, iron, 'vat'), box(0, 0.8, 0, 0.7, 0.1, 0.7, 0x6a5a4a, 'plate'),
    box(0, 0.9, 0, 0.2, 1.0, 0.2, screw, 'screw'), box(0, 1.72, 0, 1.0, 0.07, 0.07, screw, 'lever'), box(0.5, 1.6, 0, 0.07, 0.19, 0.07, screw),
    box(-0.62, 0, 0, 0.12, 2.0, 0.2, wood), box(0.62, 0, 0, 0.12, 2.0, 0.2, wood), box(0, 1.9, 0, 1.36, 0.15, 0.3, wood, 'beam'),
    box(0, 0.22, -0.54, 0.16, 0.1, 0.12, iron, 'tap'), glow(0, 0.08, -0.6, 0.1, 0.16, 0.1, 0xffa030, 0.9, 'drip'),
    emissive(box(0, 0, -0.75, 0.36, 0.08, 0.3, 0xffa030), 0xffa030, 0.5),                                      // basin of oil
    // both barrels on the +x side: placed rot PI against a wall they sit west of the vat, leaving anchor +1 x (the keeper) clear
    box(1.0, 0, -0.3, 0.5, 0.7, 0.5, barrel), box(1.0, 0.12, -0.3, 0.54, 0.05, 0.54, hoop), box(1.0, 0.55, -0.3, 0.54, 0.05, 0.54, hoop),
    box(1.0, 0, 0.3, 0.5, 0.6, 0.5, barrel), box(1.0, 0.1, 0.3, 0.54, 0.05, 0.54, hoop), box(1.0, 0.46, 0.3, 0.54, 0.05, 0.54, hoop),
  ];
  const g = build(b, { jitter: 0.04 });
  return finish(g, 'oilPress', { drip: named(g, 'drip') });
}
// cartTable() — table, map sheet with ink lines, 2 candles, stool behind (+Z); 16 boxes.
export function cartTable() {
  const wood = 0x8a6a4a, leg = 0x5a4530, paper = 0xe0d0a0, ink = 0x3a3050, b = [
    box(0, 0.7, 0, 1.4, 0.08, 0.9, wood, 'top'),
    box(-0.6, 0, -0.35, 0.1, 0.7, 0.1, leg), box(0.6, 0, -0.35, 0.1, 0.7, 0.1, leg), box(-0.6, 0, 0.35, 0.1, 0.7, 0.1, leg), box(0.6, 0, 0.35, 0.1, 0.7, 0.1, leg),
    box(0, 0.78, 0, 1.2, 0.02, 0.7, paper, 'map'),
    box(-0.2, 0.8, -0.1, 0.5, 0.01, 0.05, ink), box(0.15, 0.8, 0.12, 0.05, 0.01, 0.4, ink), box(0.3, 0.8, -0.2, 0.3, 0.01, 0.05, ink),
    box(-0.35, 0.8, 0.15, 0.12, 0.02, 0.12, 0x8a2a2a),                                           // red wax seal / marker
    box(-0.58, 0.78, -0.32, 0.1, 0.04, 0.1, 0x3a3030), glow(-0.58, 0.82, -0.32, 0.06, 0.14, 0.06, 0xffd080, 1, 'candle0'),
    box(0.58, 0.78, 0.3, 0.1, 0.04, 0.1, 0x3a3030), glow(0.58, 0.82, 0.3, 0.06, 0.12, 0.06, 0xffd080, 1, 'candle1'),
    box(0, 0.35, 0.75, 0.36, 0.06, 0.36, wood, 'stool'), box(0, 0, 0.75, 0.1, 0.35, 0.1, leg),
  ];
  const g = build(b, { jitter: 0.04 });
  return finish(g, 'cartTable', { candles: [named(g, 'candle0'), named(g, 'candle1')] });
}
// shrine() — plinth, hooded idol with a blue halo, 6 candles in an arc in front; 13 boxes, 1.7 tall.
export function shrine() {
  const stone = 0x4a4a55, idol = 0x9a9aa8, b = [
    box(0, 0, 0, 1.0, 0.6, 1.0, stone, 'plinth'), box(0, 0.6, 0.15, 0.4, 0.8, 0.3, idol, 'idol'),
    box(0, 1.4, 0.15, 0.24, 0.24, 0.24, 0xb0b0c0, 'head'), box(0, 1.64, 0.15, 0.3, 0.06, 0.3, 0x7a7a90),
    glow(0, 1.32, 0.34, 0.5, 0.5, 0.04, 0x6060ff, 0.9, 'halo'),
    box(0, 0.6, 0.45, 0.9, 1.15, 0.08, 0x3a3a48, 'back'),
  ];
  for (let i = 0; i < 6; i++) { const a = Math.PI * (1.1 + 0.16 * i); b.push(glow(Math.cos(a) * 0.38, 0.6, Math.sin(a) * 0.38 - 0.05, 0.07, 0.1 + 0.03 * (i % 3), 0.07, 0xffc070, 1, `candle${i}`)); }
  const g = build(b, { jitter: 0.03 });
  return finish(g, 'shrine', { halo: named(g, 'halo'), candles: [0, 1, 2, 3, 4, 5].map(i => named(g, `candle${i}`)) });
}
// board({locked:[bool×4]}) — departure board: 2 posts, board, cap, 4 papers (dark grey when locked); 8 boxes, 2.1 tall.
// userData.papers[i] and userData.setLocked(i, bool) recolour a paper.
export function board(opts = {}) {
  const locked = opts.locked || [false, true, true, true];
  const wood = 0x3a2a1a, b = [
    box(-0.8, 0, 0, 0.08, 2.0, 0.08, wood), box(0.8, 0, 0, 0.08, 2.0, 0.08, wood),
    box(0, 1.0, 0, 1.6, 1.0, 0.06, 0x4a3a2a, 'board'), box(0, 2.0, 0, 1.7, 0.06, 0.2, wood, 'cap'),
  ];
  const xs = [-0.57, -0.19, 0.19, 0.57];
  xs.forEach((x, i) => b.push(box(x, 1.25, -0.04, 0.3, 0.42, 0.02, locked[i] ? 0x555555 : 0xe0d0a0, `paper${i}`)));
  const g = build(b, { jitter: 0 });
  const papers = xs.map((_, i) => named(g, `paper${i}`));
  const setLocked = (i, on) => { if (papers[i]) papers[i].material.color.set(on ? 0x555555 : 0xe0d0a0); };
  return finish(g, 'board', { papers, setLocked });
}
// flameBase({tier=1}) — the great flame: brazier base, rim, corner posts, coal bed and a 4-tongue fire stack.
// userData: base, rim, fire (group at y 0.5), tongues[4], setTier(t), update(t) (sway, as hub.js v1). 11 boxes.
export function flameBase(opts = {}) {
  const g = build([
    box(0, 0, 0, 1.2, 0.5, 1.2, 0x3a3028, 'base'),
    box(-0.55, 0, -0.55, 0.14, 0.62, 0.14, 0x2a2420), box(0.55, 0, -0.55, 0.14, 0.62, 0.14, 0x2a2420),
    box(-0.55, 0, 0.55, 0.14, 0.62, 0.14, 0x2a2420), box(0.55, 0, 0.55, 0.14, 0.62, 0.14, 0x2a2420),
    box(0, 0.5, 0, 1.0, 0.08, 1.0, 0x1a1210, 'rim'),
    glow(0, 0.5, 0, 0.7, 0.1, 0.7, 0xff4010, 0.8, 'coals'),
  ], { jitter: 0.03 });
  const base = named(g, 'base'), rim = named(g, 'rim');
  base.material.emissive.set(0xff6a28); base.material.emissiveIntensity = 0.12;
  rim.material.emissive.set(0xff7a30); rim.material.emissiveIntensity = 0.45;
  const fire = new THREE.Group(); fire.position.y = 0.5;
  const tongues = []; let y = 0;
  [0.6, 0.45, 0.32, 0.2].forEach((s, i) => {
    const m = new THREE.Mesh(geom(s, s, s), new THREE.MeshLambertMaterial({ color: 0x000000, emissive: 0xff6020, emissiveIntensity: 1 }));
    m.position.y = y + s / 2; y += s * 0.8; m.rotation.y = i * 0.7; m.name = `tongue${i}`;
    fire.add(m); tongues.push(m);
  });
  g.add(fire);
  let tier = 1;
  const setTier = (t) => {
    tier = t;
    const c = new THREE.Color(0xff6020).lerp(new THREE.Color(0xffd080), (t - 1) / 3);
    tongues.forEach((b, i) => b.material.emissive.copy(c).lerp(new THREE.Color(0xfff0c0), i * 0.22).multiplyScalar(0.7 + 0.1 * i));
    base.material.emissiveIntensity = 0.08 + 0.05 * (t - 1);
    rim.material.emissiveIntensity = 0.35 + 0.12 * (t - 1);
    fire.scale.setScalar(0.6 + 0.4 * (t - 1));
  };
  const update = (time) => {
    const s = 0.6 + 0.4 * (tier - 1);
    fire.scale.set(s, s * (0.95 + 0.08 * Math.sin(time * 9.7)), s);
    tongues.forEach((b, i) => { b.rotation.y = i * 0.7 + 0.35 * Math.sin(time * (2.1 + i * 0.6) + i); b.position.x = 0.04 * i * Math.sin(time * 3.3 + i * 1.7); b.position.z = 0.04 * i * Math.cos(time * 2.7 + i * 0.9); });
  };
  setTier(opts.tier || 1);
  return finish(g, 'flameBase', { base, rim, fire, tongues, setTier, update, lightY: 1.2 });
}

/* ============================================================
   Hub props (DESIGN.md §3 hub) — box lists, so world.js can merge a whole camp into two draw calls
   ============================================================ */
// Each *Boxes(opts) returns a box list (origin at the feet, front toward -Z); the matching factory wraps it in a
// Group for models.html / makeModel. Heights matter: world.js/hub.js treat a box as a collider when its top is
// above HUB_BLOCK.minTop (0.25) and its bottom below HUB_BLOCK.maxBottom (1.2): rugs, bedrolls, candle clusters
// and anything hung from the ceiling stay walkable.
const WOOD = 0x6a4a2a, WOOD_D = 0x3a2a1a, IRON = 0x2a2420, CANDLE = 0xffd080;
// rug({w, d, color, border}) — flat carpet with a border and two inner stripes; 7 boxes, 0.03 tall.
export function rugBoxes(o = {}) {
  const w = o.w || 3.0, d = o.d || 2.0, c = o.color != null ? o.color : 0x7a2a22, b = o.border != null ? o.border : 0xc09040, inner = o.inner != null ? o.inner : 0x5a1e1a;
  return [
    box(0, 0, 0, w, 0.02, d, c, 'rug'),
    box(0, 0.02, -d / 2 + 0.1, w, 0.01, 0.12, b), box(0, 0.02, d / 2 - 0.1, w, 0.01, 0.12, b),
    box(-w / 2 + 0.1, 0.02, 0, 0.12, 0.01, d, b), box(w / 2 - 0.1, 0.02, 0, 0.12, 0.01, d, b),
    box(0, 0.02, -d * 0.18, w * 0.6, 0.01, 0.08, inner), box(0, 0.02, d * 0.18, w * 0.6, 0.01, 0.08, inner),
  ];
}
export const rug = (o) => finish(build(rugBoxes(o), { jitter: 0.02 }), 'rug');
// bench() — plank seat on two block legs with a folded blanket at one end; 4 boxes, 0.58 tall, 1.4 long (x).
export function benchBoxes() {
  return [box(0, 0.4, 0, 1.4, 0.08, 0.42, WOOD, 'seat'), box(-0.55, 0, 0, 0.1, 0.4, 0.36, WOOD_D), box(0.55, 0, 0, 0.1, 0.4, 0.36, WOOD_D),
    box(-0.42, 0.48, 0, 0.36, 0.1, 0.34, 0x8a3a30, 'blanket')];
}
export const bench = () => finish(build(benchBoxes(), { jitter: 0.04 }), 'bench');
// bedroll() — mat, blanket, pillow along z; 3 boxes, 0.2 tall (walkable).
export function bedrollBoxes(o = {}) {
  const blanket = o.color != null ? o.color : 0x7a3a30;
  return [box(0, 0, 0, 0.7, 0.12, 1.5, 0x5a4a3a, 'mat'), box(0, 0.12, 0.15, 0.66, 0.08, 1.0, blanket, 'blanket'), box(0, 0.12, -0.55, 0.5, 0.1, 0.3, 0xd8c8a8, 'pillow')];
}
export const bedroll = (o) => finish(build(bedrollBoxes(o), { jitter: 0.04 }), 'bedroll');
// crate({s}) — planked box with two iron bands; 3 boxes. crateStack() — two crates and a small one; 9 boxes, 1.2 tall.
export function crateBoxes(o = {}) {
  const s = o.s || 0.7, x = o.x || 0, y = o.y || 0, z = o.z || 0, ry = o.ry || 0;
  return [rotated(box(x, y, z, s, s, s, 0x7a5a3a, 'crate'), ry), rotated(box(x, y + s * 0.3, z, s + 0.02, 0.05, s + 0.02, IRON), ry), rotated(box(x, y + s * 0.7, z, s + 0.02, 0.05, s + 0.02, IRON), ry)];
}
export const crate = (o) => finish(build(crateBoxes(o), { jitter: 0.04 }), 'crate');
export function crateStackBoxes() {
  return [...crateBoxes({ s: 0.7, x: -0.1 }), ...crateBoxes({ s: 0.6, x: 0.05, y: 0.7, ry: 0.35 }), ...crateBoxes({ s: 0.4, x: -0.25, y: 1.3, z: 0.05, ry: -0.5 })];
}
export const crateStack = () => finish(build(crateStackBoxes(), { jitter: 0.04 }), 'crateStack');
// barrel() — hooped barrel, 0.5 wide, 0.7 tall; 4 boxes.
export function barrelBoxes(o = {}) {
  const h = o.h || 0.7;
  return [box(0, 0, 0, 0.5, h, 0.5, 0x6a4a2a, 'barrel'), box(0, 0.1, 0, 0.54, 0.05, 0.54, IRON), box(0, h - 0.15, 0, 0.54, 0.05, 0.54, IRON), box(0, h, 0, 0.42, 0.03, 0.42, 0x4a3520, 'lid')];
}
export const barrel = (o) => finish(build(barrelBoxes(o), { jitter: 0.04 }), 'barrel');
// logPile() — six logs in three rows; 6 boxes, 0.66 tall, logs run along z.
export function logPileBoxes() {
  const b = [];
  [[-0.24, 0], [0, 0], [0.24, 0], [-0.12, 0.22], [0.12, 0.22], [0, 0.44]].forEach(([x, y]) => b.push(box(x, y, 0, 0.22, 0.22, 0.9, 0x5a3a20, 'log')));
  b.push(box(0, 0.05, 0.48, 0.74, 0.5, 0.02, 0x8a6a40, 'ends'));
  return b;
}
export const logPile = () => finish(build(logPileBoxes(), { jitter: 0.05 }), 'logPile');
// cookpot() — ash ring, ember bed, tripod, chain, hanging pot with lid; 10 boxes, 1.15 tall. Emissive: embers, stew.
export function cookpotBoxes() {
  const b = [box(0, 0, 0, 0.8, 0.04, 0.8, 0x2a2420, 'ash'), glow(0, 0.04, 0, 0.5, 0.07, 0.5, 0xff4a10, 0.7, 'embers')];
  for (let i = 0; i < 3; i++) { const a = i / 3 * Math.PI * 2 + 0.5; b.push(box(Math.cos(a) * 0.3, 0, Math.sin(a) * 0.3, 0.05, 1.1, 0.05, IRON)); }
  b.push(box(0, 1.1, 0, 0.62, 0.05, 0.05, IRON), box(0, 0.78, 0, 0.03, 0.32, 0.03, IRON, 'chain'),
    box(0, 0.42, 0, 0.42, 0.34, 0.42, 0x24242a, 'pot'), box(0, 0.76, 0, 0.46, 0.04, 0.46, 0x34343a, 'lid'), box(0, 0.8, 0, 0.08, 0.06, 0.08, 0x34343a),
    glow(0, 0.74, 0, 0.3, 0.02, 0.3, 0xd08040, 0.5, 'stew'));
  return b;
}
export const cookpot = () => finish(build(cookpotBoxes(), { jitter: 0.04 }), 'cookpot');
// bookshelf() — back, two sides, four shelves, ten books; 17 boxes, 1.8 tall, 1.0 wide, back toward +Z.
export function bookshelfBoxes() {
  const b = [box(0, 0, 0.14, 1.0, 1.8, 0.06, WOOD_D, 'back'), box(-0.48, 0, 0, 0.05, 1.8, 0.34, WOOD), box(0.48, 0, 0, 0.05, 1.8, 0.34, WOOD)];
  const books = [0x8a2a2a, 0x2a4a7a, 0x4a6a3a, 0xa08040, 0x5a3a6a, 0x9a8a6a, 0x3a3a3a, 0x7a4a2a, 0x2a6a6a, 0xc0a060];
  let k = 0;
  [0.02, 0.48, 0.94, 1.4].forEach((y, i) => {
    b.push(box(0, y, 0, 0.9, 0.04, 0.32, WOOD, 'shelf'));
    let x = -0.42;
    const n = 2 + (i % 2);
    for (let j = 0; j < n && k < books.length; j++) { const w = 0.18 + 0.1 * ((k * 7) % 3), h = 0.28 + 0.05 * ((k * 5) % 3); b.push(box(x + w / 2, y + 0.04, 0.02, w, h, 0.22, books[k++], 'book')); x += w + 0.04; }
  });
  return b;
}
export const bookshelf = () => finish(build(bookshelfBoxes(), { jitter: 0.03 }), 'bookshelf');
// herbRail() — a rail hung under the ceiling with herb bundles and amber bottles; 12 boxes, y 1.85..2.35 (walkable).
export function herbRailBoxes() {
  const b = [box(0, 2.32, 0, 1.6, 0.04, 0.04, IRON, 'rail')];
  [[-0.62, 0x4a5a2a, 0.42], [-0.34, 0x5a5a30, 0.36], [0.05, 0x3a4a22, 0.44], [0.62, 0x6a6a38, 0.34]].forEach(([x, c, h]) => {
    b.push(box(x, 2.32 - 0.14, 0, 0.02, 0.14, 0.02, 0xa09070), box(x, 2.18 - h, 0, 0.13, h, 0.13, c, 'herbs'));
  });
  [[-0.15, 0.3], [0.32, 0.26], [0.45, 0.22]].forEach(([x, h]) => {
    b.push(box(x, 2.32 - 0.12, 0, 0.02, 0.12, 0.02, 0xa09070), emissive(box(x, 2.2 - h, 0, 0.1, h, 0.1, 0xffa030, 'bottle'), 0xffa030, 0.55));
  });
  return b;
}
export const herbRail = () => finish(build(herbRailBoxes(), { jitter: 0.04 }), 'herbRail');
// candleCluster() — seven candle stubs of different heights with small flames; 14 boxes, ≤ 0.22 tall (walkable).
export function candleClusterBoxes() {
  const b = [];
  [[0, 0, 0.14], [0.14, 0.06, 0.1], [-0.13, 0.05, 0.08], [0.06, -0.14, 0.12], [-0.08, 0.15, 0.16], [0.2, -0.1, 0.07], [-0.2, -0.08, 0.11]].forEach(([x, z, h]) => {
    b.push(box(x, 0, z, 0.07, h, 0.07, 0xe8dcc0, 'stub'), glow(x, h, z, 0.035, 0.045, 0.035, CANDLE, 1, 'flame'));
  });
  return b;
}
export const candleCluster = () => finish(build(candleClusterBoxes(), { jitter: 0.02 }), 'candleCluster');
// hangLantern() — chain from the ceiling (y 3), cap, four cage posts, glass, foot; 9 boxes, y 2.1..3.0 (walkable underneath).
// The glass is the only emissive box ("glass"): world.js flickers whole tier groups through the merged glow material.
export function hangLanternBoxes(o = {}) {
  const y = o.y != null ? o.y : 2.1;
  const b = [box(0, y + 0.42, 0, 0.03, 3.0 - (y + 0.42), 0.03, IRON, 'chain'), box(0, y + 0.36, 0, 0.26, 0.06, 0.26, IRON, 'cap'),
    glow(0, y + 0.08, 0, 0.16, 0.26, 0.16, 0xffc070, 0.85, 'glass'), box(0, y + 0.02, 0, 0.24, 0.06, 0.24, IRON, 'foot')];
  for (const [x, z] of [[-0.1, -0.1], [0.1, -0.1], [-0.1, 0.1], [0.1, 0.1]]) b.push(box(x, y + 0.08, z, 0.03, 0.28, 0.03, IRON));
  b.push(box(0, y + 0.34, 0, 0.12, 0.03, 0.12, IRON));
  return b;
}
export const hangLantern = (o) => finish(build(hangLanternBoxes(o), { jitter: 0 }), 'hangLantern');
// stool() — a round-ish stool; 2 boxes, 0.42 tall.
export function stoolBoxes() { return [box(0, 0.36, 0, 0.36, 0.06, 0.36, WOOD, 'seat'), box(0, 0, 0, 0.1, 0.36, 0.1, WOOD_D)]; }
export const stool = () => finish(build(stoolBoxes(), { jitter: 0.04 }), 'stool');
// PROP_BOXES: name → box-list factory (world.js merges placed copies with mergeBoxes).
export const PROP_BOXES = { rug: rugBoxes, bench: benchBoxes, bedroll: bedrollBoxes, crate: crateBoxes, crateStack: crateStackBoxes, barrel: barrelBoxes,
  logPile: logPileBoxes, cookpot: cookpotBoxes, bookshelf: bookshelfBoxes, herbRail: herbRailBoxes, candleCluster: candleClusterBoxes, hangLantern: hangLanternBoxes, stool: stoolBoxes };

// placeBoxes(boxes, x, z, ry) → new box list moved to (x, z) and yawed by ry (three's rotation.y convention).
export function placeBoxes(boxes, x, z, ry = 0) {
  const c = Math.cos(ry), s = Math.sin(ry);
  return boxes.map(b => ({ ...b, x: x + b.x * c + b.z * s, z: z - b.x * s + b.z * c, ry: (b.ry || 0) + ry }));
}
// mergeBoxes(boxes, {jitter}) → Group of at most two meshes: "solid" (MeshLambertMaterial, vertexColors) for plain
// boxes and "glow" (MeshBasicMaterial, vertexColors — unlit, so it reads as emissive) for the glowing ones. One draw
// call each, however many boxes; userData.boxes = count. Geometry is fresh (dispose it with the group).
export function mergeBoxes(boxes, { jitter = 0.05 } = {}) {
  const lists = { solid: [], glow: [] };
  for (const b of boxes) (b.emissive != null ? lists.glow : lists.solid).push(b);
  const g = new THREE.Group();
  const col = new THREE.Color(), v = new THREE.Vector3(), n = new THREE.Vector3(), q = new THREE.Quaternion(), up = new THREE.Vector3(0, 1, 0);
  for (const [name, list] of Object.entries(lists)) {
    if (!list.length) continue;
    const pos = [], nor = [], colors = [], index = [];
    let base = 0;
    for (const b of list) {
      const geo = geom(b.w, b.h, b.d), p = geo.attributes.position, nn = geo.attributes.normal, ix = geo.index;
      q.setFromAxisAngle(up, b.ry || 0);
      col.set(b.emissive != null ? b.emissive : b.color);
      if (jitter > 0 && b.emissive == null) col.multiplyScalar(1 + (Math.random() * 2 - 1) * jitter);
      const k = b.emissive != null ? (b.k == null ? 1 : b.k) : 1;
      for (let i = 0; i < p.count; i++) {
        v.set(p.getX(i), p.getY(i), p.getZ(i)).applyQuaternion(q); pos.push(v.x + b.x, v.y + b.y + b.h / 2, v.z + b.z);
        n.set(nn.getX(i), nn.getY(i), nn.getZ(i)).applyQuaternion(q); nor.push(n.x, n.y, n.z);
        colors.push(col.r * k, col.g * k, col.b * k);
      }
      for (let i = 0; i < ix.count; i++) index.push(ix.getX(i) + base);
      base += p.count;
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    geo.setAttribute('normal', new THREE.Float32BufferAttribute(nor, 3));
    geo.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
    geo.setIndex(index);
    const mat = name === 'glow' ? new THREE.MeshBasicMaterial({ vertexColors: true }) : new THREE.MeshLambertMaterial({ vertexColors: true });
    const mesh = new THREE.Mesh(geo, mat); mesh.name = name; g.add(mesh);
  }
  g.userData = { model: 'merged', boxes: boxes.length, merged: true };
  return g;
}

/* ============================================================
   Creature roster (DESIGN.md §5.1–5) — one factory per profile, feet origin, front toward −Z.
   Every group's userData carries `model` (= the profile name), `boxes`, `height`, `eyes[]` (per-mesh emissive
   materials: set eyes[i].material.emissiveIntensity per state, exactly like the hunter) and the live handles the
   AI animates, listed per factory. `update(t)` (absolute seconds) drives the idle motion that needs no game state
   (bob / ripple pulse / lure flicker); everything state-driven is a handle the caller sets.
   ============================================================ */
const TAU = Math.PI * 2;
const pivot = (x, y, z) => { const p = new THREE.Group(); p.position.set(x, y, z); return p; };
let rippleGeo = null;
function ripple() {
  if (!rippleGeo) { rippleGeo = new THREE.RingGeometry(0.86, 1.0, 32); rippleGeo.rotateX(-Math.PI / 2); }
  const m = new THREE.Mesh(rippleGeo, new THREE.MeshLambertMaterial({ color: 0x000000, emissive: 0x2a5a6a, emissiveIntensity: 0.4,
    side: THREE.DoubleSide, transparent: true, opacity: 0.9 }));
  m.material._shared = false;
  return m;
}

// lampwight() — the light-drinker: a hooded, legless shroud 2.2 tall that drifts 0.15 u off the floor; 12 boxes.
// Pale blue-grey (0x6a7488) against the hunter's near-black. userData: eyes[2] (0x9ad0ff), ember (chest mesh,
// 0xffb265: emissiveIntensity 0 → 1.5 at SNUFF, decaying through SATED — the stolen light; unlit it reads as a recess
// in the shroud, never a black hole), body (sub-group the bob
// moves), hands[2], update(t) → 0.05 u bob at 0.7 Hz.
export function lampwight(opts = {}) {
  const shroud = 0x6a7488, hood = 0x0a0a10, rag = 0x46505f, eyeK = opts.eyeIntensity != null ? opts.eyeIntensity : 0.3;
  const g = new THREE.Group();
  const body = new THREE.Group(); body.position.y = 0.15;
  const parts = build([
    box(0, 0.0, 0.02, 0.34, 0.36, 0.26, rag, 'skirtLow'),                 // tattered hem, floating
    box(0, 0.34, 0, 0.5, 0.36, 0.32, shroud, 'skirtHigh'),
    box(0, 0.68, 0, 0.36, 1.0, 0.28, shroud, 'torso'),
    box(0, 1.5, 0.02, 0.28, 0.3, 0.28, hood, 'head'),                      // the face is the hood's shadow
    box(0, 1.7, 0.03, 0.42, 0.35, 0.4, shroud, 'hood'),
    box(-0.29, 0.3, -0.04, 0.1, 1.2, 0.1, shroud, 'armL'), box(0.29, 0.3, -0.04, 0.1, 1.2, 0.1, shroud, 'armR'),
    box(-0.29, 0.16, -0.05, 0.13, 0.15, 0.13, hood, 'handL'), box(0.29, 0.16, -0.05, 0.13, 0.15, 0.13, hood, 'handR'),
    glow(-0.07, 1.62, -0.13, 0.06, 0.06, 0.06, 0x9ad0ff, eyeK, 'eyeL'),
    glow(0.07, 1.62, -0.13, 0.06, 0.06, 0.06, 0x9ad0ff, eyeK, 'eyeR'),
    emissive(box(0, 1.2, -0.13, 0.14, 0.14, 0.08, 0x3a4250, 'ember'), 0xffb265, 0),   // unlit it is a recess in the shroud, not a black hole
  ], { jitter: 0 });
  body.add(parts); g.add(body);
  const update = (t) => { body.position.y = 0.15 + 0.05 * Math.sin(t * TAU * 0.7); };
  return finish(g, 'lampwight', { eyes: [named(parts, 'eyeL'), named(parts, 'eyeR')], ember: named(parts, 'ember'),
    hands: [named(parts, 'handL'), named(parts, 'handR')], body, update });
}

// warden() — the sentinel: an upright bronze-and-verdigris statue 2.5 tall on a plinth, halberd in hand; 16 boxes.
// userData: head (sub-group pivoted at the neck, y 2.05 — yaw it for the sweep when the body stays put), conePivot
// (an empty Object3D on the head at the visor, facing −Z: attach the SpotLight and its target here), visor (the
// emissive slit, 0x7fd0ff: k = spotlight ÷ 2), eyes = [visor], legs[2] (hip pivots: rotation.x ±0.3 in CHASE),
// plinth (hide it when it leaves the post), halberd, spotY 2.28.
export function warden(opts = {}) {
  const bronze = 0x3a2f22, verdigris = 0x2f5a4a, tabard = 0x1c1614, stone = 0x2a2620, dark = 0x241d15, helm = 0x4a3c28;
  const visorK = opts.visorIntensity != null ? opts.visorIntensity : 0.6;
  const g = new THREE.Group();
  g.add(build([box(0, 0, 0, 0.9, 0.15, 0.9, stone, 'plinth')], { jitter: 0 }));
  const plinth = named(g, 'plinth');
  const legs = [];
  for (const s of [-1, 1]) {
    const p = pivot(s * 0.18, 0.95, 0);
    p.add(build([box(0, -0.8, 0, 0.22, 0.8, 0.26, dark, 'leg')], { jitter: 0 }));
    g.add(p); legs.push(p);
  }
  const torso = build([
    box(0, 0.95, 0, 0.5, 0.22, 0.32, dark, 'pelvis'),
    box(0, 1.15, 0, 0.7, 0.9, 0.4, bronze, 'torso'),
    box(0, 1.0, -0.21, 0.4, 0.95, 0.04, tabard, 'tabard'),
    box(-0.48, 1.85, 0, 0.3, 0.3, 0.34, verdigris, 'pauldronL'), box(0.48, 1.85, 0, 0.3, 0.3, 0.34, verdigris, 'pauldronR'),
    box(-0.48, 1.1, 0, 0.2, 0.78, 0.2, dark, 'armL'), box(0.48, 1.1, 0, 0.2, 0.78, 0.2, dark, 'armR'),
    box(0.58, 0.3, -0.2, 0.06, 2.2, 0.06, 0x2a2018, 'halberd'),
    box(0.58, 2.0, -0.2, 0.3, 0.5, 0.05, 0x6a6a60, 'blade'),
  ], { jitter: 0 });
  g.add(torso);
  const head = pivot(0, 2.05, 0);
  const hb = build([
    box(0, 0, 0, 0.4, 0.45, 0.4, helm, 'helm'),
    box(0, 0.4, 0.02, 0.08, 0.14, 0.34, verdigris, 'crest'),
    glow(0, 0.24, -0.2, 0.3, 0.06, 0.03, 0x7fd0ff, visorK, 'visor'),
  ], { jitter: 0 });
  head.add(hb);
  const conePivot = new THREE.Object3D(); conePivot.name = 'conePivot'; conePivot.position.set(0, 0.23, -0.22); head.add(conePivot);
  g.add(head);
  const visor = named(hb, 'visor');
  return finish(g, 'warden', { eyes: [visor], visor, head, conePivot, legs, plinth, halberd: named(torso, 'halberd'), spotY: 2.28 });
}

// drowner() — the thing under the surface: a low black crocodile shape 0.55 tall × 1.8 long (head toward −Z); 12
// boxes + a ripple ring. The group origin is the WATER SURFACE: `body` (every box; its belly at body.position.y = 0)
// is what the AI sinks (−0.7 submerged) and raises (−0.15 surfaced); `ripple` stays on the surface (y 0.02).
// userData: eyes[2] (0x40ff9a), jaw (pivot at the hinge: rotation.x to gape), body, ripple, ridges[3],
// update(t) → pulses the ripple scale 0.6 → 1.4 over 1.2 s (fading out) while ripple.visible; wake(k) → a 0.5 s
// wake: sets the ripple to a tight bright ring scaled by k (0 → 1) for SURGE.
export function drowner(opts = {}) {
  const hide = 0x06080a, ridge = 0x102028, eyeK = opts.eyeIntensity != null ? opts.eyeIntensity : 0;
  const g = new THREE.Group();
  const body = new THREE.Group();
  const bb = build([
    box(0, 0, 0, 0.6, 0.3, 0.8, hide, 'trunk'),
    box(0, 0.12, -0.65, 0.4, 0.25, 0.5, hide, 'head'),
    glow(-0.13, 0.32, -0.8, 0.05, 0.05, 0.05, 0x40ff9a, eyeK, 'eyeL'), glow(0.13, 0.32, -0.8, 0.05, 0.05, 0.05, 0x40ff9a, eyeK, 'eyeR'),
    box(0, 0.3, -0.25, 0.16, 0.25, 0.2, ridge, 'ridge0'), box(0, 0.3, 0.0, 0.14, 0.2, 0.2, ridge, 'ridge1'), box(0, 0.3, 0.25, 0.12, 0.16, 0.2, ridge, 'ridge2'),
    box(0, 0.04, 0.6, 0.3, 0.18, 0.4, hide, 'tailRoot'), box(0, 0.06, 0.85, 0.16, 0.12, 0.2, ridge, 'tailTip'),
    box(-0.38, 0, -0.35, 0.16, 0.14, 0.3, hide, 'limbL'), box(0.38, 0, -0.35, 0.16, 0.14, 0.3, hide, 'limbR'),
  ], { jitter: 0 });
  body.add(bb);
  const jaw = pivot(0, 0.12, -0.42);
  jaw.add(build([box(0, -0.06, -0.24, 0.36, 0.08, 0.46, 0x0c1216, 'jaw')], { jitter: 0 }));
  body.add(jaw);
  g.add(body);
  const rip = ripple(); rip.position.y = 0.02; rip.name = 'ripple'; g.add(rip);
  const wakeT = { k: 0 };
  const update = (t) => {
    if (!rip.visible) return;
    if (wakeT.k > 0) { rip.scale.setScalar(0.5 + 0.3 * wakeT.k); rip.material.opacity = 0.9; rip.material.emissiveIntensity = 0.8; return; }
    const ph = (t % 1.2) / 1.2;
    rip.scale.setScalar(0.6 + 0.8 * ph);
    rip.material.opacity = 0.9 * (1 - ph * ph);
    rip.material.emissiveIntensity = 0.4;
  };
  const wake = (k) => { wakeT.k = Math.max(0, Math.min(1, k || 0)); };
  return finish(g, 'drowner', { eyes: [named(bb, 'eyeL'), named(bb, 'eyeR')], jaw, body, ripple: rip,
    ridges: [0, 1, 2].map(i => named(bb, `ridge${i}`)), update, wake, length: 1.8 });
}

// falseLight() — the lantern that isn't: the planted lantern's exact silhouette (1.6 tall, wood 0x3a2a1a / iron
// 0x2a2420) standing on two stilt legs that read as the pole while they are together; 16 boxes.
// userData: glass (the lure, 0xffc070 k 1.0 — flicker it like a lantern's; k 0 from DARK on), lightY 1.28 (put the
// PointLight here), eyes[2] (0xff2020; hidden (visible = false) while LIT so the glass stays a clean lantern glass),
// jaw (pivot: rotation.x 0.9 = dropped), legs[2] (pivots at the tray: rotation.z ±0.25 = splayed), feet[2],
// setDark(bool) → the pose switch: legs splayed + jaw dropped + glass off *and cold* + eyes shown/hidden (LIT ↔ DARK;
// the caller still drives eyes[i].material.emissiveIntensity per state), update(t) → the lantern glass flicker.
export function falseLight(opts = {}) {
  const wood = 0x3a2a1a, iron = 0x2a2420, glassK = opts.glassIntensity != null ? opts.glassIntensity : 1.0;
  const g = new THREE.Group();
  const legs = [], feet = [];
  for (const s of [-1, 1]) {
    const p = pivot(s * 0.02, 1.08, 0);
    const lb = build([box(0, -1.05, 0, 0.06, 1.05, 0.06, wood, 'leg'), box(s * 0.055, -1.08, 0, 0.15, 0.05, 0.3, wood, 'foot')], { jitter: 0 });
    p.add(lb); g.add(p); legs.push(p); feet.push(named(lb, 'foot'));
  }
  const cage = [box(0, 1.08, 0, 0.3, 0.04, 0.3, iron, 'tray')];
  for (const [x, z] of [[-0.12, -0.12], [0.12, -0.12], [-0.12, 0.12], [0.12, 0.12]]) cage.push(box(x, 1.1, z, 0.04, 0.36, 0.04, iron));
  cage.push(emissive(box(0, 1.14, 0, 0.22, 0.26, 0.22, 0xffc070, 'glass'), 0xffc070, glassK),
    box(0, 1.46, 0, 0.3, 0.06, 0.3, iron, 'cap'), box(0, 1.52, 0, 0.12, 0.05, 0.12, iron), box(0, 1.57, 0, 0.04, 0.08, 0.04, iron, 'hook'),
    glow(-0.05, 1.3, -0.12, 0.05, 0.05, 0.03, 0xff2020, 0, 'eyeL'), glow(0.05, 1.3, -0.12, 0.05, 0.05, 0.03, 0xff2020, 0, 'eyeR'));
  const cb = build(cage, { jitter: 0 }); g.add(cb);
  const jaw = pivot(0, 1.14, -0.11);
  jaw.add(build([box(0, -0.05, -0.02, 0.16, 0.05, 0.06, 0x2a2420, 'jaw')], { jitter: 0 }));
  g.add(jaw);
  const glass = named(cb, 'glass'), eyes = [named(cb, 'eyeL'), named(cb, 'eyeR')];
  for (const e of eyes) e.visible = false;   // an unlit eye box would show as a dark square on the glowing glass
  const setDark = (dark) => {
    legs[0].rotation.z = dark ? 0.25 : 0; legs[1].rotation.z = dark ? -0.25 : 0;
    jaw.rotation.x = dark ? 0.9 : 0;
    glass.material.emissiveIntensity = dark ? 0 : glassK;
    glass.material.color.set(dark ? 0x1c1610 : 0xffc070);   // the glass goes cold, not just unlit
    for (const e of eyes) { e.visible = !!dark; e.material.emissiveIntensity = dark ? 1.0 : 0; }
  };
  const update = (t) => { if (glass.material.emissiveIntensity > 0) glass.material.emissiveIntensity = glassK * (0.93 + 0.07 * Math.sin(13 * t)); };
  return finish(g, 'falseLight', { glass, lightY: 1.28, eyes, jaw, legs, feet, setDark, update });
}

// brute() — the wall that walks: 2.6 tall, 1.3 wide, hide 0x141210 with lighter plates 0x2a2420 and knuckles
// 0x3a3028, boulder shoulders and a head sunk between them, two tusks; 20 boxes.
// userData: eyes[2] (0xff6a20), legs[2] (hip pivots, y 1.12: rotation.x for the stride), feet[2] (the foot meshes
// inside the leg pivots — dip/lift them for the stomp bob), body (everything above the hips: sway it ±0.06 u per
// stride via body.position.x, or rotation.z), fists[2], tusks[2], plates[3].
export function brute(opts = {}) {
  const hide = 0x141210, plate = 0x2a2420, knuckle = 0x3a3028, tusk = 0x5a5040, eyeK = opts.eyeIntensity != null ? opts.eyeIntensity : 0.3;
  const g = new THREE.Group();
  const legs = [], feet = [];
  for (const s of [-1, 1]) {
    const p = pivot(s * 0.28, 1.12, 0);
    const lb = build([box(0, -1.0, 0, 0.32, 1.0, 0.36, hide, 'leg'), box(0, -1.12, -0.04, 0.4, 0.12, 0.5, plate, 'foot')], { jitter: 0 });
    p.add(lb); g.add(p); legs.push(p); feet.push(named(lb, 'foot'));
  }
  const body = new THREE.Group();
  const bb = build([
    box(0, 1.0, 0, 0.76, 0.32, 0.5, hide, 'pelvis'),
    box(0, 1.3, 0, 1.0, 1.0, 0.6, hide, 'torso'),
    box(-0.425, 2.0, 0.02, 0.45, 0.45, 0.5, plate, 'shoulderL'), box(0.425, 2.0, 0.02, 0.45, 0.45, 0.5, plate, 'shoulderR'),
    box(-0.5, 0.9, 0.02, 0.28, 1.3, 0.28, hide, 'armL'), box(0.5, 0.9, 0.02, 0.28, 1.3, 0.28, hide, 'armR'),
    box(-0.5, 0.58, 0.0, 0.34, 0.34, 0.34, knuckle, 'fistL'), box(0.5, 0.58, 0.0, 0.34, 0.34, 0.34, knuckle, 'fistR'),
    box(0, 2.16, -0.06, 0.36, 0.36, 0.36, hide, 'head'),
    glow(-0.09, 2.36, -0.25, 0.06, 0.05, 0.04, 0xff6a20, eyeK, 'eyeL'), glow(0.09, 2.36, -0.25, 0.06, 0.05, 0.04, 0xff6a20, eyeK, 'eyeR'),
    box(-0.13, 2.08, -0.26, 0.08, 0.2, 0.1, tusk, 'tuskL'), box(0.13, 2.08, -0.26, 0.08, 0.2, 0.1, tusk, 'tuskR'),
    box(0, 2.3, 0.22, 0.6, 0.3, 0.16, plate, 'plate0'), box(0, 2.1, 0.32, 0.5, 0.24, 0.12, plate, 'plate1'), box(0, 1.9, 0.36, 0.4, 0.2, 0.1, plate, 'plate2'),
  ], { jitter: 0 });
  body.add(bb); g.add(body);
  return finish(g, 'brute', { eyes: [named(bb, 'eyeL'), named(bb, 'eyeR')], legs, feet, body,
    fists: [named(bb, 'fistL'), named(bb, 'fistR')], tusks: [named(bb, 'tuskL'), named(bb, 'tuskR')], plates: [0, 1, 2].map(i => named(bb, `plate${i}`)) });
}

/* ---------- short-lived bursts (the Brute's lantern smash) ---------- */
// Both return a Group placed by the caller (feet origin) with userData.step(dt) → true while alive (false once
// `life` has elapsed: remove + disposeModel it), userData.reset() → restart, userData.done, userData.life.
// lanternDebris() — 8 boxes (wood splinters, iron ribs, two glowing glass shards) thrown out and up, falling under
// gravity, fading over 0.9 s.
export function lanternDebris() {
  const pieces = [
    box(0, 0.9, 0, 0.06, 0.4, 0.06, 0x3a2a1a), box(0, 1.0, 0, 0.06, 0.3, 0.06, 0x3a2a1a), box(0, 1.1, 0, 0.3, 0.04, 0.3, 0x2a2420),
    box(0, 1.2, 0, 0.04, 0.36, 0.04, 0x2a2420), box(0, 1.2, 0, 0.04, 0.3, 0.04, 0x2a2420), box(0, 1.45, 0, 0.3, 0.06, 0.3, 0x2a2420),
    emissive(box(0, 1.2, 0, 0.12, 0.12, 0.08, 0xffc070), 0xffc070, 1.0), emissive(box(0, 1.25, 0, 0.1, 0.08, 0.1, 0xffc070), 0xffc070, 1.0),
  ];
  const g = build(pieces, { jitter: 0 });
  const life = 0.9, D = { t: 0, done: false, v: [] };
  const seed = () => {
    D.t = 0; D.done = false; D.v = [];
    g.children.forEach((m, i) => {
      const a = i / g.children.length * TAU + Math.random() * 0.8, s = 1.6 + Math.random() * 1.6;
      m.position.set(0, pieces[i].y + pieces[i].h / 2, 0); m.rotation.set(0, 0, 0); m.visible = true;
      m.material.transparent = true; m.material.opacity = 1;
      D.v.push({ x: Math.cos(a) * s, y: 2.2 + Math.random() * 1.8, z: Math.sin(a) * s, rx: (Math.random() - 0.5) * 12, rz: (Math.random() - 0.5) * 12 });
    });
  };
  const step = (dt) => {
    if (D.done) return false;
    D.t += dt;
    const fade = Math.max(0, 1 - D.t / life);
    g.children.forEach((m, i) => {
      const v = D.v[i]; v.y -= 9.8 * dt;
      m.position.x += v.x * dt; m.position.y += v.y * dt; m.position.z += v.z * dt;
      if (m.position.y < 0.03) { m.position.y = 0.03; v.y = -v.y * 0.25; v.x *= 0.6; v.z *= 0.6; }
      m.rotation.x += v.rx * dt; m.rotation.z += v.rz * dt;
      m.material.opacity = fade;
    });
    if (D.t >= life) { D.done = true; return false; }
    return true;
  };
  seed();
  finish(g, 'lanternDebris', { step, reset: seed, life });
  Object.defineProperty(g.userData, 'done', { get: () => D.done, enumerable: true });
  return g;
}
// emberBurst({count=24, color=0xffa040}) — additive points rising 0.8 u/s for 0.8 s with a little drift, fading out.
// Counts as 0 boxes (a Points object). Same step/reset/done/life contract as lanternDebris.
export function emberBurst(opts = {}) {
  const n = opts.count || 24, color = opts.color != null ? opts.color : 0xffa040, life = opts.life || 0.8, rise = 0.8;
  const pos = new Float32Array(n * 3), vel = new Float32Array(n * 3);
  const geo = new THREE.BufferGeometry(); geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  const mat = new THREE.PointsMaterial({ color, size: 0.12, sizeAttenuation: true, transparent: true, opacity: 1, blending: THREE.AdditiveBlending, depthWrite: false });
  const pts = new THREE.Points(geo, mat); pts.name = 'embers'; pts.frustumCulled = false;
  const g = new THREE.Group(); g.add(pts);
  const D = { t: 0, done: false };
  const seed = () => {
    D.t = 0; D.done = false; mat.opacity = 1;
    for (let i = 0; i < n; i++) {
      const a = Math.random() * TAU, r = Math.random() * 0.25;
      pos[i * 3] = Math.cos(a) * r; pos[i * 3 + 1] = 0.9 + Math.random() * 0.5; pos[i * 3 + 2] = Math.sin(a) * r;
      vel[i * 3] = (Math.random() - 0.5) * 1.2; vel[i * 3 + 1] = rise * (0.7 + Math.random() * 0.6); vel[i * 3 + 2] = (Math.random() - 0.5) * 1.2;
    }
    geo.attributes.position.needsUpdate = true;
  };
  const step = (dt) => {
    if (D.done) return false;
    D.t += dt;
    for (let i = 0; i < n * 3; i += 3) { pos[i] += vel[i] * dt; pos[i + 1] += vel[i + 1] * dt; pos[i + 2] += vel[i + 2] * dt; vel[i] *= 0.96; vel[i + 2] *= 0.96; }
    geo.attributes.position.needsUpdate = true;
    mat.opacity = Math.max(0, 1 - D.t / life);
    if (D.t >= life) { D.done = true; return false; }
    return true;
  };
  seed();
  finish(g, 'emberBurst', { step, reset: seed, life, points: pts, dispose: () => { geo.dispose(); mat.dispose(); if (g.parent) g.parent.remove(g); } });
  Object.defineProperty(g.userData, 'done', { get: () => D.done, enumerable: true });
  return g;
}
// CREATURE_MODELS: profile → factory (hunter.js looks a record's model up here; base/fast share `hunter`).
export const CREATURE_MODELS = { base: hunter, fast: hunter, lampwight, warden, drowner, falseLight, brute };

/* ============================================================
   Registry
   ============================================================ */
export const MODELS = {
  hunter, npc, flask, relic, richRelic, quest, bundle, lantern,
  lampwight, warden, drowner, falseLight, brute, lanternDebris, emberBurst,
  stairs, elevator, gate, shortcutBarred, shortcutOpen, altar, water: waterTile,
  tram, workshop, oilPress, cartTable, shrine, board, flameBase,
  // hub props (merged into two draw calls by world.js; these factories are for models.html / makeModel)
  rug, bench, bedroll, crate, crateStack, barrel, logPile, cookpot, bookshelf, herbRail, candleCluster, hangLantern, stool,
  // NPC aliases
  lamplighter: () => npc('lamplighter'), cartographer: () => npc('cartographer'), keeper: () => npc('keeper'), deacon: () => npc('deacon'),
  // item-kind aliases matching ctx.items[].kind
  oil: flask, rich: richRelic,
};
export const MODEL_NAMES = Object.keys(MODELS);
// makeModel(name, opts) → THREE.Group (throws on an unknown name).
export function makeModel(name, opts) {
  const f = MODELS[name];
  if (!f) throw new Error(`models: unknown model "${name}"`);
  return f(opts);
}
