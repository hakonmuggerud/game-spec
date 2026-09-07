// src/maps/cistern.js — The Cistern: the ASCII rows and every map-shaped fact about this zone.
// ONE AUTHOR OWNS THIS FILE (DESIGN.md §3.7 authoring contract). Exports exactly:
//   ID SIZE ROWS REGIONS SHORTCUTS ANCHORS META
// maps.js composes ZONES.cistern = { id, ...TUNING.cistern, ...META, rows: ROWS } and owns everything that is not
// map-shaped (PALETTES, requires/lockReason, intro/threat, ambience). Legend: DESIGN.md §3.1.
// Cells are [cx, cz] = [column, row]; `idx = cz * SIZE + cx`.
//
// 64×64 (DESIGN.md §3.3/§3.4). Bands: z1–11 north (inlet gallery · flooded vault · drain sump) · z12 wall ·
// z13–45 the lake, flanked by the west and east channels · z45 the south quay · z46 wall · z47–58 south rooms ·
// z59–62 the tram apron. Water is the star: the lake is ONE body (the Drowner's), every other flood — the vault,
// the channels, the Sunken Nave, each filter bed — is sealed off from it by a dry door or a wall line.
//
// Reading the zone: the quay (z45) is the artery, and a dry causeway ring runs off its east end — east causeway
// (x49–50, a 2-cell wade at z26–27) → the north walkway (z13) → the West Aisle (x13–14) and the middle causeway
// (x20, a 2-cell wade at z25–26) → the z33 crossing (a 6-cell wade at x29–34) and the lagoon island. Every one of
// those gaps is a real choice: wade it (slow, heard 7 u, the Drowner's water) or walk the long dry way round.
// The west half hangs off ONE door — the causeway head (12,20), reached along the West Aisle, which the collapsed
// arcade (x15) seals off from the quay so it can only be entered from the lake's north strip; the first run to the
// pump room, the Sunken Nave, the inlet gallery and the flooded vault is therefore a ~200-cell walk. The east half
// hangs off (51,45) and the baffled east channels (the sump at their head, Ines' block at their foot). The three
// shortcuts (§3.6) cut exactly those three walks, and each is barred on the side you first see it from.

export const ID = 'cistern';
export const SIZE = 64;

