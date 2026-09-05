# The Undercroft — Prototype

A vertical-slice prototype of [`../spec.md`](../spec.md): one hub, one handcrafted zone, one
hunter, and the full light economy, in a single self-contained `index.html` (Three.js 0.160,
plain JS, no build step). Design notes and all numbers live in [`DESIGN.md`](DESIGN.md).

## Run

```sh
python3 -m http.server 8765 --directory prototype   # from the repo root
# then open http://localhost:8765/index.html
```

Needs internet: Three.js is loaded from the jsDelivr CDN via an importmap. Opening the file
directly (`file://`) will not work because ES modules require HTTP. Any modern desktop browser
with WebGL and pointer lock. Click the title screen to lock the mouse and begin.

## Controls

| Key | Action |
|---|---|
| Mouse / Arrow keys | Look |
| W A S D | Move (walk) |
| Shift | Sprint (faster, but noise draws the hunter) |
| F | Toggle handlamp (off = stealth, burns no oil) |
| Q | Flash: spend 15 oil to stagger the hunter in front of you |
| R | Plant lantern: spend 20 oil to create a safe pool at your feet |
| E | Interact: pick up item / bank at zone stairs / descend at hub stairs |
| T | Pour a carried oil flask into the lamp (+25 oil) |
| Esc | Release pointer (pause); click to resume |

## The loop

1. **Hub** ("The Last Lantern"): safe, warm, a small great flame. Walk to the stairs, press E.
2. **Descend** into the Undercroft with a lamp holding 50–80 oil depending on flame tier.
3. **Loot**: oil flasks (1 pt), relics (3), rich relics (5) glint in the dark. The hunter is drawn
   to your lit lamp and to sprinting; douse the lamp, flash it, or hide in a planted pool.
4. **Bank**: return to the stairs and press E. Carried loot becomes flame points.
5. **Flame grows**: at 6 / 15 / 30 points the hub flame steps up a tier, lighting alcoves that were
   pitch black. Die and you drop your carried loot as a retrievable bundle; points persist
   (also mirrored to `localStorage`).

## Spec pillars in the slice

| Pillar | Representation |
|---|---|
| The hub is the heart | Flame tiers brighten the hub and reveal alcoves; brightness is the progress meter |
| Light is one economy | One oil pool feeds the lamp timer, flash (weapon), lantern (territory), and a flask is either +1 point or +25 oil, never both |
| Dread over combat | Pitch-black ruin, fog, low-res pixelated render, near-invisible hunter with glowing eyes; no way to kill it |
| Bank it or push deeper | Loot only counts when banked at the stairs; death drops it as a bundle at the death spot |

## Known gaps vs the spec

- One zone, one creature type, no NPCs, contracts, tools, shortcuts, or light-tech tiers.
- Hub "construction" is only the flame's light; no buildings or services.
- No narrative, endgame, or multiple endings. No audio.
- Oil/flame numbers are placeholders, not balanced; expeditions run a few minutes, not 20–40.
- Blocky instanced meshes rather than true voxels; no save beyond points in `localStorage`.

## Tuning knobs

Everything numeric is in the **"2. Constants / tuning table"** block near the top of the module
script in `index.html`: `CFG` (movement, oil, lamp, flash, lantern, interaction), `H` (hunter
speeds, sense radii, timers), `TIERS` (flame thresholds and light), `POINTS` (loot values),
`SCONCE_INT` (hub alcove lighting per tier). Maps are ASCII in `ZONE_ROWS` and `HUB_ROWS` just
below; the legend is in `DESIGN.md` §3. `window.__proto` exposes game state for headless tests.
