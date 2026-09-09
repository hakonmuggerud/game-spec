//! Mouse plumbing for the list menus and the hub screens' `[n]` lines — `ui.js:makeListMenu`'s
//! "Mouse: hover selects, click activates" (`ui.js:155`), plus `ui.js:19`'s click-anywhere death
//! screen and `endgame.js`'s "Click to continue".
//!
//! The workspace's Bevy build deliberately excludes `bevy_picking`, so this rides on `bevy_ui`'s
//! classic [`Interaction`] component (updated by `ui_focus_system`, which needs no `Button` marker
//! or `FocusPolicy` — see `bevy_ui::focus`) rather than `Pointer<Click>` observers. [`render::MenuRow`]
//! / [`render::PickLine`] tag the row/line entities `render.rs` spawns with the index a click or
//! hover should act on; everything here turns that back into the exact same call the keyboard makes
//! (`ui::menu::MenuState::activate`, `ui::keys::apply_item` / `apply_pick`), so the resulting
//! [`crate::debug::DebugCommand`] is dispatched identically.
//!
//! Registered only when [`crate::has_renderer`] is true (`ui::plugin`): every system here reads a
//! resource — `ButtonInput<MouseButton>`, `CursorMoved`, [`PointerLock`] — that the headless harness
//! never inserts (no `InputPlugin`, no `WindowPlugin`, no world lane).

use bevy::prelude::*;
use bevy::window::CursorMoved;

use crate::debug::DebugQueue;
use crate::resources::Game;
use crate::world::input::PointerLock;

use super::keys::{apply_item, apply_pick};
use super::render::{MenuRow, PickLine};
use super::UiState;

/// `ui.js:190–200`'s row `mousemove`: hover-select rides on a *real* cursor move only. Every
/// selection change respawns the row entities (`ui::render_panels`), so a resting cursor would
/// otherwise land on a freshly spawned row next frame and re-select it — indistinguishable, from
/// `Interaction` alone, from a genuine hover — undoing whatever the keyboard just did. Gating on an
/// actual change of [`CursorMoved::position`] is this port's `m.mx`/`m.my` check, including across a
/// pointer-lock release (no synthetic move is ever posted here, so there is nothing to filter out
/// beyond "did the OS report a new position").
pub(super) fn hover_rows(
    mut motion: MessageReader<CursorMoved>,
    mut last: Local<Option<Vec2>>,
    rows: Query<(&MenuRow, &Interaction)>,
    mut ui: ResMut<UiState>,
) {
    let mut moved = false;
    for m in motion.read() {
        if *last != Some(m.position) {
            *last = Some(m.position);
            moved = true;
        }
    }
    if !moved {
        return;
    }
    if let Some((row, _)) = rows.iter().find(|(_, i)| **i == Interaction::Hovered) {
        ui.menu.hover(row.0);
    }
}

/// A row's click. `Interaction::Pressed` fires once, on the mousedown that produced it
/// (`Changed<Interaction>`, guarded to the transition into `Pressed`), and is routed through
/// [`crate::ui::menu::MenuState::activate`] — the exact call `KEY_GO` makes — so
/// [`crate::ui::keys::apply_item`] cannot tell it apart from a keyboard activation. A disabled row's
/// click is a no-op there, same as the keyboard's.
pub(super) fn click_rows(
    rows: Query<(&MenuRow, &Interaction), Changed<Interaction>>,
    game: Game,
    mut ui: ResMut<UiState>,
    mut queue: ResMut<DebugQueue>,
) {
    let Some(i) = rows
        .iter()
        .find(|(_, interaction)| **interaction == Interaction::Pressed)
        .map(|(row, _)| row.0)
    else {
        return;
    };
    let ctx = ui.menu_ctx.clone();
    let act = ui.menu.activate(i, &ctx);
    apply_item(&game, &mut ui, &mut queue, act);
}

/// A hub screen's `[n]` line, clicked — `ui::keys::menu_key`'s digit branch, minus the keyboard.
/// There is no hover state for these (`ui.js` had none either): a click either lands on a pick or,
/// for a line the screen left without one, does nothing.
pub(super) fn click_screen_picks(
    lines: Query<(&PickLine, &Interaction), Changed<Interaction>>,
    ui: Res<UiState>,
    mut queue: ResMut<DebugQueue>,
) {
    let Some(n) = lines
        .iter()
        .find(|(_, interaction)| **interaction == Interaction::Pressed)
        .map(|(pick, _)| pick.0)
    else {
        return;
    };
    let Some(screen) = &ui.text_screen else {
        return;
    };
    if let Some(Some(p)) = screen.picks.get(n - 1) {
        apply_pick(&mut queue, p.clone());
    }
}

/// A click anywhere on a screen whose `cta` is showing ("Click to return to the Lantern" /
/// "Click to continue") — `ui.js:19`'s `dom.death.addEventListener('click', ...)` and
/// `endgame.js`'s ending screen, generalised over [`crate::ui::screens::TextScreen::confirm`] so
/// both drive the same [`apply_pick`] the death/ending keyboard paths use.
///
/// Guarded against the one frame a *mode* change — not this click — drops the pointer lock (Zone/
/// Dying → Dead): [`PointerLock::released_this_frame`] is true only then, so the click that was mid-
/// flight for some other reason (re-locking the pointer, say) cannot double as "dismiss the death
/// screen" before the player has even seen it.
pub(super) fn click_screen_cta(
    buttons: Res<ButtonInput<MouseButton>>,
    lock: Res<PointerLock>,
    ui: Res<UiState>,
    mut queue: ResMut<DebugQueue>,
) {
    if !buttons.just_pressed(MouseButton::Left) || lock.released_this_frame {
        return;
    }
    let Some(screen) = &ui.text_screen else {
        return;
    };
    if screen.cta.is_empty() {
        return;
    }
    if let Some(p) = screen.confirm.clone() {
        apply_pick(&mut queue, p);
    }
}
