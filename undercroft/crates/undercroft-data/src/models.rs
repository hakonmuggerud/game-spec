//! Voxel-box models (`reference/prototype/src/models.js`): every `MODELS` factory and every `PROP_BOXES` box list, as
//! recorded by `tools/export/export.mjs` walking the built `THREE.Group`s.
//!
//! Conventions (`models.js` header): a model's origin is at its feet (y = 0 is the floor) and its front faces
//! −Z; `BoxDef::y` is the BOTTOM of the box (three's mesh centre minus `h / 2`); positions are local to the
//! box's `part` (an animation pivot group) or to the model root when `part` is `None`. A part's `pivot` and
//! `rotation` are relative to its `parent` part (or the root). Parts are named after the factory's `userData`
//! handles (`upper` = hunter upper body at the hips, `legs0`/`legs1` = warden/brute hip pivots, `head` /
//! `conePivot` = warden neck and visor, `jaw` = drowner / false light hinge, `body` = the group the AI sinks or
//! bobs, `fire` = the flame stack, `cart` = the tram cart); unnamed non-identity groups are `groupN`.
//!
//! Colours are `0xRRGGBB` as three reports them (`Color.getHex()`), jitter disabled. Material state is what
//! the factory leaves after construction: `flameBase` at tier 1, `board` with papers 1–3 locked, `falseLight`
//! LIT (its eyes `hidden`). `extras` lists meshes that are not boxes (the drowner's ripple ring, the water
//! tile's instanced sheet, the ember burst's points).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One box (`models.js:box` + `emissive` + `rotated`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoxDef {
    /// Centre x (local to `part`).
    pub x: f32,
    /// Bottom y.
    pub y: f32,
    /// Centre z.
    pub z: f32,
    pub w: f32,
    pub h: f32,
    pub d: f32,
    pub color: u32,
    pub emissive: Option<u32>,
    /// `emissiveIntensity` (0 when not emissive).
    pub emissive_k: f32,
    /// Mesh name (`eyeL`, `glass`, `torso` …) — the handles the animation code looks up.
    pub name: Option<String>,
    /// Yaw about the box's own centre (radians).
    pub ry: f32,
    /// Owning pivot part, or the root.
    pub part: Option<String>,
    /// `mesh.visible == false` at build time (false light eyes while LIT).
    pub hidden: bool,
}

/// An animation pivot group inside a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartDef {
    pub name: String,
    pub parent: Option<String>,
    /// Position relative to the parent part / root.
    pub pivot: [f32; 3],
    /// Euler XYZ rotation (radians) at build time.
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

/// One `MODELS[name]()` group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDef {
    pub name: String,
    /// `userData.height` — bounding-box height at build time.
    pub height: f32,
    /// `userData.boxes` — the factory's own count (instanced water counts as 1).
    pub box_count: u32,
    /// Root scale (`hunter` fast = `[1, 1.15, 1]`).
    pub scale: [f32; 3],
    pub parts: Vec<PartDef>,
    pub boxes: Vec<BoxDef>,
    pub extras: Vec<String>,
}

impl ModelDef {
    /// First box with this mesh name (`models.js:named`).
    pub fn named(&self, name: &str) -> Option<&BoxDef> {
        self.boxes.iter().find(|b| b.name.as_deref() == Some(name))
    }

    /// Part by name.
    pub fn part(&self, name: &str) -> Option<&PartDef> {
        self.parts.iter().find(|p| p.name == name)
    }
}

/// `models.js:MODELS` (minus the pure aliases `oil` / `rich`, plus `hunterFast`) and `PROP_BOXES`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTable {
    pub models: Vec<ModelDef>,
    /// Hub prop box lists (`rug`, `bench`, `bedroll`, `crate`, `crateStack`, `barrel`, `logPile`, `cookpot`,
    /// `bookshelf`, `herbRail`, `candleCluster`, `hangLantern`, `stool`), default options.
    pub props: BTreeMap<String, Vec<BoxDef>>,
}

impl ModelTable {
    /// Model by factory name.
    pub fn get(&self, name: &str) -> Option<&ModelDef> {
        self.models.iter().find(|m| m.name == name)
    }
}

/// `models.js:placeBoxes` — move a box list to `(x, z)` yawed by `ry` (three's `rotation.y` convention).
pub fn place_boxes(boxes: &[BoxDef], x: f32, z: f32, ry: f32) -> Vec<BoxDef> {
    let (s, c) = ry.sin_cos();
    boxes
        .iter()
        .map(|b| BoxDef {
            x: x + b.x * c + b.z * s,
            z: z - b.x * s + b.z * c,
            ry: b.ry + ry,
            ..b.clone()
        })
        .collect()
}
