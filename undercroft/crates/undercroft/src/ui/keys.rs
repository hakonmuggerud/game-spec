//! `main.js`'s `keydown` listener, the half the UI lane owns: the title menu (with the hidden
//! Backspace ×2 wipe), the pause menu, the hub menu digits, the altar screens and the minimap toggle.
//!
//! Keys arrive as `SimEvent::Key {code, mode}` on the message bus — `player.rs` emits one for every
//! press, in every mode, after acting on the ones it owns. What it already handles is deliberately not
//! repeated here:
//!
//! | mode | `player.rs::on_key` | this module |
//! |---|---|---|
//! | `TITLE` | nothing | the whole main menu, `Backspace` ×2 → `Reset` |
//! | `HUB` / `ZONE` | `Escape` → `OpenPause`; `F Q R E T` | `Tab` → `ToggleMinimap` |
//! | `MENU` (`pause`) | nothing — the stack is this lane's | ↑↓ / W S, ← →, Enter / Space / E, 1–9, `Escape` (pops a sub-panel, `ClosePause` at the root) |
//! | `MENU` (`dialog`) | `Escape` → `CloseMenu`; 1 / Enter / Space → `Accept` + `CloseMenu` | nothing |
//! | `MENU` (`board` / `build` / `service`) | `Escape` → `CloseMenu` | 1–9, Enter / Space / E |
//! | `DEAD` | Enter / Space → `ReturnToHub` | nothing |
//! | `ENDING` | nothing | 1–3 → `Choose`, Enter / Space / E → `ContinueEnding` |

use bevy::prelude::*;

use undercroft_sim::SimEvent;

use crate::debug::{DebugCommand, DebugQueue};
use crate::messages::SimMessage;
use crate::resources::{Game, Toasts};
use crate::state::MenuKind;
use crate::ui::menu::ItemAct;
use crate::ui::screens::Pick;
use crate::ui::UiState;

/// `main.js:resetArmedT` — the Backspace ×2 window on the main menu (seconds).
pub const RESET_ARM_T: f32 = 3.0;

/// Is `code` bound to `action` in `CFG.KEYS`?
fn is_key(game: &Game, action: &str, code: &str) -> bool {
    game.get().is_some_and(|a| {
        a.data
            .config
            .keys
            .get(action)
            .is_some_and(|ks| ks.iter().any(|k| k == code))
    })
}

/// The `keydown` routing this lane owns.
pub fn route_keys(
    mut r: MessageReader<SimMessage>,
    game: Game,
    kind: Res<MenuKind>,
    mut ui: ResMut<UiState>,
    mut queue: ResMut<DebugQueue>,
    mut toasts: ResMut<Toasts>,
    time: Res<Time>,
) {
    ui.reset_armed_t = (ui.reset_armed_t - time.delta_secs()).max(0.0);
    for m in r.read() {
        let SimEvent::Key { code, mode } = &m.0 else {
            continue;
        };
        match mode.as_str() {
            "TITLE" | "LOADING" => title_key(&game, &mut ui, &mut queue, &mut toasts, code),
            "MENU" => menu_key(&game, &kind, &mut ui, &mut queue, code),
            "ENDING" => ending_key(&game, &mut ui, &mut queue, code),
            // `main.js:586` — Tab; `run.rs` owns the flag and the Cartographer's Table gate.
            "HUB" | "ZONE" | "DYING" if is_key(&game, "minimap", code) => {
                queue.push(DebugCommand::ToggleMinimap);
            }
            _ => {}
        }
    }
}

/// `main.js:560–566` — the main menu plus the hidden save wipe.
fn title_key(
    game: &Game,
    ui: &mut UiState,
    queue: &mut DebugQueue,
    toasts: &mut Toasts,
    code: &str,
) {
    if is_key(game, "reset", code) {
        if ui.reset_armed_t > 0.0 {
            queue.push(DebugCommand::Reset);
            ui.reset_armed_t = 0.0;
            // `saveReset` flushes the toast queue, so this one is pushed after that flush lands.
            ui.wipe_toast_pending = true;
        } else {
            ui.reset_armed_t = RESET_ARM_T;
            toasts
                .0
                .push_back("Press again to wipe the save".to_string());
        }
        return;
    }
    let ctx = ui.menu_ctx.clone();
    let act = ui.menu.key(code, &ctx).act;
    apply_item(game, ui, queue, act);
}

