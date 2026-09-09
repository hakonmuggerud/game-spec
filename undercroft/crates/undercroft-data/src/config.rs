//! Every export of `reference/prototype/src/config.js` as typed structs (plus the module-local tuning blocks of
//! `npc.js` `NPC_CFG`, `contracts.js` `CONTRACT_CFG`, `hub.js` `HUB_CFG` and `endgame.js` `ENDGAME`).
//! Field names are the snake_case of the JS names; colours are `0xRRGGBB` as `u32`.
//! Loaded from `assets/data/config.ron` (written by `json2ron` from the exporter's `config.json`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The whole of `config.js` (`config.js:*`), one struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// `config.js:HUB_OX` — hub grid x offset in world units (60).
    pub hub_ox: i32,
    /// `config.js:SAVE_KEY` — the v2 localStorage key.
    pub save_key: String,
    /// `config.js:SAVE_KEY_V1`.
    pub save_key_v1: String,
    /// `config.js:CFG`.
    pub cfg: Cfg,
    /// `config.js:HUNTER` — shared senses/timers.
    pub hunter: HunterCfg,
    /// `config.js:HUNTER_PROFILES` keyed by profile name (`base`, `fast`, `lampwight`, `warden`, `drowner`,
    /// `falseLight`, `brute`).
    pub hunter_profiles: BTreeMap<String, HunterProfile>,
    /// `config.js:CREATURE` — creature-specific timers and geometry.
    pub creature: CreatureCfg,
    /// `config.js:TIERS` — flame tiers, index 0 = tier 1.
    pub tiers: Vec<Tier>,
    /// `config.js:SCONCE_INT` — hub alcove sconce intensity per tier.
    pub sconce_int: [f32; 4],
    /// `config.js:POINTS` — flame points per banked item kind.
    pub points: Points,
    /// `config.js:LABEL` — display names per item kind.
    pub label: Labels,
    /// `config.js:LIGHT_TECH` — Workshop tiers, index 0 = none.
    pub light_tech: Vec<LightTech>,
    /// `config.js:BUILD_COSTS`.
    pub build_costs: BuildCosts,
    /// `config.js:AUDIO` — master numbers (later phase).
    pub audio: AudioCfg,
    /// `config.js:KEYS` — action → `KeyboardEvent.code` list (single bindings are one-element lists).
    pub keys: BTreeMap<String, Vec<String>>,
    /// `config.js:TOOLS` — tool id → display name.
    pub tools: BTreeMap<String, String>,
    /// `config.js:HUB_WARMTH`.
    pub hub_warmth: HubWarmth,
    /// `config.js:EMBERS`.
    pub embers: Embers,
    /// `config.js:HUB_BLOCK` — hub collision mask bits and collider thresholds.
    pub hub_block: HubBlock,
    /// `npc.js:NPC_CFG`.
    pub npc_cfg: NpcCfg,
    /// `contracts.js:CONTRACT_CFG`.
    pub contract_cfg: ContractCfg,
    /// `hub.js:HUB_CFG`.
    pub hub_cfg: HubCfg,
    /// `endgame.js:ENDGAME`.
    pub endgame: EndgameCfg,
}

/// `config.js:CFG.bandBurn` / `bandLamp` — the Source's per-lap closures `base + per_lap * lap`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LapLinear {
    pub base: f32,
    pub per_lap: f32,
}

impl LapLinear {
    /// Evaluate at a lap (`bandBurn(lap)` = `1.1 + 0.12 * lap`, `bandLamp(lap)` = `0.95 - 0.08 * lap`).
    pub fn at(&self, lap: i32) -> f32 {
        self.base + self.per_lap * lap as f32
    }
}

/// `config.js:CFG` — player, lamp, flash, lantern and water numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cfg {
    pub walk: f32,
    pub sprint: f32,
    pub radius: f32,
    pub eye: f32,
    pub fov: f32,
    pub fov_sprint: f32,
    pub look_keys: f32,
    pub mouse_sens: f32,
    pub oil_max: f32,
    /// Start oil per flame tier (index 0 = tier 1).
    pub start_oil: [f32; 4],
    pub burn: f32,
    pub deep_burn_mul: f32,
    pub low_oil: f32,
    pub lamp_color: u32,
    pub lamp_int: f32,
    pub lamp_dist: f32,
    pub deep_lamp_mul: f32,
    pub flash_cost: f32,
    pub flash_cd: f32,
    pub flash_dur: f32,
    pub flash_mul: f32,
    pub flash_range: f32,
    pub flash_dot: f32,
    pub flash_fx: f32,
    pub lantern_cost: f32,
    pub lantern_cd: f32,
    pub lantern_max: u32,
    pub pool_r: f32,
    pub flask_oil: f32,
    pub interact_r: f32,
    pub fade_t: f32,
    pub toast_t: f32,
    pub dying_t: f32,
    pub water_walk_mul: f32,
    pub water_sprint_mul: f32,
    pub hunter_water_mul: f32,
    pub band_burn: LapLinear,
    pub band_lamp: LapLinear,
}

