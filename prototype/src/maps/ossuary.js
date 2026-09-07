// src/maps/ossuary.js — The Ossuary: the ASCII rows and every map-shaped fact about this zone.
// ONE AUTHOR OWNS THIS FILE (DESIGN.md §3.7 authoring contract). Exports exactly:
//   ID SIZE ROWS REGIONS SHORTCUTS ANCHORS META
// maps.js composes ZONES.ossuary = { id, ...TUNING.ossuary, ...META, rows: ROWS } and owns everything that is not
// map-shaped (PALETTES, requires/lockReason, intro/threat, ambience). Legend: DESIGN.md §3.1.
// Cells are [cx, cz] = [column, row]; `idx = cz * SIZE + cx`.
//
// 62×62 (DESIGN.md §3.3/§3.4). Claustrophobia is the point: 63 % of the grid is bone and rock, every corridor is
// one cell wide (only the galleries, the pits and the chambers they open into are broader), junctions come every few
// steps, and each band of the map hangs off a single choke — so a chase has nowhere to go but forward or back.
//
// READING THE ZONE (bands north → south)
//   z1–12   the four north galleries: West Gallery (Oren) · the Reliquary behind the Censer · Central Gallery ·
//           East Gallery. All deep (`D`): lamp ×0.6 and oil burning ×1.3 make the north the expensive half.
//   z13     the north corridor, cut in two by a bone fall at x18–22: the west half (x1–17) serves the West Gallery,
//           the east half (x23–38) the Charnel Wheel, the Central Gallery and, through it, everything east.
//   z14–28  West Ossuary (two dead-end pits) · The Charnel Wheel (rim ring, eight radial spokes, deep hub — the only
//           curve in the zone) · the Spine's seven-leg switchback (x33–38) · the East Bone-Pit (deep, spot 0).
//   z29     the middle corridor: the Lime Pits' door (3,30), the Winding Stair (19,30), both Nave chambers, the
//           Wheel Rim's barred door (22,28) and the foot of the Spine switchback (38,29).
//   z30–44  The Lime Pits (ledges round open pits — the one long sightline) · Nave of Bones · The Deep Stacks
//           (honeycomb of 2×2 deep cells, the deepest pockets) · the Bone Stair (three switchback landings, z39/41/43).
//   z45     the south artery, dog-legged twice round bone falls; the two barred doors (7,44) and (47,44) stand on it.
//   z46–61  The South Vault (the 45 s vigil) · the Cage Vestibule with the elevator and its east crypt.
//
// FIRST RUN (every shortcut shut) the zone is one long chain: cage → artery → Bone Stair → z37 → Winding Stair →
// z29 → Lime Pits / Nave / Spine switchback → z13 east → Central Gallery → East Gallery → East Bone-Pit → Deep
// Stacks. Greedy full clear: 1082 cells (§3.3 asks ≥ 1000) — ~22 min of walking at the Ossuary's measured 0.8 cells/s.
// WITH THE THREE DOORS OPEN each barred door becomes a stair between bands (§3.6): the Chute drops the artery straight
// into the Lime Pits and out onto z29, the Wheel Rim carries z29 up through the wheel into z13 east, the Stacks Door
// opens the artery into the honeycomb and the pit above it. The same clear is 638 cells = 59 % of the first run.

export const ID = 'ossuary';
export const SIZE = 62;

