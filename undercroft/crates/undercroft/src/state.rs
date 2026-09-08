//! Game modes. Mirrors `state.mode` in `main.js:53` (`'TITLE' | 'HUB' | 'ZONE' | 'DYING' | 'DEAD' |
//! 'MENU' | 'ENDING'`) plus a `Loading` mode the JS has no need for: the Bevy build reads its data
//! through the asset server, which is asynchronous.

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
