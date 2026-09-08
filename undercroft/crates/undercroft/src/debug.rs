//! `DebugCommand` — the Rust replacement for `window.__game.actions` (`main.js:660–740`), the hook
//! the prototype's Playwright suite drove the game through. One variant per action that *changes*
//! state; the query-only actions (`saveInfo`, `mainMenu`, `pauseMenu`, `los`, `bfsField`) are
//! omitted because tests read the resources directly.
//!
//! Commands are queued in [`DebugQueue`] and drained in [`SimSet::Debug`] at the top of the fixed
//! tick. `run.rs` and `player.rs` each add a system in [`DebugSet::Handle`] that takes only the
//! variants they own with [`DebugQueue::take`]; whatever is left when [`DebugSet::Drain`] runs is
//! logged and dropped, so an unimplemented command never wedges the queue.

use bevy::prelude::*;
use std::collections::VecDeque;
use undercroft_sim::creature::SpawnOpts;

use crate::tick::SimSet;

/// One `ctx.actions.*` call. The doc comment on each variant names the JS action.
#[derive(Debug, Clone, PartialEq)]
pub enum DebugCommand {
    /// `begin()` — start a game from the title.
    Begin,
    /// `descend()` — leave the hub down the stairs.
    Descend,
    /// `bank()` — hand the carried loot in at the extraction marker.
    Bank,
    /// `flash()` — the lamp flash.
    Flash,
    /// `plantLantern()`.
    PlantLantern,
    /// `interact()` — E on whatever `interactTarget()` resolves to.
    Interact,
    /// `topUp()` — pour a flask into the handlamp.
    TopUp,
    /// `toggleLamp()`.
    ToggleLamp,
    /// `returnToHub()` — from the death screen.
    ReturnToHub,
    /// `die()`.
    Die,
    /// `enterHub()`.
    EnterHub,
    /// `openMenu(kind)` — `board`, `build`, `service`, `dialog` …
    OpenMenu(String),
    /// `closeMenu()`.
    CloseMenu,
    /// `openMainMenu()` / `toMainMenu()` — abandons a run in progress.
    OpenMainMenu,
    /// `openPause()`.
    OpenPause,
    /// `closePause()`.
    ClosePause,
    /// `clearSave()`.
    ClearSave,
    /// `newGame()`.
    NewGame,
    /// `loadZone(id)` — v2 semantics: in the hub it only loads the zone inactive.
    LoadZone(String),
    /// `selectZone(id)` — the Departure Board choice (`hub.select`).
    SelectZone(String),
    /// `freeNpc(id)`.
    FreeNpc(String),
    /// `accept(id)` — accept a contract.
    Accept(String),
    /// `build(id, opts)` — `free` is `opts.free`, the debug "no cost" build.
    Build { id: String, free: bool },
    /// `choose(id)` — pick an ending at the altar.
    Choose(String),
    /// `giveTool(id)`.
    GiveTool(String),
    /// `setPoints(n)`.
    SetPoints(u32),
    /// `setResources({oil, relics, rich})` — absolute banked ledgers; `None` keeps the current value.
    SetResources {
        oil: Option<u32>,
        relics: Option<u32>,
        rich: Option<u32>,
    },
    /// `rescue(id)` — mark an NPC rescued as if banked with them.
    Rescue(String),
    /// `unlockAll()`.
    UnlockAll,
    /// `gotoZone(id)` — start a run in that zone right now, no fade, no lock check.
    GotoZone(String),
    /// `teleport(x, z, yaw?)`.
    Teleport { x: f32, z: f32, yaw: Option<f32> },
    /// `spawnHunter(cx, cz, profile)`.
    SpawnHunter { cx: i32, cz: i32, profile: String },
    /// `spawnCreature(profile, cx, cz, opts)` (DESIGN.md §5.8).
    SpawnCreature {
        profile: String,
        cx: i32,
        cz: i32,
        opts: SpawnOpts,
    },
    /// `rideUp()` — the Source elevator.
    RideUp,
    /// `openChoice()` — the altar menu.
    OpenChoice,
    /// `continueEnding()` — `endgame.continueToHub()`.
    ContinueEnding,
    /// `reset()` — `resetRuntime()`.
    Reset,
    /// A raw key press fed to the same handler the real input uses (`main.js:560–640`), so tests can
    /// press `KeyE`, `Escape` or `Digit1`.
    Key(String),
}

