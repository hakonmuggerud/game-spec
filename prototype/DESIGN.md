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
| W A S D | Move (walk 3.5 u/s) · Shift sprint (6.0 u/s, noise, FOV 75→82) |
| F | Toggle handlamp (off = stealth, burns nothing) |
| Q | Flash: −flashCost oil, staggers a hunter in a cone ahead (§5) |
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
| `N` | Captive NPC cell (`ZONES[id].npcs` names who) |
| `C` | Contract spot (row-major index = `spot` in contracts) |
| `W` | Water: walkable; player ×0.55 (sprint ×0.6), hunters ×0.85; wading is heard 7 u away without LOS |
| `X` | Gate: solid until the zone's tool is owned; E opens it for good (`save.gatesOpened`) |
| `A` | The Source altar |
| `F` / `0–9` | Hub great flame / hub building anchors (floor for the grid; the hub's `blockMask` adds footprints, props and residents) |

### Zones (`maps.ZONES`; 40×40, row 0 = north, x = column, z = row)

| id | name | entry | burn | lamp | hunters | captives | gate (tool → opens) | requires |
|---|---|---|---|---|---|---|---|---|
| `undercroft` | The Undercroft | S | ×1 | ×1 | 1 base | Wick (5,22), Deacon (2,3) | Pry Bar → X (4,5), NW crypt | — |
| `cistern` | The Cistern | S | ×1 | ×1 | 2 base | Ines (38,22) | Sluice Key → X (20,5), flooded vault | building `tram` |
| `ossuary` | The Ossuary | V | ×1.3 | ×0.85 | 1 fast | Oren (7,5) | Censer → X (14,4), reliquary | building `elevator` + lightTech ≥ 2 |
| `source` | The Source | V | bands | bands | 2 fast (+§9) | — | — | flame tier 4 + Deacon rescued; no banking |

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
| Lamp | PointLight 0xffb265, intensity 9, distance 16 × deep × zone × light-tech, decay 2; flicker 0.93+0.07·sin(13t), ×3 amplitude below 15 oil |
| Flash (Q) | cost 15 (12 at tech II, 10 at III), cooldown 1.5 s; lamp ×8 for 0.15 s, white screen 0.1 s |
| Plant lantern (R) | cost 20 (16 at tech III), cooldown 1 s, max 4 alive (oldest removed); pool radius 2.5 u; lasts until you leave the zone |
| Flask (T) | +25 oil, consumes one carried flask |
| Light-tech I / II / III | lamp distance ×1.15 / 1.3 / 1.45, burn ×0.9 / 0.8 / 0.7; cost 6 / 10 / 14 relics (+2 rich at III) |
| Warning | HUD bar red below 20 oil, "Lamp guttering"; vignette darkens |

Tension: a flask is +1 flame point and +25 banked oil, or +25 oil now — never both.

## 5. Hunters (`hunter.js`)

Mesh: `models.hunter()` hunched 11-box figure, 0x050505 with emissive eyes (wander 0.4 / investigate 0.8 / chase 1.5 /
staggered 0.05). Profiles: `base` 1.8 / 3.0 / 4.3 u/s, catch 0.8 u, loses target after 3 s; `fast` 2.2 / 3.6 / 5.0, catch 0.9,
loses after 4 s, scale y 1.15, amber eyes.

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
| `config.js` | `CFG`, `HUNTER`, `HUNTER_PROFILES`, `TIERS`, `SCONCE_INT`, `POINTS`, `LIGHT_TECH`, `BUILD_COSTS`, `AUDIO`, `KEYS`, `TOOLS`, `HUB_WARMTH`, `EMBERS`, `HUB_BLOCK` |
| `maps.js` | rows, `ZONES`, `PALETTES`, `HUB_ROWS_V2`, `parseMap`, `zoneLocked`, `lapOf`, grid helpers (`idx`, `isSolid`, `los`, `bfsField`, `pathTo`, …) |
| `models.js` | `box/build/pattern`, factories: hunter, npc, flask, relic, richRelic, quest, bundle, lantern, stairs, elevator, gate, altar, water, tram, workshop, oilPress, cartTable, shrine, board, flameBase (≤ 30 boxes each; `models.html` previews them); hub props as box lists (`PROP_BOXES`: rug, bench, bedroll, crate, crateStack, barrel, logPile, cookpot, bookshelf, herbRail, candleCluster, hangLantern, stool) with `placeBoxes` / `mergeBoxes` (one solid + one glow mesh for any number of boxes) |
| `world.js` | instanced blocks, palette/fog (+ hub warmth per tier), items, lanterns, sconces, hub camp props + hanging lanterns + `map.blockMask`, gates, water surface, collision helpers |
| `hunter.js` | `ctx.hunters`, senses, FSM, pathing |
| `npc.js` | `ctx.npcs`, follower, hub residents, dialogue |
| `contracts.js` | contract state, quest items, HUD lines |
| `hub.js` | flame tiers + flicker + embers, buildings (placement offsets, footprints → `blockMask`), services, board, minimap + explored bitsets, blessing, resident idle |
| `endgame.js` | Source laps and hunter staging, altar, endings, night visit |
| `audio.js` / `save.js` / `ui.js` | sound · persistence · all DOM (HUD, hint, toast, screens, generic menu, minimap canvas) |
| `main.js` | loop, `ctx`, events, input, player movement/lamp/flash/lantern/topUp/interact, transitions, `window.__game` |

Main events: `begin`, `hubEnter`, `zoneEnter`, `zoneExit`, `pickup`, `bank`, `flameTier`, `death`, `hunterCatch`,
`hunterState`, `flash`, `lantern`, `lanternRemoved`, `gateOpened`, `npcFreed`, `npcCaught`, `npcRescued`, `contractAccepted`,
`contractComplete`, `contractFailed`, `toolGained`, `build`, `lightTech`, `service`, `zoneSelected`, `blessing`, `lap`,
`ending`, `uiClick`, `uiError`, `toast`. `window.__game` (alias `__proto`) exposes `ctx` fields and `actions.{loadZone,
selectZone, freeNpc, accept, build, choose, giveTool, setPoints, setResources, reset, …}` so tests can jump anywhere.

## 13. Acceptance checklist

- [ ] Title → click → hub at tier 1: flame dim, alcoves dark; E at the stairs descends into the selected zone.
- [ ] Zone is black beyond lamp radius; items glint; deep cells shrink the lamp; water slows and is heard from 7 u.
- [ ] Lamp on within 12 u + LOS ⇒ chase; lamp off + walking ⇒ it passes within 3 u unaware; Q ⇒ 3 s stagger; R ⇒ it will not enter the pool.
- [ ] Bank 6+ pts ⇒ tier 2 toast, hub brighter, tram ghost; tier 3 elevator; tier 4 at 30 pts.
- [ ] Death ⇒ bundle at the death spot, points intact, recoverable next run; blessing banks half.
- [ ] Free Wick, bank with him ≤ 4 u ⇒ rescued, Workshop ghost, contract offered; Pry Bar opens the NW crypt to Maud.
- [ ] Board locks: Cistern until Tram, Ossuary until Elevator + Light-tech II, Source until tier 4 + Maud.
- [ ] All 8 contracts complete and pay; Workshop/Press/Table/Shrine services work; Tab minimap shows explored cells.
- [ ] Source: darker per lap, extra hunters at laps 3–4, ride-up confirm, altar choice; `dawn` greyed unless tier 4 & 3 rescued; `save.endings` set.
- [ ] Save round-trips on reload; v1 points import once; Backspace ×2 on the title wipes it; no console errors headless.
