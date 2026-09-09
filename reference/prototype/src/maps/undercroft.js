// src/maps/undercroft.js — The Undercroft: the ASCII rows and every map-shaped fact about this zone.
// ONE AUTHOR OWNS THIS FILE (DESIGN.md §3.7 authoring contract). Exports exactly:
//   ID SIZE ROWS REGIONS SHORTCUTS ANCHORS META
// maps.js composes ZONES.undercroft = { id, ...TUNING.undercroft, ...META, rows: ROWS } and owns everything that is not
// map-shaped (PALETTES, requires/lockReason, intro/threat, ambience). Legend: DESIGN.md §3.1.
// Cells are [cx, cz] = [column, row]; `idx = cz * SIZE + cx`.
//
// 62×62 (DESIGN.md §3.3: walkable 2380 ±5 %, walls ~35.7 %, full-clear route ≥ 600 cells with every shortcut shut,
// ≤ 70 % of it with all three open). The gentlest zone and the tutorial ground: you learn the loop here.
//
// THE SHAPE OF THE PLACE (bands of rows; each wall row carries the doors between the bands)
//   z1–11  north crypts     NW (behind the Pry Bar) · Middle · NE (the Warden's ground, spot 0)   — all deep
//   z12    wall             two doors only: (27,12) from the Lantern Well, (50,12) from the East Cloister
//   z13–20 the middle floor Chapter House (deep well, one door) · Lantern Well · East Cloister
//   z21    wall             (7,21) chapter · (27,21) well · (41,21) cloister
//   z22–35 the three bays   West · Central · East, walls at x15 / x39; only (15,28) is still a door — the arcade at
//                           x39 has fallen, so the East Bay is reached the long way round (well → Middle → NE crypt →
//                           the cloister ramp) until `u_rood` opens beneath it. That collapse is what makes the Rood
//                           Door worth 152 cells and keeps the §3.7 check-8 route ratio inside 70 %.
//   z36    wall             (30,36) hall↔central bay · `=` u_wingstair (7,36) · `=` u_rood (53,36)
//   z37–51 halls & wings    West Wing (Brute, Wick) · the Great Hall (base hunter, spot 1) · the Collapsed Nave
//   z52    wall             (30,52) stair head↔hall · (57,52) nave↔south apron
//   z53–60 the south        the Stair Head (`S`) · the nave's south apron (deep, 1 R) · `=` u_navedoor (37,55)(37,56)
// One long axis runs the whole map: z44 from the west wing's vestibule (x10) through the Great Hall, the rood-screen
// door (37,44) and the length of the nave to x60 — the processional aisle, and the zone's only long sightline.

export const ID = 'undercroft';
export const SIZE = 62;

