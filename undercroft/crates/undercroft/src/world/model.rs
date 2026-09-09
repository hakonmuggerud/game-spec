//! `models.js` factories rebuilt from `Game.data().models` (`ModelTable`). The world lane only needs
//! the *static* props — items, gates, shortcut bars, the stairs, the elevator, the altar and the
//! planted lantern — so this is deliberately smaller than the `creatures`/`hub` builders: boxes are
//! merged per material and each `ModelDef` becomes one root entity with a handful of mesh children.
//!
//! Conventions are `undercroft-data/src/models.rs`'s: feet origin, front −Z, `BoxDef::y` is the box
//! *bottom*, positions are local to the box's `part` (or the root).

use bevy::prelude::*;
use std::collections::BTreeMap;
use undercroft_data::models::{BoxDef, ModelDef};

use super::palette::{emissive, lambert, rgb};

/// One merged mesh + the material it wants. Keyed by `(color, emissive, emissive_k)` as
/// PHASE2_LANES §1 asks, plus an optional box name so a mesh the game animates (a lantern's `glass`)
/// keeps its own material.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshGroup {
    /// `BoxDef::name` when this group was split out by name, else `None`.
    pub name: Option<String>,
    pub color: u32,
    pub emissive: Option<u32>,
    pub emissive_k: f32,
    pub mesh: Mesh,
}

/// `ModelDef` → merged meshes. `split` names boxes that must stay in their own group because the
/// game animates their material (`glass`, `lamp`, `rune`). Hidden boxes are skipped, as three's
/// `visible = false` does.
pub fn build_model(def: &ModelDef, split: &[&str]) -> Vec<MeshGroup> {
    let parts = part_transforms(def);
    let root = Vec3::from(def.scale);
    // BTreeMap keeps the output order stable for the tests.
    let mut by_key: BTreeMap<(Option<String>, u32, Option<u32>, u32), Mesh> = BTreeMap::new();
    for b in &def.boxes {
        if b.hidden {
            continue;
        }
        let name = b
            .name
            .as_deref()
            .filter(|n| split.contains(n))
            .map(str::to_string);
        let key = (name, b.color, b.emissive, b.emissive_k.to_bits());
        let local = parts
            .get(b.part.as_deref().unwrap_or(""))
            .copied()
            .unwrap_or(Transform::IDENTITY);
        let mesh = box_mesh(b, local, root);
        match by_key.get_mut(&key) {
            Some(acc) => acc.merge(&mesh).expect("cuboid meshes always merge"),
            None => {
                by_key.insert(key, mesh);
            }
        }
    }
    by_key
        .into_iter()
        .map(|((name, color, emis, k), mesh)| MeshGroup {
            name,
            color,
            emissive: emis,
            emissive_k: f32::from_bits(k),
            mesh,
        })
        .collect()
}

/// One `BoxDef` as a cuboid, moved into the model's local space (`y` is the box bottom).
fn box_mesh(b: &BoxDef, part: Transform, root_scale: Vec3) -> Mesh {
    let local = Transform {
        translation: Vec3::new(b.x, b.y + b.h / 2.0, b.z),
        rotation: Quat::from_rotation_y(b.ry),
        scale: Vec3::ONE,
    };
    let combined = Transform::from_scale(root_scale) * part * local;
    Mesh::from(Cuboid::new(b.w, b.h, b.d)).transformed_by(combined)
}

/// Each `PartDef` resolved against its parents; the root is the empty key.
fn part_transforms(def: &ModelDef) -> BTreeMap<String, Transform> {
    let mut out: BTreeMap<String, Transform> = BTreeMap::new();
    out.insert(String::new(), Transform::IDENTITY);
    // Parents always appear before children in the exported table; two passes cover a stray reorder.
    for _ in 0..2 {
        for p in &def.parts {
            let parent = out
                .get(p.parent.as_deref().unwrap_or(""))
                .copied()
                .unwrap_or(Transform::IDENTITY);
            let local = Transform {
                translation: Vec3::from(p.pivot),
                rotation: Quat::from_euler(
                    EulerRot::XYZ,
                    p.rotation[0],
                    p.rotation[1],
                    p.rotation[2],
                ),
                scale: Vec3::from(p.scale),
            };
            out.insert(p.name.clone(), parent * local);
        }
    }
    out
}