export const ROWS = [
  '##############################################################',
  '##############################################################',
  '##DD#D#DD#D#DR##########DDDDDD##DDDDDD########################',
  '##DD#D#DD#D#DD##DDDDDD##DD#oDD##DDD#DD##DDD#DD#DD#DD#DD#DD#DD#',
  '##DD#D#DD#D#DD##DDDRDD##DD#DDD##DDD#DD##DoD#DD#DD#DD#DD#DD#DD#',
  '##DD#D#DD#D#DD##DDDDDD##DD#DDD##DDD#DD##DDD#DD#DD#DD#DD#DD#DD#',
  '##DDDDDDDDDDDD##DRDDDD##DDDDDD..DDDDDD..DDDDDDDDDDDDDDDDDDDDD#',
  '##DDDNDDDDDDDDDXDDDDDD##DD#DDD##DDD#DD##DDDDDDDDDDDDDDDDDDDDD#',
  '##DD#D#DD#D#DD##DDDDDD##DD#DDD##DDD#DD##DDD#DD#DD#DD#DD#DD#DD#',
  '##DD#D#DD#D#DD##DDDGDD##DD#DDD##DDr#DD##DDD#DD#DD#DR#DD#DD#DD#',
  '##DD#D#DD#D#DD##DDDDDD##DD#DDD##DDD#DD##DDD#DD#DD#DD#DD#DD#Dr#',
  '##Dr#D#DD#D#DD##DDDDDD###D###############################D####',
  '#####D######D############D###############################D####',
  '#.................#####................#DDDDoDDDDDDDDDDDDDDDD#',
  '###.####.##.##.#.##.........##.##.#######D#################D##',
  '#.....#####.######..###.###..####...r..##D#################D##',
  '#.....#####.#####...###.###...########.##D#################D##',
  '#.o...#####.#####.#..##.##..#.###......##D#################D##',
  '#.....#####.#####.##..#.#..##.###.#######D####DDDDDD#######D##',
  '#.....#####.#####.###.DDD.###.###.r....##D####DDDDDD#######D##',
  '######.#.##.#.###.....DDD.....########.##DDDDDDDDDDD#######D##',
  '#..............##.####DDD.###.###......##D####DDDDCD#######D##',
  '###.##.#.##.#####.###.Y.#..##o###.#######D####DDDDDD#######D##',
  '###.#####......##.##.r#.##..#.###......##D####DDDDDD#######D##',
  '###.#####......##....##.###...########.##D########D##DDDDDDD##',
  '###.#####......###..###.###..####......##D########D###########',
  '###.#####....r.####.........#####.#######D########D##DDDDDD###',
  '###.#####......#######.##########......##D########D##DDDrDD###',
  '###.##.#.#######.#####=#######.#######.##D########D##DDDDDD###',
  '#......................................#DDDDDDDDDDDDDDDDDDD###',
  '###D############....####.#####.###################D###########',
  '##DDDDDDDDrDDD##.#####.....#....##########RDDDD#DDDDD#DD######',
  '##D##########D##.#####.....#....##########DD#DDDDD#DDDDD######',
  '##D##########D##.#####...o.#.r..##########D###D########D######',
  '##D##########D##.....#.....#....##########DD#DD#DDDDD#DD######',
  '##D##########D######.#.....#....##########DD#oDDDD#DDDDD######',
  '##D##########D######.######################D####D#####D#######',
  '##DDDDDDDDDDDD#................H.......###DD#DDDDD#DDDDD######',
  '##D##########D##.#########################DDDDD#DDDrY#DD######',
  '##D##########D##......................####D######D#####D######',
  '##D##########D#######################.####DD#DDDDD#DDDDD######',
  '##D##########R##......................####DDDDD#DDDDD#RD######',
  '##D##########D##.##########################D#####D#####D######',
  '##DDDoDDDDDDDD##......................####DDDDDDDDDDDDDD######',
  '#######=#############################.#########=##############',
  '#...........##...##..................................#########',
  '###.###.###.##.#.##.####.######.######.#####.###.#...#########',
  '###.###.###....#....######DDDDDDDDDDD#############...#########',
  '###.###.##################DDDDDDDDoDD#########################',
  '###.###.##################DDDDCDDDDDD#########################',
  '###.###.##################DDDDDDDDDDD#########################',
  '###.###.##################DDDDDDDDDDD#########################',
  '###.###.##.###.###############################################',
  '#......................#######################################',
  '#######.############.#########################################',
  '####.......#####.....#########################################',
  '####.......#####.....#########################################',
  '####.................#########################################',
  '####...V...#####..o..#########################################',
  '####.......#####.....#########################################',
  '####.......###################################################',
  '##############################################################',
];

