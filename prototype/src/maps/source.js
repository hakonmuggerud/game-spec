// src/maps/source.js — The Source: the ASCII rows and every map-shaped fact about this zone.
// ONE AUTHOR OWNS THIS FILE (DESIGN.md §3.7 authoring contract). Exports exactly:
//   ID SIZE ROWS REGIONS SHORTCUTS ANCHORS META
// maps.js composes ZONES.source = { id, ...TUNING.source, ...META, rows: ROWS } and owns everything that is not
// map-shaped (PALETTES, requires/lockReason, intro/threat, ambience). Legend: DESIGN.md §3.1.
// Cells are [cx, cz] = [column, row]; `idx = cz * SIZE + cx`.
//
// SHAPE (DESIGN.md §3.3/§3.4) — 60×60, walkable 2288, walls 36.4 %, band 5, route 1926 cells shut / 1086 open (56 %).
// `lapOf` = min(5, floor((ring − 1) / 5)) with ring = min(cx, cz, 59 − cx, 59 − cz), so each lap owns five rings:
//
//   lap L   gallery rings 5L+1…5L+3 (the 3-wide walk)   spur ring 5L+4 (a 1-wide back passage)   lap wall 5L+5
//   lap 5   rings 26–29 = the 8×8 altar chamber (x,z ∈ 26–33), the altar at (30,30)
//
// Each lap is walked TWICE: the gallery ring is cut once (a "break"), so from that lap's door you are forced the
// whole way round to the far end; there a crossing drops you into the spur behind the gallery's inner wall, and the
// spur runs most of the way back round to the next lap's door. Every spur is fenced off from the gallery it hides
// behind (the ring between them is solid except at the one crossing), so nothing short-circuits the descent.
//
//   lap   break            crossing → spur          spur end → door → next entry     hall / guard
//   0     (1–3, 9)   W     (3,10) → (4,10)          (28,55) → (28,54) → (28,53)      reliquary niches; V (2,2)
//   1     (29, 51–53) S    (30,51) → (30,50)        (28,9)  → (28,10) → (28,11)      The Ash Pits; fast H (7,20)
//   2     (29, 11–13) N    (30,13) → (30,14)        (30,45) → (30,44) → (30,43)      The Choir of Stones, L (13,30)
//   3     (29, 41–43) S    (28,41) → (28,40)        (30,19) → (30,20) → (30,21)      The Weeping Wall, Y (41,26)
//   4     (29, 21–24) N    the Antechamber itself   (24,30) → (25,30) → (26,30)      The Antechamber, B (30,23)
//
// The three fissures (`=`, DESIGN.md §3.6) are radial stubs through a lap wall: each is barred on the lap-L side
// and opens from lap L+1, so `player.lap` stays monotonic and endgame.onDeeper still fires on every lap line.
// s_fissure1's bars sit 7 cells from the elevator, in line of sight down the west arm of the Outer Walk — the promise
// you cannot keep until you have walked the whole first lap and its spur.

export const ID = 'source';
export const SIZE = 60;

