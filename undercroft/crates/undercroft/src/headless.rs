//! The headless test harness (PHASE2_SKELETON §5). `MinimalPlugins` + `StatesPlugin` + the asset
//! server, with `assets/data/` loaded synchronously off disk and inserted straight into
//! `Assets<GameDataAsset>`, so no frame is spent in `GameMode::Loading` and the very same
//! [`SkeletonPlugin`] systems run as in the real app.
//!
//! It replaces the prototype's Playwright harness (`window.__game.actions` driven from a headless
//! browser): [`send`] queues a [`DebugCommand`], [`step`] advances an exact number of fixed ticks,
//! [`log`] reads the [`EventLog`]. Native only — it reads the workspace's `assets/data` with
//! `std::fs`.

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

use undercroft_data::GameData;
use undercroft_sim::SimRng;

use crate::assets::{asset_plugin, GameDataAsset, GameDataHandle};
use crate::debug::DebugCommand;
use crate::messages::EventLog;
use crate::resources::{MemoryStore, RngRes, SaveStore, SaveStoreRes};
use crate::state::GameMode;
use crate::tick::TICK_HZ;
use crate::{DebugQueue, SkeletonPlugin};

/// `assets/data/` loaded off the workspace, with the creature tuning derived.
pub fn workspace_asset() -> GameDataAsset {
    let data =
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data/ loads from disk");
    GameDataAsset::from_data(data).expect("config.ron builds the creature tuning")
}

/// A headless app in [`GameMode::Title`] with the data loaded and a fresh, private in-memory save
/// store — never the platform default (`FileStore`, a real `./undercroft-save.json` on native),
/// which would leak state between tests. Use [`headless_app_with_store`] to share a store on
/// purpose.
pub fn headless_app() -> App {
    build(None, None)
}

/// The same, with a fixed RNG seed so a command script is reproducible (HANDOFF §8).
pub fn headless_app_seeded(seed: u64) -> App {
    build(Some(seed), None)
}

/// The same, sharing a caller-owned save store — a `MemoryStore` clone lets one test bank in one
/// app and read the points back in another.
pub fn headless_app_with_store(store: impl SaveStore + Send + Sync + 'static) -> App {
    build(None, Some(SaveStoreRes::new(store)))
}

fn build(seed: Option<u64>, store: Option<SaveStoreRes>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_once()))
        .add_plugins(StatesPlugin)
        .add_plugins(AssetPlugin {
            // never watch files in tests, even when the `dev` feature is on
            watch_for_changes_override: Some(false),
            ..asset_plugin()
        })
        .init_asset::<GameDataAsset>();

    let handle = app
        .world_mut()
        .resource_mut::<Assets<GameDataAsset>>()
        .add(workspace_asset());
    app.insert_resource(GameDataHandle(handle));

    if let Some(seed) = seed {
        app.insert_resource(RngRes(SimRng::seed(seed)));
    }
    // Always install a store here, before `SkeletonPlugin` (`resources::plugin`) runs: it only
    // inserts `default_store()` — a `FileStore` natively — when nothing has claimed the resource
    // yet, and that file would carry state from one test into the next.
    app.insert_resource(store.unwrap_or_else(|| SaveStoreRes::new(MemoryStore::new())));
    // The harness skips `Loading`: the data is already there.
    app.insert_state(GameMode::Title);
    app.add_plugins(SkeletonPlugin);
    let dt = tick_duration(&app);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(dt));
    // One boot update: `Time<Real>` has no `last_update` yet, so this frame's delta is zero and no
    // fixed tick runs — it only flushes `Startup` and the initial state transition. Every later
    // `App::update` then advances exactly one fixed tick.
    app.update();
    app
}

/// Exactly one fixed tick of wall time. Taken from `Time<Fixed>` itself rather than recomputed:
/// `Duration::from_secs_f32(1.0 / 60.0)` is one nanosecond *shorter* than `Time::from_hz(60.0)`'s
/// timestep, which silently swallows one tick in every sixty.
fn tick_duration(app: &App) -> Duration {
    app.world().resource::<Time<Fixed>>().timestep()
}

/// Advance `round(secs * 60)` fixed ticks — one `App::update` per tick, with time supplied
/// manually so a test never depends on the wall clock.
pub fn step(app: &mut App, secs: f32) {
    let dt = tick_duration(app);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(dt));
    let ticks = (secs * TICK_HZ as f32).round().max(0.0) as u32;
    for _ in 0..ticks {
        app.update();
    }
}

/// Queue a debug command (`window.__game.actions.foo()`); it is handled on the next tick.
pub fn send(app: &mut App, cmd: DebugCommand) {
    app.world_mut().resource_mut::<DebugQueue>().push(cmd);
}

/// Everything emitted so far.
pub fn log(app: &App) -> &EventLog {
    app.world().resource::<EventLog>()
}

