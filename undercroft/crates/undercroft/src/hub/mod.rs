//! Hub lane: the hub scene. Ports `hub.js` (great flame and its tiers, buildings, board papers,
//! embers, blessing glow), the `buildProps` / `buildSconces` / `setAlcoveTier` / hanging-lantern
//! portions of `world.js`, and the visual half of `npc.js` (resident, captive and follower meshes
//! with their idle / walk animation).
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).
//!
//! The lane reads `HubMapRes`, `HubRes`, `SaveRes`, `Npcs`, `Player`, `Clock` and `Game`, and is
//! the only writer of `HubMap::mask` (prop footprints, [`props::mark_prop_footprints`]).

use bevy::prelude::*;

pub mod buildings;
pub mod flame;
pub mod lights;
pub mod model;
pub mod npcs;
pub mod props;

/// `0xRRGGBB` (the colour form of every RON table) as a Bevy colour. The world lane exports the
/// same helper as `world::palette::rgb`; PHASE2_LANES §1 allows each lane a private copy named
/// `rgb_u32` during the parallel step, and asks the reviewer to dedupe them on merge.
pub fn rgb_u32(hex: u32) -> Color {
    Color::srgb_u8(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// The same colour as linear RGB, for `StandardMaterial::emissive`.
pub fn linear_u32(hex: u32) -> LinearRgba {
    LinearRgba::from(rgb_u32(hex))
}

/// `three.Color.multiplyScalar` — scale the colour, leave the alpha alone (Bevy's componentwise
/// `Mul<f32>` would scale alpha too, and `emissive` is read as an opaque RGB triple).
pub fn scale_rgb(c: LinearRgba, k: f32) -> LinearRgba {
    LinearRgba::new(c.red * k, c.green * k, c.blue * k, 1.0)
}

/// `three.Color.lerp` — component-wise mix in the (linear) working colour space, which is what
/// three does with colour management on (`models.js:flameBase setTier`).
pub fn lerp_linear(a: LinearRgba, b: LinearRgba, t: f32) -> LinearRgba {
    let t = t.clamp(0.0, 1.0);
    LinearRgba::new(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        1.0,
    )
}

/// `three.PointLight.intensity` → Bevy lumens.
///
/// three's forward Lambert uses `intensity / d²` with `decay = 2`; Bevy's clustered PBR wants
/// luminous power in lumens and divides by `4π` internally, then applies the camera exposure. One
/// shared factor keeps the *relative* brightness of the flame, the lanterns and the sconces exactly
/// as the prototype had it; the absolute value is the only free parameter, tuned on the preview
/// example's default exposure. The world lane owns the real camera, so the reviewer may need to
/// rescale this single constant once the two land together.
pub const LUMENS_PER_JS_INTENSITY: f32 = 120_000.0;

/// Every hub system runs in this set, so one run condition gates them all.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HubSet;

/// The mesh/material collections exist. `UndercroftPlugin` is added by `headless.rs`'s asset-loader
/// test under `MinimalPlugins`, which has no renderer and therefore no `Assets<Mesh>`; without this
/// guard every lane system would fail parameter validation there.
pub fn render_assets_ready(
    meshes: Option<Res<Assets<Mesh>>>,
    mats: Option<Res<Assets<StandardMaterial>>>,
) -> bool {
    meshes.is_some() && mats.is_some()
}

/// Everything the hub lane draws.
pub fn plugin(app: &mut App) {
    app.init_resource::<model::BoxAssets>()
        .configure_sets(Update, HubSet.run_if(render_assets_ready))
        .add_plugins((
            props::plugin,
            buildings::plugin,
            flame::plugin,
            lights::plugin,
            npcs::plugin,
        ));
}
