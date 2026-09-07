//! The flat creature record (`hunter.js:makeHunter`, lines ~55–73): one stable shape for every profile, so the
//! driver, the tests and (later) the audio never depend on which creature they are looking at. THREE objects
//! (`group`, `eyes`, `light`, `ring`, `embers`, `debris`) are replaced by plain animation state in [`Anim`]
//! that the Bevy shell renders.

use undercroft_data::zone::{CreatureSpawn, Facing};
use undercroft_data::CreatureKind;

/// The seven behaviour profiles (`hunter.js:PROFILES` keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProfileKind {
    Base,
    Fast,
    Lampwight,
    Warden,
    Drowner,
    FalseLight,
    Brute,
}

impl ProfileKind {
    /// Every profile, in `buildProfiles` order.
    pub const ALL: [ProfileKind; 7] = [
        ProfileKind::Base,
        ProfileKind::Fast,
        ProfileKind::Lampwight,
        ProfileKind::Warden,
        ProfileKind::Drowner,
        ProfileKind::FalseLight,
        ProfileKind::Brute,
    ];

    /// The JS profile name (`'base'`, `'falseLight'` …) — also the `models.js` factory name for creatures.
    pub fn js_name(self) -> &'static str {
        match self {
            ProfileKind::Base => "base",
            ProfileKind::Fast => "fast",
            ProfileKind::Lampwight => "lampwight",
            ProfileKind::Warden => "warden",
            ProfileKind::Drowner => "drowner",
            ProfileKind::FalseLight => "falseLight",
            ProfileKind::Brute => "brute",
        }
    }

    /// Parse a JS profile name; `None` for an unknown one (the JS falls back to `base`, see
    /// [`ProfileKind::from_js_name_or_base`]).
    pub fn from_js_name(name: &str) -> Option<ProfileKind> {
        ProfileKind::ALL.into_iter().find(|p| p.js_name() == name)
    }

    /// `PROFILES[profile] || PROFILES.base` — the `makeHunter` fallback.
    pub fn from_js_name_or_base(name: &str) -> ProfileKind {
        ProfileKind::from_js_name(name).unwrap_or(ProfileKind::Base)
    }

    /// The profile a creature cell spawns (`spawnAll`: `profile = c.kind`).
    pub fn from_creature(kind: CreatureKind) -> ProfileKind {
        match kind {
            CreatureKind::Lampwight => ProfileKind::Lampwight,
            CreatureKind::Warden => ProfileKind::Warden,
            CreatureKind::FalseLight => ProfileKind::FalseLight,
            CreatureKind::Drowner => ProfileKind::Drowner,
            CreatureKind::Brute => ProfileKind::Brute,
        }
    }

    /// `prof.creature` — everything but `base` / `fast` (the endgame counts only plain hunters).
    pub fn is_creature(self) -> bool {
        !matches!(self, ProfileKind::Base | ProfileKind::Fast)
    }
}

/// Every FSM state name any profile uses (the union of `BASE_FSM`, `LAMPWIGHT_FSM`, `WARDEN_FSM`, `DROWNER_FSM`,
/// `FALSELIGHT_FSM` and `BRUTE_FSM` keys). Which states a profile accepts is the profile's table
/// (`crate::creature::profiles::Fsm::row`); a state the profile does not know resets it to its initial state,
/// as `hunter.js:stateDef` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum HState {
    // base / fast / brute
    Wander,
    Investigate,
    Chase,
    Staggered,
    // lampwight
    Drift,
    Drawn,
    Snuff,
    Sated,
    // warden
    Sentry,
    Alert,
    Return,
    Flinch,
    // drowner
    Submerged,
    Surfacing,
    Surge,
    Lurk,
    Sink,
    // false light
    Lit,
    Dark,
    Pounce,
    Retreat,
    Relight,
    Revealed,
}

impl HState {
    /// Number of states (the per-state tables are arrays this long).
    pub const COUNT: usize = 23;

    /// Every state in discriminant order.
    pub const ALL: [HState; HState::COUNT] = [
        HState::Wander,
        HState::Investigate,
        HState::Chase,
        HState::Staggered,
        HState::Drift,
        HState::Drawn,
        HState::Snuff,
        HState::Sated,
        HState::Sentry,
        HState::Alert,
        HState::Return,
        HState::Flinch,
        HState::Submerged,
        HState::Surfacing,
        HState::Surge,
        HState::Lurk,
        HState::Sink,
        HState::Lit,
        HState::Dark,
        HState::Pounce,
        HState::Retreat,
        HState::Relight,
        HState::Revealed,
    ];

