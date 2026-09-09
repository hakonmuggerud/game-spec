//! The hub camp (`world.js:HUB_PROPS_V2`, `buildProps`, `propBoxesWorld`, `markBoxCells`).
//!
//! Two jobs:
//!
//! 1. **Collision.** Every tall prop box is written into `HubMap::mask` with
//!    [`BlockMask::mark_box_cells`] + [`Aabb::of_yawed_box`], exactly as `world.js:buildProps`'
//!    `cellsOf` does. This lane is the only writer of the mask (PHASE2_LANES §1); the footprint is
//!    computed once and re-applied if the hub map is ever rebuilt.
//! 2. **Geometry.** The same placed boxes become entities under one `HubProps` root at `HUB_OX`.
//!    The JS merged them into two draw calls (`models.mergeBoxes`); Bevy batches instanced cuboids
//!    of the same mesh + material handle, which is what [`super::model::BoxAssets`] gives us, so
//!    the camp is one entity per box with a handful of shared handles.

use bevy::prelude::*;
use undercroft_data::config::HubBlock;
use undercroft_data::models::{place_boxes, BoxDef, ModelTable};
use undercroft_data::ParsedMap;
use undercroft_sim::collision::{Aabb, BlockMask};
use undercroft_sim::grid::{in_bounds, is_solid};

use crate::resources::{Game, HubMapRes};

use super::model::{spawn_box, BoxAssets};

/* ============================================================
The table (`world.js:38 HUB_PROPS_V2`)
============================================================ */

/// `world.js:37 faceFlame` vs a literal `ry` — how a placement's yaw is worked out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Yaw {
    /// `ry: <number>` (absent means `0`).
    Fixed(f32),
    /// `ry: faceFlame` — the prop turns its front (−Z) toward the brazier.
    FaceFlame,
}

/// The `opts` a `HUB_PROPS_V2` row passes to its `PROP_BOXES` factory. `models.ron` records every
/// factory with its *default* options, so the two options the table actually uses are re-applied
/// here: the nave rug's size (`rugBoxes` is rebuilt) and a bedroll's blanket colour.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PropOpts {
    /// `rug` `{w, d}`.
    pub rug_size: Option<(f32, f32)>,
    /// `bedroll` `{color}` — the blanket box.
    pub color: Option<u32>,
}

/// One `HUB_PROPS_V2` row: a `PROP_BOXES` model at a hub cell, offset by `dx`/`dz` and yawed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropPlacement {
    pub model: &'static str,
    pub cx: i32,
    pub cz: i32,
    pub dx: f32,
    pub dz: f32,
    pub ry: Yaw,
    pub opts: PropOpts,
}

/// A row with defaults for everything the JS table leaves out.
const fn prop(model: &'static str, cx: i32, cz: i32) -> PropPlacement {
    PropPlacement {
        model,
        cx,
        cz,
        dx: 0.0,
        dz: 0.0,
        ry: Yaw::Fixed(0.0),
        opts: PropOpts {
            rug_size: None,
            color: None,
        },
    }
}

const fn with_off(mut p: PropPlacement, dx: f32, dz: f32) -> PropPlacement {
    p.dx = dx;
    p.dz = dz;
    p
}

const fn with_ry(mut p: PropPlacement, ry: Yaw) -> PropPlacement {
    p.ry = ry;
    p
}

const fn with_color(mut p: PropPlacement, color: u32) -> PropPlacement {
    p.opts.color = Some(color);
    p
}

const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;