export const ROWS = [
  '##############################################################',
  '#DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDDD#',
  '#DD#DD#RD#DD#DD#DD#DD#DD#DD#DD#DD#D#DDDDD#DDD#DDD#DDD#DDD#DDD#',
  '#DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#D#DoDDD#DDD#DDD#DDD#DDD#DDD#',
  '#DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDDDD#DDD#DDD#DDD#DDD#DDD#',
  '#DDDNDDDDDDDDDD#DDDDoDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDDD#',
  '#D#DD#DD#DD#DDDXDDD#DD#DD#DD#DD#DDDDDD####################DDD#',
  '#DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDD#DDDD#DDDDDDD#DDD#DDDD#',
  '#DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDD#DDDD#DDDDDDD#DDD#DDDD#',
  '#DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#D#DDDDDDDDDDDDDDDDDDDDDDDDr#',
  '#Do#DD#DD#DD#DD#DD#DD#DD#DD#DD#rD#D#DDDDDDGDDDDDCDDDDDDDDDDDD#',
  '#DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDDD#',
  '###########################D######################D###########',
  '#####DDDDDDDD#####...................#.........P...P.........#',
  '###DDDDDDDDDDDD###..P..P........P..P.#....P.............P....#',
  '##DDDPDDDDDPDDDD##......#######......#.......#########.......#',
  '#DDDDDDDrDDDDDDDD#......#######...o..#....P#.#########..P..o.#',
  '#DDDDDDDDDDDDRDDD#......#######......#.....#.#########.......#',
  '##DDDPDDDDDPDDDD##......#######......#....P#.#########..P....#',
  '###DDDDDDDDDDDD###..P..P........P..P.#.....#.................#',
  '#####DDDDDDDD#####...................#.....#...P...P.........#',
  '#######D###################.#############.####################',
  '#..............#........#............P.#..........P......#...#',
  '#..P.......P...#........#......#######.#.................#...#',
  '#..............#..P.P...#.P.P.P#######.#...P###########.P#...#',
  '#########......#........#......#######.#....###########..#...#',
  '#..............#..P.....#......#######.#....###########..#.r.#',
  '#...P...P...P..#...............#######.#....###########..#...#',
  '#......................................#..P.###########......#',
  '#..............#.......................#....###########......#',
  '#...P...P...P..#......#................#....###########.....P#',
  '#..............#.####.#...P.P.P...P.P..#....###########......#',
  '#.....##########.####.#................#...P.................#',
  '#..............#.####.#................#..........P.....P....#',
  '#.rP.......P...#.####.#..............o.#.....................#',
  '#..............#......#................#.....................#',
  '#######=######################.######################=########',
  '#.....#..#....#.........####.....#####.....#.....###.........#',
  '#.r...#..#....#.o.......####.....#####...P.#P....###....P..o.#',
  '#..P..#..#..P.#.........####.....#####.....#.....###.........#',
  '#.....#..#....#......................#.......................#',
  '#.....#..#....#..P.P.P.P.P.P.P.P.P.P.##########.##############',
  '#....B........#......................#.......................#',
  '#######..#....#......................#...P...P.....P.....P...#',
  '#.....#..#..................H................................#',
  '#.....#..#....#......................#...P...P.....P.....P...#',
  '#........######......................######.###########.######',
  '#...N.#..#....#..P.P.P.P.P.P.P.C.P.P.#........#..............#',
  '#.....#.......#####..................#...#....#.....#..#.....#',
  '#######..#....#####..................#...#....#.....#..#.....#',
  '#######..#.o..#####.....r..........o.#...#....#.....#..#..r..#',
  '#######..#....#####..................#...#....#.....#..#.....#',
  '#######..#..P.################.##########################.####',
  '#######..#....###########............##############..........#',
  '#######..#....###########............#DDDDDDDDDDD............#',
  '#######..#....###########..P......P..=DDDDD#DD#DD...#........#',
  '#########################............=DDDRD#DD#DD...#........#',
  '#########################............#DDDDD#DD#DD...#........#',
  '#########################......S.....#########################',
  '#########################..P......P..#########################',
  '#########################............#########################',
  '##############################################################',
];

