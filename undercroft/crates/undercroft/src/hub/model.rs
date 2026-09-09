//! `models.js` factories → an entity hierarchy. The exporter recorded every factory as a
//! [`ModelDef`] (boxes + animation pivot parts, `undercroft-data/src/models.rs`); this module turns
//! one into a root entity with a named child per [`PartDef`] and one `Mesh3d` child per [`BoxDef`].
//!
//! PHASE2_LANES §1 "Models": the creatures lane writes an identical module in `creatures/model.rs`
//! (the two lanes must not share a file during the parallel step) and the reviewer merges them
//! afterwards. The contract both follow:
//!
//! - root entity: `Transform` (the caller's placement, times `ModelDef::scale`), `Visibility`,
//!   `Name` = the model name;
//! - one child per `PartDef`, `Name`d after the part, `Transform` from `pivot` / `rotation` (Euler
//!   XYZ) / `scale`, parented to its `parent` part or to the root;
//! - one child per `BoxDef`, parented to its `part` (or the root), a `Cuboid` mesh of
//!   `w × h × d` centred at `(x, y + h/2, z)` — `BoxDef::y` is the box *bottom* — yawed by `ry`,
//!   `Name`d after `BoxDef::name` when the exporter recorded one, hidden when `BoxDef::hidden`;
//! - one `StandardMaterial` per distinct `(color, emissive, emissive_k)`, cached in [`BoxAssets`],
//!   Lambert-flat as PHASE2_LANES §1 requires (`perceptual_roughness: 1.0`, `reflectance: 0.0`,
//!   shadows off).

use bevy::light::NotShadowCaster;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use undercroft_data::models::{BoxDef, ModelDef};

use super::{linear_u32, rgb_u32, scale_rgb};

/// Cuboid meshes and box materials, shared across every model in the hub.
///
/// Keyed by the values that actually differ, so the whole camp collapses onto a handful of
/// handles: one mesh per `(w, h, d)` and one material per `(color, emissive, emissive_k)`.
#[derive(Resource, Default)]
pub struct BoxAssets {
    meshes: HashMap<[u32; 3], Handle<Mesh>>,
    mats: HashMap<(u32, u32, u32), Handle<StandardMaterial>>,
}

impl BoxAssets {
    /// A `w × h × d` [`Cuboid`], cached.
    pub fn mesh(&mut self, meshes: &mut Assets<Mesh>, w: f32, h: f32, d: f32) -> Handle<Mesh> {
        let key = [w.to_bits(), h.to_bits(), d.to_bits()];
        self.meshes
            .entry(key)
            .or_insert_with(|| meshes.add(Cuboid::new(w, h, d)))
            .clone()
    }

    /// The Lambert-flat material for a box colour, cached.
    ///
    /// `emissive` is the `three` `MeshLambertMaterial.emissive` × `emissiveIntensity`; a box with
    /// none gets a plain lit material.
    pub fn material(
        &mut self,
        mats: &mut Assets<StandardMaterial>,
        color: u32,
        emissive: Option<u32>,
        emissive_k: f32,
    ) -> Handle<StandardMaterial> {
        let key = (color, emissive.unwrap_or(u32::MAX), emissive_k.to_bits());
        self.mats
            .entry(key)
            .or_insert_with(|| mats.add(box_material(color, emissive, emissive_k)))
            .clone()
    }
}

/// The material one box asks for (pure; the tests build it without a render device).
///
/// `emissive_exposure_weight: 0.0` (Bevy's default, spelled out because it is load-bearing here)
/// keeps the emissive term display-referred — `emissive` is added to the shaded colour without the
/// camera exposure, which is exactly what three's `MeshLambertMaterial.emissive ×
/// emissiveIntensity` did, so the recorded `emissive_k` values transfer unchanged.
pub fn box_material(color: u32, emissive: Option<u32>, emissive_k: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: rgb_u32(color),
        emissive: emissive
            .map(|e| scale_rgb(linear_u32(e), emissive_k))
            .unwrap_or(LinearRgba::BLACK),
        emissive_exposure_weight: 0.0,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        unlit: false,
        ..default()
    }
}

/// The local transform of one box: `BoxDef::y` is the bottom, so the mesh centre sits half a
/// height above it, and `ry` is a yaw about the box's own centre (`models.js:rotated`).
pub fn box_transform(b: &BoxDef) -> Transform {
    Transform::from_xyz(b.x, b.y + b.h / 2.0, b.z).with_rotation(Quat::from_rotation_y(b.ry))
}

/// Spawn one loose box (a merged prop's box, an ember, the board candle) as a child of `parent`.
pub fn spawn_box(
    commands: &mut Commands,
    assets: &mut BoxAssets,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    parent: Entity,
    b: &BoxDef,
) -> Entity {
    let mesh = assets.mesh(meshes, b.w, b.h, b.d);
    let mat = assets.material(mats, b.color, b.emissive, b.emissive_k);
    let mut e = commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        box_transform(b),
        if b.hidden {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        },
        NotShadowCaster,
        ChildOf(parent),
    ));
    if let Some(name) = b.name.as_deref() {
        e.insert(Name::new(name.to_string()));
    }
    e.id()
}

/// What [`spawn_model`] built: the root plus the entities of the named parts and boxes, so a
/// caller can animate `fire`, `head` or `armL` without a name query.
#[derive(Debug, Clone)]
pub struct SpawnedModel {
    pub root: Entity,
    /// `PartDef::name` → the pivot entity.
    pub parts: Vec<(String, Entity)>,
    /// `BoxDef::name` → the box entity, in box order (a name may repeat, e.g. `book`).
    pub boxes: Vec<(String, Entity)>,
}