/// Spawn a built model under `parent_of`: one child entity per [`MeshGroup`], each carrying a
/// [`Name`] when the group was split by box name so later systems can find it (`glass`).
pub fn spawn_groups(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
    groups: Vec<MeshGroup>,
) {
    for g in groups {
        let mut mat = lambert(rgb(g.color));
        if let Some(e) = g.emissive {
            mat.emissive = emissive(e, g.emissive_k);
        }
        let mut child = commands.spawn((
            Mesh3d(meshes.add(g.mesh)),
            MeshMaterial3d(materials.add(mat)),
            Transform::IDENTITY,
        ));
        if let Some(n) = g.name {
            child.insert(Name::new(n));
        }
        let id = child.id();
        commands.entity(root).add_child(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::models::PartDef;

    fn boxed(name: Option<&str>, color: u32, y: f32) -> BoxDef {
        BoxDef {
            x: 0.0,
            y,
            z: 0.0,
            w: 1.0,
            h: 1.0,
            d: 1.0,
            color,
            emissive: None,
            emissive_k: 0.0,
            name: name.map(str::to_string),
            ry: 0.0,
            part: None,
            hidden: false,
        }
    }

    fn def(boxes: Vec<BoxDef>, parts: Vec<PartDef>) -> ModelDef {
        ModelDef {
            name: "t".into(),
            height: 1.0,
            box_count: boxes.len() as u32,
            scale: [1.0, 1.0, 1.0],
            parts,
            boxes,
            extras: vec![],
        }
    }

    /// Boxes of the same colour merge into one group; a `split` name gets its own.
    #[test]
    fn groups_merge_by_material_and_split_by_name() {
        let d = def(
            vec![
                boxed(None, 0x112233, 0.0),
                boxed(None, 0x112233, 1.0),
                boxed(Some("glass"), 0x112233, 2.0),
                boxed(None, 0x445566, 3.0),
            ],
            vec![],
        );
        let groups = build_model(&d, &["glass"]);
        assert_eq!(groups.len(), 3);
        let glass = groups
            .iter()
            .find(|g| g.name.as_deref() == Some("glass"))
            .expect("glass group");
        assert_eq!(glass.mesh.count_vertices(), 24, "one cuboid");
        let merged = groups
            .iter()
            .find(|g| g.name.is_none() && g.color == 0x112233)
            .expect("merged group");
        assert_eq!(merged.mesh.count_vertices(), 48, "two cuboids merged");
    }

    /// A hidden box is left out (three's `visible = false`).
    #[test]
    fn hidden_boxes_are_skipped() {
        let mut b = boxed(None, 1, 0.0);
        b.hidden = true;
        assert!(build_model(&def(vec![b], vec![]), &[]).is_empty());
    }

    /// A part's pivot moves its boxes; `y` is the box bottom, so a 1-unit box at `y = 0` is centred
    /// at 0.5.
    #[test]
    fn part_pivot_offsets_boxes() {
        let mut b = boxed(None, 1, 0.0);
        b.part = Some("upper".into());
        let d = def(
            vec![b],
            vec![PartDef {
                name: "upper".into(),
                parent: None,
                pivot: [0.0, 2.0, 0.0],
                rotation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
            }],
        );
        let groups = build_model(&d, &[]);
        let pos = groups[0].mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        let ys: Vec<f32> = match pos {
            bevy::mesh::VertexAttributeValues::Float32x3(v) => v.iter().map(|p| p[1]).collect(),
            _ => panic!("positions"),
        };
        let min = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!((min - 2.0).abs() < 1e-5, "min {min}");
        assert!((max - 3.0).abs() < 1e-5, "max {max}");
    }
}