/// `world.js:38 HUB_PROPS_V2` — the camp props of the 25×13 hub, verbatim and in the JS order.
pub const HUB_PROPS_V2: [PropPlacement; 23] = [
    // nave: hearth rug, two benches turned to the fire, firewood and a stew pot beside the
    // brazier, bedrolls along the walls
    PropPlacement {
        model: "rug",
        cx: 11,
        cz: 3,
        dx: 0.0,
        dz: 0.0,
        ry: Yaw::Fixed(0.0),
        opts: PropOpts {
            rug_size: Some((4.6, 2.6)),
            color: None,
        },
    },
    with_ry(prop("bench", 8, 3), Yaw::FaceFlame),
    with_ry(prop("bench", 14, 3), Yaw::FaceFlame),
    with_off(prop("logPile", 9, 1), 0.0, -0.12),
    with_off(prop("cookpot", 13, 1), 0.0, -0.05),
    with_off(prop("bedroll", 7, 1), -0.1, 0.5),
    with_color(with_off(prop("bedroll", 15, 1), 0.1, 0.5), 0x3a5a6a),
    with_off(prop("crateStack", 7, 9), -0.05, 0.05),
    with_off(prop("barrel", 7, 8), -0.15, 0.0),
    with_off(prop("barrel", 6, 11), -0.15, 0.15),
    with_ry(with_off(prop("crate", 14, 11), 0.1, 0.1), Yaw::Fixed(0.3)),
    with_off(prop("candleCluster", 12, 2), 0.35, -0.3),
    // NW room: the Workshop and the Cartographer's Table
    with_off(prop("crate", 5, 1), 0.1, -0.1),
    with_ry(
        with_off(prop("bookshelf", 5, 3), 0.3, 0.0),
        Yaw::Fixed(HALF_PI),
    ),
    with_ry(
        with_off(prop("bookshelf", 5, 4), 0.3, 0.0),
        Yaw::Fixed(HALF_PI),
    ),
    // NE room: the Oil Press and the Shrine
    with_off(prop("herbRail", 19, 1), 0.0, 0.55),
    with_off(prop("candleCluster", 21, 3), 0.0, 0.32),
    with_color(with_off(prop("bedroll", 17, 3), -0.1, 0.5), 0x6a5a30),
    with_ry(with_off(prop("crate", 17, 1), -0.1, -0.1), Yaw::Fixed(-0.2)),
    // W alcove (tram): crates at the rail end. E alcove (elevator): stores in the SW corner
    with_off(prop("crate", 5, 8), 0.1, 0.0),
    with_off(prop("barrel", 5, 9), 0.15, 0.1),
    with_off(prop("crateStack", 17, 9), -0.05, 0.05),
    with_off(prop("barrel", 18, 9), 0.0, 0.1),
];

/* ============================================================
Placement (`world.js:247 propBoxesWorld`)
============================================================ */

/// `world.js:37 faceFlame` — `atan2(-(flame.x - x), -(flame.z - z))` about the prop's *cell*
/// centre (the `dx`/`dz` offsets are deliberately not part of the JS expression).
pub fn face_flame(m: &ParsedMap, cx: i32, cz: i32) -> f32 {
    let Some(f) = m.flame else {
        return 0.0;
    };
    let (fx, fz) = ((m.ox + f.cx) as f32 + 0.5, f.cz as f32 + 0.5);
    let (x, z) = ((m.ox + cx) as f32 + 0.5, cz as f32 + 0.5);
    (-(fx - x)).atan2(-(fz - z))
}

/// The yaw of one placement.
pub fn yaw_of(m: &ParsedMap, p: &PropPlacement) -> f32 {
    match p.ry {
        Yaw::Fixed(r) => r,
        Yaw::FaceFlame => face_flame(m, p.cx, p.cz),
    }
}

/// `models.js:518 rugBoxes({w, d, color, border, inner})` — the one `PROP_BOXES` factory whose
/// options change its geometry, so it cannot come from the recorded default box list.
pub fn rug_boxes(w: f32, d: f32) -> Vec<BoxDef> {
    let (c, b, inner) = (0x7a2a22, 0xc09040, 0x5a1e1a);
    let mk =
        |x: f32, y: f32, z: f32, w: f32, h: f32, d: f32, color: u32, name: Option<&str>| BoxDef {
            x,
            y,
            z,
            w,
            h,
            d,
            color,
            emissive: None,
            emissive_k: 0.0,
            name: name.map(str::to_string),
            ry: 0.0,
            part: None,
            hidden: false,
        };
    vec![
        mk(0.0, 0.0, 0.0, w, 0.02, d, c, Some("rug")),
        mk(0.0, 0.02, -d / 2.0 + 0.1, w, 0.01, 0.12, b, None),
        mk(0.0, 0.02, d / 2.0 - 0.1, w, 0.01, 0.12, b, None),
        mk(-w / 2.0 + 0.1, 0.02, 0.0, 0.12, 0.01, d, b, None),
        mk(w / 2.0 - 0.1, 0.02, 0.0, 0.12, 0.01, d, b, None),
        mk(0.0, 0.02, -d * 0.18, w * 0.6, 0.01, 0.08, inner, None),
        mk(0.0, 0.02, d * 0.18, w * 0.6, 0.01, 0.08, inner, None),
    ]
}

