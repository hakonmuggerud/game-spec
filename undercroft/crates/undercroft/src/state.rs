//! Game modes. Mirrors `state.mode` in `main.js:53` (`'TITLE' | 'HUB' | 'ZONE' | 'DYING' | 'DEAD' |
//! 'MENU' | 'ENDING'`) plus a `Loading` mode the JS has no need for: the Bevy build reads its data
//! through the asset server, which is asynchronous.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// `main.js:53 state.mode`. `Loading` is the Bevy-only start mode (see the module docs).
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameMode {
    /// Waiting for `assets/data/game.gamedata.ron`; no JS equivalent.
    #[default]
    Loading,
    /// `'TITLE'`.
    Title,
    /// `'HUB'`.
    Hub,
    /// `'ZONE'`.
    Zone,
    /// `'DYING'` — the death camera, `state.dyingT` counting down (`main.js:die`).
    Dying,
    /// `'DEAD'`.
    Dead,
    /// `'MENU'` — any panel or the pause menu (`main.js:426 openMenu`).
    Menu,
    /// `'ENDING'`.
    Ending,
}

impl GameMode {
    /// The prototype's mode string, as it appears in `key` events (`main.js:emit('key', {code, mode})`).
    pub fn js_name(self) -> &'static str {
        match self {
            GameMode::Loading => "LOADING",
            GameMode::Title => "TITLE",
            GameMode::Hub => "HUB",
            GameMode::Zone => "ZONE",
            GameMode::Dying => "DYING",
            GameMode::Dead => "DEAD",
            GameMode::Menu => "MENU",
            GameMode::Ending => "ENDING",
        }
    }
}

/// `main.js:53 state.prevMode` — the mode a `Menu` was opened from, returned to on close.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrevMode(pub Option<GameMode>);

/// `main.js:53 state.menuKind` — `board`, `build`, `service`, `dialog`, `pause`, `main` …
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuKind(pub Option<String>);

/// A run is in progress: the player is in a zone or dying in one (`main.js` treats `DYING` as still
/// "in the run"; the loot is only lost when `die()` resolves).
pub fn in_run(mode: GameMode) -> bool {
    matches!(mode, GameMode::Zone | GameMode::Dying)
}

/// The simulation advances: creatures, the player and the run clock only tick in `ZONE`
/// (`main.js:385` — neither runs while `DYING`).
pub fn sim_runs(mode: GameMode) -> bool {
    matches!(mode, GameMode::Zone)
}

/// The handlamp may stay lit (`main.js:135 updateLamp` forces `lampOn = false` everywhere else).
pub fn lamp_allowed(mode: GameMode) -> bool {
    matches!(
        mode,
        GameMode::Zone | GameMode::Dying | GameMode::Menu | GameMode::Ending
    )
}

/// A menu can be opened from here (`main.js:426 openMenu` returns early otherwise).
pub fn can_open_menu(mode: GameMode) -> bool {
    matches!(mode, GameMode::Hub | GameMode::Zone)
}

/// `ctx.state.mode` as `main.js` would read it *within the same tick*: the pending
/// [`NextState`] wins over the applied [`State`].
///
/// Bevy applies a queued state change in the `StateTransition` schedule, which runs once per frame
/// before `RunFixedMainLoop`, so a system reading only `State<GameMode>` still sees the previous
/// mode for the rest of the tick — and for every further fixed tick of the same frame — after
/// another system called [`Mode::set`]. `main.js` assigned `state.mode = m` synchronously, so every
/// listener that ran later in the same call already saw the new mode. Every `FixedUpdate` system in
/// this crate therefore asks [`effective`] (through [`Mode`] or `run.rs`'s `ModeParam`) rather than
/// reading `State<GameMode>` on its own.
pub fn effective(state: &State<GameMode>, next: &NextState<GameMode>) -> GameMode {
    match next {
        NextState::Pending(s) | NextState::PendingIfNeq(s) => *s,
        NextState::Unchanged => *state.get(),
    }
}

/// Read-only [`effective`] mode for any system: `fn sys(mode: Mode) { if mode.get() == … }`.
/// Systems that also *change* the mode use `run.rs`'s `ModeParam`, which holds the same two
/// resources with `NextState` mutable.
#[derive(SystemParam)]
pub struct Mode<'w> {
    state: Res<'w, State<GameMode>>,
    next: Res<'w, NextState<GameMode>>,
}

impl Mode<'_> {
    /// `ctx.state.mode`, pending change included.
    pub fn get(&self) -> GameMode {
        effective(&self.state, &self.next)
    }

    /// The mode Bevy has actually applied — only for systems that must not act twice in one frame.
    pub fn applied(&self) -> GameMode {
        *self.state.get()
    }
}

/// Log every applied [`GameMode`] transition (`Loading -> Title`, `Title -> Hub`, …) so a run can
/// be followed from `cargo run`'s output alone — useful for the `UNDERCROFT_SCRIPT` smoke test and
/// for debugging in general. Identity transitions (`allow_same_state_transitions`, not used here)
/// are the only ones this would skip; `init_state`/`insert_state`'s first transition (`None ->
/// Loading` or `None -> Title`) is included.
fn log_mode_transitions(mut transitions: MessageReader<StateTransitionEvent<GameMode>>) {
    for t in transitions.read() {
        info!("GameMode: {:?} -> {:?}", t.exited, t.entered);
    }
}

/// Wires up [`log_mode_transitions`]. Added by `SkeletonPlugin`, so it runs in the real app, on
/// the web and in headless tests alike.
pub fn plugin(app: &mut App) {
    app.add_systems(Update, log_mode_transitions);
}
