//! `world.js:buildWater` / `updateWater` — the `W` cells' translucent animated skin over the sunken
//! bed that `blocks.rs` builds.
//!
//! One merged sheet with *shared* vertices, exactly as the JS: neighbouring tiles neither overlap
//! nor crack, so the surface reads as one continuous rippling skin. Vertices sit at `y = −0.1` and
//! bob `±0.02` by `sin(t·2 + x + z)`; the material's emissive breathes `0.25 + 0.1·sin(t·1.7)`.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::BTreeMap;
use undercroft_data::{CellKind, ParsedMap};

/// Marks the water sheet so [`animate_water`] can find its mesh and material; `glow` is the
/// palette's `waterGlow`, whose `emissiveIntensity` the JS breathes.
#[derive(Component, Debug, Clone, Copy)]
pub struct WaterSheet {
    pub glow: u32,
}

/// The sheet's resting height (`vid()` pushes `y = -0.1`).
const Y: f32 = -0.1;

/// `buildWater(map, group, meta)` — `None` when the map has no `W` cell.
///
/// The returned mesh has one vertex per distinct grid corner; [`animate_water`] rewrites only the
/// `y` of each, which is what `updateWater` does to the `position` attribute.
pub fn build_water(map: &ParsedMap) -> Option<Mesh> {
    let mut ids: BTreeMap<(i32, i32), u32> = BTreeMap::new();
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    // corner (cx, cz) is the world point (ox + cx, cz)
    let vid =
        |ids: &mut BTreeMap<(i32, i32), u32>, pos: &mut Vec<[f32; 3]>, cx: i32, cz: i32| -> u32 {
            *ids.entry((cx, cz)).or_insert_with(|| {
                pos.push([(map.ox + cx) as f32, Y, cz as f32]);
                (pos.len() - 1) as u32
            })
        };
    for cz in 0..map.h {
        for cx in 0..map.w {
            if map.cell_type(cx, cz) != CellKind::Water {
                continue;
            }
            let a = vid(&mut ids, &mut pos, cx, cz);
            let b = vid(&mut ids, &mut pos, cx + 1, cz);
            let d = vid(&mut ids, &mut pos, cx + 1, cz + 1);
            let e = vid(&mut ids, &mut pos, cx, cz + 1);
            // `index.push(a, e, d, a, d, b)` — wound so the surface faces up.
            idx.extend_from_slice(&[a, e, d, a, d, b]);
        }
    }
    if pos.is_empty() {
        return None;
    }
    let n = pos.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    mesh.insert_indices(Indices::U32(idx));
    Some(mesh)
}

/// `updateWater(time)` — the vertex bob and the emissive breath.
pub(super) fn animate_water(
    clock: Res<crate::tick::Clock>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &WaterSheet)>,
) {
    let t = clock.time;
    for (mesh, mat, sheet) in &q {
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            if let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
                m.attribute_mut(Mesh::ATTRIBUTE_POSITION)
            {
                for v in p.iter_mut() {
                    v[1] = Y + 0.02 * (t * 2.0 + v[0] + v[2]).sin();
                }
            }
        }
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.emissive = super::palette::emissive(sheet.glow, 0.25 + 0.1 * (t * 1.7).sin());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::map::parse_map;

    fn map(rows: &[&str]) -> ParsedMap {
        parse_map(
            &rows.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
            0,
            "t",
            None,
            None,
        )
        .expect("map")
    }

    #[test]
    fn no_water_no_sheet() {
        assert!(build_water(&map(&["###", "#.#", "###"])).is_none());
    }

    /// Two adjacent water cells share the two corners between them: 6 vertices, not 8.
    #[test]
    fn adjacent_cells_share_their_corners() {
        let m = build_water(&map(&["####", "#WW#", "####"])).expect("sheet");
        assert_eq!(m.count_vertices(), 6);
        assert_eq!(m.indices().map(|i| i.len()), Some(12), "two quads");
    }

    /// The sheet sits at −0.1, above the −0.15 top of the bed `blocks.rs` sinks.
    #[test]
    fn the_sheet_rests_just_above_the_bed() {
        let m = build_water(&map(&["###", "#W#", "###"])).expect("sheet");
        match m.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x3(v) => {
                assert!(v.iter().all(|p| (p[1] - -0.1).abs() < 1e-6));
                assert_eq!(v.len(), 4);
            }
            _ => panic!("positions"),
        }
    }
}
