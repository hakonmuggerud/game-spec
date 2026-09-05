# The Undercroft — Playable Prototype (Vertical Slice) Design

Target: one self-contained `prototype/index.html` (HTML + CSS + one ES module), Three.js 0.160.0 via
importmap (`three` + `three/addons/` → jsdelivr), plain JS, no build, ≤ ~1500 lines. Serve with
`python3 -m http.server 8765 --directory prototype`. Audio: skipped.

## 1. Slice goal (what must be felt in 5 minutes)

Hub (warm, safe, small flame) → descend → lamp-lit crawl through pitch-black ruin → hunter drawn to
your light → douse / flash / plant lantern to survive → bank loot at the stairs → flame visibly grows.
Die and you lose carried loot (dropped as a retrievable bundle); hub progress persists.

## 2. Controls

| Key | Action |
|---|---|
| Mouse | Look (PointerLockControls; click title screen / canvas to lock) |
| W A S D | Move (walk 3.5 u/s) |
| Shift | Sprint (6.0 u/s, makes noise, FOV 75→82) |
| F | Toggle handlamp on/off (off = dark stealth, burns nothing) |
| Q | Flash: −15 oil, staggers hunter in a cone ahead (see §6) |
| R | Plant lantern: −20 oil, creates a safe pool at your feet |
| E | Interact: pick up item / bank at stairs / descend at hub stairs |
| T | Top up: pour one carried oil flask into the lamp (+25 oil) |
| Esc | Release pointer (pause; HUD shows "click to resume") |

## 3. World / grid conventions

- 1 cell = 1 world unit. Cell (cx, cz) occupies x∈[cx, cx+1), z∈[cz, cz+1). Floor top at y=0, ceiling at y=3.
- Player: cylinder radius 0.3, eye height 1.6 (camera y). Camera: fov 75, near 0.05, far 40.
- Two grids in one scene: **zone** at origin, **hub** offset to x=+60 (fog hides everything past ~20 u).
- Rendering: `renderer.setSize(floor(w/3), floor(h/3), false)`, canvas CSS 100vw×100vh,
  `image-rendering: pixelated`. `MeshLambertMaterial`, per-instance color jitter (±6%) via `setColorAt`.
- Geometry: one `InstancedMesh` per block type: floor (1×0.2×1, top y=0), wall (1×3×1), pillar
  (0.8×3×0.8, own tint), ceiling (1×0.2×1 at y=3, over every non-wall cell), deep floor (bluish-black tint).
- Atmosphere: `scene.fog = FogExp2(0x000000, 0.11)`, `AmbientLight(0x0b0a14, 1)` (barely visible),
  background black. All real light comes from point lights (lamp, planted lanterns, hub flame).

### ASCII legend (both maps)

| Char | Meaning |
|---|---|
| `#` | Wall block (solid) |
| `P` | Pillar (solid for collision, drawn 0.8 wide) |
| `.` | Floor |
| `D` | Deep floor (pitch-black pocket: lamp radius ×0.6, burn rate ×1.5, walkable) |
| `S` | Stairs (zone: spawn + extraction point; hub: descend). Has a faint cold-blue emissive marker |
| `o` | Floor + oil flask item (carried; bank = 1 pt, or T = +25 oil) |
| `r` | Floor + relic item (bank = 3 pts) |
| `R` | Deep floor + rich relic (bank = 5 pts) |
| `H` | Floor + hunter spawn |
| `F` | Floor + the great flame (hub only) |

### Zone map — "The Undercroft" (40×40, row 0 = north, x = column, z = row)

```
########################################
#DDDDDDDD###############DDDDDDDDDDDDDDD#
#DDDRDDDD###############DDDDDDDDDRDDDDD#
#DDDDDDDD###############DDDoDDDDDDDDDDD#
#DDDDDDDD###############DDDDDDDDDDDDDDD#
####.#############################.#####
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
#........#...P......P...#......P.......#
#........#..............#..............#
#........#..............######.#########
#........#...P......P...#..............#
#.........................o............#
#........#..............#..............#
#........#...P......P...#.........r....#
#..r.....#..............#..............#
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

Layout: entry hall (S) → pillared great hall (loops west/east wings) → north crypt (hunter home) →
two deep pockets (NW via west antechamber, NE via east upper room), each through a 1-wide door.
Loot: 6 oil, 5 relics, 2 rich relics = 31 pts per full clear. All items respawn each expedition.

### Hub map — "The Last Lantern" (17×9). Placed at x offset +60.

```
#################
#....#.....#....#
#....#.....#....#
#.......F.......#
#....#.....#....#
#....#.....#....#
######.....######
######..S..######
#################
```

Side alcoves are pitch black at tier 1 and emerge as the flame grows (that IS the progress meter).
Hub has no hunter, lamp is forced off and does not burn. Spawn at S facing the flame.

## 4. Collision (player)

Solid cells: `#`, `P`. Per frame, move X then Z separately. For an axis step, compute the AABB
`[x−0.3, x+0.3]×[z−0.3, z+0.3]` at the new position, check the ≤4 cells it overlaps
(`floor` of each corner); if any is solid, cancel that axis' movement. No Y movement (flat floor).
Hunter follows BFS path cell centres, so it needs no collision. Position outside grid = solid.