    /// The JS state name (`'WANDER'` …) — the `hunterState.state` payload and the `speed` / `eye` table keys.
    pub fn js_name(self) -> &'static str {
        match self {
            HState::Wander => "WANDER",
            HState::Investigate => "INVESTIGATE",
            HState::Chase => "CHASE",
            HState::Staggered => "STAGGERED",
            HState::Drift => "DRIFT",
            HState::Drawn => "DRAWN",
            HState::Snuff => "SNUFF",
            HState::Sated => "SATED",
            HState::Sentry => "SENTRY",
            HState::Alert => "ALERT",
            HState::Return => "RETURN",
            HState::Flinch => "FLINCH",
            HState::Submerged => "SUBMERGED",
            HState::Surfacing => "SURFACING",
            HState::Surge => "SURGE",
            HState::Lurk => "LURK",
            HState::Sink => "SINK",
            HState::Lit => "LIT",
            HState::Dark => "DARK",
            HState::Pounce => "POUNCE",
            HState::Retreat => "RETREAT",
            HState::Relight => "RELIGHT",
            HState::Revealed => "REVEALED",
        }
    }

    /// Parse a JS state name.
    pub fn from_js_name(name: &str) -> Option<HState> {
        HState::ALL.into_iter().find(|s| s.js_name() == name)
    }

    /// Index into a per-state table.
    #[inline]
    pub fn ix(self) -> usize {
        self as usize
    }
}

/// Who the hunter is after (`h.target`: `'player'` | `'npc'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Player,
    Npc,
}

/// The per-creature options from `META.creatures[i]` (`h.opts`): the Warden's facing / sweep / reach / territory
/// and the Brute's leash. Empty (`Default`) for hunters and debug spawns.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpawnOpts {
    pub facing: Option<Facing>,
    /// Degrees.
    pub sweep: Option<f32>,
    pub reach: Option<f32>,
    /// Cone half-angle (degrees); `h.opts.cone` in the JS — never set by the zone data.
    pub cone: Option<f32>,
    pub territory: Option<f32>,
    pub leash: Option<f32>,
}

impl From<&CreatureSpawn> for SpawnOpts {
    fn from(c: &CreatureSpawn) -> SpawnOpts {
        SpawnOpts {
            facing: c.facing,
            sweep: c.sweep,
            reach: c.reach,
            cone: None,
            territory: c.territory,
            leash: c.leash,
        }
    }
}

/// The spawn cell and its centre (`h.home`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Home {
    pub cx: i32,
    pub cz: i32,
    pub x: f32,
    pub z: f32,
}

/// Render-only state the per-profile `anim` hooks produce each frame (what `hunter.js` wrote onto THREE
/// objects). The sim never reads these back; Phase 2 maps them onto the model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Anim {
    /// Eye emissive intensity (`syncMesh`: `prof.eye[state]`, 0.3 for an unlisted state).
    pub eye_k: f32,
    /// Leg swing phase (`stepPhase`, Warden / Brute) — legs at `±a·sin(phase)`.
    pub leg_phase: f32,
    /// Leg swing amplitude this frame (0 when standing).
    pub leg_swing: f32,
    /// Jaw rotation (Drowner: negative = dropped).
    pub jaw: f32,
    /// Lampwight chest ember intensity (`emberK`).
    pub ember_k: f32,
    /// Spot / point light intensity (Warden cone, false light glass) and whether it is on.
    pub light_k: f32,
    pub light_on: bool,
    /// Visor / glass emissive intensity.
    pub glass_k: f32,
    /// Drowner ripple ring scale and emissive intensity; `ring_y` its height offset.
    pub ripple_scale: f32,
    pub ripple_k: f32,
    pub ring_y: f32,
    /// Drowner: whether the body (everything but the ring) is shown.
    pub body_shown: bool,
    /// False light: posed dark (legs splayed, jaw down, glass dead).
    pub posed_dark: bool,
    /// Warden: the plinth is visible (it stands on its post).
    pub plinth: bool,
    /// Brute: body sway offset this frame.
    pub sway: f32,
    /// Brute: ember / debris burst life left (0 = hidden) and where it plays.
    pub burst_t: f32,
    pub burst_x: f32,
    pub burst_z: f32,
    /// Set for exactly one frame when a burst is fired (a smash happened this frame).
    pub burst_fired: bool,
}

