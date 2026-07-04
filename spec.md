# Game Spec — Working Title: "The Undercroft" (placeholder)

> Status: DRAFT — brainstorming in progress (2026-07-04).
> Purpose: feed a research assistant to evaluate competing/similar games.

## One-line pitch

A single-player first-person voxel horror-extraction game where you delve into
pitch-black handcrafted ruins from a slowly-reviving settlement hub. You cannot
kill the dark's inhabitants — you evade them or burn precious light to repel
them. Light is your health, weapon, timer, and currency in one.

## Pillars

1. **The hub is the heart.** A safe, warm, continuously-improving settlement
   (Firelink Shrine / Dirtmouth feeling). Every expedition exists to make home
   better and brighter. Hub growth is the primary long-term motivator.
2. **Light is one economy.** Torches/oil deplete (soft expedition timer),
   light repels/staggers creatures (weapon), planted lanterns claim territory
   (safety), and light reveals what darkness hides. Spending light for one use
   starves the others.
3. **Dread over combat.** Dark Souls 1 Catacombs / Tomb of the Giants
   atmosphere: oppressive underground ruin, psychological pressure of true
   darkness, relief of reaching light as an emotional payoff.
4. **Bank it or push deeper.** Extraction tension — die and you lose what you
   carry, but hub progress and knowledge persist.

## Core decisions (locked so far)

| Area | Decision |
|---|---|
| Genre frame | Single-player adventure; stealth/evasion + extraction/gathering |
| World structure | Discrete **handcrafted zones** launched as expeditions from the hub; zones revisited with new tools/objectives |
| Core activities | Extraction/gathering + stealth/evasion (combat is NOT a core verb) |
| Light mechanics | Depleting resource (torch/oil soft-timer) + safety/territory (planted lanterns create safe ground) |
| Combat model | **Light IS the weapon** — flash to stagger, burning oil to repel/kill; spends the survival resource. No conventional weapons |
| Failure stakes | Lose carried loot; keep hub progress, unlocked shortcuts, and knowledge |
| Hub growth | **Both**: rescued NPCs unlock what CAN be built; hauled resources decide WHEN it's built |
| Perspective | **First-person** (max dread; Amnesia-style) |
| Sensing model | **Carried light attracts, planted light repels.** Moving light is a beacon that draws hunters; anchored lanterns/braziers create ground creatures won't enter. Stealth = dousing your handlamp and darting between pools of safety you create |
| Hub identity | **The Last Lantern** — an underground vault built around the last surviving great flame. Rebuilding = the flame growing, pushing the wall of dark back, revealing more rooms/structures to restore. Hub brightness IS the visible progress meter |
| Progression gates | All four, each with a distinct job: **light-tech tiers** = primary depth gate (deeper darkness eats weak light); **hub construction** = zone access (tram/elevator/causeway open new zones); **tools** = metroidvania keys for revisiting old zones' locked depths; **NPC knowledge** = soft gating (routes, rituals) that makes rescues mechanically valuable |
| Art style | 3D pixel art / **voxel**, lighting and ambiance as the primary vibe-creator |
| Atmosphere ref | Dark Souls 1 — Catacombs / Tomb of the Giants: dark underground ruin, light-vs-dark |
| Scope | Tight indie: 10–15 h campaign, 20–40 min expeditions, zones revisited 2–3× |
| Horror intensity | **Full horror with active hunts** — creatures stalk and chase; being hunted in the dark is a core experience (Amnesia / Alien: Isolation register) |
| Expedition structure | **NPC contracts + freeform**: rescued NPCs post requests layered over free scavenging in a chosen zone |
| Endgame | **Descend to the source + final choice**: reaching the bottom presents a choice that determines the hub's fate; multiple endings keyed to how much you rebuilt and who you rescued |
| Platform | **PC only** (Steam; mouse-look and voxel lighting assumptions stay simple) |

## Design notes / rationale

- **DS1 light lessons** (from Catacombs/Tomb of the Giants research):
  light costs you something moment-to-moment (Skull Lantern occupies a hand);
  light reveals hidden guidance (blue orbs to Nito); darkness→light transitions
  are emotional payoffs (Ash Lake vista).
- **Single-resource spine:** oil/light doubles as HP, ammo, timer, currency.
  Elegant but needs careful balancing — starving the player of light too hard
  kills exploration joy; too generous kills dread.
- **Return-pressure** comes naturally from light depletion (no artificial
  timers needed).
- **Tension to watch — full horror × lose-loot stakes:** active hunts plus
  losing your haul on death can compound into rage-quit territory. Candidate
  mitigations (undecided): field stashes that survive death near planted
  lanterns; hunts that are reliably escapable through skilled light play;
  partial-loot insurance via an NPC service. At least one mitigation should
  ship.
- **All-four progression gates** are acceptable in a tight scope only because
  each has a distinct, non-overlapping job (depth / access / revisits / soft
  guidance). Guard against double-locking the same door.

## Open questions (to resolve)

- [ ] Narrative frame: who are you, what is this place, why did it go dark?
      (**Deliberately deferred** — mechanics first; the "Last Lantern" hub and
      light-vs-dark economy already imply strong theming.)
- [ ] Creature design: roster, senses beyond light-attraction, behaviors.
- [ ] Hub systems detail: which services/upgrades exist per NPC/building.
- [ ] Expedition prep: loadout choices, oil economy numbers.
- [ ] Death-penalty mitigation choice (see tension note above).
- [ ] Engine choice (voxel lighting tech is non-trivial in first person).
- [ ] Oil/light economy numbers; creature roster; hub building list.
- [ ] Title.

## Comparables (for research assistant)

| Game | Overlap | Key difference |
|---|---|---|
| Lethal Company | FP extraction horror, hub-ship, quota loop | Co-op-centric; ours is single-player, handcrafted zones |
| Darkest Dungeon | Torch/light meter, hub (Hamlet) rebuilding, expedition loop | Turn-based party RPG |
| Sunless Sea | Fuel/light economy, port-as-hub, terror of the dark | Top-down naval trading/roguelite |
| Darkwood | Scavenging in hostile dark, hideout, light safety | Top-down, survival crafting, procedural |
| Subnautica | Depth-pressure exploration, resource extraction, base-building, resource-as-lifeline (O2) | Open-world survival crafting |
| Hollow Knight | Reviving hub (Dirtmouth), rescued NPCs, underground ruin melancholy | 2D metroidvania, combat-focused |
| Amnesia: The Dark Descent | FP horror, no combat, light/tinderbox resource, sanity in darkness | Linear narrative horror, no hub/extraction |
| Alien: Isolation | FP being-hunted-as-core-experience, stalker AI | Sci-fi, linear, no hub/economy |
| Dark Souls 1 | Atmosphere ref (Catacombs/ToG), hub feeling (Firelink), corpse-run stakes | Combat-centric ARPG |
