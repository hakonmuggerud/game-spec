//! LANE: creatures. Every creature in the ruin — the port of `hunter.js` (DESIGN.md §5, §5.8).
//!
//! Layout:
//! * [`record`] — the flat creature record (`makeHunter`), [`ProfileKind`], [`HState`], [`Anim`].
//! * [`profiles`] — the profile table (`profile` / `buildProfiles`): numbers from [`Config`] plus the hooks and
//!   FSM tables as function pointers; [`Tuning`] is the module's read-only config.
//! * [`senses`] — `stimAt`, `genericSense`, `targetPos`.
//! * [`driver`] — `setState`, `escapeIfTrapped`, `pathToPoint`, `pickWander`, `enterChase`, `chasePath`,
//!   `investigate`, `stagger`, `onFlash`, the 0.2 s `tick`, `updateOne` / `update` and `main.js:flash` targeting.
//! * [`spawn`] — `spawnAll`, `spawnHunter`, `spawnCreature`, `reset`, `clear`, the event listeners and `hint`.
//! * [`base`], [`lampwight`], [`warden`], [`drowner`], [`false_light`], [`brute`] — one module per FSM.
//!
//! Inputs per frame are an [`Env`] (map, pool bitmap, player snapshot, planted lanterns, world items, mode,
//! clock) and `dt`; every state change appends to `Ctx::events`. Randomness goes through
//! [`crate::rng::RandomSource`]. Lantern pools are *read* here (`Env::pool`) and computed by [`crate::pool`]
//! (world lane): a Brute smash emits `lanternRemoved` + `lanternSmashed` and the shell recomputes the pool.

pub mod base;
pub mod brute;
pub mod driver;
pub mod drowner;
pub mod false_light;
pub mod lampwight;
pub mod profiles;
pub mod record;
pub mod senses;
pub mod spawn;
pub mod warden;

pub use driver::{
    flash, flash_targets, investigate, on_flash, pick_wander, stagger, update, update_one,
};
pub use profiles::{Fsm, Hooks, Move, ProfileDef, ProfileError, StateRow, Tuning};
pub use record::{Anim, HState, Home, Hunter, ProfileKind, SpawnOpts, Target};
pub use spawn::{
    clear, clear_paths, hint, make_hunter, on_npc_caught, reset, spawn_all, spawn_creature,
    spawn_hunter,
};

use crate::events::SimEvent;
use crate::player::PlayerView;
use crate::pool::Pool;
use crate::rng::RandomSource;
use undercroft_data::config::Config;
use undercroft_data::ParsedMap;

/// The world as the creatures see it for one frame (what `hunter.js` read off `ctx`).
#[derive(Clone, Copy)]
pub struct Env<'a> {
    /// `ctx.zone.map`.
    pub map: &'a ParsedMap,
    /// `map.pool` — the lantern-pool bitmap (`crate::pool::recompute`).
    pub pool: &'a Pool,
    /// `ctx.player` (+ `ctx.npc.stimulus()` as `player.follower`).
    pub player: &'a PlayerView,
    /// `ctx.lanterns` — planted lantern positions (never a false light).
    pub lanterns: &'a [(f32, f32)],
    /// `ctx.items` — world item positions (the false light's rest-spot lure score).
    pub items: &'a [(f32, f32)],
    /// `ctx.state.mode === 'ZONE'`.
    pub in_zone: bool,
    /// `ctx.state.time` — the run clock (animation phases only).
    pub time: f32,
}

/// Mutable per-call context: the frame's [`Env`], the [`Tuning`], the RNG and the event sink.
pub struct Ctx<'a> {
    pub env: &'a Env<'a>,
    pub tuning: &'a Tuning,
    pub rng: &'a mut dyn RandomSource,
    pub events: &'a mut Vec<SimEvent>,
}

impl<'a> Ctx<'a> {
    /// Bundle the frame's inputs.
    pub fn new(
        env: &'a Env<'a>,
        tuning: &'a Tuning,
        rng: &'a mut dyn RandomSource,
        events: &'a mut Vec<SimEvent>,
    ) -> Ctx<'a> {
        Ctx {
            env,
            tuning,
            rng,
            events,
        }
    }

    /// `ctx.events.emit(...)`.
    #[inline]
    pub fn emit(&mut self, e: SimEvent) {
        self.events.push(e);
    }

    /// `ctx.zone.map`.
    #[inline]
    pub fn map(&self) -> &'a ParsedMap {
        self.env.map
    }

    /// `PROFILES[h.profile]`.
    #[inline]
    pub fn prof(&self, kind: ProfileKind) -> &'a ProfileDef {
        self.tuning.prof(kind)
    }

    /// `(Math.random() * n) | 0` — a random index in `0..n` (`n > 0`).
    pub fn rand_index(&mut self, n: usize) -> usize {
        debug_assert!(n > 0, "rand_index() needs a non-empty range");
        ((self.rng.unit() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

/// What a sense returned (`genericSense` / `wardenSense` / `drownerSense` / `falseLightSense` results).
#[derive(Debug, Clone, PartialEq)]
pub struct Stim {
    pub kind: Target,
    /// The follower's NPC id when `kind` is `Npc`.
    pub id: Option<String>,
    pub x: f32,
    pub z: f32,
    pub d: f32,
    /// Drowner only: SUBMERGED → SURFACING trigger.
    pub trigger: bool,
    /// Drowner only: keeps it surfaced.
    pub keep: bool,
    /// Drowner only: a lit lamp to home on while submerged.
    pub home: bool,
    /// `t.stim !== false` — counts as a stimulus for `lastKnown` / `noStimT` (the Drowner's `home`-only result
    /// does not).
    pub stim: bool,
}

impl Stim {
    /// A plain stimulus at a target (`{kind, id, x, z, d}`).
    pub fn at(kind: Target, id: Option<String>, x: f32, z: f32, d: f32) -> Stim {
        Stim {
            kind,
            id,
            x,
            z,
            d,
            trigger: false,
            keep: false,
            home: false,
            stim: true,
        }
    }
}

/// What a flash did to a creature (`prof.onFlash` return values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashResult {
    None,
    Stagger,
    Flinch,
    Sink,
    Reveal,
    Abort,
}

impl FlashResult {
    /// The JS string.
    pub fn js_name(self) -> &'static str {
        match self {
            FlashResult::None => "none",
            FlashResult::Stagger => "stagger",
            FlashResult::Flinch => "flinch",
            FlashResult::Sink => "sink",
            FlashResult::Reveal => "reveal",
            FlashResult::Abort => "abort",
        }
    }
}

/// Build the profile table from a loaded config (`hunter.js:init` → `buildProfiles`).
pub fn tuning(cfg: &Config) -> Result<Tuning, ProfileError> {
    Tuning::from_config(cfg)
}

#[cfg(test)]
mod tests;