export const ROWS = [
  '############################################################',
  '#..........................................................#',
  '#.V........................................................#',
  '#..........................................................#',
  '#...######..##o#####..####..####..####o.####..##..##..##...#',
  '#...####################################################...#',
  '#...##DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##...#',
  '#....=DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##...#',
  '#...##DDD####################DDDDDDDDDDDDDDDDDDDDDDDDD##...#',
  '######DD#DDDDDDDDDDDDDDDDDDDD####DD###oD####DD###DDDDD##...#',
  '#....#DD#D##################D######################DDD##...#',
  '#..#.#DD#D#DDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD##DDD##...#',
  '#..#.#DD#D#DDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD##DDD#....#',
  '#..#.#DD#D#DDDDDDDDDDDDDDDDDD#D###############DDD##DDD#....#',
  '#..#.#DD#D#DDD####DD###DD###D#DDDDDDDDDDDDDDDD#DD#DDDD##...#',
  '#..#.#DD#D#DDD################=##############D#DD#DDDD##...#',
  '#..#.#DD#D#DDD##DDDDDDDDDDDDDDDDDDDDDDDDDDDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDDD#DDDDDDDDDDDDDDDDDDDDDDDDDDDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDDD#DDD############DDDDDDDDDDDDD#D#DD##DDD#....#',
  '#..#.#DD#D#DDD##DD#DDDDDDDDDDDD##D##D##D#DDD#D#DD##DDD##...#',
  '#..#.#DH#D#DDDD#DD#D##########D#########DDDD#D#DD#oDDD##...#',
  '#..#.#DD#D#rDDD#DD#D#DDDDDDDD#DDDDDDDDD##DDD#D#DD#DDDD##...#',
  '#..#.#DD#D#DDDD#DD#D#DDDDDDDD#DDDDDDDDD#DDDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDD##DD#D#DDDDDDDD#BDDDDDDDD#DRDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDD##DD#D#DDDDDRDD#DDDoDDDDD#DD#D#D#DD##DDD#....#',
  '#..#.#DD#D#D#DD#DH#D#DD#D###########DDD#DDDD#D#DD##DDD#....#',
  '#..#.#DD#D#DDoD#DD#D#DD#D#DDDDDDDD#DDDD#DYDD#D#DD#DDDD##...#',
  '#..#.#DD#D##DDD#DD#D#DD#D#DDDDDDDD#DDDD#DDDD#D#DD#DDDD##...#',
  '#..#.#DD#D#DDDD#DD#D#DD#D#DDDDDDDD##DDD#DDDD#D#DD##DDD##...#',
  '#..#.#DD#D#D#DD#DD#D#DD#D#DDDDDDDD##DDD#DDDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDLD#DD#D#DD#DDDDDDADDD#DDDD#DD#D#D#DD##DDD#o...#',
  '#..#.#DD#D#DDDD#DD#D#DD#D#DDDDDDDD#DDDD#DDDD#D#DD##DDD##...#',
  '#..#.#DD#D##DDD#DD#D#DD#D#DDDDDoDD##DDD#DDDD#D#DD#DDDD##...#',
  '#..#.#DD#D#DDDD#DD#D#DD#D#DDDDDDDD#DDDD#DDDD#D#DD#DDDD##...#',
  '#..#.#DD#D#DRDD#DD#D#DD#D##########DDDD#DDDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDDD#DD#D#DDD##DDD##DD#DDDDD#DD#D#D#DD##DDD##...#',
  '#..#.#DD#D#D#D##DD#D#DDDDDDDDDDDDDDDDDD#DDDD#D#DD##DDD#....#',
  '#..#.#DD#D#DDD##DD#D#DDDDDDDDDDDDDDDDDD#DDDD#D#DD##DDD#....#',
  '#..#.#DD#D#DDD##DD#D#DDDDDDDDDDDDDDDDDD##DDD#D#DD#DDDD##...#',
  '#..#.#DD#D#DDD##DD#D####################DDDD#D#DD#DDDD##...#',
  '#..#.#DD#D#DDDD#DD#DDDDDDDDDD##D#DD#D#D##DDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDDD#DDD#########D#DDDDDDDDDDDDDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDD##DDDDDDDDDDDDD#DDDDDDDDDDDDDD#D#DD##DDD#....#',
  '#..#.#DD#D#DDDD#DDDDDDDDDDDDD#DDDDDDDDDDDDDD#D#DD##DDD##...#',
  '#..#.#DD#D#DDDD###############D##############D#DD#DDDD##...#',
  '#..#.#DD#D#DDD##D###D####D##D#DDDDDDDDDDDDDDDD#DD#DDDD##...#',
  '#..#.#DD#D#DDDDDDDDDDDDDDDDDDD################DDD##DDD##...#',
  '#..#.#DD#D#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##DDD##...#',
  '#..#.#DD#D#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##DDD#....#',
  '#..#.#DD#D####################=####################DDD#....#',
  '#..#.#DD#DDDDDDDDDDDDDDDDDDDDDD###DD####rD####DD###DDD##...#',
  '#..#.#DDD#####################DDDDDDDDDDDDDDDDDDDDDDDD##...#',
  '#..#.#DDDDDDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDD##...#',
  '#..#.#DDDDDDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDD##...#',
  '#..#.#######################.###########################...#',
  '#..#.........................#####..####..####r.####.###...#',
  '#...#########################..............................#',
  '#..........................................................#',
  '#..........................................................#',
  '############################################################',
];

