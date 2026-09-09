//! `DebugCommand` — the Rust replacement for `window.__game.actions` (`main.js:660–740`), the hook
//! the prototype's Playwright suite drove the game through. One variant per action that *changes*
//! state; the query-only actions (`saveInfo`, `mainMenu`, `pauseMenu`, `los`, `bfsField`) are
//! omitted because tests read the resources directly.
//!
//! Commands are queued in [`DebugQueue`] and drained in [`SimSet::Debug`] at the top of the fixed
//! tick. `run.rs` and `player.rs` each add a system in [`DebugSet::Handle`] that takes only the
//! variants they own with [`DebugQueue::take`]; whatever is left when [`DebugSet::Drain`] runs is
//! logged and dropped, so an unimplemented command never wedges the queue.
//!
//! [`DebugCommand::parse`] gives every variant a text form, and [`parse_script`]/[`DebugScript`]
//! chain a whole `;`-separated sequence of them (plus `wait`/`screenshot`/`quit`) into a scripted
//! run — natively driven by the `UNDERCROFT_SCRIPT` environment variable, so `cargo run` can be
//! smoke-tested end to end without a human at the keyboard.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::collections::VecDeque;
use undercroft_sim::creature::SpawnOpts;

use crate::resources::Game;
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

    /// The text form of a command: the `js_name()` (case-insensitive) followed by
    /// whitespace-separated args — strings as bare words, numbers as decimals. Used by
    /// [`parse_script`] and, natively, by the `UNDERCROFT_SCRIPT` startup script.
    ///
    /// Grammar, one example per variant with args: `openMenu board`, `loadZone undercroft`,
    /// `selectZone undercroft`, `freeNpc oswin`, `accept q1`, `build workshop free` (the `free`
    /// word is optional; its absence means `free: false`), `choose ember`, `giveTool pry`,
    /// `setPoints 5`, `setResources 50 3 0` (three positional numbers, oil/relics/rich; `_` in a
    /// slot means "leave unchanged", i.e. `None`), `rescue oswin`, `gotoZone undercroft`,
    /// `teleport 12.5 8.5 1.57` (x, z, an optional yaw), `spawnHunter 10 10 base` (cx, cz,
    /// profile), `spawnCreature drowner 10 10` (profile, cx, cz — always spawned with default
    /// `SpawnOpts`; the text form has no way to set facing/sweep/reach/territory/leash), `key
    /// KeyE`.
    pub fn parse(line: &str) -> Result<DebugCommand, String> {
        let mut words = line.split_whitespace();
        let name = words
            .next()
            .ok_or_else(|| "empty debug command".to_string())?
            .to_ascii_lowercase();
        let args: Vec<&str> = words.collect();

        fn arg<'a>(args: &[&'a str], i: usize, what: &str) -> Result<&'a str, String> {
            args.get(i)
                .copied()
                .ok_or_else(|| format!("missing arg {i} ({what})"))
        }
        fn num<T: std::str::FromStr>(args: &[&str], i: usize, what: &str) -> Result<T, String> {
            let s = arg(args, i, what)?;
            s.parse::<T>()
                .map_err(|_| format!("bad {what} (arg {i}): {s:?}"))
        }
        fn opt_u32(args: &[&str], i: usize, what: &str) -> Result<Option<u32>, String> {
            let s = arg(args, i, what)?;
            if s == "_" {
                Ok(None)
            } else {
                s.parse::<u32>()
                    .map(Some)
                    .map_err(|_| format!("bad {what} (arg {i}): {s:?}"))
            }
        }

        Ok(match name.as_str() {
            "begin" => DebugCommand::Begin,
            "descend" => DebugCommand::Descend,
            "bank" => DebugCommand::Bank,
            "flash" => DebugCommand::Flash,
            "plantlantern" => DebugCommand::PlantLantern,
            "interact" => DebugCommand::Interact,
            "topup" => DebugCommand::TopUp,
            "togglelamp" => DebugCommand::ToggleLamp,
            "returntohub" => DebugCommand::ReturnToHub,
            "die" => DebugCommand::Die,
            "enterhub" => DebugCommand::EnterHub,
            "openmenu" => DebugCommand::OpenMenu(arg(&args, 0, "kind")?.to_string()),
            "closemenu" => DebugCommand::CloseMenu,
            "openmainmenu" => DebugCommand::OpenMainMenu,
            "openpause" => DebugCommand::OpenPause,
            "closepause" => DebugCommand::ClosePause,
            "clearsave" => DebugCommand::ClearSave,
            "newgame" => DebugCommand::NewGame,
            "loadzone" => DebugCommand::LoadZone(arg(&args, 0, "id")?.to_string()),
            "selectzone" => DebugCommand::SelectZone(arg(&args, 0, "id")?.to_string()),
            "freenpc" => DebugCommand::FreeNpc(arg(&args, 0, "id")?.to_string()),
            "accept" => DebugCommand::Accept(arg(&args, 0, "id")?.to_string()),
            "build" => {
                let id = arg(&args, 0, "id")?.to_string();
                let free = args.get(1).is_some_and(|s| s.eq_ignore_ascii_case("free"));
                DebugCommand::Build { id, free }
            }
            "choose" => DebugCommand::Choose(arg(&args, 0, "id")?.to_string()),
            "givetool" => DebugCommand::GiveTool(arg(&args, 0, "id")?.to_string()),
            "setpoints" => DebugCommand::SetPoints(num(&args, 0, "n")?),
            "setresources" => DebugCommand::SetResources {
                oil: opt_u32(&args, 0, "oil")?,
                relics: opt_u32(&args, 1, "relics")?,
                rich: opt_u32(&args, 2, "rich")?,
            },
            "rescue" => DebugCommand::Rescue(arg(&args, 0, "id")?.to_string()),
            "unlockall" => DebugCommand::UnlockAll,
            "gotozone" => DebugCommand::GotoZone(arg(&args, 0, "id")?.to_string()),
            "teleport" => DebugCommand::Teleport {
                x: num(&args, 0, "x")?,
                z: num(&args, 1, "z")?,
                yaw: match args.get(2) {
                    Some(s) => Some(
                        s.parse::<f32>()
                            .map_err(|_| format!("bad yaw (arg 2): {s:?}"))?,
                    ),
                    None => None,
                },
            },
            "spawnhunter" => DebugCommand::SpawnHunter {
                cx: num(&args, 0, "cx")?,
                cz: num(&args, 1, "cz")?,
                profile: arg(&args, 2, "profile")?.to_string(),
            },
            "spawncreature" => DebugCommand::SpawnCreature {
                profile: arg(&args, 0, "profile")?.to_string(),
                cx: num(&args, 1, "cx")?,
                cz: num(&args, 2, "cz")?,
                opts: SpawnOpts::default(),
            },
            "rideup" => DebugCommand::RideUp,
            "openchoice" => DebugCommand::OpenChoice,
            "continueending" => DebugCommand::ContinueEnding,
            "reset" => DebugCommand::Reset,
            "key" => DebugCommand::Key(arg(&args, 0, "code")?.to_string()),
            other => return Err(format!("unknown debug command: {other:?}")),
        })
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