/// `world.js:247 propBoxesWorld(m, p)` — the placement's box list in world space, or an empty
/// list when `PROP_BOXES` has no such model.
pub fn prop_boxes_world(models: &ModelTable, m: &ParsedMap, p: &PropPlacement) -> Vec<BoxDef> {
    let base = match p.opts.rug_size {
        Some((w, d)) => rug_boxes(w, d),
        None => {
            let Some(list) = models.props.get(p.model) else {
                return Vec::new();
            };
            let mut list = list.clone();
            if let Some(color) = p.opts.color {
                // `bedrollBoxes({color})` only recolours the blanket.
                for b in &mut list {
                    if b.name.as_deref() == Some("blanket") {
                        b.color = color;
                    }
                }
            }
            list
        }
    };
    let x = (m.ox + p.cx) as f32 + 0.5 + p.dx;
    let z = p.cz as f32 + 0.5 + p.dz;
    place_boxes(&base, x, z, yaw_of(m, p))
}

/// The AABB of one placed box (`world.js:253 buildProps` `cellsOf`).
pub fn box_aabb(b: &BoxDef) -> Aabb {
    Aabb::of_yawed_box(b.x, b.y, b.z, b.w, b.h, b.d, b.ry)
}

/// Mark every prop's tall boxes into `mask` and return the cell indices that ended up carrying
/// `HUB_BLOCK.PROP`, in ascending order. Pure — the mask goes in as an explicit value, so the
/// footprint test runs with no app at all.
pub fn mark_props(
    models: &ModelTable,
    hb: &HubBlock,
    m: &ParsedMap,
    mask: &mut BlockMask,
) -> Vec<usize> {
    for p in &HUB_PROPS_V2 {
        // `world.js:269` — a placement on a solid or off-grid cell is skipped entirely.
        if !in_bounds(m, p.cx, p.cz) || is_solid(m, p.cx, p.cz) {
            continue;
        }
        for b in prop_boxes_world(models, m, p) {
            mask.mark_box_cells(m, &box_aabb(&b), hb.prop, hb);
        }
    }
    mask.mask
        .iter()
        .enumerate()
        .filter(|(_, &b)| b & hb.prop != 0)
        .map(|(i, _)| i)
        .collect()
}

/* ============================================================
Systems
============================================================ */

/// The footprint this lane wrote into `HubMap::mask`, so it can tell whether the mask still
/// carries it (a rebuilt hub map arrives with a fresh, empty mask).
#[derive(Resource, Debug, Default)]
pub struct PropFootprint {
    /// Cell indices carrying `HUB_BLOCK.PROP`, ascending.
    pub cells: Vec<usize>,
    /// The mask length the footprint was computed against.
    pub mask_len: usize,
}

impl PropFootprint {
    /// Is this footprint still present in `mask`? False for a fresh mask, a different map size, or
    /// before the first marking.
    fn intact(&self, mask: &BlockMask, prop_bit: u8) -> bool {
        self.mask_len == mask.mask.len()
            && !self.cells.is_empty()
            && self.cells.iter().all(|&i| mask.bits(i) & prop_bit != 0)
    }
}

