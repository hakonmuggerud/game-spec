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
pub mod npcs;
pub mod props;

/// `three.PointLight.intensity` → Bevy lumens.
///
/// three's forward Lambert uses `intensity / d²` with `decay = 2`; Bevy's clustered PBR wants
/// luminous power in lumens and divides by `4π` internally, then applies the camera exposure. The
/// world camera's [`crate::world::palette::EXPOSURE_EV100`] is chosen so those two factors cancel
/// and a three.js candela value goes straight into `PointLight::intensity`, so this factor is 1.0
/// and must stay 1.0 while that exposure holds. It is kept as a named constant so the convention is
/// visible at the call sites.
pub const LUMENS_PER_JS_INTENSITY: f32 = 1.0;

/// Every hub system runs in this set, so one run condition gates them all.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HubSet;

/// Everything the hub lane draws.
pub fn plugin(app: &mut App) {
    app.init_resource::<crate::model::BoxAssets>()
        .configure_sets(Update, HubSet.run_if(crate::render_ready))
        .add_plugins((
            props::plugin,
            buildings::plugin,
            flame::plugin,
            lights::plugin,
            npcs::plugin,
        ));
}