export const ROWS = [
  '################################################################',
  '#..............##WW#WW#WW#WW#WWWWW#WW#WW#WW#WWW#DDDD#DDDDDDDDDD#',
  '#.r.####.##.##.##WW#PW#WW#PW#WWPWW#WP#WW#WW#PWW#DDDD#DDDDDDDDRD#',
  '#...####.##.##.##WW#WW#WW#WW#WWRWW#WW#WW#WW#WWW#DDDD#DDDDDDDDDD#',
  '#..............##WWWWW#WWWWW#WWWWW#WWWWW#WWWWWW#DDDD#DDDDD#DDDD#',
  '############...##..WWWWWWWWWWWWWWWWWWWWWWWWWWWW#DDDD#DDDDD#DDDD#',
  '#..........##...X..WWWWWWWWWWWWWWWWWWWWWWWWWWWW#DDDD#DDDDD#DDDD#',
  '#.P........##..##..WWWWWWWWWWWWWWWWWWWWWWWWWWWW#DDDDDDDDDD#DDDD#',
  '#..##...##.....##WWWWW#WWWWW#WWWWW#WWWWW#WWWWWW#DDDDDDDDDD#DDDD#',
  '#..##...##.....##WW#WW#WW#WW#WWWWW#WW#WR#WW#WWW#DoDDDDDDDD#DDDD#',
  '#.....##....o..##WW#WP#WW#PW#WWPWW#WP#WW#WW#PWW#DDDDDDDDDD#DDDD#',
  '#.....##.......##WW#WW#WW#WW#WWWWW#WW#WW#WW#WWW#DDDDDDDDDD#DDDD#',
  '###.####################################################.#######',
  '#WW.#WW.#WW.#......................................#.WW#.WW#.WW#',
  '#WW..WW.#WW.#..#WWWW.WWWWWWWWWWWWWWWWWWWWWW.....W..#.WW..WW#.WW#',
  '#WW.#WW.#WW.#..#PWWW.WWWPWWWPWWWPWWWPWWWPWW.###.P..#.WW#.WW#.WW#',
  '#WW.#WW.#.###..#WWWW.WWWWWWWWWWWWWWWWWWWWWW.###....###.#.WW#.WW#',
  '#WW.#WW.#WW.#..#....H.......WWWWWWWWWWWWWWW.....W..#.WW#.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWW.......WWWWWWWWWWWWWWWW.....W..#.WW#.WW#.WW#',
  '#WW.#WW.###.#..#W#WW..WWWW.WWWWWWWWWWWWWWWWWWWWWW..#.###.WW#.WW#',
  '#WW.#WW.#WW....#W#WW..WRWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#',
  '#WW.#WW.#WW.#..#W#WW..WWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#',
  '#WW.#WW.#.###..#W#WW..WWWW.WWWWWWWWWWWWWWWWWW#WWW..###.#.WW#.WW#',
  '#WW.#WW.#WW.#..#W#WW.......WWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#',
  '#WW.#WWo#WW.#..#WWWW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#oWW#.WW#',
  '#WW.#WW.###.#..#PWWWPWWWPWWWPWWWPWWWPWWWPWWWP#WWP..#.###.WW#.WW#',
  '#WW.#WW.#WW.#..#W#WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW#.WW#.WW#.WW#',
  '#WW.#WW.#WW.#..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWWWWWWWW#.WW#.WW#.WW#',
  '#WW.#WW.#.###..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..###.#.WW#.WW#',
  '#WW.#WW.#WW.#..#W#WW.WWWWWWWWWWwWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#',
  '#WW.#WW.#WW.#..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#',
  '#WW.#WW.###.#..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.###.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#',
  '#WWr#WW.#WW.#..#.............WWWWWW................#.WW#.WW#rWW#',
  '#WW.#WW.#.###..#WWWWWWWWWWWWWW..WWWWWWWWWWWWWWWWW..###.#.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..........WWWW..#.WW#.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..######..WWWW..#.WW#.WW#.WW#',
  '#WW.#WW.###.#..#PWWWPWWWPWWWPW..PWW..#....#..WWWP..#.###.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..#....#..WWWW..#.WW#.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..#....#..WWWW..#.WW#.WW#.WW#',
  '#WW.#WW.#.###..#WWWWWWWWWWWWWWH.WWW..##.###..WWWW..###.#.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..........WWWW..#.WW#.WW#.WW#',
  '#WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..........WWWW..#.WW#.WW#.WW#',
  '#WW.#WW.###.#..#WWWWWWWWWWWWWW..WWWWWWWWWWWWWWWWW..#.###.WW#.WW#',
  '#WW.#WW..WW.####WWWWWWWWWWWWWW......WWWWWWWWWWWWW..#.WW#.WW..WW#',
  '#WW.#WW.#WW.=....................C...................WW#.WW#.WW#',
  '###.#############################.##########################.###',
  '#............#WWW#WWW#WWW#..####..####..#.W#WW.WW#WWW##.#.#....#',
  '#.###..###.r.#WWW#.oW#WWW#..####..####..#rW#WW.WW#WWW##........#',
  '#.###..###.###WPW#WPW#WPW#..............#WW#WW.WW#WWW##.#.#....#',
  '#.###......###WWW#WWW#WWW#..............#WW#WW.WW#WWW##.########',
  '#............#WWWWWWWWWWW#..P..P..P..P..#WW#WW.WW#WWW##.###..#.#',
  '#.o...##......WPWWWPWWWPW=..............=.....L.............N..#',
  '#....C##.....#WWWWWWWWWWW=..............#WW#.W.WW#WWW##.###....#',
  '#............#WWWWWWWWWWW#...##....##...#WW#rW.WW#WWW##.###....#',
  '#.##....###..#WWW#WWW#WWW#...##....##...#WW#WW.WW#WWW##.########',
  '#.##....###..#WPW#WPW#WPW#..P..P..P..P..#WW#WW.WW#WW.##.#.#....#',
  '#.##.###.....#WWW#WWW#.rW#..............#WW#WW.WW#WWo##......r.#',
  '#....###.....#WWW#WWW#WWW#..............#WWWWWWWWWWWW####.#....#',
  '###########################............#########################',
  '###########################.....S......#########################',
  '###########################..P......P..#########################',
  '###########################............#########################',
  '################################################################',
];

// REGIONS (DESIGN.md §3.4) — extents are inclusive and partition every non-solid cell exactly once.
export const REGIONS = [
  { id: 'c_head',  name: 'The Sluice Head',    x: [1, 16],  z: [1, 11],  loot: { oil: 1, relic: 1 } },
  { id: 'c_vault', name: 'The Flooded Vault',  x: [17, 46], z: [1, 11],  loot: { rich: 2 } },
  { id: 'c_sump',  name: 'The Drain Sump',     x: [47, 62], z: [1, 11],  deep: true, loot: { oil: 1, rich: 1 } },
  { id: 'c_west',  name: 'The West Channels',  x: [1, 12],  z: [12, 45], loot: { oil: 1, relic: 1 } },
  { id: 'c_lake',  name: 'The Drowned Hall',   x: [13, 50], z: [12, 45], loot: { rich: 1 } },
  { id: 'c_east',  name: 'The East Channels',  x: [51, 62], z: [12, 45], loot: { oil: 1, relic: 1 } },
  { id: 'c_pump',  name: 'The Pump Room',      x: [1, 13],  z: [46, 58], loot: { oil: 1, relic: 1 } },
  { id: 'c_nave',  name: 'The Sunken Nave',    x: [14, 25], z: [46, 58], loot: { oil: 1, relic: 1 } },
  { id: 'c_land',  name: 'The Tram Landing',   x: [26, 40], z: [46, 62] },
  { id: 'c_beds',  name: 'The Filter Beds',    x: [41, 53], z: [46, 58], loot: { oil: 1, relic: 2 } },
  { id: 'c_cells', name: "Ines' Cell Block",   x: [54, 62], z: [46, 58], loot: { relic: 1 } },
];

