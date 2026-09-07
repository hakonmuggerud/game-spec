//! The profile table (`hunter.js:profile` ~802 and `buildProfiles` ~811–840): per profile the numbers from
//! `HUNTER_PROFILES` / `CREATURE`, the FSM table and the hooks (`sense`, `onFlash`, `canEnter`, `canEnterLoose`,
//! `onReset`, `onTick`, `onNearLantern`, `catchIf`, `onCatch`, `anim`) as plain function pointers. Nothing here is
//! re-typed: every number is read from [`Config`] by [`Tuning::from_config`].

use super::record::{HState, Hunter, ProfileKind};
use super::{Ctx, Env, FlashResult, Stim};
use crate::player::PlayerView;
use std::fmt;
use undercroft_data::config::{Config, CreatureCfg, HunterCfg, Senses};

/// How a state moves (`fsm[STATE].move`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// Follows `h.path` (and the `close` fallback).
    Path,
    /// Stands.
    Still,
    /// The false light's straight-line lunge at `h.lunge`.
    Lunge,
}

/// One FSM state's declarative flags (`hunter.js` state rows: `move / repath / close / catch / sweep /
/// ignoreStim / lastKnownWhenBlind / speed`). The `tick` lives in the profile's [`Fsm::tick`].
#[derive(Debug, Clone, Copy)]
pub struct StateRow {
    pub mv: Move,
    /// BFS toward the target every `HUNTER.repath` s.
    pub repath: bool,
    /// Path exhausted: walk straight at the target.
    pub close: bool,
    /// The state can catch.
    pub catch: bool,
    /// Eyes sweep while waiting (INVESTIGATE).
    pub sweep: bool,
    /// The sense is not run in this state.
    pub ignore_stim: bool,
    /// Repath to `lastKnown` (not the live target) while unstimulated.
    pub last_known_when_blind: bool,
    /// State-specific speed override (the Drowner's SUBMERGED homing speed).
    pub speed: Option<fn(&Hunter, &Tuning) -> f32>,
}

impl StateRow {
    /// A row with every flag off.
    pub const fn new(mv: Move) -> StateRow {
        StateRow {
            mv,
            repath: false,
            close: false,
            catch: false,
            sweep: false,
            ignore_stim: false,
            last_known_when_blind: false,
            speed: None,
        }
    }
    pub const fn repath(mut self) -> StateRow {
        self.repath = true;
        self
    }
    pub const fn close(mut self) -> StateRow {
        self.close = true;
        self
    }
    pub const fn catch(mut self) -> StateRow {
        self.catch = true;
        self
    }
    pub const fn sweep(mut self) -> StateRow {
        self.sweep = true;
        self
    }
    pub const fn ignore_stim(mut self) -> StateRow {
        self.ignore_stim = true;
        self
    }
    pub const fn last_known_when_blind(mut self) -> StateRow {
        self.last_known_when_blind = true;
        self
    }
    pub const fn with_speed(mut self, f: fn(&Hunter, &Tuning) -> f32) -> StateRow {
        self.speed = Some(f);
        self
    }
}

/// A profile's state machine: the row table and the per-state tick (`hunter.js:*_FSM`).
#[derive(Clone, Copy)]
pub struct Fsm {
    /// The row for a state, `None` when the profile has no such state.
    pub row: fn(HState) -> Option<StateRow>,
    /// `fsm[h.state].tick(h, t)`.
    pub tick: fn(&mut Ctx, &mut Hunter, Option<&Stim>),
}

/// `prof.sense(h)`.
pub type SenseFn = fn(&Ctx, &Hunter) -> Option<Stim>;
/// `prof.onFlash(h)`.
pub type FlashFn = fn(&mut Ctx, &mut Hunter) -> FlashResult;
/// `prof.canEnter(m, cx, cz, h)` / `canEnterLoose`.
pub type EnterFn = fn(&Env, &Hunter, i32, i32) -> bool;
/// `prof.onReset(h)` / `onTick(h)`.
pub type HookFn = fn(&mut Ctx, &mut Hunter);
/// `prof.onNearLantern(h)` — `smashed` collects the lanterns removed this frame (indices into `Env::lanterns`).
pub type NearLanternFn = fn(&mut Ctx, &mut Hunter, &mut Vec<usize>);
/// `prof.catchIf(h, p)`.
pub type CatchIfFn = fn(&Hunter, &PlayerView) -> bool;
/// `prof.anim(h, dt, time)` — writes [`super::record::Anim`] only.
pub type AnimFn = fn(&Env, &Tuning, &mut Hunter, f32);

/// The hook set of one profile (`hunter.js:buildProfiles` extras).
#[derive(Clone, Copy)]
pub struct Hooks {
    pub sense: SenseFn,
    pub on_flash: FlashFn,
    pub can_enter: EnterFn,
    pub can_enter_loose: EnterFn,
    pub on_reset: Option<HookFn>,
    pub on_tick: Option<HookFn>,
    pub on_near_lantern: Option<NearLanternFn>,
    pub catch_if: Option<CatchIfFn>,
    pub on_catch: Option<HookFn>,
    pub anim: Option<AnimFn>,
}

