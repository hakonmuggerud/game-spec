//! Audio lane: ports `audio.js` (ambience, footsteps, one-shot stings, the ducking rules).
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs the renderer's asset pipeline
//! for sound assets, and must stay out of the headless harness (`crate::headless`).

use bevy::prelude::*;

/// Nothing yet — the skeleton stub. Stage-2 fan-out fills this in.
pub fn plugin(_app: &mut App) {}
