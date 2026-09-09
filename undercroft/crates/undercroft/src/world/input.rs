//! Keyboard, mouse and pointer lock → [`MoveIntent`] and `DebugCommand::Key`
//! (`main.js:80–95 updatePlayer`'s key block, `main.js:520–595` pointer lock / mousemove / keydown).
//!
//! The world lane is the only writer of [`MoveIntent`] (PHASE2_LANES §1). Two schedules are
//! involved, on purpose:
//!
//! - `Update` accumulates what the frame produced — raw pointer deltas into [`LookAccum`], every
//!   key *press* into [`DebugQueue`] as `DebugCommand::Key(code)`, and the pointer-lock flow.
//! - `FixedUpdate` / [`SimSet::Input`] turns the held keys plus the accumulated deltas into one
//!   [`MoveIntent`] per tick, which `player.rs` consumes and clears in [`SimSet::Player`].
//!
//! PHASE2_LANES §2 says "`Update`" and PHASE2_SKELETON §5 says "`SimSet::Input`"; splitting it this
//! way satisfies both and is the only correct option: `player_movement` zeroes `MoveIntent` every
//! fixed tick, so an `Update`-only writer would drop movement on every extra tick of a slow frame,
//! while mouse deltas must be summed at frame rate to stay smooth.
//!
//! `look_dx` / `look_dy` are the *raw* pointer deltas — `player.rs` multiplies by `CFG.mouseSens` —
//! so the keyboard look (`CFG.lookKeys × dt`) is pre-divided by it here, and its sign flipped,
//! because the JS *adds* `rot` to the yaw while `player.rs` subtracts `look_dx · sens`.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseMotion;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::debug::{DebugCommand, DebugQueue};
use crate::resources::{Game, MoveIntent};
use crate::state::GameMode;

/// Pointer deltas summed since the last fixed tick (`mousemove`'s `movementX/Y`).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LookAccum {
    pub dx: f32,
    pub dy: f32,
}

/// `state.locked` / `state.lockEver` (`main.js:53`).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PointerLock {
    pub locked: bool,
    pub lock_ever: bool,
    /// True for exactly the one [`pointer_lock`] call that drops `locked` — always a *mode* change
    /// (Zone/Dying → Dead, a menu opening, …), never a click. `ui::mouse`'s death-screen handler
    /// reads this so the click that happened to be mid-flight for some other reason (re-locking the
    /// pointer, say) on that same frame cannot also register as "dismiss the screen that just
    /// appeared".
    pub released_this_frame: bool,
}

/// `KEYS` (`config.js:132`) as Bevy key codes. Only the bindings the game reads; everything else
/// still reaches `player.rs` / the ui lane through `DebugCommand::Key`.
const FORWARD: KeyCode = KeyCode::KeyW;
const BACK: KeyCode = KeyCode::KeyS;
const LEFT: KeyCode = KeyCode::KeyA;
const RIGHT: KeyCode = KeyCode::KeyD;
const SPRINT: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];
const LOOK_LEFT: KeyCode = KeyCode::ArrowLeft;
const LOOK_RIGHT: KeyCode = KeyCode::ArrowRight;
const LOOK_UP: KeyCode = KeyCode::ArrowUp;
const LOOK_DOWN: KeyCode = KeyCode::ArrowDown;

/// `KeyboardEvent.code` for a Bevy [`KeyCode`] — the string every `key` event, `DebugCommand::Key`
/// and `SimEvent::Key` carries (`main.js:552 events.emit('key', {code, mode})`).
///
/// Bevy's `KeyCode` is modelled on the same W3C UI Events table, so the names line up one for one;
/// this table is spelled out anyway because the whole key routing of `player.rs` and the ui lane
/// hangs off these exact strings. `None` means "no `code` the prototype would ever see".
pub fn js_code(key: KeyCode) -> Option<&'static str> {
    use KeyCode as K;
    Some(match key {
        // letters
        K::KeyA => "KeyA",
        K::KeyB => "KeyB",
        K::KeyC => "KeyC",
        K::KeyD => "KeyD",
        K::KeyE => "KeyE",
        K::KeyF => "KeyF",
        K::KeyG => "KeyG",
        K::KeyH => "KeyH",
        K::KeyI => "KeyI",
        K::KeyJ => "KeyJ",
        K::KeyK => "KeyK",
        K::KeyL => "KeyL",
        K::KeyM => "KeyM",
        K::KeyN => "KeyN",
        K::KeyO => "KeyO",
        K::KeyP => "KeyP",
        K::KeyQ => "KeyQ",
        K::KeyR => "KeyR",
        K::KeyS => "KeyS",
        K::KeyT => "KeyT",
        K::KeyU => "KeyU",
        K::KeyV => "KeyV",
        K::KeyW => "KeyW",
        K::KeyX => "KeyX",
        K::KeyY => "KeyY",
        K::KeyZ => "KeyZ",
        // digits (the menus read Digit1–Digit4)
        K::Digit0 => "Digit0",
        K::Digit1 => "Digit1",
        K::Digit2 => "Digit2",
        K::Digit3 => "Digit3",
        K::Digit4 => "Digit4",
        K::Digit5 => "Digit5",
        K::Digit6 => "Digit6",
        K::Digit7 => "Digit7",
        K::Digit8 => "Digit8",
        K::Digit9 => "Digit9",
        // arrows
        K::ArrowUp => "ArrowUp",
        K::ArrowDown => "ArrowDown",
        K::ArrowLeft => "ArrowLeft",
        K::ArrowRight => "ArrowRight",
        // control keys the prototype binds
        K::Escape => "Escape",
        K::Enter => "Enter",
        K::NumpadEnter => "NumpadEnter",
        K::Space => "Space",
        K::Backspace => "Backspace",
        K::Tab => "Tab",
        K::ShiftLeft => "ShiftLeft",
        K::ShiftRight => "ShiftRight",
        K::ControlLeft => "ControlLeft",
        K::ControlRight => "ControlRight",
        K::AltLeft => "AltLeft",
        K::AltRight => "AltRight",
        K::BracketLeft => "BracketLeft",
        K::BracketRight => "BracketRight",
        K::Minus => "Minus",
        K::Equal => "Equal",
        K::Comma => "Comma",
        K::Period => "Period",
        K::Slash => "Slash",
        K::Backslash => "Backslash",
        K::Semicolon => "Semicolon",
        K::Quote => "Quote",
        K::Backquote => "Backquote",
        K::Home => "Home",
        K::End => "End",
        K::Delete => "Delete",
        K::PageUp => "PageUp",
        K::PageDown => "PageDown",
        _ => return None,
    })
}

