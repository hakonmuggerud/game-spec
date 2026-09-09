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
//! | `HUB` / `ZONE` | `Escape` → `OpenPause`; `F Q R E T` | `Tab` → minimap |
//! | `MENU` (`pause`) | `Escape` → `ClosePause` | ↑↓ / W S, ← →, Enter / Space / E, 1–9, `Escape` in a sub-panel |
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
            "HUB" | "ZONE" | "DYING" if is_key(&game, "minimap", code) => {
                ui.minimap_toggle_requested = true;
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
    apply_item(ui, queue, act);
}

/// `main.js:571–577` — `MENU` mode.
fn menu_key(game: &Game, kind: &MenuKind, ui: &mut UiState, queue: &mut DebugQueue, code: &str) {
    match kind.0.as_deref() {
        Some("pause") => {
            // `player.rs` already turns Escape at the root into `ClosePause`; only a sub-panel pops.
            if is_key(game, "menu_close", code) && ui.menu.depth() <= 1 {
                return;
            }
            let ctx = ui.menu_ctx.clone();
            let act = ui.menu.key(code, &ctx).act;
            apply_item(ui, queue, act);
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
                    apply_pick(ui, queue, p.clone());
                }
                return;
            }
            if is_key(game, "confirm", code) || is_key(game, "interact", code) {
                match &screen.confirm {
                    Some(p) => apply_pick(ui, queue, p.clone()),
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
            apply_pick(ui, queue, p.clone());
        }
        return;
    }
    if is_key(game, "menu_close", code) && screen.confirm.is_none() {
        // `endgame.js:cancel()` — no `DebugCommand` reaches `run.rs`'s `cancel_choice`; see the report.
        warn_missing("CancelChoice");
        return;
    }
    if is_key(game, "confirm", code) || is_key(game, "interact", code) {
        if let Some(p) = screen.confirm.clone() {
            apply_pick(ui, queue, p);
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

/// Perform what activating a list-menu row produced.
pub fn apply_item(ui: &mut UiState, queue: &mut DebugQueue, act: Option<ItemAct>) {
    match act {
        Some(ItemAct::Cmd(c)) => queue.push(c),
        Some(ItemAct::Volume(_)) => warn_missing("SetVolume"),
        Some(ItemAct::ToggleMute) => warn_missing("ToggleMute"),
        Some(ItemAct::Push(_)) | Some(ItemAct::Pop) | Some(ItemAct::Nothing) | None => {}
    }
    let _ = ui;
}

/// Perform a `[n]` pick from a text screen.
pub fn apply_pick(ui: &mut UiState, queue: &mut DebugQueue, pick: Pick) {
    match pick {
        Pick::Cmd(c) => queue.push(c),
        Pick::Cmds(cs) => {
            for c in cs {
                queue.push(c);
            }
        }
        Pick::Missing(what) => warn_missing(what),
        Pick::ToggleMinimap => ui.minimap_toggle_requested = true,
    }
}

/// One line per missing command, so a play session says exactly what `debug.rs` still needs.
fn warn_missing(what: &str) {
    warn!("ui: no DebugCommand for {what} — the row is inert (see the ui lane report)");
}
