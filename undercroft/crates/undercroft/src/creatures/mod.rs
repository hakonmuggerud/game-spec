//! Creatures lane: the visual side of hunters and other creatures. Ports the rendering half of
//! `hunter.js` (mesh, eye glow, animation hooks) and `models.js` (the per-profile meshes). The sim
//! side (`undercroft_sim::creature`) is authoritative; this lane only mirrors `Zone.hunters` onto
//! entities, indexed by `Hunter.id`, and never owns creature state.
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).

use bevy::prelude::*;

/// Nothing yet — the skeleton stub. Stage-2 fan-out fills this in.
pub fn plugin(_app: &mut App) {}
