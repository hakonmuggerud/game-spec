//! LANE: world. Box collision against the grid — `world.js:boxBlocked`, `world.js:moveWithCollision`,
//! `world.js:markBoxCells` and the hub `blockMask` (`config.js:HUB_BLOCK` bits, `hub.js` footprints).
//!
//! Must contain: the block-mask type (one byte per cell, `HUB_BLOCK.PROP|BUILDING|NPC|FLAME` bits) with
//! `mark_box_cells` (a world-space box → the cells it blocks, honouring `minTop` / `maxBottom` / `shrink` /
//! `minSize`), `box_blocked(map, mask, x, z, radius) -> bool` (any of the ≤ 4 overlapped cells solid or masked;
//! off-grid = solid, DESIGN.md §3) and `move_with_collision(map, mask, x, z, dx, dz, radius) -> (x, z)` (move X
//! then Z, cancel an axis that would collide — read `world.js` for the exact sliding rule). Pure functions; the
//! player and follower radii come from `Config`. Nothing else.