/// The current mode (`ctx.state.mode`).
pub fn mode(app: &App) -> GameMode {
    *app.world().resource::<State<GameMode>>().get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::SimMessage;
    use crate::resources::Game;
    use crate::tick::{SimSet, TickCount};
    use undercroft_sim::SimEvent;

    fn ticks(app: &App) -> u64 {
        app.world().resource::<TickCount>().0
    }

    #[test]
    fn boots_to_title_with_data_loaded() {
        let mut app = headless_app();
        step(&mut app, 1.0 / 60.0);
        assert_eq!(mode(&app), GameMode::Title);
        let asset = {
            let handle = app.world().resource::<GameDataHandle>().0.clone();
            let assets = app.world().resource::<Assets<GameDataAsset>>();
            assets.get(&handle).expect("data inserted").clone()
        };
        assert_eq!(asset.data.zones.len(), 4);
        assert_eq!(asset.data.hub_rows.len(), 13);
        assert!(asset.tuning.profiles().len() >= 5);
        assert_eq!(asset.tuning.hunter.lamp_r, 12.0);
    }

    #[test]
    fn game_system_param_sees_the_data() {
        fn check(game: Game) {
            assert_eq!(game.config().hub_ox, 60);
            assert_eq!(game.data().zones.len(), 4);
            assert_eq!(game.tuning().flash_range, game.config().cfg.flash_range);
        }
        let mut app = headless_app();
        app.add_systems(Update, check);
        step(&mut app, 1.0 / 60.0);
    }

    #[test]
    fn step_runs_exactly_one_fixed_tick_per_sixtieth() {
        let mut app = headless_app();
        step(&mut app, 1.0);
        assert_eq!(ticks(&app), 60);
        step(&mut app, 0.5);
        assert_eq!(ticks(&app), 90);
        step(&mut app, 1.0 / 60.0);
        assert_eq!(ticks(&app), 91);
        step(&mut app, 0.0);
        assert_eq!(ticks(&app), 91);
    }

    #[test]
    fn emitted_messages_reach_the_event_log() {
        fn emit_once(mut w: MessageWriter<SimMessage>, mut done: Local<bool>) {
            if !*done {
                *done = true;
                crate::messages::emit(&mut w, vec![SimEvent::UiClick, SimEvent::toast("hi")]);
            }
        }
        let mut app = headless_app();
        app.add_systems(FixedUpdate, emit_once.in_set(SimSet::Economy));
        step(&mut app, 0.1);
        // `run.rs`'s boot (the port of `main.js`'s init block) announces the flame tier on the
        // first tick, so this asserts the tail rather than the whole log.
        let names = log(&app).names();
        assert!(names.ends_with(&["uiClick", "toast"]), "{names:?}");
        assert_eq!(log(&app).count("toast"), 1);
        assert_eq!(
            log(&app).last("toast"),
            Some(&SimEvent::toast("hi")),
            "payloads are kept, not just names"
        );
        assert_eq!(log(&app).entries()[0].0, 1, "stamped with the fixed tick");
    }

    /// Whatever the stage-2 handlers leave behind is logged and dropped, and the app keeps
    /// stepping. (`Begin` used to stand in for "nobody handles this"; `run.rs` handles it now.)
    #[test]
    fn unhandled_debug_commands_are_dropped_and_the_app_keeps_running() {
        let mut app = headless_app();
        send(&mut app, DebugCommand::Flash);
        send(&mut app, DebugCommand::Key("KeyE".into()));
        step(&mut app, 1.0 / 60.0);
        assert!(app.world().resource::<DebugQueue>().is_empty());
        step(&mut app, 1.0);
        assert_eq!(ticks(&app), 61);
    }

    /// End-to-end check of the real asset path: `UndercroftPlugin` on a bare `App`, the loader
    /// reading `assets/data/game.gamedata.ron` and everything beside it, then `Loading -> Title`.
    #[test]
    fn the_asset_loader_reads_the_data_directory() {
        use bevy::app::ScheduleRunnerPlugin;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_once()))
            .add_plugins(StatesPlugin)
            .add_plugins(AssetPlugin {
                watch_for_changes_override: Some(false),
                ..asset_plugin()
            })
            .add_plugins(crate::UndercroftPlugin);
        assert_eq!(mode(&app), GameMode::Loading, "starts in Loading");
        for _ in 0..200 {
            app.update();
            if mode(&app) == GameMode::Title {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            mode(&app),
            GameMode::Title,
            "the data asset finished loading"
        );
        let handle = app.world().resource::<GameDataHandle>().0.clone();
        let assets = app.world().resource::<Assets<GameDataAsset>>();
        let asset = assets.get(&handle).expect("asset present");
        assert_eq!(asset.data.zones.len(), 4);
        assert_eq!(asset.data.config.cfg.lamp_dist, 11.0);
        assert_eq!(asset.data.hub_rows.len(), 13);
    }

    #[test]
    fn seeded_apps_share_a_random_stream() {
        let draw = |app: &mut App| {
            let mut rng = app.world_mut().resource_mut::<RngRes>();
            (0..4).map(|_| rng.0.unit()).collect::<Vec<_>>()
        };
        let mut a = headless_app_seeded(7);
        let mut b = headless_app_seeded(7);
        assert_eq!(draw(&mut a), draw(&mut b));
    }

    #[test]
    fn the_save_store_can_be_shared_between_apps() {
        use crate::resources::MemoryStore;
        let shared = MemoryStore::new();
        let app = headless_app_with_store(shared.clone());
        app.world()
            .resource::<SaveStoreRes>()
            .0
            .store("{\"points\":3}");
        assert_eq!(shared.peek().as_deref(), Some("{\"points\":3}"));
        let other = headless_app_with_store(shared.clone());
        assert_eq!(
            other.world().resource::<SaveStoreRes>().0.load().as_deref(),
            Some("{\"points\":3}")
        );
    }
}