impl SpawnedModel {
    /// The first part with this name.
    pub fn part(&self, name: &str) -> Option<Entity> {
        self.parts.iter().find(|(n, _)| n == name).map(|(_, e)| *e)
    }

    /// The first box with this name (`models.js:named`).
    pub fn named(&self, name: &str) -> Option<Entity> {
        self.boxes.iter().find(|(n, _)| n == name).map(|(_, e)| *e)
    }

    /// Every box with this name, in box order.
    pub fn all_named(&self, name: &str) -> Vec<Entity> {
        self.boxes
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, e)| *e)
            .collect()
    }
}

/// Build `def` under a fresh root entity placed at `at` (the model's own `scale` is applied on
/// top of the caller's). Returns the root and the named parts/boxes.
pub fn spawn_model(
    commands: &mut Commands,
    assets: &mut BoxAssets,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    def: &ModelDef,
    at: Transform,
) -> SpawnedModel {
    let mut root_t = at;
    root_t.scale *= Vec3::from(def.scale);
    let root = commands
        .spawn((root_t, Visibility::Inherited, Name::new(def.name.clone())))
        .id();
    spawn_model_under(commands, assets, meshes, mats, def, root)
}

/// Build `def` as children of an existing `root` (used when the caller owns the root entity).
pub fn spawn_model_under(
    commands: &mut Commands,
    assets: &mut BoxAssets,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    def: &ModelDef,
    root: Entity,
) -> SpawnedModel {
    let mut out = SpawnedModel {
        root,
        parts: Vec::new(),
        boxes: Vec::new(),
    };
    // Parts first, parents before children: the exporter writes them in creation order, but a
    // second pass costs nothing and makes the order irrelevant.
    let mut pending: Vec<usize> = (0..def.parts.len()).collect();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|&i| {
            let p = &def.parts[i];
            let parent = match p.parent.as_deref() {
                None => Some(root),
                Some(name) => out.part(name),
            };
            let Some(parent) = parent else {
                return true;
            };
            let t = Transform {
                translation: Vec3::from(p.pivot),
                rotation: Quat::from_euler(
                    EulerRot::XYZ,
                    p.rotation[0],
                    p.rotation[1],
                    p.rotation[2],
                ),
                scale: Vec3::from(p.scale),
            };
            let e = commands
                .spawn((
                    t,
                    Visibility::Inherited,
                    Name::new(p.name.clone()),
                    ChildOf(parent),
                ))
                .id();
            out.parts.push((p.name.clone(), e));
            false
        });
        if pending.len() == before {
            // A cycle or a dangling parent: hang the rest off the root rather than loop forever.
            for &i in &pending {
                warn!(
                    "model {}: part {:?} has an unknown parent {:?}",
                    def.name, def.parts[i].name, def.parts[i].parent
                );
            }
            break;
        }
    }
    for b in &def.boxes {
        let parent = b.part.as_deref().and_then(|n| out.part(n)).unwrap_or(root);
        let e = spawn_box(commands, assets, meshes, mats, parent, b);
        if let Some(n) = b.name.as_deref() {
            out.boxes.push((n.to_string(), e));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::models::PartDef;

    fn plain_box(name: &str, part: Option<&str>) -> BoxDef {
        BoxDef {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
            h: 2.0,
            d: 1.0,
            color: 0x808080,
            emissive: None,
            emissive_k: 0.0,
            name: Some(name.to_string()),
            ry: 0.0,
            part: part.map(str::to_string),
            hidden: false,
        }
    }

    /// A bare `World` is enough: `spawn_model` only needs `Commands` and the two asset
    /// collections, no render device and no plugins.
    fn build(def: &ModelDef) -> (World, SpawnedModel) {
        let mut world = World::new();
        let mut assets = BoxAssets::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut mats = Assets::<StandardMaterial>::default();
        let out = {
            let mut commands = world.commands();
            spawn_model(
                &mut commands,
                &mut assets,
                &mut meshes,
                &mut mats,
                def,
                Transform::IDENTITY,
            )
        };
        world.flush();
        (world, out)
    }

    #[test]
    fn box_bottom_becomes_the_mesh_centre() {
        let b = plain_box("torso", None);
        assert_eq!(box_transform(&b).translation, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn parts_and_boxes_are_named_and_parented() {
        let def = ModelDef {
            name: "toy".into(),
            height: 2.0,
            box_count: 2,
            scale: [1.0, 1.0, 1.0],
            parts: vec![PartDef {
                name: "fire".into(),
                parent: None,
                pivot: [0.0, 0.5, 0.0],
                rotation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
            }],
            boxes: vec![plain_box("base", None), plain_box("tongue0", Some("fire"))],
            extras: vec![],
        };
        let (world, out) = build(&def);
        let fire = out.part("fire").expect("fire part");
        assert_eq!(
            world.get::<Name>(fire).map(|n| n.as_str().to_string()),
            Some("fire".to_string())
        );
        assert_eq!(world.get::<ChildOf>(fire).map(|c| c.0), Some(out.root));
        let tongue = out.named("tongue0").expect("tongue0 box");
        assert_eq!(
            world.get::<ChildOf>(tongue).map(|c| c.0),
            Some(fire),
            "a box with `part: fire` hangs off the fire pivot"
        );
        let base = out.named("base").expect("base box");
        assert_eq!(world.get::<ChildOf>(base).map(|c| c.0), Some(out.root));
    }

    #[test]
    fn materials_are_shared_per_colour_triplet() {
        let mut assets = BoxAssets::default();
        let mut mats = Assets::<StandardMaterial>::default();
        let a = assets.material(&mut mats, 0x112233, None, 0.0);
        let b = assets.material(&mut mats, 0x112233, None, 0.0);
        let c = assets.material(&mut mats, 0x112233, Some(0xff0000), 1.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(mats.len(), 2);
    }
}
