//! `world.js:buildBlocks` — the map as merged, vertex-coloured meshes, one per block kind.
//!
//! The prototype drew one `InstancedMesh` of oversize boxes per kind (`S = 1.02`, so neighbours
//! overlap and no rasterization cracks show at the ⅓-resolution render). The port keeps the very same
//! boxes, sizes, heights and per-cell colour jitter, but merges them into one mesh per kind and drops
//! the faces that can never be seen: a wall's side is emitted only where it borders a non-wall cell,
//! floors/decks lose their undersides, ceilings their tops. Geometry is otherwise identical.
//!
//! Heights (`buildBlocks`'s `kinds` table, `y` is the box centre, all boxes `1.02 × 1.02` in plan
//! except the pillar's `0.8 × 0.8`):
//!
//! | kind | height | centre y | span |
//! |---|---|---|---|
//! | floor | 0.2 | −0.10 | −0.20 … 0.00 |
//! | deep | 0.2 | −0.10 | −0.20 … 0.00 |
//! | water bed | 0.2 | −0.25 | −0.35 … −0.15 |
//! | wall | 3.4 | 1.50 | −0.20 … 3.20 |
//! | pillar | 3.4 | 1.50 | −0.20 … 3.20 |
//! | ceiling | 0.2 | 3.10 | 3.00 … 3.20 |
//!
//! Membership per cell, straight from the JS: a `WALL` cell is a wall and nothing else (`continue`,
//! so walls carry no ceiling); a `DEEP` cell is deep, except in a `bands` zone where lap 0 reads as
//! plain floor; `WATER` is a water bed; every other cell is floor (gates, shortcuts, the altar and
//! the elevator included) unless `hole` is set and it is the hub's `S` cell, which is left open for
//! the stairwell; every non-wall cell also gets a ceiling; a `PILLAR` cell gets a pillar *on top of*
//! its floor.

use bevy::asset::RenderAssetUsages;
use bevy::color::LinearRgba;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use undercroft_data::zone::Palette;
use undercroft_data::{CellKind, ParsedMap};
use undercroft_sim::grid;

use super::palette::jitter;

/// `buildBlocks`'s `S` — blocks are 2 % oversize so neighbours overlap.
const S: f32 = 1.02;
/// Per-cell colour jitter, `±6 %`.
const JITTER: f32 = 0.06;

/// One merged mesh's block kind (`mesh.name = "blocks:<kind>"` in the JS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockKind {
    Floor,
    Deep,
    Water,
    Wall,
    Pillar,
    Ceil,
}

impl BlockKind {
    /// Every kind, in `buildBlocks`'s declaration order.
    pub const ALL: [BlockKind; 6] = [
        BlockKind::Floor,
        BlockKind::Deep,
        BlockKind::Water,
        BlockKind::Wall,
        BlockKind::Pillar,
        BlockKind::Ceil,
    ];

    /// The JS mesh name suffix.
    pub fn name(self) -> &'static str {
        match self {
            BlockKind::Floor => "floor",
            BlockKind::Deep => "deep",
            BlockKind::Water => "water",
            BlockKind::Wall => "wall",
            BlockKind::Pillar => "pillar",
            BlockKind::Ceil => "ceil",
        }
    }

    /// `pal[kind]` — the palette colour the JS gives this kind.
    pub fn color(self, pal: &Palette) -> u32 {
        match self {
            BlockKind::Floor => pal.floor,
            BlockKind::Deep => pal.deep,
            BlockKind::Water => pal.water,
            BlockKind::Wall => pal.wall,
            BlockKind::Pillar => pal.pillar,
            BlockKind::Ceil => pal.ceil,
        }
    }

    /// `(y_bottom, y_top)` of the box.
    fn span(self) -> (f32, f32) {
        match self {
            BlockKind::Floor | BlockKind::Deep => (-0.2, 0.0),
            BlockKind::Water => (-0.35, -0.15),
            BlockKind::Wall | BlockKind::Pillar => (-0.2, 3.2),
            BlockKind::Ceil => (3.0, 3.2),
        }
    }

    /// Half the box's plan size (the pillar is the only narrow one).
    fn half(self) -> f32 {
        match self {
            BlockKind::Pillar => 0.4,
            _ => S / 2.0,
        }
    }

    /// Is the top face ever visible? Floors and decks yes, ceilings and full-height blocks no.
    fn top_face(self) -> bool {
        matches!(self, BlockKind::Floor | BlockKind::Deep | BlockKind::Water)
    }

    /// Is the bottom face ever visible? Only the ceiling's, which is what the player looks up at.
    fn bottom_face(self) -> bool {
        matches!(self, BlockKind::Ceil)
    }
}

