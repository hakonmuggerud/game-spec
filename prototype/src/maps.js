// maps.js — ASCII maps, zone metadata, map parsing, validation and pure grid helpers (DESIGN.md §3, DESIGN-v2 §2).
// Pure data + pure functions over a parsed map: no THREE, no ctx. Any module may import this file.
// Coordinates everywhere are (x = column, z = row); `idx = z * w + x`.
import { HUB_OX, HUNTER_PROFILES, TOOLS } from './config.js';

// Cell types stored in map.cells.
export const T = { FLOOR: 0, WALL: 1, PILLAR: 2, DEEP: 3, STAIRS: 4, WATER: 5, GATE: 6, ELEVATOR: 7, ALTAR: 8 };
export const T_NAME = ['floor', 'wall', 'pillar', 'deep', 'stairs', 'water', 'gate', 'elevator', 'altar'];

// Legend (DESIGN-v2 §2): char → what the parser does with it.
export const LEGEND = {
  '#': 'wall', 'P': 'pillar', '.': 'floor', 'D': 'deep', 'S': 'stairs', 'V': 'elevator',
  'o': 'item:oil', 'r': 'item:relic', 'R': 'deep+item:rich', 'H': 'hunter', 'F': 'flame',
  'N': 'npc', 'C': 'spot', 'W': 'water', 'X': 'gate', 'A': 'altar', '0-9': 'anchor (hub building)',
  // DESIGN.md §5.7 — creature spawns (L G Y B keep the cell floor/deep like H; w is a water cell with a spawn)
  'L': 'creature:lampwight', 'G': 'creature:warden', 'Y': 'creature:falseLight', 'B': 'creature:brute', 'w': 'water+creature:drowner',
};
const LEGEND_CHARS = new Set(['#', 'P', '.', 'D', 'S', 'V', 'o', 'r', 'R', 'H', 'F', 'N', 'C', 'W', 'X', 'A', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
  'L', 'G', 'Y', 'B', 'w']);
// creature legend char → hunter.js profile name (the parser stores the profile as `kind`)
export const CREATURE_CHARS = { L: 'lampwight', G: 'warden', Y: 'falseLight', B: 'brute', w: 'drowner' };

/* ============================================================
   Zone rows
   ============================================================ */
// The Undercroft — v1 rows with the DESIGN-v2 §2 patch applied:
//   N (5,22) Wick the Lamplighter · N (2,3) Deacon Maud (NW deep pocket, cell stays deep)
//   X (4,5) Pry-Bar gate sealing the NW pocket · C (30,2) spot 0 (NE pocket) · C (16,29) spot 1 (great hall)
const UNDERCROFT_ROWS = [
  '########################################',
  '#DDDDDDDD###############DDDDDDDDDDDDDDD#',
  '#DDDRDDDD###############DDGDDDCDDRDDDDD#',
  '#DNDDDDDD###############DDDoDDDDDDDDDDD#',
  '#DDDDDDDD###############DDDDDDDDDDDDDDD#',
  '####X#############################.#####',
  '#........#.............#...............#',
  '#........#..P...P...P..#.....P...P.....#',
  '#........#......H......#...............#',
  '#..r......................o............#',
  '#........#..P...P...P..#.....P...P.....#',
  '#........#......r......#...............#',
  '#........#.............#...............#',
  '#........#.............#...............#',
  '####.###########...#################.###',
  '#........#..............#..............#',
  '#..P.....#...P......P...#......P.......#',
  '#...B....#..............#........o.....#',
  '#........#..............#..............#',
  '#........#...P......P...#......P...r...#',
  '####.#####..............#..............#',
  '#.o......#..............#..............#',
  '#....N...#...P......P...#......P.......#',
  '#........#..............#..............#',
  '#........#..............######.#########',
  '#........#...P......P...#..............#',
  '#.........................o............#',
  '#........#..............#..............#',
  '#........#...P......P...#.........r....#',
  '#..r.....#......C.......#..............#',
  '#........#..............#..............#',
  '#........#..............#............o.#',
  '##################....##################',
  '###############..........###############',
  '###############.P......P.###############',
  '###############..........###############',
  '###############.P......P.###############',
  '###############....S.....###############',
  '###############..........###############',
  '########################################',
];