// SHORTCUTS (DESIGN.md §3.6). Barred from the entrance side, opened for good with E from `openFrom` — the far
// flank. `saves` = the BFS detour the door removes with every shortcut shut (measured, gates open).
export const SHORTCUTS = [
  // The West Bulkhead: a 2-wide dock door in the landing's west wall. Barred flank (26,52) is 14 cells from the
  // stairs and in plain sight of them; the far flank (24,52) is waist-deep in the Sunken Nave, 216 cells away.
  { id: 'c_bulkhead', name: 'The West Bulkhead', cells: [[25, 52], [25, 53]], openFrom: 'W', from: 'c_land', to: 'c_nave', saves: 214 },
  // The East Screen: the landing's east wall onto the filter-bed walkway — the Lampwight's lane and the way to Ines.
  { id: 'c_screen',   name: 'The East Screen',   cells: [[40, 52]],           openFrom: 'E', from: 'c_land', to: 'c_beds', saves: 144 },
  // The Sluice Screen: the dead end at the quay's west tip onto the west channels (and the whole north-west run).
  { id: 'c_sluice',   name: 'The Sluice Screen', cells: [[12, 45]],           openFrom: 'W', from: 'c_lake', to: 'c_west', saves: 92 },
];

// ANCHORS (DESIGN.md §3.7) — THE TEST CONTRACT. Cells, not world units: a suite teleports to (cx + 0.5, cz + 0.5).
// Whoever moves a feature moves its anchor in the same commit; no suite may hard-code a grid cell again.
export const ANCHORS = {
  entry: [32, 60],                       // S — the tram landing apron
  gate: [16, 6],                         // X (Sluice Key)
  gateNear: [15, 6], gateFar: [17, 6],   // the reachable flank (inlet gallery) / the sealed flank (vault landing)
  ines: [60, 52],                        // N (meta.npcs.cartographer)
  spot0: [33, 45], spot1: [5, 53],       // C cells, row-major: the drowned hall (quay) · the pump room
  hunters: [[20, 17], [30, 40]],         // H, row-major: the north cross causeway · the quay stub
  drowner: [31, 29], lampwight: [46, 52],// creature cells, row-major = meta.creatures order
  drownerShore: [33, 44],                // dry quay cell one step off the lake: it surges and can reach you here
  drownerSafe: [33, 45],                 // one full cell back (= spot 0): the 30 s vigil is never caught
  drownerWater: [33, 43],                // the lake water at that shore (wade / plant a lantern in the shallows)
  otherWater: [9, 24],                   // water in the west channels — a different body: a Drowner put here snaps home
  drownerNear: [33, 41],                 // lake water 3 cells north of the shore: SURGE reach, outside a shore lantern's pool
  drownerDeep: [33, 40],                 // lake water 3 cells north of drownerWater: outside a pool planted in the shallows
  lake: [33, 42], quay: [33, 45],        // open lake water · the quay walkway
  island: [22, 18], causeway: [20, 33],  // the lagoon island's rim · the central causeway
  aisle: [13, 30], vault: [18, 6],       // the West Aisle behind the collapsed arcade · the dry landing in the vault
  sump: [55, 8],                         // deep pocket (D): lamp ×0.6
  shortcuts: {
    c_bulkhead: [25, 52], c_bulkhead2: [25, 53], c_bulkheadNear: [26, 52], c_bulkheadFar: [24, 52],
    c_screen: [40, 52], c_screenNear: [39, 52], c_screenFar: [41, 52],
    c_sluice: [12, 45], c_sluiceNear: [13, 45], c_sluiceFar: [11, 45],
  },
};

// META — everything ZONES.cistern needs that is map-shaped. maps.js adds id/palette/intro/threat/requires/ambience.
export const META = {
  name: 'The Cistern', entry: 'S', exit: 'stairs',
  burnMul: 1.0, lampMul: 1.0, deepStyle: 'flat',
  hunters: ['base', 'base'],
  // row-major creature cells (DESIGN.md §5.7): w (31,29) in the lake body · L (46,52) on the filter-bed walkway,
  // 14 cells of open LOS down the lane to Ines' cell block — the run to Ines is an oil tax, not a death trap.
  creatures: [{ kind: 'drowner' }, { kind: 'lampwight' }],
  npc: 'cartographer', npcs: { cartographer: [60, 52] },
  gate: { tool: 'sluice', cells: [[16, 6]], opens: 'the flooded vault (2 rich relics)' },
  spots: [{ id: 0, cell: [33, 45], label: 'the drowned hall' }, { id: 1, cell: [5, 53], label: 'the pump room' }],
  loot: { oil: 7, relic: 8, rich: 4 }, points: 51,
  size: SIZE, regions: REGIONS, shortcuts: SHORTCUTS, anchors: ANCHORS,
  targets: { size: 64, walkable: 2760, wallShare: 0.312, route: 700 },   // DESIGN.md §3.3
};
