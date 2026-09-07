// maps.js — legend, parsing, pure grid helpers, validation and the ZONES assembly (DESIGN.md §3, §3.7).
// The ASCII rows and every map-shaped fact live in one file per zone under src/maps/ (one author each); this file
// owns everything that is not map-shaped: the legend, the parser, the grid helpers, PALETTES, the per-zone TUNING
// (requires / lockReason / intro / threat / ambience) and the hub rows.
// Pure data + pure functions over a parsed map: no THREE, no ctx. Any module may import this file.
// Coordinates everywhere are (x = column, z = row); `idx = z * w + x`.
import { HUB_OX, HUNTER_PROFILES, TOOLS } from './config.js';
import * as UNDERCROFT from './maps/undercroft.js';
import * as CISTERN from './maps/cistern.js';
import * as OSSUARY from './maps/ossuary.js';
import * as SOURCE from './maps/source.js';

// Cell types stored in map.cells.
export const T = { FLOOR: 0, WALL: 1, PILLAR: 2, DEEP: 3, STAIRS: 4, WATER: 5, GATE: 6, ELEVATOR: 7, ALTAR: 8, SHORTCUT: 9 };
export const T_NAME = ['floor', 'wall', 'pillar', 'deep', 'stairs', 'water', 'gate', 'elevator', 'altar', 'shortcut'];

// Legend (DESIGN.md §3.1): char → what the parser does with it.
export const LEGEND = {
  '#': 'wall', 'P': 'pillar', '.': 'floor', 'D': 'deep', 'S': 'stairs', 'V': 'elevator',
  'o': 'item:oil', 'r': 'item:relic', 'R': 'deep+item:rich', 'H': 'hunter', 'F': 'flame',
  'N': 'npc', 'C': 'spot', 'W': 'water', 'X': 'gate', '=': 'shortcut', 'A': 'altar', '0-9': 'anchor (hub building)',
  // DESIGN.md §5.7 — creature spawns (L G Y B keep the cell floor/deep like H; w is a water cell with a spawn)
  'L': 'creature:lampwight', 'G': 'creature:warden', 'Y': 'creature:falseLight', 'B': 'creature:brute', 'w': 'water+creature:drowner',
};
const LEGEND_CHARS = new Set(['#', 'P', '.', 'D', 'S', 'V', 'o', 'r', 'R', 'H', 'F', 'N', 'C', 'W', 'X', '=', 'A', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
  'L', 'G', 'Y', 'B', 'w']);
// creature legend char → hunter.js profile name (the parser stores the profile as `kind`)
export const CREATURE_CHARS = { L: 'lampwight', G: 'warden', Y: 'falseLight', B: 'brute', w: 'drowner' };

/* ============================================================
   Hub rows
   ============================================================ */
// v1 hub (17×9) — kept for reference/tests; parseHub() now defaults to HUB_ROWS_V2 (spawn 70.5,10.5, flame 71.5,1.5).
// (v1 spawn was 68.5,7.5 — pass HUB_ROWS explicitly to rebuild the old layout.)
export const HUB_ROWS = [
  '#################',
  '#....#.....#....#',
  '#....#.....#....#',
  '#.......F.......#',
  '#....#.....#....#',
  '#....#.....#....#',
  '######.....######',
  '######..S..######',
  '#################',
];
// v2 hub (25×13, DESIGN.md §3 hub): digits are building anchors (floor for collision). Rooms: nave x7-15 with the
// flame against the north wall, stairs room x6-14 z10-11, W/E alcoves x1-5 / x17-23 z6-9 (2-wide mouths at x6 / x16,
// z6-7), NW/NE corner rooms z1-4 entered from the alcoves through 1-wide doors at (1,5) / (23,5). Every building
// stands against a wall or in an alcove back; rescued NPCs stand at anchor +1 x, beside their building.
export const HUB_ROWS_V2 = [
  '#########################',
  '#..1..#....F....#..2....#',
  '#.....#.........#.......#',
  '#.....#.........#.......#',
  '#..3..#.........#....4..#',
  '#.#####.........#######.#',
  '#.......................#',
  '#.......................#',
  '#.....#.........#.......#',
  '#.6...#........5#.....7.#',
  '######....S....##########',
  '######.........##########',
  '#########################',
];

/* ============================================================
   Per-zone look (world.js reads these; every colour is a hex int)
   ============================================================ */
// floor/wall/pillar/ceil/deep/water: instanced block colours · fog: {color, density} · ambient: AmbientLight colour
// · waterSurface/waterGlow: the translucent surface mesh · sky: scene background.
export const PALETTES = {
  // warm stone; ambient/fog are the tier-1 values of config.HUB_WARMTH (world.js warms them per flame tier)
  hub:        { floor: 0x6e675e, wall: 0x5c564d, pillar: 0x7d7160, ceil: 0x4d4841, deep: 0x0c0c18, water: 0x2a2f36,
                waterSurface: 0x1a2a3a, waterGlow: 0x06101a, fog: { color: 0x050302, density: 0.10 }, ambient: 0x16100b, sky: 0x000000 },
  undercroft: { floor: 0x6e675e, wall: 0x5c564d, pillar: 0x7d7160, ceil: 0x4d4841, deep: 0x0c0c18, water: 0x2a2f36,
                waterSurface: 0x1a2a3a, waterGlow: 0x06101a, fog: { color: 0x000000, density: 0.11 }, ambient: 0x0b0a14, sky: 0x000000 },
  // slick green-grey stone, cold blue haze, black water
  cistern:    { floor: 0x56676a, wall: 0x46585c, pillar: 0x66787a, ceil: 0x384648, deep: 0x0a1018, water: 0x1c262e,
                waterSurface: 0x16303e, waterGlow: 0x0a1c2a, fog: { color: 0x02070b, density: 0.10 }, ambient: 0x0a1218, sky: 0x02070b },
  // bone-yellow dust, brown mortar, warm dry murk
  ossuary:    { floor: 0x7d6f55, wall: 0x6a5a44, pillar: 0x8d7c5e, ceil: 0x54473a, deep: 0x100c08, water: 0x2a2620,
                waterSurface: 0x1a2a3a, waterGlow: 0x06101a, fog: { color: 0x060300, density: 0.13 }, ambient: 0x120d08, sky: 0x060300 },
  // violet-black basalt, thin purple haze; the spiral darkens per lap (world.js applies lapOf)
  source:     { floor: 0x4e465e, wall: 0x3a3450, pillar: 0x5a5272, ceil: 0x2e2a40, deep: 0x0a0814, water: 0x1e1a2a,
                waterSurface: 0x1a2a3a, waterGlow: 0x06101a, fog: { color: 0x04000c, density: 0.12 }, ambient: 0x0e0a1a, sky: 0x04000c },
};