impl DebugCommand {
    /// The JS action name, for logs.
    pub fn js_name(&self) -> &'static str {
        match self {
            DebugCommand::Begin => "begin",
            DebugCommand::Descend => "descend",
            DebugCommand::Bank => "bank",
            DebugCommand::Flash => "flash",
            DebugCommand::PlantLantern => "plantLantern",
            DebugCommand::Interact => "interact",
            DebugCommand::TopUp => "topUp",
            DebugCommand::ToggleLamp => "toggleLamp",
            DebugCommand::ReturnToHub => "returnToHub",
            DebugCommand::Die => "die",
            DebugCommand::EnterHub => "enterHub",
            DebugCommand::OpenMenu(_) => "openMenu",
            DebugCommand::CloseMenu => "closeMenu",
            DebugCommand::OpenMainMenu => "openMainMenu",
            DebugCommand::OpenPause => "openPause",
            DebugCommand::ClosePause => "closePause",
            DebugCommand::ClearSave => "clearSave",
            DebugCommand::NewGame => "newGame",
            DebugCommand::LoadZone(_) => "loadZone",
            DebugCommand::SelectZone(_) => "selectZone",
            DebugCommand::FreeNpc(_) => "freeNpc",
            DebugCommand::Accept(_) => "accept",
            DebugCommand::Build { .. } => "build",
            DebugCommand::Choose(_) => "choose",
            DebugCommand::GiveTool(_) => "giveTool",
            DebugCommand::SetPoints(_) => "setPoints",
            DebugCommand::SetResources { .. } => "setResources",
            DebugCommand::Rescue(_) => "rescue",
            DebugCommand::UnlockAll => "unlockAll",
            DebugCommand::GotoZone(_) => "gotoZone",
            DebugCommand::Teleport { .. } => "teleport",
            DebugCommand::SpawnHunter { .. } => "spawnHunter",
            DebugCommand::SpawnCreature { .. } => "spawnCreature",
            DebugCommand::RideUp => "rideUp",
            DebugCommand::OpenChoice => "openChoice",
            DebugCommand::ContinueEnding => "continueEnding",
            DebugCommand::Reset => "reset",
            DebugCommand::Key(_) => "key",
        }
    }
}

/// Pending debug commands, oldest first. `headless::send` and (later) the dev console push here.
#[derive(Resource, Debug, Default)]
pub struct DebugQueue(pub VecDeque<DebugCommand>);

impl DebugQueue {
    /// Queue one command.
    pub fn push(&mut self, cmd: DebugCommand) {
        self.0.push_back(cmd);
    }

    /// Take every queued command matching `pred`, oldest first, leaving the rest for the other
    /// handlers. This is how `run.rs` and `player.rs` share the queue.
    pub fn take(&mut self, mut pred: impl FnMut(&DebugCommand) -> bool) -> Vec<DebugCommand> {
        let mut taken = Vec::new();
        let mut kept = VecDeque::with_capacity(self.0.len());
        for cmd in self.0.drain(..) {
            if pred(&cmd) {
                taken.push(cmd);
            } else {
                kept.push_back(cmd);
            }
        }
        self.0 = kept;
        taken
    }

    /// Nothing queued.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of queued commands.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// The two halves of [`SimSet::Debug`]: lane handlers first, then the leftover sweep.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugSet {
    /// `run.rs` / `player.rs` handlers. Every stage-2 debug system belongs here.
    Handle,
    /// The foundation's sweep: log and drop anything nobody took.
    Drain,
}

/// Anything still queued after the handlers ran is not implemented (yet): say so once and drop it,
/// so the app keeps stepping. `main.js` had no equivalent — an unknown action was simply `undefined`.
fn drop_unhandled(mut queue: ResMut<DebugQueue>) {
    for cmd in queue.0.drain(..) {
        warn!("unhandled DebugCommand::{} ({cmd:?})", cmd.js_name());
    }
}

/// The queue and the dispatch sets.
pub fn plugin(app: &mut App) {
    app.init_resource::<DebugQueue>()
        .configure_sets(
            FixedUpdate,
            (DebugSet::Handle, DebugSet::Drain)
                .chain()
                .in_set(SimSet::Debug),
        )
        .add_systems(FixedUpdate, drop_unhandled.in_set(DebugSet::Drain));
}
