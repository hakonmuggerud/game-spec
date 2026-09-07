//! LANE: world. Lantern pools — the `map.pool` bitmap of the JS (`hunter.js:recomputePools`) and the planted
//! lantern record the sim keeps (`world.js:spawnLantern` minus the meshes).
//!
//! A planted lantern casts a pool of radius `CFG.poolR` (2.5 u): every cell whose *centre* lies within that
//! distance of the lantern is marked `1`. Hunters treat pool cells as blocked (`maps.js:isBlocked`), the Brute
//! wades in at `CREATURE.brute.poolMul` speed and smashes the lantern (`hunter.js:bruteNearLantern`), and the
//! player / follower "in pool" flags (`main.js:updatePlayer`, `npc.js:stimulus`) use the plain distance test,
//! not the bitmap. Everything here is pure: the bitmap is recomputed from the lantern list on `lantern` /
//! `lanternRemoved` / `zoneEnter` (`hunter.js` lines 30–34), never patched incrementally.

use crate::grid::{center, dist2d, idx, in_bounds, to_cell};
use undercroft_data::ParsedMap;

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

    /// `hunter.js:recomputePools` in place: clear the bitmap (resizing it to the map if needed) and mark every
    /// cell within `pool_r` of a lantern. See [`recompute`].
    pub fn recompute(&mut self, m: &ParsedMap, lanterns: &[Lantern], pool_r: f32) {
        let n = m.len();
        if self.mask.len() != n {
            self.mask = vec![0; n];
        } else {
            self.mask.fill(0);
        }
        for l in lanterns {
            let (lcx, lcz) = to_cell(m, l.x, l.z);
            // the JS scans a fixed 7×7 window (dz, dx in −3..=3) around the lantern's cell; poolR is 2.5 so the
            // window is never the limiting factor, but keep it for exact parity with a modded radius
            for dz in -3..=3 {
                for dx in -3..=3 {
                    let (cx, cz) = (lcx + dx, lcz + dz);
                    if !in_bounds(m, cx, cz) {
                        continue;
                    }
                    let (px, pz) = center(m, cx, cz);
                    if dist2d(px, pz, l.x, l.z) <= pool_r {
                        self.mask[idx(m, cx, cz)] = 1;
                    }
                }
            }
        }
    }

    /// Number of pool cells (debug / tests).
    pub fn count(&self) -> usize {
        self.mask.iter().filter(|&&b| b == 1).count()
    }
}

/// A planted lantern (`world.js:spawnLantern` record `{x, z, …}` without the meshes). `planted` is the sim
/// time it was planted (`state.time`), so the lantern list stays in plant order — `main.js:plantLantern`
/// removes `ctx.lanterns[0]`, the oldest, when `CFG.lanternMax` is reached, and the Brute smashes the first
/// lantern in list order within reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lantern {
    /// World position (the player's position when planted).
    pub x: f32,
    pub z: f32,
    /// Sim time at planting.
    pub planted: f32,
}

impl Lantern {
    /// A lantern planted at `(x, z)` at time `planted`.
    pub fn new(x: f32, z: f32, planted: f32) -> Lantern {
        Lantern { x, z, planted }
    }
}

/// `hunter.js:recomputePools` — the pool bitmap for a map and its planted lanterns: every cell whose centre
/// is within `pool_r` (`CFG.poolR`) of some lantern is 1. The JS scans a 7×7 cell window around each
/// lantern's cell, so a radius above 3.5 would be clipped exactly as in the prototype.
pub fn recompute(m: &ParsedMap, lanterns: &[Lantern], pool_r: f32) -> Pool {
    let mut p = Pool::empty(m.len());
    p.recompute(m, lanterns, pool_r);
    p
}

/// `main.js:updatePlayer` `player.inPool` / `npc.js:stimulus` `inPool`: is the world point within `pool_r`
/// of any planted lantern? (A distance test on the lantern list, not the bitmap — a point can be in a pool
/// while its cell centre is not, and vice versa.)
pub fn in_pool(lanterns: &[Lantern], x: f32, z: f32, pool_r: f32) -> bool {
    lanterns.iter().any(|l| dist2d(l.x, l.z, x, z) <= pool_r)
}

/// The first lantern in plant order within `r` of a point — what `hunter.js:bruteNearLantern` smashes
/// (it walks `ctx.lanterns` in order and takes the first within `CREATURE.brute.smashR`, one per tick).
/// Returns the index into `lanterns`.
pub fn first_within(lanterns: &[Lantern], x: f32, z: f32, r: f32) -> Option<usize> {
    lanterns.iter().position(|l| dist2d(l.x, l.z, x, z) <= r)
}