/// The three parts of [`SimSet::Debug`], in order: the script driver, then the lane handlers, then
/// the leftover sweep.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugSet {
    /// [`run_script`] (and, in `UndercroftPlugin`, [`take_screenshots`]) — turns [`DebugScript`]
    /// steps into [`DebugQueue`] pushes before anyone drains the queue this tick.
    Script,
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

/* ============================================================
Debug scripts: `wait`/`screenshot`/`quit` plus a sequence of DebugCommands
============================================================ */

/// One line of a parsed debug script.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptStep {
    /// A plain [`DebugCommand`], pushed into [`DebugQueue`] as soon as this step is reached.
    Command(DebugCommand),
    /// `wait <seconds>` — hold the rest of the script for this many seconds of fixed sim time.
    Wait(f32),
    /// `screenshot <path>` — ask the renderer to save a PNG of the primary window to `path`.
    /// Handled by [`take_screenshots`], which only `UndercroftPlugin` registers; elsewhere the
    /// step is popped and silently has no effect (there is no renderer to ask).
    Screenshot(String),
    /// `quit` — write [`AppExit::Success`].
    Quit,
}

/// Parse a `;`-separated debug script (the form `UNDERCROFT_SCRIPT` and tests use). Each piece is
/// trimmed and, if non-empty, is either one of the three pseudo-commands (`wait <secs>`,
/// `screenshot <path>`, `quit`, matched case-insensitively) or a [`DebugCommand::parse`] line.
/// Empty pieces (a trailing `;`, doubled `;;`) are skipped.
///
/// Example: `"wait 1; begin; wait 2; gotoZone undercroft; wait 1; screenshot /tmp/z.png; wait 1;
/// quit"`.
pub fn parse_script(text: &str) -> Result<Vec<ScriptStep>, String> {
    let mut steps = Vec::new();
    for (i, part) in text.split(';').enumerate() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut words = part.split_whitespace();
        // `split_whitespace` already found at least one word, since `part` is non-empty and
        // trimmed.
        let head = words.next().unwrap().to_ascii_lowercase();
        let step = match head.as_str() {
            "wait" => {
                let raw = words
                    .next()
                    .ok_or_else(|| format!("script step {i}: wait needs a duration: {part:?}"))?;
                let secs: f32 = raw
                    .parse()
                    .map_err(|_| format!("script step {i}: bad wait duration: {raw:?}"))?;
                ScriptStep::Wait(secs)
            }
            "screenshot" => {
                let path = words
                    .next()
                    .ok_or_else(|| format!("script step {i}: screenshot needs a path: {part:?}"))?;
                ScriptStep::Screenshot(path.to_string())
            }
            "quit" => ScriptStep::Quit,
            _ => ScriptStep::Command(
                DebugCommand::parse(part).map_err(|e| format!("script step {i}: {e}"))?,
            ),
        };
        steps.push(step);
    }
    Ok(steps)
}