/// One entry of `PROFILES` (`hunter.js:profile(name, extra)`).
#[derive(Clone)]
pub struct ProfileDef {
    pub kind: ProfileKind,
    /// `models.js` factory name (`hunter` for base / fast).
    pub model: &'static str,
    /// `HUNTER_PROFILES[name].speed[STATE]` (0 for a state the profile has no entry for).
    pub speed: [f32; HState::COUNT],
    /// `HUNTER_PROFILES[name].eye[STATE]` (`None` → `syncMesh` uses 0.3).
    pub eye: [Option<f32>; HState::COUNT],
    pub catch_r: f32,
    pub lose_t: f32,
    pub scale_y: f32,
    pub eye_color: u32,
    /// `kills !== false`.
    pub kills: bool,
    /// `cfg.senses || baseSenses`.
    pub senses: Senses,
    pub creature: bool,
    pub initial: HState,
    pub pool_mul: f32,
    pub water_mul: f32,
    pub catch_in_pool: bool,
    /// Brute wander leash (BFS cells).
    pub leash: Option<i32>,
    /// Warden default territory.
    pub territory: Option<f32>,
    /// Brute stride period per state (`creatureStep`), 0.6 default for an unlisted state.
    pub step: Option<[f32; HState::COUNT]>,
    pub fsm: Fsm,
    pub hooks: Hooks,
}

impl fmt::Debug for ProfileDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileDef")
            .field("kind", &self.kind)
            .field("initial", &self.initial)
            .field("catch_r", &self.catch_r)
            .field("lose_t", &self.lose_t)
            .field("kills", &self.kills)
            .finish_non_exhaustive()
    }
}

impl ProfileDef {
    /// `prof.speed[state] || 0`.
    pub fn speed_of(&self, s: HState) -> f32 {
        self.speed[s.ix()]
    }

    /// `prof.eye[state] ?? 0.3` (`syncMesh`).
    pub fn eye_of(&self, s: HState) -> f32 {
        self.eye[s.ix()].unwrap_or(0.3)
    }

    /// `prof.step[state] || 0.6`.
    pub fn step_of(&self, s: HState) -> f32 {
        match self.step {
            Some(t) if t[s.ix()] > 0.0 => t[s.ix()],
            _ => 0.6,
        }
    }

    /// The row for the profile's state (`prof.fsm[state]`).
    pub fn row(&self, s: HState) -> Option<StateRow> {
        (self.fsm.row)(s)
    }
}

/// A config problem found while building the table.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("config.hunter_profiles has no `{0}` entry")]
    MissingProfile(&'static str),
    #[error("config.hunter_profiles.{profile}.{table} names an unknown state `{state}`")]
    UnknownState {
        profile: &'static str,
        table: &'static str,
        state: String,
    },
}

/// Everything the creatures read from `config.js`, plus the built profile table — the equivalent of the
/// module-level `H`, `CR`, `ctx.cfg.hunterWaterMul`, `CFG.flashRange/flashDot` and `PROFILES`.
#[derive(Debug, Clone)]
pub struct Tuning {
    /// `HUNTER`.
    pub hunter: HunterCfg,
    /// `CREATURE`.
    pub creature: CreatureCfg,
    /// `CFG.hunterWaterMul`.
    pub hunter_water_mul: f32,
    /// `CFG.flashRange` / `CFG.flashDot` (`main.js:flash` targeting).
    pub flash_range: f32,
    pub flash_dot: f32,
    profiles: Vec<ProfileDef>,
}

impl Tuning {
    /// `hunter.js:buildProfiles` over a loaded [`Config`].
    pub fn from_config(cfg: &Config) -> Result<Tuning, ProfileError> {
        let mut profiles = Vec::with_capacity(ProfileKind::ALL.len());
        for kind in ProfileKind::ALL {
            profiles.push(build_profile(cfg, kind)?);
        }
        Ok(Tuning {
            hunter: cfg.hunter.clone(),
            creature: cfg.creature.clone(),
            hunter_water_mul: cfg.cfg.hunter_water_mul,
            flash_range: cfg.cfg.flash_range,
            flash_dot: cfg.cfg.flash_dot,
            profiles,
        })
    }

    /// `PROFILES[kind]`.
    pub fn prof(&self, kind: ProfileKind) -> &ProfileDef {
        &self.profiles[kind as usize]
    }

    /// Every profile in table order.
    pub fn profiles(&self) -> &[ProfileDef] {
        &self.profiles
    }
}

