//! LANE: economy. Captive NPCs and the follower AI — `npc.js` (DESIGN-v2 §3), minus the hub-resident meshes.
//!
//! Must contain: the NPC record (`npc.js:record` — state `IDLE` / `FOLLOW` / `CAUGHT` / `RESCUED`, position,
//! path, `lit`, `inPool`), `free(id)`, the follower tick (`NPC_CFG.tick` repath through `grid::bfs_field` /
//! `grid::path_to`, `speed` / `fastSpeed` / `stopDist` / `teleportDist`, wall sliding with `NPC_CFG.radius`),
//! `stimulus()` (what the hunters sense: `{id, x, z, moving, lit, inPool}` → `PlayerView::follower`), `caught`
//! (`npcCaught` + `npcLost`, the `caughtT` sink, return to the cell), the bank check (`saveR` → `npcRescued`),
//! and the hub stand spots (anchor + 1 x, `HUB_FALLBACK`). Returns `Vec<SimEvent>`. Nothing else.
