//! Throwaway harness for the ui lane's visual check: the real app minus the world lane's camera.
//!
//! Until the world lane lands there is no camera in the app at all, so `bevy_ui` has nothing to draw
//! on. This example adds the one thing world will own — a 2D camera marked [`IsDefaultUiCamera`] —
//! and nothing else, so what it shows is exactly what the lane draws. Delete it once world is merged.
//!
//! ```sh
//! Xvfb :93 -screen 0 1280x800x24 &
//! DISPLAY=:93 WINIT_UNIX_BACKEND=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!   WGPU_BACKEND=vulkan UNDERCROFT_SCRIPT="wait 1; screenshot /tmp/title.png; quit" \
//!   cargo run -p undercroft --features dev --example ui_preview
//! ```

use bevy::prelude::*;
use bevy::ui::IsDefaultUiCamera;
use undercroft::assets::asset_plugin;
use undercroft::UndercroftPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "The Undercroft — ui preview".into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(asset_plugin()),
        )
        // `UndercroftPlugin` is `SkeletonPlugin` + the asset loader + `debug::screenshot_plugin`
        // (which reads `UNDERCROFT_SCRIPT`) + the five lane plugins; four of them are still stubs.
        .add_plugins(UndercroftPlugin)
        .add_systems(Startup, spawn_ui_camera)
        .run();
}

/// What the world lane will spawn: the 2D camera that presents the low-res 3D image and carries the
/// default UI target. Nothing in `src/ui/` may spawn a camera itself.
fn spawn_ui_camera(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera));
}
