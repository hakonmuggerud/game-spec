//! The fixed timestep and the order the simulation runs in. `main.js:animate` ran everything once
//! per rendered frame with a clamped `dt`; the port runs the sim at a fixed 60 Hz in `FixedUpdate`
//! so headless tests and the wasm build agree, and leaves rendering/interpolation to `Update`.
//!
//! The sim's own cadences (0.2 s creature sense tick, 0.3 s repath, 0.25 s follower and minimap)
//! are counters inside `undercroft_sim`; the shell only feeds them `dt = time.delta_secs()` of the
//! fixed clock.

use bevy::prelude::*;

/// Simulation rate (Hz). `Time<Fixed>` is set to this.
pub const TICK_HZ: f64 = 60.0;

/// One fixed tick in seconds.
pub const TICK_DT: f32 = 1.0 / 60.0;

/// The order every `FixedUpdate` system belongs to. Lanes put their systems in exactly one of
/// these; the whole set is chained, so `Debug` always sees the queue before `Input` writes intent
/// and `Fanout` always sees the messages the tick produced.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    /// `window.__game.actions` — drain [`crate::debug::DebugQueue`] (`main.js:660`).
    Debug,
    /// Keyboard/mouse → [`crate::resources::MoveIntent`]; empty in the skeleton (world lane).
    Input,
    /// `main.js:updatePlayer` / `updateLamp` (player lane).
    Player,
    /// `hunter.js` — `creature::update` (run lane).
    Creatures,
    /// `npc.js` — `follower.update_zone` / `update_hub` (run lane).
    Follower,
    /// `contracts.js:tick` (run lane).
    Contracts,
    /// `economy` / `endgame` per-tick work: source run, fade, tier checks (run lane).
    Economy,
    /// Mirror this tick's messages into [`crate::messages::EventLog`] and let listeners react.
    Fanout,
}

/// Fixed ticks since the app started, for [`crate::messages::EventLog`] stamps and cadence debug.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickCount(pub u64);

/// `main.js:53 state.time` — the game clock in seconds. `main.js:655` adds `dt` every frame in every
/// mode (title, hub, menus and the death screen included); the sim's timestamps (`Lantern.planted`,
/// `Hunter.busy_t`, blink phases, hub resident bob) are all relative to it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct Clock {
    pub time: f32,
}

/// Runs before every other `FixedUpdate` system.
fn advance_clock(time: Res<Time>, mut tick: ResMut<TickCount>, mut clock: ResMut<Clock>) {
    tick.0 = tick.0.wrapping_add(1);
    clock.time += time.delta_secs();
}

/// The 60 Hz clock, the `SimSet` chain and the run clock.
pub fn plugin(app: &mut App) {
    app.insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
        .init_resource::<TickCount>()
        .init_resource::<Clock>()
        .configure_sets(
            FixedUpdate,
            (
                SimSet::Debug,
                SimSet::Input,
                SimSet::Player,
                SimSet::Creatures,
                SimSet::Follower,
                SimSet::Contracts,
                SimSet::Economy,
                SimSet::Fanout,
            )
                .chain(),
        )
        .add_systems(FixedUpdate, advance_clock.before(SimSet::Debug));
}
