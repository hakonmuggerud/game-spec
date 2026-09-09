//! `models.js:build` / `finish` ported to Bevy entities: one [`ModelDef`] (recorded from a
//! `models.js` factory) becomes a small entity hierarchy under a caller-owned root.
//!
//! The shape follows PHASE2_LANES §1 "Models" — the contract the hub lane duplicates in
//! `hub/model.rs` and the reviewer merges afterwards:
//!
//! ```text
//! root (caller's entity: Transform + Visibility, never touched here)
//!  ├── part entity per `PartDef`  (`Name(part.name)`, Transform from pivot/rotation/scale,
//!  │    parented to its `parent` part or to the root)
//!  └── box entity per `BoxDef`    (`Cuboid` mesh, `Name` when the box has one, child of its
//!       `part` or of the root, translated so `BoxDef::y` is the box *bottom*)
//! ```
//!
//! Conventions carried over from `models.js` (see `undercroft_data::models`): the model origin is
//! at the feet, the front faces −Z, `BoxDef::y` is the bottom of the box (three's mesh centre is
//! `y + h / 2`), `BoxDef::ry` yaws the box about its own centre, and one material is shared by
//! every box with the same `(color, emissive, emissive_k)`. Materials are built fresh per call
//! because the animation writes per-instance emissive intensities (`hunter.js:syncMesh` sets
//! `eyes[i].material.emissiveIntensity`, exactly as the JS cloned its cached materials).

use std::collections::HashMap;

use bevy::prelude::*;
use undercroft_data::{BoxDef, ModelDef, PartDef};

/// `0xRRGGBB` (the colour form every RON table uses) → a Bevy sRGB colour. PHASE2_LANES §1 lets
/// each lane keep a private copy of this helper during the parallel step; the reviewer dedupes it
/// against `world/palette.rs::rgb` on merge.
pub fn rgb_u32(c: u32) -> Color {
    crate::world::palette::rgb(c)
}

/// `models.js:emissive(box, color, k)` — three multiplies the emissive colour by
/// `emissiveIntensity`; Bevy's `StandardMaterial::emissive` is that product, in linear space.
pub fn emissive_rgba(color: u32, k: f32) -> LinearRgba {
    // alpha 0 = exposure weight 0: emissive stays unexposed, as in three (world::palette::emissive)
    crate::world::palette::emissive(color, k)
}

/// The "Lambert look" every lane uses (PHASE2_LANES §1): rough, non-reflective, lit.
pub fn lambert(color: u32, emissive: Option<u32>, emissive_k: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: rgb_u32(color),
        emissive: emissive.map_or(LinearRgba::BLACK, |e| emissive_rgba(e, emissive_k)),
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        ..default()
    }
}

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

/// One named box (`BoxDef::name`) as an entity, with its material and the *unit* emissive colour
/// the animation scales (`material.emissiveIntensity = k`).
#[derive(Debug, Clone, PartialEq)]
pub struct BoxRef {
    pub entity: Entity,
    pub material: Handle<StandardMaterial>,
    /// `0xRRGGBB` of `BoxDef::color`.
    pub color: u32,
    /// `0xRRGGBB` of `BoxDef::emissive`, if the box glows.
    pub emissive: Option<u32>,
}

/// What [`spawn_model`] built: the handles the animation step needs.
#[derive(Debug, Clone, Default)]
pub struct ModelEntities {
    /// `ModelDef::scale` (three's root `group.scale`; only `hunterFast` is not 1). The caller owns
    /// the root transform, so it applies this itself — `sync.rs` writes it every frame.
    pub scale: Vec3,
    /// Part entities by `PartDef::name`.
    pub parts: HashMap<String, PartRef>,
    /// Named box entities by `BoxDef::name`; the first box with a name wins, like `models.js:named`.
    pub named: HashMap<String, BoxRef>,
    /// Every box entity, in `ModelDef::boxes` order (plus any `extras`).
    pub boxes: Vec<Entity>,
}

impl ModelEntities {
    /// The part entity, if the model has one with that name.
    pub fn part(&self, name: &str) -> Option<PartRef> {
        self.parts.get(name).copied()
    }

    /// The named box, if the model has one.
    pub fn named(&self, name: &str) -> Option<&BoxRef> {
        self.named.get(name)
    }
}

/// Build `def` under `root`: `models.js`'s factory output as entities.
///
/// `root` keeps whatever `Transform` / `Visibility` the caller gave it (the creature root is driven
/// by `sync.rs`, a hub prop is placed once); only children are added. Meshes and materials are
/// deduplicated within this call: one `Cuboid` per distinct `(w, h, d)` and one `StandardMaterial`
/// per distinct `(color, emissive, emissive_k)`.
pub fn spawn_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    def: &ModelDef,
    root: Entity,
) -> ModelEntities {
    let mut out = ModelEntities {
        scale: Vec3::from(def.scale),
        ..default()
    };
    spawn_parts(commands, def, root, &mut out);

    let mut mesh_cache: HashMap<[u32; 3], Handle<Mesh>> = HashMap::new();
    let mut mat_cache: HashMap<(u32, u32, u32), Handle<StandardMaterial>> = HashMap::new();
    for b in &def.boxes {
        let mesh = mesh_cache
            .entry([b.w.to_bits(), b.h.to_bits(), b.d.to_bits()])
            .or_insert_with(|| meshes.add(Cuboid::new(b.w, b.h, b.d)))
            .clone();
        let material = mat_cache
            .entry((
                b.color,
                b.emissive.unwrap_or(u32::MAX),
                b.emissive_k.to_bits(),
            ))
            .or_insert_with(|| materials.add(lambert(b.color, b.emissive, b.emissive_k)))
            .clone();
        let parent = b
            .part
            .as_deref()
            .and_then(|p| out.parts.get(p))
            .map_or(root, |p| p.entity);
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                box_transform(b),
                visibility(b.hidden),
                ChildOf(parent),
            ))
            .id();
        if let Some(name) = b.name.as_deref() {
            commands.entity(entity).insert(Name::new(name.to_string()));
            out.named.entry(name.to_string()).or_insert(BoxRef {
                entity,
                material,
                color: b.color,
                emissive: b.emissive,
            });
        }
        out.boxes.push(entity);
    }
    spawn_extras(commands, meshes, materials, def, root, &mut out);
    out
}

/// `models.js:box` placement: the recorded `y` is the box bottom, three's mesh centre is
/// `y + h / 2`, and `ry` yaws the box about its own centre.
fn box_transform(b: &BoxDef) -> Transform {
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
/// ripple ring matters to this lane (`models.js:ripple`, a `RingGeometry(0.86, 1.0)` laid flat);
/// the ember burst's `THREE.Points` is `creatures/burst.rs` instead, and the water sheet is the
/// world lane's.
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
            ..lambert(0x000000, Some(RIPPLE_COLOR), RIPPLE_K)
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
        out.boxes.push(entity);
    }
}

/// `models.js:ripple` ring colour (`CREATURE.drowner.ripple.color` carries the same value).
pub const RIPPLE_COLOR: u32 = 0x2a5a6a;
/// `models.js:ripple` build-time emissive intensity.
pub const RIPPLE_K: f32 = 0.4;

/// `hunter.js:fallbackModel` (~line 80) — the coloured box with two eyes a profile gets when
/// `models.js` has no factory for it, so the FSM and the tests never depend on the model. Returned
/// as a [`ModelDef`] so it goes through exactly the same [`spawn_model`] path.
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