/// The map facts `buildBlocks` needs: which cells hold which box.
struct Cells<'a> {
    m: &'a ParsedMap,
    bands: bool,
    hole: bool,
}

impl Cells<'_> {
    fn t(&self, cx: i32, cz: i32) -> CellKind {
        self.m.cell_type(cx, cz)
    }

    /// `lapOf(x, z, m)` in a `bands` zone, 0 everywhere else.
    fn lap(&self, cx: i32, cz: i32) -> i32 {
        if self.bands {
            grid::lap_of_map(self.m, cx, cz)
        } else {
            0
        }
    }

    /// Does cell `(cx, cz)` hold a box of this kind? Out-of-bounds cells read as `Wall`
    /// (`maps.js:cellType`), which is what makes the map border cull correctly.
    fn has(&self, k: BlockKind, cx: i32, cz: i32) -> bool {
        let t = self.t(cx, cz);
        match k {
            BlockKind::Wall => t == CellKind::Wall,
            BlockKind::Pillar => t == CellKind::Pillar,
            BlockKind::Ceil => t != CellKind::Wall,
            BlockKind::Water => t == CellKind::Water,
            BlockKind::Deep => t == CellKind::Deep && !(self.bands && self.lap(cx, cz) == 0),
            BlockKind::Floor => match t {
                CellKind::Wall | CellKind::Water => false,
                CellKind::Deep => self.bands && self.lap(cx, cz) == 0,
                CellKind::Stairs => !self.hole,
                _ => true,
            },
        }
    }

    /// Is this face of a `k` box against `(nx, nz)` hidden by whatever that cell holds?
    fn occluded(&self, k: BlockKind, nx: i32, nz: i32) -> bool {
        match k {
            // a neighbouring wall covers −0.2 … 3.2, i.e. everything but the water bed's skirt
            BlockKind::Wall => self.has(BlockKind::Wall, nx, nz),
            BlockKind::Floor | BlockKind::Deep => {
                self.has(BlockKind::Wall, nx, nz)
                    || self.has(BlockKind::Floor, nx, nz)
                    || self.has(BlockKind::Deep, nx, nz)
            }
            BlockKind::Water => self.has(BlockKind::Water, nx, nz),
            BlockKind::Ceil => {
                self.has(BlockKind::Ceil, nx, nz) || self.has(BlockKind::Wall, nx, nz)
            }
            // the pillar is inset by 0.1 on every side: nothing next door touches it
            BlockKind::Pillar => false,
        }
    }
}