/* ============================================================
   Zone assembly: TUNING (here) + the per-zone file's META (src/maps/<id>.js)
   ============================================================ */
// Each zone file exports ID SIZE ROWS REGIONS SHORTCUTS ANCHORS META (DESIGN.md §3.7). META carries everything
// map-shaped: name, entry/exit, burnMul, lampMul, deepStyle, hunters, creatures, npc/npcs, gate, spots, loot, points,
// size, regions, shortcuts, anchors, bands?, targets. TUNING below carries what is NOT map-shaped, so a map author
// never edits this file: access rules, board text, flavour, ambience and palette.
// `hunters` lists one speed-profile NAME per `H` in row-major order (hunter.js reads the string; the resolved
// numbers are in `hunterSpeeds`). `npcs` = {id: [cx, cz]} for every N; `npc` = the captive named on the board.
// `gate` = {tool, cells: [[cx, cz]]} for the X cells. `spots` = contract spots in row-major C order (index =
// the `spot` field in contracts.js). `loot` = expected item counts (validated). `requires` = access key, and
// `lockReason` the board text when locked. `intro` is the flavour line shown on zoneEnter, `threat` the one-line
// creature warning the Departure Board prints under an unlocked zone (DESIGN.md §5.7).
const speeds = (names) => names.map(n => { const p = HUNTER_PROFILES[n] || HUNTER_PROFILES.base; return { profile: n, ...p.speed, catchR: p.catchR }; });
export const ZONE_FILES = { undercroft: UNDERCROFT, cistern: CISTERN, ossuary: OSSUARY, source: SOURCE };
export const TUNING = {
  undercroft: {
    requires: null, lockReason: null, ambience: 'undercroft', palette: PALETTES.undercroft,
    intro: 'Stone steps, wet with old dark. Something below is listening for light. A cold beam sweeps the north-east crypt, and something heavy walks the west wing.',
    threat: 'a hunter, a Warden watching the north-east crypt, a Brute in the west wing',
  },
  cistern: {
    requires: { building: 'tram' }, lockReason: 'Needs the Tram dock', ambience: 'cistern', palette: PALETTES.cistern,
    intro: 'The tram groans to a stop above black water. Every step you take in it will be heard — and something in the south hall drinks flame.',
    threat: 'two hunters, a Drowner in the lake, a Lampwight in the south hall',
  },
  ossuary: {
    requires: { building: 'elevator', lightTech: 2 }, lockReason: 'Needs the Elevator and Light-tech II', ambience: 'ossuary', palette: PALETTES.ossuary,
    intro: 'The cage opens on corridors of stacked bone. The air eats oil; the thing here is quick. Not every lantern down here is one you planted.',
    threat: 'a quick hunter, a Warden in the reliquary, two false lights among the alcoves',
  },
  source: {
    noBank: true,
    requires: { tier: 4, rescued: 'deacon' }, lockReason: 'Needs the flame at tier 4 and Deacon Maud', ambience: 'source', palette: PALETTES.source,
    intro: 'A spiral, turning inward. Each lap is darker than the last. There is no banking here — only the altar. The lights lie down here, and the last ring takes your lanterns.',
    threat: 'quick hunters that multiply as you descend, a Lampwight, a false light, a Brute on the last ring',
  },
};
export const ZONE_ORDER = ['undercroft', 'cistern', 'ossuary', 'source'];
// ZONES[id] = { id, ...TUNING[id], ...FILE.META, rows: FILE.ROWS, hunterSpeeds }
export const ZONES = {};
for (const id of ZONE_ORDER) {
  const F = ZONE_FILES[id];
  if (F.ID !== id) console.warn(`maps: src/maps/${id}.js exports ID '${F.ID}'`);
  ZONES[id] = { id, ...TUNING[id], ...F.META, rows: F.ROWS, hunterSpeeds: speeds(F.META.hunters || []) };
}

// zoneLocked(id, save, tier) → null when the zone can be entered, else the reason string (DESIGN-v2 §7 board).
export function zoneLocked(id, save, tier) {
  const z = ZONES[id]; if (!z) return 'Unknown zone';
  const r = z.requires; if (!r) return null;
  const b = (save && save.buildings) || {}, rescued = (save && save.rescued) || {};
  const missing = [];
  if (r.building && !b[r.building]) missing.push(r.building === 'tram' ? 'the Tram dock' : 'the Elevator');
  if (r.lightTech && ((save && save.lightTech) | 0) < r.lightTech) missing.push(`Light-tech ${'I'.repeat(r.lightTech)}`);
  if (r.tier && (tier | 0) < r.tier) missing.push(`flame tier ${r.tier}`);
  if (r.rescued && !rescued[r.rescued]) missing.push(r.rescued === 'deacon' ? 'Deacon Maud' : r.rescued);
  return missing.length ? `Needs ${missing.join(' and ')}` : null;
}
export const gateToolName = (id) => { const z = ZONES[id]; return z && z.gate ? (TOOLS[z.gate.tool] || z.gate.tool) : null; };

// Source bands (DESIGN.md §3.4): ring = min(cx, cz, w−1−cx, h−1−cz), lap = min(maxLap, floor((ring−1)/band)).
// `m` may be a parsed map or any {w, h, bands}: `m.bands` comes from the zone file's META.bands. Called without a
// map it falls back to the legacy 40×40 / band 3 spiral so old probes keep working.
export const LEGACY_BANDS = { size: 40, band: 3, maxLap: 5 };
export function lapOf(cx, cz, m) {
  const w = m && m.w ? m.w : LEGACY_BANDS.size, h = m && m.h ? m.h : w;
  const b = (m && m.bands) || LEGACY_BANDS, band = b.band || LEGACY_BANDS.band, maxLap = b.maxLap != null ? b.maxLap : LEGACY_BANDS.maxLap;
  return Math.min(maxLap, Math.floor((Math.min(cx, cz, w - 1 - cx, h - 1 - cz) - 1) / band));
}

/* ============================================================
   Parser
   ============================================================ */
// Does a marker cell (N, C, H, A) sit inside a deep pocket? Majority of its 4-neighbours are D/R.
function deepNeighbourhood(rows, x, z) {
  let deep = 0, open = 0;
  for (const [dx, dz] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
    const ch = (rows[z + dz] || '')[x + dx];
    if (ch === undefined || ch === '#' || ch === 'P') continue;
    open++; if (ch === 'D' || ch === 'R') deep++;
  }
  return open > 0 && deep * 2 >= open;
}

