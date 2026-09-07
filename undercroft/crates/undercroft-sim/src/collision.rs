//! LANE: world. Box collision against the grid — `world.js:boxBlocked`, `world.js:moveWithCollision`,
//! `world.js:markBoxCells`, the hub `blockMask` (`config.js:HUB_BLOCK` bits, `hub.js:rebuildColliders` /
//! `hub.js:colliders`) and the movement speed rule of `main.js:updatePlayer`.
//!
//! Collision (DESIGN.md §3): the mover is an axis-aligned square of half-size `CFG.radius` (0.3 for the player,
//! `NPC_CFG.radius` for the follower). A position is blocked when any of the ≤ 4 cells under its corners is
//! solid (`maps.js:isSolid`: wall, pillar, closed gate, barred shortcut) or carries a hub block-mask bit; off-grid
//! is solid because `cellType` returns `Wall` outside the grid. Movement is X then Z, and an axis step that
//! would collide is dropped while the other still applies (sliding along walls). No Y movement anywhere.
//!
//! Everything here is pure: the map is read, the block mask is an explicit value, positions go in and come out.

use crate::grid::{cell_type, idx, in_bounds, is_solid, to_cell};
use undercroft_data::config::{Cfg, HubBlock};
use undercroft_data::{CellKind, ParsedMap};

/* ============================================================
Block mask (hub props / buildings / NPCs / brazier)
============================================================ */

/// `map.blockMask` — one byte per cell of `HUB_BLOCK` bits (`PROP` 1, `BUILDING` 2, `NPC` 4, `FLAME` 8).
/// Any non-zero byte is solid for `box_blocked`. Zones have no mask (`None`); the hub builds one from its
/// prop footprints (`world.js:buildProps`) and `hub.js:rebuildColliders` refreshes the other bits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockMask {
    /// Row-major, same indexing as `ParsedMap::cells`.
    pub mask: Vec<u8>,
}

/// One blocked hub cell as `hub.js:colliders()` reports it: `{cx, cz, bits, kinds[]}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collider {
    pub cx: i32,
    pub cz: i32,
    pub bits: u8,
    /// `HUB_BLOCK` bit names present, in `PROP BUILDING NPC FLAME` order.
    pub kinds: Vec<&'static str>,
}

impl BlockMask {
    /// An all-clear mask for a map of `n` cells (`world.js:buildHub`: `new Uint8Array(map.w * map.h)`).
    pub fn empty(n: usize) -> BlockMask {
        BlockMask { mask: vec![0; n] }
    }

    /// An all-clear mask sized to a map.
    pub fn for_map(m: &ParsedMap) -> BlockMask {
        BlockMask::empty(m.len())
    }

    /// Is any bit set on the cell (`world.js:hubBlocked` without the bounds test)?
    pub fn is_blocked(&self, idx: usize) -> bool {
        self.mask.get(idx).copied().unwrap_or(0) != 0
    }

    /// The bits on a cell (0 outside the mask).
    pub fn bits(&self, idx: usize) -> u8 {
        self.mask.get(idx).copied().unwrap_or(0)
    }

    /// `hub.js:rebuildColliders` first step: keep only the `keep` bits (the JS keeps `HUB_BLOCK.PROP` and
    /// re-marks buildings, the flame and the NPCs).
    pub fn retain_bits(&mut self, keep: u8) {
        for b in &mut self.mask {
            *b &= keep;
        }
    }

    /// `hub.js:rebuildColliders` NPC step: set `bit` on the cell under a world point when it is in bounds and
    /// not solid. Returns whether a bit was set.
    pub fn mark_point(&mut self, m: &ParsedMap, x: f32, z: f32, bit: u8) -> bool {
        let (cx, cz) = to_cell(m, x, z);
        if in_bounds(m, cx, cz) && !is_solid(m, cx, cz) {
            let i = idx(m, cx, cz);
            if let Some(b) = self.mask.get_mut(i) {
                *b |= bit;
                return true;
            }
        }
        false
    }

