//! The Undercroft, Bevy shell. The simulation lives in `undercroft_sim` (functional core); this
//! crate owns entities, time and I/O (ECS shell) and calls the sim from `FixedUpdate`.
//!
//! Plugin layout:
//!
//! - [`SkeletonPlugin`] — states, shared resources, messages, debug queue and the fixed tick.
//!   Everything that has no window, no renderer and no asset server. Added by the real app *and*
//!   by the headless harness, so tests exercise the same systems.
//! - [`UndercroftPlugin`] — `SkeletonPlugin` plus the `GameDataAsset` loader, the
//!   `Loading → Title` handoff, and the five rendering/UI lanes (`audio`, `creatures`, `hub`,
//!   `ui`, `world`). Needs `AssetPlugin` and a renderer, so only `main.rs` adds it; the lanes stay
//!   out of the headless harness on purpose.
//!
//! Stage 2 adds `run.rs` (the `main.js` lifecycle) and `player.rs` (movement, lamp, interact).

pub mod assets;
pub mod audio;
pub mod creatures;
pub mod debug;
#[cfg(not(target_arch = "wasm32"))]
pub mod headless;
pub mod hub;
pub mod messages;
pub mod model;
pub mod player;
pub mod resources;
pub mod run;
pub mod state;
pub mod tick;
pub mod ui;
pub mod world;

pub use assets::{GameDataAsset, GameDataHandle};
pub use debug::{
    parse_script, DebugCommand, DebugQueue, DebugScript, DebugSet, ScriptStep, TakeScreenshot,
};
pub use messages::{emit, EventLog, SimMessage};
pub use model::ModelEntities;
pub use resources::{
    Fade, FadeMode, Game, HubMapRes, LampRes, MoveIntent, Npcs, PendingTransition, Player,
    PlayerViewRes, RngRes, SaveRes, SaveStore, SaveStoreRes, Spawns, Zone, ZoneRes,
};
pub use state::{GameMode, MenuKind, Mode, PrevMode};
pub use tick::{Clock, SimSet, TickCount, TICK_DT, TICK_HZ};

use bevy::prelude::*;

/// Run condition: the mesh and material collections exist, so a lane may build geometry.
///
/// `UndercroftPlugin` is also added without a renderer — `headless::tests::
/// the_asset_loader_reads_the_data_directory` builds it on `MinimalPlugins` + `AssetPlugin` — and
/// there `Assets<Mesh>` does not exist, so a lane system asking for it would fail parameter
/// validation. Every lane wrote its own version of this during the parallel step; this is the one.
pub fn render_ready(
    meshes: Option<Res<Assets<Mesh>>>,
    mats: Option<Res<Assets<StandardMaterial>>>,
) -> bool {
    meshes.is_some() && mats.is_some()
}

/// The build-time half of [`render_ready`]: this `App` has a renderer at all. A lane that needs
/// renderer-*owned* resources (`ClearColor`, the window, cameras) cannot be gated by a run
/// condition — those resources are missing, not empty — so it stands down at `Plugin::build` time.
pub fn has_renderer(app: &App) -> bool {
    app.is_plugin_added::<bevy::render::RenderPlugin>()
}

/// Everything that runs identically in the window, on the web and headless.
///
/// It deliberately does *not* register the state itself with a value: [`UndercroftPlugin`] starts in
/// [`GameMode::Loading`], the harness in [`GameMode::Title`]. Add the state before this plugin, or
/// let it default to `Loading`.
pub struct SkeletonPlugin;

impl Plugin for SkeletonPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<State<GameMode>>() {
            app.init_state::<GameMode>();
        }
        app.init_resource::<PrevMode>()
            .init_resource::<MenuKind>()
            .add_plugins((
                state::plugin,
                tick::plugin,
                resources::plugin,
                messages::plugin,
                debug::plugin,
                run::plugin,
                player::plugin,
            ));
    }
}

/// The full app: [`SkeletonPlugin`] plus asynchronous data loading and the five rendering/UI
/// lanes. Requires `AssetPlugin` and a renderer (`DefaultPlugins`); the lane plugins are added
/// here — and only here — because they need a window and the renderer, and must stay out of the
/// headless harness (`crate::headless`), which only ever adds [`SkeletonPlugin`].
pub struct UndercroftPlugin;

impl Plugin for UndercroftPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            SkeletonPlugin,
            assets::plugin,
            debug::screenshot_plugin,
            audio::plugin,
            creatures::plugin,
            hub::plugin,
            ui::plugin,
            world::plugin,
        ));
    }
}