/// Pending script steps, oldest first. Native `UndercroftPlugin` fills this from `UNDERCROFT_SCRIPT`
/// at startup; tests insert it directly (`app.insert_resource(DebugScript(steps.into()))`). Empty
/// by default, which is why [`run_script`] is a no-op — and does not even run, see its `run_if` —
/// until something populates it.
#[derive(Resource, Debug, Default)]
pub struct DebugScript(pub VecDeque<ScriptStep>);

/// A [`ScriptStep::Screenshot`] handed off to whoever can act on it. `SkeletonPlugin`/the headless
/// harness have no reader for this message — it is simply dropped — because taking a screenshot
/// needs a renderer, which only `UndercroftPlugin` (via [`take_screenshots`]) has.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct TakeScreenshot(pub String);

/// The game data has finished loading, mirroring `run.rs`'s private `data_ready` (duplicated
/// rather than imported so `debug.rs` does not need to depend on `run.rs`).
fn script_ready(game: Game) -> bool {
    game.get().is_some()
}

/// Drains [`DebugScript`] one (or several immediate) steps at a time, in [`DebugSet::Script`] —
/// first in the [`SimSet::Debug`] chain, so a scripted command is queued before `run.rs`/
/// `player.rs` drain [`DebugQueue`] the same tick. `Command` and `Screenshot` steps are popped and
/// acted on immediately (a script can queue several in the same tick with no `wait` between);
/// `Wait` counts down against the fixed [`Time`] delta and blocks the rest of the script until it
/// reaches zero; `Quit` pops itself and exits.
fn run_script(
    mut script: ResMut<DebugScript>,
    mut queue: ResMut<DebugQueue>,
    time: Res<Time>,
    mut shots: MessageWriter<TakeScreenshot>,
    mut exit: MessageWriter<AppExit>,
) {
    let dt = time.delta_secs();
    loop {
        match script.0.front_mut() {
            None => break,
            Some(ScriptStep::Wait(remaining)) => {
                *remaining -= dt;
                if *remaining > 0.0 {
                    break;
                }
                script.0.pop_front();
            }
            Some(ScriptStep::Command(_)) => {
                let Some(ScriptStep::Command(cmd)) = script.0.pop_front() else {
                    unreachable!("front_mut just matched Command");
                };
                queue.push(cmd);
            }
            Some(ScriptStep::Screenshot(_)) => {
                let Some(ScriptStep::Screenshot(path)) = script.0.pop_front() else {
                    unreachable!("front_mut just matched Screenshot");
                };
                shots.write(TakeScreenshot(path));
            }
            Some(ScriptStep::Quit) => {
                script.0.pop_front();
                exit.write(AppExit::Success);
                break;
            }
        }
    }
}