/// Vertex-coloured triangle soup under construction.
#[derive(Default)]
struct Soup {
    pos: Vec<[f32; 3]>,
    norm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Soup {
    /// One quad, corners counter-clockwise as seen from `normal`.
    fn quad(&mut self, c: [[f32; 3]; 4], normal: [f32; 3], color: [f32; 4]) {
        let base = self.pos.len() as u32;
        for v in c {
            self.pos.push(v);
            self.norm.push(normal);
            self.col.push(color);
        }
        self.idx
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.norm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        mesh.insert_indices(Indices::U32(self.idx));
        mesh
    }
}

/// `world.js:buildBlocks(m, group, meta, {hole})` — one merged mesh per non-empty block kind.
///
/// `bands` is `meta.deepStyle === 'bands'` (the Source spiral: lap 0 deep reads as floor and every
/// block darkens 12 % per lap, bottoming out at 30 %); `hole` is the hub's open stairwell.
pub fn build_blocks(
    map: &ParsedMap,
    pal: &Palette,
    bands: bool,
    hole: bool,
) -> Vec<(BlockKind, Mesh)> {
    let cells = Cells {
        m: map,
        bands,
        hole,
    };
    let mut out = Vec::new();
    for (ki, &k) in BlockKind::ALL.iter().enumerate() {
        let base = LinearRgba::from(super::palette::rgb(k.color(pal)));
        let (y0, y1) = k.span();
        let half = k.half();
        let mut soup = Soup::default();
        for cz in 0..map.h {
            for cx in 0..map.w {
                if !cells.has(k, cx, cz) {
                    continue;
                }
                // ±6 % per-cell jitter, then the Source's per-lap darkening. The water bed is the
                // one kind the JS pushes without a lap (`kinds.water.list.push(p)`).
                let mut f = jitter(cx, cz, ki as u32, JITTER);
                let lap = if k == BlockKind::Water {
                    0
                } else {
                    cells.lap(cx, cz)
                };
                if lap != 0 {
                    f *= (1.0 - 0.12 * lap as f32).max(0.3);
                }
                let color = [base.red * f, base.green * f, base.blue * f, 1.0];

                let x = (map.ox + cx) as f32 + 0.5;
                let z = cz as f32 + 0.5;
                let (x0, x1) = (x - half, x + half);
                let (z0, z1) = (z - half, z + half);

                if k.top_face() {
                    soup.quad(
                        [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
                        [0.0, 1.0, 0.0],
                        color,
                    );
                }
                if k.bottom_face() {
                    soup.quad(
                        [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
                        [0.0, -1.0, 0.0],
                        color,
                    );
                }
                if !cells.occluded(k, cx + 1, cz) {
                    soup.quad(
                        [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
                        [1.0, 0.0, 0.0],
                        color,
                    );
                }
                if !cells.occluded(k, cx - 1, cz) {
                    soup.quad(
                        [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
                        [-1.0, 0.0, 0.0],
                        color,
                    );
                }
                if !cells.occluded(k, cx, cz + 1) {
                    soup.quad(
                        [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
                        [0.0, 0.0, 1.0],
                        color,
                    );
                }
                if !cells.occluded(k, cx, cz - 1) {
                    soup.quad(
                        [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
                        [0.0, 0.0, -1.0],
                        color,
                    );
                }
            }
        }
        if !soup.is_empty() {
            out.push((k, soup.into_mesh()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::map::parse_map;

    fn map(rows: &[&str]) -> ParsedMap {
        parse_map(
            &rows.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
            0,
            "test",
            None,
            None,
        )
        .expect("valid test map")
    }

    fn built(rows: &[&str], hole: bool) -> Vec<(BlockKind, Mesh)> {
        build_blocks(&map(rows), &pal(), false, hole)
    }

    fn pal() -> Palette {
        Palette {
            floor: 0x6e675e,
            wall: 0x5c564d,
            pillar: 0x7d7160,
            ceil: 0x4d4841,
            deep: 0x0c0c18,
            water: 0x2a2f36,
            water_surface: 0x1a2a3a,
            water_glow: 0x06101a,
            fog: undercroft_data::zone::Fog {
                color: 0x000000,
                density: 0.11,
            },
            ambient: 0x0b0a14,
            sky: 0x000000,
        }
    }

    fn find(v: &[(BlockKind, Mesh)], k: BlockKind) -> Option<&Mesh> {
        v.iter().find(|(kk, _)| *kk == k).map(|(_, m)| m)
    }

    fn quads(v: &[(BlockKind, Mesh)], k: BlockKind) -> usize {
        find(v, k).map(|m| m.count_vertices() / 4).unwrap_or(0)
    }

    /// A 3×3 ring of walls round one floor cell: the floor is one top quad (all four sides border
    /// walls), the ceiling one bottom quad, and the eight walls show exactly the four faces that
    /// look into the room.
    #[test]
    fn one_room_emits_only_the_visible_faces() {
        let b = built(&["###", "#.#", "###"], false);
        assert_eq!(quads(&b, BlockKind::Floor), 1, "floor: top only");
        assert_eq!(quads(&b, BlockKind::Ceil), 1, "ceiling: bottom only");
        // 8 wall cells; only the 4 orthogonal neighbours of the open cell face it. The corner walls
        // are surrounded by walls and the out-of-bounds border, which reads as wall too.
        assert_eq!(quads(&b, BlockKind::Wall), 4, "wall: the four inner faces");
        assert!(find(&b, BlockKind::Water).is_none(), "no water on this map");
        assert!(find(&b, BlockKind::Deep).is_none());
        assert!(find(&b, BlockKind::Pillar).is_none());
    }

    /// A pillar cell keeps its floor *and* gets a pillar with four sides (it is inset, so nothing
    /// next door hides it) and no cap.
    #[test]
    fn a_pillar_cell_has_both_floor_and_pillar() {
        let b = built(&["###", "#P#", "###"], false);
        assert_eq!(quads(&b, BlockKind::Floor), 1);
        assert_eq!(quads(&b, BlockKind::Pillar), 4);
        assert_eq!(quads(&b, BlockKind::Ceil), 1, "pillars are ceiled");
    }

    /// Water sinks its bed 0.15 below the floor, so the floor next to it keeps that side face and
    /// the bed keeps all four of its own.
    #[test]
    fn water_lowers_the_bed_and_reopens_the_floor_side() {
        let b = built(&["####", "#.W#", "####"], false);
        assert_eq!(quads(&b, BlockKind::Water), 1 + 4, "top + four skirts");
        assert_eq!(
            quads(&b, BlockKind::Floor),
            1 + 1,
            "top + the face on the water"
        );
        let m = find(&b, BlockKind::Water).expect("water bed");
        let ys: Vec<f32> = match m.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x3(v) => v.iter().map(|p| p[1]).collect(),
            _ => panic!("positions"),
        };
        assert!(ys.iter().cloned().fold(f32::INFINITY, f32::min) - -0.35 < 1e-5);
    }

    /// `hole: true` (the hub) leaves the `S` cell without a floor so the stairwell is open; a zone
    /// keeps it.
    #[test]
    fn the_hub_hole_removes_the_stairs_floor() {
        assert_eq!(
            quads(&built(&["###", "#S#", "###"], false), BlockKind::Floor),
            1
        );
        assert_eq!(
            quads(&built(&["###", "#S#", "###"], true), BlockKind::Floor),
            0
        );
    }

    /// Gates, shortcuts, the altar and the elevator all render as plain floor (`buildBlocks`'s
    /// `else` branch).
    #[test]
    fn doors_and_markers_are_floor() {
        let b = built(&["#####", "#XA=#", "#V..#", "#####"], false);
        assert_eq!(quads(&b, BlockKind::Floor), 6, "six open cells, tops only");
        assert_eq!(quads(&b, BlockKind::Ceil), 6);
    }

    /// Colours come from the palette, jittered by ±6 % and never anything else.
    #[test]
    fn vertex_colours_follow_the_palette() {
        let b = built(&["###", "#.#", "###"], false);
        let m = find(&b, BlockKind::Floor).expect("floor");
        let want = LinearRgba::from(super::super::palette::rgb(pal().floor));
        match m.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x4(v) => {
                for c in v {
                    assert!((c[0] / want.red - 1.0).abs() <= 0.061, "{:?}", c);
                    assert_eq!(c[3], 1.0);
                }
                assert_eq!(v.len(), 4);
            }
            _ => panic!("colours"),
        }
    }

    /// In a `bands` zone lap 0 deep reads as floor and deeper laps darken; the water bed never does.
    #[test]
    fn bands_move_lap_zero_deep_to_the_floor_mesh() {
        let rows: Vec<String> = (0..9)
            .map(|z| {
                (0..9)
                    .map(|x| {
                        if x == 0 || z == 0 || x == 8 || z == 8 {
                            '#'
                        } else {
                            'D'
                        }
                    })
                    .collect()
            })
            .collect();
        let m = parse_map(&rows, 0, "bands", None, None).expect("map");
        let flat = build_blocks(&m, &pal(), false, false);
        let banded = build_blocks(&m, &pal(), true, false);
        assert_eq!(quads(&flat, BlockKind::Floor), 0, "flat: every D is deep");
        assert!(
            quads(&banded, BlockKind::Floor) > 0,
            "banded: lap 0 is floor"
        );
        assert!(
            quads(&banded, BlockKind::Deep) > 0,
            "banded: deeper laps stay deep"
        );
    }
}