// parseMap(rows, ox, name, opts) → { name, w, h, ox, bands, cells, pool, items, stairs, hunterSpawns[], npcCells[],
//   spots[], gates[], shortcuts[], flame, anchors{}, altar, creatures[] ({kind, cx, cz, idx, x, z} row-major),
//   hunterSpawn (= hunterSpawns[0]) }
// Marker records carry {cx, cz, idx, x, z} (x/z = world centre) so consumers need no extra lookups.
// opts: {bands} → the Source lap bands (lapOf reads map.bands); {shortcuts} → the zone file's SHORTCUTS metadata,
// which is bound onto each parsed `=` record (id, name, openFrom, from, to, saves, cells = the whole door group).
export function parseMap(rows, ox, name, { bands = null, shortcuts: scMeta = null } = {}) {
  const h = rows.length, w = rows[0].length;
  for (const r of rows) if (r.length !== w) console.warn(`map ${name}: ragged row "${r}"`);
  const cells = new Uint8Array(w * h), pool = new Uint8Array(w * h);
  const items = [], hunterSpawns = [], npcCells = [], spots = [], gates = [], shortcuts = [], anchors = {}, creatures = [];
  let stairs = null, flame = null, altar = null;
  const mark = (x, z, extra) => ({ cx: x, cz: z, idx: z * w + x, x: ox + x + 0.5, z: z + 0.5, ...extra });
  for (let z = 0; z < h; z++) for (let x = 0; x < w; x++) {
    const ch = rows[z][x];
    let t = T.FLOOR;
    const deepIfPocket = () => (deepNeighbourhood(rows, x, z) ? T.DEEP : T.FLOOR);
    switch (ch) {
      case '#': t = T.WALL; break;
      case 'P': t = T.PILLAR; break;
      case 'D': t = T.DEEP; break;
      case 'W': t = T.WATER; break;
      case 'X': t = T.GATE; gates.push(mark(x, z, { open: false, mesh: null, id: null })); break;
      // `=` shortcut (DESIGN.md §3.6): barred = solid (isSolid) and sight-blocking until opened from its far side
      case '=': t = T.SHORTCUT; shortcuts.push(mark(x, z, { open: false, mesh: null, id: null })); break;
      case 'S': t = T.STAIRS; stairs = mark(x, z, { kind: 'stairs' }); break;
      case 'V': t = T.ELEVATOR; stairs = mark(x, z, { kind: 'elevator' }); break;
      case 'A': t = T.ALTAR; altar = mark(x, z, {}); break;
      case 'o': items.push({ kind: 'oil', cx: x, cz: z }); break;          // v1: floor (even inside D)
      case 'r': items.push({ kind: 'relic', cx: x, cz: z }); break;
      case 'R': t = T.DEEP; items.push({ kind: 'rich', cx: x, cz: z }); break;
      case 'H': t = deepIfPocket(); hunterSpawns.push(mark(x, z, {})); break;
      case 'N': t = deepIfPocket(); npcCells.push(mark(x, z, {})); break;
      case 'C': t = deepIfPocket(); spots.push(mark(x, z, { id: spots.length })); break;
      case 'F': flame = mark(x, z, {}); break;
      // creatures (DESIGN.md §5.7): L G Y B keep floor/deep like H; w is a water cell with a Drowner in it
      case 'L': case 'G': case 'Y': case 'B': t = deepIfPocket(); creatures.push(mark(x, z, { kind: CREATURE_CHARS[ch] })); break;
      case 'w': t = T.WATER; creatures.push(mark(x, z, { kind: 'drowner' })); break;
      default:
        if (ch >= '0' && ch <= '9') anchors[ch] = mark(x, z, {});
        break;
    }
    cells[z * w + x] = t;
  }
  const m = { name, w, h, ox, bands, cells, pool, items, stairs, hunterSpawns, npcCells, spots, gates, shortcuts, flame, anchors, altar, creatures,
    hunterSpawn: hunterSpawns[0] || null };
  bindShortcuts(m, scMeta);
  return m;
}
// bindShortcuts(map, meta): copy each SHORTCUTS entry onto the parsed `=` cells it lists (row-major). A parsed cell
// with no meta entry keeps id === null (validateMap reports it); a meta cell with no `=` is reported too.
export function bindShortcuts(m, scMeta) {
  if (!m.shortcuts.length || !Array.isArray(scMeta)) return m;
  for (const s of m.shortcuts) {
    const e = scMeta.find(o => Array.isArray(o.cells) && o.cells.some(c => c[0] === s.cx && c[1] === s.cz));
    if (!e) continue;
    s.id = e.id; s.name = e.name; s.openFrom = e.openFrom; s.from = e.from; s.to = e.to; s.saves = e.saves; s.cells = e.cells;
  }
  return m;
}
// bindGates(map, gateMeta): stable save ids for the X cells (DESIGN.md §3.6 — save.gatesOpened is keyed by id,
// never by cell index, because the grids change size). One gate cell → the tool name ('prybar'); more → 'prybar0'…
export function bindGates(m, gateMeta) {
  const tool = (gateMeta && gateMeta.tool) || 'gate';
  m.gates.forEach((g, i) => { g.id = m.gates.length > 1 ? `${tool}${i}` : tool; });
  return m;
}
export const parseZone = (id) => {
  const z = ZONES[id];
  const m = parseMap(z.rows, 0, id, { bands: z.bands, shortcuts: z.shortcuts });
  return bindGates(m, z.gate);
};
export const parseHub = (rows = HUB_ROWS_V2) => parseMap(rows, HUB_OX, 'hub');

/* ============================================================
   Pure grid helpers (world.js re-exports these; hunter/npc may import them directly)
   ============================================================ */
export const idx = (m, cx, cz) => cz * m.w + cx;
export const inBounds = (m, cx, cz) => cx >= 0 && cz >= 0 && cx < m.w && cz < m.h;
export const cellType = (m, cx, cz) => (inBounds(m, cx, cz) ? m.cells[idx(m, cx, cz)] : T.WALL);
// A barred shortcut (`=`) is solid exactly like a closed gate, so player/hunter/follower movement and `los` all
// agree with no new rule anywhere; opening it rewrites the cell to T.FLOOR (world.openShortcut).
export const isSolid = (m, cx, cz) => { const t = cellType(m, cx, cz); return t === T.WALL || t === T.PILLAR || t === T.GATE || t === T.SHORTCUT; };
export const isBlocked = (m, cx, cz) => isSolid(m, cx, cz) || m.pool[idx(m, cx, cz)] === 1;
export const isExtraction = (t) => t === T.STAIRS || t === T.ELEVATOR;
export const isWater = (m, cx, cz) => cellType(m, cx, cz) === T.WATER;
export const toCell = (m, wx, wz) => ({ cx: Math.floor(wx - m.ox), cz: Math.floor(wz) });
export const center = (m, cx, cz) => ({ x: m.ox + cx + 0.5, z: cz + 0.5 });
export const dist2d = (ax, az, bx, bz) => Math.hypot(ax - bx, az - bz);