/// `world.js:223` — `map.blockMask = new Uint8Array(...)` then `buildProps(map, group)`. Runs on
/// the first frame `HubMapRes` is `Some` and again whenever the mask no longer carries the
/// footprint (i.e. `run.rs` rebuilt the hub map).
fn mark_prop_footprints(game: Game, mut hub_map: ResMut<HubMapRes>, mut fp: ResMut<PropFootprint>) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let hb = data.config.hub_block.clone();
    let Some(hm) = hub_map.bypass_change_detection().0.as_mut() else {
        return;
    };
    if fp.intact(&hm.mask, hb.prop) {
        return;
    }
    let cells = mark_props(&data.models, &hb, &hm.map, &mut hm.mask);
    info!(
        "hub: prop footprints -> {} cell(s) of HUB_BLOCK.PROP",
        cells.len()
    );
    fp.mask_len = hm.mask.mask.len();
    fp.cells = cells;
}

/// The root of the merged camp (`world.js` `hub:props`), at `HUB_OX` so its children carry hub
/// grid coordinates.
#[derive(Component, Debug)]
pub struct HubProps;

/// `world.js:253 buildProps` — the camp geometry, once.
#[allow(clippy::too_many_arguments)]
fn spawn_props(
    mut commands: Commands,
    game: Game,
    hub_map: Res<HubMapRes>,
    mut assets: ResMut<BoxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<HubProps>>,
) {
    if !existing.is_empty() {
        return;
    }
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let Some(hm) = hub_map.0.as_ref() else {
        return;
    };
    let m = &hm.map;
    let ox = m.ox as f32;
    let root = commands
        .spawn((
            HubProps,
            Name::new("hub:props"),
            Transform::from_xyz(ox, 0.0, 0.0),
            Visibility::Inherited,
        ))
        .id();
    let mut n = 0;
    for p in &HUB_PROPS_V2 {
        if !in_bounds(m, p.cx, p.cz) || is_solid(m, p.cx, p.cz) {
            continue;
        }
        for mut b in prop_boxes_world(&data.models, m, p) {
            b.x -= ox; // the root already carries HUB_OX
            spawn_box(&mut commands, &mut assets, &mut meshes, &mut mats, root, &b);
            n += 1;
        }
    }
    info!("hub: camp props -> {n} boxes");
}

