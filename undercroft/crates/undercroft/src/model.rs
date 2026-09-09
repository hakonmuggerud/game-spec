//! `models.js:build` / `finish` ported to Bevy entities: one [`ModelDef`] (recorded from a
//! `models.js` factory) becomes a small entity hierarchy under a root.
//!
//! During the parallel lane step the creatures and hub lanes each wrote their own copy of this
//! (PHASE2_LANES §1 "Models" told them to, on disjoint files); this is the merge of the two, and
//! the only one. It keeps the union of what both lanes need:
//!
//! - **per-instance materials** (creatures): pass `None` for the cache and every call gets fresh
//!   `StandardMaterial`s, because `hunter.js:syncMesh` writes `eyes[i].material.emissiveIntensity`
//!   per creature, exactly as the JS cloned its cached materials;
//! - **a shared cache** (the hub): pass `Some(&mut BoxAssets)` and the whole camp collapses onto
//!   one `Cuboid` per `(w, h, d)` and one material per `(color, emissive, emissive_k)`;
//! - `NotShadowCaster` on every box (shadows are off everywhere, PHASE2_LANES §1);
//! - `emissive_exposure_weight: 0.0` through [`crate::world::palette::emissive`], so the recorded
//!   `emissive_k` values transfer from three unchanged (see that module's docs);
//! - the `extras` ripple ring (the Drowner) and [`fallback_def`] (`hunter.js:fallbackModel`).
//!
//! The shape both lanes build:
//!
//! ```text
//! root (the caller's entity: its Transform and Visibility are never touched here)
//!  ├── part entity per `PartDef`  (`Name(part.name)`, Transform from pivot/rotation/scale,
//!  │    parented to its `parent` part or to the root)
//!  └── box entity per `BoxDef`    (`Cuboid` mesh, `Name` when the box has one, child of its
//!       `part` or of the root, translated so `BoxDef::y` is the box *bottom*)
//! ```
//!
//! Conventions carried over from `models.js` (see `undercroft_data::models`): the model origin is
//! at the feet, the front faces −Z, `BoxDef::y` is the bottom of the box (three's mesh centre is
//! `y + h / 2`), and `BoxDef::ry` yaws the box about its own centre.

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use undercroft_data::{BoxDef, ModelDef, PartDef};

use crate::world::palette;

/* ============================================================
Materials and meshes
============================================================ */

/// The "Lambert look" every lane uses (PHASE2_LANES §1): rough, non-reflective, lit, shadowless,
/// with the emissive term left display-referred so `emissive_k` means what it meant in three.
pub fn box_material(color: u32, emissive: Option<u32>, emissive_k: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: palette::rgb(color),
        emissive: emissive.map_or(LinearRgba::BLACK, |e| palette::emissive(e, emissive_k)),
        // Bevy's default, spelled out because it is load-bearing: `emissive` is added to the
        // shaded colour without the camera exposure, as three's `emissive × emissiveIntensity` was.
        emissive_exposure_weight: 0.0,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        unlit: false,
        ..default()
    }
}

/// Cuboid meshes and box materials shared across many models (the hub's camp).
///
/// Keyed by the values that actually differ: one mesh per `(w, h, d)`, one material per
/// `(color, emissive, emissive_k)`. Creatures deliberately do *not* use it — see the module docs.
#[derive(Resource, Default)]
pub struct BoxAssets {
    meshes: HashMap<[u32; 3], Handle<Mesh>>,
    mats: HashMap<(u32, u32, u32), Handle<StandardMaterial>>,
}

impl BoxAssets {
    /// A `w × h × d` [`Cuboid`], cached.
    pub fn mesh(&mut self, meshes: &mut Assets<Mesh>, w: f32, h: f32, d: f32) -> Handle<Mesh> {
        self.meshes
            .entry([w.to_bits(), h.to_bits(), d.to_bits()])
            .or_insert_with(|| meshes.add(Cuboid::new(w, h, d)))
            .clone()
    }

    /// The Lambert-flat material for a box colour, cached.
    pub fn material(
        &mut self,
        mats: &mut Assets<StandardMaterial>,
        color: u32,
        emissive: Option<u32>,
        emissive_k: f32,
    ) -> Handle<StandardMaterial> {
        self.mats
            .entry((color, emissive.unwrap_or(u32::MAX), emissive_k.to_bits()))
            .or_insert_with(|| mats.add(box_material(color, emissive, emissive_k)))
            .clone()
    }
}

