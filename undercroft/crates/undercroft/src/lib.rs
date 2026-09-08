//! The Undercroft, Bevy shell. The simulation lives in `undercroft_sim` (functional core); this
//! crate owns entities, time and I/O (ECS shell) and calls the sim from `FixedUpdate`.
//!
//! Plugin layout:
//!
//! - [`SkeletonPlugin`] — states, shared resources, messages, debug queue and the fixed tick.
//!   Everything that has no window, no renderer and no asset server. Added by the real app *and*
//!   by the headless harness, so tests exercise the same systems.
//! - [`UndercroftPlugin`] — `SkeletonPlugin` plus the `GameDataAsset` loader and the
//!   `Loading → Title` handoff. Needs `AssetPlugin`, so only `main.rs` adds it.
//!
//! Stage 2 adds `run.rs` (the `main.js` lifecycle) and `player.rs` (movement, lamp, interact).

pub mod assets;
pub mod debug;
#[cfg(not(target_arch = "wasm32"))]
pub mod headless;
pub mod messages;
pub mod player;
pub mod resources;
pub mod run;
pub mod state;
pub mod tick;

pub use assets::{GameDataAsset, GameDataHandle};
pub use debug::{DebugCommand, DebugQueue, DebugSet};
pub use messages::{emit, EventLog, SimMessage};
pub use resources::{
    Fade, FadeMode, Game, HubMapRes, LampRes, MoveIntent, Npcs, PendingTransition, Player,
    PlayerViewRes, RngRes, SaveRes, SaveStore, SaveStoreRes, Spawns, Zone, ZoneRes,
};
pub use state::{GameMode, MenuKind, Mode, PrevMode};
pub use tick::{Clock, SimSet, TickCount, TICK_DT, TICK_HZ};

use bevy::prelude::*;

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
                tick::plugin,
                resources::plugin,
                messages::plugin,
                debug::plugin,
                run::plugin,
                player::plugin,
            ));
    }
}

/// The full app: [`SkeletonPlugin`] plus asynchronous data loading. Requires `AssetPlugin`
/// (`DefaultPlugins`).
pub struct UndercroftPlugin;

impl Plugin for UndercroftPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((SkeletonPlugin, assets::plugin));
    }
}
