//! Hub lane: the hub scene. Ports `hub.js` (flame tiers, resident placement) in full, the
//! `buildHub`/prop/sconce portions of `world.js`, and the visual half of `npc.js` (resident and
//! follower meshes, idle/bob animation).
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).

use bevy::prelude::*;

/// Nothing yet — the skeleton stub. Stage-2 fan-out fills this in.
pub fn plugin(_app: &mut App) {}