/// Either the shared cache or "give me my own handles", as the two lanes need.
///
/// A plain `Option<&mut BoxAssets>` would do, but the two calls below would then have to thread the
/// same borrow through every box; this keeps one match in one place.
enum Cache<'a> {
    Shared(&'a mut BoxAssets),
    PerInstance {
        meshes: HashMap<[u32; 3], Handle<Mesh>>,
        mats: HashMap<(u32, u32, u32), Handle<StandardMaterial>>,
    },
}

impl Cache<'_> {
    fn get(
        &mut self,
        meshes: &mut Assets<Mesh>,
        mats: &mut Assets<StandardMaterial>,
        b: &BoxDef,
    ) -> (Handle<Mesh>, Handle<StandardMaterial>) {
        match self {
            Cache::Shared(a) => (
                a.mesh(meshes, b.w, b.h, b.d),
                a.material(mats, b.color, b.emissive, b.emissive_k),
            ),
            // Within one model the handles are still shared; across models they are not, which is
            // what "per-instance" means here (`hunter.js` cloned per creature, not per box).
            Cache::PerInstance {
                meshes: mc,
                mats: tc,
            } => (
                mc.entry([b.w.to_bits(), b.h.to_bits(), b.d.to_bits()])
                    .or_insert_with(|| meshes.add(Cuboid::new(b.w, b.h, b.d)))
                    .clone(),
                tc.entry((
                    b.color,
                    b.emissive.unwrap_or(u32::MAX),
                    b.emissive_k.to_bits(),
                ))
                .or_insert_with(|| mats.add(box_material(b.color, b.emissive, b.emissive_k)))
                .clone(),
            ),
        }
    }
}

/* ============================================================
What a build produced
============================================================ */

/// One animation pivot group (`PartDef`) as an entity, with the build-time transform kept so the
/// animation can offset it (`hunter.js:bruteAnim` moves `body.position.x` from its pivot).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartRef {
    pub entity: Entity,
    /// `PartDef::pivot` — the local translation the model was built with.
    pub pivot: Vec3,
    /// `PartDef::rotation` — the local rotation the model was built with.
    pub rotation: Quat,
}

/// One box as an entity, with its material and the colours the animation edits
/// (`material.emissiveIntensity = k`, `material.color.set(...)`).
#[derive(Debug, Clone, PartialEq)]
pub struct BoxRef {
    pub entity: Entity,
    pub material: Handle<StandardMaterial>,
    /// `0xRRGGBB` of `BoxDef::color`.
    pub color: u32,
    /// `0xRRGGBB` of `BoxDef::emissive`, if the box glows.
    pub emissive: Option<u32>,
}

/// What [`spawn_model_under`] built: the handles both lanes' animation steps need.
#[derive(Debug, Clone)]
pub struct ModelEntities {
    /// The entity everything hangs off (the caller's, or the one [`spawn_model`] made).
    pub root: Entity,
    /// `ModelDef::scale` (three's root `group.scale`; only `hunterFast` is not 1). [`spawn_model`]
    /// has already applied it to the root; a caller that owns the root applies it itself.
    pub scale: Vec3,
    /// Part entities by `PartDef::name`.
    pub parts: HashMap<String, PartRef>,
    /// Named box entities by `BoxDef::name`; the first box with a name wins, like `models.js:named`.
    pub named: HashMap<String, BoxRef>,
    /// Every box entity, in `ModelDef::boxes` order (plus any `extras`).
    pub boxes: Vec<Entity>,
    /// Every *named* box in box order, so a repeated name (the shrine's `book`) stays reachable.
    pub named_all: Vec<(String, Entity)>,
}

impl ModelEntities {
    /// An empty result rooted at `root`, for a caller that spawns its own boxes.
    pub fn empty(root: Entity) -> ModelEntities {
        ModelEntities {
            root,
            scale: Vec3::ONE,
            parts: HashMap::new(),
            named: HashMap::new(),
            boxes: Vec::new(),
            named_all: Vec::new(),
        }
    }

    /// The part, with its build-time pivot and rotation.
    pub fn part(&self, name: &str) -> Option<PartRef> {
        self.parts.get(name).copied()
    }