/// One creature (`hunter.js:makeHunter`). Every field any profile uses, on every record.
#[derive(Debug, Clone, PartialEq)]
pub struct Hunter {
    pub id: u32,
    pub profile: ProfileKind,
    pub opts: SpawnOpts,
    /// Spawned after `spawnAll` (`spawnHunter` / `spawnCreature`).
    pub extra: bool,
    pub active: bool,
    pub state: HState,
    pub x: f32,
    pub z: f32,
    pub y: f32,
    pub yaw: f32,
    /// The rendered yaw (the Brute's turn-rate-limited one; = `yaw` for the others).
    pub yaw_vis: f32,
    /// Cell centres left to walk (`h.path`).
    pub path: Vec<(f32, f32)>,
    pub tick_t: f32,
    pub repath_t: f32,
    pub no_stim_t: f32,
    pub idle_t: f32,
    pub wait_t: f32,
    pub stagger_t: f32,
    pub daze_t: f32,
    pub unreach_t: f32,
    pub busy_t: f32,
    /// The generic per-state timer (`h.t`).
    pub t: f32,
    pub unreachable: bool,
    pub wander_far: bool,
    /// Stimulated this tick.
    pub stim: bool,
    pub last_known: (f32, f32),
    pub target: Target,
    pub target_id: Option<String>,
    pub home: Home,
    /// Animation phase offset (`Math.random() * TAU` at creation).
    pub phase: f32,
    // ---- creature fields (unused by base / fast) ----
    /// Warden: the state FLINCH resumes.
    pub prev_state: Option<HState>,
    pub snuffed: bool,
    /// Warden post position and facing.
    pub post: (f32, f32),
    pub post_yaw: f32,
    /// `CREATURE.warden.territory`, copied in by `wardenReset` so `canEnter` needs no config.
    pub territory_default: f32,
    pub sweep: f32,
    pub sweep_dir: f32,
    /// Drowner: its water body (flood fill from the spawn), one byte per cell; empty for others.
    pub body: Vec<u8>,
    pub homing: bool,
    pub lunge: (f32, f32),
    pub lunge_done: bool,
    pub rest_idx: Option<usize>,
    /// Brute: BFS-from-home distances over static solids; empty for others.
    pub leash: Vec<i16>,
    pub step_t: f32,
    pub moved: bool,
    pub wake_t: f32,
    pub ember_k: f32,
    pub step_phase: f32,
    pub posed_dark: Option<bool>,
    pub trapped: bool,
    /// Render state.
    pub anim: Anim,
}

impl Hunter {
    /// `makeHunter(id, profile)` with the phase already drawn (the caller passes `rng.unit() * TAU`).
    pub fn new(id: u32, profile: ProfileKind, initial: HState, phase: f32) -> Hunter {
        Hunter {
            id,
            profile,
            opts: SpawnOpts::default(),
            extra: false,
            active: false,
            state: initial,
            x: 0.0,
            z: 0.0,
            y: 0.0,
            yaw: 0.0,
            yaw_vis: 0.0,
            path: Vec::new(),
            tick_t: 0.0,
            repath_t: 0.0,
            no_stim_t: 0.0,
            idle_t: 1.0,
            wait_t: 0.0,
            stagger_t: 0.0,
            daze_t: 0.0,
            unreach_t: 0.0,
            busy_t: 0.0,
            t: 0.0,
            unreachable: false,
            wander_far: false,
            stim: false,
            last_known: (0.0, 0.0),
            target: Target::Player,
            target_id: None,
            home: Home::default(),
            phase,
            prev_state: None,
            snuffed: false,
            post: (0.0, 0.0),
            post_yaw: 0.0,
            territory_default: 0.0,
            sweep: 0.0,
            sweep_dir: 1.0,
            body: Vec::new(),
            homing: false,
            lunge: (0.0, 0.0),
            lunge_done: false,
            rest_idx: None,
            leash: Vec::new(),
            step_t: 0.0,
            moved: false,
            wake_t: 0.0,
            ember_k: 0.0,
            step_phase: 0.0,
            posed_dark: None,
            trapped: false,
            anim: Anim::default(),
        }
    }

    /// `h.opts.territory ?? CREATURE.warden.territory` (`hunter.js:wardenTerritory`).
    pub fn territory(&self) -> f32 {
        self.opts.territory.unwrap_or(self.territory_default)
    }

    /// Is the point inside the Warden's territory (`hunter.js:inTerritory`)?
    pub fn in_territory(&self, x: f32, z: f32) -> bool {
        crate::grid::dist2d(x, z, self.post.0, self.post.1) <= self.territory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for p in ProfileKind::ALL {
            assert_eq!(ProfileKind::from_js_name(p.js_name()), Some(p));
        }
        for (i, s) in HState::ALL.iter().enumerate() {
            assert_eq!(s.ix(), i);
            assert_eq!(HState::from_js_name(s.js_name()), Some(*s));
        }
        assert_eq!(ProfileKind::from_js_name_or_base("nope"), ProfileKind::Base);
        assert!(ProfileKind::FalseLight.is_creature());
        assert!(!ProfileKind::Fast.is_creature());
    }
}
