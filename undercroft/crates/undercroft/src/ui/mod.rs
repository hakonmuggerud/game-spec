//! UI lane: the HUD, menus and screens. Ports `ui.js` in full, plus the HUD/menu/screen portions
//! of `main.js` (fade overlay, prompts), `hub.js` (departure board, build/service panels) and
//! `endgame.js` (the altar choice and ending screens).
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).

use bevy::prelude::*;

/// Nothing yet — the skeleton stub. Stage-2 fan-out fills this in.
pub fn plugin(_app: &mut App) {}