    /// Just the part's entity.
    pub fn part_entity(&self, name: &str) -> Option<Entity> {
        self.parts.get(name).map(|p| p.entity)
    }

    /// The first box with this name (`models.js:named`), with its material.
    pub fn named(&self, name: &str) -> Option<&BoxRef> {
        self.named.get(name)
    }

    /// Just the first-named box's entity.
    pub fn named_entity(&self, name: &str) -> Option<Entity> {
        self.named.get(name).map(|b| b.entity)
    }

    /// Every box with this name, in box order.
    pub fn all_named(&self, name: &str) -> Vec<Entity> {
        self.named_all
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, e)| *e)
            .collect()
    }
}

/* ============================================================
Building
============================================================ */

/// `models.js:box` placement: the recorded `y` is the box bottom, three's mesh centre is
/// `y + h / 2`, and `ry` yaws the box about its own centre.
pub fn box_transform(b: &BoxDef) -> Transform {
    Transform {
        translation: Vec3::new(b.x, b.y + b.h * 0.5, b.z),
        rotation: Quat::from_rotation_y(b.ry),
        scale: Vec3::ONE,
    }
}

/// `mesh.visible === false` at build time (the false light's eyes while LIT).
fn visibility(hidden: bool) -> Visibility {
    if hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    }
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
    let mut cache = Cache::Shared(assets);
    spawn_one_box(commands, &mut cache, meshes, mats, parent, b).entity
}

/// The shared body of [`spawn_box`] and the box loop of [`spawn_model_under`].
fn spawn_one_box(
    commands: &mut Commands,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    parent: Entity,
    b: &BoxDef,
) -> BoxRef {
    let (mesh, material) = cache.get(meshes, mats, b);
    let mut e = commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material.clone()),
        box_transform(b),
        visibility(b.hidden),
        NotShadowCaster,
        ChildOf(parent),
    ));
    if let Some(name) = b.name.as_deref() {
        e.insert(Name::new(name.to_string()));
    }
    BoxRef {
        entity: e.id(),
        material,
        color: b.color,
        emissive: b.emissive,
    }
}

/// Build `def` under a fresh root placed at `at` (the model's own `scale` multiplies the caller's).
///
/// `assets` is the hub's shared [`BoxAssets`]; pass `None` for per-instance materials.
pub fn spawn_model(
    commands: &mut Commands,
    assets: Option<&mut BoxAssets>,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    def: &ModelDef,
    at: Transform,
) -> ModelEntities {
    let mut root_t = at;
    root_t.scale *= Vec3::from(def.scale);
    let root = commands
        .spawn((root_t, Visibility::Inherited, Name::new(def.name.clone())))
        .id();
    spawn_model_under(commands, assets, meshes, mats, def, root)
}

/// Build `def` as children of an existing `root`; the root's own `Transform` / `Visibility` are
/// left exactly as the caller made them (a creature root is driven by `creatures::sync`).
pub fn spawn_model_under(
    commands: &mut Commands,
    assets: Option<&mut BoxAssets>,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    def: &ModelDef,
    root: Entity,
) -> ModelEntities {
    let mut out = ModelEntities {
        scale: Vec3::from(def.scale),
        ..ModelEntities::empty(root)
    };
    let mut cache = match assets {
        Some(a) => Cache::Shared(a),
        None => Cache::PerInstance {
            meshes: HashMap::new(),
            mats: HashMap::new(),
        },
    };
    spawn_parts(commands, def, root, &mut out);
    for b in &def.boxes {
        let parent = b
            .part
            .as_deref()
            .and_then(|p| out.parts.get(p))
            .map_or(root, |p| p.entity);
        let r = spawn_one_box(commands, &mut cache, meshes, mats, parent, b);
        if let Some(name) = b.name.as_deref() {
            out.named.entry(name.to_string()).or_insert(r.clone());
            out.named_all.push((name.to_string(), r.entity));
        }
        out.boxes.push(r.entity);
    }
    spawn_extras(commands, meshes, mats, def, root, &mut out);
    out
}