// REGIONS (DESIGN.md §3.4) — {id, name, x:[x0,x1], z:[z0,z1], deep?, loot?:{oil,relic,rich}}.
// The extents tile the whole grid (every non-solid cell belongs to exactly one region, walls included for free), so a
// `=` cell falls in the region on its barred side and every door cell belongs to the band it opens into.
// `deep: true` = a real deep pocket (≥ 60 % D, every cell in a deep neighbourhood): the lamp shrinks and oil burns.
export const REGIONS = [
  // --- the north crypts (z0–12): deep burial vaults, the richest and the farthest
  { id: 'u_nwcrypt',  name: 'The North-West Crypt', x: [0, 15],  z: [0, 12],  deep: true, loot: { oil: 1, relic: 0, rich: 1 } },
  { id: 'u_midcrypt', name: 'The Middle Crypt',     x: [16, 35], z: [0, 12],  deep: true, loot: { oil: 1, relic: 1, rich: 0 } },
  { id: 'u_necrypt',  name: 'The North-East Crypt', x: [36, 61], z: [0, 12],  deep: true, loot: { oil: 1, relic: 1, rich: 0 } },
  // --- the middle floor (z13–21)
  { id: 'u_chapter',  name: 'The Chapter House',    x: [0, 17],  z: [13, 21], deep: true, loot: { oil: 0, relic: 1, rich: 1 } },
  { id: 'u_well',     name: 'The Lantern Well',     x: [18, 37], z: [13, 21], loot: { oil: 1, relic: 0, rich: 0 } },
  { id: 'u_cloister', name: 'The East Cloister',    x: [38, 61], z: [13, 21], loot: { oil: 1, relic: 0, rich: 0 } },
  // --- the three bays of the old pillar hall (z22–35)
  { id: 'u_bayw',     name: 'The West Bay',         x: [0, 15],  z: [22, 35], loot: { oil: 0, relic: 1, rich: 0 } },
  { id: 'u_bayc',     name: 'The Central Bay',      x: [16, 39], z: [22, 35], loot: { oil: 1, relic: 0, rich: 0 } },
  // the East Bay is cut off from the Central Bay (the x39 arcade has fallen): reached from the cloister ramp above,
  // or, once the Rood Door is lifted, straight up out of the nave. Its relic sits in the far pocket x58–60 / z22–27,
  // 138 BFS cells from the stairs with every shortcut shut and 62 with them open — the deepest walk in the zone.
  { id: 'u_baye',     name: 'The East Bay',         x: [40, 61], z: [22, 35], loot: { oil: 0, relic: 1, rich: 0 } },
  // --- the south half (z36–61)
  { id: 'u_wing',     name: 'The West Wing',        x: [0, 14],  z: [36, 61], loot: { oil: 1, relic: 1, rich: 0 } },
  { id: 'u_hall',     name: 'The Great Hall',       x: [15, 37], z: [36, 52], loot: { oil: 2, relic: 1, rich: 0 } },
  { id: 'u_stair',    name: 'The Stair Head',       x: [15, 37], z: [53, 61], loot: { oil: 0, relic: 0, rich: 0 } },
  { id: 'u_nave',     name: 'The Collapsed Nave',   x: [38, 61], z: [36, 61], loot: { oil: 1, relic: 1, rich: 1 } },
];

// SHORTCUTS (DESIGN.md §3.6). `cells` are the `=` cells of ONE door (1, or 2 where the doorway is 2 wide);
// `openFrom` is the compass side the player must STAND ON to lift the bars (always the side farther from the entry).
// Barred they are solid and sight-blocking, exactly like a closed gate; opened (E, no tool, no cost) they stay open
// for good — persisted per zone in the save, so the next run walks straight in. `saves` = the detour removed, in BFS
// cells between the two flanks with every shortcut shut (measured: scratchpad/walk-undercroft.mjs).
export const SHORTCUTS = [
  // The great processional door beside the stairs: two cells wide, barred 8 cells from `S` and in plain sight of it
  // (the whole stair head can see it), and it opens from the nave's deep south apron — 72 cells away the long way
  // round. The first promise the zone makes and the last one it keeps.
  { id: 'u_navedoor',  name: 'The Nave Door',  cells: [[37, 55], [37, 56]], openFrom: 'E', from: 'u_stair', to: 'u_nave',  saves: 74 },
  // The stair at the head of the wing's spine: seen (barred) on the way to Wick, opened from the West Bay above.
  { id: 'u_wingstair', name: 'The Wing Stair', cells: [[7, 36]],            openFrom: 'N', from: 'u_wing',  to: 'u_bayw',  saves: 80 },
  // The rood door: the nave back up into the East Bay, and from there the cloister ramp to the NE crypt.
  { id: 'u_rood',      name: 'The Rood Door',  cells: [[53, 36]],           openFrom: 'N', from: 'u_nave',  to: 'u_baye',  saves: 152 },
];

