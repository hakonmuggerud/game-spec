# The Undercroft — Prototype Design (as implemented)

What `prototype/` actually does, with the numbers read from the code. `index.html` (CSS, overlay DOM, importmap)
loads plain ES modules from `src/` (Three.js 0.160.0 via jsDelivr, no build, no assets: all audio is procedural,
all models are voxel boxes). Serve with `python3 -m http.server 8765 --directory prototype`.

## 1. Goal (what must be felt)

Hub (warm, safe, small flame) → descend → lamp-lit crawl through pitch-black ruin → hunters drawn to your light →
douse / flash / plant lantern to survive → bank loot at the stairs → the flame visibly grows, NPCs come home, buildings
rise → deeper zones open → the Source and a final choice. Die and you lose carried loot (dropped as a retrievable bundle);
hub progress persists.

## 2. Controls (`config.KEYS`, `KeyboardEvent.code`)

| Key | Action |
|---|---|
| Mouse / Arrow keys | Look (pointer lock; arrows 1.9 rad/s fallback) |
| W A S D | Move (walk 2.6 u/s) · Shift sprint (4.2 u/s, noise, FOV 75→80) |
| F | Toggle handlamp (off = stealth, burns nothing) |
| Q | Flash: −flashCost oil, staggers a hunter in a cone ahead; each creature answers it its own way (§5) |
| R | Plant lantern: −lanternCost oil, safe pool at your feet |
| E | Interact, asked in order: endgame (altar / ride up) → npc → hub → items → gates → shortcuts (§3.6) → stairs |
| T | Pour one carried flask into the lamp (+25 oil) |
| M · [ · ] | Mute · volume ±0.1 |
| Tab | Minimap overlay (needs the Cartographer's Table) |
| 1–5 (and numpad) | Menu picks; Enter/Space/E confirm; Esc closes a menu or pauses |
| Backspace ×2 | Title screen: wipe the save (3 s window) |

## 3. World / grid conventions

- 1 cell = 1 world unit; cell (cx, cz) occupies x∈[cx, cx+1), z∈[cz, cz+1); floor top y=0, ceiling y=3.
- Player: radius 0.3, eye height 1.6. Camera fov 75, near 0.05, far 40. Collision: move X then Z, cancel an axis if any
  of the ≤4 overlapped cells is solid (`#`, `P`, closed `X`); off-grid = solid. No Y movement.
- One zone at a time at the origin (`world.loadZone` disposes the previous one); the hub stays resident at x=+60.
- Render at ⅓ resolution (`image-rendering: pixelated`), `MeshLambertMaterial`, per-instance colour jitter ±6%. One
  `InstancedMesh` per block type. Per-zone palette / fog / ambient in `maps.PALETTES` (Undercroft: fog exp2 0.11, ambient
  0x0b0a14; the hub warms its ambient/fog per flame tier, §3.8). All real light is point lights: lamp, planted lanterns, hub
  flame, hub sconces, five hub lanterns.

(§3.3–§3.7 were `DESIGN-maps.md`, the map spec for the enlarged zones and the shortcut mechanic; its §1 is §3.3 here,
§2 → §3.4, §3 → §3.5, §4 → §3.6 and §5 → §3.7, which is what the `DESIGN.md §3.n` comments in `src/maps*` point at.)

### 3.1 Legend

| Char | Meaning |
|---|---|
| `#` / `P` | Wall block / pillar (solid; pillar drawn 0.8 wide) |
| `.` / `D` | Floor / deep floor (Undercroft, Cistern, Ossuary: lamp radius ×0.6, burn ×1.5; Source: per-lap bands, below) |
| `S` / `V` | Stairs / elevator cage: spawn + extraction (cold-blue marker) |
| `o` `r` `R` | Oil flask (1 pt or +25 oil) · relic (3 pts) · deep floor + rich relic (5 pts) |
| `H` | Hunter spawn (one hunter per `H`, row-major; profile from `ZONES[id].hunters`) |
| `L` `G` `Y` `B` | Creature spawns (§5.7): Lampwight · Warden (guard) · false light · Brute. The cell stays floor/deep like `H`; options come from `ZONES[id].creatures` |
| `w` | Water cell **with** a Drowner in it (§5.3) |
| `N` | Captive NPC cell (`ZONES[id].npcs` names who) |
| `C` | Contract spot (row-major index = `spot` in contracts) |
| `W` | Water: walkable; player ×0.55 (sprint ×0.6), hunters ×0.85; wading is heard 7 u away without LOS |
| `X` | Gate: solid until the zone's tool is owned; E opens it from either side, for good (`save.gatesOpened[zoneId] = [toolId]`) |
| `=` | Shortcut (§3.6): **barred** — solid and sight-blocking; needs no tool; E from the `openFrom` side ONLY lifts the bars for good (`save.shortcuts[zoneId] = [shortcutId]`). One or two adjacent cells per door, in a wall line |
| `A` | The Source altar |
| `F` / `0–9` | Hub great flame / hub building anchors (floor for the grid; the hub's `blockMask` adds footprints, props and residents) |

### 3.2 Zones (`maps.ZONES`; row 0 = north, x = column, z = row; one file per zone in `src/maps/`)

| id | name | grid | entry | walkable | burn | lamp | hunters | captives | gate (tool → opens) | requires |
|---|---|---|---|---|---|---|---|---|---|---|
| `undercroft` | The Undercroft | 62×62 | S (31,58) | 2401 | ×1 | ×1 | 1 base (28,44) + Warden (42,10) E + Brute (5,42) leash 14 | Wick (4,47), Deacon (4,5) | Pry Bar → X (15,6), NW crypt | — |
| `cistern` | The Cistern | 64×64 | S (32,60) | 2832 | ×1 | ×1 | 2 base (20,17) (30,40) + Drowner (31,29) + Lampwight (46,52) | Ines (60,52) | Sluice Key → X (16,6), flooded vault | building `tram` |
| `ossuary` | The Ossuary | 62×62 | V (7,58) | 1407 | ×1.3 | ×0.85 | 1 fast (31,37) + Warden (19,9) N `gateOk` + false lights (22,22) (52,38) | Oren (5,7) | Censer → X (15,7), reliquary | building `elevator` + lightTech ≥ 2 |
| `source` | The Source | 60×60 | V (2,2) | 2288 | bands | bands | 2 fast (7,20) (17,25) (+§9) + Brute (30,23) lap 4 + false light (41,26) lap 3 + Lampwight (13,30) lap 2 | — | — | flame tier 4 + Deacon rescued; no banking |

Loot per full clear: Undercroft 10o+8r+3R = 49 · Cistern 7o+8r+4R = 51 · Ossuary 10o+11r+7R = 78 · Source 8o+3r+3R = 32.
Items respawn each expedition. Contract spots: Undercroft (48,10) NE crypt, (31,47) great hall · Cistern (33,45) drowned hall,
(5,53) pump room · Ossuary (50,21) east bone-pit, (30,49) south vault.

Shortcuts (`=`, §3.6 — barred from the entrance side, lifted with `E` from `openFrom` only, permanent):

| zone | shortcut | cells | opens from | links | detour removed | full-clear route: shut → all open |
|---|---|---|---|---|---|---|
| undercroft | `u_navedoor` The Nave Door | (37,55) (37,56) | E | Stair Head ↔ the nave's deep south apron | 74 | 952 → 640 cells |
| undercroft | `u_wingstair` The Wing Stair | (7,36) | N | West Wing ↔ West Bay | 80 | |
| undercroft | `u_rood` The Rood Door | (53,36) | N | Collapsed Nave ↔ East Bay | 152 | |
| cistern | `c_bulkhead` The West Bulkhead | (25,52) (25,53) | W | Tram Landing ↔ Sunken Nave → the whole west half | 214 | 1002 → 678 cells |
| cistern | `c_screen` The East Screen | (40,52) | E | Tram Landing ↔ Filter Beds → Ines, the sump | 144 | |
| cistern | `c_sluice` The Sluice Screen | (12,45) | W | the quay ↔ West Channels | 92 | |
| ossuary | `o_chute` The Lime Chute | (7,44) | N | the cage corridor ↔ the Lime Pits | 158 | 1082 → 638 cells |
| ossuary | `o_stackdoor` The Stacks Door | (47,44) | N | the artery ↔ the Deep Stacks | 288 | |
| ossuary | `o_rim` The Wheel Rim | (22,28) | N | the z29 corridor ↔ the Charnel Wheel | 92 | |
| source | `s_fissure1` The First Fissure | (5,7) | E | lap 0 ↔ lap 1 | 354 | 1926 → 1086 cells |
| source | `s_fissure2` The Second Fissure | (30,49) | N | lap 1 ↔ lap 2 | 152 | |
| source | `s_fissure3` The Third Fissure | (30,15) | S | lap 2 ↔ lap 3 | 112 | |

Each zone's named regions, deep pockets and per-region loot split are §3.4 (and `REGIONS` in `src/maps/<zone>.js`);
`ZONES[id].anchors` is the cell contract every test suite reads instead of a literal (§3.7).

#### The Undercroft — 62×62 (`src/maps/undercroft.js`)
```
    0         1         2         3         4         5         6 
    01234567890123456789012345678901234567890123456789012345678901
  0 ##############################################################
  1 #DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDDD#
  2 #DD#DD#RD#DD#DD#DD#DD#DD#DD#DD#DD#D#DDDDD#DDD#DDD#DDD#DDD#DDD#
  3 #DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#D#DoDDD#DDD#DDD#DDD#DDD#DDD#
  4 #DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDDDD#DDD#DDD#DDD#DDD#DDD#
  5 #DDDNDDDDDDDDDD#DDDDoDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDDD#
  6 #D#DD#DD#DD#DDDXDDD#DD#DD#DD#DD#DDDDDD####################DDD#
  7 #DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDD#DDDD#DDDDDDD#DDD#DDDD#
  8 #DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDD#DDDD#DDDDDDD#DDD#DDDD#
  9 #DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#D#DDDDDDDDDDDDDDDDDDDDDDDDr#
 10 #Do#DD#DD#DD#DD#DD#DD#DD#DD#DD#rD#D#DDDDDDGDDDDDCDDDDDDDDDDDD#
 11 #DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDDD#
 12 ###########################D######################D###########
 13 #####DDDDDDDD#####...................#.........P...P.........#
 14 ###DDDDDDDDDDDD###..P..P........P..P.#....P.............P....#
 15 ##DDDPDDDDDPDDDD##......#######......#.......#########.......#
 16 #DDDDDDDrDDDDDDDD#......#######...o..#....P#.#########..P..o.#
 17 #DDDDDDDDDDDDRDDD#......#######......#.....#.#########.......#
 18 ##DDDPDDDDDPDDDD##......#######......#....P#.#########..P....#
 19 ###DDDDDDDDDDDD###..P..P........P..P.#.....#.................#
 20 #####DDDDDDDD#####...................#.....#...P...P.........#
 21 #######D###################.#############.####################
 22 #..............#........#............P.#..........P......#...#
 23 #..P.......P...#........#......#######.#.................#...#
 24 #..............#..P.P...#.P.P.P#######.#...P###########.P#...#
 25 #########......#........#......#######.#....###########..#...#
 26 #..............#..P.....#......#######.#....###########..#.r.#
 27 #...P...P...P..#...............#######.#....###########..#...#
 28 #......................................#..P.###########......#
 29 #..............#.......................#....###########......#
 30 #...P...P...P..#......#................#....###########.....P#
 31 #..............#.####.#...P.P.P...P.P..#....###########......#
 32 #.....##########.####.#................#...P.................#
 33 #..............#.####.#................#..........P.....P....#
 34 #.rP.......P...#.####.#..............o.#.....................#
 35 #..............#......#................#.....................#
 36 #######=######################.######################=########
 37 #.....#..#....#.........####.....#####.....#.....###.........#
 38 #.r...#..#....#.o.......####.....#####...P.#P....###....P..o.#
 39 #..P..#..#..P.#.........####.....#####.....#.....###.........#
 40 #.....#..#....#......................#.......................#
 41 #.....#..#....#..P.P.P.P.P.P.P.P.P.P.##########.##############
 42 #....B........#......................#.......................#
 43 #######..#....#......................#...P...P.....P.....P...#
 44 #.....#..#..................H................................#
 45 #.....#..#....#......................#...P...P.....P.....P...#
 46 #........######......................######.###########.######
 47 #...N.#..#....#..P.P.P.P.P.P.P.C.P.P.#........#..............#
 48 #.....#.......#####..................#...#....#.....#..#.....#
 49 #######..#....#####..................#...#....#.....#..#.....#
 50 #######..#.o..#####.....r..........o.#...#....#.....#..#..r..#
 51 #######..#....#####..................#...#....#.....#..#.....#
 52 #######..#..P.################.##########################.####
 53 #######..#....###########............##############..........#
 54 #######..#....###########............#DDDDDDDDDDD............#
 55 #######..#....###########..P......P..=DDDDD#DD#DD...#........#
 56 #########################............=DDDRD#DD#DD...#........#
 57 #########################............#DDDDD#DD#DD...#........#
 58 #########################......S.....#########################
 59 #########################..P......P..#########################
 60 #########################............#########################
 61 ##############################################################
```
#### The Cistern — 64×64 (`src/maps/cistern.js`)
```
    0         1         2         3         4         5         6   
    0123456789012345678901234567890123456789012345678901234567890123
  0 ################################################################
  1 #..............##WW#WW#WW#WW#WWWWW#WW#WW#WW#WWW#DDDD#DDDDDDDDDD#
  2 #.r.####.##.##.##WW#PW#WW#PW#WWPWW#WP#WW#WW#PWW#DDDD#DDDDDDDDRD#
  3 #...####.##.##.##WW#WW#WW#WW#WWRWW#WW#WW#WW#WWW#DDDD#DDDDDDDDDD#
  4 #..............##WWWWW#WWWWW#WWWWW#WWWWW#WWWWWW#DDDD#DDDDD#DDDD#
  5 ############...##..WWWWWWWWWWWWWWWWWWWWWWWWWWWW#DDDD#DDDDD#DDDD#
  6 #..........##...X..WWWWWWWWWWWWWWWWWWWWWWWWWWWW#DDDD#DDDDD#DDDD#
  7 #.P........##..##..WWWWWWWWWWWWWWWWWWWWWWWWWWWW#DDDDDDDDDD#DDDD#
  8 #..##...##.....##WWWWW#WWWWW#WWWWW#WWWWW#WWWWWW#DDDDDDDDDD#DDDD#
  9 #..##...##.....##WW#WW#WW#WW#WWWWW#WW#WR#WW#WWW#DoDDDDDDDD#DDDD#
 10 #.....##....o..##WW#WP#WW#PW#WWPWW#WP#WW#WW#PWW#DDDDDDDDDD#DDDD#
 11 #.....##.......##WW#WW#WW#WW#WWWWW#WW#WW#WW#WWW#DDDDDDDDDD#DDDD#
 12 ###.####################################################.#######
 13 #WW.#WW.#WW.#......................................#.WW#.WW#.WW#
 14 #WW..WW.#WW.#..#WWWW.WWWWWWWWWWWWWWWWWWWWWW.....W..#.WW..WW#.WW#
 15 #WW.#WW.#WW.#..#PWWW.WWWPWWWPWWWPWWWPWWWPWW.###.P..#.WW#.WW#.WW#
 16 #WW.#WW.#.###..#WWWW.WWWWWWWWWWWWWWWWWWWWWW.###....###.#.WW#.WW#
 17 #WW.#WW.#WW.#..#....H.......WWWWWWWWWWWWWWW.....W..#.WW#.WW#.WW#
 18 #WW.#WW.#WW.#..#WWWW.......WWWWWWWWWWWWWWWW.....W..#.WW#.WW#.WW#
 19 #WW.#WW.###.#..#W#WW..WWWW.WWWWWWWWWWWWWWWWWWWWWW..#.###.WW#.WW#
 20 #WW.#WW.#WW....#W#WW..WRWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#
 21 #WW.#WW.#WW.#..#W#WW..WWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#
 22 #WW.#WW.#.###..#W#WW..WWWW.WWWWWWWWWWWWWWWWWW#WWW..###.#.WW#.WW#
 23 #WW.#WW.#WW.#..#W#WW.......WWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#
 24 #WW.#WWo#WW.#..#WWWW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#oWW#.WW#
 25 #WW.#WW.###.#..#PWWWPWWWPWWWPWWWPWWWPWWWPWWWP#WWP..#.###.WW#.WW#
 26 #WW.#WW.#WW.#..#W#WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW#.WW#.WW#.WW#
 27 #WW.#WW.#WW.#..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWWWWWWWW#.WW#.WW#.WW#
 28 #WW.#WW.#.###..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..###.#.WW#.WW#
 29 #WW.#WW.#WW.#..#W#WW.WWWWWWWWWWwWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#
 30 #WW.#WW.#WW.#..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#
 31 #WW.#WW.###.#..#W#WW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.###.WW#.WW#
 32 #WW.#WW.#WW.#..#WWWW.WWWWWWWWWWWWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#
 33 #WWr#WW.#WW.#..#.............WWWWWW................#.WW#.WW#rWW#
 34 #WW.#WW.#.###..#WWWWWWWWWWWWWW..WWWWWWWWWWWWWWWWW..###.#.WW#.WW#
 35 #WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..........WWWW..#.WW#.WW#.WW#
 36 #WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..######..WWWW..#.WW#.WW#.WW#
 37 #WW.#WW.###.#..#PWWWPWWWPWWWPW..PWW..#....#..WWWP..#.###.WW#.WW#
 38 #WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..#....#..WWWW..#.WW#.WW#.WW#
 39 #WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..#....#..WWWW..#.WW#.WW#.WW#
 40 #WW.#WW.#.###..#WWWWWWWWWWWWWWH.WWW..##.###..WWWW..###.#.WW#.WW#
 41 #WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..........WWWW..#.WW#.WW#.WW#
 42 #WW.#WW.#WW.#..#WWWWWWWWWWWWWW..WWW..........WWWW..#.WW#.WW#.WW#
 43 #WW.#WW.###.#..#WWWWWWWWWWWWWW..WWWWWWWWWWWWWWWWW..#.###.WW#.WW#
 44 #WW.#WW..WW.####WWWWWWWWWWWWWW......WWWWWWWWWWWWW..#.WW#.WW..WW#
 45 #WW.#WW.#WW.=....................C...................WW#.WW#.WW#
 46 ###.#############################.##########################.###
 47 #............#WWW#WWW#WWW#..####..####..#.W#WW.WW#WWW##.#.#....#
 48 #.###..###.r.#WWW#.oW#WWW#..####..####..#rW#WW.WW#WWW##........#
 49 #.###..###.###WPW#WPW#WPW#..............#WW#WW.WW#WWW##.#.#....#
 50 #.###......###WWW#WWW#WWW#..............#WW#WW.WW#WWW##.########
 51 #............#WWWWWWWWWWW#..P..P..P..P..#WW#WW.WW#WWW##.###..#.#
 52 #.o...##......WPWWWPWWWPW=..............=.....L.............N..#
 53 #....C##.....#WWWWWWWWWWW=..............#WW#.W.WW#WWW##.###....#
 54 #............#WWWWWWWWWWW#...##....##...#WW#rW.WW#WWW##.###....#
 55 #.##....###..#WWW#WWW#WWW#...##....##...#WW#WW.WW#WWW##.########
 56 #.##....###..#WPW#WPW#WPW#..P..P..P..P..#WW#WW.WW#WW.##.#.#....#
 57 #.##.###.....#WWW#WWW#.rW#..............#WW#WW.WW#WWo##......r.#
 58 #....###.....#WWW#WWW#WWW#..............#WWWWWWWWWWWW####.#....#
 59 ###########################............#########################
 60 ###########################.....S......#########################
 61 ###########################..P......P..#########################
 62 ###########################............#########################
 63 ################################################################
```
#### The Ossuary — 62×62 (`src/maps/ossuary.js`)
```
    0         1         2         3         4         5         6 
    01234567890123456789012345678901234567890123456789012345678901
  0 ##############################################################
  1 ##############################################################
  2 ##DD#D#DD#D#DR##########DDDDDD##DDDDDD########################
  3 ##DD#D#DD#D#DD##DDDDDD##DD#oDD##DDD#DD##DDD#DD#DD#DD#DD#DD#DD#
  4 ##DD#D#DD#D#DD##DDDRDD##DD#DDD##DDD#DD##DoD#DD#DD#DD#DD#DD#DD#
  5 ##DD#D#DD#D#DD##DDDDDD##DD#DDD##DDD#DD##DDD#DD#DD#DD#DD#DD#DD#
  6 ##DDDDDDDDDDDD##DRDDDD##DDDDDD..DDDDDD..DDDDDDDDDDDDDDDDDDDDD#
  7 ##DDDNDDDDDDDDDXDDDDDD##DD#DDD##DDD#DD##DDDDDDDDDDDDDDDDDDDDD#
  8 ##DD#D#DD#D#DD##DDDDDD##DD#DDD##DDD#DD##DDD#DD#DD#DD#DD#DD#DD#
  9 ##DD#D#DD#D#DD##DDDGDD##DD#DDD##DDr#DD##DDD#DD#DD#DR#DD#DD#DD#
 10 ##DD#D#DD#D#DD##DDDDDD##DD#DDD##DDD#DD##DDD#DD#DD#DD#DD#DD#Dr#
 11 ##Dr#D#DD#D#DD##DDDDDD###D###############################D####
 12 #####D######D############D###############################D####
 13 #.................#####................#DDDDoDDDDDDDDDDDDDDDD#
 14 ###.####.##.##.#.##.........##.##.#######D#################D##
 15 #.....#####.######..###.###..####...r..##D#################D##
 16 #.....#####.#####...###.###...########.##D#################D##
 17 #.o...#####.#####.#..##.##..#.###......##D#################D##
 18 #.....#####.#####.##..#.#..##.###.#######D####DDDDDD#######D##
 19 #.....#####.#####.###.DDD.###.###.r....##D####DDDDDD#######D##
 20 ######.#.##.#.###.....DDD.....########.##DDDDDDDDDDD#######D##
 21 #..............##.####DDD.###.###......##D####DDDDCD#######D##
 22 ###.##.#.##.#####.###.Y.#..##o###.#######D####DDDDDD#######D##
 23 ###.#####......##.##.r#.##..#.###......##D####DDDDDD#######D##
 24 ###.#####......##....##.###...########.##D########D##DDDDDDD##
 25 ###.#####......###..###.###..####......##D########D###########
 26 ###.#####....r.####.........#####.#######D########D##DDDDDD###
 27 ###.#####......#######.##########......##D########D##DDDrDD###
 28 ###.##.#.#######.#####=#######.#######.##D########D##DDDDDD###
 29 #......................................#DDDDDDDDDDDDDDDDDDD###
 30 ###D############....####.#####.###################D###########
 31 ##DDDDDDDDrDDD##.#####.....#....##########RDDDD#DDDDD#DD######
 32 ##D##########D##.#####.....#....##########DD#DDDDD#DDDDD######
 33 ##D##########D##.#####...o.#.r..##########D###D########D######
 34 ##D##########D##.....#.....#....##########DD#DD#DDDDD#DD######
 35 ##D##########D######.#.....#....##########DD#oDDDD#DDDDD######
 36 ##D##########D######.######################D####D#####D#######
 37 ##DDDDDDDDDDDD#................H.......###DD#DDDDD#DDDDD######
 38 ##D##########D##.#########################DDDDD#DDDrY#DD######
 39 ##D##########D##......................####D######D#####D######
 40 ##D##########D#######################.####DD#DDDDD#DDDDD######
 41 ##D##########R##......................####DDDDD#DDDDD#RD######
 42 ##D##########D##.##########################D#####D#####D######
 43 ##DDDoDDDDDDDD##......................####DDDDDDDDDDDDDD######
 44 #######=#############################.#########=##############
 45 #...........##...##..................................#########
 46 ###.###.###.##.#.##.####.######.######.#####.###.#...#########
 47 ###.###.###....#....######DDDDDDDDDDD#############...#########
 48 ###.###.##################DDDDDDDDoDD#########################
 49 ###.###.##################DDDDCDDDDDD#########################
 50 ###.###.##################DDDDDDDDDDD#########################
 51 ###.###.##################DDDDDDDDDDD#########################
 52 ###.###.##.###.###############################################
 53 #......................#######################################
 54 #######.############.#########################################
 55 ####.......#####.....#########################################
 56 ####.......#####.....#########################################
 57 ####.................#########################################
 58 ####...V...#####..o..#########################################
 59 ####.......#####.....#########################################
 60 ####.......###################################################
 61 ##############################################################
```
#### The Source — 60×60, five laps of gallery + back-spur + lap wall, altar chamber at the centre (`src/maps/source.js`)
```
    0         1         2         3         4         5         
    012345678901234567890123456789012345678901234567890123456789
  0 ############################################################
  1 #..........................................................#
  2 #.V........................................................#
  3 #..........................................................#
  4 #...######..##o#####..####..####..####o.####..##..##..##...#
  5 #...####################################################...#
  6 #...##DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##...#
  7 #....=DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##...#
  8 #...##DDD####################DDDDDDDDDDDDDDDDDDDDDDDDD##...#
  9 ######DD#DDDDDDDDDDDDDDDDDDDD####DD###oD####DD###DDDDD##...#
 10 #....#DD#D##################D######################DDD##...#
 11 #..#.#DD#D#DDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD##DDD##...#
 12 #..#.#DD#D#DDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD##DDD#....#
 13 #..#.#DD#D#DDDDDDDDDDDDDDDDDD#D###############DDD##DDD#....#
 14 #..#.#DD#D#DDD####DD###DD###D#DDDDDDDDDDDDDDDD#DD#DDDD##...#
 15 #..#.#DD#D#DDD################=##############D#DD#DDDD##...#
 16 #..#.#DD#D#DDD##DDDDDDDDDDDDDDDDDDDDDDDDDDDD#D#DD##DDD##...#
 17 #..#.#DD#D#DDDD#DDDDDDDDDDDDDDDDDDDDDDDDDDDD#D#DD##DDD##...#
 18 #..#.#DD#D#DDDD#DDD############DDDDDDDDDDDDD#D#DD##DDD#....#
 19 #..#.#DD#D#DDD##DD#DDDDDDDDDDDD##D##D##D#DDD#D#DD##DDD##...#
 20 #..#.#DH#D#DDDD#DD#D##########D#########DDDD#D#DD#oDDD##...#
 21 #..#.#DD#D#rDDD#DD#D#DDDDDDDD#DDDDDDDDD##DDD#D#DD#DDDD##...#
 22 #..#.#DD#D#DDDD#DD#D#DDDDDDDD#DDDDDDDDD#DDDD#D#DD##DDD##...#
 23 #..#.#DD#D#DDD##DD#D#DDDDDDDD#BDDDDDDDD#DRDD#D#DD##DDD##...#
 24 #..#.#DD#D#DDD##DD#D#DDDDDRDD#DDDoDDDDD#DD#D#D#DD##DDD#....#
 25 #..#.#DD#D#D#DD#DH#D#DD#D###########DDD#DDDD#D#DD##DDD#....#
 26 #..#.#DD#D#DDoD#DD#D#DD#D#DDDDDDDD#DDDD#DYDD#D#DD#DDDD##...#
 27 #..#.#DD#D##DDD#DD#D#DD#D#DDDDDDDD#DDDD#DDDD#D#DD#DDDD##...#
 28 #..#.#DD#D#DDDD#DD#D#DD#D#DDDDDDDD##DDD#DDDD#D#DD##DDD##...#
 29 #..#.#DD#D#D#DD#DD#D#DD#D#DDDDDDDD##DDD#DDDD#D#DD##DDD##...#
 30 #..#.#DD#D#DDLD#DD#D#DD#DDDDDDADDD#DDDD#DD#D#D#DD##DDD#o...#
 31 #..#.#DD#D#DDDD#DD#D#DD#D#DDDDDDDD#DDDD#DDDD#D#DD##DDD##...#
 32 #..#.#DD#D##DDD#DD#D#DD#D#DDDDDoDD##DDD#DDDD#D#DD#DDDD##...#
 33 #..#.#DD#D#DDDD#DD#D#DD#D#DDDDDDDD#DDDD#DDDD#D#DD#DDDD##...#
 34 #..#.#DD#D#DRDD#DD#D#DD#D##########DDDD#DDDD#D#DD##DDD##...#
 35 #..#.#DD#D#DDDD#DD#D#DDD##DDD##DD#DDDDD#DD#D#D#DD##DDD##...#
 36 #..#.#DD#D#D#D##DD#D#DDDDDDDDDDDDDDDDDD#DDDD#D#DD##DDD#....#
 37 #..#.#DD#D#DDD##DD#D#DDDDDDDDDDDDDDDDDD#DDDD#D#DD##DDD#....#
 38 #..#.#DD#D#DDD##DD#D#DDDDDDDDDDDDDDDDDD##DDD#D#DD#DDDD##...#
 39 #..#.#DD#D#DDD##DD#D####################DDDD#D#DD#DDDD##...#
 40 #..#.#DD#D#DDDD#DD#DDDDDDDDDD##D#DD#D#D##DDD#D#DD##DDD##...#
 41 #..#.#DD#D#DDDD#DDD#########D#DDDDDDDDDDDDDD#D#DD##DDD##...#
 42 #..#.#DD#D#DDD##DDDDDDDDDDDDD#DDDDDDDDDDDDDD#D#DD##DDD#....#
 43 #..#.#DD#D#DDDD#DDDDDDDDDDDDD#DDDDDDDDDDDDDD#D#DD##DDD##...#
 44 #..#.#DD#D#DDDD###############D##############D#DD#DDDD##...#
 45 #..#.#DD#D#DDD##D###D####D##D#DDDDDDDDDDDDDDDD#DD#DDDD##...#
 46 #..#.#DD#D#DDDDDDDDDDDDDDDDDDD################DDD##DDD##...#
 47 #..#.#DD#D#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##DDD##...#
 48 #..#.#DD#D#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD##DDD#....#
 49 #..#.#DD#D####################=####################DDD#....#
 50 #..#.#DD#DDDDDDDDDDDDDDDDDDDDDD###DD####rD####DD###DDD##...#
 51 #..#.#DDD#####################DDDDDDDDDDDDDDDDDDDDDDDD##...#
 52 #..#.#DDDDDDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDD##...#
 53 #..#.#DDDDDDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDD##...#
 54 #..#.#######################.###########################...#
 55 #..#.........................#####..####..####r.####.###...#
 56 #...#########################..............................#
 57 #..........................................................#
 58 #..........................................................#
 59 ############################################################
```
Source bands (`maps.lapOf(cx, cz, m)`, reading `m.bands` from `META.bands = {band: 5, maxLap: 5}`):
`ring = min(x, z, w−1−x, h−1−z)`, `lap = min(5, floor((ring − 1) / 5))` — so at 60×60 each lap owns five rings: a 3-wide
gallery (`5L+1…5L+3`), a 1-wide back spur (`5L+4`) fenced off from it, and the lap wall (`5L+5`). Every lap is therefore
walked **twice**: the gallery is cut once, so you go the whole way round to a crossing that drops you into the spur, which
runs back to the next lap's door. `D` cells burn ×(1.1 + 0.12·lap) and light ×(0.95 − 0.08·lap). Lap 0 is plain floor, the
chamber (rings 26–29) is lap 5. Fog density ×(1 + 0.10·lap), ambient ×(1 − 0.13·lap) (min 0.25), eased at 2/s; a flavour
line per lap. The three fissures each join lap L to lap L+1 only, so `player.lap` stays monotonic and every lap line,
dormancy wake and lap-3/4 reinforcement still fires.

### 3.3 Scale, cost and expedition length (measured — `__game.mapsApi.validateAll()`, `scratchpad/validate-maps.mjs`)

Walkable = every non-`#`, non-`P` cell (`.` `D` `W` `S` `V` `X` `A` `=` and every marker cell). Each zone file declares
`META.targets = {size, walkable, wallShare, route}`; `validateMap` fails outside a ±5 % walkable band and ±5 pts of the
wall share, so these are contracts, not observations. "Route" is the greedy nearest-first full clear (entry → every item
/ `N` / `C` → entry) with the tool gate **open** and every shortcut **shut**; the open column is the same walk with all
three doors lifted, and check 8 requires it to be ≤ 70 % of the shut one.

| zone | grid | cells | walkable (target) | walls (target) | pillars | water | route shut → open | eccentricity |
|---|---|---|---|---|---|---|---|---|
| undercroft | 62×62 | 3844 | 2401 (2380) | 35.2 % (35.7) | 89 | — | **952 → 640** (67 %) | 144 |
| cistern | 64×64 | 4096 | 2832 (2760) | 29.6 % (31.2) | 52 | 1588 (lake body 765) | **1002 → 678** (68 %) | 220 |
| ossuary | 62×62 | 3844 | 1407 (1420) | 63.4 % (62.7) | 0 | — | **1082 → 638** (59 %) | 334 |
| source | 60×60 | 3600 | 2288 (2300) | 36.4 % (36.1) | 0 | — | **1926 → 1086** (56 %) | 955 |

Eccentricity = the deepest BFS cell from the entry with gates open and shortcuts shut; it is why `CFG.lowOil` is 35 and
not 20 (20 oil buys 40 s ≈ 104 cells of lit walking, which no longer gets you back).

**Expedition length and oil** (`scratchpad/oil-model.mjs` — the route walked cell by cell, deep and water cells costed
exactly, at the light-tech/tier a player realistically holds in that zone). "Walk" is movement alone at walk 2.6, before
the 30 s and 45 s contract vigils, an escort at follower pace, or a death. `greedy` = lit the whole way, `careful` = lit
20 % of it; budget = start oil + 25 per flask on the route.

| zone | walk shut → open | deep / water cells on the route | start oil | flasks | greedy need vs budget | careful need |
|---|---|---|---|---|---|---|
| undercroft (tech 0, tier 1) | 6.1 → 4.1 min | 274 / 0 | 45 | 10 | 209 vs 295 | 42 |
| cistern (tech I, tier 2) | 7.9 → 5.4 min | 45 / 280 | 50 | 7 | 217 vs 225 | 43 |
| ossuary (tech II, tier 3) | 6.9 → 4.1 min | 400 / 0 | 55 | 10 | 256 vs 305 | 51 |
| source (tech III, tier 4) | 12.3 → 7.0 min | 1343 / 0 | 60 | 8 | **321 vs 260 — does not finish** | 64 |

That last row is deliberate: the Source is the one zone a lit-the-whole-way full clear cannot afford, and its fissures
are the difference (open, the same clear needs 188 of 260). The balance autopilot's measured walking pace with pathing
and turning is 0.8–1.7 cells/s rather than 2.6, so a real first descent runs 2–3× those minutes.

**Payout per full clear** (unchanged building costs, so ~1.6× the old payout across a ~3× longer route):
Undercroft 10o+8r+3R = 49 pts · Cistern 7o+8r+4R = 51 · Ossuary 10o+11r+7R = 78 · Source 8o+3r+3R = 32 (unbankable).

**Render and CPU cost** (`scratchpad/rendercost2.mjs`, `bfsbench.mjs`, headless swiftshader at 426×240). Draw calls do
not grow with the map: one `InstancedMesh` per block kind (`blocks:floor|deep|water|wall|pillar|ceil`), 4–6 per zone
inside a 15–113-call frame (the spread is items, creature groups, the hub group and the water sheet, not the grid).
Instances = `cells + nonWall + pillars`, 12 triangles each.

| zone | block instances | scene triangles | `loadZone` | `bfsField` per repath |
|---|---|---|---|---|
| undercroft | 6423 | 79.4k | 30.5 ms | 0.44 ms / 38 KB |
| cistern | 7032 | 92.0k | 29.5 ms | 0.27 ms / 41 KB |
| ossuary | 5251 | 65.8k | 25.6 ms | 0.18 ms / 38 KB |
| source | 5888 | 70.8k | 14.0 ms | 0.26 ms / 36 KB |

The build happens once, inside the descend fade. `bfsField` allocates `10 × w·h` bytes per repath: 7 creatures ×
3.3 repaths/s ≈ 0.9 MB/s of garbage and ~1 % of a core. Lights do not scale with the map (lamp + ≤ 4 lanterns + the
parked hub set). The explored bitset grows to 450–512 B per zone (one bit per cell, ~684 base64 chars at 64×64; the whole save stays under 4 KB), and
the minimap's `px = max(2, min(5, …))` drops to **3 px/cell** at 60–64 wide, which is why every marker has to read at
3 px (§8).

### 3.4 Regions

`REGIONS` in each zone file is the authoring partition: inclusive `x`/`z` extents that never overlap and cover every
non-solid cell exactly once (`validateMap` check 7 — an unassigned cell warns, an overlap is an error), `deep: true`
where the pocket must really shrink the lamp (≥ 60 % `D`, ≥ 1 non-solid entrance, every interior cell satisfying
`deepNeighbourhood`), and a `loot` split that must equal the `o` / `r` / `R` chars inside the extent. Creature and
hunter counts are frozen (3 / 4 / 4 / 5 records; `enemies-test.mjs` asserts them), so pressure comes from siting each
one on a must-visit region — never from adding more.

**The Undercroft** — 62×62, bands of rows, each wall row carrying that band's doors: z1–11 north crypts (all deep) ·
z12 wall, two doors only, (27,12) and (50,12) · z13–20 Chapter House / Lantern Well / East Cloister · z21 wall ·
z22–35 the three bays, walls at x15 / x39 (only (15,28) is still a door: the x39 arcade has fallen, which is what makes
the Rood Door worth 152 cells) · z36 wall · z37–51 wing, Great Hall, Collapsed Nave · z52 wall · z53–60 the south.
One long axis runs the whole map: row z44 from the wing vestibule (x10) through the hall and the length of the nave to
x60 — the processional aisle and the zone's only long sightline.

| region | id | x | z | deep | loot (o / r / R) |
|---|---|---|---|---|---|
| The North-West Crypt | `u_nwcrypt` | 0–15 | 0–12 | deep | 1 / 0 / 1 |
| The Middle Crypt | `u_midcrypt` | 16–35 | 0–12 | deep | 1 / 1 / 0 |
| The North-East Crypt | `u_necrypt` | 36–61 | 0–12 | deep | 1 / 1 / 0 |
| The Chapter House | `u_chapter` | 0–17 | 13–21 | deep | 0 / 1 / 1 |
| The Lantern Well | `u_well` | 18–37 | 13–21 | — | 1 / 0 / 0 |
| The East Cloister | `u_cloister` | 38–61 | 13–21 | — | 1 / 0 / 0 |
| The West Bay | `u_bayw` | 0–15 | 22–35 | — | 0 / 1 / 0 |
| The Central Bay | `u_bayc` | 16–39 | 22–35 | — | 1 / 0 / 0 |
| The East Bay | `u_baye` | 40–61 | 22–35 | — | 0 / 1 / 0 |
| The West Wing | `u_wing` | 0–14 | 36–61 | — | 1 / 1 / 0 |
| The Great Hall | `u_hall` | 15–37 | 36–52 | — | 2 / 1 / 0 |
| The Stair Head | `u_stair` | 15–37 | 53–61 | — | — |
| The Collapsed Nave | `u_nave` | 38–61 | 36–61 | — | 1 / 1 / 1 |

**The Cistern** — 64×64. z1–11 north (inlet gallery · flooded vault · drain sump) · z12 wall · z13–45 the lake flanked
by the west and east channels · z45 the south quay (the artery) · z46 wall · z47–58 south rooms · z59–62 the tram apron.
The lake is **one** water body of 765 cells (the Drowner's); every other flood — the vault, the channels, the Sunken
Nave, each filter bed — is sealed off from it. A dry causeway ring runs off the quay's east end with three deliberate
wades in it (x49–50 at z26–27, x20 at z25–26, x29–34 at z33): wade (slow, heard 7 u, the Drowner's water) or walk the
long dry way round. The west half hangs off one door, the causeway head (12,20); the east half off (51,45).

| region | id | x | z | deep | loot (o / r / R) |
|---|---|---|---|---|---|
| The Sluice Head | `c_head` | 1–16 | 1–11 | — | 1 / 1 / 0 |
| The Flooded Vault | `c_vault` | 17–46 | 1–11 | — | 0 / 0 / 2 |
| The Drain Sump | `c_sump` | 47–62 | 1–11 | deep | 1 / 0 / 1 |
| The West Channels | `c_west` | 1–12 | 12–45 | — | 1 / 1 / 0 |
| The Drowned Hall | `c_lake` | 13–50 | 12–45 | — | 0 / 0 / 1 |
| The East Channels | `c_east` | 51–62 | 12–45 | — | 1 / 1 / 0 |
| The Pump Room | `c_pump` | 1–13 | 46–58 | — | 1 / 1 / 0 |
| The Sunken Nave | `c_nave` | 14–25 | 46–58 | — | 1 / 1 / 0 |
| The Tram Landing | `c_land` | 26–40 | 46–62 | — | — |
| The Filter Beds | `c_beds` | 41–53 | 46–58 | — | 1 / 2 / 0 |
| Ines' Cell Block | `c_cells` | 54–62 | 46–58 | — | 0 / 1 / 0 |

**The Ossuary** — 62×62, 63 % wall. Every corridor is one cell wide; junctions come every few steps and each band hangs
off a single choke. z1–12 the four north galleries, all deep (lamp ×0.6 on top of the zone's ×0.85 and burn ×1.3 — the
north is the expensive half) · z13 the north corridor, cut in two by a bone fall at x18–22 · z14–28 West Ossuary,
Charnel Wheel, the Spine's seven-leg switchback (x33–38), East Bone-Pit · z29 the middle corridor (Lime Pits' door
(3,30), the Winding Stair (19,30), the Wheel Rim's bars) · z30–44 Lime Pits, Nave of Bones, Deep Stacks, the Bone Stair ·
z45 the south artery, dog-legged twice, carrying both other barred doors · z46–61 South Vault and Cage Vestibule.

| region | id | x | z | deep | loot (o / r / R) |
|---|---|---|---|---|---|
| The West Gallery | `o_wgal` | 1–14 | 1–12 | deep | 0 / 1 / 1 |
| The Reliquary | `o_relic` | 15–22 | 1–12 | deep | 0 / 0 / 2 |
| The Central Gallery | `o_cgal` | 23–38 | 1–12 | — | 1 / 1 / 0 |
| The East Gallery | `o_egal` | 39–61 | 1–12 | deep | 1 / 1 / 1 |
| The West Ossuary | `o_west` | 1–14 | 13–29 | — | 1 / 1 / 0 |
| The Charnel Wheel | `o_wheel` | 15–31 | 13–27 | — | 1 / 1 / 0 |
| The Spine | `o_spine` | 32–39 | 13–44 | — | 0 / 2 / 0 |
| The East Bone-Pit | `o_pit` | 40–61 | 13–29 | deep | 1 / 1 / 0 |
| The Nave of Bones | `o_nave` | 15–31 | 28–44 | — | 1 / 1 / 0 |
| The Lime Pits | `o_lime` | 1–14 | 30–44 | deep | 1 / 1 / 1 |
| The Deep Stacks | `o_stacks` | 40–61 | 30–44 | deep | 1 / 1 / 2 |
| The Cage Vestibule | `o_cage` | 1–22 | 45–61 | — | 1 / 0 / 0 |
| The South Vault | `o_vault` | 23–61 | 45–61 | — | 1 / 0 / 0 |

**The Source** — 60×60, band 5. The spiral is annular, so each lap is tiled by its legs (gallery, the leg that widens
into that lap's hall, the south leg and the spur), and every ring from 6 outward is deep. Each lap is walked twice: the
gallery ring is cut once, so from the lap door you go the whole way round to a crossing that drops you into the 1-wide
spur behind it, and the spur runs back to the next lap's door.

| lap | break (gallery cut) | crossing → spur | spur end → door → next entry | hall / guard |
|---|---|---|---|---|
| 0 | (1–3, 9) W | (3,10) → (4,10) | (28,55) → (28,54) → (28,53) | reliquary niches; `V` (2,2) |
| 1 | (29, 51–53) S | (30,51) → (30,50) | (28,9) → (28,10) → (28,11) | The Ash Pits; fast `H` (7,20) |
| 2 | (29, 11–13) N | (30,13) → (30,14) | (30,45) → (30,44) → (30,43) | The Choir of Stones, `L` (13,30) |
| 3 | (29, 41–43) S | (28,41) → (28,40) | (30,19) → (30,20) → (30,21) | The Weeping Wall, `Y` (41,26) |
| 4 | (29, 21–24) N | the Antechamber itself | (24,30) → (25,30) → (26,30) | The Antechamber, `B` (30,23) |
| 5 | — | — | — | the 8×8 chamber, `A` (30,30) |

| region | id | x | z | deep | loot (o / r / R) |
|---|---|---|---|---|---|
| The Outer Walk | `s_outer` | 1–58 | 1–5 | — | 2 / 0 / 0 |
| The Eastern Arm | `s_eastwalk` | 54–58 | 6–53 | — | 1 / 0 / 0 |
| The Processional Arm | `s_procession` | 1–58 | 54–58 | — | 0 / 1 / 0 |
| The First Crack | `s_firstcrack` | 1–5 | 6–53 | — | — |
| The First Turn | `s_turn` | 6–53 | 6–10 | deep | 1 / 0 / 0 |
| The First Turn, East Leg | `s_turneast` | 49–53 | 11–48 | deep | 1 / 0 / 0 |
| The Ash Pits | `s_ashpits` | 6–53 | 49–53 | deep | 0 / 1 / 0 |
| The First Turn, West Leg | `s_turnwest` | 6–10 | 11–48 | deep | — |
| The Second Turn | `s_second` | 11–48 | 11–15 | deep | — |
| The Broken Lecterns | `s_lecterns` | 44–48 | 16–43 | deep | — |
| The Second Turn, South Leg | `s_secondsouth` | 11–48 | 44–48 | deep | — |
| The Choir of Stones | `s_choir` | 11–15 | 16–43 | deep | 1 / 1 / 1 |
| The Third Turn | `s_third` | 16–43 | 16–20 | deep | — |
| The Weeping Wall | `s_weep` | 39–43 | 21–38 | deep | 0 / 0 / 1 |
| The Third Turn, South Leg | `s_thirdsouth` | 16–43 | 39–43 | deep | — |
| The Third Turn, West Leg | `s_thirdwest` | 16–20 | 21–38 | deep | — |
| The Antechamber | `s_ante` | 21–38 | 21–25 | deep | 1 / 0 / 1 |
| The Fourth Turn, East Leg | `s_anteeast` | 34–38 | 26–33 | deep | — |
| The Fourth Turn, South Leg | `s_antesouth` | 21–38 | 34–38 | deep | — |
| The Chamber Approach | `s_approach` | 21–25 | 26–33 | deep | — |
| The Chamber of the Source | `s_chamber` | 26–33 | 26–33 | deep | 1 / 0 / 0 |

### 3.5 What moved when the maps grew

The zones were 40×40 (walkable 1019 / 1152 / 604 / 1024, routes 220–816 cells). Nothing was dropped and no feature
changed meaning: every entry, spawn, gate, captive and contract spot moved outward into the new grid, and the two or
three new regions per zone (the Chapter House and the Collapsed Nave, the Drain Sump and the Sunken Nave, the Charnel
Wheel and the Deep Stacks, the Choir and the Antechamber) were built around them. The suites follow each cell through
`ANCHORS` (§3.7) rather than literals. Row-major order decides which `H` / creature record gets which options from
`META`, so these cells are load-bearing, not decorative.

| feature | 40×40 | now | why there |
|---|---|---|---|
| Undercroft entry `S` | (20,37) | **(31,58)** | the Stair Head, 5×5 clear, three free 4-neighbours |
| Undercroft base `H` | (16,8) | **(28,44)** | the Great Hall on the processional aisle: the first hunter, on the main route |
| Warden `G` facing E | (26,2) | **(42,10)** | the NE crypt: spot 0 (48,10) at 6 u and the cloister door (50,12) at 8.25 u both on its axis — inside the 9 u cone, outside the 8 u territory |
| Brute `B` leash 14 | (4,17) | **(5,42)** | the wing spine: leash covers Wick at 10 and the wing doors at 7/11, never the hall centre (23) or `S` (42) |
| Wick `N` | (5,22) | **(4,47)** | the West Wing, past the Brute |
| Deacon Maud `N` | (2,3) | **(4,5)** | the NW crypt, behind the Pry Bar |
| Pry Bar `X` | (4,5) | **(15,6)** | the wall column x15, NW ↔ Middle crypt |
| Undercroft spots 0 / 1 | (30,2) (16,29) | **(48,10)** (31,47) | the NE crypt (83–89 BFS out) · the Great Hall |
| Cistern entry `S` | (20,37) | **(32,60)** | the tram apron |
| Cistern base `H` ×2 | (14,7) (29,10) | **(20,17)** (30,40) | causeway floor inside the lake band, 24 and 57 BFS from the entry |
| Drowner `w` | (20,13) | **(31,29)** | the lake body (765 cells): its flood-fill reaches the quay shore and nothing else |
| Lampwight `L` | (30,27) | **(46,52)** | the filter-bed walkway, 14 cells of open LOS toward Ines: the run to her is an oil tax, not a death trap |
| Ines `N` | (38,22) | **(60,52)** | the east cell block |
| Sluice Key `X` | (20,5) | **(16,6)** | the wall column x16, head ↔ vault |
| Cistern spots 0 / 1 | (20,19) (3,26) | **(33,45)** (5,53) | the quay is 2 cells deep there, so the 30 s vigil cell is never water-adjacent while one step north is · the pump room |
| Ossuary entry `V` | (7,36) | **(7,58)** | the elevator cage |
| Ossuary fast `H` | (19,5) | **(31,37)** | the Spine's z37 crossing: the map's single choke, on every route |
| Warden `G` facing N (`gateOk`) | (12,6) | **(19,9)** | the Reliquary, behind the Censer on purpose; both `R` and the gate cell inside its cone |
| False lights `Y` ×2 | (18,19) (26,25) | **(22,22)** (52,38) | a Charnel Wheel spoke and a Deep Stacks cell, each 1 cell from a relic and seen from a corridor at 5.1 u |
| Oren `N` / Censer `X` | (7,5) (14,4) | **(5,7)** (15,7) | the West Gallery cross aisle · the wall column x15 |
| Ossuary spots 0 / 1 | (29,18) (19,28) | **(50,21)** (30,49) | the east bone-pit ledge · the deep south vault (45 s lamp-off vigil) |
| Source entry `V` | (1,1) | **(2,2)** | a corner cell cannot hold the 5×5 entry pocket |
| Source altar `A` | (19,20) | **(30,30)** | `lapOf` = 5 |
| Source fast `H` ×2 | (11,26) (37,30) | **(7,20)** (17,25) | lap 1 (awake from the first step) and lap 3 (dormant until lap 2) |
| Source `L` / `Y` / `B` | (7,20) (29,14) (13,20) | **(13,30)** (41,26) (30,23) | lap 2 Choir · lap 3 Weeping Wall · lap 4 Antechamber (dormancy wakes each at lap − 1) |

### 3.6 Shortcuts (`=`) — the mechanic

A shortcut is **one barred door** (one cell, or two adjacent where the doorway is 2 wide) set in a wall line between two
regions that are far apart on foot. It is not a tunnel: it adds no corridor, it removes a detour. Distinct from `X`: a
gate needs a tool and opens from either side, a shortcut needs nothing and opens from **one** side only — the far side.

- **Data.** `parseMap` sets the cell to `T.SHORTCUT` (`T_NAME[9] = 'shortcut'`) and pushes
  `map.shortcuts = [{cx, cz, idx, x, z, open, mesh, id}]` row-major; `world.loadZone` binds each parsed cell to the
  `SHORTCUTS` entry that lists it and copies its `id`. `isSolid` and `los` both treat a barred cell as solid, so the
  player, hunters, the follower's BFS and every sense check agree with no new rule in `hunter.js`.
- **Interaction.** `main.interactTarget()` runs endgame → npc → hub → items → gates → **shortcuts** → stairs;
  `shortcutTarget()` mirrors `gateTarget()` (within `CFG.interactR + 0.5`, facing it). From the `openFrom` side the hint
  is `[E] Lift the bars` → `world.openShortcut()` rewrites every cell of the group to `T.FLOOR`, swaps the barred mesh
  for the raised one, records the id in `save.shortcuts[zone]` and emits **`shortcutOpened {zoneId, id, name, cx, cz,
  idx}`** (toast "The bars fall. <name> is open for good."). From the barred side the hint is `Barred from the other
  side` and `E` only emits `uiError`. `hunter.js` drops its cached paths on `shortcutOpened` exactly as on `gateOpened`;
  `audio.js` answers with the gate cue pitched down plus a chain rattle and a stone boom.
- **Rendering.** `models.shortcutBarred()` is a portcullis — stone lintel, heavy iron bars, a chain drum, and a brass
  lift-bar with a small emissive amber glint **on the far side only**, so the side that can open it is readable through
  the bars; `models.shortcutOpen()` is the same frame with the bars raised into the lintel (walkable underneath), so you
  can always see which doors you have opened. On the minimap a barred one is dim green `#2f8f6a`, an opened one mint
  `#5ff0b0` plus a 1-px mint diamond (readable at 3 px/cell); an opened one draws whether or not its cell is in the
  explored bitset, because you know it is there. Gates stay brown `#8a6a3a`.
- **Persistence.** `save.shortcuts = {zoneId: [id]}` — stable string ids, never cell indices, because the grids changed
  size (§11). Opening one is permanent: it survives death, re-entry and reload.
- **Design rules** (`validateMap` check 5): ids unique and matching the map's `=` cells exactly; cells contiguous and
  ≤ 2; the `openFrom` flank is the *farther* one from the entry; the removed detour ≥ 40 cells (Source fissures ≥ 100);
  the barred flank reachable and with LOS to the `=` cell, so the promise is seen. Check 3 walks the zone with every
  shortcut shut: nothing in a zone may *need* one. No shortcut bypasses a guard — `u_rood` still walks you under the
  Warden's cloister door, `c_screen` runs through the Lampwight's tanks, `o_stackdoor` opens inside a false light's
  honeycomb, and each fissure drops you onto the ring its creature patrols. In the Source a fissure joins lap **L to
  L+1 only**, so `player.lap` stays monotonic. The per-door table is in §3.2.

### 3.7 Map files, `ANCHORS` and validation

One file per zone under `src/maps/`, each exporting exactly `ID SIZE ROWS REGIONS SHORTCUTS ANCHORS META`:

```js
export const ID    = 'undercroft';            // matches the filename and ZONE_ORDER
export const SIZE  = 62;                      // square; ROWS.length === SIZE, every row .length === SIZE
export const ROWS  = [ /* SIZE strings */ ];
export const REGIONS   = [{ id, name, x: [x0, x1], z: [z0, z1], deep?, loot?: {oil, relic, rich} }, …];
export const SHORTCUTS = [{ id, name, cells: [[cx, cz], …], openFrom: 'N'|'E'|'S'|'W', from, to, saves }];
export const ANCHORS   = { entry, gate, spot0, spot1, hunter, warden, brute, …, shortcuts: {id: [cx, cz], …} };
export const META  = { name, entry, exit, burnMul, lampMul, deepStyle, hunters, creatures, npc, npcs, gate, spots,
                       loot, points, size: SIZE, regions, shortcuts, anchors, bands?, targets };
```

`maps.js` keeps everything that is not map-shaped (`PALETTES`, `TUNING`, `requires`/`lockReason`, `intro`/`threat`,
`ambience`, `hunterSpeeds`) and composes `ZONES[id] = {id, ...TUNING[id], ...FILE.META, rows: FILE.ROWS}`; the hub rows
stay in `maps.js`. `lapOf(cx, cz, m)` reads `m.w/m.h` and `m.bands` (`{band: 5, maxLap: 5}` from `META.bands`), falling
back to the legacy 40×40 `{band: 3, maxLap: 5}` when called with no map.

**`ANCHORS` is the test contract.** `integration.mjs` and `enemies-test.mjs` teleport to `ANCHORS` lookups rather than
to literal grid cells (`enemy-art-test.mjs` needs none: it finds each creature record at runtime and walks to a cell near
it); whoever moves a feature moves its anchor in the same commit. Beyond the feature cells each zone declares the probe
cells the suites need — the Warden's axis (`wardenDark` `wardenNear`
`wardenProbe` `wardenAside` `wardenOut`), clear sense lanes (`laneW` … `laneE`, `nsN`/`nsS`), the Drowner's shore
(`drownerShore` `drownerSafe` `drownerWater` `otherWater`), each shortcut's `=` cell plus its `…Near` (barred) and
`…Far` (`openFrom`) flanks, and the Source's `lap1`–`lap5`, `doors` and `crossings`.

**`validateMap(rows, meta, {size, strict})` must pass with zero errors for all four zones plus both hubs**
(`__game.mapsApi.validateAll()`, or `node scratchpad/validate-maps.mjs`, which also runs three negative controls).
Checks: dimensions and border; legend; one `S`/`V` matching `entry`; altar only in the Source; connectivity with gates
open; every item / `N` / `C` reachable; hunters and creatures reachable with gates closed unless `gateOk`; `G` in a deep
neighbourhood; `w` in a water body; `Y` with LOS ≤ 6 u from a floor cell; meta agreement for hunters / creatures / npcs /
gate / spots / loot — plus the ten contract checks: **1** size and the walkable / wall-share bands of §3.3; **2** markers
on legal cells (`o r` on `.`/`D`, `R` in a `deep` region or standing in the flooded vault, `w` on `W`, `=` and `X` in a
wall line); **3** first-run topology with gates open and every shortcut shut; **4** full connectivity with everything
open, no orphan cell; **5** the shortcut rules of §3.6; **6** deep pockets ≥ 60 % `D` with a real entrance; **7** the
region partition and its declared loot; **8** the route floor of §3.3 and the ≤ 70 % open ratio; **9** the entry pocket
(≥ 3 free 4-neighbours, no spawn within 10 BFS cells); **10** the Source's laps (`lapOf(altar) === 5`, each creature's
lap, every fissure joining L to L+1). `validateAll()` returns each zone's `stats` including `shortcuts` and
`route: {closed, open}`, so every number in §3.3 and §3.2 can be read straight off the page.

### 3.8 Hub — "The Last Lantern" (25×13 at x=+60; digits = building anchors)
```
#########################
#..1..#....F....#..2....#
#.....#.........#.......#
#.....#.........#.......#
#..3..#.........#....4..#
#.#####.........#######.#
#.......................#
#.......................#
#.....#.........#.......#
#.6...#........5#.....7.#
######....S....##########
######.........##########
#########################
```
Rooms: the nave x7–15 z1–9 with the great flame `F` against the north wall; the stairs room x6–14 z10–11 (spawn `S`
(10,10), yaw 0 faces the flame); W and E alcoves x1–5 / x17–23 z6–9 opening onto the nave through 2-wide mouths at x6 / x16
(z6–7); NW and NE corner rooms z1–4 reached from the alcoves through 1-wide doors at (1,5) and (23,5). No hunter; the
lamp is forced off and does not burn.

Buildings stand against a wall or in an alcove back, never in a mouth; the digit is the E-interaction point
(`HUB_CFG.interactR` 1.8) and `BUILDINGS[id].rot/dx/dz` place the model so that anchor +1 x (where the rescued NPC
stands, `npc.js hubSpot`) and the cell(s) west of the anchor stay free: Workshop `1` (3,1) rot π, Wick at (4,1) ·
Cartographer's Table `3` (3,4) rot 0 dz −0.25 against the south wall, Ines at (4,4) · Oil Press `2` (19,1) rot π (both
barrels west of the vat), Oren at (20,1) · Shrine `4` (21,4) rot 0, Maud at (22,4) · Departure Board `5` (15,9) rot π/2
on the nave's east wall by the stairs · Tram dock `6` (2,9), rails along the alcove's south wall into the west wall ·
Elevator `7` (22,9) dx +0.5 dz −0.5, the cage in the alcove's SE corner (cells 22–23 × 8–9).

Collision (`map.blockMask`, bits `config.HUB_BLOCK`): building footprints (ghost or built), the brazier, the residents'
stand cells (hub.js `rebuildColliders`, on hubEnter / build / rescue and every 1 s) and the camp props (world.js) are solid
for the player: a box counts when its top is above 0.25, its bottom below 1.2 and it is wider than 0.25 in either axis
(posts, legs and candles never block on their own); the world AABB shrunk 0.2 per side picks the cells. Rugs, bedrolls,
candle clusters and anything hung from the ceiling stay walkable. `__game.hub.colliders()` lists the blocked cells.

Camp (`world.js HUB_PROPS_V2`, all merged by `models.mergeBoxes` into one Lambert + one unlit vertex-colour mesh = 2 draw
calls): hearth rug (9–13 × 2–4), two benches turned to the fire (8,3) (14,3), firewood (9,1) and a stew pot over embers
(13,1) beside the brazier, bedrolls along the nave walls (7,1–2) (15,1–2) and in the NE room (17,3–4), crates and barrels
in the corners ((7,8–9), (6,11), (14,11), (5,1), (5,8–9), (17,1), (17–18,9)), two bookshelves on the NW room's east wall
(5,3–4), herbs and bottles hung over the press (19,1), a candle cluster before the shrine (21,3) and one by the hearth (12,2).

Light and warmth (hub only; zones keep their palettes): ambient / fog tint per tier `config.HUB_WARMTH` (ambient 0x1c140d →
0x4c3620, fog 0x050302 → 0x0f0a06 at density 0.10). Hanging lanterns (`HUB_LANTERNS_V2`, one merged group per tier, shown
from that tier): tier 1 by the board (13,8) · tier 2 the alcove mouths (6,7) (16,6) · tier 3 the room doors (1,5) (23,5) and
the hearth (8,5) (14,5) · tier 4 the alcoves and rooms (3,7) (20,7) (3,2) (20,2) (9,8). The five keyed ones (tier ≤ 3 minus the
hearth pair) carry a PointLight 0xffb060 int 1.2 dist 5.5; with the flame, four sconces and the stairs landing lamp that is
11 hub point lights, all parked by `setHubLights(false)` while a zone runs. Alcove sconces light up by tier as before: tier 2
the W/E alcoves (1,7) (23,7), tier 3 the N rooms (1,2) (23,2), tier 4 everything (`SCONCE_INT` 1.6 / 3.2 / 5.0; below its tier a sconce keeps a 0.45 banked-ember glow so no room is pitch black). The flame
light flickers on three sines (0.80–1.0 × tier intensity); `config.EMBERS` embers (18 / 36 / 60 / 90 alive by tier, one
additive Points cloud) rise from the coals and go out below the ceiling. Residents bob and turn to the player within 5 u
(npc.js) and idle in place (head turn, breathing, arm sway; hub.js). The v1 17×9 hub rows are kept in `maps.HUB_ROWS` for
tests only.

QA: `scratchpad/hub-audit.mjs` (playwright) dumps every hub cell and collider with everything built / everyone rescued and
checks that every doorway is free with free cells on both sides, that every free cell is reachable from the spawn with
colliders solid, and that every building, resident and the stairs have a free approach cell where the E hint fires.

## 4. Light economy

| Value | Number |
|---|---|
| Oil capacity | 100 |
| Start oil on descend | 50 / 60 / 70 / 80 by flame tier, +15 per Oil Press reservoir level (max 2) |
| Burn while lit | 0.5 oil/s × deep × zone × light-tech; oil 0 → lamp forced off |
| Lamp | PointLight 0xffb265, intensity 6, distance 11 × deep × zone × light-tech, decay 2; flicker 0.93+0.07·sin(13t), ×3 amplitude below 15 oil |
| Flash (Q) | cost 15 (12 at tech II, 10 at III), cooldown 1.5 s; lamp ×8 for 0.15 s, white screen 0.1 s |
| Plant lantern (R) | cost 20 (16 at tech III), cooldown 1 s, max 4 alive (oldest removed); pool radius 2.5 u; lasts until you leave the zone |
| Flask (T) | +25 oil, consumes one carried flask |
| Light-tech I / II / III | lamp distance ×1.15 / 1.3 / 1.45 (11 u → 12.7 / 14.3 / 16), burn ×0.9 / 0.8 / 0.7; cost 6 / 10 / 14 relics (+2 rich at III) |
| Warning | HUD bar red below 20 oil, "Lamp guttering"; vignette darkens |

Tension: a flask is +1 flame point and +25 banked oil, or +25 oil now — never both.

## 5. Hunters and creatures (`hunter.js`)

Mesh: `models.hunter()` hunched 11-box figure, 0x050505 with emissive eyes (wander 0.4 / investigate 0.8 / chase 1.5 /
staggered 0.05). Profiles: `base` 1.5 / 2.6 / **3.6** u/s (wander / investigate / chase), catch 0.8 u, loses target after 3 s;
`fast` 1.8 / 3.0 / **4.4**, catch 0.9, loses after 4 s, scale y 1.15, amber eyes. A chase is faster than a walk (2.6) and
slower than a sprint (4.2): you cannot walk away from one, and a sprint buys distance at the price of being heard 9 u away.
Five more creatures run off the same driver — §5.1–5.9 below.

| Value | Number |
|---|---|
| Senses (every 0.2 s) | lamp lit or flashing: 12 u **with LOS** · sprinting: 9 u · wading: 7 u · walking dark: 2.5 u · still dark: 1.0 u |
| LOS | grid DDA at y 1, blocked by `#`/`P`/closed `X` |
| Stagger | 3 s frozen, then WANDER to a cell ≥ 12 away, dazed (ignores stimuli) 4 s |
| Flash hit | hunter within 7 u, `dot(forward, toHunter) > 0.4`, LOS |
| Pools | BFS treats cells within 2.5 u of a lantern as blocked; a target in a pool cannot be caught; 6 s unreachable → gives up |
| Follower | counts as "walking dark" (2.5 u) and as lit (12 u + LOS) within 3 u of the lit player; hunters chase the nearest stimulated target; a caught follower makes the hunter INVESTIGATE its own spot for 2 s |

FSM: WANDER (random reachable cell ≤ 10 away, idle 1–3 s) · INVESTIGATE(pos) (wait 2 s, eyes sweep) · CHASE (repath every
0.3 s; no stimulus for loseT → INVESTIGATE(lastKnown)) · STAGGERED (flash only). BFS 4-neighbour, only on repath ticks.
Escape recipe: plant a lantern, step in, douse the lamp, wait.

### 5.0 The roster

Five creatures on top of the `base`/`fast` hunters, each adding one rule to the light economy and each escapable by
light play (douse + pools + flash), never only by outrunning. Numbers are relative to `config.js`: walk **2.6** / sprint
**4.2** (water ×0.55/×0.6 → 1.43/2.52), lamp reach 11 (hunter `lampR` 12), base chase **3.6**, fast chase **4.4**, pool
2.5 u, flash 7 u cone, hunter tick 0.2 s, repath 0.3 s. (This was `DESIGN-enemies.md`; its §n is §5.n here, which is
what the `DESIGN.md §5.n` comments in `src/` point at.)

| Creature | One rule it adds | Kills? | Flash | Pools | Douse |
|---|---|---|---|---|---|
| Lampwight | a lit lamp is a beacon at 2× range; it taxes oil, not life | no (snuffs) | stagger 3 s | repel | loses you at once |
| Warden | some ground is watched: a sweeping cone you must time | yes | flinch 0.5 s only | block | 6 s, then it walks home |
| Drowner | water is its territory: lit or loud near water = surge | yes | forced dive | block (on water) | no stimulus → sinks in 4 s |
| False light | not every glow is a lantern: check before you trust it | yes | reveals (lit) / aborts (dark) | block | no effect (proximity) |
| Brute | pools are not walls: it smashes lanterns, ignores the flash | yes | nothing (snort) | slow ×0.5, not block | loses you at 2.0 u walk-dark |

### 5.1 Lampwight (`L`) — the light-drinker
**States** DRIFT (wander, 1.2 u/s) → DRAWN (lit lamp seen: 2.2 u/s toward the target's live position) → SNUFF (touch:
0.8 s hold, the snuff happens at t = 0.3 s) → SATED (5 s, 1.6 u/s to a wander cell ≥ 8 cells from the player, ignores
every stimulus) → DRIFT. DRAWN → DRIFT after `loseT` 2 s without stimulus (paths to `lastKnown`, not live). STAGGERED
(flash): 3 s frozen, then DRIFT dazed 4 s. Timers: `snuffT` 0.8, `satedT` 5, `loseT` 2, `staggerT` 3, `dazeT` 4.
**Senses** lit handlamp or flash within **24 u** (2 × `lampR`) **with LOS**. Nothing else: sprint, walk, still and
wading are ignored (a doused player is invisible to it even at 0.3 u). Ignores the follower (it wants the flame).
**Movement** floor / deep / water (×0.85), never a pool cell (BFS `isBlocked`), never a solid. Speeds above; below walk in
every state so a lit player can always back off, but it never stops coming while the lamp is lit.
**Touch (0.9 u, target not in a pool)** no death. Emits `lampSnuffed {hunterId, oil: 12, lockout: 2, x, z}`; main.js:
`player.lampOn = false`, `player.oil = max(0, oil − 12)`, `player.lampLock = 2` (F refuses with toast "The wick is cold"
while `lampLock > 0`). A dark player after the snuff no longer registers, so it drifts off; it returns only if you relight.
**Model** (12 boxes, 2.2 tall, thinner than the hunter; shroud 0x6a7488, hood 0x0a0a10): hood 0.42×0.35×0.4 · head ·
torso 0.36×1.0×0.28 · two arms 0.1×1.2 past the knees · two hands · two-box tattered skirt (0.5 / 0.34 wide) · two eyes 0.06
emissive pale blue 0x9ad0ff (DRIFT 0.3 / DRAWN 1.2 / SNUFF 2.5 / SATED 0.6 / STAGGERED 0.05) · one chest "ember" 0.14
emissive 0xffb265, k 0 → 1.5 at SNUFF decaying through SATED (the stolen light). No legs: it drifts, bobbing 0.05 u at 0.7 Hz.
**Sound** presence: a breathy whistle (sine 660 ±12 Hz vibrato 4 Hz, tremolo 0.2 Hz, gain ≤ 0.10, fades by 20 u) —
"breath through a keyhole" · trigger (DRAWN): a rising sigh, noise bandpass 300 → 1200 over 0.8 s g 0.2 · snuff: hard
exhale burst 0.15 s + lamp crackle cut + low thud 55 Hz · none on death (it never kills).
**HUD** DRAWN within 12 u + LOS: `Something is drawn to your light` · `lampLock > 0`: `Snuffed — [F] relight in 2 s` ·
while it is DRAWN and you are in a pool: the usual `Safe — it will not enter the light`.

### 5.2 Warden (`G`) — the sentinel
**States** SENTRY (at the post, 0 u/s; yaw sweeps `postYaw ± 75°` as a triangle wave at 0.35 rad/s, ~7.5 s per full
sweep; cyan spotlight on) → ALERT (0.6 s: light flares, clang; still) → CHASE (**3.4 u/s**, repath 0.3, BFS restricted to
its territory) → RETURN (2.0 u/s back to the post; senses stay on) → SENTRY when within 0.3 u of the post. FLINCH (flash
in any state: 0.5 s frozen, light off, then the previous state resumes with its path kept). No STAGGERED, ever.
CHASE → RETURN when the target leaves the territory (Euclid > 8 u from the post) **or** `noStimT` ≥ 6 s **or** unreachable
6 s. RETURN → CHASE on any stimulus inside the territory. In CHASE with no stimulus it paths to `lastKnown` (never live).
**Senses** SENTRY: lit lamp or flash with LOS inside the cone — `d ≤ 9`, angle to facing ≤ 35° (70° cone) — or a lit lamp
within 1.2 u at any angle (it feels the heat). ALERT/CHASE/RETURN: lit lamp with LOS anywhere inside the territory, or
sprinting ≤ 4 u. Never walk / still / wading; ignores the follower.
**Movement** `canEnter(cell) = !solid && !pool && dist(cell centre, post) ≤ 8`. It never leaves its territory: the BFS
field is built with that predicate, and the direct "close the last stretch" move is refused outside it.
**Catch** 0.8 u in CHASE or RETURN, target not in a pool → `hunterCatch` (death). SENTRY / ALERT / FLINCH never catch: a
dark player may brush past it.
**Tell** a THREE.SpotLight on the head (0x7fd0ff, intensity SENTRY 1.2 / ALERT 3.0 / CHASE 2.0 / RETURN 1.2 / FLINCH 0,
distance 9, angle 35°, penumbra 0.5, decay 2) aimed along its facing — the only cold light in the ruin, and the sweep is
readable from across the pocket. Its visor is the same colour.
**Model** (16 boxes, 2.5 tall): plinth 0.9×0.15×0.9 0x2a2620 · two legs 0.22×0.8 · pelvis · torso 0.7×0.9×0.4 bronze
0x3a2f22 · two pauldrons 0.3 verdigris 0x2f5a4a · helm 0.4×0.45×0.4 · visor slit 0.3×0.06 emissive 0x7fd0ff (k = spotlight
÷ 2) · two arms · halberd shaft 0.06×2.2 + blade 0.3×0.5×0.05 · tabard 0x1c1614. The group turns with the sweep; CHASE legs ±0.3 rad.
**Sound** presence: stone grind (saw 28 Hz → lowpass 90, gain 0.06 × |sweep speed|, ≤ 14 u), a click at each reversal ·
alert (`wardenAlert`): clang (square 220 → 110, 0.3 s ring + noise hit) then a horn (sine 55 → 82 over 1 s, g 0.3) · chase:
footfall bursts (lowpass 200, g 0.25) every 0.45 s, panned · death: the death hit + a bell 165 Hz decaying 1.5 s.
**HUD** ALERT: override 2 s `The Warden has seen your light` · CHASE: `Leave its ground or go dark` · RETURN: nothing.

### 5.3 Drowner (`w`, a water cell) — the thing under the surface
**States** SUBMERGED (body hidden, ripple ring only; drifts through its water body at 0.6 u/s picking a random water
cell ≤ 8 cells away every 3–6 s; homes on a lit lamp seen from the water within 12 u at 1.0 u/s) → SURFACING (0.4 s:
splash, body rises to y −0.15) → SURGE (**5.0 u/s**, water cells only, repath 0.3, live target) → LURK (4 s without
stimulus: 1.5 u/s along the shore nearest the last known position, growling) → SINK (0.6 s) → SUBMERGED dazed 4 s.
Flash while surfaced: → SINK immediately (its stagger), then SUBMERGED dazed 4 s; flash while submerged: nothing.
**Trigger (SUBMERGED → SURFACING)** the player is in water or 4-adjacent to a water cell of *its* body, within **6 u**,
and (lamp lit with LOS **or** sprinting **or** wading = `inWater && moving`). **Keeps it surfaced (SURGE/LURK stimulus)**:
lit lamp with LOS ≤ 10 u, wading ≤ 8 u, sprinting ≤ 8 u. Never walk-dark / still. Ignores the follower.
**Movement** `canEnter(cell) = cell === WATER && sameBody && !pool`. `waterMul` 1.0 (the 0.85 penalty is for land
creatures). Its water body = the flood-fill of water cells from its spawn; it can never cross a causeway.
**Catch** 0.9 u in SURGE only, target not in a pool (a lantern planted in the shallows makes a safe island).
The catch reaches ~0.4 u onto a shore cell, so the water's edge is dangerous, one full cell back is not.
**Model** (12 boxes, 0.55 tall × 1.8 long, 0x06080a with 0x102028 ridges): head 0.4×0.25×0.5 · jaw · two eyes 0.05 emissive
0x40ff9a (SUBMERGED 0 / SURFACING 0.8 / SURGE 1.6 / LURK 1.0 / SINK 0.3) · body 0.6×0.3×0.8 · three back ridges · tail root +
tip · two forelimbs. Plus a ripple ring (the lantern `ringGeo`, emissive 0x2a5a6a k 0.4, y 0.02) pulsing scale 0.6 → 1.4
over 1.2 s while SUBMERGED (visible from 12 u, so the drift is readable) and a 0.5 s wake in SURGE.
**Sound** presence: a plop every 2–5 s (sine 180 → 90, 0.12 s) panned at its position, ≤ 12 u · trigger
(`drownerSurge`): splash (noise bandpass 400 → 2000, 0.6 s, g 0.5) + gurgling roar (saw 48 → 36, 1.2 s, g 0.35) · surge:
the Cistern `waterWash` noise ×4, modulated by its speed · sink: reverse splash 0.4 s · death: splash + the death hit with
a master lowpass sweep 2000 → 80 Hz over 1.2 s ("pulled under").
**HUD** SUBMERGED ≤ 8 u while you are on or next to water: `The water is moving` · SURGE: `It's in the water — get out`.

### 5.4 False light (`Y`) — the lantern that isn't
**States** LIT (still; PointLight 0xffc070 int 2.0 dist 6 decay 2 — the planted lantern's exact light — plus the flicker
`0.93 + 0.07·sin(13t)`; eyes k 0) → DARK (0.35 s: light off, eyes 0xff2020 open, chitter — the tell) → POUNCE (1.5 s
lunge at **6.0 u/s** toward the player's position *at pounce start*, no retargeting; stops at walls and pool edges) →
RETREAT (3.0 u/s to a new rest spot, body 0x050505, eyes 0.2; cannot catch, cannot be caught) → RELIGHT (10 s at the
spot, dark and still) → LIT. REVEALED (flash while LIT: eyes flare k 3.0 for 1.0 s, shriek) → RETREAT without a pounce.
Flash in DARK or POUNCE: STAGGERED 1.0 s → RETREAT (the panic button). Flash in RETREAT/RELIGHT: nothing.
**Trigger** LIT → DARK when the player is within **3.0 u** with LOS, lamp on or off (it is a proximity trap; dousing does
not help, distance and the flash do). Ignores the follower and every other stimulus.
**Rest spot** a reachable non-solid, non-pool cell with BFS distance 8–30 from the player and **no LOS** from the player;
score = +2 per solid 4-neighbour (nooks), +3 if an item lies within 2 cells (the lure), +1 if a floor `.` corridor cell
has LOS to it within 6 u (it must be seen to work); best of the top 6 at random. Spawn `Y` cells are hand-picked to
satisfy the same rule.
**Movement** floor / deep / water (×0.85), never a pool cell. POUNCE also refuses pool cells, so a pool is an absolute wall.
**Catch** 0.9 u in POUNCE only, target not in a pool → death.
**Never a pool** it is never pushed into `ctx.lanterns`; `recomputePools`, `player.inPool`, the minimap lantern dots and
hunter BFS read `ctx.lanterns` only. Base hunters ignore it entirely (they sense the player, not lights).
**Model** (14 boxes, 1.6 tall — the planted lantern's silhouette, wood 0x3a2a1a / iron 0x2a2420): two stilt legs 0.06×1.05
(LIT: together, reading as the pole; DARK+: splayed ±0.25) · two feet · tray 0.3×0.04 · four ribs 0.04×0.36 · glass
0.22×0.26×0.22 emissive 0xffc070 k 1.0 (its lure; k 0 from DARK on) · cap 0.3×0.06 · two eyes 0.05 emissive 0xff2020 · a jaw 0.16×0.05 that drops in DARK.
**Sound** presence: **none** (no hunter voice — silence is the lure), only a faint fake crackle at 0.6 × the lamp's
(≤ 6 u) · trigger (`falseLightPounce`): a snap-click as the light dies, then a chitter (square 900 Hz, 8 stutters in
0.35 s) · pounce: scrabbling bursts at 12 Hz · reveal (`falseLightReveal`): shriek saw 1200 → 2400 0.4 s g 0.35 · death:
the death hit + the chitter.
**HUD** LIT: nothing (the deception) · REVEALED: override 2 s `It was never a lantern` · POUNCE: `It has seen you`.

### 5.5 Brute (`B`) — the wall that walks
**States** the base FSM: WANDER (1.3 u/s, leashed) · INVESTIGATE (2.0) · CHASE (**2.6 = walk**) — no STAGGERED. Turn
rate limited to 2.0 rad/s (it corners badly). `loseT` 4 s; unreachable 6 s → WANDER. Flash: no state change; emits
`flashResisted {hunterId, profile}` (a snort, the HUD line). `catchR` 1.0.
**Senses** the base set with shorter ranges: lamp **8 u** + LOS, sprint 6, wading 5, walk-dark **1.5** (§5.10), still 1.0; senses the
follower like the base hunter.
**Movement** floor / deep / water (×0.85), pools **allowed** at `poolMul` 0.5 (BFS with `ignorePools`). Wander leash: a
candidate cell must be ≤ 14 BFS cells from its spawn (`home`); if none, path to the nearest leash cell. CHASE is not
leashed; the leash pulls it back over the following wanders.
**Smash** any planted lantern within 1.0 u → `world.removeLantern` + `lanternSmashed {hunterId, x, z}` (contract
progress is unaffected: `plant` completes on planting) + an ember burst: 24 additive points 0xffa040 rising 0.8 u/s for
0.8 s + a crunch. `recomputePools` runs through the existing `lanternRemoved` listener.
**Catch** 1.0 u in INVESTIGATE / CHASE, **even in a pool** (the only creature that can) — the pool buys the seconds it
takes to wade in and smash the lantern, and the douse-and-walk that follows is the real answer.
**Footsteps** one `creatureStep {hunterId, profile: 'brute', x, z, d}` per stride (every 0.6 s WANDER, 0.45 CHASE);
audio thuds it (≤ 26 u); main.js shakes the camera when d ≤ 8: pitch `0.012·(1 − d/8)·sin(28t)` decaying over 0.25 s,
eye height dip 0.01 u.
**Model** (20 boxes, 2.6 tall, 1.3 wide; hide 0x141210, plates 0x2a2420, knuckles 0x3a3028): two legs 0.32×1.0 · two feet
0.4×0.12×0.5 · pelvis · torso 1.0×1.0×0.6 · two boulder shoulders 0.45 · two arms 0.28×1.3 · two fists 0.34 · head 0.36 sunk
between the shoulders · two eyes 0.06 emissive 0xff6a20 (WANDER 0.3 / INVESTIGATE 0.6 / CHASE 1.0) · three back plates · two tusks 0x5a5040. Sways ±0.06 u per stride.
**Sound** presence: breathing drone (sine 38 Hz + noise lowpass 120, g ≤ 0.12, ≤ 20 u) plus stride thuds (burst lowpass
90 Hz, g 0.35 × falloff, ≤ 26 u) · trigger (CHASE): roar saw 70 → 45 over 1.0 s + noise swell g 0.45 · attack (`lanternSmashed`):
crunch (noise 0.15 s lowpass 600 + square 180 Hz ×3 cracks) · flash: snort (noise bandpass 250, 0.2 s) · death: the death hit ×1.3 + a 40 Hz crunch.
**HUD** CHASE: `It has seen you` · `lanternSmashed`: override 2 s `It smashed your lantern` · `flashResisted`: `It does not
flinch` · in a pool while it is within 6 u: `Not safe — it will wade in` (replaces the Safe line).

### 5.6 Tuning table (`config.js` `HUNTER_PROFILES` + the `CREATURE` block)
| Profile | Speeds (u/s) | catchR | lamp / sprint / walk / still / water | loseT | flash | pool | water | kills |
|---|---|---|---|---|---|---|---|---|---|
| base | 1.5 / 2.6 / 3.6 | 0.8 | 12 LOS / 9 / 2.5 / 1.0 / 7 | 3 | stagger 3 + daze 4 | block | ×0.85 | yes |
| fast | 1.8 / 3.0 / 4.4 | 0.9 | same | 4 | same | block | ×0.85 | yes |
| lampwight | DRIFT 1.2 · DRAWN 2.2 · SATED 1.6 | 0.9 | **24 LOS** / – / – / – / – | 2 | stagger 3 + daze 4 | block | ×0.85 | **no**: −12 oil, lock 2 s, sated 5 s |
| warden | SENTRY 0 · CHASE 3.4 · RETURN 2.0 | 0.8 | cone 9 u 70° LOS (+1.2 u any) / 4 / – / – / – | 6 | flinch 0.5 | block | ×0.85 | yes (CHASE/RETURN) |
| drowner | SUBMERGED 0.6 (home 1.0) · SURGE 5.0 · LURK 1.5 | 0.9 | 10 LOS / 8 / – / – / 8 (trigger: all at 6, near water) | 4 | forced sink + daze 4 | block | ×1.0, water only | yes (SURGE) |
| falseLight | POUNCE 6.0 (1.5 s) · RETREAT 3.0 | 0.9 | proximity 3.0 LOS | – | reveal / abort | block | ×0.85 | yes (POUNCE) |
| brute | 1.3 / 2.0 / 2.6 | 1.0 | 8 LOS / 6 / **1.5** / 1.0 / 5 | 4 | none (snort) | ×0.5, catches inside | ×0.85 | yes |

Warden: sweep ±75° at 0.35 rad/s, ALERT 0.6 s, territory 8 u, give-up 6 s · Drowner: surface 0.4 s, sink 0.6 s, lurk 4 s ·
False light: dark 0.35 s, relight 10 s, rest BFS 8–30 · Brute: leash 14 BFS, stride 0.6/0.45 s, shake ≤ 8 u · Lampwight:
snuff 12 oil, lockout 2 s, hold 0.8 s (all in a new `CREATURE` block) · endgame `maxHunters` 4 counts `base`/`fast` only.

### 5.7 Placement
New legend characters (`maps.js` `LEGEND`, `LEGEND_CHARS`): `L` lampwight · `G` warden (guard; `W` is already water) ·
`Y` false light · `B` brute · `w` drowner (**a water cell** with a spawn; the parser stores `T.WATER`). `L G Y B` keep the
cell floor/deep by `deepIfPocket()` exactly like `H`. The parser collects `map.creatures = [{kind, cx, cz, idx, x, z}]`
row-major; `ZONES[id].creatures` lists per-kind options in the same order (Warden `facing` N/E/S/W → yaw 0 / −π/2 / π /
π/2, `sweep`, `reach`, `territory`; Brute `leash`). `validateMap` counts each kind against the meta, requires a `G` cell
to satisfy `deepNeighbourhood`, a `w` to be water, a `Y` to have LOS to some `.`/`D` cell within 6 u, and every creature
cell reachable from the spawn (gates closed for `L Y B w`; the Ossuary `G` is *behind* the gate on purpose: `gateOk`).

**Undercroft** (62×62) — base `H` **(28,44)** in the Great Hall. `G` **(42,10)** facing **E** in the NE crypt,
`B` **(5,42)** in the West Wing, leash 14.
```
z9  #DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#DD#D#DDDDDDDDDDDDDDDDDDDDDDDDr#      G at (42,10): the post at the west end of the NE crypt, facing
z10 #Do#DD#DD#DD#DD#DD#DD#DD#DD#DD#rD#D#DDDDDDGDDDDDCDDDDDDDDDDDD#      east. Reach 9 along row z10 covers spot 0 C (48,10) at 6 u and
z11 #DDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDD#DDDDDDDDDDDDDDDDDDDDDDDDD#      the East Cloister door (50,12) at 8.25 u — inside the 9 u cone,
z12 ###########################D######################D###########      outside the 8 u territory, so one step back still ends a chase.

z41 #.....#..#....#..P.P.P.P.P.P.P.P.P.P.##########.##############      B at (5,42): the second chamber of the wing spine. Leash 14 BFS
z42 #....B........#......................#.......................#      covers Wick (4,47) at 10, the wing stair at 7 and the hall door
z43 #######..#....#......................#...P...P.....P.....P...#      at 11 — but the hall centre is 23 and the S stairs 42, so it can
z44 #.....#..#..................H................................#      never reach either.
```
Justification, unchanged in spirit and moved outward with the map: the player meets the Brute on the way to Wick (4,47) —
the first "pools are not walls" lesson, mid-run — and the Warden on the way to the NE crypt rich relic and spot 0, now
the far corner at 83–89 BFS cells. Both are far from the Stair Head, so a first run still starts against the single base
hunter in the Great Hall. `u_wingstair` opens *past* the Brute and `u_rood` still walks you under the Warden cloister
door: no shortcut bypasses a guard.

**Cistern** (64×64) — base `H` **(20,17)** on the north cross causeway and **(30,40)** on the quay stub. `w` **(31,29)**
in the lake, `L` **(46,52)** on the filter-bed walkway.
```
z29 #WW.#WW.#WW.#..#W#WW.WWWWWWWWWWwWWWWWWWWWWWWW#WWW..#.WW#.WW#.WW#      The Drowned Hall (z13-45, x13-50) is ONE water body of 765 cells:
z43 #WW.#WW.###.#..#WWWWWWWWWWWWWW..WWWWWWWWWWWWWWWWW..#.###.WW#.WW#      the flood-fill from w (31,29) reaches the quay shore and nothing
z44 #WW.#WW..WW.####WWWWWWWWWWWWWW......WWWWWWWWWWWWW..#.WW#.WW..WW#      else — not the vault, not the channels, not the Sunken Nave, not
z45 #WW.#WW.#WW.=....................C...................WW#.WW#.WW#      the beds. Spot 0 C (33,45) sits on a quay 2 cells deep here, so
                                                                    the 30 s vigil cell is never water-adjacent while one
                                                                    step north (33,44) is → trigger. Today's reading, kept.

z52 #.o...##......WPWWWPWWWPW=..............=.....L.............N..#      L at (46,52): the single walkway lane through the Filter Beds,
                                                                    with 14 cells of open LOS east to Ines (60,52) — the
                                                                    walk to the cartographer is still an oil tax.
```
Justification: the lake keeps the Drowner, and both base hunters stand on causeway floor inside it (24 and 57 BFS from
the entry); the Lampwight moved to the beds, so the route to Ines is a tax rather than a death trap, and the flooded
vault behind the Sluice Key gate (16,6) crosses nothing new. `c_screen` opens *into* the Lampwight tanks.

**Ossuary** (62×62) — fast `H` **(31,37)** on the Spine z37 crossing, the map's single choke. `G` **(19,9)** facing
**N** inside the Reliquary (`gateOk`: it is *behind* the Censer gate on purpose). `Y` **(22,22)** in a Charnel Wheel
spoke, `Y` **(52,38)** in a Deep Stacks cell.
```
z4  ##DD#D#DD#D#DD##DDDRDD##DD#DDD##DDD#DD##DoD#DD#DD#DD#DD#DD#DD#      G at (19,9): the Reliquary is x15-22 x z1-12 behind the Censer
z6  ##DDDDDDDDDDDD##DRDDDD##DDDDDD..DDDDDD..DDDDDDDDDDDDDDDDDDDDD#      gate X (15,7). Post at its south end facing north, sweep +/-75:
z7  ##DDDNDDDDDDDDDXDDDDDD##DD#DDD##DDD#DD##DDDDDDDDDDDDDDDDDDDDD#      both rich relics (19,4) at 5.0 u and (17,6) at 3.6 u / 34 deg lie
z9  ##DD#D#DD#D#DD##DDDGDD##DD#DDD##DDr#DD##DDD#DD#DD#DR#DD#DD#DD#      in the cone, and the gate cell is 4.5 u at 63 deg — swept, so a
                                                                lit player opening it is seen.

z22 ###.##.#.##.#####.###.Y.#..##o###.#######D####DDDDDD#######D##      Y at (22,22): the SW spoke alcove of the Charnel Wheel, relic
z23 ###.#####......##.##.r#.##..#.###......##D####DDDDDD#######D##      (21,23) one cell away, seen from a floor cell at 5.1 u.

z37 ##DDDDDDDDDDDD#................H.......###DD#DDDDD#DDDDD######      Y at (52,38): a honeycomb cell in the Deep Stacks, relic (51,38)
z38 ##D##########D##.#########################DDDDD#DDDrY#DD######      one cell away, also 5.1 u — and o_stackdoor opens INTO it.
```
Justification: unchanged. The Ossuary is where light-tech II makes the lamp reach 14 u, so it is the zone that teaches
"a glow you did not plant"; both false lights still rest beside loot in a pocket the corridor shows you, and the Warden
still makes the Censer reward a timing puzzle inside a deep pocket (lamp ×0.6) rather than a free grab. The fast hunter
now owns the one crossing every route must pass.

**Source** (60×60, band 5) — fast `H` **(7,20)** lap 1, awake from the first step, and **(17,25)** lap 3. `L` **(13,30)**
lap 2 in the Choir of Stones · `Y` **(41,26)** lap 3 on the Weeping Wall · `B` **(30,23)** lap 4 in the Antechamber. The
row-major creature order is therefore `B · Y · L`.
```
z23 #..#.#DD#D#DDD##DD#D#DDDDDDDD#BDDDDDDDD#DRDD#D#DD##DDD##...#      B at (30,23): the Antechamber, the 4-wide hall the lap-4 break
z26 #..#.#DD#D#DDoD#DD#D#DD#D#DDDDDDDD#DDDD#DYDD#D#DD#DDDD##...#      splits in two. Its west half holds the rich relic (26,24) and is
z30 #..#.#DD#D#DDLD#DD#D#DD#DDDDDDADDD#DDDD#DD#D#D#DD##DDD#o...#      the last room before the chamber door (25,30) — lanterns are the
                                                            only light there and it takes them.
                                                            Y at (41,26): three cells down the Weeping Wall from its
                                                            own rich relic (41,23), so you turn the corner, see a
                                                            lantern beside an R, and the grab is 2.2 u from it.
                                                            L at (13,30): the Choir of Stones, with column x13 kept
                                                            clear so it is a 28-cell beacon down the lap-2 west leg.
                                                            endgame.js dormancy still wakes each at lap - 1.
```
Justification: unchanged, now spread over ~950 cells to the altar instead of ~460. The Source stacks its lessons by
depth — the Lampwight taxes oil on lap 2 exactly where the bands raise burn, the false light guards a rich relic on lap
3, and the Brute walks the ring before the chamber. `pickSpawnCell` and `maxHunters` still count only base/fast records.

### 5.8 Architecture — how `hunter.js` grew
One list (`ctx.hunters`), one record shape, one `update`/`syncMesh`/`catch` path; behaviour comes from a **profile
table**. `spawnAll` reads `map.hunterSpawns` (H, profile from `meta.hunters`) then `map.creatures` (kind = profile,
options from `meta.creatures`); every record gets `home` (spawn cell), `opts`, and `prof = PROFILES[profile]`.
```js
// hunter.js (or creatures.js exporting the table; hunter.js stays the driver)
PROFILES[profile] = {
  model: 'hunter'|'lampwight'|'warden'|'drowner'|'falseLight'|'brute',   // models.js factory name
  initial: 'WANDER'|'SENTRY'|'SUBMERGED'|'LIT',
  speed: {STATE: u/s}, eye: {STATE: k}, catchR, loseT, scaleY, eyeColor,   // as today
  senses: { lamp, sprint, walk, still, water, cone?: {deg, reach}, near?: 1.2, proximity?: 3.0, follower: bool },
  canEnter(m, cx, cz, h) → bool,        // pools · water body · territory (BFS + the direct-move fallback use it)
  poolMul: 1 | 0.5, waterMul: 0.85 | 1.0,
  catchStates: ['CHASE', ...], catchInPool: false | true,
  onFlash(h) → 'stagger' | 'flinch' | 'sink' | 'reveal' | 'abort' | 'none',
  onCatch(h, target) → 'kill' | 'snuff',            // main.js's hunterCatch listener stays; snuff emits lampSnuffed
  onNearLantern?(h, l),                                // brute smash
  fsm: { STATE: { enter?(h), tick(h, t /*sense() result*/), move?: 'path' | 'still' | 'lunge' } },
  leash?: 14, territory?: 8, step?: {WANDER: 0.6, CHASE: 0.45},
};
```
- **Driver** `tick(h)`: decay `dazeT/busyT` → `t = sense(h)` (generic over `prof.senses`; cone / near / proximity are
  extra predicates) → `prof.fsm[h.state].tick(h, t)`; `setState` runs `fsm[s].enter`. `updateOne` keeps today's
  path-follow / direct-close / catch code but reads `canEnter`, `poolMul`, `waterMul`, `catchStates`, `catchInPool` and
  `move` (`'still'`: Warden SENTRY, False light LIT/RELIGHT · `'lunge'`: straight line, no path).
- **Base FSM as data** `BASE_FSM = {WANDER, INVESTIGATE, CHASE, STAGGERED}` from today's code, unchanged for `base`/`fast`;
  `brute` = it minus STAGGERED plus `leash`/`onNearLantern`/`step`; `lampwight` = it with DRIFT/DRAWN aliases plus
  SNUFF/SATED (~25 lines); Warden, Drowner, False light are their own 5–6-state tables (~40 lines each).
  `bfsField(m, sx, sz, blockedFn)` takes the predicate instead of `ignorePools`.
- **Flash / catch** `main.flash()` calls `hunterMod.onFlash(h)` (cone/LOS test stays in main.js). `updateOne` emits
  `hunterCatch` only for `onCatch === 'kill'`; `'snuff'` emits `lampSnuffed`. main.js adds `player.lampLock`, `ctx.shake`,
  `actions.creature(kind)`. **Endgame** dormancy already loops `ctx.hunters` by spawn lap; only the active-count and
  `pickSpawnCell` filters change to `base`/`fast`. **Audio** `presenceFor` becomes per-profile voices (growl · whistle ·
  grind · plops · breath; false light none); new one-shots hang off the new events. **Minimap / pools / follower**
  untouched: they read `ctx.lanterns` and `ctx.npc.stimulus()` only.

**Events reused** `zoneEnter`, `zoneExit`, `hubEnter`, `lantern`, `lanternRemoved` (also fired by a smash → pools),
`gateOpened`, `npcCaught`, `flash`, `waterEnter/Exit`, `hunterState {id, state, prev}` (ui's "It has seen you" keys on
CHASE/POUNCE), `hunterCatch {x, z, hunterId, target}` (death), `hunterWoken`, `hunterSpawned`.
**New events** `lampSnuffed {hunterId, oil, lockout, x, z}` · `lanternSmashed {hunterId, x, z}` · `wardenAlert
{hunterId, x, z}` · `wardenReturn {hunterId}` · `drownerSurge {hunterId, x, z}` · `drownerSink {hunterId}` ·
`falseLightPounce {hunterId, x, z}` · `falseLightReveal {hunterId, x, z}` · `flashResisted {hunterId, profile}` ·
`creatureStep {hunterId, profile, x, z, d}`. All payloads carry `hunterId` so audio pans and tests filter by profile.

### 5.9 Acceptance checklist (`scratchpad/enemies-test.mjs`, headless, zero console errors)
Harness: `gotoZone`, `teleport`, `actions.creature(kind)`, the `__ev` log, playwright keys, `waitFor(() => __game.game.time >= T)`.
- [ ] **Maps** `mapsApi.validateAll().ok`; each zone's `map.creatures` matches §5.7 cells/kinds; `(20,13)` is `T.WATER`;
      `spawnAll` gives Undercroft 3, Cistern 4, Ossuary 4, Source 5 records; Source `L Y B` start inactive (dormant).
- [ ] **Lampwight** place it 15 u from the player with LOS, lamp on → `DRAWN` within 0.4 s (base hunter at the same
      spot: no chase — 15 > 12). Lamp off → `DRIFT` within 2.2 s and it walks past at 0.5 u without reacting even while
      the player sprints. Lamp on, let it touch: `lampSnuffed` fires, `lampOn === false`, oil dropped by 12, `F` within
      2 s refused (`lampLock > 0`, toast), state `SATED`, no `death`; after 2.1 s `F` relights. Q at 5 u → `STAGGERED` 3 s.
      Player in a pool, lamp on: it never enters a pool cell over 10 s (`m.pool[idx]` of its cell stays 0).
- [ ] **Warden** Undercroft: teleport to (33.5,2.5) lamp on → within one sweep (≤ 8 s) `wardenAlert`, `ALERT` → `CHASE`;
      its cell always within 8 u of (26.5,2.5) while chasing; step to (34.5,4.5) → `RETURN` within 0.4 s, then `SENTRY`
      at the post. Lamp off at (30.5,2.5) for 10 s → never alerted. Lit in the cone, then douse and stand still 6 s → `RETURN`.
      Q during CHASE → `FLINCH` 0.5 s then `CHASE` again (never `STAGGERED`). Pool at (30,2): its path never enters
      pool cells. Catch at 0.8 u in CHASE → `DYING`. Ossuary: with the gate closed it never alerts to a lit player at (15,4).
- [ ] **Drowner** Cistern: teleport to (20.5,18.5) (adjacent) lamp on with it at (20.5,15.5) → `SURFACING` → `SURGE`,
      `drownerSurge`; every position sample is on a `T.WATER` cell of the lake; it never reaches (20.5,19.5) (spot cell
      + 1 u from the shore) — no catch in 6 s. Lamp off, still, 1 u from the shore → `LURK` then `SINK`/`SUBMERGED`
      within 4.6 s. Wade at (20.5,13.5) dark → triggers (water noise ≤ 6 u) and catches → `DYING`. Q while `SURGE`
      → `SINK` at once, dazed: lit and wading beside it for 4 s → no re-surface. Lantern on (20,17) and stand in it → no catch.
      Teleport it to the west band (2.5,8.5) via debug → the flood-fill refuses: it snaps back to its body.
- [ ] **False light** Ossuary: `LIT` record has a PointLight child (intensity ≈ 2), `ctx.lanterns.length === 0`,
      `player.inPool === false` at 1 u from it, minimap draw leaves no lantern dot at its cell (`drawMinimap` + canvas
      pixel probe), base/fast hunters' `recomputePools` leaves `m.pool` all zero. Walk to 2.9 u → `DARK` (light 0) →
      `POUNCE` → catch → `DYING` (`falseLightPounce` fired). Reload: Q at 5 u while `LIT` → `falseLightReveal`,
      `REVEALED` → `RETREAT`, no pounce, it re-lights ≥ 8 cells away, out of LOS, after 10 s. Q during `DARK` → aborts,
      no catch. Pool between it and the player → the lunge stops at the pool edge (no pool cell entered), no catch.
- [ ] **Brute** Undercroft: chase speed sample = 2.6 ± 0.1 (walk 2.6, base 3.6 in the same probe); it sees a lit lamp at
      7 u and not at 10 u (base: yes at 10). Q from 3 u → state unchanged, `flashResisted` fired, no `STAGGERED`. Plant
      a lantern, stand in it, lamp on: it enters pool cells at half speed, `lanternSmashed` fires, `ctx.lanterns` empty,
      `lanternRemoved` recomputed pools, then `hunterCatch` inside the former pool → `DYING`. Douse + walk away from 3 u
      → no catch over 8 s and `INVESTIGATE` after 4 s. `creatureStep` events at 0.45 s intervals in CHASE; camera pitch
      jitter non-zero within 8 u, zero at 20 u. Wander for 60 s: every cell ≤ 14 BFS from (4,17); never (16,29) or (19,37).
- [ ] **Cross-cutting** every creature `group.userData.boxes ≤ 30` with `.model` = profile, and listed in `models.html`;
      a follower is never targeted by Lampwight/Warden/Drowner/False light; `hunterState` fires on every transition; audio
      `stats().presence` is 0 for the False light, > 0 for a Brute at 10 u; the death cam works for every killer;
      `integration.mjs` still passes (135 checks) and the console stays clean across all four zones for 60 s of wandering.

### 5.10 Balance pass (measured, `scratchpad/balance.mjs` + `scratchpad/escape.mjs`)

**Method.** A BFS autopilot walks each zone's whole loot route (every item, nearest-first from the spawn, then the exit)
for 70 s of game time, three times: **lit** (lamp on the whole way, never reacts — the exposure baseline), **react** (a
competent player: go dark while anything is in a chase-like state within 12 u, flash a chaser inside 5 u, sprint while
one is inside 5 u), **doused** (lamp off the whole way — blind, so it walks into things). A death restarts the run at the
stairs and the walk continues; deaths are attributed by the `hunterCatch` payload. Escapes are measured separately:
each creature is provoked from an LOS-checked cell and the design's counter-play is applied the moment it triggers,
with every other hunter in the zone deactivated so nothing else can score the kill.

**Routes** (deaths per 70 s route, light-tech 0 / I / II / III by zone):

| Zone | lit, never reacts | react (douse · flash · sprint) | doused (blind) | what killed, and what the creatures did |
|---|---|---|---|---|
| Undercroft | 4 (Warden 1, hunter 3) | **1** (Warden, caught inside its ground) | 2 (hunter, walked into it) | 1 Warden alert per pass; the Brute chased twice and killed nobody once "douse and walk" worked (below) |
| Cistern | 3 (hunter) + 1 snuff | **0** (4 Lampwight approaches, 0 snuffs) | 2 (hunter) | the Lampwight is drawn 4–6 times per lit route (−12 oil per touch); the Drowner never fired — the lake is a shortcut, not on the loot path |
| Ossuary | 2 (false light) | **0** (3 pounces, all aborted by the flash) | 2 (false light — dousing does not help) | both false lights sit on loot; the Warden stayed in its gated reliquary |
| Source (from the entry) | 3 (fast hunter) | **0** | 2 (fast hunter) | the bot never got past lap 1, so the three dormant creatures never woke |
| Source (dropped at lap 4) | 5 in 42 s (fast hunters) | 5 in 30 s (fast hunters) | — | the Brute chased 4× and killed nobody; the deep Source is a gauntlet of `fast` hunters (chase 4.4 > sprint 4.2) by design, and respawning into lap 4 is worse than descending into it |

Read: a player who ignores the light rules dies 2–4 times per route; a player who uses them clears the same route with
0–1 deaths and pays in oil instead (Undercroft react run: 80 → 27 oil for 4 flashes, 3 sprints and a lot of dark walking).
Every creature earns its rule without becoming the reason a run ends.

**Escape matrix** (19/19 trials behaved as designed; "provoked" is the state it reached before the counter-play):

| Creature | counter-play | result |
|---|---|---|
| Lampwight | douse | DRAWN → DRIFT, no catch, it drifts off at 0.15 u |
| Lampwight | flash at 5 u | STAGGERED 3 s |
| Lampwight | let it touch you | −12 oil, wick cold 2 s, **no death**, then SATED |
| Warden | walk off its ground | ALERT → SENTRY at the post, no catch |
| Warden | douse and walk out of its ground | ALERT → SENTRY, no catch (ends 21 u away) |
| Warden | douse and stand still inside its ground | **caught** — CHASE 3.4 crosses its 8 u territory in 2.4 s; the 6 s give-up only saves you from outside |
| Warden | flash | FLINCH 0.5 s, never STAGGERED, no catch |
| Drowner | one full cell back from the shore | SURGE, no catch (it stops ~0.4 u onto land) |
| Drowner | flash it while surfaced | forced SINK, no catch |
| Drowner | plant a lantern at the water's edge and stand in it | SURGE, no catch — the island works |
| False light | flash it while it still glows | REVEALED → RETREAT, no pounce |
| False light | a planted pool between you and it | the lunge stops at the pool edge, no catch |
| False light | keep 3 u away | it never leaves its spot (LIT) |
| Brute | douse and walk away from 4 u | no catch, INVESTIGATE, 18 u away after 9 s |
| Brute | douse and walk away from 2 u | no catch, but it trails you at 1.6 u for as long as you walk |
| Brute | sprint away from 2 u | no catch, INVESTIGATE, 17 u away |
| Brute | stand in a planted pool | **caught** — it wades in, smashes the lantern and takes you (the one creature pools do not stop) |
| Brute | flash it | nothing but a snort (`flashResisted`) |

**The one number this pass changed.** `HUNTER_PROFILES.brute.senses.walk` **2.0 → 1.5 u**. At 2.0 the Brute's chase
speed (2.6) *is* the walk speed, so a doused player walking away from ~2 u stayed exactly on its sense edge, refreshed
its stimulus every tick and was eventually caught: the profile's own escape recipe ("loses you at walk-dark range")
could not be executed at the range where it matters. At 1.5 the douse breaks contact at once, the Brute follows the
last known position for `loseT` 4 s and then investigates — and sprinting still beats it outright. Everything else in
§5.6 measured as intended and was left alone.

**Kept deliberately (measured, not overlooked)**
- Warden `loseT` 6 s and CHASE 3.4: dousing inside its ground is *not* an escape; leaving it is, and the cone reaches
  9 u while the ground is 8 u, so the place you are first seen is already one step from safety.
- False light POUNCE 6.0 u/s for 1.5 s at 3.0 u proximity: unavoidable once it fires, but the flash aborts it and a pool
  is an absolute wall — the reactive run survived three pounces without a scratch.
- Lampwight 12 oil per touch: 1–3 touches on a careless lit route (12–36 oil) — a real tax, never a death.
- Drowner trigger 6 u: it only bites when you use the water; the Cistern's 30 s vigil spot (33,45) is one full cell from the
  shore, which is where the zone teaches it.
- The deep Source stays the hardest ground in the game (no banking, `fast` chase 4.4 > sprint 4.2, up to four hunters
  plus the three creatures); the creature layer added pressure there but no deaths in the probe.

## 6. Items, extraction, death, flame tiers

- Pickup: E within 1.6 u. `carried = {oil, relic, rich, quest}`. Items glint (emissive) in total darkness.
- Bank (E at `S`/`V`): `points += oil + 3·relic + 5·rich`; `save.oil += 25·oil`, `save.relics += relic`, `save.rich += rich`;
  a following NPC within 4 u is rescued. Fade, spawn in the hub, toast.
- Death: 1.2 s red vignette; carried loot becomes a bundle at the death spot (one per zone, survives zone switches); points
  untouched. With the Shrine blessing lit, ⌊half⌋ of each carried kind is banked on the spot (both ledgers) first.
- Flame tiers (`TIERS`; hub PointLight 0xffa040, decay 2, flicker as lamp): tier 1 ≥ 0 pts, int 2.0, dist 7, "Only embers
  remain." · tier 2 ≥ 6, 3.5 / 11, "The flame stirs. The alcoves take shape." · tier 3 ≥ 15, 5.5 / 16, "The flame grows. The
  vault is warm again." · tier 4 ≥ 30, 8.0 / 24, "The Last Lantern burns bright."

## 7. NPCs (`npc.js`) and contracts (`contracts.js`)

| id | name | held in | unlocks | line |
|---|---|---|---|---|
| `lamplighter` | Wick the Lamplighter | Undercroft (4,47), the West Wing | Workshop | "Every lamp I ever lit is out. Let's fix that." |
| `cartographer` | Ines the Cartographer | Cistern (60,52), the cell block | Cartographer's Table | "I mapped every one of these halls. Then they moved." |
| `keeper` | Oren the Oil-press Keeper | Ossuary (5,7), the West Gallery | Oil Press | "Relics burn better than they pray." |
| `deacon` | Deacon Maud | Undercroft (4,5), the NW crypt behind the Pry Bar gate | Shrine; the Source | "The Source can be fed, or freed. Both are prayers." |

CAPTIVE (E within 1.6 u: "[E] Free Wick") → FOLLOW → rescued on bank ≤ 4 u away, or CAUGHT (hunter within 0.8 u, not in a
pool) → sinks 1 s → CAPTIVE again in its cell (never lost). One follower at a time ("You cannot shepherd two"). Follow AI every
0.25 s: BFS toward the player (pools not blocked), 3.5 u/s (5.0 beyond 6 u), stops at 1.5 u, snaps next to the player beyond
14 u; wall-sliding radius 0.25. Rescued NPCs stand at their anchor +1 x in the hub (beside their building, never in a walkway — §3.8), turn toward the
player within 5 u; E opens a dialogue with the next contract (1/Enter accepts).

Contracts: each NPC posts one at a time in table order; max 2 active; progress in `save.contracts`. `fetch` = bank ≥ n of a kind
from that zone in one run (progress = carried while there); `plant` = lantern within 1.5 u of the spot; `recover` = a quest
item spawns at the spot, pick it up and bank it (lost on death like loot); `survive` = contiguous seconds within 3 u of the spot
(`lampOff` variants need the lamp doused; leaving resets). Death, an early bank or leaving the zone resets run-scoped progress
(`contractFailed`) but keeps the contract.

| id | poster | type | target | reward |
|---|---|---|---|---|
| `c_relight` | Wick | plant | Undercroft great hall, spot 1 (31,47) | Pry Bar + 4 pts |
| `c_wick` | Wick | fetch | 4 oil flasks, Undercroft | 60 oil |
| `c_sound` | Ines | survive 30 s | Cistern drowned hall, spot 0 (33,45), lamp allowed | Sluice Key |
| `c_chart` | Ines | recover "lost chart" | Cistern pump room, spot 1 (5,53) | 8 pts |
| `c_censer` | Oren | fetch | 2 rich relics, Ossuary | Censer |
| `c_ledger` | Oren | recover "ledger" | Ossuary east bone-pit, spot 0 (50,21) | 80 oil |
| `c_vigil` | Maud | survive 45 s, lamp off | Ossuary south vault, spot 1 (30,49) | 10 pts |
| `c_bones` | Maud | fetch | 3 relics, Ossuary | 6 pts |

## 8. Hub buildings and services (`hub.js`)

Ghost (wireframe 0x3a3020) appears when unlocked; E within 1.8 u of the anchor opens the build / service menu. Costs spend the
resource ledger only (`save.oil/relics/rich`); flame points are never spent.

| Building (anchor cell) | Unlock | Cost | Service |
|---|---|---|---|
| Departure Board (5 · 15,9) | always | — | zones with lock reasons + active contract targets; 1–4 selects `save.zoneSelected`; stairs, tram and elevator all descend there |
| Workshop (1 · 3,1) | Wick | 6 relics | Light-tech I–III (§4) |
| Oil Press (2 · 19,1) | Oren | 80 oil | press 1 / all relics at 30 oil each; "Deeper reservoir" 5 then 8 relics → +15 start oil each |
| Cartographer's Table (3 · 3,4) | Ines | 100 oil | Tab minimap (200×200 canvas, `px = max(2, min(5, floor(min(W/w, H/h))))` — 5 px/cell in the 25×13 hub, **3 px/cell** in the 60–64 wide zones, 8 Hz): cells seen within lamp distance (≤ 16 u, 2 u dark) with LOS, plus bordering walls; items seen, lanterns, spots ◇, exit ▲, NPCs; tool gates brown `#8a6a3a`; **shortcuts dim green `#2f8f6a` while barred and mint `#5ff0b0` + a mint diamond once opened** — an opened one is drawn whether or not its cell is in the explored bitset, because you know it is there; per-zone bitset saved base64 (a stored bitset whose length does not match the current grid is dropped, §3.3); % charted per zone |
| Shrine (4 · 21,4) | Maud | 150 oil | toggle blessing: 40 oil charged at each descent; on death ⌊half⌋ of each carried kind is banked anyway |
| Tram dock (6 · 2,9) | flame tier 2 | 120 oil | opens The Cistern (ride menu at the cart) |
| Elevator (7 · 22,9) | flame tier 3 | 250 oil + 4 relics | opens The Ossuary and the way to The Source |

## 9. Endgame (`endgame.js`)

The Source has no banking: `V` offers "[E] Ride up" (a second press within 3 s confirms when carrying loot; loot is
discarded). Hunter pressure grows with depth: the two mapped fast hunters start dormant and wake when the player is one lap
above them; laps 3 and 4 each spawn one extra fast hunter 12–34 BFS cells from the player (max 4). E at the altar (1.6 u)
enters ENDING: pointer released, sim frozen, a 3-choice screen (1/2/3, click, Esc cancels):

| Choice | Available | Ending | Theme |
|---|---|---|---|
| Feed the great flame | always | `cage` "A Brighter Cage" | the Lantern blazes (tier 4) or flares and "will need feeding again"; the rescued keep watch |
| Kindle a new flame | flame tier 4 **and** ≥ 3 rescued (else greyed with the reason) | `dawn` "The Lantern Eternal" | Maud's ritual carried by the crowd; the vault becomes a town |
| Free the dark | always | `night` "The Long Night" | the Source and the hub flame go out; you walk out with whoever you rescued |

End screen: title, four lines varied by tier and rescued names, stats (runs, deaths, rescued n/4, points), click to continue →
hub. `save.endings[id] = true`; the hub HUD shows `Endings: ✦✧✧` and a mark per ending seen; the Source stays open for
replays; after `night` the flame shows as tier 1 until you leave the hub.

## 10. Audio (`audio.js`, procedural WebAudio)

Context created on the first gesture; master gain 0.8 (0 when muted), persisted in `save.audio`. Layers: ambience drone
(hub 0.03 / zone 0.08, Cistern water noise, Ossuary lower base, Source scaled by lap; 1.5 s crossfade), drips, lamp crackle
(×3 below 15 oil), footsteps by distance walked (water slosh), hunter presence per hunter (0.22 max, fades out by 18 u,
scaled by state), chase sting (≥ 6 s apart), flash whoosh, lantern, pickup, bank chime, death hit, hub flame by tier, NPC
freed/caught/rescued, UI click/error/build/contract, ending chords.

One presence voice per hunter record, by profile (rebuilt when the profile at that index changes): `base`/`fast` growl ·
Lampwight breathy whistle (≤ 20 u) · Warden stone grind scaled by its sweep speed with a click at each reversal and treads
in CHASE/RETURN · Drowner water wash by its speed plus a plop every 2–5 s · **false light: no presence at all** — silence
is the lure; instead a fake lamp crackle at 0.6 × the handlamp's plus a faint chime while it is LIT within 6 u, cut the tick
it goes dark, and scrabbling at 12 Hz during a pounce · Brute breathing drone plus a stride thud per `creatureStep` (≤ 26 u,
panned). One-shots for every new event (§5.8): snuff, lantern crunch, warden clang + horn and its return, surge splash and
sink, pounce chitter, reveal shriek, the Brute's snort at a flash, a per-killer death hit.

## 11. Save (`save.js`, key `undercroft-v2`)

Written debounced 250 ms on every event and on pagehide. Imports v1 `undercroft-proto` points/oil once and keeps mirroring
`{points, bankedOil}` to it. Unknown keys are ignored, wrong types fall back to defaults.
```js
{ v: 3, points, oil, relics, rich, lightTech, reservoir, buildings: {workshop, press, cart, shrine, tram, elevator},
  rescued: {lamplighter, cartographer, keeper, deacon}, tools: {prybar, sluice, censer},
  gatesOpened: {zoneId: [gateId]},        // stable ids (the zone's tool), never cell indices
  shortcuts:   {zoneId: [shortcutId]},    // `=` doors lifted for good (§3.6)
  contracts: {active: [], done: [], progress: {}}, zoneSelected, blessing, endings: {cage, dawn, night},
  explored: {zoneId: base64 bitset}, stats: {runs, deaths, rescues, banked}, audio: {vol, muted}, importedV1 }
```
**v2 → v3 migration** (§3.6, additive): a v2 record keeps points/oil/relics/tools/buildings/rescued/
contracts/endings/stats verbatim; `explored` is dropped (a 40×40 bitset would light random cells once a zone is
re-authored bigger) and numeric `gatesOpened` entries are dropped (they were 40×40 cell indices) — the only cost is one
`E` press on a gate whose tool the save already owns. Unknown versions and entry types are ignored, never crashed on.

## 12. Modules, ctx and events (`main.js` owns the loop)

Modules import only `config.js`, `maps.js`, `models.js` (and THREE); everything else goes through the shared `ctx` and the
synchronous event bus (`on/off/emit`, try/catch per listener). Frame: `dt = min(dt, 0.05)`; input → player → world → hunter
→ npc → contracts → hub → endgame → audio → ui → render. Modes: `TITLE → HUB ↔ ZONE → DYING → DEAD → HUB`, plus `MENU`
(pointer released, sim paused) and `ENDING`.

| File | Owns |
|---|---|
| `config.js` | `CFG`, `HUNTER`, `HUNTER_PROFILES`, `CREATURE`, `TIERS`, `SCONCE_INT`, `POINTS`, `LIGHT_TECH`, `BUILD_COSTS`, `AUDIO`, `KEYS`, `TOOLS`, `HUB_WARMTH`, `EMBERS`, `HUB_BLOCK` |
| `maps.js` | the legend, `TUNING`, `PALETTES`, hub rows, `parseMap`/`parseZone`, `zoneLocked`, `lapOf`, grid helpers (`idx`, `isSolid`, `los`, `bfsField`, `pathTo`, `routeCells`), `validateMap`/`validateAll`, the `ZONES` assembly |
| `maps/<zone>.js` | one file per zone (§3.7): `ID SIZE ROWS REGIONS SHORTCUTS ANCHORS META` — the ASCII map, its region partition, its shortcut doors, the test anchors and every map-shaped number |
| `models.js` | `box/build/pattern`, factories: hunter, npc, flask, relic, richRelic, quest, bundle, lantern, stairs, elevator, gate, altar, water, tram, workshop, oilPress, cartTable, shrine, board, flameBase (≤ 30 boxes each; `models.html` previews them); hub props as box lists (`PROP_BOXES`: rug, bench, bedroll, crate, crateStack, barrel, logPile, cookpot, bookshelf, herbRail, candleCluster, hangLantern, stool) with `placeBoxes` / `mergeBoxes` (one solid + one glow mesh for any number of boxes) |
| `world.js` | instanced blocks, palette/fog (+ hub warmth per tier), items, lanterns, sconces, hub camp props + hanging lanterns + `map.blockMask`, gates, water surface, collision helpers |
| `hunter.js` | `ctx.hunters`, senses, the shared FSM driver and the per-creature `PROFILES` table (§5) |
| `npc.js` | `ctx.npcs`, follower, hub residents, dialogue |
| `contracts.js` | contract state, quest items, HUD lines |
| `hub.js` | flame tiers + flicker + embers, buildings (placement offsets, footprints → `blockMask`), services, board, minimap + explored bitsets, blessing, resident idle |
| `endgame.js` | Source laps and hunter staging, altar, endings, night visit |
| `audio.js` / `save.js` / `ui.js` | sound · persistence · all DOM (HUD, hint, toast, screens, generic menu, minimap canvas) |
| `main.js` | loop, `ctx`, events, input, player movement/lamp/flash/lantern/topUp/interact, transitions, `window.__game` |

Main events: `begin`, `hubEnter`, `zoneEnter`, `zoneExit`, `pickup`, `bank`, `flameTier`, `death`, `hunterCatch`,
`hunterState`, `flash`, `lantern`, `lanternRemoved`, `gateOpened`, `shortcutOpened`, `npcFreed`, `npcCaught`, `npcRescued`, `contractAccepted`,
`contractComplete`, `contractFailed`, `toolGained`, `build`, `lightTech`, `service`, `zoneSelected`, `blessing`, `lap`,
`ending`, `uiClick`, `uiError`, `toast`, plus the creature events of §5.8 (`lampSnuffed`, `lanternSmashed`, `wardenAlert`,
`wardenReturn`, `drownerSurge`, `drownerSink`, `falseLightPounce`, `falseLightReveal`, `flashResisted`, `creatureStep`). `window.__game` (alias `__proto`) exposes `ctx` fields and `actions.{loadZone,
selectZone, freeNpc, accept, build, choose, giveTool, setPoints, setResources, reset, …}` so tests can jump anywhere.

## 13. Acceptance checklist

- [ ] Title → click → hub at tier 1: flame dim, alcoves dark; E at the stairs descends into the selected zone.
- [ ] Zone is black beyond lamp radius; items glint; deep cells shrink the lamp; water slows and is heard from 7 u.
- [ ] Lamp on within 12 u + LOS ⇒ chase; lamp off + walking ⇒ it passes within 3 u unaware; Q ⇒ 3 s stagger; R ⇒ it will not enter the pool.
- [ ] Each creature teaches its rule and can be escaped by light play: Lampwight (douse), Warden (leave its ground / go dark),
      Drowner (one cell back from the water), false light (flash it / a pool), Brute (douse and walk; pools do not stop it) — §5.9.
- [ ] Bank 6+ pts ⇒ tier 2 toast, hub brighter, tram ghost; tier 3 elevator; tier 4 at 30 pts.
- [ ] Death ⇒ bundle at the death spot, points intact, recoverable next run; blessing banks half.
- [ ] Free Wick, bank with him ≤ 4 u ⇒ rescued, Workshop ghost, contract offered; Pry Bar opens the NW crypt to Maud.
- [ ] A barred `=` door refuses `E` from the entrance side ("Barred from the other side") and lifts with one `E` from
      its far side; the toast fires, the minimap turns mint, and it is still open after a death and a reload (§3.6).
- [ ] Board locks: Cistern until Tram, Ossuary until Elevator + Light-tech II, Source until tier 4 + Maud.
- [ ] All 8 contracts complete and pay; Workshop/Press/Table/Shrine services work; Tab minimap shows explored cells.
- [ ] Source: darker per lap, extra hunters at laps 3–4, ride-up confirm, altar choice; `dawn` greyed unless tier 4 & 3 rescued; `save.endings` set.
- [ ] Save round-trips on reload; v1 points import once; Backspace ×2 on the title wipes it; no console errors headless.