/// One entity per [`PartDef`], parented to its `parent` part (or the root). The export lists
/// parents before children, but the loop does not rely on it: it repeats until nothing is left to
/// place and drops parts whose parent is missing (with a warning), rather than looping forever.
fn spawn_parts(commands: &mut Commands, def: &ModelDef, root: Entity, out: &mut ModelEntities) {
    let mut pending: Vec<&PartDef> = def.parts.iter().collect();
    while !pending.is_empty() {
        let mut progress = false;
        pending.retain(|part| {
            let parent = match part.parent.as_deref() {
                None => Some(root),
                Some(p) => out.parts.get(p).map(|r| r.entity),
            };
            let Some(parent) = parent else {
                return true; // parent not built yet
            };
            let pivot = Vec3::from(part.pivot);
            let rotation = Quat::from_euler(
                EulerRot::XYZ,
                part.rotation[0],
                part.rotation[1],
                part.rotation[2],
            );
            let entity = commands
                .spawn((
                    Name::new(part.name.clone()),
                    Transform {
                        translation: pivot,
                        rotation,
                        scale: Vec3::from(part.scale),
                    },
                    Visibility::Inherited,
                    ChildOf(parent),
                ))
                .id();
            out.parts.insert(
                part.name.clone(),
                PartRef {
                    entity,
                    pivot,
                    rotation,
                },
            );
            progress = true;
            false
        });
        if !progress {
            warn!(
                "models: {} has parts with an unknown parent: {:?}",
                def.name,
                pending.iter().map(|p| &p.name).collect::<Vec<_>>()
            );
            break;
        }
    }
}

/// `ModelDef::extras` — the meshes the exporter could not record as boxes. Only the Drowner's
/// ripple ring matters here (`models.js:ripple`, a `RingGeometry(0.86, 1.0)` laid flat); the ember
/// burst's `THREE.Points` is `creatures/burst.rs` and the water sheet is the world lane's.
fn spawn_extras(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    def: &ModelDef,
    root: Entity,
    out: &mut ModelEntities,
) {
    for extra in &def.extras {
        if extra != "ripple" {
            continue;
        }
        let material = materials.add(StandardMaterial {
            // `models.js:ripple` — black, emissive 0x2a5a6a at k 0.4, double sided.
            double_sided: true,
            cull_mode: None,
            ..box_material(0x000000, Some(RIPPLE_COLOR), RIPPLE_K)
        });
        let entity = commands
            .spawn((
                Name::new("ripple"),
                Mesh3d(meshes.add(Annulus::new(0.86, 1.0))),
                MeshMaterial3d(material.clone()),
                // `RingGeometry` is built in the XY plane and rotated flat by `rotateX(-PI/2)`.
                Transform {
                    translation: Vec3::new(0.0, 0.02, 0.0),
                    rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                    scale: Vec3::ONE,
                },
                Visibility::Inherited,
                NotShadowCaster,
                ChildOf(root),
            ))
            .id();
        out.named.insert(
            "ripple".to_string(),
            BoxRef {
                entity,
                material,
                color: 0x000000,
                emissive: Some(RIPPLE_COLOR),
            },
        );
        out.named_all.push(("ripple".to_string(), entity));
        out.boxes.push(entity);
    }
}

/// `models.js:ripple` ring colour (`CREATURE.drowner.ripple.color` carries the same value).
pub const RIPPLE_COLOR: u32 = 0x2a5a6a;
/// `models.js:ripple` build-time emissive intensity.
pub const RIPPLE_K: f32 = 0.4;