// BFS distance/parent field over the grid (4-neighbour, blocked = solid ∪ pool unless ignorePools).
// The third argument may also be a predicate blocked(m, cx, cz) → bool (creature movement rules: territory, water body…).
export function bfsField(m, sx, sz, ignorePools = false) {
  const n = m.w * m.h;
  const dist = new Int16Array(n).fill(-1), parent = new Int32Array(n).fill(-1), q = new Int32Array(n);
  let qh = 0, qt = 0;
  const s = idx(m, sx, sz); dist[s] = 0; q[qt++] = s;
  const blocked = typeof ignorePools === 'function' ? ignorePools : (ignorePools ? isSolid : isBlocked);
  while (qh < qt) {
    const i = q[qh++], cx = i % m.w, cz = (i / m.w) | 0;
    for (const [dx, dz] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const nx = cx + dx, nz = cz + dz;
      if (!inBounds(m, nx, nz)) continue;
      const j = idx(m, nx, nz);
      if (dist[j] >= 0 || blocked(m, nx, nz)) continue;
      dist[j] = dist[i] + 1; parent[j] = i; q[qt++] = j;
    }
  }
  return { dist, parent };
}
export function pathTo(m, field, ti) {
  const out = [];
  for (let i = ti; i >= 0 && field.parent[i] >= 0; i = field.parent[i]) out.push(i);
  out.reverse();
  return out.map(i => center(m, i % m.w, (i / m.w) | 0));
}
export function nearestReachable(m, field, wx, wz) {
  let best = -1, bd = Infinity;
  for (let i = 0; i < field.dist.length; i++) {
    if (field.dist[i] < 0) continue;
    const p = center(m, i % m.w, (i / m.w) | 0), d = dist2d(p.x, p.z, wx, wz);
    if (d < bd) { bd = d; best = i; }
  }
  return best;
}
// Grid DDA line of sight (Amanatides–Woo); blocked by walls/pillars/closed gates
export function los(m, x0, z0, x1, z1) {
  let cx = Math.floor(x0 - m.ox), cz = Math.floor(z0);
  const ex = Math.floor(x1 - m.ox), ez = Math.floor(z1);
  const dx = x1 - x0, dz = z1 - z0;
  const stepX = dx > 0 ? 1 : -1, stepZ = dz > 0 ? 1 : -1;
  const tdx = dx !== 0 ? Math.abs(1 / dx) : Infinity, tdz = dz !== 0 ? Math.abs(1 / dz) : Infinity;
  const fx = x0 - m.ox - cx, fz = z0 - cz;
  let tmx = dx !== 0 ? (dx > 0 ? 1 - fx : fx) * tdx : Infinity;
  let tmz = dz !== 0 ? (dz > 0 ? 1 - fz : fz) * tdz : Infinity;
  for (let i = 0; i < 200; i++) {
    if (isSolid(m, cx, cz)) return false;
    if (cx === ex && cz === ez) return true;
    if (tmx < tmz) { cx += stepX; tmx += tdx; } else { cz += stepZ; tmz += tdz; }
  }
  return false;
}


/* ============================================================
   Validation (run headless: `node scratchpad/validate-maps.mjs`, or __game.mapsApi.validateAll() in the page)
   ============================================================ */
const DIRS4 = [[1, 0], [-1, 0], [0, 1], [0, -1]];
const DIR_OF = { N: [0, -1], S: [0, 1], E: [1, 0], W: [-1, 0] };
const solidType = (t) => t === T.WALL || t === T.PILLAR;
// blocked() predicate for flood/bfsField: `gatesOpen` decides whether X cells are passable, `scOpen` whether `=` are.
export const passPred = (gatesOpen, scOpen) => (m, cx, cz) => {
  const t = cellType(m, cx, cz);
  return solidType(t) || (t === T.GATE && !gatesOpen) || (t === T.SHORTCUT && !scOpen);
};
function flood(m, sx, sz, gatesOpen, scOpen = false) {
  const blocked = passPred(gatesOpen, scOpen);
  const seen = new Uint8Array(m.w * m.h), q = [idx(m, sx, sz)];
  seen[q[0]] = 1;
  while (q.length) {
    const i = q.pop(), cx = i % m.w, cz = (i / m.w) | 0;
    for (const [dx, dz] of DIRS4) {
      const nx = cx + dx, nz = cz + dz;
      if (!inBounds(m, nx, nz)) continue;
      const j = idx(m, nx, nz);
      if (seen[j] || blocked(m, nx, nz)) continue;
      seen[j] = 1; q.push(j);
    }
  }
  return seen;
}
const distField = (m, cx, cz, gatesOpen, scOpen = false) => bfsField(m, cx, cz, passPred(gatesOpen, scOpen));
// routeCells(m, {gatesOpen, scOpen}) → {cells, unreachable}: the greedy nearest-first full-clear walk
// entry → every item / N / C → entry (DESIGN.md §3.3 "route cells").
export function routeCells(m, { gatesOpen = true, scOpen = false } = {}) {
  if (!m.stairs) return { cells: 0, unreachable: 0 };
  const blocked = passPred(gatesOpen, scOpen);
  const left = [...m.items.map(i => ({ cx: i.cx, cz: i.cz })), ...m.npcCells.map(n => ({ cx: n.cx, cz: n.cz })), ...m.spots.map(s => ({ cx: s.cx, cz: s.cz }))];
  let cur = { cx: m.stairs.cx, cz: m.stairs.cz }, cells = 0, unreachable = 0;
  while (left.length) {
    const f = bfsField(m, cur.cx, cur.cz, blocked);
    let best = -1, bd = Infinity;
    left.forEach((t, i) => { const d = f.dist[idx(m, t.cx, t.cz)]; if (d >= 0 && d < bd) { bd = d; best = i; } });
    if (best < 0) { unreachable = left.length; break; }
    cells += bd; cur = left.splice(best, 1)[0];
  }
  const f = bfsField(m, cur.cx, cur.cz, blocked);
  const back = f.dist[idx(m, m.stairs.cx, m.stairs.cz)];
  if (back > 0) cells += back;
  return { cells, unreachable };
}
const same = (a, b) => a && b && a[0] === b[0] && a[1] === b[1];
const cellKey = (c) => `${c[0]},${c[1]}`;

