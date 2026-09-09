//! Throwaway visual check for the hub lane (PHASE2_LANES §0).
//!
//! The world lane owns the real camera, and is being written in parallel, so `cargo run -p
//! undercroft` renders black for this lane. This example is the smallest app that shows the hub:
//! `DefaultPlugins` + `SkeletonPlugin` + the asset loader + `hub::plugin`, a fixed camera above the
//! brazier looking down at ~45°, and a dim ambient light. Everything else is driven by the standard
//! `UNDERCROFT_SCRIPT` debug script, e.g.
//!
//! ```sh
//! UNDERCROFT_SCRIPT="wait 1; begin; wait 3; screenshot /tmp/hub.png; wait 1; quit" \
//!   cargo run -p undercroft --features dev --example hub_preview
//! ```
//!
//! Delete this file once the world lane's camera has landed.

use bevy::prelude::*;
use undercroft::assets::asset_plugin;
use undercroft::{assets, debug, hub, SkeletonPlugin};

/// Where the camera sits: above and south of the camp, looking down at 45°. The hub grid is
/// 25 × 13 at `HUB_OX = 60`, so it spans x 60…85, z 0…13 and its middle is (72.5, 6.5).
const EYE: Vec3 = Vec3::new(72.5, 17.0, 23.5);
const LOOK_AT: Vec3 = Vec3::new(72.5, 0.0, 6.5);

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Transform::from_translation(EYE).looking_at(LOOK_AT, Vec3::Y),
        // `world.js:applyHubWarmth` — the tier-4 hub ambient, as a component on the camera
        // (Bevy 0.19 makes `AmbientLight` a per-camera component).
        AmbientLight {
            color: Color::srgb_u8(0x4c, 0x36, 0x20),
            brightness: 400.0,
            affects_lightmapped_meshes: false,
        },
    ));
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "hub preview".into(),
                        resolution: (960u32, 600u32).into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(asset_plugin()),
        )
        .add_plugins((
            SkeletonPlugin,
            assets::plugin,
            debug::screenshot_plugin,
            hub::plugin,
        ))
        .add_systems(Startup, setup)
        .run();
}