// The Cistern — flooded hall with causeways, pump room W, cell block E, flooded vault N behind the Sluice-Key gate.
const CISTERN_ROWS = [
  '########################################',
  '#......#WWWWWWWWWWWWWWWWWWWWWWWW#......#',
  '#..r...#WWWWWWWWWWWWWWWWWWWWWWWW#...r..#',
  '#......#WWWWWWWWWWWR.WWWWWWWWWWW#......#',
  '#......#WWWWWWWWWWW.RWWWWWWWWWWW#......#',
  '###.################X###############.###',
  '#WWWW.............................WWWW.#',
  '#WWoW.........H..P.....P..........WWWW.#',
  '#WWWW.........P...........P.......WWWW.#',
  '#.WWWW............WWWWWW.........WWWW..#',
  '#..WWWW.........WWWWWWWWWW...H..WWWW...#',
  '#..WWWW.........WWWWWWWWWW......WWWW...#',
  '#...WWWW..P...WWWWWWWWWWWWWW...PWWW....#',
  '#....WWWW.....WWWWWWwWWWWWWW..WWWW.....#',
  '#...WWWW......WWWWWWWWWWWWWW....WWW....#',
  '#..WWWW.........WWWWWWWWWW......WWWW...#',
  '#..WWWW.........WWWWWWWWWW......WWWW...#',
  '#.WWWW........P...WWWWWW..P......WWWW..#',
  '#WWWW.............................WWWW.#',
  '#WWWW...............C.............WWWW.#',
  '#WWWW.WWWWWWWWWWWWWWWWWWWWWWWW....WWWW.#',
  '####..###WWWWWWWWWWWWWWWWWWWW#.....#####',
  '#....#..........WWWWWWWWWW.........#..N#',
  '#.r..#..........WWWWWWWWWW.........#..o#',
  '#....#.....P......WWWWWW....P..........#',
  '#..................................#...#',
  '#..C.#......WWWW........WWWW.......##.##',
  '#....#......WWWW........WWWW..L....#...#',
  '#....#......WWWW.....r..WWWW.......#...#',
  '#....#.P....WWWW........WWWW....P..#...#',
  '#.o..#......................r......#...#',
  '#....#.............................#.o.#',
  '##################....##################',
  '###############..........###############',
  '###############.P..WW..P.###############',
  '###############....WW....###############',
  '###############.P......P.###############',
  '###############....S.....###############',
  '###############..........###############',
  '########################################',
];

// The Ossuary — 1-wide bone corridors, three dark vaults N, bone pits, reliquary behind the Censer gate.
const OSSUARY_ROWS = [
  '########################################',
  '########################################',
  '##DDDDDDDD#DDD#DDDDDDDDD######DDDDDDDD##',
  '##DRDDDDDD#DRD#DDDDDDDDD######DDDDDRDD##',
  '##DDDDDDDD#DDDXDDDDDrDDD######DDDDDDDD##',
  '##DDDDDNDD#DRD#DDDDHDDDD######DDDDDDDD##',
  '##DDDDDDDD#DGD#DDDDDDDDD######DDDDDDDD##',
  '##DDDDDDDD#####DDDDDDDDD######DDDDDDDD##',
  '##DDDDDDDD#####DDDDDDDDD######DDDDDDDD##',
  '#####.#############.#############.######',
  '#####.#############.#############.######',
  '#####.#############.#############.######',
  '#......................................#',
  '#.DDDDDD.#..##########.#..###.DDDDDD.###',
  '#.DDDDDD.#r.##########.#r.###.DDDDDD.###',
  '#.DDDDDD.#############.######.DDRoDD.###',
  '#.DDDDDD.#############.######.DDDDDD.###',
  '#.DDDDDD.#############.######.DDDDDD.###',
  '#.....................P......C.........#',
  '#.##..##.########.Y###.######.#..###.###',
  '#.##o.##.########r.###.######.#r.###.###',
  '#.######.#############.######.######.###',
  '#.######.#############.######.######.###',
  '#.######.#############.######.######.###',
  '#.......P..............................#',
  '#...####.##..##.DDDDDD.##.Y##.######...#',
  '#.r.####.##o.##.DDDDDD.##o.##.######.r.#',
  '#.######.######.DDDDDD.######.######.###',
  '#.######.######.DDDCDD.######.######.###',
  '#.######.######.DDDDDD.######.######.###',
  '#......................................#',
  '###..##########.#..##########.#######..#',
  '###..##########.#..##########.#######o.#',
  '###############.#############.##########',
  '###############.#############.##########',
  '######.....####.#############.##########',
  '######..o.......#############.##########',
  '######.V...#############################',
  '######.....#############################',
  '########################################',
];