/// The nearest lantern within `r` of a point (index and distance), for callers that want the closest one
/// rather than the JS's first-in-list. Ties keep the earlier lantern.
pub fn nearest_within(lanterns: &[Lantern], x: f32, z: f32, r: f32) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for (i, l) in lanterns.iter().enumerate() {
        let d = dist2d(l.x, l.z, x, z);
        if d <= r && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{bfs_blocked, bfs_solid, is_blocked};
    use undercroft_data::map::parse_map;

    fn open_map(w: usize, h: usize) -> ParsedMap {
        let mut rows = vec!["#".repeat(w)];
        for _ in 1..h - 1 {
            rows.push(format!("#{}#", ".".repeat(w - 2)));
        }
        rows.push("#".repeat(w));
        parse_map(&rows, 0, "t", None, None).expect("parse")
    }

    #[test]
    fn pool_marks_cells_within_radius_of_centre() {
        let m = open_map(12, 12);
        // lantern at the centre of cell (5,5): (5.5, 5.5)
        let p = recompute(&m, &[Lantern::new(5.5, 5.5, 0.0)], 2.5);
        // centre-to-centre distances: 0,1,2 on axes (<=2.5), sqrt2, sqrt5 (2.236) in, sqrt8 (2.83) out, 3 out
        assert!(p.is_pool(m.idx(5, 5)));
        assert!(p.is_pool(m.idx(7, 5)));
        assert!(p.is_pool(m.idx(6, 7)));
        assert!(!p.is_pool(m.idx(7, 7)));
        assert!(!p.is_pool(m.idx(8, 5)));
        // 1 + 4*2 (axes) + 4 (diagonal 1) + 8 (knight) = 21 cells
        assert_eq!(p.count(), 21);
    }

    #[test]
    fn pool_is_measured_from_the_lantern_not_its_cell() {
        let m = open_map(12, 12);
        // lantern at the east edge of cell (5,5): cell (8,5) centre is 2.55 away → out; (2,5) centre is 3.45 → out
        let p = recompute(&m, &[Lantern::new(5.95, 5.5, 0.0)], 2.5);
        assert!(!p.is_pool(m.idx(8, 5)));
        assert!(!p.is_pool(m.idx(2, 5)));
        assert!(p.is_pool(m.idx(3, 5))); // 2.45
        assert!(p.is_pool(m.idx(7, 5)));
    }

    #[test]
    fn pool_clips_at_the_grid_edge_and_marks_solids_too() {
        // the JS marks any in-bounds cell, walls included (they are solid anyway)
        let m = open_map(8, 8);
        let p = recompute(&m, &[Lantern::new(1.5, 1.5, 0.0)], 2.5);
        assert!(p.is_pool(m.idx(0, 1)));
        assert!(p.is_pool(m.idx(1, 0)));
        assert_eq!(p.mask.len(), 64);
        let (cx, cz) = (1, 3);
        assert!(is_blocked(&m, &p, cx, cz));
        assert!(!is_blocked(&m, &Pool::empty(64), cx, cz));
    }

    #[test]
    fn recompute_clears_previous_pools_and_hunters_route_around_them() {
        let m = open_map(14, 6);
        let mut p = Pool::empty(m.len());
        p.recompute(&m, &[Lantern::new(7.5, 2.5, 0.0)], 2.5);
        assert!(p.count() > 0);
        // a lantern wall across the corridor: BFS with pools must detour or fail, without pools it passes
        let solid = bfs_solid(&m, 1, 2);
        let blocked = bfs_blocked(&m, &p, 1, 2);
        assert!(solid.reachable(m.idx(12, 2)));
        assert!(
            blocked.dist[m.idx(12, 2)] < 0 || blocked.dist[m.idx(12, 2)] > solid.dist[m.idx(12, 2)]
        );
        p.recompute(&m, &[], 2.5);
        assert_eq!(p.count(), 0);
        assert_eq!(p, Pool::empty(m.len()));
    }

    #[test]
    fn in_pool_and_lantern_lookup() {
        let ls = [
            Lantern::new(2.0, 2.0, 1.0),
            Lantern::new(5.0, 2.0, 2.0),
            Lantern::new(4.2, 2.0, 3.0),
        ];
        assert!(in_pool(&ls, 4.0, 2.0, 2.5));
        assert!(!in_pool(&ls, 9.0, 9.0, 2.5));
        assert!(in_pool(&ls, 4.5, 2.0, 2.5));
        // brute at (4.6, 2): both lantern 1 (0.4) and 2 (0.4) are within 1.0; first in list wins
        assert_eq!(first_within(&ls, 4.6, 2.0, 1.0), Some(1));
        assert_eq!(first_within(&ls, 4.6, 2.0, 0.1), None);
        let (i, d) = nearest_within(&ls, 4.3, 2.0, 1.0).expect("one lantern near");
        assert_eq!(i, 2);
        assert!((d - 0.1).abs() < 1e-5);
        assert_eq!(nearest_within(&[], 0.0, 0.0, 5.0), None);
    }
}