// REGIONS (DESIGN.md §3.4) — the spiral is annular, so each lap is tiled by its four legs (north / east /
// south / west, each covering that lap's five rings) plus the chamber: 21 rectangles, no overlap, every non-solid
// cell owned. Lap 0 is plain floor; laps 1–5 are `D` bands (lamp ×(0.95 − 0.08·lap), burn ×(1.1 + 0.12·lap)), so
// every one of them is a deep pocket by DESIGN.md §3.5.
export const REGIONS = [
  // lap 0 — rings 1–5, the only warm floor in the zone
  { id: 's_outer',      name: 'The Outer Walk',              x: [1, 58],  z: [1, 5],   loot: { oil: 2, relic: 0, rich: 0 } },
  { id: 's_eastwalk',   name: 'The Eastern Arm',             x: [54, 58], z: [6, 53],  loot: { oil: 1, relic: 0, rich: 0 } },
  { id: 's_procession', name: 'The Processional Arm',        x: [1, 58],  z: [54, 58], loot: { oil: 0, relic: 1, rich: 0 } },
  { id: 's_firstcrack', name: 'The First Crack',             x: [1, 5],   z: [6, 53] },
  // lap 1 — rings 6–10, the first turn: fast hunter #1 walks it awake from the first step
  { id: 's_turn',       name: 'The First Turn',              x: [6, 53],  z: [6, 10],  deep: true, loot: { oil: 1, relic: 0, rich: 0 } },
  { id: 's_turneast',   name: 'The First Turn, East Leg',    x: [49, 53], z: [11, 48], deep: true, loot: { oil: 1, relic: 0, rich: 0 } },
  { id: 's_ashpits',    name: 'The Ash Pits',                x: [6, 53],  z: [49, 53], deep: true, loot: { oil: 0, relic: 1, rich: 0 } },
  { id: 's_turnwest',   name: 'The First Turn, West Leg',    x: [6, 10],  z: [11, 48], deep: true },
  // lap 2 — rings 11–15, the west leg widens into the Choir (the Lampwight's beacon runs its whole length)
  { id: 's_second',     name: 'The Second Turn',             x: [11, 48], z: [11, 15], deep: true },
  { id: 's_lecterns',   name: 'The Broken Lecterns',         x: [44, 48], z: [16, 43], deep: true },
  { id: 's_secondsouth', name: 'The Second Turn, South Leg', x: [11, 48], z: [44, 48], deep: true },
  { id: 's_choir',      name: 'The Choir of Stones',         x: [11, 15], z: [16, 43], deep: true, loot: { oil: 1, relic: 1, rich: 1 } },
  // lap 3 — rings 16–20, the east leg widens into the Weeping Wall (false light beside the rich relic)
  { id: 's_third',      name: 'The Third Turn',              x: [16, 43], z: [16, 20], deep: true },
  { id: 's_weep',       name: 'The Weeping Wall',            x: [39, 43], z: [21, 38], deep: true, loot: { oil: 0, relic: 0, rich: 1 } },
  { id: 's_thirdsouth', name: 'The Third Turn, South Leg',   x: [16, 43], z: [39, 43], deep: true },
  { id: 's_thirdwest',  name: 'The Third Turn, West Leg',    x: [16, 20], z: [21, 38], deep: true },
  // lap 4 — rings 21–25, the low Antechamber and the last walled approach to the chamber door
  { id: 's_ante',       name: 'The Antechamber',             x: [21, 38], z: [21, 25], deep: true, loot: { oil: 1, relic: 0, rich: 1 } },
  { id: 's_anteeast',   name: 'The Fourth Turn, East Leg',   x: [34, 38], z: [26, 33], deep: true },
  { id: 's_antesouth',  name: 'The Fourth Turn, South Leg',  x: [21, 38], z: [34, 38], deep: true },
  { id: 's_approach',   name: 'The Chamber Approach',        x: [21, 25], z: [26, 33], deep: true },
  // lap 5
  { id: 's_chamber',    name: 'The Chamber of the Source',   x: [26, 33], z: [26, 33], deep: true, loot: { oil: 1, relic: 0, rich: 0 } },
];