// REGIONS (DESIGN.md §3.4) — inclusive extents that partition every non-solid cell exactly once. `deep` is the
// §3.7 check-6 contract (≥ 60 % `D` and every cell in a deep neighbourhood, so the lamp really shrinks): six of the
// seven pockets the relocation table names carry it. The seventh, the vault's `D` run (x26–36, z47–51), lives inside
// `o_vault`, whose extent has to cover the south corridors as well, so it is a pocket by geometry and not by flag.
export const REGIONS = [
  { id: 'o_wgal',   name: 'The West Gallery',      x: [ 1, 14], z: [ 1, 12], deep: true, loot: { relic: 1, rich: 1 } },   // stacked-bone bays off a cross aisle; Oren's cell
  { id: 'o_relic',  name: 'The Reliquary',         x: [15, 22], z: [ 1, 12], deep: true, loot: { rich: 2 } },   // sealed but for the Censer gate; the Warden's post
  { id: 'o_cgal',   name: 'The Central Gallery',   x: [23, 38], z: [ 1, 12], loot: { oil: 1, relic: 1 } },   // two chambers and the north crossing: the only way east
  { id: 'o_egal',   name: 'The East Gallery',      x: [39, 61], z: [ 1, 12], deep: true, loot: { oil: 1, relic: 1, rich: 1 } },   // the long north bone-pit; its far end drops into the pit
  { id: 'o_west',   name: 'The West Ossuary',      x: [ 1, 14], z: [13, 29], loot: { oil: 1, relic: 1 } },   // the old west pits, two dead ends off a zig-zag
  { id: 'o_wheel',  name: 'The Charnel Wheel',     x: [15, 31], z: [13, 27], loot: { oil: 1, relic: 1 } },   // rim ring, eight radial spokes, deep hub
  { id: 'o_spine',  name: 'The Spine',             x: [32, 39], z: [13, 44], loot: { relic: 2 } },   // the seven-leg switchback and the z37 crossing
  { id: 'o_pit',    name: 'The East Bone-Pit',     x: [40, 61], z: [13, 29], deep: true, loot: { oil: 1, relic: 1 } },   // ledges round open pits, two chambers, spot 0
  { id: 'o_nave',   name: 'The Nave of Bones',     x: [15, 31], z: [28, 44], loot: { oil: 1, relic: 1 } },   // two bone chambers on z29, the Winding Stair, the Bone Stair
  { id: 'o_lime',   name: 'The Lime Pits',         x: [ 1, 14], z: [30, 44], deep: true, loot: { oil: 1, relic: 1, rich: 1 } },   // 1-wide ledges round open pits: the one long sightline
  { id: 'o_stacks', name: 'The Deep Stacks',       x: [40, 61], z: [30, 44], deep: true, loot: { oil: 1, relic: 1, rich: 2 } },   // honeycomb of 2x2 deep cells: the deepest pockets
  { id: 'o_cage',   name: 'The Cage Vestibule',    x: [ 1, 22], z: [45, 61], loot: { oil: 1 } },   // the elevator cage, its lit sill and the east crypt
  { id: 'o_vault',  name: 'The South Vault',       x: [23, 61], z: [45, 61], loot: { oil: 1 } },   // the deep vigil vault and the south crypt runs
];

// SHORTCUTS (DESIGN.md §3.6). Barred from the entrance side, opened for good with E from `openFrom` — the far
// flank, always the deep side. `saves` = the detour the door removes, measured with every shortcut shut, gates open.
export const SHORTCUTS = [
  // The Lime Chute: the bars stand at the head of the cage's own corridor, 13 cells from the elevator and in plain
  // sight of it. Behind them is the Lime Pits' south ledge, 171 cells away on the first run.
  { id: 'o_chute',     name: 'The Lime Chute',  cells: [[7, 44]],  openFrom: 'N', from: 'o_cage',  to: 'o_lime',   saves: 158 },
  // The Stacks Door: the artery's east end under the honeycomb. Opening it turns the zone's deepest pocket set
  // (2 rich relics, a false light) and the bone-pit above it into a 60-cell walk instead of a 330-cell one.
  { id: 'o_stackdoor', name: 'The Stacks Door', cells: [[47, 44]], openFrom: 'N', from: 'o_vault', to: 'o_stacks', saves: 288 },
  // The Wheel Rim: the middle corridor's north wall, under the Charnel Wheel's south rim. Opened from inside the
  // wheel it becomes the stair from z29 up into the north galleries, past the false light in the spoke.
  { id: 'o_rim',       name: 'The Wheel Rim',   cells: [[22, 28]], openFrom: 'N', from: 'o_nave',  to: 'o_wheel',  saves: 92 },
];