/// `config.js:HUNTER` — senses and timers shared by every profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HunterCfg {
    pub tick: f32,
    pub repath: f32,
    pub lamp_r: f32,
    pub sprint_r: f32,
    pub walk_r: f32,
    pub still_r: f32,
    pub water_r: f32,
    pub stagger_t: f32,
    pub daze_t: f32,
    pub unreach_t: f32,
    pub wait_t: f32,
    pub wander_cells: i32,
    pub far_cells: i32,
    pub follower_lit_r: f32,
    pub catch_busy_t: f32,
}

/// `config.js:HUNTER_PROFILES[*].senses` — ranges the generic sense reads (0 = ignored).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Senses {
    pub lamp: f32,
    pub sprint: f32,
    pub walk: f32,
    pub still: f32,
    pub water: f32,
    pub follower: bool,
    /// The false light's proximity trigger.
    pub proximity: Option<f32>,
}

/// `config.js:HUNTER_PROFILES[name]`. `speed` / `eye` are keyed by FSM state name (the states differ per profile).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HunterProfile {
    pub speed: BTreeMap<String, f32>,
    pub eye: BTreeMap<String, f32>,
    pub catch_r: f32,
    pub lose_t: f32,
    pub scale_y: f32,
    pub eye_color: u32,
    /// `kills: false` = it never emits `hunterCatch` (the Lampwight snuffs). Plain hunters have no field: true.
    pub kills: bool,
    /// Absent for `base` / `fast`, which use `HunterCfg` ranges.
    pub senses: Option<Senses>,
}

/// `config.js:CREATURE`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatureCfg {
    pub lampwight: LampwightCfg,
    pub warden: WardenCfg,
    pub drowner: DrownerCfg,
    pub false_light: FalseLightCfg,
    pub brute: BruteCfg,
}

/// `config.js:CREATURE.lampwight`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LampwightCfg {
    pub snuff_t: f32,
    pub snuff_at: f32,
    pub sated_t: f32,
    pub sated_cells: i32,
    pub stagger_t: f32,
    pub daze_t: f32,
    pub oil: f32,
    pub lockout: f32,
}

/// `config.js:CREATURE.warden.light` — the cyan cone spotlight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WardenLight {
    pub color: u32,
    pub dist: f32,
    pub angle: f32,
    pub penumbra: f32,
    pub decay: f32,
    /// Intensity per FSM state.
    pub int: BTreeMap<String, f32>,
}

/// `config.js:CREATURE.warden`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WardenCfg {
    pub sweep: f32,
    pub sweep_rate: f32,
    pub reach: f32,
    pub cone: f32,
    pub near: f32,
    pub territory: f32,
    pub alert_t: f32,
    pub flinch_t: f32,
    pub give_up_t: f32,
    pub at_post: f32,
    pub light: WardenLight,
}

/// `config.js:CREATURE.drowner.ripple`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ripple {
    pub color: u32,
    pub k: f32,
    pub period: f32,
    pub min: f32,
    pub max: f32,
}

/// `config.js:CREATURE.drowner`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrownerCfg {
    pub trigger: f32,
    pub home: f32,
    pub home_speed: f32,
    pub drift_cells: i32,
    pub drift_t: [f32; 2],
    pub surface_t: f32,
    pub sink_t: f32,
    pub lurk_t: f32,
    pub daze_t: f32,
    pub y_sub: f32,
    pub y_surf: f32,
    pub ripple: Ripple,
}

/// `config.js:CREATURE.falseLight.light`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FalseLightLight {
    pub color: u32,
    pub int: f32,
    pub dist: f32,
    pub decay: f32,
    pub y: f32,
}

/// `config.js:CREATURE.falseLight`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FalseLightCfg {
    pub proximity: f32,
    pub dark_t: f32,
    pub pounce_t: f32,
    pub relight_t: f32,
    pub reveal_t: f32,
    pub stagger_t: f32,
    pub rest_min: i32,
    pub rest_max: i32,
    pub rest_top: i32,
    pub max_lights: u32,
    pub light: FalseLightLight,
}

/// `config.js:CREATURE.brute.embers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BruteEmbers {
    pub n: u32,
    pub rise: f32,
    pub life: f32,
    pub color: u32,
}

/// `config.js:CREATURE.brute`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BruteCfg {
    pub leash: i32,
    /// Stride period per state (`WANDER`, `INVESTIGATE`, `CHASE`).
    pub step: BTreeMap<String, f32>,
    pub smash_r: f32,
    pub pool_mul: f32,
    pub turn_rate: f32,
    pub shake_r: f32,
    pub shake_amp: f32,
    pub embers: BruteEmbers,
    pub unreach_t: f32,
}

/// `config.js:TIERS[i]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tier {
    pub pts: u32,
    pub int: f32,
    pub dist: f32,
    pub msg: String,
}