/// Turns a [`TakeScreenshot`] into a real capture: `bevy_render::view::screenshot::Screenshot` /
/// `save_to_disk` (`bevy_render-0.19.1/src/view/window/screenshot.rs:78-135` — spawn a
/// `Screenshot::primary_window()` entity, observe it with `save_to_disk(path)`). Registered only
/// by [`crate::UndercroftPlugin`] via [`screenshot_plugin`]: it needs the renderer running, so it
/// must stay out of `SkeletonPlugin`/the headless harness.
fn take_screenshots(mut shots: MessageReader<TakeScreenshot>, mut commands: Commands) {
    for TakeScreenshot(path) in shots.read() {
        info!("debug script: screenshot -> {path}");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
    }
}

/// Natively, read `UNDERCROFT_SCRIPT` once at startup and queue it as a [`DebugScript`]; an
/// unparsable script is logged and ignored (the app still starts, just with nothing scripted).
#[cfg(not(target_arch = "wasm32"))]
fn load_script_from_env(mut script: ResMut<DebugScript>) {
    let Ok(text) = std::env::var("UNDERCROFT_SCRIPT") else {
        return;
    };
    match parse_script(&text) {
        Ok(steps) => {
            info!("UNDERCROFT_SCRIPT: {} step(s) queued", steps.len());
            script.0 = steps.into();
        }
        Err(e) => error!("UNDERCROFT_SCRIPT: unparsable script ignored: {e}"),
    }
}
// TODO(wasm): read `?script=` from the page URL (`web_sys::window().location().search()`) and
// queue it the same way, once the web build needs scripted smoke tests.

/// Everything [`crate::UndercroftPlugin`] adds on top of the generic script/debug machinery in
/// [`plugin`]: the screenshot capture system, and — natively — the `UNDERCROFT_SCRIPT` startup
/// read. Never added by `SkeletonPlugin`/the headless harness.
pub fn screenshot_plugin(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        take_screenshots.in_set(DebugSet::Script).after(run_script),
    );
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Startup, load_script_from_env);
}

