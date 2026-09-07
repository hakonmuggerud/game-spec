//! LANE: world. Lantern pools — the `map.pool` bitmap of the JS (`hunter.js:recomputePools`).
//!
//! Must contain: `recompute(map, lanterns, pool_r) -> Pool` porting `hunter.js:recomputePools` (every cell whose
//! centre lies within `CFG.poolR` of a planted lantern is marked 1 — check the exact predicate and the Brute's
//! `poolMul` exemption in `hunter.js` before porting) and the lantern record type it reads (`x, z` world
//! position, planted time). Nothing else. The `Pool` type itself is defined here by the contract step because
//! `grid::is_blocked` and `grid::bfs_blocked` already read it.

/// The lantern-pool bitmap: one byte per cell, 1 inside a planted lantern's pool (`map.pool` in `maps.js`).
/// Hunters treat pool cells as blocked (`maps.js:isBlocked`); the Brute wades in (`CREATURE.brute.poolMul`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pool {
    /// Row-major, same indexing as `ParsedMap::cells`.
    pub mask: Vec<u8>,
}

impl Pool {
    /// An empty pool for a map of `n` cells (no lanterns planted).
    pub fn empty(n: usize) -> Pool {
        Pool { mask: vec![0; n] }
    }

    /// `map.pool[idx] === 1`.
    pub fn is_pool(&self, idx: usize) -> bool {
        self.mask.get(idx).copied() == Some(1)
    }
}
