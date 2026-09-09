# docs/history

How the port was run, kept for the record. Nothing here is a current instruction; the README at the
workspace root is.

- `HANDOFF_PHASE2.md` — the handoff as it stood when Phase 2 (the Bevy shell) was planned. Code
  comments citing "HANDOFF §6" / "HANDOFF §8" refer to this file.
- `PHASE2_SKELETON.md` — the contract for the skeleton step (states, resources, messages, debug
  queue, fixed tick, headless harness). Its §0 rules about file ownership and not editing the RON
  are superseded (note at the top); the conventions about resources and the harness still describe
  the code.
- `PHASE2_LANES.md` — the contract for the five parallel plugin lanes (world, creatures, ui, hub,
  audio): what each owned, the resource-ownership rules, the Xvfb screenshot recipe.

The Three.js prototype the port was verified against, and the tools that extracted its data, are
under `../../../reference/`.