## 5. Light economy — numbers

| Value | Number |
|---|---|
| Oil capacity | 100 |
| Starting oil on descend (by flame tier 1/2/3/4) | 50 / 60 / 70 / 80 |
| Lamp burn while lit | 0.5 oil/s (×1.5 on `D` cells). Oil 0 → lamp forced off |
| Lamp light | PointLight 0xffb265, intensity 9 (cd, r160 physical units), distance 16 (×0.6 on `D`), decay 2, child of camera at (0.25, −0.2, 0). Flicker: intensity × (0.93 + 0.07·sin(t·13)); below 15 oil flicker ×3 amplitude |
| Flash (Q) | cost 15, cooldown 1.5 s, needs oil ≥ 15. Lamp intensity ×8 for 0.15 s, screen white flash 0.1 s |
| Plant lantern (R) | cost 20, needs oil ≥ 20, cooldown 1 s, max 4 alive (oldest removed). Pool radius 2.5 u. PointLight 0xffc070 int 2.0 dist 6 + emissive ground ring r 2.5. Lasts until you leave the zone |
| Oil flask (T) | +25 oil, consumes one carried flask |
| Warning | HUD oil bar turns red < 20; hint "Lamp guttering" |

Design tension: a flask is either +1 flame point banked or +25 oil now — never both.

## 6. Hunter — one creature

Mesh: 0.6×1.8×0.6 box, colour 0x050505 (invisible except against lit surfaces) + two emissive eye
spheres (r 0.06, 0xff3a20) at y 1.5, emissive intensity by state: wander 0.4 / investigate 0.8 /
chase 1.5 / staggered 0.05. Spawns at `H` each expedition.

| Value | Number |
|---|---|
| Speeds: wander / investigate / chase | 1.8 / 3.0 / 4.3 u/s (player walks 3.5, sprints 6.0) |
| Catch radius | 0.8 u (xz distance) and player not inside a pool |
| Stimulus radii (checked every 0.2 s) | lamp lit: 12 u **with LOS** · sprinting: 9 u (no LOS needed, noise) · walking lamp off: 2.5 u · still lamp off: 1.0 u |
| LOS | Grid DDA from hunter to player at y 1; blocked by `#`/`P` |
| Lose target | 3 s without any stimulus → INVESTIGATE(lastKnown) |
| Stagger | 3.0 s frozen; then WANDER to a cell ≥ 12 cells from the player, ignoring all stimuli for 4 s (dazed) so the retreat is actually seen |
| Flash hit test | hunter within 7 u, `dot(camForward, toHunter) > 0.4`, LOS |
| Pools | BFS treats every cell whose centre is within 2.5 u of a lantern as blocked; hunter never enters |
| Unreachable target | if player is unreachable (inside pool) for 6 s → INVESTIGATE→WANDER far away |

State machine (tick every 0.2 s; movement every frame):

- **WANDER**: pick random reachable floor cell within 10 cells (BFS from self), walk at 1.8; on arrival
  idle 1–3 s, repeat. Any stimulus → CHASE.
- **INVESTIGATE(pos)**: path to pos at 3.0; on arrival wait 2 s (eyes sweep: slow yaw); → WANDER.
  Any stimulus → CHASE.
- **CHASE**: repath to player cell every 0.3 s, move at 4.3, `lastKnown = playerPos` on each stimulus.
  No stimulus 3 s → INVESTIGATE(lastKnown). Path fails (player in pool) → walk to nearest reachable
  cell to player and pace; 6 s unreachable → INVESTIGATE(own pos). Catch → player death.
- **STAGGERED**: no movement for 3 s, eyes dim, then WANDER (far target). Entered only by flash.

Pathing: BFS on the 40×40 grid (4-neighbour), blocked = solid ∪ pool cells. Follow path by moving
toward next cell centre; pop when within 0.1 u. Escape recipe the player should discover: plant a
lantern, step in, douse the lamp, wait 6 s.

## 7. Items, extraction, hub progression