/// `hunter.js:fallbackModel` (~line 80) — the coloured box with two eyes a profile gets when
/// `models.js` has no factory for it, so the FSM and the tests never depend on the model. Returned
/// as a [`ModelDef`] so it goes through exactly the same build path.
pub fn fallback_def(name: &str, color: u32, size: (f32, f32), eye_color: u32) -> ModelDef {
    let (w, ht) = size;
    let eye = |x: f32| BoxDef {
        x,
        y: ht * 0.85,
        z: -w * 0.36,
        w: 0.08,
        h: 0.05,
        d: 0.05,
        color: 0x000000,
        emissive: Some(eye_color),
        emissive_k: 0.4,
        name: None,
        ry: 0.0,
        part: None,
        hidden: false,
    };
    ModelDef {
        name: name.to_string(),
        height: ht,
        box_count: 3,
        scale: [1.0, 1.0, 1.0],
        parts: Vec::new(),
        boxes: vec![
            BoxDef {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w,
                h: ht,
                d: w * 0.7,
                color,
                emissive: None,
                emissive_k: 0.0,
                name: Some("body".to_string()),
                ry: 0.0,
                part: None,
                hidden: false,
            },
            BoxDef {
                name: Some("eyeL".to_string()),
                ..eye(-w * 0.2)
            },
            BoxDef {
                name: Some("eyeR".to_string()),
                ..eye(w * 0.2)
            },
        ],
        extras: Vec::new(),
    }
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

    fn toy() -> ModelDef {
        ModelDef {
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
        }
    }

    /// A bare `World` is enough: building only needs `Commands` and the two asset collections, no
    /// render device and no plugins.
    fn build(def: &ModelDef, shared: bool) -> (World, ModelEntities, usize) {
        let mut world = World::new();
        let mut assets = BoxAssets::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut mats = Assets::<StandardMaterial>::default();
        let out = {
            let mut commands = world.commands();
            spawn_model(
                &mut commands,
                shared.then_some(&mut assets),
                &mut meshes,
                &mut mats,
                def,
                Transform::IDENTITY,
            )
        };
        world.flush();
        let n = mats.len();
        (world, out, n)
    }

    #[test]
    fn box_bottom_becomes_the_mesh_centre() {
        let b = plain_box("torso", None);
        assert_eq!(box_transform(&b).translation, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn parts_and_boxes_are_named_and_parented() {
        let def = toy();
        let (world, out, _) = build(&def, true);
        let fire = out.part_entity("fire").expect("fire part");
        assert_eq!(
            world.get::<Name>(fire).map(|n| n.as_str().to_string()),
            Some("fire".to_string())
        );
        assert_eq!(world.get::<ChildOf>(fire).map(|c| c.0), Some(out.root));
        let tongue = out.named_entity("tongue0").expect("tongue0 box");
        assert_eq!(
            world.get::<ChildOf>(tongue).map(|c| c.0),
            Some(fire),
            "a box with `part: fire` hangs off the fire pivot"
        );
        let base = out.named_entity("base").expect("base box");
        assert_eq!(world.get::<ChildOf>(base).map(|c| c.0), Some(out.root));
        assert_eq!(out.boxes.len(), 2);
        assert_eq!(out.all_named("base"), vec![base]);
        // shadows are off everywhere (PHASE2_LANES §1)
        assert!(world.get::<NotShadowCaster>(base).is_some());
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

    /// Two builds of the same model share materials through [`BoxAssets`] and do *not* without it
    /// — the difference the creatures lane depends on (per-creature eye emissive).
    #[test]
    fn the_cache_is_what_makes_materials_shared_between_models() {
        let def = toy();
        let mut world = World::new();
        let mut assets = BoxAssets::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut mats = Assets::<StandardMaterial>::default();
        {
            let mut commands = world.commands();
            for _ in 0..2 {
                spawn_model(
                    &mut commands,
                    Some(&mut assets),
                    &mut meshes,
                    &mut mats,
                    &def,
                    Transform::IDENTITY,
                );
            }
        }
        assert_eq!(mats.len(), 1, "one colour, one material, two models");
        {
            let mut commands = world.commands();
            for _ in 0..2 {
                spawn_model(
                    &mut commands,
                    None,
                    &mut meshes,
                    &mut mats,
                    &def,
                    Transform::IDENTITY,
                );
            }
        }
        assert_eq!(mats.len(), 3, "per-instance: one fresh material per model");
    }

    /// The `extras` ripple ring lands as a named box with its own material.
    #[test]
    fn the_ripple_extra_is_built_and_named() {
        let mut def = toy();
        def.extras = vec!["ripple".to_string()];
        let (world, out, _) = build(&def, true);
        let ring = out.named("ripple").expect("ripple");
        assert_eq!(
            world.get::<ChildOf>(ring.entity).map(|c| c.0),
            Some(out.root)
        );
        assert_eq!(ring.emissive, Some(RIPPLE_COLOR));
    }

    #[test]
    fn the_fallback_model_is_a_body_and_two_eyes() {
        let def = fallback_def("brute", 0x223344, (0.9, 2.0), 0xff6a20);
        let (_, out, _) = build(&def, false);
        assert_eq!(out.boxes.len(), 3);
        assert!(out.named("body").is_some());
        assert_eq!(out.named("eyeL").and_then(|b| b.emissive), Some(0xff6a20));
    }
}
