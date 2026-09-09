//! World lane: the camera, keyboard/mouse input and 3D rendering of the map. Ports
//! `world.js` (map geometry, gates, shortcuts, water) and the camera/input/render portions of
//! `main.js` (`animate`, pointer lock, `updateCamera`).
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).

use bevy::prelude::*;

/// Nothing yet — the skeleton stub. Stage-2 fan-out fills this in.
pub fn plugin(_app: &mut App) {}