/// The queue, the debug script machinery, and the dispatch sets. Added by `SkeletonPlugin`, so it
/// runs identically in the real app and headless tests; [`screenshot_plugin`] layers the
/// render-dependent half on top, `UndercroftPlugin`-only.
pub fn plugin(app: &mut App) {
    app.init_resource::<DebugQueue>()
        .init_resource::<DebugScript>()
        .add_message::<TakeScreenshot>()
        .configure_sets(
            FixedUpdate,
            (DebugSet::Script, DebugSet::Handle, DebugSet::Drain)
                .chain()
                .in_set(SimSet::Debug),
        )
        .add_systems(
            FixedUpdate,
            run_script
                .in_set(DebugSet::Script)
                .run_if(script_ready)
                .run_if(|s: Res<DebugScript>| !s.0.is_empty()),
        )
        .add_systems(FixedUpdate, drop_unhandled.in_set(DebugSet::Drain));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One row of the round-trip table: the text form and the command it must parse to.
    fn cases() -> Vec<(&'static str, DebugCommand)> {
        vec![
            ("begin", DebugCommand::Begin),
            ("descend", DebugCommand::Descend),
            ("bank", DebugCommand::Bank),
            ("flash", DebugCommand::Flash),
            ("plantLantern", DebugCommand::PlantLantern),
            ("interact", DebugCommand::Interact),
            ("topUp", DebugCommand::TopUp),
            ("toggleLamp", DebugCommand::ToggleLamp),
            ("returnToHub", DebugCommand::ReturnToHub),
            ("die", DebugCommand::Die),
            ("enterHub", DebugCommand::EnterHub),
            (
                "openMenu board",
                DebugCommand::OpenMenu("board".to_string()),
            ),
            ("closeMenu", DebugCommand::CloseMenu),
            ("openMainMenu", DebugCommand::OpenMainMenu),
            ("openPause", DebugCommand::OpenPause),
            ("closePause", DebugCommand::ClosePause),
            ("clearSave", DebugCommand::ClearSave),
            ("newGame", DebugCommand::NewGame),
            (
                "loadZone undercroft",
                DebugCommand::LoadZone("undercroft".to_string()),
            ),
            (
                "selectZone undercroft",
                DebugCommand::SelectZone("undercroft".to_string()),
            ),
            ("freeNpc oswin", DebugCommand::FreeNpc("oswin".to_string())),
            ("accept q1", DebugCommand::Accept("q1".to_string())),
            (
                "build workshop free",
                DebugCommand::Build {
                    id: "workshop".to_string(),
                    free: true,
                },
            ),
            (
                "build workshop",
                DebugCommand::Build {
                    id: "workshop".to_string(),
                    free: false,
                },
            ),
            ("choose ember", DebugCommand::Choose("ember".to_string())),
            ("giveTool pry", DebugCommand::GiveTool("pry".to_string())),
            ("setPoints 5", DebugCommand::SetPoints(5)),
            (
                "setResources 50 3 0",
                DebugCommand::SetResources {
                    oil: Some(50),
                    relics: Some(3),
                    rich: Some(0),
                },
            ),
            (
                "setResources _ 3 _",
                DebugCommand::SetResources {
                    oil: None,
                    relics: Some(3),
                    rich: None,
                },
            ),
            ("rescue oswin", DebugCommand::Rescue("oswin".to_string())),
            ("unlockAll", DebugCommand::UnlockAll),
            (
                "gotoZone undercroft",
                DebugCommand::GotoZone("undercroft".to_string()),
            ),
            (
                "teleport 12.5 8.5 1.57",
                DebugCommand::Teleport {
                    x: 12.5,
                    z: 8.5,
                    yaw: Some(1.57),
                },
            ),
            (
                "teleport 1 2",
                DebugCommand::Teleport {
                    x: 1.0,
                    z: 2.0,
                    yaw: None,
                },
            ),
            (
                "spawnHunter 10 10 base",
                DebugCommand::SpawnHunter {
                    cx: 10,
                    cz: 10,
                    profile: "base".to_string(),
                },
            ),
            (
                "spawnCreature drowner 10 10",
                DebugCommand::SpawnCreature {
                    profile: "drowner".to_string(),
                    cx: 10,
                    cz: 10,
                    opts: SpawnOpts::default(),
                },
            ),
            ("rideUp", DebugCommand::RideUp),
            ("openChoice", DebugCommand::OpenChoice),
            ("continueEnding", DebugCommand::ContinueEnding),
            ("reset", DebugCommand::Reset),
            ("key KeyE", DebugCommand::Key("KeyE".to_string())),
        ]
    }

    /// Every variant round-trips through its `js_name()` text form, and the action name is
    /// matched case-insensitively.
    #[test]
    fn parse_round_trips_every_variant() {
        for (text, expected) in cases() {
            assert_eq!(
                DebugCommand::parse(text).as_ref(),
                Ok(&expected),
                "parsing {text:?}"
            );
            // Only the action name (the first word) is case-insensitive; string args (zone ids,
            // menu kinds, `KeyE` …) are passed through verbatim, so upper-case only that word.
            let mut words = text.splitn(2, ' ');
            let head = words.next().unwrap().to_ascii_uppercase();
            let rest = words.next();
            let shouted = match rest {
                Some(rest) => format!("{head} {rest}"),
                None => head,
            };
            assert_eq!(
                DebugCommand::parse(&shouted).as_ref(),
                Ok(&expected),
                "parsing {shouted:?} (action name upper-cased)"
            );
            // The text form always starts with the JS action name.
            assert!(
                text.to_ascii_lowercase()
                    .starts_with(&expected.js_name().to_ascii_lowercase()),
                "{text:?} should start with js_name {}",
                expected.js_name()
            );
        }
    }

    #[test]
    fn parse_rejects_empty_and_unknown_and_missing_args() {
        assert!(DebugCommand::parse("").is_err());
        assert!(DebugCommand::parse("   ").is_err());
        assert!(DebugCommand::parse("notARealAction").is_err());
        assert!(DebugCommand::parse("teleport 1").is_err(), "missing z");
        assert!(DebugCommand::parse("teleport x 2").is_err(), "bad number");
        assert!(DebugCommand::parse("openMenu").is_err(), "missing kind");
    }

    #[test]
    fn parse_script_splits_on_semicolons_and_trims() {
        let steps = parse_script(
            "wait 1; begin; wait 2; gotoZone undercroft; wait 1; screenshot /tmp/z.png; wait 1; quit",
        )
        .expect("valid script");
        assert_eq!(
            steps,
            vec![
                ScriptStep::Wait(1.0),
                ScriptStep::Command(DebugCommand::Begin),
                ScriptStep::Wait(2.0),
                ScriptStep::Command(DebugCommand::GotoZone("undercroft".to_string())),
                ScriptStep::Wait(1.0),
                ScriptStep::Screenshot("/tmp/z.png".to_string()),
                ScriptStep::Wait(1.0),
                ScriptStep::Quit,
            ]
        );
    }

    #[test]
    fn parse_script_skips_empty_pieces_and_is_case_insensitive() {
        let steps = parse_script(" ;;QUIT;; ").expect("valid script");
        assert_eq!(steps, vec![ScriptStep::Quit]);
    }

    #[test]
    fn parse_script_rejects_bad_steps() {
        assert!(parse_script("wait").is_err(), "wait needs a duration");
        assert!(parse_script("wait soon").is_err(), "wait needs a number");
        assert!(
            parse_script("screenshot").is_err(),
            "screenshot needs a path"
        );
        assert!(
            parse_script("not a command").is_err(),
            "unknown action inside a script step"
        );
    }

    #[test]
    fn take_returns_only_matching_and_leaves_the_rest_in_order() {
        let mut q = DebugQueue::default();
        q.push(DebugCommand::Begin);
        q.push(DebugCommand::Flash);
        q.push(DebugCommand::Descend);
        let taken = q.take(|c| matches!(c, DebugCommand::Begin | DebugCommand::Descend));
        assert_eq!(taken, vec![DebugCommand::Begin, DebugCommand::Descend]);
        assert_eq!(
            q.0.into_iter().collect::<Vec<_>>(),
            vec![DebugCommand::Flash]
        );
    }

    mod script_system {
        use super::*;
        use crate::headless::*;
        use crate::state::GameMode;

        /// A scripted `Wait` blocks a following `Command` until enough fixed ticks have passed,
        /// and the command reaches the real dispatch (`begin` moves `Title -> Hub`).
        #[test]
        fn wait_blocks_the_next_command_until_it_elapses() {
            let mut app = headless_app();
            app.insert_resource(DebugScript(
                vec![
                    ScriptStep::Wait(0.5),
                    ScriptStep::Command(DebugCommand::Begin),
                ]
                .into(),
            ));
            step(&mut app, 0.4);
            assert_eq!(mode(&app), GameMode::Title, "wait has not elapsed yet");
            step(&mut app, 0.2);
            // `begin` was queued once the wait elapsed and `run.rs` handled it this tick.
            assert_eq!(mode(&app), GameMode::Hub);
            assert!(app.world().resource::<DebugScript>().0.is_empty());
        }

        /// `Quit` writes `AppExit::Success`.
        #[test]
        fn quit_step_writes_app_exit() {
            #[derive(Resource, Default)]
            struct ExitSeen(bool);

            fn watch_exit(mut seen: ResMut<ExitSeen>, mut exits: MessageReader<AppExit>) {
                if exits.read().next().is_some() {
                    seen.0 = true;
                }
            }

            let mut app = headless_app();
            app.init_resource::<ExitSeen>()
                .add_systems(FixedUpdate, watch_exit.after(DebugSet::Script));
            app.insert_resource(DebugScript(vec![ScriptStep::Quit].into()));
            step(&mut app, 1.0 / 60.0);
            assert!(app.world().resource::<ExitSeen>().0, "AppExit was written");
            assert!(app.world().resource::<DebugScript>().0.is_empty());
        }

        /// With an empty `DebugScript` (the default), the script system does not even run — it
        /// is gated by `run_if(!DebugScript::is_empty)` — so it never interferes with ordinary
        /// `DebugQueue` sends.
        #[test]
        fn an_empty_script_never_touches_the_queue() {
            let mut app = headless_app();
            send(&mut app, DebugCommand::Flash);
            step(&mut app, 1.0 / 60.0);
            assert!(app.world().resource::<DebugQueue>().is_empty());
        }
    }
}