// The Source — clockwise square spiral, 2 wide, one door per lap at the top-left, altar chamber at the centre.
const SOURCE_ROWS = [
  '########################################',
  '#V.....................................#',
  '#.....................................o#',
  '#####################################..#',
  '#..DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD#..#',
  '#..DoDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD#..#',
  '#..###############################DD#..#',
  '#..#DDDDDDDDDDDDDDDDDDDDDDDDDDDDD#DD#..#',
  '#..#DDDoDDDDDDDDDDDDDDDDDDDDDDDDD#DD#..#',
  '#..#DD#########################DD#DD#..#',
  '#..#DD#DDDDDDDDDDDDDDDDDDDDDDD#DD#DD#..#',
  '#..#DD#DDDDDDDDDDDDDDDDDDDDDDD#DD#DD#..#',
  '#..#DD#DD###################RD#DD#DD#..#',
  '#..#DD#DD#DDDDDDDDDDDDDDDDD#DD#DD#DD#..#',
  '#..#DD#DD#DDDDDDDDDDDDDDDDD#DY#DD#DD#..#',
  '#..#DD#DD#DD#############DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DDDDDDDDDDD#DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DDDDDDDDDDD#DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DD#DDDADDDD#DD#DD#DD#DD#..#',
  '#..#DD#LD#DD#BD#DDDDDDDD#DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DD##########DD#DD#DD#DD#..#',
  '#..#DD#DD#DD#DDDDDDDDDDDDDD#DD#DD#DD#..#',
  '#..#DD#DD#DH#DDDDDDDDDDDDDD#DD#DD#DD#..#',
  '#..#DD#DD#DD################DD#DD#DD#..#',
  '#..#DD#DD#DDDDDDDDDDDDDDDDDDDD#DD#DD#..#',
  '#..#DD#DD#oDDDDDDDDDDDDDDDDDDD#DD#DD#..#',
  '#..#DD#DD######################DD#DD#H.#',
  '#..#DD#DDDDDDDDDDDDDDDDDDDDDDDDDR#DD#..#',
  '#..#DD#DDDDDDDDDDDDDDDDDDDDDDDDDD#DD#..#',
  '#..#DD############################DD#..#',
  '#..#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD#..#',
  '#..#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDr#..#',
  '#..##################################..#',
  '#......................................#',
  '#...................r..................#',
  '########################################',
];

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
   Zone metadata (DESIGN-v2 §2 table)
   ============================================================ */