fn state_table(
    profile: &'static str,
    table: &'static str,
    map: &std::collections::BTreeMap<String, f32>,
) -> Result<[Option<f32>; HState::COUNT], ProfileError> {
    let mut out = [None; HState::COUNT];
    for (k, v) in map {
        let s = HState::from_js_name(k).ok_or_else(|| ProfileError::UnknownState {
            profile,
            table,
            state: k.clone(),
        })?;
        out[s.ix()] = Some(*v);
    }
    Ok(out)
}

/// `hunter.js:profile(name, extra)` merged with the per-profile `extra` of `buildProfiles`.
fn build_profile(cfg: &Config, kind: ProfileKind) -> Result<ProfileDef, ProfileError> {
    use super::{base, brute, drowner, false_light, lampwight, warden};
    let name = kind.js_name();
    let hp = cfg
        .hunter_profiles
        .get(name)
        .ok_or(ProfileError::MissingProfile(name))?;
    let speed = state_table(name, "speed", &hp.speed)?.map(|v| v.unwrap_or(0.0));
    let eye = state_table(name, "eye", &hp.eye)?;
    let h = &cfg.hunter;
    // `baseSenses` — the shared HUNTER ranges, follower on.
    let senses = hp.senses.clone().unwrap_or(Senses {
        lamp: h.lamp_r,
        sprint: h.sprint_r,
        walk: h.walk_r,
        still: h.still_r,
        water: h.water_r,
        follower: true,
        proximity: None,
    });
    let base_hooks = Hooks {
        sense: super::senses::generic_sense,
        on_flash: base::base_flash,
        can_enter: base::not_blocked,
        can_enter_loose: base::not_solid,
        on_reset: None,
        on_tick: None,
        on_near_lantern: None,
        catch_if: None,
        on_catch: None,
        anim: None,
    };
    let mut def = ProfileDef {
        kind,
        model: name,
        speed,
        eye,
        catch_r: hp.catch_r,
        lose_t: hp.lose_t,
        scale_y: hp.scale_y,
        eye_color: hp.eye_color,
        kills: hp.kills,
        senses,
        creature: true,
        initial: HState::Wander,
        pool_mul: 1.0,
        water_mul: cfg.cfg.hunter_water_mul,
        catch_in_pool: false,
        leash: None,
        territory: None,
        step: None,
        fsm: base::BASE_FSM,
        hooks: base_hooks,
    };
    match kind {
        ProfileKind::Base | ProfileKind::Fast => {
            def.creature = false;
            def.model = "hunter";
        }
        ProfileKind::Lampwight => {
            def.initial = HState::Drift;
            def.fsm = lampwight::LAMPWIGHT_FSM;
            def.hooks.on_flash = lampwight::on_flash;
            def.hooks.catch_if = Some(lampwight::catch_if);
            def.hooks.on_catch = Some(lampwight::on_catch);
            def.hooks.anim = Some(lampwight::anim);
        }
        ProfileKind::Warden => {
            def.initial = HState::Sentry;
            def.fsm = warden::WARDEN_FSM;
            def.hooks.sense = warden::sense;
            def.hooks.on_flash = warden::on_flash;
            def.hooks.can_enter = warden::can_enter;
            def.hooks.on_reset = Some(warden::on_reset);
            def.hooks.anim = Some(warden::anim);
            def.territory = Some(cfg.creature.warden.territory);
        }
        ProfileKind::Drowner => {
            def.initial = HState::Submerged;
            def.fsm = drowner::DROWNER_FSM;
            def.hooks.sense = drowner::sense;
            def.hooks.on_flash = drowner::on_flash;
            def.water_mul = 1.0;
            def.hooks.can_enter = drowner::can_enter;
            def.hooks.can_enter_loose = drowner::can_enter_loose;
            def.hooks.on_reset = Some(drowner::on_reset);
            def.hooks.on_tick = Some(drowner::guard);
            def.hooks.anim = Some(drowner::anim);
        }
        ProfileKind::FalseLight => {
            def.initial = HState::Lit;
            def.fsm = false_light::FALSELIGHT_FSM;
            def.hooks.sense = false_light::sense;
            def.hooks.on_flash = false_light::on_flash;
            def.hooks.anim = Some(false_light::anim);
        }
        ProfileKind::Brute => {
            def.fsm = brute::BRUTE_FSM;
            def.hooks.on_flash = brute::on_flash;
            def.hooks.can_enter = base::not_solid;
            def.pool_mul = cfg.creature.brute.pool_mul;
            def.catch_in_pool = true;
            def.leash = Some(cfg.creature.brute.leash);
            def.step = Some(
                state_table(name, "step", &cfg.creature.brute.step)?.map(|v| v.unwrap_or(0.0)),
            );
            def.hooks.on_reset = Some(brute::on_reset);
            def.hooks.on_near_lantern = Some(brute::near_lantern);
            def.hooks.anim = Some(brute::anim);
        }
    }
    Ok(def)
}
