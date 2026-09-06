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
| E | Interact, asked in order: endgame (altar / ride up) → npc → hub → items / gates / stairs |
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
  0x0b0a14; the hub warms its ambient/fog per flame tier, §3 hub). All real light is point lights: lamp, planted lanterns, hub
  flame, hub sconces, five hub lanterns.

### Legend

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
| `X` | Gate: solid until the zone's tool is owned; E opens it for good (`save.gatesOpened`) |
| `A` | The Source altar |
| `F` / `0–9` | Hub great flame / hub building anchors (floor for the grid; the hub's `blockMask` adds footprints, props and residents) |

### Zones (`maps.ZONES`; 40×40, row 0 = north, x = column, z = row)

| id | name | entry | burn | lamp | hunters | captives | gate (tool → opens) | requires |
|---|---|---|---|---|---|---|---|---|
| `undercroft` | The Undercroft | S | ×1 | ×1 | 1 base + Warden (26,2) + Brute (4,17) | Wick (5,22), Deacon (2,3) | Pry Bar → X (4,5), NW crypt | — |
| `cistern` | The Cistern | S | ×1 | ×1 | 2 base + Drowner (20,13) + Lampwight (30,27) | Ines (38,22) | Sluice Key → X (20,5), flooded vault | building `tram` |
| `ossuary` | The Ossuary | V | ×1.3 | ×0.85 | 1 fast + Warden (12,6) + false lights (18,19) (26,25) | Oren (7,5) | Censer → X (14,4), reliquary | building `elevator` + lightTech ≥ 2 |
| `source` | The Source | V | bands | bands | 2 fast (+§9) + Lampwight (7,20) + false light (29,14) + Brute (13,20) | — | — | flame tier 4 + Deacon rescued; no banking |

Loot per full clear: Undercroft 6o+5r+2R = 31 · Cistern 4o+5r+2R = 29 · Ossuary 6o+7r+5R = 52 · Source 4o+2r+2R = 20.
Items respawn each expedition. Contract spots: Undercroft (30,2) NE crypt, (16,29) great hall · Cistern (20,19) drowned hall,
(3,26) pump room · Ossuary (29,18) east bone-pit, (19,28) south vault.

#### The Undercroft
```
########################################
#DDDDDDDD###############DDDDDDDDDDDDDDD#
#DDDRDDDD###############DDDDDDCDDRDDDDD#
#DNDDDDDD###############DDDoDDDDDDDDDDD#
#DDDDDDDD###############DDDDDDDDDDDDDDD#
####X#############################.#####
#........#.............#...............#
#........#..P...P...P..#.....P...P.....#
#........#......H......#...............#
#..r......................o............#
#........#..P...P...P..#.....P...P.....#
#........#......r......#...............#
#........#.............#...............#
#........#.............#...............#
####.###########...#################.###
#........#..............#..............#
#..P.....#...P......P...#......P.......#
#........#..............#........o.....#
#........#..............#..............#
#........#...P......P...#......P...r...#
####.#####..............#..............#
#.o......#..............#..............#
#....N...#...P......P...#......P.......#
#........#..............#..............#
#........#..............######.#########
#........#...P......P...#..............#
#.........................o............#
#........#..............#..............#
#........#...P......P...#.........r....#
#..r.....#......C.......#..............#
#........#..............#..............#
#........#..............#............o.#
##################....##################
###############..........###############
###############.P......P.###############
###############..........###############
###############.P......P.###############
###############....S.....###############
###############..........###############
########################################
```
#### The Cistern
```
########################################
#......#WWWWWWWWWWWWWWWWWWWWWWWW#......#
#..r...#WWWWWWWWWWWWWWWWWWWWWWWW#...r..#
#......#WWWWWWWWWWWR.WWWWWWWWWWW#......#
#......#WWWWWWWWWWW.RWWWWWWWWWWW#......#
###.################X###############.###
#WWWW.............................WWWW.#
#WWoW.........H..P.....P..........WWWW.#
#WWWW.........P...........P.......WWWW.#
#.WWWW............WWWWWW.........WWWW..#
#..WWWW.........WWWWWWWWWW...H..WWWW...#
#..WWWW.........WWWWWWWWWW......WWWW...#
#...WWWW..P...WWWWWWWWWWWWWW...PWWW....#
#....WWWW.....WWWWWWWWWWWWWW..WWWW.....#
#...WWWW......WWWWWWWWWWWWWW....WWW....#
#..WWWW.........WWWWWWWWWW......WWWW...#
#..WWWW.........WWWWWWWWWW......WWWW...#
#.WWWW........P...WWWWWW..P......WWWW..#
#WWWW.............................WWWW.#
#WWWW...............C.............WWWW.#
#WWWW.WWWWWWWWWWWWWWWWWWWWWWWW....WWWW.#
####..###WWWWWWWWWWWWWWWWWWWW#.....#####
#....#..........WWWWWWWWWW.........#..N#
#.r..#..........WWWWWWWWWW.........#..o#
#....#.....P......WWWWWW....P..........#
#..................................#...#
#..C.#......WWWW........WWWW.......##.##
#....#......WWWW........WWWW.......#...#
#....#......WWWW.....r..WWWW.......#...#
#....#.P....WWWW........WWWW....P..#...#
#.o..#......................r......#...#
#....#.............................#.o.#
##################....##################
###############..........###############
###############.P..WW..P.###############
###############....WW....###############
###############.P......P.###############
###############....S.....###############
###############..........###############
########################################
```
#### The Ossuary
```
########################################
########################################
##DDDDDDDD#DDD#DDDDDDDDD######DDDDDDDD##
##DRDDDDDD#DRD#DDDDDDDDD######DDDDDRDD##
##DDDDDDDD#DDDXDDDDDrDDD######DDDDDDDD##
##DDDDDNDD#DRD#DDDDHDDDD######DDDDDDDD##
##DDDDDDDD#DDD#DDDDDDDDD######DDDDDDDD##
##DDDDDDDD#####DDDDDDDDD######DDDDDDDD##
##DDDDDDDD#####DDDDDDDDD######DDDDDDDD##
#####.#############.#############.######
#####.#############.#############.######
#####.#############.#############.######
#......................................#
#.DDDDDD.#..##########.#..###.DDDDDD.###
#.DDDDDD.#r.##########.#r.###.DDDDDD.###
#.DDDDDD.#############.######.DDRoDD.###
#.DDDDDD.#############.######.DDDDDD.###
#.DDDDDD.#############.######.DDDDDD.###
#.....................P......C.........#
#.##..##.########..###.######.#..###.###
#.##o.##.########r.###.######.#r.###.###
#.######.#############.######.######.###
#.######.#############.######.######.###
#.######.#############.######.######.###
#.......P..............................#
#...####.##..##.DDDDDD.##..##.######...#
#.r.####.##o.##.DDDDDD.##o.##.######.r.#
#.######.######.DDDDDD.######.######.###
#.######.######.DDDCDD.######.######.###
#.######.######.DDDDDD.######.######.###
#......................................#
###..##########.#..##########.#######..#
###..##########.#..##########.#######o.#
###############.#############.##########
###############.#############.##########
######.....####.#############.##########
######..o.......#############.##########
######.V...#############################
######.....#############################
########################################
```
#### The Source (clockwise spiral, one door per lap at the top-left, altar chamber at the centre)
```
########################################
#V.....................................#
#.....................................o#
#####################################..#
#..DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD#..#
#..DoDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD#..#
#..###############################DD#..#
#..#DDDDDDDDDDDDDDDDDDDDDDDDDDDDD#DD#..#
#..#DDDoDDDDDDDDDDDDDDDDDDDDDDDDD#DD#..#
#..#DD#########################DD#DD#..#
#..#DD#DDDDDDDDDDDDDDDDDDDDDDD#DD#DD#..#
#..#DD#DDDDDDDDDDDDDDDDDDDDDDD#DD#DD#..#
#..#DD#DD###################RD#DD#DD#..#
#..#DD#DD#DDDDDDDDDDDDDDDDD#DD#DD#DD#..#
#..#DD#DD#DDDDDDDDDDDDDDDDD#DD#DD#DD#..#
#..#DD#DD#DD#############DD#DD#DD#DD#..#
#..#DD#DD#DD#DDDDDDDDDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DDDDDDDDDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DD#DDDADDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DD#DDDDDDDD#DD#DD#DD#DD#..#
#..#DD#DD#DD#DD##########DD#DD#DD#DD#..#
#..#DD#DD#DD#DDDDDDDDDDDDDD#DD#DD#DD#..#
#..#DD#DD#DH#DDDDDDDDDDDDDD#DD#DD#DD#..#
#..#DD#DD#DD################DD#DD#DD#..#
#..#DD#DD#DDDDDDDDDDDDDDDDDDDD#DD#DD#..#
#..#DD#DD#oDDDDDDDDDDDDDDDDDDD#DD#DD#..#
#..#DD#DD######################DD#DD#H.#
#..#DD#DDDDDDDDDDDDDDDDDDDDDDDDDR#DD#..#
#..#DD#DDDDDDDDDDDDDDDDDDDDDDDDDD#DD#..#
#..#DD############################DD#..#
#..#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD#..#
#..#DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDr#..#
#..##################################..#
#......................................#
#...................r..................#
########################################
```
Source bands: `lap = min(5, floor((min(x, z, 39−x, 39−z) − 1) / 3))`; `D` cells burn ×(1.1 + 0.12·lap) and light ×(0.95 −
0.08·lap). Lap 0 is plain floor, the chamber is lap 5. Fog density ×(1 + 0.10·lap), ambient ×(1 − 0.13·lap) (min 0.25),
eased at 2/s; a flavour line per lap.

### Hub — "The Last Lantern" (25×13 at x=+60; digits = building anchors)
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

**Undercroft** — base `H` (16,8) stays. `G` (26,2) facing **E**, `B` (4,17).
```
z1  #DDDDDDDD###############DDDDDDDDDDDDDDD#
z2  #DDDRDDDD###############DDGDDDCDDRDDDDD#      G at x26: post at the west end of the NE crypt, facing east
z3  #DNDDDDDD###############DDDoDDDDDDDDDDD#      cone reach 9 covers o (27,3) 1.4 u, spot C (30,2) 4 u, R (33,2) 7 u
z4  #DDDDDDDD###############DDDDDDDDDDDDDDD#      and the doorway (34,4) at 8.25 u — seen on entry when the sweep is east,
z5  ####X#############################.#####      but the doorway is just outside the 8 u territory: one step back ends a chase
z15 #........#..............#..............#
z16 #..P.....#...P......P...#......P.......#      B at (4,17): the west wing's middle chamber; leash 14 BFS covers the
z17 #...B....#..............#........o.....#      wing (doors (4,14), (4,20)) and lets it stand in the hall mouths
z18 #........#..............#..............#      (9,9) / (9,26) at BFS 13–14, never the great hall or the S stairs.
```
Justification: the player meets the Brute when going for Wick (5,22) and the relic (3,9) — the first "pools are not
walls" lesson, mid-run — and the Warden when going for the NE crypt's rich relic and spot 0, the far corner. Both are
away from the entry hall, so a first run still starts against the single base hunter.

**Cistern** — base `H` (14,7) and (29,10) stay. `w` (20,13), `L` (30,27).
```
z9  #.WWWW............WWWWWW.........WWWW..#      The central lake (rows 9–17, x 14–27 at its widest) is one water body:
z12 #...WWWW..P...WWWWWWWWWWWWWW...PWWW....#      the flood-fill from (20,13) never reaches the west/east bands or the
z13 #....WWWW.....WWWWWWwWWWWWWW..WWWW.....#      south moat. Every causeway wraps around it, so it is a risky shortcut,
z17 #.WWWW........P...WWWWWW..P......WWWW..#      never a mandatory crossing. Spot C (20,19) is 1.5 u from the south
z19 #WWWW...............C.............WWWW.#      shore: the 30 s vigil is safe, one step north (20,18) is adjacent → trigger.
z27 #....#......WWWW........WWWW..L....#...#      L at (30,27): the south-east hall on the way to Ines' cell block
z28 #....#......WWWW.....r..WWWW.......#...#      (door (35,24) 5.8 u, Ines (38,22)); 24 u LOS across the open south hall.
```
Justification: the north hall keeps the two hunters plus the lake; the south hall gets the Lampwight, so the route to
Ines is an oil tax rather than a death trap, and the flooded vault run (gate (20,5)) crosses nothing new.

**Ossuary** — fast `H` (19,5) stays. `G` (12,6) facing **N**, `Y` (18,19), `Y` (26,25).
```
z3  ##DRDDDDDD#DRD#DDDDDDDDD######DDDDDRDD##      G at (12,6): the reliquary is x11–13 × z2–6 behind the Censer gate X
z4  ##DDDDDDDD#DDDXDDDDDrDDD######DDDDDDDD##      (14,4). Post at its south end facing north, sweep ±75°: both rich
z5  ##DDDDDNDD#DRD#DDDDHDDDD######DDDDDDDD##      relics (12,3) 3 u and (12,5) 1 u are in the cone; the gate cell is
z6  ##DDDDDDDD#DGD#DDDDDDDDD######DDDDDDDD##      2.8 u at bearing 45° — swept, so a lit player opening it is seen.
z18 #.....................P......C.........#      Y at (18,19): the 2×2 alcove off corridor row 18 that holds the
z19 #.##..##.########.Y###.######.#..###.###      relic (17,20): LOS from the mouth cells (16–18,18), its 6 u glow spills onto the corridor.
z20 #.##o.##.########r.###.######.#r.###.###
z24 #.......P..............................#      Y at (26,25): the alcove off corridor row 24 with the flask (25,26),
z25 #...####.##..##.DDDDDD.##.Y##.######...#      (LOS from (24–27,24)), beside the south vault (spot C (19,28), the vigil).
z26 #.r.####.##o.##.DDDDDD.##o.##.######.r.#
```
Justification: the Ossuary is where light-tech II makes the lamp reach 14 u, so it is the zone to teach "a glow you did
not plant". Both false lights rest beside loot in alcoves the corridor shows you; the Warden makes the Censer reward a
timing puzzle inside a deep pocket (lamp ×0.6) rather than a free grab.

**Source** — fast `H` (11,26) lap 3 and (37,30) lap 0 stay. `L` (7,20) lap 2 · `Y` (29,14) lap 3 · `B` (13,20) lap 4.
```
z12 #..#DD#DD###################RD#DD#DD#..#      Y at (29,14): two cells down the lap-3 east passage from the rich relic
z13 #..#DD#DD#DDDDDDDDDDDDDDDDD#DD#DD#DD#..#      nook (28,12); LOS only from the passage end (28–29, 10–13), so you turn
z14 #..#DD#DD#DDDDDDDDDDDDDDDDD#DY#DD#DD#..#      the corner, see a lantern by a rich relic, and grabbing R is 2.2 u from it.
z20 #..#DD#LD#DD#BD#DDDDDDDD#DD#DD#DD#DD#..#      L at (7,20) on the lap-2 west corridor (long LOS along the 2-wide passage,
                                                 a 24 u beacon); B at (13,20) on the lap-4 ring, one wall from the chamber.
                                                 endgame.js dormancy applies to all three (wake at lap − 1).
```
Justification: the Source stacks its lessons by depth — the Lampwight taxes oil on lap 2 exactly where the bands raise
burn, the False light guards a rich relic at lap 3, and the Brute walks the last ring where lanterns are the only safe
light and it takes them away. `pickSpawnCell` and `maxHunters` count only base/fast records.

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
- Drowner trigger 6 u: it only bites when you use the water; the Cistern's 30 s vigil spot (20,19) is 1.5 u from the
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
| `lamplighter` | Wick the Lamplighter | Undercroft (5,22) | Workshop | "Every lamp I ever lit is out. Let's fix that." |
| `cartographer` | Ines the Cartographer | Cistern (38,22) | Cartographer's Table | "I mapped every one of these halls. Then they moved." |
| `keeper` | Oren the Oil-press Keeper | Ossuary (7,5) | Oil Press | "Relics burn better than they pray." |
| `deacon` | Deacon Maud | Undercroft (2,3), behind the Pry Bar gate | Shrine; the Source | "The Source can be fed, or freed. Both are prayers." |

CAPTIVE (E within 1.6 u: "[E] Free Wick") → FOLLOW → rescued on bank ≤ 4 u away, or CAUGHT (hunter within 0.8 u, not in a
pool) → sinks 1 s → CAPTIVE again in its cell (never lost). One follower at a time ("You cannot shepherd two"). Follow AI every
0.25 s: BFS toward the player (pools not blocked), 3.5 u/s (5.0 beyond 6 u), stops at 1.5 u, snaps next to the player beyond
14 u; wall-sliding radius 0.25. Rescued NPCs stand at their anchor +1 x in the hub (beside their building, never in a walkway — §3 hub), turn toward the
player within 5 u; E opens a dialogue with the next contract (1/Enter accepts).

Contracts: each NPC posts one at a time in table order; max 2 active; progress in `save.contracts`. `fetch` = bank ≥ n of a kind
from that zone in one run (progress = carried while there); `plant` = lantern within 1.5 u of the spot; `recover` = a quest
item spawns at the spot, pick it up and bank it (lost on death like loot); `survive` = contiguous seconds within 3 u of the spot
(`lampOff` variants need the lamp doused; leaving resets). Death, an early bank or leaving the zone resets run-scoped progress
(`contractFailed`) but keeps the contract.

| id | poster | type | target | reward |
|---|---|---|---|---|
| `c_relight` | Wick | plant | Undercroft great hall (16,29) | Pry Bar + 4 pts |
| `c_wick` | Wick | fetch | 4 oil flasks, Undercroft | 60 oil |
| `c_sound` | Ines | survive 30 s | Cistern drowned hall (20,19), lamp allowed | Sluice Key |
| `c_chart` | Ines | recover "lost chart" | Cistern pump room (3,26) | 8 pts |
| `c_censer` | Oren | fetch | 2 rich relics, Ossuary | Censer |
| `c_ledger` | Oren | recover "ledger" | Ossuary east bone-pit (29,18) | 80 oil |
| `c_vigil` | Maud | survive 45 s, lamp off | Ossuary south vault (19,28) | 10 pts |
| `c_bones` | Maud | fetch | 3 relics, Ossuary | 6 pts |

## 8. Hub buildings and services (`hub.js`)

Ghost (wireframe 0x3a3020) appears when unlocked; E within 1.8 u of the anchor opens the build / service menu. Costs spend the
resource ledger only (`save.oil/relics/rich`); flame points are never spent.

| Building (anchor cell) | Unlock | Cost | Service |
|---|---|---|---|
| Departure Board (5 · 15,9) | always | — | zones with lock reasons + active contract targets; 1–4 selects `save.zoneSelected`; stairs, tram and elevator all descend there |
| Workshop (1 · 3,1) | Wick | 6 relics | Light-tech I–III (§4) |
| Oil Press (2 · 19,1) | Oren | 80 oil | press 1 / all relics at 30 oil each; "Deeper reservoir" 5 then 8 relics → +15 start oil each |
| Cartographer's Table (3 · 3,4) | Ines | 100 oil | Tab minimap (200×200 canvas, 5 px/cell, 8 Hz): cells seen within lamp distance (≤ 16 u, 2 u dark) with LOS, plus bordering walls; items seen, lanterns, spots ◇, exit ▲, NPCs; per-zone bitset saved base64; % charted per zone |
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
{ v: 2, points, oil, relics, rich, lightTech, reservoir, buildings: {workshop, press, cart, shrine, tram, elevator},
  rescued: {lamplighter, cartographer, keeper, deacon}, tools: {prybar, sluice, censer}, gatesOpened: {zoneId: {...}},
  contracts: {active: [], done: [], progress: {}}, zoneSelected, blessing, endings: {cage, dawn, night},
  explored: {zoneId: base64 bitset}, stats: {runs, deaths, rescues, banked}, audio: {vol, muted}, importedV1 }
```

## 12. Modules, ctx and events (`main.js` owns the loop)

Modules import only `config.js`, `maps.js`, `models.js` (and THREE); everything else goes through the shared `ctx` and the
synchronous event bus (`on/off/emit`, try/catch per listener). Frame: `dt = min(dt, 0.05)`; input → player → world → hunter
→ npc → contracts → hub → endgame → audio → ui → render. Modes: `TITLE → HUB ↔ ZONE → DYING → DEAD → HUB`, plus `MENU`
(pointer released, sim paused) and `ENDING`.

| File | Owns |
|---|---|
| `config.js` | `CFG`, `HUNTER`, `HUNTER_PROFILES`, `CREATURE`, `TIERS`, `SCONCE_INT`, `POINTS`, `LIGHT_TECH`, `BUILD_COSTS`, `AUDIO`, `KEYS`, `TOOLS`, `HUB_WARMTH`, `EMBERS`, `HUB_BLOCK` |
| `maps.js` | rows, `ZONES`, `PALETTES`, `HUB_ROWS_V2`, `parseMap`, `zoneLocked`, `lapOf`, grid helpers (`idx`, `isSolid`, `los`, `bfsField`, `pathTo`, …) |
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
`hunterState`, `flash`, `lantern`, `lanternRemoved`, `gateOpened`, `npcFreed`, `npcCaught`, `npcRescued`, `contractAccepted`,
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
- [ ] Board locks: Cistern until Tram, Ossuary until Elevator + Light-tech II, Source until tier 4 + Maud.
- [ ] All 8 contracts complete and pay; Workshop/Press/Table/Shrine services work; Tab minimap shows explored cells.
- [ ] Source: darker per lap, extra hunters at laps 3–4, ride-up confirm, altar choice; `dawn` greyed unless tier 4 & 3 rescued; `save.endings` set.
- [ ] Save round-trips on reload; v1 points import once; Backspace ×2 on the title wipes it; no console errors headless.