// `hunters` lists one speed-profile NAME per `H` in row-major order (hunter.js reads the string; the resolved
// numbers are in `hunterSpeeds`). `npcs` = {id: [cx, cz]} for every N; `npc` = the captive named on the board.
// `gate` = {tool, cells: [[cx, cz]]} for the X cells. `spots` = contract spots in row-major C order (index =
// the `spot` field in contracts.js). `loot` = expected item counts (validated). `requires` = access key, and
// `lockReason` the board text when locked. `intro` is the flavour line shown on zoneEnter, `threat` the one-line
// creature warning the Departure Board prints under an unlocked zone (DESIGN.md §5.7).
const speeds = (names) => names.map(n => { const p = HUNTER_PROFILES[n] || HUNTER_PROFILES.base; return { profile: n, ...p.speed, catchR: p.catchR }; });
export const ZONES = {
  undercroft: {
    id: 'undercroft', name: 'The Undercroft', rows: UNDERCROFT_ROWS, entry: 'S', exit: 'stairs',
    burnMul: 1.0, lampMul: 1.0, deepStyle: 'flat', hunters: ['base'], hunterSpeeds: speeds(['base']),
    // row-major creature cells: G (26,2) facing E over the NE crypt · B (4,17) leashed to the west wing (DESIGN.md §5.7)
    creatures: [{ kind: 'warden', facing: 'E', sweep: 75, reach: 9, territory: 8 }, { kind: 'brute', leash: 14 }],
    npc: 'lamplighter', npcs: { lamplighter: [5, 22], deacon: [2, 3] },
    gate: { tool: 'prybar', cells: [[4, 5]], opens: 'the north-west crypt (Deacon Maud, 1 rich relic)' },
    spots: [{ id: 0, cell: [30, 2], label: 'the north-east crypt' }, { id: 1, cell: [16, 29], label: 'the great hall' }],
    loot: { oil: 6, relic: 5, rich: 2 }, points: 31,
    requires: null, lockReason: null, ambience: 'undercroft', palette: PALETTES.undercroft,
    intro: 'Stone steps, wet with old dark. Something below is listening for light. A cold beam sweeps the north-east crypt, and something heavy walks the west wing.',
    threat: 'a hunter, a Warden watching the north-east crypt, a Brute in the west wing',
  },
  cistern: {
    id: 'cistern', name: 'The Cistern', rows: CISTERN_ROWS, entry: 'S', exit: 'stairs',
    burnMul: 1.0, lampMul: 1.0, deepStyle: 'flat', hunters: ['base', 'base'], hunterSpeeds: speeds(['base', 'base']),
    // w (20,13) in the central lake · L (30,27) in the south-east hall on the way to Ines
    creatures: [{ kind: 'drowner' }, { kind: 'lampwight' }],
    npc: 'cartographer', npcs: { cartographer: [38, 22] },
    gate: { tool: 'sluice', cells: [[20, 5]], opens: 'the flooded vault (2 rich relics)' },
    spots: [{ id: 0, cell: [20, 19], label: 'the drowned hall' }, { id: 1, cell: [3, 26], label: 'the pump room' }],
    loot: { oil: 4, relic: 5, rich: 2 }, points: 29,
    requires: { building: 'tram' }, lockReason: 'Needs the Tram dock', ambience: 'cistern', palette: PALETTES.cistern,
    intro: 'The tram groans to a stop above black water. Every step you take in it will be heard — and something in the south hall drinks flame.',
    threat: 'two hunters, a Drowner in the lake, a Lampwight in the south hall',
  },
  ossuary: {
    id: 'ossuary', name: 'The Ossuary', rows: OSSUARY_ROWS, entry: 'V', exit: 'elevator',
    burnMul: 1.3, lampMul: 0.85, deepStyle: 'flat', hunters: ['fast'], hunterSpeeds: speeds(['fast']),
    // G (12,6) facing N inside the reliquary (behind the Censer gate: gateOk) · Y (18,19) · Y (26,25) beside loot
    creatures: [{ kind: 'warden', facing: 'N', sweep: 75, reach: 9, territory: 8, gateOk: true }, { kind: 'falseLight' }, { kind: 'falseLight' }],
    npc: 'keeper', npcs: { keeper: [7, 5] },
    gate: { tool: 'censer', cells: [[14, 4]], opens: 'the reliquary (2 rich relics)' },
    spots: [{ id: 0, cell: [29, 18], label: 'the east bone-pit' }, { id: 1, cell: [19, 28], label: 'the south vault' }],
    loot: { oil: 6, relic: 7, rich: 5 }, points: 52,
    requires: { building: 'elevator', lightTech: 2 }, lockReason: 'Needs the Elevator and Light-tech II', ambience: 'ossuary', palette: PALETTES.ossuary,
    intro: 'The cage opens on corridors of stacked bone. The air eats oil; the thing here is quick. Not every lantern down here is one you planted.',
    threat: 'a quick hunter, a Warden in the reliquary, two false lights among the alcoves',
  },
  source: {
    id: 'source', name: 'The Source', rows: SOURCE_ROWS, entry: 'V', exit: 'elevator', noBank: true,
    burnMul: 1.0, lampMul: 1.0, deepStyle: 'bands', hunters: ['fast', 'fast'], hunterSpeeds: speeds(['fast', 'fast']),
    // row-major: Y (29,14) lap 3 · L (7,20) lap 2 · B (13,20) lap 4 (endgame dormancy wakes each at lap − 1)
    creatures: [{ kind: 'falseLight' }, { kind: 'lampwight' }, { kind: 'brute', leash: 14 }],
    npc: null, npcs: {}, gate: null, spots: [],
    loot: { oil: 4, relic: 2, rich: 2 }, points: 20,
    requires: { tier: 4, rescued: 'deacon' }, lockReason: 'Needs the flame at tier 4 and Deacon Maud', ambience: 'source', palette: PALETTES.source,
    intro: 'A spiral, turning inward. Each lap is darker than the last. There is no banking here — only the altar. The lights lie down here, and the last ring takes your lanterns.',
    threat: 'quick hunters that multiply as you descend, a Lampwight, a false light, a Brute on the last ring',
  },
};
export const ZONE_ORDER = ['undercroft', 'cistern', 'ossuary', 'source'];

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