/// `mousemove` — sum the raw deltas at frame rate. The JS only looked while pointer-locked in
/// `HUB` / `ZONE` / `DYING`; `player.rs` applies the same mode filter, so the accumulation is
/// unconditional and the lock gate is here.
pub(super) fn accumulate_look(
    mut motion: MessageReader<MouseMotion>,
    lock: Res<PointerLock>,
    mode: Res<State<GameMode>>,
    mut accum: ResMut<LookAccum>,
) {
    let looking = matches!(
        *mode.get(),
        GameMode::Hub | GameMode::Zone | GameMode::Dying
    );
    for m in motion.read() {
        if lock.locked && looking && m.delta.is_finite() {
            accum.dx += m.delta.x;
            accum.dy += m.delta.y;
        }
    }
}

/// `window.addEventListener('keydown')` — every press is forwarded verbatim; `player.rs` and the ui
/// lane own what it *means*. Repeats are dropped (`if (e.repeat) return`), which `just_pressed`
/// gives us for free.
pub(super) fn forward_key_presses(
    mut keys: MessageReader<KeyboardInput>,
    mut queue: ResMut<DebugQueue>,
) {
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed || ev.repeat {
            continue;
        }
        if let Some(code) = js_code(ev.key_code) {
            queue.push(DebugCommand::Key(code.to_string()));
        }
    }
}

