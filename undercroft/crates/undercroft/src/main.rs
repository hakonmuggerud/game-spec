//! Window, plugins, run. Everything else lives in the library (`undercroft::*`).
//!
//! `GameMode::Title` shows the ui lane's main menu over the world lane's view of the hub, as
//! `main.js` does; the Phase 0 placeholder scene (a lit spinning cube on its own `Camera3d`) was
//! removed when the lanes landed — only the world lane spawns cameras (PHASE2_LANES §1).

use bevy::prelude::*;
use undercroft::assets::asset_plugin;
use undercroft::UndercroftPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "The Undercroft".into(),
                        // On the web, size the canvas to the page and let CSS own it.
                        fit_canvas_to_parent: true,
                        #[cfg(target_arch = "wasm32")]
                        canvas: Some("#undercroft".into()),
                        ..default()
                    }),
                    ..default()
                })
                .set(asset_plugin()),
        )
        .add_plugins(UndercroftPlugin)
        .run();
}
