#!/usr/bin/env node
// export.mjs — dump the prototype's data tables to JSON (out/), the map rows to assets/data/maps/*.txt and the
// parity fixtures to assets/fixtures/*.json, by importing reference/prototype/src/*.js directly. Nothing is transcribed by
// hand: config.js, maps.js, maps/*.js, contracts.js, npc.js, hub.js, endgame.js and models.js are evaluated as ES
// modules (the bare 'three' import is redirected to node_modules/three by hooks.mjs; hub/endgame/contracts/npc only
// need `document` to exist at import time, which a tiny stub provides).
//
//   cd reference/tools/export && npm install && node export.mjs
//   (historical: the json2ron step that turned out/*.json into assets/data/*.ron was deleted when the RON became the source)
//
// JSON keys are the snake_case field names of the Rust structs in crates/undercroft-data/src (serde default
// naming), so json2ron is a plain deserialise + serialise round trip that fails loudly on any drift.
import { register } from 'node:module';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

register('./hooks.mjs', import.meta.url);

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(here, '..', '..', '..', 'undercroft');   // undercroft/ (the cargo workspace)
const PROTO = path.resolve(here, '..', '..', 'prototype', 'src');   // reference/prototype/src (read only)
const OUT = path.join(here, 'out');
const DATA = path.join(ROOT, 'assets', 'data');
const FIX = path.join(ROOT, 'assets', 'fixtures');
for (const d of [OUT, path.join(DATA, 'maps'), FIX]) fs.mkdirSync(d, { recursive: true });

// Minimal DOM stub: hub.js / npc.js / contracts.js / endgame.js touch `document` only lazily, but a few module-level
// statements (style injection, element lookups) run at import time.
const noopEl = () => ({ style: {}, classList: { add() {}, remove() {}, toggle() {} }, appendChild() {}, append() {}, remove() {},
  addEventListener() {}, removeEventListener() {}, setAttribute() {}, querySelector: () => null, querySelectorAll: () => [],
  getContext: () => null, textContent: '', innerHTML: '', hidden: true, children: [], dataset: {} });
globalThis.document = { getElementById: () => null, createElement: noopEl, head: noopEl(), body: noopEl(), querySelector: () => null,
  addEventListener() {}, removeEventListener() {} };
globalThis.window = globalThis;
globalThis.localStorage = { getItem: () => null, setItem() {}, removeItem() {} };
globalThis.requestAnimationFrame = () => 0;
// models.js jitters material colours by ±jitter·(2·random−1); with random ≡ 0.5 the jitter is exactly 0 so the walk
// below reads the authored colours. Restored after the model walk (fixtures do not use Math.random anyway).
const realRandom = Math.random;

const mod = (f) => import(pathToFileURL(path.join(PROTO, f)).href);
const CONFIG = await mod('config.js');
const MAPS = await mod('maps.js');
const CONTRACTS = await mod('contracts.js');
const NPC = await mod('npc.js');
const HUB = await mod('hub.js');
const ENDGAME = await mod('endgame.js');
Math.random = () => 0.5;
const MODELS = await mod('models.js');
const THREE = await import('three');