// Source bands (DESIGN-v2 §2): lap 0 = outer ring (plain floor), lap 5 = altar chamber.
export const lapOf = (cx, cz) => Math.min(5, Math.floor((Math.min(cx, cz, 39 - cx, 39 - cz) - 1) / 3));

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

// parseMap(rows, ox, name) → { name, w, h, ox, cells, pool, items, stairs, hunterSpawns[], npcCells[],
//   spots[], gates[], flame, anchors{}, altar, creatures[] ({kind, cx, cz, idx, x, z} row-major), hunterSpawn (= hunterSpawns[0]) }
// Marker records carry {cx, cz, idx, x, z} (x/z = world centre) so consumers need no extra lookups.
export function parseMap(rows, ox, name) {
  const h = rows.length, w = rows[0].length;
  for (const r of rows) if (r.length !== w) console.warn(`map ${name}: ragged row "${r}"`);
  const cells = new Uint8Array(w * h), pool = new Uint8Array(w * h);
  const items = [], hunterSpawns = [], npcCells = [], spots = [], gates = [], anchors = {}, creatures = [];
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
      case 'X': t = T.GATE; gates.push(mark(x, z, { open: false, mesh: null })); break;
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
  return { name, w, h, ox, cells, pool, items, stairs, hunterSpawns, npcCells, spots, gates, flame, anchors, altar, creatures,
    hunterSpawn: hunterSpawns[0] || null };
}
export const parseZone = (id) => parseMap(ZONES[id].rows, 0, id);
export const parseHub = (rows = HUB_ROWS_V2) => parseMap(rows, HUB_OX, 'hub');

/* ============================================================
   Pure grid helpers (world.js re-exports these; hunter/npc may import them directly)
   ============================================================ */
export const idx = (m, cx, cz) => cz * m.w + cx;
export const inBounds = (m, cx, cz) => cx >= 0 && cz >= 0 && cx < m.w && cz < m.h;
export const cellType = (m, cx, cz) => (inBounds(m, cx, cz) ? m.cells[idx(m, cx, cz)] : T.WALL);
export const isSolid = (m, cx, cz) => { const t = cellType(m, cx, cz); return t === T.WALL || t === T.PILLAR || t === T.GATE; };
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
   Validation (run headless: `node scratchpad/validate-maps.mjs`, or ctx.maps.validateAll() in the page)
   ============================================================ */
// Flood from (sx, sz) over walkable cells; `gatesOpen` decides whether X cells are passable.
function flood(m, sx, sz, gatesOpen) {
  const seen = new Uint8Array(m.w * m.h), q = [idx(m, sx, sz)];
  seen[q[0]] = 1;
  const passable = (t) => t !== T.WALL && t !== T.PILLAR && (t !== T.GATE || gatesOpen);
  while (q.length) {
    const i = q.pop(), cx = i % m.w, cz = (i / m.w) | 0;
    for (const [dx, dz] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const nx = cx + dx, nz = cz + dz;
      if (!inBounds(m, nx, nz)) continue;
      const j = idx(m, nx, nz);
      if (seen[j] || !passable(m.cells[j])) continue;
      seen[j] = 1; q.push(j);
    }
  }
  return seen;
}
const same = (a, b) => a[0] === b[0] && a[1] === b[1];