    /// `world.js:markBoxCells(map, bb, bit)` — set `bit` on every cell the world-space box overlaps once shrunk
    /// by `HUB_BLOCK.shrink` per side (a box thinner than that marks its centre cell). Boxes whose top is below
    /// `minTop` (rugs, bedrolls) or whose bottom is above `maxBottom` (hanging things) are walkable, and so is a
    /// box thinner than `minSize` in *both* axes (a post, a leg, a candle). Solid cells are never marked.
    /// Returns the number of cells marked (the JS `n`, counting cells already carrying the bit too).
    pub fn mark_box_cells(&mut self, m: &ParsedMap, bb: &Aabb, bit: u8, hb: &HubBlock) -> usize {
        if bb.max[1] < hb.min_top || bb.min[1] > hb.max_bottom {
            return 0;
        }
        if bb.max[0] - bb.min[0] < hb.min_size && bb.max[2] - bb.min[2] < hb.min_size {
            return 0;
        }
        let sh = hb.shrink as f64;
        let (mut x0, mut x1) = (bb.min[0] as f64 + sh, bb.max[0] as f64 - sh);
        let (mut z0, mut z1) = (bb.min[2] as f64 + sh, bb.max[2] as f64 - sh);
        if x1 < x0 {
            x0 = (bb.min[0] as f64 + bb.max[0] as f64) / 2.0;
            x1 = x0;
        }
        if z1 < z0 {
            z0 = (bb.min[2] as f64 + bb.max[2] as f64) / 2.0;
            z1 = z0;
        }
        let ox = m.ox as f64;
        let mut n = 0;
        let cz0 = z0.floor() as i32;
        let cz1 = (z1 - 1e-6).floor() as i32;
        let cx0 = (x0 - ox).floor() as i32;
        let cx1 = (x1 - ox - 1e-6).floor() as i32;
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if !in_bounds(m, cx, cz) || is_solid(m, cx, cz) {
                    continue;
                }
                let i = idx(m, cx, cz);
                if let Some(b) = self.mask.get_mut(i) {
                    *b |= bit;
                    n += 1;
                }
            }
        }
        n
    }

    /// `hub.js:colliders()` — every blocked cell with its bits and bit names.
    pub fn colliders(&self, m: &ParsedMap, hb: &HubBlock) -> Vec<Collider> {
        self.mask
            .iter()
            .enumerate()
            .filter(|(_, &b)| b != 0)
            .map(|(i, &b)| {
                let (cx, cz) = m.cell_of(i);
                let mut kinds = Vec::new();
                for (name, bit) in [
                    ("PROP", hb.prop),
                    ("BUILDING", hb.building),
                    ("NPC", hb.npc),
                    ("FLAME", hb.flame),
                ] {
                    if bit != 0 && b & bit != 0 {
                        kinds.push(name);
                    }
                }
                Collider {
                    cx,
                    cz,
                    bits: b,
                    kinds,
                }
            })
            .collect()
    }
}

/// A world-space axis-aligned box (`THREE.Box3`): `min` / `max` as `[x, y, z]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    /// A box from its corners.
    pub fn new(min: [f32; 3], max: [f32; 3]) -> Aabb {
        Aabb { min, max }
    }

    /// `world.js:buildProps` `cellsOf`: the AABB of a placed prop box — centre `(x, z)`, bottom `y`, size
    /// `w × h × d`, yawed by `ry` about Y (`models.js:placeBoxes` output, `undercroft_data::models::BoxDef`
    /// after placement).
    pub fn of_yawed_box(x: f32, y: f32, z: f32, w: f32, h: f32, d: f32, ry: f32) -> Aabb {
        let (c, s) = ((ry as f64).cos().abs(), (ry as f64).sin().abs());
        let (w, d) = (w as f64, d as f64);
        let hw = ((w * c + d * s) / 2.0) as f32;
        let hd = ((w * s + d * c) / 2.0) as f32;
        Aabb {
            min: [x - hw, y, z - hd],
            max: [x + hw, y + h, z + hd],
        }
    }
}

/* ============================================================
Box collision
============================================================ */

/// `world.js:boxBlocked(m, x, z)` with the radius made explicit: is a square mover of half-size `radius` centred
/// at `(x, z)` overlapping a solid cell or a masked hub cell? The four corners `(±r, ±r)` are tested in the JS
/// order `(-r,-r) (r,-r) (-r,r) (r,r)`; off-grid corners read as wall. `mask` is `None` in zones.
pub fn box_blocked(m: &ParsedMap, mask: Option<&BlockMask>, x: f32, z: f32, radius: f32) -> bool {
    let r = radius;
    for (dx, dz) in [(-r, -r), (r, -r), (-r, r), (r, r)] {
        let (cx, cz) = to_cell(m, x + dx, z + dz);
        if is_solid(m, cx, cz) {
            return true;
        }
        if let Some(mk) = mask {
            if mk.is_blocked(idx(m, cx, cz)) {
                return true;
            }
        }
    }
    false
}