// SHORTCUTS (DESIGN.md §3.6) — three radial fissures, one per lap wall from lap 0 to lap 3. A fissure joins lap L
// to lap L+1 ONLY (never L+2), so player.lap stays monotonic and endgame.onDeeper keeps firing on every lap line.
// Each is barred on the lap-L side (`openFrom` names the deeper flank) and cuts the whole remaining lap: the walk
// round the gallery, the spur behind it and the first stretch of the next lap.
export const SHORTCUTS = [
  { id: 's_fissure1', name: 'The First Fissure',  cells: [[5, 7]],   openFrom: 'E', from: 's_firstcrack',  to: 's_turn',         saves: 354 },
  { id: 's_fissure2', name: 'The Second Fissure', cells: [[30, 49]], openFrom: 'N', from: 's_ashpits',     to: 's_secondsouth',  saves: 152 },
  { id: 's_fissure3', name: 'The Third Fissure',  cells: [[30, 15]], openFrom: 'S', from: 's_second',      to: 's_third',        saves: 112 },
];

// ANCHORS — the test contract (cells). Every suite reads these instead of hard-coding a cell.
export const ANCHORS = {
  entry: [2, 2],                        // V — the elevator, lap 0 (a corner cell cannot hold the §3.7 check-9 pocket)
  altar: [30, 30],                      // A — lapOf(altar) === 5
  altarStand: [30, 31],                 // stand here facing north (yaw 0) to interact with the altar
  hunters: [[7, 20], [17, 25]],         // fast ×2: lap 1 (awake) and lap 3 (dormant until lap 2)
  brute: [30, 23], falseLight: [41, 26], lampwight: [13, 30],   // row-major: B (lap 4) · Y (lap 3) · L (lap 2)
  // one walk cell per lap: teleport here to cross that lap line (endgame.onDeeper)
  lap1: [6, 20], lap2: [11, 20], lap3: [16, 20], lap4: [21, 22], lap5: [28, 30],
  // the doors between laps (openings in each lap wall) and the far end of each lap's gallery walk
  doors: [[28, 54], [28, 10], [30, 44], [30, 20], [25, 30]],
  crossings: [[4, 10], [30, 50], [30, 14], [28, 40], [24, 24]],
  shortcuts: {
    s_fissure1: [5, 7],   s_fissure1Barred: [4, 7],   s_fissure1Far: [6, 7],
    s_fissure2: [30, 49], s_fissure2Barred: [30, 50], s_fissure2Far: [30, 48],
    s_fissure3: [30, 15], s_fissure3Barred: [30, 14], s_fissure3Far: [30, 16],
  },
};

export const META = {
  name: 'The Source', entry: 'V', exit: 'elevator',
  burnMul: 1.0, lampMul: 1.0, deepStyle: 'bands',
  // lapOf(cx, cz, map) reads this: ring = min(cx, cz, w-1-cx, h-1-cz), lap = min(maxLap, floor((ring-1)/band)).
  // band 5 on the 60×60 grid (DESIGN.md §3.4): five rings per lap, laps 0–5, the chamber at lap 5.
  bands: { band: 5, maxLap: 5 },
  hunters: ['fast', 'fast'],
  // row-major: B (30,23) lap 4 · Y (41,26) lap 3 · L (13,30) lap 2 (endgame dormancy wakes each at lap − 1)
  creatures: [{ kind: 'brute', leash: 14 }, { kind: 'falseLight' }, { kind: 'lampwight' }],
  npc: null, npcs: {}, gate: null, spots: [],
  loot: { oil: 8, relic: 3, rich: 3 }, points: 32,
  size: SIZE, regions: REGIONS, shortcuts: SHORTCUTS, anchors: ANCHORS,
  targets: { size: 60, walkable: 2300, wallShare: 0.361, route: 1800 },   // DESIGN.md §3.3
};