- Items: oil flask = 0.3 amber emissive box (0xffa030, emissive 0.5), relic = 0.35 cyan octahedron
  (0x60e0ff, emissive 0.6), rich relic = 0.5 violet octahedron (0xc070ff, emissive 0.9). Bob + slow spin.
  Emissive makes them glint in total darkness (guidance-by-light, DS1 blue-orb lesson).
- Pickup: E within 1.6 u of nearest item in front (dot > 0). Goes to `carried = {oil, relic, rich}`.
- Extraction: E within 1.6 u of zone `S` → bank: `points += oil·1 + relic·3 + rich·5`, `bankedOil += oil·25`
  (display only), fade to black 0.6 s, spawn in hub, toast "Banked: 2 flasks, 1 relic (+5)". Carried cleared.
- Death: hunter catch → 1.2 s red vignette + "The dark took you." → **bundle** item (grey emissive sack)
  is placed at death position holding the lost carried loot (one bundle max; a new death replaces it).
  Respawn in hub; points untouched. Picking the bundle up returns its contents to carried.
- Descend: E at hub `S` → fade, spawn at zone `S`, oil = tier start value, items + hunter reset,
  lanterns cleared, bundle kept.

Flame tiers (hub PointLight 0xffa040 at F, y 1.2, decay 2, flicker as lamp):

| Tier | Points ≥ | Intensity | Distance | Message on reaching |
|---|---|---|---|---|
| 1 | 0 | 2.0 | 7 | (start) "Only embers remain." |
| 2 | 6 | 3.5 | 11 | "The flame stirs. The alcoves take shape." |
| 3 | 15 | 5.5 | 16 | "The flame grows. The vault is warm again." |
| 4 | 30 | 8.0 | 24 | "The Last Lantern burns bright." |

Flame mesh: stacked emissive boxes (0.6 → 0.2) whose scale = 0.6 + 0.4·(tier−1); emissive colour
warms from 0xff6020 (tier 1) to 0xffd080 (tier 4). Hub state (`points`, `tier`) persists in memory for
the session; also mirrored to `localStorage` key `undercroft-proto` (best-effort try/catch).

## 8. HUD & screens (DOM overlay, monospace, no libs)

- Top-left: oil bar (200×12, amber; red < 20) + numeric; lamp status `LAMP ON/OFF`; flash/lantern
  cooldown dots. Top-right: `Carried: 2 flask · 1 relic · 1 rich` and `Flame: tier 2 (9/15)`.
- Bottom-centre hint line, context-driven: "[E] Pick up oil flask", "[E] Bank loot", "[E] Descend",
  "Lamp guttering", "It has seen you" (on CHASE enter), "Safe — it will not enter the light" (in pool).
- Centre dot crosshair. Vignette that darkens as oil < 20.
- Title screen: name, one-paragraph premise, controls table, "Click to begin". Death screen: message,
  loot lost, "Click to return to the Lantern". Toast (bottom, 2.5 s) for bank and tier-up messages.
- Fade overlay (black div, opacity tween) for hub↔zone transitions.

## 9. Code layout (single module, target line counts)

1. CSS + overlay DOM (~120) 2. constants/tuning table (~60) 3. maps + parse to grids (~110)
4. world build: instanced blocks, items, stairs marker, flame, lights (~200) 5. player controller +
collision + lamp (~170) 6. flash / lantern / pools (~110) 7. hunter: BFS, LOS, FSM, mesh (~230)
8. items / bank / death / descend / tiers (~140) 9. HUD + screens (~130) 10. main loop + resize (~80).

Game states: `TITLE → HUB → ZONE → DEAD → HUB`. Frame: `dt = min(clock.getDelta(), 0.05)`.

## 10. Acceptance checklist

- [ ] Title → click → hub at tier 1: flame dim, alcoves invisible; E at stairs descends.
- [ ] Zone is black beyond lamp radius; items glint; deep pockets visibly shrink the lamp.
- [ ] Lamp on within 12 u + LOS ⇒ hunter chases; lamp off + walking ⇒ it passes within 3 u unaware.
- [ ] Sprinting with lamp off draws it. Q in front of it ⇒ 3 s stagger. R ⇒ it will not step in the ring.
- [ ] Oil hits 0 ⇒ lamp dies; T with a carried flask restores 25.
- [ ] Bank 6+ pts ⇒ tier 2 toast, hub visibly brighter, alcoves appear. Tier 4 at 30 pts.
- [ ] Death ⇒ loot bundle at death spot, hub points intact, bundle recoverable next run.
- [ ] Headless playwright load: no console errors, `window.__proto.state === 'TITLE'` exposed for tests.