// validateMap(rows, meta, {size}) → { ok, errors[], warnings[], stats }. `meta` may be a ZONES entry or
// {name, entry:'F'} for the hub. Checks: dimensions, legend, border, single spawn matching `entry`,
// connectivity of every walkable cell from the spawn (gates open), every item/N/C/H/A reachable, gates
// really seal something (with them closed, at least one cell becomes unreachable) and sit in a wall line,
// counts vs meta (hunters, npcs, gate cells, spots, loot), Source altar present only there.
export function validateMap(rows, meta = {}, { size = 40 } = {}) {
  const errors = [], warnings = [], name = meta.name || meta.id || 'map';
  const err = (s) => errors.push(`${name}: ${s}`), warn = (s) => warnings.push(`${name}: ${s}`);
  const h = rows.length, w = rows[0] ? rows[0].length : 0;
  const isHub = meta.entry === 'F' || meta.hub;
  if (!isHub && (w !== size || h !== size)) err(`expected ${size}×${size}, got ${w}×${h}`);
  rows.forEach((r, z) => { if (r.length !== w) err(`row ${z} has length ${r.length}, expected ${w}`); });
  rows.forEach((r, z) => { for (let x = 0; x < r.length; x++) if (!LEGEND_CHARS.has(r[x])) err(`unknown char '${r[x]}' at (${x},${z})`); });
  rows.forEach((r, z) => { for (let x = 0; x < r.length; x++) if ((z === 0 || z === h - 1 || x === 0 || x === w - 1) && r[x] !== '#') err(`border not wall at (${x},${z})`); });
  if (errors.length) return { ok: false, errors, warnings, stats: null };

  const m = parseMap(rows, 0, name);
  const count = (ch) => rows.reduce((n, r) => n + (r.split(ch).length - 1), 0);
  const stats = { w, h, S: count('S'), V: count('V'), A: count('A'), H: count('H'), N: count('N'), C: count('C'), X: count('X'), W: count('W'),
    o: count('o'), r: count('r'), R: count('R'), D: count('D'), P: count('P'), F: count('F'),
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

  // connectivity
  const open = flood(m, m.stairs.cx, m.stairs.cz, true), closed = flood(m, m.stairs.cx, m.stairs.cz, false);
  let orphans = 0, gatedCells = 0;
  const orphanList = [];
  for (let z = 0; z < h; z++) for (let x = 0; x < w; x++) {
    const t = m.cells[idx(m, x, z)];
    if (t === T.WALL || t === T.PILLAR) continue;
    const i = idx(m, x, z);
    if (!open[i]) { orphans++; if (orphanList.length < 8) orphanList.push(`(${x},${z})`); }
    else if (!closed[i] && t !== T.GATE) gatedCells++;
  }
  if (orphans) err(`${orphans} walkable cell(s) unreachable from spawn even with gates open: ${orphanList.join(' ')}${orphans > 8 ? ' …' : ''}`);
  stats.reachable = w * h - orphans, stats.gatedCells = gatedCells;
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
      const wet = [[1, 0], [-1, 0], [0, 1], [0, -1]].some(([dx, dz]) => cellType(m, c.cx + dx, c.cz + dz) === T.WATER);
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
    if (!open[g.idx]) err(`gate at (${g.cx},${g.cz}) unreachable`);
  }
  if (m.gates.length && gatedCells === 0) warn('gates seal nothing (every cell reachable with them closed)');
  const behind = [];
  for (const it of m.items) if (open[idx(m, it.cx, it.cz)] && !closed[idx(m, it.cx, it.cz)]) behind.push(`${it.kind}@(${it.cx},${it.cz})`);
  for (const n of m.npcCells) if (open[n.idx] && !closed[n.idx]) behind.push(`npc@(${n.cx},${n.cz})`);
  stats.behindGate = behind;

  // metadata agreement
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
  if (meta.deepStyle === 'bands' && m.altar && lapOf(m.altar.cx, m.altar.cz) !== 5) warn(`altar at lap ${lapOf(m.altar.cx, m.altar.cz)}, expected 5`);
  return { ok: errors.length === 0, errors, warnings, stats };
}
export const validateZone = (id) => validateMap(ZONES[id].rows, ZONES[id]);
// validateAll() → { ok, errors[], warnings[], zones: {id: result}, hub: result }
export function validateAll() {
  const zones = {}, errors = [], warnings = [];
  for (const id of ZONE_ORDER) { const r = validateZone(id); zones[id] = r; errors.push(...r.errors); warnings.push(...r.warnings); }
  const hub = validateMap(HUB_ROWS, { name: 'hub', entry: 'F', hub: true });
  const hubV2 = validateMap(HUB_ROWS_V2, { name: 'hub-v2', entry: 'F', hub: true });
  for (const r of [hub, hubV2]) { errors.push(...r.errors); warnings.push(...r.warnings); }
  return { ok: errors.length === 0, errors, warnings, zones, hub, hubV2 };
}