const writeJson = (dir, name, obj) => fs.writeFileSync(path.join(dir, name), JSON.stringify(obj, null, 1) + '\n');
const hex = (c) => (c == null ? null : c >>> 0);
const snakeKey = (k) => (/^[A-Z0-9_]+$/.test(k) ? k : k.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase());
// deep snake_case of plain objects (arrays / scalars untouched; keys that are all-caps state names are kept)
const snake = (v) => (Array.isArray(v) ? v.map(snake) : v && typeof v === 'object' ? Object.fromEntries(Object.entries(v).map(([k, x]) => [snakeKey(k), snake(x)])) : v);
const linear = (f) => ({ base: f(0), per_lap: f(1) - f(0) });
const assertEq = (a, b, what) => { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${what}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`); };

/* ============================================================
   config.json — every export of config.js
   ============================================================ */
{
  const { CFG, HUNTER, HUNTER_PROFILES, CREATURE, TIERS, SCONCE_INT, POINTS, LABEL, LIGHT_TECH, BUILD_COSTS, AUDIO, KEYS, TOOLS, HUB_WARMTH, EMBERS, HUB_BLOCK, HUB_OX, SAVE_KEY, SAVE_KEY_V1 } = CONFIG;
  const cfg = snake({ ...CFG, bandBurn: undefined, bandLamp: undefined });
  delete cfg.band_burn; delete cfg.band_lamp;
  cfg.band_burn = linear(CFG.bandBurn); cfg.band_lamp = linear(CFG.bandLamp);
  // sanity: the linear fit reproduces the JS closures at every lap
  for (let lap = 0; lap <= 5; lap++) { assertEq(+(cfg.band_burn.base + cfg.band_burn.per_lap * lap).toFixed(9), +CFG.bandBurn(lap).toFixed(9), 'bandBurn'); assertEq(+(cfg.band_lamp.base + cfg.band_lamp.per_lap * lap).toFixed(9), +CFG.bandLamp(lap).toFixed(9), 'bandLamp'); }
  const profiles = {};
  for (const [id, p] of Object.entries(HUNTER_PROFILES)) {
    profiles[id] = { speed: p.speed, eye: p.eye, catch_r: p.catchR, lose_t: p.loseT, scale_y: p.scaleY, eye_color: hex(p.eyeColor),
      kills: p.kills == null ? true : !!p.kills,
      senses: p.senses ? { lamp: p.senses.lamp, sprint: p.senses.sprint, walk: p.senses.walk, still: p.senses.still, water: p.senses.water, follower: !!p.senses.follower, proximity: p.senses.proximity == null ? null : p.senses.proximity } : null };
  }
  const creature = {
    lampwight: snake(CREATURE.lampwight),
    warden: { ...snake(CREATURE.warden), light: { ...snake(CREATURE.warden.light), color: hex(CREATURE.warden.light.color), int: CREATURE.warden.light.int } },
    drowner: { ...snake(CREATURE.drowner), drift_t: CREATURE.drowner.driftT, ripple: { ...snake(CREATURE.drowner.ripple), color: hex(CREATURE.drowner.ripple.color) } },
    false_light: { ...snake(CREATURE.falseLight), light: { ...snake(CREATURE.falseLight.light), color: hex(CREATURE.falseLight.light.color) } },
    brute: { ...snake(CREATURE.brute), step: CREATURE.brute.step, embers: { ...snake(CREATURE.brute.embers), color: hex(CREATURE.brute.embers.color) } },
  };
  const keys = Object.fromEntries(Object.entries(KEYS).map(([k, v]) => [snakeKey(k), Array.isArray(v) ? v : [v]]));
  const bc = (o) => ({ oil: o.oil == null ? null : o.oil, relics: o.relics == null ? null : o.relics, npc: o.npc == null ? null : o.npc, tier: o.tier == null ? null : o.tier });
  const config = {
    hub_ox: HUB_OX, save_key: SAVE_KEY, save_key_v1: SAVE_KEY_V1,
    cfg: { ...cfg, lamp_color: hex(CFG.lampColor) },
    hunter: snake(HUNTER),
    hunter_profiles: profiles,
    creature,
    tiers: TIERS.map(t => ({ pts: t.pts, int: t.int, dist: t.dist, msg: t.msg })),
    sconce_int: SCONCE_INT,
    points: POINTS, label: LABEL,
    light_tech: LIGHT_TECH.map(l => ({ dist_mul: l.distMul, burn_mul: l.burnMul, flash_cost: l.flashCost, lantern_cost: l.lanternCost, cost: { relics: l.cost.relics, rich: l.cost.rich } })),
    build_costs: { workshop: bc(BUILD_COSTS.workshop), press: bc(BUILD_COSTS.press), cart: bc(BUILD_COSTS.cart), shrine: bc(BUILD_COSTS.shrine), tram: bc(BUILD_COSTS.tram), elevator: bc(BUILD_COSTS.elevator),
      press_relic_oil: BUILD_COSTS.pressRelicOil, reservoir: BUILD_COSTS.reservoir, reservoir_oil: BUILD_COSTS.reservoirOil, blessing_oil: BUILD_COSTS.blessingOil },
    audio: snake(AUDIO),
    keys,
    tools: TOOLS,
    hub_warmth: { ambient: HUB_WARMTH.ambient.map(hex), fog: { color: HUB_WARMTH.fog.color.map(hex), density: HUB_WARMTH.fog.density }, lantern_glow: HUB_WARMTH.lanternGlow,
      lantern_light: { color: hex(HUB_WARMTH.lanternLight.color), int: HUB_WARMTH.lanternLight.int, dist: HUB_WARMTH.lanternLight.dist } },
    embers: snake(EMBERS),
    hub_block: { prop: HUB_BLOCK.PROP, building: HUB_BLOCK.BUILDING, npc: HUB_BLOCK.NPC, flame: HUB_BLOCK.FLAME, min_top: HUB_BLOCK.minTop, max_bottom: HUB_BLOCK.maxBottom, shrink: HUB_BLOCK.shrink, min_size: HUB_BLOCK.minSize },
    npc_cfg: snake(NPC.NPC_CFG),
    contract_cfg: snake(CONTRACTS.CONTRACT_CFG),
    hub_cfg: { ...snake(HUB.HUB_CFG), ghost_color: hex(HUB.HUB_CFG.ghostColor) },
    endgame: snake(ENDGAME.ENDGAME),
  };
  writeJson(OUT, 'config.json', config);
}

/* ============================================================
   palettes.json / zones.json / maps/*.txt
   ============================================================ */
const palette = (p) => ({ floor: hex(p.floor), wall: hex(p.wall), pillar: hex(p.pillar), ceil: hex(p.ceil), deep: hex(p.deep), water: hex(p.water),
  water_surface: hex(p.waterSurface), water_glow: hex(p.waterGlow), fog: { color: hex(p.fog.color), density: p.fog.density }, ambient: hex(p.ambient), sky: hex(p.sky) });
writeJson(OUT, 'palettes.json', Object.fromEntries(Object.entries(MAPS.PALETTES).map(([k, v]) => [k, palette(v)])));

const cell2 = (c) => [c[0], c[1]];
// ANCHORS values are [cx, cz], [[cx, cz], …] or a nested object of the same → a tagged tree
const anchor = (v) => {
  if (Array.isArray(v) && v.length === 2 && typeof v[0] === 'number') return { Cell: cell2(v) };
  if (Array.isArray(v)) return { Cells: v.map(cell2) };
  return { Group: Object.fromEntries(Object.entries(v).map(([k, x]) => [k, anchor(x)])) };
};
const zones = [];
for (const id of MAPS.ZONE_ORDER) {
  const z = MAPS.ZONES[id], F = MAPS.ZONE_FILES[id];
  assertEq(F.ID, id, 'zone id'); assertEq(F.SIZE, z.size, 'zone size'); assertEq(F.ROWS.length, z.size, 'rows');
  fs.writeFileSync(path.join(DATA, 'maps', `${id}.txt`), F.ROWS.join('\n') + '\n');
  const req = z.requires ? { building: z.requires.building || null, light_tech: z.requires.lightTech || null, tier: z.requires.tier || null, rescued: z.requires.rescued || null } : null;
  zones.push({
    id, name: z.name, size: z.size, entry: z.entry, exit: z.exit,
    burn_mul: z.burnMul, lamp_mul: z.lampMul, deep_style: z.deepStyle === 'bands' ? 'Bands' : 'Flat',
    bands: z.bands ? { band: z.bands.band, max_lap: z.bands.maxLap } : null,
    hunters: z.hunters || [],
    creatures: (z.creatures || []).map(c => ({ kind: c.kind, facing: c.facing || null, sweep: c.sweep == null ? null : c.sweep, reach: c.reach == null ? null : c.reach,
      territory: c.territory == null ? null : c.territory, leash: c.leash == null ? null : c.leash, gate_ok: !!c.gateOk })),
    npc: z.npc || null,
    npcs: Object.fromEntries(Object.entries(z.npcs || {}).map(([k, v]) => [k, cell2(v)])),
    gate: z.gate ? { tool: z.gate.tool, cells: z.gate.cells.map(cell2), opens: z.gate.opens } : null,
    spots: (z.spots || []).map(s => ({ id: s.id, cell: cell2(s.cell), label: s.label })),
    loot: { oil: z.loot.oil | 0, relic: z.loot.relic | 0, rich: z.loot.rich | 0 }, points: z.points,
    regions: z.regions.map(r => ({ id: r.id, name: r.name, x: cell2(r.x), z: cell2(r.z), deep: !!r.deep, loot: r.loot ? { oil: r.loot.oil | 0, relic: r.loot.relic | 0, rich: r.loot.rich | 0 } : null })),
    shortcuts: z.shortcuts.map(s => ({ id: s.id, name: s.name, cells: s.cells.map(cell2), open_from: s.openFrom, from: s.from, to: s.to, saves: s.saves })),
    anchors: Object.fromEntries(Object.entries(z.anchors).map(([k, v]) => [k, anchor(v)])),
    targets: { size: z.targets.size, walkable: z.targets.walkable, wall_share: z.targets.wallShare, route: z.targets.route },
    no_bank: !!z.noBank,
    requires: req, lock_reason: z.lockReason || null, ambience: z.ambience, palette: palette(z.palette), intro: z.intro, threat: z.threat,
  });
}
writeJson(OUT, 'zones.json', zones);
fs.writeFileSync(path.join(DATA, 'maps', 'hub.txt'), MAPS.HUB_ROWS_V2.join('\n') + '\n');

/* ============================================================
   contracts.json / npcs.json / buildings.json / endgame.json
   ============================================================ */
{
  const c = {};
  for (const [id, x] of Object.entries(CONTRACTS.CONTRACTS)) {
    c[id] = { id, poster: x.poster, kind: x.type, zone: x.zone, title: x.title, text: x.text,
      spot: x.spot == null ? null : x.spot, item_kind: x.kind || null, n: x.n == null ? null : x.n, seconds: x.seconds == null ? null : x.seconds,
      lamp_off: !!x.lampOff, item: x.item || null,
      reward: { tool: x.reward.tool || null, pts: x.reward.pts | 0, oil: x.reward.oil | 0 } };
  }
  writeJson(OUT, 'contracts.json', { cfg: snake(CONTRACTS.CONTRACT_CFG), order: CONTRACTS.CONTRACT_ORDER, contracts: c });
}
{
  const n = {};
  for (const [id, x] of Object.entries(NPC.NPCS)) n[id] = { id, name: x.name, short: x.short, pronoun: x.pronoun, zone: x.zone, cell: cell2(x.cell), unlocks: x.unlocks, anchor: x.anchor, coat: hex(x.coat), hat: hex(x.hat), line: x.line };
  const looks = Object.fromEntries(Object.entries(MODELS.NPC_LOOKS).map(([id, l]) => [id, { hat: l.hat, hat_color: hex(l.hatColor), coat: hex(l.coat), lamp: !!l.lamp, apron: l.apron == null ? null : hex(l.apron) }]));
  writeJson(OUT, 'npcs.json', { cfg: snake(NPC.NPC_CFG), order: Object.keys(NPC.NPCS), npcs: n, looks });
}
{
  const b = {};
  for (const [id, x] of Object.entries(HUB.BUILDINGS)) b[id] = { id, name: x.name, anchor: x.anchor, npc: x.npc || null, tier: x.tier == null ? null : x.tier, model: x.model, always: !!x.always, rot: x.rot || 0, dx: x.dx || 0, dz: x.dz || 0, desc: x.desc };
  writeJson(OUT, 'buildings.json', { cfg: { ...snake(HUB.HUB_CFG), ghost_color: hex(HUB.HUB_CFG.ghostColor) }, order: HUB.BUILD_ORDER, buildings: b });
}
{
  // ENDINGS: `available()` and `lines()` are closures in endgame.js. The templates below are hand-written but VERIFIED
  // here against lines() for every combination that changes the output (tier 1/4 × 0/1/3 names), so a drift in the
  // JS fails the export. Line variants: {Fixed}, {Tier: min_tier, met, unmet}, {Names: with, without}; `{names}` is
  // the Oxford-less list ("A, B and C"), `{count}` its length.
  const E = ENDGAME.ENDINGS, EG = ENDGAME.ENDGAME;
  const T = {
    cage: [
      { Fixed: 'You pour everything you carried into the bowl, and the Source takes it the way the great flame always has: greedily, and without thanks.' },
      { Tier: { min_tier: 4, met: 'Far above, the Last Lantern blazes white. Every alcove of the vault is lit; the dark backs off to the edges of the map and waits there.',
        unmet: 'Far above, the Last Lantern flares — brighter than it has been in years, if not as bright as it could have been. And it will need feeding again.' } },
      { Names: { with: 'Those you brought up keep the watch with you: {names}. Lamps trimmed, doors barred, eyes on the stairs.', without: 'No one keeps the watch with you. The vault is lit, and it is empty.' } },
      { Fixed: 'The dark is held. It is not healed. That was never on offer.' },
    ],
    dawn: [
      { Names: { with: "Deacon Maud's ritual, spoken by {count} voices where one would not do: {names} carry the Source up the spiral, lap by lap, and the dark parts before it.", without: null } },
      { Fixed: 'The old flame and the new one meet in the vault. What burns there afterward is neither — it is the Lantern Eternal, and it does not need oil.' },
      { Fixed: 'The alcoves fill with people. Then the halls below them. The Undercroft becomes a town with a very good cellar.' },
      { Fixed: 'You hang your handlamp on a hook by the door. You do not think you will need it again.' },
    ],
    night: [
      { Fixed: 'You reach into the bowl and pinch the Source out like a wick.' },
      { Fixed: 'The spiral goes black. Then the vault above it: the great flame gutters, flares once, and is gone. Far away, something enormous exhales.' },
      { Names: { with: 'You walk out through the long night with {names} at your side. They know the way; they always did.', without: 'You walk out through the long night alone. There is no one left to walk it with you.' } },
      { Fixed: 'The dark is free. It was never the enemy — only hungry, like everything else.' },
    ],
  };
  const list = (names) => names.length <= 1 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
  const render = (line, tier, names) => {
    if (line.Fixed != null) return line.Fixed;
    if (line.Tier) return tier >= line.Tier.min_tier ? line.Tier.met : line.Tier.unmet;
    const t = names.length || line.Names.without == null ? line.Names.with : line.Names.without;
    return t.replace('{names}', list(names)).replace('{count}', String(names.length));
  };
  for (const id of ENDGAME.ENDING_ORDER) for (const tier of [1, 4]) for (const names of [[], ['Wick the Lamplighter'], ['A', 'B', 'C']]) {
    assertEq(T[id].map(l => render(l, tier, names)), E[id].lines({ tier, names, rescued: names.length }), `ending ${id} lines(tier ${tier}, ${names.length} names)`);
  }
  // available(): cage/night always; dawn needs tier ≥ dawnTier and rescued ≥ dawnRescued — verified here too
  assertEq(E.cage.available({ tier: 1, rescued: 0 }), { ok: true }, 'cage available');
  assertEq(E.night.available({ tier: 1, rescued: 0 }), { ok: true }, 'night available');
  assertEq(E.dawn.available({ tier: 4, rescued: 3 }), { ok: true }, 'dawn available');
  assertEq(E.dawn.available({ tier: 3, rescued: 1 }), { ok: false, why: `Needs flame tier ${EG.dawnTier} (now 3) and ${EG.dawnRescued} rescued (now 1)` }, 'dawn unavailable');
  const endings = ENDGAME.ENDING_ORDER.map(id => ({ id, title: E[id].title, choice: E[id].choice, key: E[id].key, mark: E[id].mark,
    requires: id === 'dawn' ? { tier: EG.dawnTier, rescued: EG.dawnRescued } : null, lines: T[id] }));
  // LAP_LINES is a module-private const in endgame.js: lift the array literal out of the source text (it is a plain
  // string array, so evaluating it is safe) rather than transcribing it.
  const src = fs.readFileSync(path.join(PROTO, 'endgame.js'), 'utf8');
  const lapSrc = src.match(/const LAP_LINES = (\[[\s\S]*?\]);/);
  if (!lapSrc) throw new Error('endgame.js: LAP_LINES not found');
  const lapLines = new Function(`return ${lapSrc[1]};`)();
  if (!Array.isArray(lapLines) || lapLines.length !== 6 || lapLines[0] !== null) throw new Error('endgame.js: LAP_LINES shape changed');
  writeJson(OUT, 'endgame.json', { cfg: snake(EG), lap_lines: lapLines, endings });
}

/* ============================================================
   models.json — walk every MODELS factory's THREE.Group; PROP_BOXES straight from the box lists
   ============================================================ */
{
  const boxOf = (b, part) => ({ x: b.x, y: b.y, z: b.z, w: b.w, h: b.h, d: b.d, color: hex(b.color), emissive: b.emissive == null ? null : hex(b.emissive), emissive_k: b.emissive == null ? 0 : (b.k == null ? 1 : b.k),
    name: b.name || null, ry: b.ry || 0, part: part || null, hidden: false });
  const props = Object.fromEntries(Object.entries(MODELS.PROP_BOXES).map(([k, f]) => [k, f().map(b => boxOf(b, null))]));
  const near = (v, eps = 1e-9) => Math.abs(v) < eps;
  const identity = (o) => near(o.position.x) && near(o.position.y) && near(o.position.z) && near(o.rotation.x) && near(o.rotation.y) && near(o.rotation.z) && near(o.scale.x - 1) && near(o.scale.y - 1) && near(o.scale.z - 1);
  const walk = (name, g) => {
    g.updateMatrixWorld(true);
    // names for the animation pivots come from the factory's userData handles (upper, body, head, legs[], jaw, fire, cart …)
    const handle = new Map();
    for (const [k, v] of Object.entries(g.userData)) {
      if (v && v.isObject3D && !v.isMesh && v !== g) handle.set(v, k);
      else if (Array.isArray(v)) v.forEach((o, i) => { if (o && o.isObject3D && !o.isMesh) handle.set(o, `${k}${i}`); });
    }
    const parts = [], boxes = [], extras = [];
    let anon = 0;
    const rec = (o, part) => {
      let cur = part;
      if (o !== g && !o.isMesh && (handle.has(o) || !identity(o))) {
        const pname = handle.get(o) || o.name || `group${anon++}`;
        parts.push({ name: pname, parent: part, pivot: [o.position.x, o.position.y, o.position.z], rotation: [o.rotation.x, o.rotation.y, o.rotation.z], scale: [o.scale.x, o.scale.y, o.scale.z] });
        cur = pname;
      }
      if (o.isMesh) {
        const p = o.geometry && o.geometry.parameters;
        if (o.geometry && o.geometry.type === 'BoxGeometry' && p && !o.isInstancedMesh) {
          const m = o.material, em = m.emissive ? m.emissive.getHex() : 0;
          const hasEm = em !== 0 || (m.emissiveIntensity != null && m.emissiveIntensity !== 1 && em !== 0);
          boxes.push({ x: o.position.x, y: o.position.y - p.height / 2, z: o.position.z, w: p.width, h: p.height, d: p.depth,
            color: m.color.getHex(), emissive: hasEm ? em : null, emissive_k: hasEm ? m.emissiveIntensity : 0,
            name: o.name || null, ry: o.rotation.y || 0, part: cur, hidden: !o.visible });
        } else extras.push(o.name || o.type);
      }
      for (const c of o.children) rec(c, cur);
    };
    rec(g, null);
    return { name, height: g.userData.height, box_count: g.userData.boxes | 0, scale: [g.scale.x, g.scale.y, g.scale.z], parts, boxes, extras };
  };
  const models = [];
  for (const name of MODELS.MODEL_NAMES) {
    if (name === 'oil' || name === 'rich') continue;   // pure aliases of flask / richRelic
    models.push(walk(name, MODELS.makeModel(name)));
  }
  models.push(walk('hunterFast', MODELS.hunter({ profile: 'fast' })));
  writeJson(OUT, 'models.json', { models, props });
}
Math.random = realRandom;

/* ============================================================
   Parity fixtures (assets/fixtures/*.json)
   ============================================================ */
const T_NAME = MAPS.T_NAME;
const flatAnchors = (a, prefix = '') => {
  const out = [];
  for (const [k, v] of Object.entries(a)) {
    if (Array.isArray(v) && v.length === 2 && typeof v[0] === 'number') out.push([prefix + k, v]);
    else if (Array.isArray(v)) v.forEach((c, i) => out.push([`${prefix}${k}[${i}]`, c]));
    else out.push(...flatAnchors(v, `${prefix}${k}.`));
  }
  return out;
};
function fixtureFor(id, m, anchors, bands) {
  const counts = {};
  for (let i = 0; i < m.cells.length; i++) counts[T_NAME[m.cells[i]]] = (counts[T_NAME[m.cells[i]]] | 0) + 1;
  let walkable = 0;
  for (let i = 0; i < m.cells.length; i++) if (!MAPS.isSolid(m, i % m.w, (i / m.w) | 0)) walkable++;
  const entry = m.stairs;
  const field = MAPS.bfsField(m, entry.cx, entry.cz);           // isBlocked over an empty pool = isSolid
  const named = anchors.filter(([, c]) => MAPS.inBounds(m, c[0], c[1]));
  const paths = [];
  for (const [k, c] of named.slice(0, 12)) {
    const ti = MAPS.idx(m, c[0], c[1]);
    paths.push({ from: 'entry', to: k, target: [c[0], c[1]], cells: MAPS.pathTo(m, field, ti).map(p => [Math.floor(p.x - m.ox), Math.floor(p.z)]) });
  }
  // a few paths from other anchors too (tie-breaking exercise)
  for (let i = 1; i < Math.min(named.length, 6); i++) {
    const [ka, a] = named[i], [kb, b] = named[(i * 3) % named.length];
    const f = MAPS.bfsField(m, a[0], a[1]);
    paths.push({ from: ka, to: kb, target: [b[0], b[1]], cells: MAPS.pathTo(m, f, MAPS.idx(m, b[0], b[1])).map(p => [Math.floor(p.x - m.ox), Math.floor(p.z)]) });
  }
  const los = [];
  for (let i = 0; i < named.length; i++) for (let j = i + 1; j < named.length && los.length < 60; j += 1) {
    const [ka, a] = named[i], [kb, b] = named[j];
    const ax = m.ox + a[0] + 0.5, az = a[1] + 0.5, bx = m.ox + b[0] + 0.5, bz = b[1] + 0.5;
    los.push({ from: ka, to: kb, a: [ax, az], b: [bx, bz], ab: MAPS.los(m, ax, az, bx, bz), ba: MAPS.los(m, bx, bz, ax, az) });
  }
  // off-centre LOS probes (the DDA's fractional start matters)
  for (let i = 0; i < Math.min(named.length, 10); i++) {
    const [ka, a] = named[i], [kb, b] = named[(i + 7) % named.length];
    const ax = m.ox + a[0] + 0.13, az = a[1] + 0.87, bx = m.ox + b[0] + 0.71, bz = b[1] + 0.29;
    los.push({ from: ka + '+', to: kb + '+', a: [ax, az], b: [bx, bz], ab: MAPS.los(m, ax, az, bx, bz), ba: MAPS.los(m, bx, bz, ax, az) });
  }
  const nearest = [];
  for (const [wx, wz] of [[0.5, 0.5], [m.ox + m.w / 2, m.h / 2], [m.ox + 3.3, 7.7], [m.ox + m.w - 1.5, m.h - 1.5], [m.ox + 10.1, 10.9]]) {
    nearest.push({ at: [wx, wz], idx: MAPS.nearestReachable(m, field, wx, wz) });
  }
  const laps = bands ? Array.from({ length: m.w * m.h }, (_, i) => MAPS.lapOf(i % m.w, (i / m.w) | 0, m)) : null;
  const rc = m.stairs ? { closed: MAPS.routeCells(m, { gatesOpen: true, scOpen: false }), open: MAPS.routeCells(m, { gatesOpen: true, scOpen: true }), gates_closed: MAPS.routeCells(m, { gatesOpen: false, scOpen: false }) } : null;
  return { id, w: m.w, h: m.h, ox: m.ox, entry: [entry.cx, entry.cz], cells: Array.from(m.cells), counts, walkable, bfs_dist: Array.from(field.dist), bfs_parent: Array.from(field.parent),
    paths, los, nearest, laps, route: rc,
    markers: { items: m.items, stairs: m.stairs, hunter_spawns: m.hunterSpawns, npc_cells: m.npcCells, spots: m.spots, gates: m.gates.map(({ mesh, ...g }) => g), shortcuts: m.shortcuts.map(({ mesh, ...s }) => s), flame: m.flame, anchors: m.anchors, altar: m.altar, creatures: m.creatures } };
}
for (const id of MAPS.ZONE_ORDER) {
  const z = MAPS.ZONES[id], m = MAPS.parseZone(id);
  writeJson(FIX, `${id}.json`, fixtureFor(id, m, flatAnchors(z.anchors), z.deepStyle === 'bands'));
}
{
  const m = MAPS.parseHub();
  const anchors = [['entry', [m.stairs.cx, m.stairs.cz]], ['flame', [m.flame.cx, m.flame.cz]], ...Object.entries(m.anchors).map(([k, v]) => [`anchor${k}`, [v.cx, v.cz]])];
  writeJson(FIX, 'hub.json', fixtureFor('hub', m, anchors, false));
  const m1 = MAPS.parseHub(MAPS.HUB_ROWS);
  const f1 = fixtureFor('hub_v1', m1, [['entry', [m1.stairs.cx, m1.stairs.cz]], ['flame', [m1.flame.cx, m1.flame.cz]]], false);
  f1.rows = MAPS.HUB_ROWS;
  writeJson(FIX, 'hub_v1.json', f1);
}
// lapOf on the legacy 40×40 fallback (no map): the JS default path
writeJson(FIX, 'lap_legacy.json', { size: 40, band: 3, max_lap: 5, laps: Array.from({ length: 1600 }, (_, i) => MAPS.lapOf(i % 40, (i / 40) | 0)) });
const va = MAPS.validateAll();
writeJson(FIX, 'validate_all.json', va);
console.log(`exported ${MAPS.ZONE_ORDER.length} zones, validateAll ok=${va.ok} (${va.errors.length} errors, ${va.warnings.length} warnings)`);