/// The prop footprint marker and the camp geometry.
pub fn plugin(app: &mut App) {
    app.init_resource::<PropFootprint>().add_systems(
        Update,
        (mark_prop_footprints, spawn_props).in_set(super::HubSet),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::GameData;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    #[test]
    fn table_matches_the_js_row_count() {
        assert_eq!(HUB_PROPS_V2.len(), 23, "world.js:38 HUB_PROPS_V2 rows");
        assert_eq!(
            HUB_PROPS_V2
                .iter()
                .filter(|p| p.ry == Yaw::FaceFlame)
                .count(),
            2,
            "the two nave benches face the flame"
        );
    }

    /// `world.js:37 faceFlame(8, 3, m)`: the west bench at cell (8, 3) turns its front (−Z)
    /// toward the brazier. `maps/hub.txt` puts the `F` at cell (11, 1), so with `HUB_OX = 60` the
    /// flame is at `(71.5, 1.5)` and the bench cell centre at `(68.5, 3.5)`:
    /// `atan2(-(71.5 - 68.5), -(1.5 - 3.5)) = atan2(-3, 2) = -0.98279…` — the bench looks north
    /// and a little west, i.e. up the nave at the fire.
    #[test]
    fn face_flame_yaw_for_the_west_bench() {
        let data = data();
        let m = data.parse_hub().expect("hub map");
        let f = m.flame.expect("hub flame marker");
        assert_eq!((f.cx, f.cz), (11, 1), "maps/hub.txt puts the F at (11, 1)");
        let want = (-3.0_f32).atan2(2.0);
        let got = face_flame(&m, 8, 3);
        assert!((got - want).abs() < 1e-6, "{got} vs {want}");
        assert!((got + 0.982_793_7).abs() < 1e-5, "yaw {got}");
        // Its mirror east of the nave leans the other way by the same amount.
        let east = face_flame(&m, 14, 3);
        assert!((east + want).abs() < 1e-6, "{east} vs {}", -want);
    }

    /// The tall props' mask footprint, against a hand computation for one of them: the barrel at
    /// cell (7, 8) with `dx = -0.15`. `barrelBoxes` is 0.7 tall (above `minTop`) and its widest box
    /// is 0.54 across, so the AABB is `x ∈ [ox + 7.08, ox + 7.62]`, `z ∈ [8.23, 8.77]`; shrunk by
    /// `HUB_BLOCK.shrink` (0.2) that is `[ox + 7.28, ox + 7.42] × [8.43, 8.57]`, i.e. exactly cell
    /// (7, 8).
    #[test]
    fn barrel_footprint_is_its_own_cell() {
        let data = data();
        let m = data.parse_hub().expect("hub map");
        let hb = &data.config.hub_block;
        let p = HUB_PROPS_V2
            .iter()
            .find(|p| p.model == "barrel" && p.cx == 7 && p.cz == 8)
            .expect("the barrel by the crate stack");
        let mut mask = BlockMask::for_map(&m);
        for b in prop_boxes_world(&data.models, &m, p) {
            mask.mark_box_cells(&m, &box_aabb(&b), hb.prop, hb);
        }
        let cells: Vec<usize> = mask
            .mask
            .iter()
            .enumerate()
            .filter(|(_, &b)| b != 0)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(cells, vec![m.idx(7, 8)], "cells {cells:?}");
    }

    /// Rugs, bedrolls, candle clusters and anything hung from the ceiling stay walkable
    /// (`HUB_BLOCK.minTop` / `maxBottom` / `minSize`, `models.js:515`).
    #[test]
    fn flat_and_hanging_props_never_block() {
        let data = data();
        let m = data.parse_hub().expect("hub map");
        let hb = &data.config.hub_block;
        for name in ["rug", "bedroll", "candleCluster", "herbRail"] {
            let p = HUB_PROPS_V2
                .iter()
                .find(|p| p.model == name)
                .unwrap_or_else(|| panic!("{name} in HUB_PROPS_V2"));
            let mut mask = BlockMask::for_map(&m);
            for b in prop_boxes_world(&data.models, &m, p) {
                mask.mark_box_cells(&m, &box_aabb(&b), hb.prop, hb);
            }
            assert!(
                mask.mask.iter().all(|&b| b == 0),
                "{name} must stay walkable"
            );
        }
    }

    /// Every model the table names exists in `models.ron` (the rug is generated, so it is exempt
    /// only from the *default* list lookup).
    #[test]
    fn every_placement_resolves_to_boxes() {
        let data = data();
        let m = data.parse_hub().expect("hub map");
        for p in &HUB_PROPS_V2 {
            assert!(
                !prop_boxes_world(&data.models, &m, p).is_empty(),
                "no boxes for {}",
                p.model
            );
        }
    }

    /// The whole camp's footprint, so a change in the tables shows up as a diff.
    #[test]
    fn camp_footprint_is_stable() {
        let data = data();
        let m = data.parse_hub().expect("hub map");
        let mut mask = BlockMask::for_map(&m);
        let cells = mark_props(&data.models, &data.config.hub_block, &m, &mut mask);
        let named: Vec<(i32, i32)> = cells.iter().map(|&i| m.cell_of(i)).collect();
        assert_eq!(
            named,
            vec![
                (5, 1),   // the NW-room crate by the workbench
                (9, 1),   // the firewood pile
                (13, 1),  // the stew pot on its tripod
                (17, 1),  // the NE-room crate
                (5, 3),   // bookshelf
                (8, 3),   // the west bench
                (14, 3),  // the east bench
                (5, 4),   // bookshelf
                (5, 8),   // the W alcove crate
                (7, 8),   // barrel
                (5, 9),   // barrel
                (7, 9),   // crate stack
                (17, 9),  // crate stack
                (18, 9),  // barrel
                (6, 11),  // barrel
                (14, 11), // crate
            ],
            "cells {named:?}"
        );
    }
}