/// `main.js:571–577` — `MENU` mode.
fn menu_key(game: &Game, kind: &MenuKind, ui: &mut UiState, queue: &mut DebugQueue, code: &str) {
    match kind.0.as_deref() {
        Some("pause") => {
            // Escape is entirely this lane's: `MenuState::key` pops a sub-panel, and at the root
            // falls through to `pauseRoot`'s `onEscape` — `ItemAct::Cmd(ClosePause)`.
            let ctx = ui.menu_ctx.clone();
            let act = ui.menu.key(code, &ctx).act;
            apply_item(game, ui, queue, act);
        }
        // `npc.js:onKey` is `player.rs`'s; Escape is too.
        Some("dialog") | None => {}
        // `hub.js:handleMenuKey` — digits pick, confirm activates.
        Some(_) => {
            let Some(screen) = ui.text_screen.clone() else {
                return;
            };
            if let Some(n) = digit(code) {
                if let Some(Some(p)) = screen.picks.get(n - 1) {
                    apply_pick(queue, p.clone());
                }
                return;
            }
            if is_key(game, "confirm", code) || is_key(game, "interact", code) {
                match &screen.confirm {
                    Some(p) => apply_pick(queue, p.clone()),
                    None => queue.push(DebugCommand::CloseMenu),
                }
            }
        }
    }
}

/// `endgame.js:onKey` — the altar choice and the end screen.
fn ending_key(game: &Game, ui: &mut UiState, queue: &mut DebugQueue, code: &str) {
    let Some(screen) = ui.text_screen.clone() else {
        return;
    };
    if let Some(n) = digit(code) {
        if let Some(Some(p)) = screen.picks.get(n - 1) {
            apply_pick(queue, p.clone());
        }
        return;
    }
    if is_key(game, "menu_close", code) && screen.confirm.is_none() {
        // `endgame.js:cancel()` — Esc on the choice screen goes back to the run.
        queue.push(DebugCommand::CancelChoice);
        return;
    }
    if is_key(game, "confirm", code) || is_key(game, "interact", code) {
        if let Some(p) = screen.confirm.clone() {
            apply_pick(queue, p);
        }
    }
}

/// `/^(?:Digit|Numpad)([1-9])$/`.
fn digit(code: &str) -> Option<usize> {
    let rest = code
        .strip_prefix("Digit")
        .or_else(|| code.strip_prefix("Numpad"))?;
    match rest.parse::<usize>() {
        Ok(n) if (1..=9).contains(&n) => Some(n),
        _ => None,
    }
}

/// `AUDIO.volStep` — how far one `←` / `→` / `[` / `]` moves the master volume.
fn vol_step(game: &Game) -> f32 {
    let step = game.get().map_or(0.0, |a| a.data.config.audio.vol_step);
    if step > 0.0 {
        step
    } else {
        0.1
    }
}

/// Perform what activating a list-menu row produced. The two audio rows become
/// [`DebugCommand::SetVolume`] / [`DebugCommand::ToggleMute`], which the audio lane drains
/// (it owns `save.audio`).
pub fn apply_item(game: &Game, ui: &mut UiState, queue: &mut DebugQueue, act: Option<ItemAct>) {
    let vol = ui.menu_ctx.volume;
    match act {
        Some(ItemAct::Cmd(c)) => queue.push(c),
        // `ui.js:soundPanel` `adjust(dir)` — `setVolume(vol + dir × step)`.
        Some(ItemAct::Volume(dir)) => {
            queue.push(DebugCommand::SetVolume(vol + dir as f32 * vol_step(game)));
        }
        // … and its `run()`, which wraps 100 % back to 0 instead of stepping.
        Some(ItemAct::VolumeCycle) => {
            let next = if vol >= 0.999 {
                0.0
            } else {
                vol + vol_step(game)
            };
            queue.push(DebugCommand::SetVolume(next));
        }
        Some(ItemAct::ToggleMute) => queue.push(DebugCommand::ToggleMute),
        Some(ItemAct::Push(_)) | Some(ItemAct::Pop) | Some(ItemAct::Nothing) | None => {}
    }
}

/// Perform a `[n]` pick from a text screen.
pub fn apply_pick(queue: &mut DebugQueue, pick: Pick) {
    match pick {
        Pick::Cmd(c) => queue.push(c),
        Pick::Cmds(cs) => {
            for c in cs {
                queue.push(c);
            }
        }
    }
}