// ANCHORS (DESIGN.md §3.7) — THE TEST CONTRACT. Cells, not world units: a suite teleports to (cx + 0.5, cz + 0.5).
// Whoever moves a feature moves its anchor in the same commit; no suite may hard-code a grid cell again.
export const ANCHORS = {
  entry: [31, 58],                      // S — spawn + extraction (Stair Head)
  gate: [15, 6],                        // X (Pry Bar), in the wall column x15 between the middle and NW crypts
  gateNear: [16, 6], gateFar: [14, 6],  // the reachable flank / the sealed flank of the gate
  wick: [4, 47], deacon: [4, 5],        // N cells (meta.npcs)
  spot0: [48, 10], spot1: [31, 47],     // C cells (contract spots, row-major)
  hunter: [28, 44],                     // H base hunter, mid-hall on the processional aisle
  warden: [42, 10], brute: [5, 42],     // creature cells, row-major = meta.creatures order
  // the Warden's east axis (post (42,10) facing E, reach 9, territory 8): every probe below is on row z10
  wardenDark: [46, 10],                 // 4 u east: in the cone, used doused (must never alert)
  wardenProbe: [49, 10],                // 7 u east → ALERT (inside the 8 u territory, on the facing axis)
  wardenNear: [45, 10],                 // 3 u east: close enough for the CHASE catch probe
  wardenAside: [48, 8],                 // 6.3 u: inside its ground but off its lastKnown (the give-up probe)
  wardenOut: [50, 12],                  // the cloister door, 8.25 u: one step out → RETURN
  hall: [26, 44], crypt: [46, 3],       // roomy probe cells used by the screenshot/QA scripts
  // straight, clear runs for the creature-sense probes (LOS along the whole line, no pillar, no item):
  //   lane*  = the processional aisle z44, open from x10 (the wing vestibule) to x60 (the nave's east end).
  //            laneNave (40,44) is its west end inside the nave and laneEastRun (56,44) near its east wall: the whole
  //            23-cell nave aisle is clear, so a creature probe can stand anywhere from +1 to +20 u of either.
  //   ns*    = the Great Hall's column x20, open from z37 to z51 (crosses the aisle at (20,44))
  laneW: [16, 44], laneMid: [31, 44], laneNave: [40, 44], laneEastRun: [56, 44], laneE: [60, 44],
  nsN: [20, 37], nsS: [20, 51],
  park: [26, 49],                       // a quiet hall cell to park scripted hunters on
  spine: [7, 50],                       // the wing's north–south spine, ≥ 4 clear cells north (follower/escort probes)
  // the three shortcuts: the `=` cells, plus each door's barred (near) and openFrom (far) flank
  shortcuts: {
    u_navedoor: [37, 56], u_navedoor2: [37, 55], u_navedoorNear: [36, 56], u_navedoorFar: [38, 56],
    u_wingstair: [7, 36], u_wingstairNear: [7, 37], u_wingstairFar: [7, 35],
    u_rood: [53, 36], u_roodNear: [53, 37], u_roodFar: [53, 35],
  },
};

// META — everything ZONES.undercroft needs that is map-shaped. maps.js adds id/palette/intro/threat/requires/ambience.
export const META = {
  name: 'The Undercroft', entry: 'S', exit: 'stairs',
  burnMul: 1.0, lampMul: 1.0, deepStyle: 'flat',
  hunters: ['base'],
  // row-major creature cells: G (42,10) facing E down the NE crypt · B (5,42) leashed to the west wing (DESIGN.md §5.7)
  creatures: [{ kind: 'warden', facing: 'E', sweep: 75, reach: 9, territory: 8 }, { kind: 'brute', leash: 14 }],
  npc: 'lamplighter', npcs: { lamplighter: [4, 47], deacon: [4, 5] },
  gate: { tool: 'prybar', cells: [[15, 6]], opens: 'the north-west crypt (Deacon Maud, 1 rich relic)' },
  spots: [{ id: 0, cell: [48, 10], label: 'the north-east crypt' }, { id: 1, cell: [31, 47], label: 'the great hall' }],
  loot: { oil: 10, relic: 8, rich: 3 }, points: 49,
  size: SIZE, regions: REGIONS, shortcuts: SHORTCUTS, anchors: ANCHORS,
  targets: { size: 62, walkable: 2380, wallShare: 0.357, route: 600 },   // DESIGN.md §3.3
};
