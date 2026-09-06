# The Undercroft — Prototype

A playable slice of [`../spec.md`](../spec.md): the hub, four handcrafted zones, hunters, NPC rescues,
contracts, hub construction and three endings, all on one light economy. `index.html` (CSS + overlay DOM +
importmap) loads plain ES modules from `src/` (Three.js 0.160, no build step). Every number and every
map lives in [`DESIGN.md`](DESIGN.md). `models.html` is a viewer for the voxel models in `src/models.js`.

## Run

```sh
python3 -m http.server 8765 --directory prototype   # from the repo root
# then open http://localhost:8765/index.html
```

Needs internet (Three.js comes from the jsDelivr CDN) and a desktop browser with WebGL and pointer lock;
`file://` will not work (ES modules). Click the title screen to lock the mouse and begin. Sound starts on
the first click/key (browser gesture rule). Progress saves to `localStorage` (`undercroft-v2`).

## Controls

The main menu (Continue · New Game · Controls · Sound) and the pause menu (Esc) are navigated with the mouse or
with **↑ ↓ / W S + Enter** (number keys pick a line directly; Esc backs out of a sub-panel). Choosing Continue or
New Game locks the pointer and starts the game; New Game asks for confirmation when a save exists.

| Key | Action |
|---|---|
| Mouse / Arrow keys | Look |
| W A S D | Move · **Shift** sprint (faster, but noise draws hunters) |
| F | Toggle handlamp (off = stealth, burns no oil) |
| Q | Flash: spend oil (15, less with light-tech) to stagger the hunter in front of you |
| R | Plant lantern: spend oil (20) to create a safe pool hunters will not enter |
| E | Interact: pick up / free NPC / open gate / bank at stairs or cage / talk / build / descend · confirm in menus |
| T | Pour a carried oil flask into the lamp (+25 oil) |
| M · [ · ] | Mute · volume down / up (also in the Sound panel of either menu) |
| Tab | Minimap (needs the Cartographer's Table) |
| 1 / 2 / 3 / 4 | Pick a line in any menu: contract offer, board zone, service, altar ending, menu entries |
| Enter / Space | Confirm in menus; activate the highlighted main/pause menu entry |
| Esc | **Pause menu** in the hub or a zone (Resume · Controls · Sound · Return to main menu · Clear save); closes a hub/NPC menu or backs out of a sub-panel; Esc again resumes. Losing the pointer lock (alt-tab) also pauses |
| Backspace ×2 | On the main menu: wipe the save (hidden shortcut; the visible route is New Game → confirm, or the pause menu's Clear save) |

Returning to the main menu from a zone abandons the run: carried loot is lost (never banked), the hub and the save
are untouched. Clearing the save resets everything in place (flame tier, buildings, rescued NPCs, contracts, tools,
endings, minimap) without a reload; sound settings are kept.

## The loop

1. **Hub** ("The Last Lantern"): safe, warm, one great flame. Read the Departure Board, talk to rescued
   NPCs, build, then take the stairs / tram / elevator (all go to the zone chosen on the board).
2. **Descend** with 50–80 oil by flame tier (+15 per Oil Press reservoir level).
3. **Loot** oil flasks (1 pt), relics (3), rich relics (5). Hunters are drawn to a lit lamp with line of
   sight (12 u), to sprinting (9 u) and to wading (7 u). Douse, flash, or hide in a planted pool.
4. **Bank** at the stairs / cage. Loot becomes flame points **and** resources (25 oil per flask, relics,
   rich relics) to spend at the hub. A following NPC within 4 u is rescued.
5. **Flame grows** at 6 / 15 / 30 points (tiers 2–4), lighting hub alcoves and unlocking the tram (tier 2)
   and elevator (tier 3). Die and carried loot drops as a retrievable bundle; everything else persists.

| Zone | Opens with | Hunters | Notes |
|---|---|---|---|
| The Undercroft | start | 1 base | Wick captive; Deacon Maud behind a Pry Bar gate |
| The Cistern | Tram dock (120 oil, tier 2) | 2 base | water slows you (×0.55) and is heard 7 u away; Sluice Key gate |
| The Ossuary | Elevator (250 oil + 4 relics, tier 3) + Light-tech II | 1 fast | oil burns ×1.3, lamp ×0.85; Censer gate |
| The Source | flame tier 4 + Deacon Maud rescued | 2 fast + 1 more at laps 3 and 4 | spiral, darker per lap, no banking; the altar |

## NPCs, contracts, services

Free a captive (E), keep them close (they are lit within 3 u of your lamp and can be caught, returning to
their cell), and bank with them nearby. Each rescued NPC stands by a hub anchor, unlocks a ghost building,
and posts contracts in order (max 2 active; run-scoped progress resets on death):

| NPC | Held in | Unlocks (cost) | Service | Contracts → reward |
|---|---|---|---|---|
| Wick the Lamplighter | Undercroft | Workshop (6 relics) | Light-tech I/II/III: lamp reach ×1.15/1.3/1.45, burn ×0.9/0.8/0.7, cheaper flash/lantern (6 / 10 / 14+2 rich relics) | plant a lantern in the great hall → Pry Bar + 4 pts; bank 4 flasks in one run → 60 oil |
| Ines the Cartographer | Cistern | Cartographer's Table (100 oil) | Tab minimap of explored cells, items seen, spots, exit | stand 30 s in the drowned hall → Sluice Key; recover the lost chart → 8 pts |
| Oren the Oil-press Keeper | Ossuary | Oil Press (80 oil) | 1 relic → 30 oil; reservoir +15 start oil ×2 (5, then 8 relics) | bank 2 rich relics in one run → Censer; recover the ledger → 80 oil |
| Deacon Maud | Undercroft (gated) | Shrine (150 oil); opens the Source | Blessing, 40 oil per descent: on death half of each carried kind is banked anyway | 45 s vigil, lamp off, in the Ossuary → 10 pts; bank 3 relics in one run → 6 pts |

Tools open the `X` gates on revisits (Pry Bar → Undercroft NW crypt, Sluice Key → Cistern flooded vault,
Censer → Ossuary reliquary), each hiding rich relics; the Departure Board (built from the start) lists zones,
lock reasons and active contract targets.

## Endings

E at the Source altar: **Feed the great flame** ("A Brighter Cage", always), **Kindle a new flame** ("The
Lantern Eternal", needs flame tier 4 and 3+ rescued) or **Free the dark** ("The Long Night", always; the hub
flame shows as embers for that visit). Text varies by tier and who you rescued; the end screen shows run
stats, `save.endings` records each, and the Source stays open for replays.

## Spec pillars

| Pillar | Representation |
|---|---|
| The hub is the heart | Flame tiers light the vault; rescued NPCs raise buildings paid with hauled resources |
| Light is one economy | One oil pool feeds the lamp timer, flash, lanterns and the blessing; a flask is +1 pt / +25 oil / +25 banked, never all |
| Dread over combat | Pitch-black voxel ruin, low-res render, procedural drone and hunter growl; hunters cannot be killed |
| Bank it or push deeper | Loot counts only when banked; death drops a bundle; the Source cannot bank at all |
| Progression gates | Light-tech (Ossuary), construction (tram/elevator), tools (gates), NPC knowledge (Deacon → Source) |

## Known gaps vs the spec

- One hunter type in two speed profiles; no burning oil to repel/kill, no shortcuts inside zones.
- Expeditions run minutes, not 20–40; numbers are placeholders, not balanced; no difficulty curve.
- Dialogue is one line per NPC; no narrative beyond zone intros, lap lines and the ending texts.
- Blocky instanced meshes rather than true voxels; no sound assets (all WebAudio synthesis).

## Files (`src/`)

`config.js` numbers and key bindings · `maps.js` ASCII maps, zone metadata, parser, grid helpers (BFS, LOS) ·
`models.js` voxel model factories · `world.js` instanced geometry, items, lanterns, gates, water, collision ·
`hunter.js` senses, FSM, pathing · `npc.js` captives, follower AI, hub residents, dialogue · `contracts.js`
contract state machine and HUD lines · `hub.js` flame tiers, buildings, services, board, minimap, blessing ·
`endgame.js` Source laps, altar, endings · `audio.js` procedural sound · `save.js` persistence · `ui.js` DOM
HUD/menus · `main.js` loop, ctx, event bus, input, player. `window.__game` exposes `ctx` and `actions`
(`loadZone`, `setPoints`, `giveTool`, `build`, `choose`, `reset`, `openMainMenu`, `openPause`, `closePause`, `newGame`,
`clearSave`, `begin`, …) for headless tests.