/// `config.js:POINTS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Points {
    pub oil: u32,
    pub relic: u32,
    pub rich: u32,
    pub quest: u32,
}

/// `config.js:LABEL`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Labels {
    pub oil: String,
    pub relic: String,
    pub rich: String,
    pub bundle: String,
    pub quest: String,
}

/// `config.js:LIGHT_TECH[i].cost`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelicCost {
    pub relics: u32,
    pub rich: u32,
}

/// `config.js:LIGHT_TECH[i]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightTech {
    pub dist_mul: f32,
    pub burn_mul: f32,
    pub flash_cost: f32,
    pub lantern_cost: f32,
    pub cost: RelicCost,
}

/// One building's cost/gate in `config.js:BUILD_COSTS` (only the fields the JS object has are `Some`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildCost {
    pub oil: Option<u32>,
    pub relics: Option<u32>,
    pub npc: Option<String>,
    pub tier: Option<u32>,
}

/// `config.js:BUILD_COSTS`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildCosts {
    pub workshop: BuildCost,
    pub press: BuildCost,
    pub cart: BuildCost,
    pub shrine: BuildCost,
    pub tram: BuildCost,
    pub elevator: BuildCost,
    pub press_relic_oil: u32,
    pub reservoir: [u32; 2],
    pub reservoir_oil: u32,
    pub blessing_oil: u32,
}

impl BuildCosts {
    /// Cost entry by building id (`board` has none: it is always built).
    pub fn get(&self, id: &str) -> Option<&BuildCost> {
        match id {
            "workshop" => Some(&self.workshop),
            "press" => Some(&self.press),
            "cart" => Some(&self.cart),
            "shrine" => Some(&self.shrine),
            "tram" => Some(&self.tram),
            "elevator" => Some(&self.elevator),
            _ => None,
        }
    }
}

/// `config.js:AUDIO`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioCfg {
    pub vol: f32,
    pub vol_step: f32,
    pub presence_max: f32,
    pub presence_range: f32,
    pub sting_gap: f32,
    pub drone_hub: f32,
    pub drone_zone: f32,
    pub crossfade: f32,
}

/// `config.js:HUB_WARMTH.fog`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubFog {
    pub color: [u32; 4],
    pub density: f32,
}

/// `config.js:HUB_WARMTH.lanternLight`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanternLight {
    pub color: u32,
    pub int: f32,
    pub dist: f32,
}

/// `config.js:HUB_WARMTH`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubWarmth {
    pub ambient: [u32; 4],
    pub fog: HubFog,
    pub lantern_glow: f32,
    pub lantern_light: LanternLight,
}

/// `config.js:EMBERS`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embers {
    pub max: u32,
    pub per_tier: [u32; 4],
    pub rise: [f32; 2],
    pub life: [f32; 2],
    pub drift: f32,
    pub size: f32,
}

/// `config.js:HUB_BLOCK` — `map.blockMask` bits and the collider thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubBlock {
    pub prop: u8,
    pub building: u8,
    pub npc: u8,
    pub flame: u8,
    pub min_top: f32,
    pub max_bottom: f32,
    pub shrink: f32,
    pub min_size: f32,
}

/// `npc.js:NPC_CFG` — follower tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcCfg {
    pub tick: f32,
    pub speed: f32,
    pub fast_speed: f32,
    pub fast_dist: f32,
    pub stop_dist: f32,
    pub teleport_dist: f32,
    pub save_r: f32,
    pub lit_r: f32,
    pub catch_r: f32,
    pub caught_t: f32,
    pub hunter_busy_t: f32,
    pub hub_look_r: f32,
    pub interact_r: f32,
    pub radius: f32,
}

/// `contracts.js:CONTRACT_CFG`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractCfg {
    pub max_active: u32,
    pub spot_r: f32,
    pub plant_r: f32,
    pub quest_y: f32,
}

/// `hub.js:HUB_CFG`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubCfg {
    pub interact_r: f32,
    pub ghost_color: u32,
    pub minimap_px: u32,
    pub explore_tick: f32,
    pub explore_max_r: f32,
    pub explore_dark_r: f32,
    pub minimap_hz: f32,
    pub explored_save_t: f32,
    pub pad_glow: f32,
}

/// `endgame.js:ENDGAME` — Source laps, altar and ending numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndgameCfg {
    pub altar_r: f32,
    pub ride_confirm_t: f32,
    pub dawn_rescued: u32,
    pub dawn_tier: u32,
    pub fog_per_lap: f32,
    pub ambient_per_lap: f32,
    pub tint_lerp: f32,
    pub wake_lap_ahead: i32,
    pub extra_laps: Vec<i32>,
    pub max_hunters: u32,
    pub spawn_min_cells: i32,
    pub spawn_max_cells: i32,
    pub cue_gain: f32,
    pub cue_dur: f32,
    pub night_int: f32,
    pub night_dist: f32,
}