// ANCHORS (DESIGN.md §3.7) — THE TEST CONTRACT. Cells, not world units: a suite teleports to (cx + 0.5, cz + 0.5).
// Every feature that moved in this commit moved its anchor with it; no suite may hard-code a grid cell again.
export const ANCHORS = {
  entry: [7, 58],                        // V — the elevator cage (5×5 clear, three free 4-neighbours)
  gate: [15, 7],                         // X (Censer) — set in the wall column x15
  gateNear: [14, 7],                     // the reachable flank: the West Gallery's alcove (lit here, the Warden
  gateFar: [16, 7],                      // cannot see you while the gate is shut) / the flank inside the Reliquary
  oren: [5, 7],                          // N — Oren the Oil-press Keeper, in the gallery's cross aisle
  spot0: [50, 21], spot1: [30, 49],      // C cells, row-major: the east bone-pit ledge · the south vault (45 s vigil)
  hunter: [31, 37],                      // H — the fast hunter, on the Spine's south crossing
  warden: [19, 9],                       // G — the reliquary Warden, facing N (both R inside its 9 u / 70° cone)
  wardenProbe: [19, 7],                  // 2 cells up its axis, inside the pocket: lit here → ALERT
  falseLights: [[22, 22], [52, 38]],     // Y, row-major: the wheel's SW spoke · the honeycomb cell in the Stacks
  corridor: [10, 29],                    // a 1-wide bone corridor cell (the z29 middle run)
  wheelHub: [23, 20], wheelRim: [23, 26],// the wheel's deep hub · its south rim, inside the Wheel Rim door
  limeLedge: [7, 43],                    // the Lime Pits' south ledge, inside the Chute
  pitLedge: [50, 24], stacksCell: [51, 37],  // the pit's ledge down to the Stacks · a honeycomb cell
  boneStair: [24, 43], windStair: [16, 32],  // the artery's switchback climb · the z29 → z37 winding stair
  cgal: [27, 6], egal: [50, 6],          // the north galleries' aisles (the only route east runs through both)
  limeEntry: [3, 30], stacksEntry: [50, 30], // the first-run door into each sealed deep pocket
  switchback: [34, 19],                  // a leg of the Spine's switchback (its second relic is 1 cell west)
  vault: [30, 49], cageSill: [7, 55],    // the deep south vault · the lit sill north of the cage
  shortcuts: {
    o_chute: [7, 44], o_chuteNear: [7, 45], o_chuteFar: [7, 43],
    o_stackdoor: [47, 44], o_stackdoorNear: [47, 45], o_stackdoorFar: [47, 43],
    o_rim: [22, 28], o_rimNear: [22, 29], o_rimFar: [22, 27],
  },
};

// META — everything ZONES.ossuary needs that is map-shaped. maps.js adds id/palette/intro/threat/requires/ambience.
export const META = {
  name: 'The Ossuary', entry: 'V', exit: 'elevator',
  burnMul: 1.3, lampMul: 0.85, deepStyle: 'flat',
  hunters: ['fast'],
  // row-major: G (19,9) facing N inside the gated Reliquary (gateOk) · Y (22,22) in the wheel's spoke, 1 cell from
  // the relic (21,23) · Y (52,38) in the Stacks' honeycomb, 1 cell from the relic (51,38) — both seen from ≤ 6 u.
  creatures: [{ kind: 'warden', facing: 'N', sweep: 75, reach: 9, territory: 8, gateOk: true }, { kind: 'falseLight' }, { kind: 'falseLight' }],
  npc: 'keeper', npcs: { keeper: [5, 7] },
  gate: { tool: 'censer', cells: [[15, 7]], opens: 'the reliquary (2 rich relics)' },
  spots: [{ id: 0, cell: [50, 21], label: 'the east bone-pit' }, { id: 1, cell: [30, 49], label: 'the south vault' }],
  loot: { oil: 10, relic: 11, rich: 7 }, points: 78,
  size: SIZE, regions: REGIONS, shortcuts: SHORTCUTS, anchors: ANCHORS,
  targets: { size: 62, walkable: 1420, wallShare: 0.627, route: 1000 },   // DESIGN.md §3.3
};