/// `world.js:moveWithCollision(p, dx, dz)` — move `(x, z)` by `dx` then by `dz`, dropping any axis step whose
/// destination is blocked ([`box_blocked`]). A zero step on an axis is skipped, exactly as the JS `if (dx)`.
/// Returns the new position; the caller writes it back to its transform.
pub fn move_with_collision(
    m: &ParsedMap,
    mask: Option<&BlockMask>,
    x: f32,
    z: f32,
    dx: f32,
    dz: f32,
    radius: f32,
) -> (f32, f32) {
    let (mut x, mut z) = (x, z);
    if dx != 0.0 {
        let nx = x + dx;
        if !box_blocked(m, mask, nx, z, radius) {
            x = nx;
        }
    }
    if dz != 0.0 {
        let nz = z + dz;
        if !box_blocked(m, mask, x, nz, radius) {
            z = nz;
        }
    }
    (x, z)
}

/// [`move_with_collision`] with the player's radius from `CFG.radius`.
pub fn move_player(
    m: &ParsedMap,
    mask: Option<&BlockMask>,
    cfg: &Cfg,
    x: f32,
    z: f32,
    dx: f32,
    dz: f32,
) -> (f32, f32) {
    move_with_collision(m, mask, x, z, dx, dz, cfg.radius)
}

/* ============================================================
Speed
============================================================ */

/// `main.js:updatePlayer` speed rule: the multiplier applied to `CFG.walk` / `CFG.sprint` for the cell the
/// player stands on — water slows to `waterWalkMul` (0.55) or `waterSprintMul` (0.6); every other cell,
/// deep pockets included, moves at full speed (deep only changes burn and lamp reach, DESIGN.md §3.4).
/// The JS reads `player.inWater` set from the previous frame's cell; feed the current cell kind here.
pub fn speed_multiplier(cfg: &Cfg, cell: CellKind, sprinting: bool) -> f32 {
    if cell == CellKind::Water {
        if sprinting {
            cfg.water_sprint_mul
        } else {
            cfg.water_walk_mul
        }
    } else {
        1.0
    }
}

/// `main.js:updatePlayer` `speed`: `CFG.sprint` or `CFG.walk` times [`speed_multiplier`] for the cell.
pub fn player_speed(cfg: &Cfg, cell: CellKind, sprinting: bool) -> f32 {
    let base = if sprinting { cfg.sprint } else { cfg.walk };
    base * speed_multiplier(cfg, cell, sprinting)
}