// validateMap(rows, meta, {size, strict}) → { ok, errors[], warnings[], stats }. `meta` may be a ZONES entry or
// {name, entry:'F'} for the hub.
//
// The v1 checks are always errors: dimensions, legend, border, one S/V matching `entry`, altar only in the Source,
// connectivity, every item/N/C reachable, hunters/creatures reachable with the gates closed unless `gateOk`, G in a
// deep neighbourhood, w in a water body, Y with LOS ≤ 6 u, X in a wall line, meta agreement (hunters, creatures,
// npcs, gate, spots, loot).
//
// The DESIGN.md §3.3/§3.4/§3.6/§3.7 contract checks (size band, walkable/wall share, marker legality, first-run
// topology, shortcut geometry, deep pockets, regions, route length, entry pocket, Source laps) are reported as
// `[v2] …` WARNINGS while the zone is still on its legacy grid and as ERRORS the moment `rows.length` reaches
// `meta.targets.size` — i.e. a resized map must satisfy the whole contract, and today's 40×40 maps stay green.
// Pass `{strict: true}` to demand the contract early, `{strict: false}` to only ever warn.
export function validateMap(rows, meta = {}, { size = 40, strict = null } = {}) {
  const errors = [], warnings = [], name = meta.name || meta.id || 'map';
  const err = (s) => errors.push(`${name}: ${s}`), warn = (s) => warnings.push(`${name}: ${s}`);
  const h = rows.length, w = rows[0] ? rows[0].length : 0;
  const isHub = meta.entry === 'F' || meta.hub;
  const targets = meta.targets || null;
  const isStrict = strict === null ? !!(targets && h === targets.size && w === targets.size) : !!strict;
  const v2 = (s) => (isStrict ? err : warn)(`[v2] ${s}`);
  if (!isHub && (w !== size || h !== size)) err(`expected ${size}×${size}, got ${w}×${h}`);
  rows.forEach((r, z) => { if (r.length !== w) err(`row ${z} has length ${r.length}, expected ${w}`); });
  rows.forEach((r, z) => { for (let x = 0; x < r.length; x++) if (!LEGEND_CHARS.has(r[x])) err(`unknown char '${r[x]}' at (${x},${z})`); });
  rows.forEach((r, z) => { for (let x = 0; x < r.length; x++) if ((z === 0 || z === h - 1 || x === 0 || x === w - 1) && r[x] !== '#') err(`border not wall at (${x},${z})`); });
  if (errors.length) return { ok: false, errors, warnings, stats: null };

  const m = parseMap(rows, 0, name, { bands: meta.bands, shortcuts: meta.shortcuts });
  const count = (ch) => rows.reduce((n, r) => n + (r.split(ch).length - 1), 0);
  // NB: `stats.w` is the count of 'w' (Drowner) chars — the grid size is `width`/`height`.
  const stats = { width: w, height: h, w, h, strict: isStrict, S: count('S'), V: count('V'), A: count('A'), H: count('H'), N: count('N'), C: count('C'), X: count('X'), W: count('W'),
    o: count('o'), r: count('r'), R: count('R'), D: count('D'), P: count('P'), F: count('F'), '=': count('='),
    L: count('L'), G: count('G'), Y: count('Y'), B: count('B'), w: count('w') };
  const spawns = stats.S + stats.V;
  if (isHub) {
    if (stats.F !== 1) err(`hub needs exactly one F, has ${stats.F}`);
    if (stats.S !== 1) err(`hub needs exactly one S, has ${stats.S}`);
  } else {
    if (spawns !== 1) err(`needs exactly one spawn (S or V), has ${spawns}`);
    else if (meta.entry && ((meta.entry === 'S' && stats.S !== 1) || (meta.entry === 'V' && stats.V !== 1))) err(`entry should be ${meta.entry}`);
    if (meta.id === 'source' ? stats.A !== 1 : stats.A !== 0) err(`altar count ${stats.A} (Source needs 1, others 0)`);
  }
  if (!m.stairs) { err('no spawn cell'); return { ok: false, errors, warnings, stats }; }

  /* ---- 1. size / walkable / wall share (DESIGN.md §3.3) ---- */
  let walkable = 0, walls = 0;
  for (let z = 0; z < h; z++) for (let x = 0; x < w; x++) {
    const ch = rows[z][x];
    if (ch === '#') walls++; else if (ch !== 'P') walkable++;
  }
  stats.walkable = walkable; stats.walls = walls; stats.wallShare = +(walls / (w * h)).toFixed(3);
  if (!isHub && meta.size && (w !== meta.size || h !== meta.size)) err(`meta.size is ${meta.size} but the rows are ${w}×${h}`);
  if (targets) {
    if (w !== targets.size || h !== targets.size) v2(`grid is ${w}×${h}; §3.3 target ${targets.size}×${targets.size} (walkable ${walkable} → ${targets.walkable} ±5 %, route ${targets.route}+)`);
    else {
      const lo = targets.walkable * 0.95, hi = targets.walkable * 1.05;
      if (walkable < lo || walkable > hi) v2(`walkable ${walkable} outside the §3.3 band ${Math.round(lo)}–${Math.round(hi)}`);
      if (Math.abs(stats.wallShare - targets.wallShare) > 0.05) v2(`wall share ${(stats.wallShare * 100).toFixed(1)} % is more than 5 pts off the §3.3 ${(targets.wallShare * 100).toFixed(1)} %`);
    }
  }

  /* ---- connectivity: `open` = gates open + every shortcut CLOSED (first-run topology, §3.7 check 3);
          `all` = gates and shortcuts open (full connectivity, check 4); `closed` = nothing open ---- */
  const open = flood(m, m.stairs.cx, m.stairs.cz, true, false);
  const all = flood(m, m.stairs.cx, m.stairs.cz, true, true);
  const closed = flood(m, m.stairs.cx, m.stairs.cz, false, false);
  let orphans = 0, gatedCells = 0, needShortcut = 0;
  const orphanList = [];
  for (let z = 0; z < h; z++) for (let x = 0; x < w; x++) {
    const t = m.cells[idx(m, x, z)];
    if (solidType(t)) continue;
    const i = idx(m, x, z);
    if (t === T.SHORTCUT) continue;                    // barred: solid until opened (its flanks are checked below)
    if (!all[i]) { orphans++; if (orphanList.length < 8) orphanList.push(`(${x},${z})`); }
    else if (!open[i]) needShortcut++;                 // reachable only through a shortcut → check 3 violation
    else if (!closed[i] && t !== T.GATE) gatedCells++;
  }
  if (orphans) err(`${orphans} walkable cell(s) unreachable from spawn even with gates and shortcuts open: ${orphanList.join(' ')}${orphans > 8 ? ' …' : ''}`);
  if (needShortcut) v2(`${needShortcut} cell(s) reachable only through a shortcut — nothing in the zone may need one (§3.7 check 3)`);
  stats.reachable = w * h - orphans; stats.gatedCells = gatedCells; stats.shortcutOnlyCells = needShortcut;
  const reach = (label, list, needClosed) => {
    for (const c of list) {
      const i = idx(m, c.cx, c.cz);
      if (!open[i]) err(`${label} at (${c.cx},${c.cz}) unreachable`);
      else if (needClosed && !closed[i]) err(`${label} at (${c.cx},${c.cz}) is behind a gate`);
    }
  };
  reach('item', m.items, false);
  reach('npc', m.npcCells, false);
  reach('spot', m.spots, false);
  reach('hunter spawn', m.hunterSpawns, true);       // hunters do not open gates
  if (m.altar) reach('altar', [m.altar], true);
  // creatures (DESIGN.md §5.7): reachable from the spawn with the gates closed unless the meta says gateOk (a Warden
  // may guard a gated pocket); G in a deep neighbourhood; w in a water body; Y visible from some floor/door cell ≤ 6 u.
  const cmeta = meta.creatures || [];
  m.creatures.forEach((c, i) => {
    const o = cmeta[i] || {}, label = `creature ${c.kind}`;
    if (!open[c.idx]) err(`${label} at (${c.cx},${c.cz}) unreachable`);
    else if (!o.gateOk && !closed[c.idx]) err(`${label} at (${c.cx},${c.cz}) is behind a gate (set gateOk to allow)`);
    if (c.kind === 'warden' && !deepNeighbourhood(rows, c.cx, c.cz)) err(`${label} at (${c.cx},${c.cz}) is not in a deep pocket`);
    if (c.kind === 'drowner') {
      const wet = DIRS4.some(([dx, dz]) => cellType(m, c.cx + dx, c.cz + dz) === T.WATER);
      if (m.cells[c.idx] !== T.WATER || !wet) err(`${label} at (${c.cx},${c.cz}) is not in a water body`);
    }
    if (c.kind === 'falseLight') {
      let seen = false;
      for (let z = Math.max(0, c.cz - 6); z <= Math.min(h - 1, c.cz + 6) && !seen; z++) for (let x = Math.max(0, c.cx - 6); x <= Math.min(w - 1, c.cx + 6) && !seen; x++) {
        const ch = rows[z][x]; if (ch !== '.' && ch !== 'D') continue;
        if (Math.hypot(x - c.cx, z - c.cz) <= 6 && los(m, x + 0.5, z + 0.5, c.cx + 0.5, c.cz + 0.5)) seen = true;
      }
      if (!seen) err(`${label} at (${c.cx},${c.cz}) has no floor/deep cell with LOS within 6 u (it must be seen to work)`);
    }
  });
  for (const g of m.gates) {
    const ns = isSolid(m, g.cx, g.cz - 1) && isSolid(m, g.cx, g.cz + 1), ew = isSolid(m, g.cx - 1, g.cz) && isSolid(m, g.cx + 1, g.cz);
    if (!ns && !ew) err(`gate at (${g.cx},${g.cz}) is not set in a wall line`);
    if (ns && (isSolid(m, g.cx - 1, g.cz) || isSolid(m, g.cx + 1, g.cz))) v2(`gate at (${g.cx},${g.cz}) has a solid flank on its open axis`);
    if (ew && (isSolid(m, g.cx, g.cz - 1) || isSolid(m, g.cx, g.cz + 1))) v2(`gate at (${g.cx},${g.cz}) has a solid flank on its open axis`);
    if (!open[g.idx]) err(`gate at (${g.cx},${g.cz}) unreachable`);
  }
  if (m.gates.length && gatedCells === 0) warn('gates seal nothing (every cell reachable with them closed)');
  const behind = [];
  for (const it of m.items) if (open[idx(m, it.cx, it.cz)] && !closed[idx(m, it.cx, it.cz)]) behind.push(`${it.kind}@(${it.cx},${it.cz})`);
  for (const n of m.npcCells) if (open[n.idx] && !closed[n.idx]) behind.push(`npc@(${n.cx},${n.cz})`);
  stats.behindGate = behind;

  /* ---- 2. markers on legal cells (§3.7 check 2) ---- */
  const openNbrs = (x, z) => DIRS4.map(([dx, dz]) => ({ cx: x + dx, cz: z + dz })).filter(c => inBounds(m, c.cx, c.cz) && !isSolid(m, c.cx, c.cz));
  const marooned = (x, z) => { const n = openNbrs(x, z); return !n.length || n.every(c => cellType(m, c.cx, c.cz) === T.WATER); };
  for (let z = 0; z < h; z++) for (let x = 0; x < w; x++) {
    const ch = rows[z][x];
    if ('or'.includes(ch) && ch !== '' && (ch === 'o' || ch === 'r')) { if (marooned(x, z)) v2(`item '${ch}' at (${x},${z}) is not on dry floor/deep (§3.4: o and r sit on '.' or 'D')`); }
    else if (ch === 'R') {
      const wet = openNbrs(x, z).every(c => cellType(m, c.cx, c.cz) === T.WATER) && openNbrs(x, z).length > 0;
      if (!deepNeighbourhood(rows, x, z) && !wet) v2(`rich relic 'R' at (${x},${z}) is neither in a deep pocket nor standing in a flooded vault`);
    } else if ('NCHLGYB'.includes(ch)) { if (marooned(x, z)) v2(`marker '${ch}' at (${x},${z}) is not on floor/deep`); }
    else if (ch === 'w') { if (!DIRS4.some(([dx, dz]) => cellType(m, x + dx, z + dz) === T.WATER)) v2(`'w' at (${x},${z}) is not in a water body`); }
  }

  /* ---- 5. shortcuts (§3.6 data + §3.7 check 5) ---- */
  const scMeta = Array.isArray(meta.shortcuts) ? meta.shortcuts : [];
  const regions = Array.isArray(meta.regions) ? meta.regions : [];
  const metaCells = new Set(), scIds = new Set();
  const scStats = [];
  for (const e of scMeta) {
    if (!e.id) { err('a SHORTCUTS entry has no id'); continue; }
    if (scIds.has(e.id)) err(`duplicate shortcut id '${e.id}'`);
    scIds.add(e.id);
    if (regions.some(r => r.id === e.id)) err(`shortcut id '${e.id}' collides with a region id`);
    if (!Array.isArray(e.cells) || e.cells.length < 1 || e.cells.length > 2) { err(`shortcut '${e.id}' must list 1 or 2 cells`); continue; }
    if (!DIR_OF[e.openFrom]) err(`shortcut '${e.id}' openFrom '${e.openFrom}' is not N/E/S/W`);
    if (e.cells.length === 2 && Math.abs(e.cells[0][0] - e.cells[1][0]) + Math.abs(e.cells[0][1] - e.cells[1][1]) !== 1) err(`shortcut '${e.id}' cells are not adjacent`);
    let ok = true;
    for (const c of e.cells) {
      metaCells.add(cellKey(c));
      if (!(rows[c[1]] && rows[c[1]][c[0]] === '=')) { err(`shortcut '${e.id}' lists (${c[0]},${c[1]}) but there is no '=' there`); ok = false; }
    }
    if (!ok || !DIR_OF[e.openFrom]) continue;
    // geometry: wall line, the two flanks, which side is farther from the entry, and the detour it removes
    const [dx, dz] = DIR_OF[e.openFrom];
    const c0 = e.cells[0];
    const of_ = { cx: c0[0] + dx, cz: c0[1] + dz }, bf = { cx: c0[0] - dx, cz: c0[1] - dz };
    const perp = dx ? [[0, -1], [0, 1]] : [[-1, 0], [1, 0]];
    for (const c of e.cells) {
      if (!perp.every(([px, pz]) => isSolid(m, c[0] + px, c[1] + pz) || e.cells.some(o => o[0] === c[0] + px && o[1] === c[1] + pz)))
        v2(`shortcut '${e.id}' cell (${c[0]},${c[1]}) is not set in a wall line`);
    }
    const openOk = !isSolid(m, of_.cx, of_.cz), barredOk = !isSolid(m, bf.cx, bf.cz);
    if (!openOk || !barredOk) { v2(`shortcut '${e.id}' flanks are not both open (open ${openOk}, barred ${barredOk})`); }
    else {
      const f = distField(m, m.stairs.cx, m.stairs.cz, true, false);
      const dBar = f.dist[idx(m, bf.cx, bf.cz)], dOpen = f.dist[idx(m, of_.cx, of_.cz)];
      if (dBar < 0) v2(`shortcut '${e.id}' barred flank (${bf.cx},${bf.cz}) is not reachable with every shortcut shut`);
      if (dOpen < 0) v2(`shortcut '${e.id}' far flank (${of_.cx},${of_.cz}) is not reachable with every shortcut shut (§3.7 check 3)`);
      if (dBar >= 0 && dOpen >= 0 && dOpen <= dBar) v2(`shortcut '${e.id}' openFrom '${e.openFrom}' is the NEARER side (${dOpen} vs ${dBar} cells from the entry) — bar the near side`);
      const g = distField(m, bf.cx, bf.cz, true, false);
      const detour = g.dist[idx(m, of_.cx, of_.cz)];
      const floorD = meta.deepStyle === 'bands' ? 100 : 40;
      if (detour < 0) v2(`shortcut '${e.id}' flanks are not connected with it shut`);
      else if (detour < floorD) v2(`shortcut '${e.id}' removes a detour of only ${detour} cells (floor ${floorD})`);
      scStats.push({ id: e.id, name: e.name, cells: e.cells, openFrom: e.openFrom, saves: e.saves, detour, dBarred: dBar, dOpen });
      if (meta.deepStyle === 'bands') {
        const lb = lapOf(bf.cx, bf.cz, m), lo2 = lapOf(of_.cx, of_.cz, m);
        if (Math.abs(lo2 - lb) !== 1) v2(`shortcut '${e.id}' joins lap ${lb} to lap ${lo2} — a fissure must join L to L+1 (§3.6)`);
      }
    }
  }
  for (const s of m.shortcuts) if (!metaCells.has(cellKey([s.cx, s.cz]))) err(`'=' at (${s.cx},${s.cz}) is in no SHORTCUTS entry (world.loadZone could not bind it)`);
  stats.shortcuts = scStats;

  /* ---- 6/7. regions and deep pockets (§3.7 checks 6, 7) ---- */
  if (!isHub && !regions.length) v2('no REGIONS declared (DESIGN.md §3.4)');
  if (regions.length) {
    const owner = new Int16Array(w * h).fill(-1);
    regions.forEach((r, ri) => {
      const x = r.x || [], z = r.z || [];
      if (!(x[0] >= 0 && x[1] < w && z[0] >= 0 && z[1] < h && x[0] <= x[1] && z[0] <= z[1])) { err(`region '${r.id}' extent x[${x}] z[${z}] is not inside the grid`); return; }
      for (let cz = z[0]; cz <= z[1]; cz++) for (let cx = x[0]; cx <= x[1]; cx++) {
        const i = idx(m, cx, cz);
        if (owner[i] >= 0) err(`regions '${regions[owner[i]].id}' and '${r.id}' overlap at (${cx},${cz})`);
        else owner[i] = ri;
      }
    });
    const unassigned = [];
    for (let cz = 0; cz < h; cz++) for (let cx = 0; cx < w; cx++) {
      const i = idx(m, cx, cz);
      if (solidType(m.cells[i]) || owner[i] >= 0) continue;
      if (unassigned.length < 8) unassigned.push(`(${cx},${cz})`);
      stats.unassigned = (stats.unassigned | 0) + 1;
    }
    if (stats.unassigned) v2(`${stats.unassigned} non-solid cell(s) belong to no region: ${unassigned.join(' ')}${stats.unassigned > 8 ? ' …' : ''}`);
    for (const r of regions) {
      const x = r.x || [], z = r.z || [];
      if (!(x.length === 2 && z.length === 2)) continue;
      const chars = { oil: 0, relic: 0, rich: 0 };
      let cells = 0, deep = 0, entrances = 0;
      for (let cz = z[0]; cz <= z[1]; cz++) for (let cx = x[0]; cx <= x[1]; cx++) {
        const ch = (rows[cz] || '')[cx];
        if (ch === 'o') chars.oil++; else if (ch === 'r') chars.relic++; else if (ch === 'R') chars.rich++;
        if (ch === '#' || ch === 'P') continue;
        cells++; if (ch === 'D' || ch === 'R') deep++;
        // an entrance = a non-solid cell of this region with a non-solid neighbour outside it
        if (DIRS4.some(([dx, dz]) => { const nx = cx + dx, nz = cz + dz; return inBounds(m, nx, nz) && !isSolid(m, nx, nz) && (nx < x[0] || nx > x[1] || nz < z[0] || nz > z[1]); })) entrances++;
      }
      if (r.loot) for (const k of ['oil', 'relic', 'rich']) if ((r.loot[k] | 0) !== chars[k]) v2(`region '${r.id}' declares ${r.loot[k] | 0} ${k} but its extent holds ${chars[k]}`);
      if (r.deep) {
        if (!cells || deep / cells < 0.6) v2(`deep region '${r.id}' is ${cells ? Math.round(100 * deep / cells) : 0} % D (needs ≥ 60 %)`);
        if (!entrances) v2(`deep region '${r.id}' has no non-solid entrance`);
        for (let cz = z[0]; cz <= z[1]; cz++) for (let cx = x[0]; cx <= x[1]; cx++) {
          const ch = (rows[cz] || '')[cx];
          if (ch === '#' || ch === 'P' || ch === '=') continue;
          if (!deepNeighbourhood(rows, cx, cz)) { v2(`deep region '${r.id}' cell (${cx},${cz}) is not in a deep neighbourhood (the lamp would not shrink)`); cz = z[1] + 1; break; }
        }
      }
    }
  }

  /* ---- 8. route length (§3.3 / §3.7 check 8) ---- */
  if (!isHub) {
    const rc = routeCells(m, { gatesOpen: true, scOpen: false });
    stats.route = { closed: rc.cells, open: rc.cells };
    if (m.shortcuts.length) stats.route.open = routeCells(m, { gatesOpen: true, scOpen: true }).cells;
    if (targets && targets.route && rc.cells < targets.route) v2(`full-clear route is ${rc.cells} cells with every shortcut shut; §3.3 target ≥ ${targets.route}`);
    if (m.shortcuts.length && stats.route.open > rc.cells * 0.7) v2(`with every shortcut open the route is ${stats.route.open} cells = ${Math.round(100 * stats.route.open / rc.cells)} % of ${rc.cells} (§4 wants ≤ 70 %)`);
  }

  /* ---- 9. entry pocket (§3.7 check 9) ---- */
  if (!isHub) {
    const free = openNbrs(m.stairs.cx, m.stairs.cz).length;
    if (free < 3) v2(`entry (${m.stairs.cx},${m.stairs.cz}) has ${free} free 4-neighbour(s), needs ≥ 3`);
    const ef = distField(m, m.stairs.cx, m.stairs.cz, true, false);
    for (const c of [...m.hunterSpawns, ...m.creatures]) {
      const d = ef.dist[idx(m, c.cx, c.cz)];
      if (d >= 0 && d < 10) v2(`${c.kind || 'hunter'} spawn at (${c.cx},${c.cz}) is ${d} BFS cells from the entry (needs ≥ 10)`);
    }
  }

  /* ---- 10. Source laps (§3.4 / §3.7 check 10) ---- */
  if (meta.deepStyle === 'bands' && m.altar) {
    const lap = lapOf(m.altar.cx, m.altar.cz, m);
    if (lap !== 5) v2(`altar at lap ${lap}, expected 5`);
    stats.altarLap = lap;
    stats.creatureLaps = m.creatures.map(c => ({ kind: c.kind, cell: [c.cx, c.cz], lap: lapOf(c.cx, c.cz, m) }));
    stats.hunterLaps = m.hunterSpawns.map(s => lapOf(s.cx, s.cz, m));
  }

  /* ---- metadata agreement ---- */
  if (meta.hunters && meta.hunters.length !== m.hunterSpawns.length) err(`meta.hunters has ${meta.hunters.length} entries, map has ${m.hunterSpawns.length} H`);
  if (meta.creatures || m.creatures.length) {
    const want = meta.creatures || [];
    if (want.length !== m.creatures.length) err(`meta.creatures has ${want.length} entries, map has ${m.creatures.length} creature cells`);
    m.creatures.forEach((c, i) => { if (want[i] && want[i].kind !== c.kind) err(`creature ${i} at (${c.cx},${c.cz}) is ${c.kind}, meta says ${want[i].kind}`); });
    for (const k of ['warden']) for (const o of want) if (o.kind === k && o.facing && !'NESW'.includes(o.facing)) err(`warden facing '${o.facing}' is not N/E/S/W`);
  }
  if (meta.npcs) {
    const cells = Object.entries(meta.npcs);
    if (cells.length !== m.npcCells.length) err(`meta.npcs names ${cells.length} NPC(s), map has ${m.npcCells.length} N`);
    for (const [id, c] of cells) if (!m.npcCells.some(n => same([n.cx, n.cz], c))) err(`npc ${id} expected at (${c[0]},${c[1]}), no N there`);
  }
  if (meta.gate) {
    if (meta.gate.cells.length !== m.gates.length) err(`meta.gate has ${meta.gate.cells.length} cell(s), map has ${m.gates.length} X`);
    for (const c of meta.gate.cells) if (!m.gates.some(g => same([g.cx, g.cz], c))) err(`gate expected at (${c[0]},${c[1]}), no X there`);
    if (!(meta.gate.tool in TOOLS)) err(`unknown gate tool '${meta.gate.tool}'`);
  } else if (m.gates.length) err(`map has ${m.gates.length} X but meta.gate is null`);
  if (meta.spots) {
    if (meta.spots.length !== m.spots.length) err(`meta.spots has ${meta.spots.length}, map has ${m.spots.length} C`);
    meta.spots.forEach((s, i) => { const c = m.spots[i]; if (!c || !same([c.cx, c.cz], s.cell)) err(`spot ${i} expected at (${s.cell[0]},${s.cell[1]}), map spot ${i} is ${c ? `(${c.cx},${c.cz})` : 'missing'}`); });
  }
  if (meta.loot) {
    const got = { oil: stats.o, relic: stats.r, rich: stats.R };
    for (const k of Object.keys(meta.loot)) if (meta.loot[k] !== got[k]) err(`loot.${k}: meta says ${meta.loot[k]}, map has ${got[k]}`);
  }
  if (meta.anchors) {
    // ANCHORS is the test contract: every anchor must be a real cell inside the grid (suites teleport to cx+0.5)
    const walk = (o, path) => { for (const [k, val] of Object.entries(o)) {
      if (Array.isArray(val) && val.length === 2 && typeof val[0] === 'number') { if (!inBounds(m, val[0], val[1])) err(`anchor ${path}${k} (${val[0]},${val[1]}) is outside the grid`); }
      else if (Array.isArray(val)) val.forEach((v, i) => { if (Array.isArray(v) && !inBounds(m, v[0], v[1])) err(`anchor ${path}${k}[${i}] (${v[0]},${v[1]}) is outside the grid`); });
      else if (val && typeof val === 'object') walk(val, `${path}${k}.`);
    } };
    walk(meta.anchors, '');
    if (meta.anchors.entry && !same(meta.anchors.entry, [m.stairs.cx, m.stairs.cz])) err(`anchors.entry (${meta.anchors.entry}) is not the spawn cell (${m.stairs.cx},${m.stairs.cz})`);
  }
  return { ok: errors.length === 0, errors, warnings, stats };
}
export const validateZone = (id, opts = {}) => validateMap(ZONES[id].rows, ZONES[id], { size: ZONES[id].size || 40, ...opts });
// validateAll() → { ok, errors[], warnings[], zones: {id: result}, hub: result, hubV2: result }
// Each zone's `stats` carries `shortcuts: [{id, saves, detour, dBarred, dOpen}]` and `route: {closed, open}` so the
// numbers in DESIGN.md §3.3/§3.6 can be read straight off the page (__game.mapsApi.validateAll()).
export function validateAll(opts = {}) {
  const zones = {}, errors = [], warnings = [];
  for (const id of ZONE_ORDER) { const r = validateZone(id, opts); zones[id] = r; errors.push(...r.errors); warnings.push(...r.warnings); }
  const hub = validateMap(HUB_ROWS, { name: 'hub', entry: 'F', hub: true });
  const hubV2 = validateMap(HUB_ROWS_V2, { name: 'hub-v2', entry: 'F', hub: true });
  for (const r of [hub, hubV2]) { errors.push(...r.errors); warnings.push(...r.warnings); }
  return { ok: errors.length === 0, errors, warnings, zones, hub, hubV2 };
}
