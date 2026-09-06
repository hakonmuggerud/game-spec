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
   Registry
   ============================================================ */
export const MODELS = {
  hunter, npc, flask, relic, richRelic, quest, bundle, lantern,
  stairs, elevator, gate, altar, water: waterTile,
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