/// `canvas.click → requestPointerLock` / `releaseLock` (`main.js:539–550`). A click locks the
/// pointer in `HUB` and `ZONE`; `Escape`, a menu, the title and the death screen release it.
///
/// X11 has no true pointer lock (winit falls back to `Confined`), which is why the deltas are read
/// from [`MouseMotion`] rather than from the cursor position.
pub(super) fn pointer_lock(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<GameMode>>,
    mut lock: ResMut<PointerLock>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    lock.released_this_frame = false;
    let Ok(mut cursor) = cursor.single_mut() else {
        return;
    };
    let playing = matches!(*mode.get(), GameMode::Hub | GameMode::Zone);
    let keep = playing || (*mode.get() == GameMode::Dying && lock.locked);
    let mut want = lock.locked && keep;
    if playing && buttons.just_pressed(MouseButton::Left) {
        want = true;
    }
    // Escape only *releases* here; `player.rs` turns the `Key("Escape")` into the pause menu.
    if keyboard.just_pressed(KeyCode::Escape) || !keep {
        want = false;
    }
    if want != lock.locked {
        let was_locked = lock.locked;
        lock.locked = want;
        lock.lock_ever |= want;
        cursor.grab_mode = if want {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        cursor.visible = !want;
        lock.released_this_frame = was_locked && !want;
    }
}

/// `SimSet::Input` — the tick's [`MoveIntent`]: WASD, `Shift` to sprint, the arrow-key look and the
/// pointer deltas gathered since the last tick.
pub(super) fn write_move_intent(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    game: Game,
    mut accum: ResMut<LookAccum>,
    mut intent: ResMut<MoveIntent>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let cfg = &asset.data.config.cfg;
    let mut it = MoveIntent {
        look_dx: accum.dx,
        look_dy: accum.dy,
        ..default()
    };
    *accum = LookAccum::default();

    if keyboard.pressed(FORWARD) {
        it.forward += 1.0;
    }
    if keyboard.pressed(BACK) {
        it.forward -= 1.0;
    }
    if keyboard.pressed(LEFT) {
        it.strafe -= 1.0;
    }
    if keyboard.pressed(RIGHT) {
        it.strafe += 1.0;
    }
    it.sprint = SPRINT.iter().any(|k| keyboard.pressed(*k));

    // `const rot = CFG.lookKeys * dt` added to yaw / pitch; `player.rs` does
    // `yaw -= look_dx * mouseSens`, so feed it `-rot / mouseSens`.
    let rot = cfg.look_keys * time.delta_secs() / cfg.mouse_sens;
    if keyboard.pressed(LOOK_LEFT) {
        it.look_dx -= rot;
    }
    if keyboard.pressed(LOOK_RIGHT) {
        it.look_dx += rot;
    }
    if keyboard.pressed(LOOK_UP) {
        it.look_dy -= rot;
    }
    if keyboard.pressed(LOOK_DOWN) {
        it.look_dy += rot;
    }
    *intent = it;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spot checks across the table: every key the prototype binds keeps its JS `code`.
    #[test]
    fn js_codes_match_the_prototype_bindings() {
        for (key, code) in [
            (KeyCode::KeyW, "KeyW"),
            (KeyCode::KeyA, "KeyA"),
            (KeyCode::KeyS, "KeyS"),
            (KeyCode::KeyD, "KeyD"),
            (KeyCode::KeyE, "KeyE"),
            (KeyCode::KeyF, "KeyF"),
            (KeyCode::KeyQ, "KeyQ"),
            (KeyCode::KeyR, "KeyR"),
            (KeyCode::KeyT, "KeyT"),
            (KeyCode::KeyM, "KeyM"),
            (KeyCode::Digit1, "Digit1"),
            (KeyCode::Digit4, "Digit4"),
            (KeyCode::ArrowUp, "ArrowUp"),
            (KeyCode::ArrowDown, "ArrowDown"),
            (KeyCode::ArrowLeft, "ArrowLeft"),
            (KeyCode::ArrowRight, "ArrowRight"),
            (KeyCode::Escape, "Escape"),
            (KeyCode::Enter, "Enter"),
            (KeyCode::Space, "Space"),
            (KeyCode::Backspace, "Backspace"),
            (KeyCode::Tab, "Tab"),
            (KeyCode::ShiftLeft, "ShiftLeft"),
            (KeyCode::ShiftRight, "ShiftRight"),
            (KeyCode::BracketLeft, "BracketLeft"),
            (KeyCode::BracketRight, "BracketRight"),
        ] {
            assert_eq!(js_code(key), Some(code), "{key:?}");
        }
        assert_eq!(js_code(KeyCode::F13), None, "unbound keys are dropped");
    }

    /// Every letter and digit round-trips to the `Key<X>` / `Digit<N>` form.
    #[test]
    fn every_letter_and_digit_is_mapped() {
        use KeyCode as K;
        let letters = [
            K::KeyA,
            K::KeyB,
            K::KeyC,
            K::KeyD,
            K::KeyE,
            K::KeyF,
            K::KeyG,
            K::KeyH,
            K::KeyI,
            K::KeyJ,
            K::KeyK,
            K::KeyL,
            K::KeyM,
            K::KeyN,
            K::KeyO,
            K::KeyP,
            K::KeyQ,
            K::KeyR,
            K::KeyS,
            K::KeyT,
            K::KeyU,
            K::KeyV,
            K::KeyW,
            K::KeyX,
            K::KeyY,
            K::KeyZ,
        ];
        for (i, k) in letters.iter().enumerate() {
            let want = format!("Key{}", (b'A' + i as u8) as char);
            assert_eq!(js_code(*k), Some(want.as_str()), "{k:?}");
        }
        let digits = [
            K::Digit0,
            K::Digit1,
            K::Digit2,
            K::Digit3,
            K::Digit4,
            K::Digit5,
            K::Digit6,
            K::Digit7,
            K::Digit8,
            K::Digit9,
        ];
        for (i, k) in digits.iter().enumerate() {
            let want = format!("Digit{i}");
            assert_eq!(js_code(*k), Some(want.as_str()), "{k:?}");
        }
    }

    /// The keyboard look is pre-divided by `CFG.mouseSens` and sign-flipped, so `player.rs`'s
    /// `yaw -= look_dx * mouseSens` reproduces the JS `yaw += CFG.lookKeys * dt`.
    #[test]
    fn keyboard_look_cancels_the_mouse_sensitivity() {
        let (look_keys, mouse_sens, dt) = (1.9f32, 0.002f32, 1.0 / 60.0);
        let rot = look_keys * dt / mouse_sens;
        // ArrowLeft: `look_dx -= rot` → `yaw -= (-rot) * sens` = `yaw += lookKeys * dt`.
        let applied = -(-rot) * mouse_sens;
        assert!((applied - look_keys * dt).abs() < 1e-6, "{applied}");
    }
}