/// [`player_speed`] for the cell under a world position (`toCell` + `cellType`, off-grid = wall = full speed).
pub fn player_speed_at(m: &ParsedMap, cfg: &Cfg, x: f32, z: f32, sprinting: bool) -> f32 {
    let (cx, cz) = to_cell(m, x, z);
    player_speed(cfg, cell_type(m, cx, cz), sprinting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::map::parse_map;
    use undercroft_data::GameData;

    fn map(rows: &[&str], ox: i32) -> ParsedMap {
        let rows: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
        parse_map(&rows, ox, "t", None, None).expect("parse")
    }

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    #[test]
    fn corners_hit_solid_cells_and_the_grid_edge() {
        let m = map(&["#####", "#...#", "#.P.#", "#...#", "#####"], 0);
        let r = 0.3;
        assert!(!box_blocked(&m, None, 1.5, 1.5, r));
        // touching the pillar cell (2,2) with the (r, r) corner
        assert!(box_blocked(&m, None, 1.75, 1.75, r));
        assert!(!box_blocked(&m, None, 1.65, 1.65, r));
        // wall
        assert!(box_blocked(&m, None, 1.2, 1.5, r));
        // off-grid
        assert!(box_blocked(&m, None, -5.0, 1.5, r));
        assert!(box_blocked(&m, None, 1.5, 40.0, r));
        // closed gate and barred shortcut are solid, water and deep are not
        let m2 = map(&["######", "#.X=W#", "#D...#", "######"], 0);
        assert!(box_blocked(&m2, None, 2.5, 1.5, r));
        assert!(box_blocked(&m2, None, 3.5, 1.5, r));
        assert!(!box_blocked(&m2, None, 4.5, 1.5, r));
        assert!(!box_blocked(&m2, None, 1.5, 2.5, r));
    }

    #[test]
    fn hub_offset_is_honoured() {
        let m = map(&["#####", "#...#", "#####"], 60);
        assert!(!box_blocked(&m, None, 62.5, 1.5, 0.3));
        assert!(box_blocked(&m, None, 2.5, 1.5, 0.3)); // zone coordinates fall off the hub grid
        assert!(box_blocked(&m, None, 60.6, 1.5, 0.3));
    }

    #[test]
    fn move_slides_along_walls_axis_by_axis() {
        let m = map(&["#######", "#.....#", "#.....#", "#######"], 0);
        let r = 0.3;
        // walk into the north wall diagonally: x advances, z is cancelled
        let (x, z) = move_with_collision(&m, None, 3.0, 1.35, 0.2, -0.2, r);
        assert!((x - 3.2).abs() < 1e-6);
        assert!((z - 1.35).abs() < 1e-6);
        // east wall: x is cancelled, z still moves
        let (x, z) = move_with_collision(&m, None, 5.65, 1.5, 0.2, 0.3, r);
        assert!((x - 5.65).abs() < 1e-6);
        assert!((z - 1.8).abs() < 1e-6);
        // free move
        assert_eq!(
            move_with_collision(&m, None, 2.0, 2.0, 0.5, -0.4, r),
            (2.5, 1.6)
        );
        // zero step is a no-op
        assert_eq!(
            move_with_collision(&m, None, 2.0, 2.0, 0.0, 0.0, r),
            (2.0, 2.0)
        );
        // player helper uses CFG.radius = 0.3
        let d = data();
        assert_eq!(d.config.cfg.radius, 0.3);
        assert_eq!(
            move_player(&m, None, &d.config.cfg, 2.0, 2.0, 0.5, 0.0),
            (2.5, 2.0)
        );
    }

    #[test]
    fn x_then_z_order_matters_at_a_corner() {
        // moving diagonally past a pillar's corner: the X step is applied first and may then let the Z step
        // through (or block it) — pin the JS order
        let m = map(&["######", "#....#", "#.P..#", "#....#", "######"], 0);
        let r = 0.3;
        // from (1.5, 1.5) step (+0.4, +0.4): X to 1.9 (corner at 2.2 → cell 2, row 1 open → ok);
        // then Z to 1.9: corner (2.2, 2.2) → pillar → cancelled
        assert_eq!(
            move_with_collision(&m, None, 1.5, 1.5, 0.4, 0.4, r),
            (1.9, 1.5)
        );
    }

    #[test]
    fn mask_blocks_the_player_only_where_set() {
        let m = map(&["#####", "#...#", "#...#", "#####"], 60);
        let mut mk = BlockMask::for_map(&m);
        let d = data();
        let hb = &d.config.hub_block;
        assert!(!box_blocked(&m, Some(&mk), 62.5, 1.5, 0.3));
        assert!(mk.mark_point(&m, 62.5, 1.5, hb.npc));
        assert!(!mk.mark_point(&m, 60.5, 0.5, hb.npc)); // wall
        assert!(box_blocked(&m, Some(&mk), 62.5, 1.5, 0.3));
        assert!(box_blocked(&m, Some(&mk), 61.75, 1.5, 0.3)); // corner reaches into the cell
        assert!(!box_blocked(&m, Some(&mk), 61.6, 1.5, 0.3));
        assert!(!box_blocked(&m, None, 62.5, 1.5, 0.3));
        let cols = mk.colliders(&m, hb);
        assert_eq!(cols.len(), 1);
        assert_eq!(
            cols[0],
            Collider {
                cx: 2,
                cz: 1,
                bits: 4,
                kinds: vec!["NPC"]
            }
        );
        mk.retain_bits(hb.prop);
        assert!(mk.colliders(&m, hb).is_empty());
    }

    #[test]
    fn mark_box_cells_honours_the_thresholds() {
        let d = data();
        let hb = &d.config.hub_block;
        assert_eq!(
            (hb.min_top, hb.max_bottom, hb.shrink, hb.min_size),
            (0.25, 1.2, 0.2, 0.25)
        );
        let m = map(
            &["########", "#......#", "#......#", "#......#", "########"],
            60,
        );
        let mut mk = BlockMask::for_map(&m);
        // a table 1.6 × 0.8 (w × d), 0.7 tall, centred on the edge between cells (2,2) and (3,2): x 62.2..63.8
        let bb = Aabb::of_yawed_box(63.0, 0.0, 2.5, 1.6, 0.7, 0.8, 0.0);
        assert_eq!(mk.mark_box_cells(&m, &bb, hb.prop, hb), 2); // shrunk to x 62.4..63.6 → cells 2 and 3
        assert!(mk.is_blocked(m.idx(2, 2)));
        assert!(mk.is_blocked(m.idx(3, 2)));
        assert!(!mk.is_blocked(m.idx(4, 2)));
        assert!(!mk.is_blocked(m.idx(3, 1)));
        // a rug (top below minTop) and a hanging lantern (bottom above maxBottom) never block
        let rug = Aabb::new([61.0, 0.0, 1.0], [63.0, 0.05, 3.0]);
        assert_eq!(mk.mark_box_cells(&m, &rug, hb.prop, hb), 0);
        let hang = Aabb::new([61.0, 1.5, 1.0], [63.0, 2.0, 3.0]);
        assert_eq!(mk.mark_box_cells(&m, &hang, hb.prop, hb), 0);
        // a post thinner than minSize in both axes never blocks; thin in one axis marks its centre column
        let post = Aabb::new([65.4, 0.0, 1.4], [65.6, 1.0, 1.6]);
        assert_eq!(mk.mark_box_cells(&m, &post, hb.prop, hb), 0);
        let rail = Aabb::new([65.45, 0.0, 1.2], [65.55, 1.0, 3.8]);
        assert_eq!(mk.mark_box_cells(&m, &rail, hb.building, hb), 3); // z 1.4..3.6 → rows 1,2,3
        assert_eq!(mk.bits(m.idx(5, 2)), hb.building);
        // a box over a wall marks nothing there; the 1e-6 epsilon keeps an exact cell edge out of the next cell
        let edge = Aabb::new([61.2, 0.0, 1.2], [62.2, 1.0, 2.2]); // shrunk: 61.4..62.0, 1.4..2.0 → one cell
        assert_eq!(mk.mark_box_cells(&m, &edge, hb.flame, hb), 1);
        assert!(mk.bits(m.idx(1, 1)) & hb.flame != 0);
        assert_eq!(mk.bits(m.idx(2, 1)) & hb.flame, 0);
        let wall = Aabb::new([59.5, 0.0, -0.5], [60.9, 1.0, 0.9]);
        assert_eq!(mk.mark_box_cells(&m, &wall, hb.prop, hb), 0);
    }

    #[test]
    fn yawed_box_aabb_swaps_extents_at_quarter_turns() {
        let a = Aabb::of_yawed_box(10.0, 0.0, 5.0, 2.0, 1.0, 0.5, 0.0);
        assert!((a.max[0] - a.min[0] - 2.0).abs() < 1e-5);
        assert!((a.max[2] - a.min[2] - 0.5).abs() < 1e-5);
        let b = Aabb::of_yawed_box(10.0, 0.0, 5.0, 2.0, 1.0, 0.5, std::f32::consts::FRAC_PI_2);
        assert!((b.max[0] - b.min[0] - 0.5).abs() < 1e-5);
        assert!((b.max[2] - b.min[2] - 2.0).abs() < 1e-5);
        assert_eq!((b.min[1], b.max[1]), (0.0, 1.0));
    }

    #[test]
    fn hub_props_are_walled_off_by_the_mask_and_not_by_the_grid() {
        // the real hub: the flame anchor cell is floor for the grid but the brazier marks it
        let d = data();
        let m = d.parse_hub().expect("hub parses");
        let f = m.flame.expect("hub flame");
        assert!(!box_blocked(&m, None, f.x, f.z, d.config.cfg.radius));
        let mut mk = BlockMask::for_map(&m);
        let bb = Aabb::of_yawed_box(f.x, 0.0, f.z, 1.2, 1.0, 1.2, 0.0);
        assert_eq!(
            mk.mark_box_cells(&m, &bb, d.config.hub_block.flame, &d.config.hub_block),
            1
        );
        assert!(box_blocked(&m, Some(&mk), f.x, f.z, d.config.cfg.radius));
        assert_eq!(
            mk.colliders(&m, &d.config.hub_block)[0].kinds,
            vec!["FLAME"]
        );
    }

    #[test]
    fn speed_multipliers_match_config() {
        let d = data();
        let cfg = &d.config.cfg;
        assert_eq!((cfg.walk, cfg.sprint), (2.6, 4.2));
        assert_eq!(speed_multiplier(cfg, CellKind::Water, false), 0.55);
        assert_eq!(speed_multiplier(cfg, CellKind::Water, true), 0.6);
        for k in [
            CellKind::Floor,
            CellKind::Deep,
            CellKind::Stairs,
            CellKind::Altar,
        ] {
            assert_eq!(speed_multiplier(cfg, k, false), 1.0);
            assert_eq!(speed_multiplier(cfg, k, true), 1.0);
        }
        assert!((player_speed(cfg, CellKind::Water, false) - 2.6 * 0.55).abs() < 1e-6);
        assert!((player_speed(cfg, CellKind::Water, true) - 4.2 * 0.6).abs() < 1e-6);
        assert_eq!(player_speed(cfg, CellKind::Deep, true), 4.2);
        let m = map(&["#####", "#.W.#", "#####"], 0);
        assert!((player_speed_at(&m, cfg, 2.5, 1.5, false) - 2.6 * 0.55).abs() < 1e-6);
        assert_eq!(player_speed_at(&m, cfg, 1.5, 1.5, false), 2.6);
    }
}
