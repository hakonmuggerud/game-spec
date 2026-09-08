//! LANE: economy. The light economy and the hub ledgers — `main.js` bank / pickup / death-bundle / oil burn,
//! `hub.js` flame tiers, buildings, services, blessing, `endgame.js` altar / endings / laps (DESIGN.md §4, §6,
//! §8, §9). The JS numbers are authoritative (`config.js` `CFG.startOil` is 45/50/55/60, not the 50–80 of the
//! older §4 table).
//!
//! Every function is pure over the [`Config`] / [`GameData`] tables plus a small piece of state — the
//! [`SaveData`] ledgers, the hub's [`HubState`] (`ctx.hub.flame.tier` + `hub.js` `run.blessed`), the player's
//! [`LampState`] and [`Carried`], the Source's [`SourceRun`] and the altar's [`EndingScreen`] — and returns
//! `Vec<SimEvent>` for what the JS `emit()`ted (toasts included, since `ui.js` queues `toast` events).

use crate::events::{BuildSpend, ServiceAction, SimEvent};
use crate::grid::{self, dist2d};
use crate::player::Carried;
use crate::rng::RandomSource;
use crate::save::{Endings, SaveData};
use undercroft_data::config::{Config, LightTech, Points, Tier};
use undercroft_data::tables::{EndingDef, ItemKind};
use undercroft_data::zone::{DeepStyle, ZoneDef, ZONE_ORDER};
use undercroft_data::{CellKind, GameData, ParsedMap};

/* ============================================================
Shared state
============================================================ */

/// `hub.js` module state that outlives a run: the applied flame tier (`ctx.hub.flame.tier`, re-derived from
/// `save.points` by [`check_tier`]) and whether the current run bought the blessing (`run.blessed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubState {
    pub tier: u32,
    pub blessed: bool,
}

impl Default for HubState {
    fn default() -> Self {
        HubState {
            tier: 1,
            blessed: false,
        }
    }
}

impl HubState {
    /// `hub.js:init` — the tier from the save's points, nothing blessed.
    pub fn from_save(cfg: &Config, save: &SaveData) -> HubState {
        HubState {
            tier: tier_for(&cfg.tiers, save.points),
            blessed: false,
        }
    }
}

/// The handlamp fields of `ctx.player` that `main.js:updateLamp` / `flash` / `plantLantern` / `topUp` touch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LampState {
    pub oil: f32,
    pub lamp_on: bool,
    /// Flash cooldown left (s).
    pub flash_cd: f32,
    /// Lantern cooldown left (s).
    pub lantern_cd: f32,
    /// Flash burst left (s).
    pub flash_t: f32,
    /// Lampwight relight lockout left (s).
    pub lamp_lock: f32,
}

impl Default for LampState {
    fn default() -> Self {
        LampState {
            oil: 0.0,
            lamp_on: false,
            flash_cd: 0.0,
            lantern_cd: 0.0,
            flash_t: 0.0,
            lamp_lock: 0.0,
        }
    }
}

/// `main.js:zoneMul()` — burn-rate and lamp-distance multipliers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneMul {
    pub burn: f32,
    pub dist: f32,
}

impl Default for ZoneMul {
    fn default() -> Self {
        ZoneMul {
            burn: 1.0,
            dist: 1.0,
        }
    }
}

/* ============================================================
Carried loot
============================================================ */

/// `main.js:describe(c)` — "2 flasks, 1 relic" / "nothing".
pub fn describe(c: &Carried) -> String {
    let mut parts = Vec::new();
    let plural = |n: u32| if n == 1 { "" } else { "s" };
    if c.oil > 0 {
        parts.push(format!("{} flask{}", c.oil, plural(c.oil)));
    }
    if c.relic > 0 {
        parts.push(format!("{} relic{}", c.relic, plural(c.relic)));
    }
    if c.rich > 0 {
        parts.push(format!("{} rich relic{}", c.rich, plural(c.rich)));
    }
    if c.quest > 0 {
        parts.push(format!("{} quest item{}", c.quest, plural(c.quest)));
    }
    if parts.is_empty() {
        "nothing".to_string()
    } else {
        parts.join(", ")
    }
}

/// `main.js:pickup(it)` — add the item (or a bundle's contents) to the carried loot. Returns the `pickup` event
/// (with the bundle's toast first, as the JS shows it). Removing the world item is the shell's job.
pub fn pickup(
    carried: &mut Carried,
    kind: ItemKind,
    contents: Option<Carried>,
    x: f32,
    z: f32,
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    match kind {
        ItemKind::Bundle => {
            let c = contents.unwrap_or_default();
            carried.oil += c.oil;
            carried.relic += c.relic;
            carried.rich += c.rich;
            carried.quest += c.quest;
            out.push(SimEvent::toast(format!(
                "Recovered your bundle: {}",
                describe(&c)
            )));
        }
        ItemKind::Oil => carried.oil += 1,
        ItemKind::Relic => carried.relic += 1,
        ItemKind::Rich => carried.rich += 1,
        ItemKind::Quest => carried.quest += 1,
    }
    out.push(SimEvent::Pickup {
        kind,
        x,
        z,
        contents: if kind == ItemKind::Bundle {
            Some(contents.unwrap_or_default())
        } else {
            None
        },
    });
    out
}

/// `main.js:bank` — `points += oil·1 + relic·3 + rich·5` (`POINTS`; quest items are worth 0).
pub fn bank_points(points: &Points, c: &Carried) -> u32 {
    c.oil * points.oil + c.relic * points.relic + c.rich * points.rich + c.quest * points.quest
}

/// `main.js:bank` (after the fade) plus `hub.js`'s `bank` listener: flame points and `stats.banked` go up,
/// the resource ledger is credited (`oil += 25·flasks`, `relics += relic`, `rich += rich`), the loot is cleared.
/// Emits `bank {carried, pts, zoneId}`, the toast and `zoneExit`; the shell then calls [`enter_hub`].
/// DESIGN.md §4: a flask is +1 point and +25 banked oil here, or +25 lamp oil via [`top_up`] — never both.
pub fn bank(
    cfg: &Config,
    save: &mut SaveData,
    carried: &mut Carried,
    zone_id: &str,
) -> Vec<SimEvent> {
    let c = *carried;
    let pts = bank_points(&cfg.points, &c);
    let msg = if pts > 0 {
        format!("Banked: {} (+{pts})", describe(&c))
    } else {
        "Nothing to bank. You return to the Lantern.".to_string()
    };
    save.points += pts;
    save.stats.banked += pts;
    save.oil += c.oil * cfg.cfg.flask_oil as u32;
    save.relics += c.relic;
    save.rich += c.rich;
    *carried = Carried::default();
    vec![
        SimEvent::Bank {
            carried: c,
            pts,
            zone_id: zone_id.to_string(),
        },
        SimEvent::toast(msg),
        SimEvent::ZoneExit {
            zone_id: zone_id.to_string(),
        },
    ]
}

/// What `main.js:die` leaves behind.
#[derive(Debug, Clone, PartialEq)]
pub struct DeathOutcome {
    /// The bundle to drop at the death spot (one per zone: the shell removes any older one), if anything was lost.
    pub bundle: Option<Carried>,
    /// `state.lostLoot` — `describe()` of what dropped.
    pub lost: String,
    pub events: Vec<SimEvent>,
}

/// `main.js:die(h)` — the economy half: a blessed run banks ⌊half⌋ of each carried kind first
/// ([`apply_blessing`]), the rest becomes a bundle, the loot is cleared, `stats.deaths` goes up and `death`
/// is emitted. The death camera and the DYING mode are the shell's.
#[allow(clippy::too_many_arguments)]
pub fn die(
    cfg: &Config,
    save: &mut SaveData,
    hub: &mut HubState,
    carried: &mut Carried,
    zone_id: &str,
    x: f32,
    z: f32,
    hunter_id: Option<u32>,
) -> DeathOutcome {
    let mut events = Vec::new();
    let mut c = *carried;
    if c.total() > 0 {
        let (rest, ev) = apply_blessing(cfg, save, hub, c, zone_id, x, z);
        c = rest;
        events.extend(ev);
    }
    let total = c.total();
    let lost = describe(&c);
    *carried = Carried::default();
    save.stats.deaths += 1;
    events.push(SimEvent::Death { x, z, hunter_id });
    DeathOutcome {
        bundle: if total > 0 { Some(c) } else { None },
        lost,
        events,
    }
}

/* ============================================================
The handlamp (main.js updateLamp / toggleLamp / topUp / flash / plantLantern)
============================================================ */

/// `hub.js:lightTech()` — the `LIGHT_TECH` row for `save.lightTech` (clamped).
pub fn light_tech<'a>(cfg: &'a Config, save: &SaveData) -> &'a LightTech {
    let i = (save.light_tech as usize).min(cfg.light_tech.len().saturating_sub(1));
    &cfg.light_tech[i]
}

/// `main.js:zoneMul()` — deep (flat ×1.5 burn / ×0.6 lamp, or the Source's per-lap bands) × zone (`burnMul`,
/// `lampMul`) × light-tech multipliers. `zone` is `None` in the hub (all ×1 apart from light-tech).
pub fn zone_mul(
    cfg: &Config,
    zone: Option<&ZoneDef>,
    tech: &LightTech,
    on_deep: bool,
    lap: i32,
) -> ZoneMul {
    let bands = zone
        .map(|z| z.deep_style == DeepStyle::Bands)
        .unwrap_or(false);
    let c = &cfg.cfg;
    let deep_burn = if on_deep {
        if bands {
            c.band_burn.at(lap)
        } else {
            c.deep_burn_mul
        }
    } else {
        1.0
    };
    let deep_lamp = if on_deep {
        if bands {
            c.band_lamp.at(lap)
        } else {
            c.deep_lamp_mul
        }
    } else {
        1.0
    };
    let zb = zone.map(|z| z.burn_mul).unwrap_or(1.0);
    let zl = zone.map(|z| z.lamp_mul).unwrap_or(1.0);
    ZoneMul {
        burn: deep_burn * zb * tech.burn_mul,
        dist: deep_lamp * zl * tech.dist_mul,
    }
}

/// `main.js:updateLamp(dt)` — burn `CFG.burn × mul.burn` per second while lit (oil 0 → lamp forced off), tick
/// the cooldowns, the flash burst and the lockout. `lamp_allowed` is false outside ZONE / DYING / MENU / ENDING,
/// where the lamp is forced off. Returns the lamp reach `CFG.lampDist × mul.dist` (`lamp.distance`).
pub fn update_lamp(
    lamp: &mut LampState,
    cfg: &Config,
    dt: f32,
    mul: ZoneMul,
    lamp_allowed: bool,
) -> f32 {
    let c = &cfg.cfg;
    if !lamp_allowed {
        lamp.lamp_on = false;
    }
    if lamp.lamp_on {
        lamp.oil -= c.burn * mul.burn * dt;
        if lamp.oil <= 0.0 {
            lamp.oil = 0.0;
            lamp.lamp_on = false;
        }
    }
    if !lamp.oil.is_finite() {
        lamp.oil = 0.0;
    }
    lamp.flash_cd = (lamp.flash_cd - dt).max(0.0);
    lamp.lantern_cd = (lamp.lantern_cd - dt).max(0.0);
    lamp.flash_t = (lamp.flash_t - dt).max(0.0);
    lamp.lamp_lock = (lamp.lamp_lock - dt).max(0.0);
    c.lamp_dist * mul.dist
}

/// `main.js:updateLamp` — the PointLight intensity at `time`: `lampInt × flicker` (0.93 + 0.07·sin 13t, ×3
/// amplitude below 15 oil), `lampInt × flashMul` mid-flash, 0 when off.
pub fn lamp_intensity(lamp: &LampState, cfg: &Config, time: f32) -> f32 {
    let c = &cfg.cfg;
    let amp = if lamp.oil < 15.0 { 0.07 * 3.0 } else { 0.07 };
    let flick = 0.93 + amp * (time * 13.0).sin();
    let mut inten = if lamp.lamp_on {
        c.lamp_int * flick
    } else {
        0.0
    };
    if lamp.flash_t > 0.0 {
        inten = c.lamp_int * c.flash_mul;
    }
    inten
}

/// `main.js:toggleLamp()` — F. Refused outside a zone, dry, or while the wick is cold (Lampwight lockout).
pub fn toggle_lamp(lamp: &mut LampState, in_zone: bool) -> (bool, Vec<SimEvent>) {
    if !in_zone {
        return (false, vec![]);
    }
    if !lamp.lamp_on && lamp.oil <= 0.0 {
        return (false, vec![SimEvent::toast("The lamp is dry.")]);
    }
    if !lamp.lamp_on && lamp.lamp_lock > 0.0 {
        return (
            false,
            vec![SimEvent::toast("The wick is cold"), SimEvent::ui_error()],
        );
    }
    lamp.lamp_on = !lamp.lamp_on;
    (true, vec![SimEvent::LampToggle { on: lamp.lamp_on }])
}

/// `main.js` `events.on('lampSnuffed')` — the Lampwight's touch (DESIGN.md §5.1): in a zone the lamp goes out,
/// `oil` (default `CREATURE.lampwight.oil`) burns off and the wick stays cold for `lockout` seconds (default
/// `CREATURE.lampwight.lockout`, never shortening a longer lockout already running). Apply it to the
/// `SimEvent::LampSnuffed { oil, lockout }` payload the creature lane emits; ignored outside ZONE mode.
pub fn on_lamp_snuffed(
    lamp: &mut LampState,
    cfg: &Config,
    oil: Option<f32>,
    lockout: Option<f32>,
    in_zone: bool,
) {
    if !in_zone {
        return;
    }
    let lw = &cfg.creature.lampwight;
    lamp.lamp_on = false;
    lamp.oil = (lamp.oil - oil.unwrap_or(lw.oil)).max(0.0);
    lamp.lamp_lock = lamp.lamp_lock.max(lockout.unwrap_or(lw.lockout));
}

/// `main.js:topUp()` — T: one carried flask → `+flaskOil` lamp oil (capped at `oilMax`).
pub fn top_up(
    lamp: &mut LampState,
    cfg: &Config,
    carried: &mut Carried,
    in_zone: bool,
) -> (bool, Vec<SimEvent>) {
    let c = &cfg.cfg;
    if !in_zone || carried.oil == 0 || lamp.oil >= c.oil_max {
        return (false, vec![]);
    }
    carried.oil -= 1;
    lamp.oil = (lamp.oil + c.flask_oil).min(c.oil_max);
    (true, vec![SimEvent::TopUp { oil: lamp.oil }])
}

/// `main.js:flash()` — Q: pay the light-tech flash cost, start the cooldown and the burst, emit `flash`.
/// Which hunters the cone hits (`CFG.flashRange` / `flashDot` + LOS → `hunter.onFlash`) is the creature lane's.
pub fn flash(
    lamp: &mut LampState,
    cfg: &Config,
    tech: &LightTech,
    in_zone: bool,
    x: f32,
    z: f32,
) -> (bool, Vec<SimEvent>) {
    let cost = tech.flash_cost;
    if !in_zone || lamp.flash_cd > 0.0 || lamp.oil < cost {
        return (false, vec![]);
    }
    lamp.oil -= cost;
    lamp.flash_cd = cfg.cfg.flash_cd;
    lamp.flash_t = cfg.cfg.flash_dur;
    (true, vec![SimEvent::Flash { x, z }])
}

/// `main.js:plantLantern()` — R: pay the light-tech lantern cost, start the cooldown; with `lanternMax`
/// alive the oldest (`oldest`, its position) is recycled first (`lanternRemoved`), then `lantern {x, z}`.
/// Spawning / removing the world lantern is the shell's job.
#[allow(clippy::too_many_arguments)]
pub fn plant_lantern(
    lamp: &mut LampState,
    cfg: &Config,
    tech: &LightTech,
    in_zone: bool,
    lantern_count: u32,
    oldest: Option<(f32, f32)>,
    x: f32,
    z: f32,
) -> (bool, Vec<SimEvent>) {
    let cost = tech.lantern_cost;
    if !in_zone || lamp.lantern_cd > 0.0 || lamp.oil < cost {
        return (false, vec![]);
    }
    lamp.oil -= cost;
    lamp.lantern_cd = cfg.cfg.lantern_cd;
    let mut out = Vec::new();
    if lantern_count >= cfg.cfg.lantern_max {
        if let Some((ox, oz)) = oldest {
            out.push(SimEvent::LanternRemoved { x: ox, z: oz });
        }
    }
    out.push(SimEvent::Lantern { x, z });
    (true, out)
}

/* ============================================================
Hub / zone transitions (main.js enterHub / startRun / descend)
============================================================ */

/// `hub.js:startOil()` — `CFG.startOil[tier − 1] + reservoirOil × reservoir` (45/50/55/60 + 15 per level).
pub fn start_oil(cfg: &Config, tier: u32, reservoir: u32) -> f32 {
    let i = (tier.max(1) as usize - 1).min(cfg.cfg.start_oil.len() - 1);
    cfg.cfg.start_oil[i] + cfg.build_costs.reservoir_oil as f32 * reservoir as f32
}

/// `main.js:enterHub()` — lamp off, cooldowns cleared, the HUD oil set to next run's start oil; emits
/// `hubEnter` then whatever `hub.js`'s `checkTier(true)` listener emits.
pub fn enter_hub(
    cfg: &Config,
    save: &SaveData,
    hub: &mut HubState,
    lamp: &mut LampState,
) -> Vec<SimEvent> {
    lamp.lamp_on = false;
    let mut out = vec![SimEvent::HubEnter];
    out.extend(check_tier(cfg, save, hub, true));
    lamp.oil = start_oil(cfg, hub.tier, save.reservoir);
    lamp.flash_cd = 0.0;
    lamp.lantern_cd = 0.0;
    lamp.flash_t = 0.0;
    out
}

/// `main.js:startRun()` — the lamp starts full of [`start_oil`] and lit, `stats.runs` goes up, `zoneEnter`.
pub fn start_run(
    cfg: &Config,
    save: &mut SaveData,
    hub: &HubState,
    lamp: &mut LampState,
    zone_id: &str,
) -> Vec<SimEvent> {
    lamp.oil = start_oil(cfg, hub.tier, save.reservoir);
    lamp.lamp_on = true;
    lamp.flash_cd = 0.0;
    lamp.lantern_cd = 0.0;
    lamp.flash_t = 0.0;
    save.stats.runs += 1;
    vec![SimEvent::ZoneEnter {
        zone_id: zone_id.to_string(),
    }]
}

/// `main.js:descend()` — the zone the stairs / tram / elevator go to (`save.zoneSelected`), refused with the
/// lock reason unless that zone is already loaded (`loaded_zone`). `Ok(zone_id)` means "fade, then start the run".
pub fn descend(
    data: &GameData,
    save: &SaveData,
    hub: &HubState,
    loaded_zone: Option<&str>,
) -> Result<String, Vec<SimEvent>> {
    let id = selected(data, save).to_string();
    if loaded_zone == Some(id.as_str()) {
        return Ok(id);
    }
    match zone_locked(data, save, hub.tier, &id) {
        Some(lock) => Err(vec![
            SimEvent::toast(format!("{}: {lock}", zone_name(data, &id))),
            SimEvent::ui_error(),
        ]),
        None => Ok(id),
    }
}

/// `main.js:actions.giveTool(id)` — grant a tool; emits `toolGained` the first time.
pub fn give_tool(save: &mut SaveData, id: &str) -> (bool, Vec<SimEvent>) {
    if !crate::save::Tools::known(id) {
        return (false, vec![]);
    }
    if save.tools.get(id) {
        return (true, vec![]);
    }
    save.tools.set(id, true);
    (true, vec![SimEvent::ToolGained { id: id.to_string() }])
}

/// `main.js:actions.setPoints(n)` — set the flame points and re-tier the flame.
pub fn set_points(cfg: &Config, save: &mut SaveData, hub: &mut HubState, n: u32) -> Vec<SimEvent> {
    save.points = n;
    check_tier(cfg, save, hub, true)
}

/* ============================================================
Flame tiers (hub.js tierFor / checkTier)
============================================================ */

/// `hub.js:tierFor(points)` — 1-based tier: the last `TIERS[i].pts ≤ points` (6 / 15 / 30 → 2 / 3 / 4).
pub fn tier_for(tiers: &[Tier], points: u32) -> u32 {
    let mut t = 1;
    for (i, tier) in tiers.iter().enumerate() {
        if points >= tier.pts {
            t = i as u32 + 1;
        }
    }
    t
}

/// `hub.js:checkTier(announce)` — re-derive the tier from `save.points`; emit `flameTier {tier, prev, initial}`
/// on change (or always, as the `initial` announcement, when `announce` is false).
pub fn check_tier(
    cfg: &Config,
    save: &SaveData,
    hub: &mut HubState,
    announce: bool,
) -> Vec<SimEvent> {
    let t = tier_for(&cfg.tiers, save.points);
    if t != hub.tier || !announce {
        let prev = hub.tier;
        hub.tier = t;
        return vec![SimEvent::FlameTier {
            tier: t,
            prev,
            initial: !announce,
        }];
    }
    vec![]
}

/// `hub.js:init` — apply the save's tier and emit the initial `flameTier {tier, prev: 0, initial: true}`.
pub fn init_tier(cfg: &Config, save: &SaveData, hub: &mut HubState) -> Vec<SimEvent> {
    let t = tier_for(&cfg.tiers, save.points);
    hub.tier = t;
    vec![SimEvent::FlameTier {
        tier: t,
        prev: 0,
        initial: true,
    }]
}

/* ============================================================
Buildings (hub.js buildStatus / canBuild / build / costText)
============================================================ */

/// `hub.js:buildStatus(id)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildStatus {
    pub id: String,
    pub built: bool,
    pub unlocked: bool,
    pub affordable: bool,
    /// Why not (`"Already built"`, `"Needs Wick the Lamplighter rescued"`, `"Needs 20 more oil, 2 more relics"`).
    pub reason: Option<String>,
    pub cost: BuildSpend,
    pub have: BuildSpend,
}

/// `hub.js:NPC_NAME[id]` (falls back to the table's `name`).
fn npc_name(data: &GameData, id: &str) -> String {
    data.npcs
        .npcs
        .get(id)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| id.to_string())
}

/// `maps.js:ZONES[id].name` or the id.
pub fn zone_name<'a>(data: &'a GameData, id: &'a str) -> &'a str {
    data.zone(id).map(|z| z.name.as_str()).unwrap_or(id)
}

/// `hub.js:costOf(id)` — `BUILD_COSTS[id]` as `{oil, relics, rich}` (missing fields 0; the board is free).
pub fn cost_of(cfg: &Config, id: &str) -> BuildSpend {
    match cfg.build_costs.get(id) {
        Some(c) => BuildSpend {
            oil: c.oil.unwrap_or(0),
            relics: c.relics.unwrap_or(0),
            rich: 0,
        },
        None => BuildSpend::default(),
    }
}

/// `hub.js:costText(cost)` — "120 oil + 4 relics" / "free".
pub fn cost_text(cost: BuildSpend) -> String {
    let mut parts = Vec::new();
    if cost.oil > 0 {
        parts.push(format!("{} oil", cost.oil));
    }
    if cost.relics > 0 {
        parts.push(format!(
            "{} relic{}",
            cost.relics,
            if cost.relics == 1 { "" } else { "s" }
        ));
    }
    if cost.rich > 0 {
        parts.push(format!("{} rich", cost.rich));
    }
    if parts.is_empty() {
        "free".to_string()
    } else {
        parts.join(" + ")
    }
}

/// `hub.js:unlocked(id)` — may the ghost show? NPC buildings need their NPC rescued, tram/elevator the tier.
pub fn unlocked(data: &GameData, save: &SaveData, tier: u32, id: &str) -> bool {
    match data.buildings.buildings.get(id) {
        None => false,
        Some(b) if b.always => true,
        Some(b) => {
            if let Some(npc) = &b.npc {
                return save.rescued.get(npc);
            }
            if let Some(t) = b.tier {
                return tier >= t;
            }
            true
        }
    }
}

/// `hub.js:isBuilt(id)` — `always` buildings or `save.buildings[id]`.
pub fn is_built(data: &GameData, save: &SaveData, id: &str) -> bool {
    data.buildings
        .buildings
        .get(id)
        .map(|b| b.always || save.buildings.get(id))
        .unwrap_or(false)
}

/// `hub.js:buildStatus(id)` → `None` for an unknown building.
pub fn build_status(data: &GameData, save: &SaveData, tier: u32, id: &str) -> Option<BuildStatus> {
    let b = data.buildings.buildings.get(id)?;
    let cost = cost_of(&data.config, id);
    let have = BuildSpend {
        oil: save.oil,
        relics: save.relics,
        rich: save.rich,
    };
    let built = is_built(data, save, id);
    let unl = unlocked(data, save, tier, id);
    let reason = if built {
        Some("Already built".to_string())
    } else if !unl {
        Some(match (&b.npc, b.tier) {
            (Some(npc), _) => format!("Needs {} rescued", npc_name(data, npc)),
            (None, Some(t)) => format!("Needs the flame at tier {t}"),
            _ => "Needs the flame at tier 0".to_string(),
        })
    } else {
        let mut short = Vec::new();
        if cost.oil > have.oil {
            short.push(format!("{} more oil", cost.oil - have.oil));
        }
        if cost.relics > have.relics {
            short.push(format!("{} more relics", cost.relics - have.relics));
        }
        if cost.rich > have.rich {
            short.push(format!("{} more rich relics", cost.rich - have.rich));
        }
        if short.is_empty() {
            None
        } else {
            Some(format!("Needs {}", short.join(", ")))
        }
    };
    Some(BuildStatus {
        id: id.to_string(),
        built,
        unlocked: unl,
        affordable: reason.is_none(),
        reason,
        cost,
        have,
    })
}

/// `hub.js:canBuild(id)`.
pub fn can_build(data: &GameData, save: &SaveData, tier: u32, id: &str) -> bool {
    build_status(data, save, tier, id)
        .map(|s| !s.built && s.unlocked && s.affordable)
        .unwrap_or(false)
}

/// `hub.js:err(msg)` — a toast (when given) plus `uiError`.
fn ui_err(msg: Option<String>) -> Vec<SimEvent> {
    let mut out = Vec::new();
    if let Some(m) = msg {
        out.push(SimEvent::toast(m));
    }
    out.push(SimEvent::ui_error());
    out
}

/// `hub.js:build(id, {free})` — spend the cost (unless `free`), raise the building, emit `build {id, cost}`,
/// `uiClick`, the toast, and "X is open" toasts for zones the building unlocked. Refused (with `uiError`) when
/// built, locked or unaffordable.
pub fn build(
    data: &GameData,
    save: &mut SaveData,
    hub: &HubState,
    id: &str,
    free: bool,
) -> (bool, Vec<SimEvent>) {
    let b = match data.buildings.buildings.get(id) {
        Some(b) => b,
        None => return (false, vec![]),
    };
    let st = match build_status(data, save, hub.tier, id) {
        Some(s) => s,
        None => return (false, vec![]),
    };
    if st.built {
        return (false, vec![]);
    }
    if !free && (!st.unlocked || !st.affordable) {
        return (false, ui_err(st.reason));
    }
    if !free {
        save.oil -= st.cost.oil;
        save.relics -= st.cost.relics;
        save.rich -= st.cost.rich;
    }
    save.buildings.set(id, true);
    let mut out = vec![
        SimEvent::Build {
            id: id.to_string(),
            cost: if free { BuildSpend::default() } else { st.cost },
        },
        SimEvent::UiClick,
        SimEvent::toast(format!("{} built.", b.name)),
    ];
    for z in ZONE_ORDER {
        let key = match z {
            "cistern" => "tram",
            "ossuary" | "source" => "elevator",
            _ => "",
        };
        if key == id && zone_locked(data, save, hub.tier, z).is_none() {
            out.push(SimEvent::toast(format!(
                "{} is open. Choose it at the Departure Board.",
                zone_name(data, z)
            )));
        }
    }
    (true, out)
}

/* ============================================================
Services (hub.js upgradeLightTech / pressRelics / deepenReservoir / toggleBlessing / applyBlessing)
============================================================ */

const ROMAN: [&str; 5] = ["none", "I", "II", "III", "IV"];

/// `hub.js` `events.on('npcRescued')` — the guidance toast when a rescued NPC's building is still unbuilt:
/// `"<NPC> could raise a <Building> at the Lantern."` (the first `BUILD_ORDER` entry whose `npc` is `id`).
/// The `refreshBuildings()` it also runs is presentation (the ghost appears) and never toasts for NPC buildings.
pub fn on_npc_rescued_hub(data: &GameData, save: &SaveData, id: &str) -> Vec<SimEvent> {
    let b = data
        .buildings
        .order
        .iter()
        .filter_map(|k| data.buildings.buildings.get(k))
        .find(|b| b.npc.as_deref() == Some(id));
    match b {
        Some(b) if !save.buildings.get(&b.id) => vec![SimEvent::toast(format!(
            "{} could raise a {} at the Lantern.",
            npc_name(data, id),
            b.name
        ))],
        _ => vec![],
    }
}

/// `hub.js:refreshBuildings(quiet = false)` on a non-initial `flameTier {tier, prev}`: every tier-gated building
/// (`BUILDINGS[id].tier`) whose ghost first appears with this tier — `prev < tier_needed ≤ tier` and not built —
/// toasts `"The flame is strong enough for a <name> (<cost>)."`, in `BUILD_ORDER`. Call it only when the event's
/// `initial` is false (`hubEnter` and the init announcement refresh quietly).
pub fn on_flame_tier_hub(data: &GameData, save: &SaveData, prev: u32, tier: u32) -> Vec<SimEvent> {
    let mut out = Vec::new();
    for id in &data.buildings.order {
        let Some(b) = data.buildings.buildings.get(id) else {
            continue;
        };
        let Some(t) = b.tier else {
            continue;
        };
        if prev < t && t <= tier && !is_built(data, save, id) {
            out.push(SimEvent::toast(format!(
                "The flame is strong enough for a {} ({}).",
                b.name,
                cost_text(cost_of(&data.config, id))
            )));
        }
    }
    out
}

/// `hub.js:upgradeLightTech()` — Workshop: buy the next `LIGHT_TECH` row for relics (+ rich at III).
pub fn upgrade_light_tech(
    data: &GameData,
    save: &mut SaveData,
    hub: &HubState,
) -> (bool, Vec<SimEvent>) {
    let cfg = &data.config;
    let cur = save.light_tech;
    let next = cur + 1;
    if next as usize >= cfg.light_tech.len() {
        return (false, ui_err(Some("Nothing more to learn here.".into())));
    }
    let row = &cfg.light_tech[next as usize];
    let mut need = Vec::new();
    if row.cost.relics > save.relics {
        need.push(format!("{} more relics", row.cost.relics - save.relics));
    }
    if row.cost.rich > save.rich {
        need.push(format!("{} more rich relics", row.cost.rich - save.rich));
    }
    if !need.is_empty() {
        return (false, ui_err(Some(format!("Needs {}", need.join(", ")))));
    }
    save.relics -= row.cost.relics;
    save.rich -= row.cost.rich;
    save.light_tech = next;
    let mut out = vec![
        SimEvent::LightTech {
            tier: next,
            prev: cur,
        },
        SimEvent::Service(ServiceAction::LightTech { tier: next }),
        SimEvent::UiClick,
        SimEvent::toast(format!(
            "Light-tech {}: reach ×{}, burn ×{}.",
            ROMAN.get(next as usize).copied().unwrap_or("?"),
            row.dist_mul,
            row.burn_mul
        )),
    ];
    for z in ZONE_ORDER {
        let wants = data
            .zone(z)
            .and_then(|zd| zd.requires.as_ref())
            .and_then(|r| r.light_tech);
        if wants == Some(next) && zone_locked(data, save, hub.tier, z).is_none() {
            out.push(SimEvent::toast(format!(
                "{} is open. Choose it at the Departure Board.",
                zone_name(data, z)
            )));
        }
    }
    (true, out)
}

/// `hub.js:pressRelics(n)` — Oil Press: `min(n, relics)` relics → `pressRelicOil` (30) oil each.
pub fn press_relics(cfg: &Config, save: &mut SaveData, n: u32) -> (bool, Vec<SimEvent>) {
    let n = n.min(save.relics);
    if n == 0 {
        return (false, ui_err(Some("No relics to press.".into())));
    }
    let oil = n * cfg.build_costs.press_relic_oil;
    save.relics -= n;
    save.oil += oil;
    (
        true,
        vec![
            SimEvent::Service(ServiceAction::Press { n, oil }),
            SimEvent::UiClick,
            SimEvent::toast(format!(
                "Pressed {n} relic{} into {oil} oil.",
                if n == 1 { "" } else { "s" }
            )),
        ],
    )
}

/// `hub.js:deepenReservoir()` — Oil Press: level `reservoir` costs `BUILD_COSTS.reservoir[level]` relics (5,
/// then 8) and adds `reservoirOil` (15) to every run's start oil. Returns the new start oil too, which the HUD
/// lamp takes when the player stands in the hub (`ctx.player.oil = startOil()`).
pub fn deepen_reservoir(
    cfg: &Config,
    save: &mut SaveData,
    hub: &HubState,
) -> (Option<f32>, Vec<SimEvent>) {
    let lvl = save.reservoir as usize;
    let costs = &cfg.build_costs.reservoir;
    if lvl >= costs.len() {
        return (
            None,
            ui_err(Some("The reservoir is as deep as it goes.".into())),
        );
    }
    let cost = costs[lvl];
    if save.relics < cost {
        return (
            None,
            ui_err(Some(format!("Needs {} more relics", cost - save.relics))),
        );
    }
    save.relics -= cost;
    save.reservoir = lvl as u32 + 1;
    let so = start_oil(cfg, hub.tier, save.reservoir);
    (
        Some(so),
        vec![
            SimEvent::Service(ServiceAction::Reservoir {
                level: save.reservoir,
                start_oil: so,
            }),
            SimEvent::UiClick,
            SimEvent::toast(format!(
                "Deeper reservoir: the lamp now starts with {so} oil."
            )),
        ],
    )
}

/// `hub.js:toggleBlessing()` — Shrine: flip `save.blessing`.
pub fn toggle_blessing(cfg: &Config, save: &mut SaveData) -> Vec<SimEvent> {
    save.blessing = !save.blessing;
    vec![
        SimEvent::Blessing { on: save.blessing },
        SimEvent::UiClick,
        SimEvent::toast(if save.blessing {
            format!(
                "The blessing is lit — {} oil at each descent.",
                cfg.build_costs.blessing_oil
            )
        } else {
            "The blessing is snuffed.".to_string()
        }),
    ]
}

/// `hub.js:onZoneEnter` — charge the blessing at each descent: with the Shrine built and the blessing lit,
/// `blessingOil` (40) banked oil buys a blessed run (or the candles stay dark). Emits `service {shrine, charge}`.
pub fn charge_blessing(cfg: &Config, save: &mut SaveData, hub: &mut HubState) -> Vec<SimEvent> {
    hub.blessed = false;
    if !(save.blessing && save.buildings.shrine) {
        return vec![];
    }
    let cost = cfg.build_costs.blessing_oil;
    let mut out = Vec::new();
    if save.oil >= cost {
        save.oil -= cost;
        hub.blessed = true;
        out.push(SimEvent::toast(format!(
            "The blessing holds (−{cost} oil)."
        )));
    } else {
        out.push(SimEvent::toast(
            "No oil for the blessing; the shrine candles stay dark.",
        ));
    }
    out.push(SimEvent::Service(ServiceAction::Charge {
        lit: hub.blessed,
    }));
    out
}

/// `hub.js:applyBlessing(carried)` — in a blessed run ⌊half⌋ of each carried kind (oil / relic / rich, never
/// quest) is banked on the spot (both ledgers) and only the rest drops; returns what goes into the bundle.
/// Unblessed: the loot comes back unchanged.
pub fn apply_blessing(
    cfg: &Config,
    save: &mut SaveData,
    hub: &mut HubState,
    carried: Carried,
    zone_id: &str,
    x: f32,
    z: f32,
) -> (Carried, Vec<SimEvent>) {
    if !hub.blessed {
        return (carried, vec![]);
    }
    hub.blessed = false;
    let mut c = carried;
    let mut kept = Carried::default();
    let mut pts = 0;
    let mut any = false;
    let p = &cfg.points;
    for (n, k, w) in [
        (&mut c.oil, &mut kept.oil, p.oil),
        (&mut c.relic, &mut kept.relic, p.relic),
        (&mut c.rich, &mut kept.rich, p.rich),
    ] {
        let keep = *n / 2;
        if keep == 0 {
            continue;
        }
        *n -= keep;
        *k = keep;
        any = true;
        pts += keep * w;
    }
    if !any {
        return (c, vec![]);
    }
    save.oil += kept.oil * cfg.cfg.flask_oil as u32;
    save.relics += kept.relic;
    save.rich += kept.rich;
    save.points += pts;
    save.stats.banked += pts;
    (
        c,
        vec![
            SimEvent::BlessingKept {
                kept,
                pts,
                zone_id: zone_id.to_string(),
                x,
                z,
            },
            SimEvent::toast(format!("The blessing kept {} (+{pts}).", describe(&kept))),
        ],
    )
}

/* ============================================================
Departure board (hub.js selected / select, maps.js zoneLocked)
============================================================ */

/// `hub.js:selected()` — `save.zoneSelected` when it names a zone, else `undercroft`.
pub fn selected<'a>(data: &GameData, save: &'a SaveData) -> &'a str {
    if data.zone(&save.zone_selected).is_some() {
        &save.zone_selected
    } else {
        "undercroft"
    }
}

/// `maps.js:zoneLocked(id, save, tier)` over a [`SaveData`].
pub fn zone_locked(data: &GameData, save: &SaveData, tier: u32, id: &str) -> Option<String> {
    data.zone_locked(
        id,
        &|b| save.buildings.get(b),
        save.light_tech,
        tier,
        &|n| save.rescued.get(n),
    )
}

/// `hub.js:select(zoneId, {check})` — record the next descent; with `check` a locked zone is refused.
pub fn select_zone(
    data: &GameData,
    save: &mut SaveData,
    hub: &HubState,
    id: &str,
    check: bool,
) -> (bool, Vec<SimEvent>) {
    if data.zone(id).is_none() {
        return (false, vec![]);
    }
    if check {
        if let Some(lock) = zone_locked(data, save, hub.tier, id) {
            return (
                false,
                ui_err(Some(format!("{}: {lock}", zone_name(data, id)))),
            );
        }
    }
    let changed = save.zone_selected != id;
    save.zone_selected = id.to_string();
    let mut out = vec![SimEvent::ZoneSelected {
        zone_id: id.to_string(),
    }];
    if check && changed {
        out.push(SimEvent::UiClick);
        out.push(SimEvent::toast(format!(
            "Next descent: {}.",
            zone_name(data, id)
        )));
    }
    (true, out)
}

/// `hub.js:updateHud` — the hub's banked-resources block (three lines).
pub fn hub_hud_text(data: &GameData, save: &SaveData) -> String {
    let cfg = &data.config;
    let mut parts = vec![format!(
        "Banked: {} oil · {} relic{} · {} rich",
        save.oil,
        save.relics,
        if save.relics == 1 { "" } else { "s" },
        save.rich
    )];
    let mut l2 = format!(
        "Light-tech {}",
        ROMAN.get(save.light_tech as usize).copied().unwrap_or("?")
    );
    if save.reservoir > 0 {
        l2.push_str(&format!(
            " · reservoir +{}",
            save.reservoir * cfg.build_costs.reservoir_oil
        ));
    }
    if save.buildings.shrine {
        l2.push_str(&format!(
            " · blessing {}",
            if save.blessing { "lit" } else { "unlit" }
        ));
    }
    parts.push(l2);
    parts.push(format!(
        "Next descent: {}",
        zone_name(data, selected(data, save))
    ));
    parts.join("\n")
}

/* ============================================================
Hub interaction (hub.js interactTarget)
============================================================ */

/// What E does in the hub (`hub.js:interactTarget`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubInteract {
    /// A ghost: `[E] Build <name> — <cost>` (`openBuilding`).
    Build { id: String, label: String },
    /// A built building: `[E] <name>` (`openBuilding`).
    Building { id: String, label: String },
    /// The hub stairs: `[E] Descend — <zone>` plus ` (locked: <reason>)` when the selected zone is locked
    /// (`main.js` runs `descend()`).
    Descend {
        zone_id: String,
        label: String,
        lock: Option<String>,
    },
}

/// `hub.js:interactTarget(ctx)` — the nearest visible (ghost or built) building whose anchor centre is strictly
/// within `HUB_CFG.interactR` (1.8) of the player, walking `BUILD_ORDER` so the first wins ties; otherwise the hub
/// stairs within `CFG.interactR` (1.6, inclusive) → `Descend` for the selected zone. `m` is the parsed hub map
/// (anchor digits with `HUB_OX` applied); a building whose anchor digit is missing from the map is skipped.
pub fn hub_interact_target(
    data: &GameData,
    save: &SaveData,
    hub: &HubState,
    m: &ParsedMap,
    px: f32,
    pz: f32,
) -> Option<HubInteract> {
    let mut best: Option<&str> = None;
    let mut bd = data.config.hub_cfg.interact_r as f64;
    for id in &data.buildings.order {
        let Some(b) = data.buildings.buildings.get(id) else {
            continue;
        };
        if !is_built(data, save, id) && !unlocked(data, save, hub.tier, id) {
            continue; // state 'none': no mesh to look at
        }
        let Some(a) = b.anchor.parse::<u8>().ok().and_then(|d| m.anchors.get(&d)) else {
            continue;
        };
        let d = grid::dist2d_f64(a.x, a.z, px, pz);
        if d < bd {
            best = Some(id);
            bd = d;
        }
    }
    if let Some(id) = best {
        let b = &data.buildings.buildings[id];
        return Some(if is_built(data, save, id) {
            HubInteract::Building {
                id: id.to_string(),
                label: format!("[E] {}", b.name),
            }
        } else {
            HubInteract::Build {
                id: id.to_string(),
                label: format!(
                    "[E] Build {} — {}",
                    b.name,
                    cost_text(cost_of(&data.config, id))
                ),
            }
        });
    }
    let st = m.stairs?;
    if grid::dist2d_f64(st.marker.x, st.marker.z, px, pz) > data.config.cfg.interact_r as f64 {
        return None;
    }
    let zid = selected(data, save);
    let lock = zone_locked(data, save, hub.tier, zid);
    let label = format!(
        "[E] Descend — {}{}",
        zone_name(data, zid),
        lock.as_deref()
            .map(|l| format!(" (locked: {l})"))
            .unwrap_or_default()
    );
    Some(HubInteract::Descend {
        zone_id: zid.to_string(),
        label,
        lock,
    })
}

/* ============================================================
Exploration (hub.js exploreTick / exploredPct — the Cartographer's Table)
============================================================ */

/// `hub.js:exploreTick()` — every `HUB_CFG.exploreTick` seconds in an unpaused zone: mark every non-solid cell
/// within `r` of the player that the player has line of sight to (the player's own cell needs none), plus every
/// solid N8 neighbour of a marked cell. `r` is `min(exploreMaxR, lamp reach)` with the lamp on, `exploreDarkR`
/// otherwise; `lamp_reach` is the handlamp light's `distance`. `bits` is the zone's explored bitset
/// (`SaveData::explored_bits`). Returns true when a new bit was set (`exploredDirty`).
pub fn explore_tick(
    cfg: &Config,
    m: &ParsedMap,
    bits: &mut [u8],
    px: f32,
    pz: f32,
    lamp_on: bool,
    lamp_reach: f32,
) -> bool {
    let hc = &cfg.hub_cfg;
    let r = if lamp_on {
        lamp_reach.min(hc.explore_max_r)
    } else {
        hc.explore_dark_r
    };
    let (pcx, pcz) = grid::to_cell(m, px, pz);
    let rc = r.ceil() as i32;
    let mut dirty = false;
    let mut mark = |cx: i32, cz: i32| {
        if grid::in_bounds(m, cx, cz) && crate::save::mark_bit(bits, grid::idx(m, cx, cz)) {
            dirty = true;
        }
    };
    for cz in pcz - rc..=pcz + rc {
        for cx in pcx - rc..=pcx + rc {
            if !grid::in_bounds(m, cx, cz) || grid::is_solid(m, cx, cz) {
                continue;
            }
            let (x, z) = grid::center(m, cx, cz);
            if grid::dist2d_f64(x, z, px, pz) > r as f64 {
                continue;
            }
            if !(cx == pcx && cz == pcz) && !grid::los(m, px, pz, x, z) {
                continue;
            }
            mark(cx, cz);
            for (dx, dz) in N8 {
                if grid::is_solid(m, cx + dx, cz + dz) {
                    mark(cx + dx, cz + dz);
                }
            }
        }
    }
    dirty
}

/// `hub.js` `N8` — the eight neighbours, in its order.
const N8: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// `hub.js:exploredPct(zoneId)` — the percentage of non-wall cells seen, rounded (pillars, gates and doors count
/// as cells to see, as in the JS `!== T.WALL`). 0 for a map with no such cells.
pub fn explored_pct(m: &ParsedMap, bits: &[u8]) -> u32 {
    let mut walkable = 0u32;
    let mut seen = 0u32;
    for (i, &c) in m.cells.iter().enumerate() {
        if c == CellKind::Wall {
            continue;
        }
        walkable += 1;
        if crate::save::bit_set(bits, i) {
            seen += 1;
        }
    }
    if walkable == 0 {
        return 0;
    }
    (100.0 * seen as f64 / walkable as f64).round() as u32
}

/* ============================================================
Endgame (endgame.js) — endings
============================================================ */

/// `endgame.js:info()` — what the choice / end screens are computed from.
#[derive(Debug, Clone, PartialEq)]
pub struct EndgameInfo {
    pub tier: u32,
    pub rescued: u32,
    /// Rescued NPC names in table order.
    pub names: Vec<String>,
    pub total: u32,
    pub runs: u32,
    pub deaths: u32,
    pub points: u32,
    pub seen: Endings,
}

/// `endgame.js:info()`.
pub fn endgame_info(data: &GameData, save: &SaveData, tier: u32) -> EndgameInfo {
    let names: Vec<String> = save
        .rescued
        .ids()
        .iter()
        .map(|id| npc_name(data, id))
        .collect();
    EndgameInfo {
        tier,
        rescued: names.len() as u32,
        names,
        total: crate::save::Rescued::IDS.len() as u32,
        runs: save.stats.runs,
        deaths: save.stats.deaths,
        points: save.points,
        seen: save.endings,
    }
}

/// `endgame.js:ENDINGS[id].available(info)` — `Ok` or the reason (`"Needs flame tier 4 (now 2) and 3 rescued
/// (now 1)"`).
pub fn ending_available(def: &EndingDef, info: &EndgameInfo) -> Result<(), String> {
    let req = match def.requires {
        Some(r) => r,
        None => return Ok(()),
    };
    let mut need = Vec::new();
    if info.tier < req.tier {
        need.push(format!("flame tier {} (now {})", req.tier, info.tier));
    }
    if info.rescued < req.rescued {
        need.push(format!("{} rescued (now {})", req.rescued, info.rescued));
    }
    if need.is_empty() {
        Ok(())
    } else {
        Err(format!("Needs {}", need.join(" and ")))
    }
}

/// `endgame.js:available(id)` over the current save.
pub fn available_ending(
    data: &GameData,
    save: &SaveData,
    tier: u32,
    id: &str,
) -> Result<(), String> {
    match data.endgame.ending(id) {
        Some(def) => ending_available(def, &endgame_info(data, save, tier)),
        None => Err("Unknown ending".to_string()),
    }
}

/// `endgame.js:ENDINGS[id].lines(info)` — the four end-screen lines.
pub fn ending_lines(def: &EndingDef, info: &EndgameInfo) -> Vec<String> {
    let names: Vec<&str> = info.names.iter().map(String::as_str).collect();
    def.lines
        .iter()
        .map(|l| l.render(info.tier, &names))
        .collect()
}

/// `endgame.js:statsLine(info)`.
pub fn stats_line(info: &EndgameInfo) -> String {
    format!(
        "Runs {} · Deaths {} · Rescued {}/{} · Points {}",
        info.runs, info.deaths, info.rescued, info.total, info.points
    )
}

/// `endgame.js:isSourceUnlocked` — flame tier 4 and Deacon Maud (`ZONES.source.requires`).
pub fn is_source_unlocked(data: &GameData, save: &SaveData, tier: u32) -> bool {
    let req = data.zone("source").and_then(|z| z.requires.as_ref());
    let t = req.and_then(|r| r.tier).unwrap_or(4);
    let who = req.and_then(|r| r.rescued.as_deref()).unwrap_or("deacon");
    tier >= t && save.rescued.get(who)
}

/// `endgame.js:sourceLockReason` — `None` when open, else the board text.
pub fn source_lock_reason(data: &GameData, save: &SaveData, tier: u32) -> Option<String> {
    if is_source_unlocked(data, save, tier) {
        return None;
    }
    let mut missing = Vec::new();
    if tier < 4 {
        missing.push("flame tier 4");
    }
    if !save.rescued.deacon {
        missing.push("Deacon Maud");
    }
    Some(format!("Needs {}", missing.join(" and ")))
}

/// `endgame.js:hudLine` — `"Endings: ✦✧✧"` in the hub once any ending was seen, else empty.
pub fn endings_hud_line(save: &SaveData, in_hub: bool) -> String {
    if !in_hub || !save.endings.any() {
        return String::new();
    }
    let marks: String = Endings::IDS
        .iter()
        .map(|id| if save.endings.get(id) { '✦' } else { '✧' })
        .collect();
    format!("Endings: {marks}")
}

/// Which endgame overlay is up (`endgame.js` `S.screen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Choice,
    End,
}

/// The altar overlay state (`endgame.js` `S.screen` / `S.current`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndingScreen {
    pub screen: Option<Screen>,
    /// Ending id shown on the end screen.
    pub current: Option<String>,
}

/// `endgame.js:openChoice()` — E at the altar: ENDING mode, the choice screen; emits `altar` + `uiClick`.
pub fn open_choice(
    scr: &mut EndingScreen,
    in_zone: bool,
    x: f32,
    z: f32,
    zone_id: &str,
) -> (bool, Vec<SimEvent>) {
    if !in_zone {
        return (false, vec![]);
    }
    scr.screen = Some(Screen::Choice);
    scr.current = None;
    (
        true,
        vec![
            SimEvent::Altar {
                x,
                z,
                zone_id: zone_id.to_string(),
            },
            SimEvent::UiClick,
        ],
    )
}

/// `endgame.js:cancel()` — Esc on the choice screen: back to the run.
pub fn cancel_choice(scr: &mut EndingScreen) -> (bool, Vec<SimEvent>) {
    if scr.screen != Some(Screen::Choice) {
        return (false, vec![]);
    }
    scr.screen = None;
    (true, vec![SimEvent::UiClick])
}

/// `endgame.js:choose(id)` — pick an ending on the choice screen: refused (toast + `uiError`) when unavailable;
/// otherwise `save.endings[id] = true`, the end screen, `ending {id, choice, title, tier, rescued, first}`.
pub fn choose_ending(
    data: &GameData,
    save: &mut SaveData,
    scr: &mut EndingScreen,
    tier: u32,
    id: &str,
) -> (bool, Vec<SimEvent>) {
    if scr.screen != Some(Screen::Choice) {
        return (false, vec![]);
    }
    let def = match data.endgame.ending(id) {
        Some(d) => d,
        None => return (false, vec![]),
    };
    let info = endgame_info(data, save, tier);
    if let Err(why) = ending_available(def, &info) {
        return (
            false,
            vec![
                SimEvent::toast(format!("{}: {why}", def.choice)),
                SimEvent::ui_error(),
            ],
        );
    }
    let first = !save.endings.get(id);
    save.endings.set(id, true);
    scr.screen = Some(Screen::End);
    scr.current = Some(id.to_string());
    (
        true,
        vec![SimEvent::Ending {
            id: id.to_string(),
            choice: def.choice.clone(),
            title: def.title.clone(),
            tier: info.tier,
            rescued: info.rescued,
            first,
        }],
    )
}

/// `endgame.js:continueToHub()` — "Click to continue": the loot is discarded, `zoneExit`, then (after the
/// shell's `enterHub`) the `night` visit's toast when the ending is `night`, and `endingContinue {id}`. Returns
/// the ending id so the shell can dim the flame for the `night` visit.
pub fn continue_to_hub(
    scr: &mut EndingScreen,
    carried: &mut Carried,
    zone_id: &str,
) -> (Option<String>, Vec<SimEvent>) {
    if scr.screen != Some(Screen::End) {
        return (None, vec![]);
    }
    let id = scr.current.take().unwrap_or_default();
    scr.screen = None;
    *carried = Carried::default();
    let mut ev = vec![SimEvent::ZoneExit {
        zone_id: zone_id.to_string(),
    }];
    if id == "night" {
        ev.extend(begin_night_visit());
    }
    ev.push(SimEvent::EndingContinue { id: id.clone() });
    ev.push(SimEvent::UiClick);
    (Some(id), ev)
}

/// `endgame.js:beginNightVisit()` — the `night` ending's hub visit: the great flame reads as tier 1 (a shell
/// concern) and the one textual cue, `"The Lantern gutters. Only embers remain."`. [`continue_to_hub`] already
/// includes it after `zoneExit` (the JS runs it right after `enterHub`, before `endingContinue`).
pub fn begin_night_visit() -> Vec<SimEvent> {
    vec![SimEvent::toast("The Lantern gutters. Only embers remain.")]
}

/* ============================================================
Endgame — the Source run: ride up, laps, staged hunters
============================================================ */

/// A hunter kept dormant until the player closes in (`S.run.dormant[]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dormant {
    pub id: u32,
    pub wake_lap: i32,
    /// Profile `base` / `fast`: once woken it counts toward `ENDGAME.maxHunters` (creatures never do).
    pub counts_toward_max: bool,
}

/// An extra hunter to spawn (`endgame.js:spawnExtraHunter`) — the creature lane creates the record and the shell
/// emits `hunterSpawned {id, x, z, profile, lap}` with the id it assigned.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraSpawn {
    pub cx: i32,
    pub cz: i32,
    pub profile: String,
    pub lap: i32,
}

/// `endgame.js` `S.run` — per Source run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceRun {
    /// Deepest lap reached.
    pub max_lap: i32,
    /// The visual lap, easing after the real one (`tintLerp`).
    pub v_lap: f32,
    pub dormant: Vec<Dormant>,
    pub extras: u32,
    pub spawned_laps: Vec<i32>,
    pub announced: Vec<i32>,
    /// `S.rideConfirmT` — seconds the "press E again" confirmation stays armed.
    pub ride_confirm_t: f32,
}

/// `endgame.js:startRun()` — hunters that spawn deep start dormant and wake as the player closes in: a hunter
/// at lap `L` sleeps when `L − wakeLapAhead > 0`. `hunters` = `(id, x, z, profile)` of the active hunters
/// (creatures included); the returned run lists the ids the shell must deactivate (`h.active = false`, mesh
/// hidden).
pub fn start_source_run(
    cfg: &Config,
    map: &ParsedMap,
    hunters: &[(u32, f32, f32, &str)],
) -> SourceRun {
    let mut run = SourceRun::default();
    for &(id, x, z, profile) in hunters {
        let (cx, cz) = grid::to_cell(map, x, z);
        let lap = grid::lap_of_map(map, cx, cz);
        let wake_lap = lap - cfg.endgame.wake_lap_ahead;
        if wake_lap > 0 {
            run.dormant.push(Dormant {
                id,
                wake_lap,
                counts_toward_max: profile == "base" || profile == "fast",
            });
        }
    }
    run
}

/// What crossing a lap line produced (`endgame.js:onDeeper`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LapOutcome {
    pub events: Vec<SimEvent>,
    /// Dormant hunters to wake (each also got a `hunterWoken` event).
    pub woken: Vec<u32>,
    /// Extra fast hunters to place.
    pub spawn: Vec<ExtraSpawn>,
}

/// `endgame.js:update` lap tracking — with the player at `player_lap` in ZONE mode (unpaused), returns
/// `Some((lap, prev))` when a new deepest lap was reached (the caller then runs [`on_deeper`]); also eases
/// `v_lap` toward the target lap (`max_lap` outside ZONE) and ticks `ride_confirm_t`.
pub fn update_source_run(
    run: &mut SourceRun,
    cfg: &Config,
    dt: f32,
    in_zone_unpaused: bool,
    player_lap: i32,
) -> Option<(i32, i32)> {
    run.ride_confirm_t = (run.ride_confirm_t - dt).max(0.0);
    let mut crossed = None;
    if in_zone_unpaused && player_lap > run.max_lap {
        let prev = run.max_lap;
        run.max_lap = player_lap;
        crossed = Some((player_lap, prev));
    }
    let target = if in_zone_unpaused {
        player_lap
    } else {
        run.max_lap
    } as f32;
    run.v_lap += (target - run.v_lap) * (dt * cfg.endgame.tint_lerp).min(1.0);
    crossed
}

/// `endgame.js:applyTint(vLap)` numbers: `(fog density multiplier, fog tint lerp k, ambient scale)` —
/// `1 + fogPerLap·vLap`, `min(1, vLap/5)·0.6`, `max(0.25, 1 − ambientPerLap·vLap)`.
pub fn lap_tint(cfg: &Config, v_lap: f32) -> (f32, f32, f32) {
    let e = &cfg.endgame;
    (
        1.0 + e.fog_per_lap * v_lap,
        (v_lap / 5.0).min(1.0) * 0.6,
        (1.0 - e.ambient_per_lap * v_lap).max(0.25),
    )
}

/// `endgame.js:onDeeper(lap, prev)` — `lap {lap, prev, zoneId}`, the `LAP_LINES` toast (once per lap), the
/// dormant hunters whose `wakeLap ≤ lap` (`hunterWoken`), and on each of `extraLaps` one extra fast hunter
/// 12–34 BFS cells from the player unless the active base/fast count has reached `maxHunters` (creatures are
/// not counted). `active_base_fast` is that count *before* this call — earlier extras included, dormant hunters
/// excluded; the JS recounts `ctx.hunters` after waking, so hunters woken here are added to it.
#[allow(clippy::too_many_arguments)]
pub fn on_deeper(
    data: &GameData,
    run: &mut SourceRun,
    lap: i32,
    prev: i32,
    zone_id: &str,
    map: &ParsedMap,
    player: (f32, f32),
    active_base_fast: u32,
    rng: &mut dyn RandomSource,
) -> LapOutcome {
    let cfg = &data.config;
    let mut out = LapOutcome::default();
    out.events.push(SimEvent::Lap {
        lap,
        prev,
        zone_id: zone_id.to_string(),
    });
    if let Some(Some(line)) = data.endgame.lap_lines.get(lap as usize) {
        if !run.announced.contains(&lap) {
            run.announced.push(lap);
            out.events.push(SimEvent::toast(line.clone()));
        }
    }
    let mut active = active_base_fast;
    let mut i = 0;
    while i < run.dormant.len() {
        if lap < run.dormant[i].wake_lap {
            i += 1;
            continue;
        }
        let d = run.dormant.remove(i);
        out.woken.push(d.id);
        out.events.push(SimEvent::HunterWoken { id: d.id, lap });
        if d.counts_toward_max {
            active += 1;
        }
    }
    for &l in &cfg.endgame.extra_laps {
        if lap < l || run.spawned_laps.contains(&l) {
            continue;
        }
        run.spawned_laps.push(l);
        if active >= cfg.endgame.max_hunters {
            continue;
        }
        if let Some((cx, cz)) = pick_spawn_cell(cfg, map, player, lap, rng) {
            run.extras += 1;
            active += 1;
            out.spawn.push(ExtraSpawn {
                cx,
                cz,
                profile: "fast".to_string(),
                lap,
            });
        }
    }
    out
}

/// `endgame.js:pickSpawnCell(lap)` — a floor/deep cell on this lap or the next, `spawnMinCells`–`spawnMaxCells`
/// BFS steps from the player (solid-blocked BFS), one of the six farthest at random.
pub fn pick_spawn_cell(
    cfg: &Config,
    map: &ParsedMap,
    player: (f32, f32),
    lap: i32,
    rng: &mut dyn RandomSource,
) -> Option<(i32, i32)> {
    let (pcx, pcz) = grid::to_cell(map, player.0, player.1);
    if !grid::in_bounds(map, pcx, pcz) {
        return None;
    }
    let f = grid::bfs_solid(map, pcx, pcz);
    let e = &cfg.endgame;
    let mut cands: Vec<(i32, i32, i16)> = Vec::new();
    for (i, &d) in f.dist.iter().enumerate() {
        if (d as i32) < e.spawn_min_cells || (d as i32) > e.spawn_max_cells {
            continue;
        }
        let t = map.cells[i];
        if t != CellKind::Floor && t != CellKind::Deep {
            continue;
        }
        let (cx, cz) = map.cell_of(i);
        let l = grid::lap_of_map(map, cx, cz);
        if l != lap && l != lap + 1 {
            continue;
        }
        cands.push((cx, cz, d));
    }
    if cands.is_empty() {
        return None;
    }
    cands.sort_by_key(|a| std::cmp::Reverse(a.2));
    let n = cands.len().min(6);
    let pick = ((rng.unit() * n as f64) as usize).min(n - 1);
    Some((cands[pick].0, cands[pick].1))
}

/// `endgame.js:rideUp()` — leave the Source from V. No banking here: with loot carried the first press only
/// arms a `rideConfirmT` confirmation (toast + `uiError`); the second (or an empty-handed press) discards the
/// loot (`sourceAbandoned`), toasts, and `zoneExit`s — the shell then fades and calls [`enter_hub`].
pub fn ride_up(
    cfg: &Config,
    run: &mut SourceRun,
    carried: &mut Carried,
    in_zone_source: bool,
    zone_id: &str,
) -> (bool, Vec<SimEvent>) {
    if !in_zone_source {
        return (false, vec![]);
    }
    if carried.total() > 0 && run.ride_confirm_t <= 0.0 {
        run.ride_confirm_t = cfg.endgame.ride_confirm_t;
        return (
            true,
            vec![
                SimEvent::toast("There is no banking here. Ride up now and what you carry stays below — press E again to leave."),
                SimEvent::ui_error(),
            ],
        );
    }
    let c = *carried;
    *carried = Carried::default();
    run.ride_confirm_t = 0.0;
    let msg = if c.total() > 0 {
        format!(
            "You ride up out of the Source. {} stays below.",
            describe(&c)
        )
    } else {
        "You ride up out of the Source. The altar keeps its light.".to_string()
    };
    (
        true,
        vec![
            SimEvent::SourceAbandoned {
                carried: c,
                zone_id: zone_id.to_string(),
            },
            SimEvent::toast(msg),
            SimEvent::ZoneExit {
                zone_id: zone_id.to_string(),
            },
        ],
    )
}

/// `endgame.js:inSource(c)` — the loaded zone is an endgame zone: it forbids banking (`META.noBank`), it is
/// the Source itself, or its map carries an altar (`A`). Everything `endgame.js` gates on (`interactTarget`,
/// `rideUp`, the lap machinery) reads this predicate.
pub fn in_source(zone: &ZoneDef, map: &ParsedMap) -> bool {
    zone.no_bank || zone.id == "source" || map.altar.is_some()
}

/// `endgame.js:interactTarget` — the altar is in reach when within `ENDGAME.altarR` of its cell centre.
pub fn altar_in_reach(cfg: &Config, map: &ParsedMap, px: f32, pz: f32) -> bool {
    match map.altar {
        Some(a) => dist2d(a.x, a.z, px, pz) <= cfg.endgame.altar_r,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::ScriptedRng;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    fn names(ev: &[SimEvent]) -> Vec<&'static str> {
        ev.iter().map(SimEvent::name).collect()
    }

    #[test]
    fn describe_and_points() {
        let d = data();
        let c = Carried {
            oil: 1,
            relic: 2,
            rich: 1,
            quest: 1,
        };
        assert_eq!(
            describe(&c),
            "1 flask, 2 relics, 1 rich relic, 1 quest item"
        );
        assert_eq!(describe(&Carried::default()), "nothing");
        assert_eq!(bank_points(&d.config.points, &c), 1 + 6 + 5);
    }

    #[test]
    fn flask_is_a_point_or_lamp_oil_never_both() {
        let d = data();
        let mut save = SaveData::default();
        let mut carried = Carried {
            oil: 2,
            ..Default::default()
        };
        let mut lamp = LampState {
            oil: 40.0,
            lamp_on: true,
            ..Default::default()
        };
        // T: one flask → +25 lamp oil, nothing banked
        let (ok, ev) = top_up(&mut lamp, &d.config, &mut carried, true);
        assert!(ok);
        assert_eq!(lamp.oil, 65.0);
        assert_eq!(carried.oil, 1);
        assert_eq!(ev, vec![SimEvent::TopUp { oil: 65.0 }]);
        // the remaining flask banks: +1 point, +25 banked oil
        let ev = bank(&d.config, &mut save, &mut carried, "undercroft");
        assert_eq!(save.points, 1);
        assert_eq!(save.oil, 25);
        assert_eq!(save.stats.banked, 1);
        assert!(carried.is_empty());
        assert_eq!(names(&ev), vec!["bank", "toast", "zoneExit"]);
        assert_eq!(ev[1], SimEvent::toast("Banked: 1 flask (+1)"));
        // full lamp: refused
        lamp.oil = 100.0;
        carried.oil = 1;
        assert!(!top_up(&mut lamp, &d.config, &mut carried, true).0);
        assert!(!top_up(&mut lamp, &d.config, &mut carried, false).0);
        // empty bank
        let mut none = Carried::default();
        let ev = bank(&d.config, &mut save, &mut none, "undercroft");
        assert_eq!(
            ev[1],
            SimEvent::toast("Nothing to bank. You return to the Lantern.")
        );
    }

    #[test]
    fn lamp_burn_multipliers_and_reach() {
        let d = data();
        let cfg = &d.config;
        let save = SaveData::default();
        let tech = light_tech(cfg, &save);
        let u = d.zone("undercroft").unwrap();
        let o = d.zone("ossuary").unwrap();
        let s = d.zone("source").unwrap();
        let m = zone_mul(cfg, Some(u), tech, false, 0);
        assert_eq!((m.burn, m.dist), (1.0, 1.0));
        let m = zone_mul(cfg, Some(u), tech, true, 0);
        assert_eq!((m.burn, m.dist), (1.5, 0.6));
        let m = zone_mul(cfg, Some(o), tech, false, 0);
        assert!((m.burn - 1.3).abs() < 1e-6 && (m.dist - 0.85).abs() < 1e-6);
        let m = zone_mul(cfg, Some(s), tech, true, 3);
        assert!((m.burn - (1.1 + 0.36)).abs() < 1e-6);
        assert!((m.dist - (0.95 - 0.24)).abs() < 1e-6);
        // light-tech III: ×0.7 burn, ×1.45 reach
        let mut save3 = SaveData {
            light_tech: 3,
            ..Default::default()
        };
        let t3 = light_tech(cfg, &save3);
        let m = zone_mul(cfg, None, t3, false, 0);
        assert!((m.burn - 0.7).abs() < 1e-6 && (m.dist - 1.45).abs() < 1e-6);
        assert_eq!(t3.flash_cost, 10.0);
        assert_eq!(t3.lantern_cost, 16.0);
        save3.light_tech = 99;
        assert_eq!(light_tech(cfg, &save3).dist_mul, 1.45); // clamped

        // burn: 0.5/s × mul, oil 0 forces the lamp off
        let mut lamp = LampState {
            oil: 1.0,
            lamp_on: true,
            flash_cd: 0.5,
            ..Default::default()
        };
        let reach = update_lamp(
            &mut lamp,
            cfg,
            1.0,
            ZoneMul {
                burn: 1.5,
                dist: 0.6,
            },
            true,
        );
        assert!((lamp.oil - 0.25).abs() < 1e-6);
        assert!(lamp.lamp_on);
        assert!((reach - 6.6).abs() < 1e-5);
        assert_eq!(lamp.flash_cd, 0.0);
        update_lamp(&mut lamp, cfg, 1.0, ZoneMul::default(), true);
        assert_eq!(lamp.oil, 0.0);
        assert!(!lamp.lamp_on);
        // hub: forced off, no burn
        lamp.oil = 50.0;
        lamp.lamp_on = true;
        update_lamp(&mut lamp, cfg, 1.0, ZoneMul::default(), false);
        assert!(!lamp.lamp_on);
        assert_eq!(lamp.oil, 50.0);
        // intensity: flash overrides, off is 0
        assert_eq!(lamp_intensity(&lamp, cfg, 0.0), 0.0);
        lamp.flash_t = 0.1;
        assert_eq!(lamp_intensity(&lamp, cfg, 0.0), 6.0 * 8.0);
        lamp.flash_t = 0.0;
        lamp.lamp_on = true;
        assert!((lamp_intensity(&lamp, cfg, 0.0) - 6.0 * 0.93).abs() < 1e-5);
    }

    #[test]
    fn toggle_flash_and_lantern_costs() {
        let d = data();
        let cfg = &d.config;
        let save = SaveData::default();
        let tech = light_tech(cfg, &save);
        let mut lamp = LampState {
            oil: 30.0,
            lamp_on: false,
            ..Default::default()
        };
        assert!(!toggle_lamp(&mut lamp, false).0);
        lamp.lamp_lock = 1.0;
        let (ok, ev) = toggle_lamp(&mut lamp, true);
        assert!(!ok);
        assert_eq!(names(&ev), vec!["toast", "uiError"]);
        lamp.lamp_lock = 0.0;
        let (ok, ev) = toggle_lamp(&mut lamp, true);
        assert!(ok && lamp.lamp_on);
        assert_eq!(ev, vec![SimEvent::LampToggle { on: true }]);
        // flash: 15 oil, 1.5 s cooldown, 0.15 s burst
        let (ok, ev) = flash(&mut lamp, cfg, tech, true, 1.0, 2.0);
        assert!(ok);
        assert_eq!(lamp.oil, 15.0);
        assert_eq!(lamp.flash_cd, 1.5);
        assert_eq!(lamp.flash_t, 0.15);
        assert_eq!(ev, vec![SimEvent::Flash { x: 1.0, z: 2.0 }]);
        assert!(!flash(&mut lamp, cfg, tech, true, 1.0, 2.0).0, "cooldown");
        lamp.flash_cd = 0.0;
        // exactly the cost left is enough; one short is not
        assert!(flash(&mut lamp, cfg, tech, true, 1.0, 2.0).0);
        assert_eq!(lamp.oil, 0.0);
        lamp.flash_cd = 0.0;
        lamp.oil = 14.9;
        assert!(!flash(&mut lamp, cfg, tech, true, 1.0, 2.0).0);
        assert!(!flash(&mut lamp, cfg, tech, false, 1.0, 2.0).0);
        // lantern: 20 oil, recycles the oldest at 4 alive
        lamp.oil = 45.0;
        let (ok, ev) = plant_lantern(&mut lamp, cfg, tech, true, 4, Some((3.0, 3.0)), 5.0, 6.0);
        assert!(ok);
        assert_eq!(lamp.oil, 25.0);
        assert_eq!(lamp.lantern_cd, 1.0);
        assert_eq!(
            ev,
            vec![
                SimEvent::LanternRemoved { x: 3.0, z: 3.0 },
                SimEvent::Lantern { x: 5.0, z: 6.0 }
            ]
        );
        assert!(
            !plant_lantern(&mut lamp, cfg, tech, true, 0, None, 5.0, 6.0).0,
            "cooldown"
        );
        lamp.lantern_cd = 0.0;
        let (ok, ev) = plant_lantern(&mut lamp, cfg, tech, true, 1, None, 5.0, 6.0);
        assert!(ok);
        assert_eq!(names(&ev), vec!["lantern"]);
        assert!(
            !plant_lantern(&mut lamp, cfg, tech, true, 1, None, 5.0, 6.0).0,
            "5 oil < 20"
        );
    }

    #[test]
    fn start_oil_by_tier_and_reservoir() {
        let d = data();
        let cfg = &d.config;
        assert_eq!(start_oil(cfg, 1, 0), 45.0);
        assert_eq!(start_oil(cfg, 2, 0), 50.0);
        assert_eq!(start_oil(cfg, 3, 0), 55.0);
        assert_eq!(start_oil(cfg, 4, 0), 60.0);
        assert_eq!(start_oil(cfg, 4, 1), 75.0);
        assert_eq!(start_oil(cfg, 4, 2), 90.0);
        assert_eq!(start_oil(cfg, 1, 2), 75.0);
    }

    #[test]
    fn tiers_at_6_15_30() {
        let d = data();
        let cfg = &d.config;
        assert_eq!(tier_for(&cfg.tiers, 0), 1);
        assert_eq!(tier_for(&cfg.tiers, 5), 1);
        assert_eq!(tier_for(&cfg.tiers, 6), 2);
        assert_eq!(tier_for(&cfg.tiers, 14), 2);
        assert_eq!(tier_for(&cfg.tiers, 15), 3);
        assert_eq!(tier_for(&cfg.tiers, 29), 3);
        assert_eq!(tier_for(&cfg.tiers, 30), 4);
        assert_eq!(tier_for(&cfg.tiers, 999), 4);
        let mut save = SaveData::default();
        let mut hub = HubState::from_save(cfg, &save);
        assert_eq!(hub.tier, 1);
        assert_eq!(
            init_tier(cfg, &save, &mut hub),
            vec![SimEvent::FlameTier {
                tier: 1,
                prev: 0,
                initial: true
            }]
        );
        assert!(check_tier(cfg, &save, &mut hub, true).is_empty());
        assert_eq!(
            check_tier(cfg, &save, &mut hub, false),
            vec![SimEvent::FlameTier {
                tier: 1,
                prev: 1,
                initial: true
            }]
        );
        let ev = set_points(cfg, &mut save, &mut hub, 16);
        assert_eq!(
            ev,
            vec![SimEvent::FlameTier {
                tier: 3,
                prev: 1,
                initial: false
            }]
        );
        assert_eq!(hub.tier, 3);
        // enterHub re-checks and sets the HUD oil to the run's start oil
        let mut lamp = LampState {
            oil: 3.0,
            lamp_on: true,
            flash_cd: 1.0,
            ..Default::default()
        };
        save.reservoir = 1;
        let ev = enter_hub(cfg, &save, &mut hub, &mut lamp);
        assert_eq!(names(&ev), vec!["hubEnter"]);
        assert!(!lamp.lamp_on);
        assert_eq!(lamp.oil, 70.0);
        assert_eq!(lamp.flash_cd, 0.0);
        // startRun lights the lamp with start oil and counts the run
        let ev = start_run(cfg, &mut save, &hub, &mut lamp, "undercroft");
        assert_eq!(
            ev,
            vec![SimEvent::ZoneEnter {
                zone_id: "undercroft".into()
            }]
        );
        assert!(lamp.lamp_on && lamp.oil == 70.0);
        assert_eq!(save.stats.runs, 1);
    }

    #[test]
    fn every_building_costs_and_gates() {
        let d = data();
        let cfg = &d.config;
        let mut save = SaveData::default();
        let hub = HubState::default();
        // the board is always built
        let st = build_status(&d, &save, 1, "board").unwrap();
        assert!(st.built && st.reason.as_deref() == Some("Already built"));
        assert_eq!(cost_text(cost_of(cfg, "board")), "free");
        assert!(!build(&d, &mut save, &hub, "board", false).0);
        assert!(build_status(&d, &save, 1, "nope").is_none());
        // NPC gates
        let st = build_status(&d, &save, 1, "workshop").unwrap();
        assert!(!st.unlocked);
        assert_eq!(
            st.reason.as_deref(),
            Some("Needs Wick the Lamplighter rescued")
        );
        let (ok, ev) = build(&d, &mut save, &hub, "workshop", false);
        assert!(!ok);
        assert_eq!(names(&ev), vec!["toast", "uiError"]);
        for (b, npc) in [
            ("workshop", "lamplighter"),
            ("press", "keeper"),
            ("cart", "cartographer"),
            ("shrine", "deacon"),
        ] {
            assert!(!unlocked(&d, &save, 4, b));
            save.rescued.set(npc, true);
            assert!(unlocked(&d, &save, 1, b));
        }
        // tier gates
        assert_eq!(
            build_status(&d, &save, 1, "tram")
                .unwrap()
                .reason
                .as_deref(),
            Some("Needs the flame at tier 2")
        );
        assert_eq!(
            build_status(&d, &save, 2, "elevator")
                .unwrap()
                .reason
                .as_deref(),
            Some("Needs the flame at tier 3")
        );
        assert!(unlocked(&d, &save, 2, "tram") && unlocked(&d, &save, 3, "elevator"));
        // costs
        assert_eq!(
            cost_of(cfg, "workshop"),
            BuildSpend {
                oil: 0,
                relics: 6,
                rich: 0
            }
        );
        assert_eq!(
            cost_of(cfg, "press"),
            BuildSpend {
                oil: 80,
                relics: 0,
                rich: 0
            }
        );
        assert_eq!(
            cost_of(cfg, "cart"),
            BuildSpend {
                oil: 100,
                relics: 0,
                rich: 0
            }
        );
        assert_eq!(
            cost_of(cfg, "shrine"),
            BuildSpend {
                oil: 150,
                relics: 0,
                rich: 0
            }
        );
        assert_eq!(
            cost_of(cfg, "tram"),
            BuildSpend {
                oil: 120,
                relics: 0,
                rich: 0
            }
        );
        assert_eq!(
            cost_of(cfg, "elevator"),
            BuildSpend {
                oil: 250,
                relics: 4,
                rich: 0
            }
        );
        assert_eq!(cost_text(cost_of(cfg, "elevator")), "250 oil + 4 relics");
        assert_eq!(
            cost_text(BuildSpend {
                oil: 0,
                relics: 1,
                rich: 2
            }),
            "1 relic + 2 rich"
        );
        // short of resources
        save.oil = 100;
        save.relics = 2;
        let st = build_status(&d, &save, 3, "elevator").unwrap();
        assert!(st.unlocked && !st.affordable);
        assert_eq!(
            st.reason.as_deref(),
            Some("Needs 150 more oil, 2 more relics")
        );
        assert!(!can_build(&d, &save, 3, "elevator"));
        // build the tram: spends, emits, and announces the Cistern
        save.oil = 130;
        let hub2 = HubState {
            tier: 2,
            blessed: false,
        };
        assert!(can_build(&d, &save, 2, "tram"));
        let (ok, ev) = build(&d, &mut save, &hub2, "tram", false);
        assert!(ok);
        assert_eq!(save.oil, 10);
        assert!(save.buildings.tram);
        assert_eq!(names(&ev), vec!["build", "uiClick", "toast", "toast"]);
        assert_eq!(
            ev[3],
            SimEvent::toast("The Cistern is open. Choose it at the Departure Board.")
        );
        assert!(zone_locked(&d, &save, 2, "cistern").is_none());
        // free build skips cost and gates
        let (ok, ev) = build(&d, &mut save, &hub, "elevator", true);
        assert!(ok);
        assert_eq!(
            ev[0],
            SimEvent::Build {
                id: "elevator".into(),
                cost: BuildSpend::default()
            }
        );
        assert_eq!(save.oil, 10);
        // the elevator alone does not open the Ossuary (light-tech II)
        assert_eq!(
            zone_locked(&d, &save, 3, "ossuary").as_deref(),
            Some("Needs Light-tech II")
        );
        for b in &d.buildings.order {
            assert!(is_built(&d, &save, b) || build_status(&d, &save, 4, b).is_some());
        }
    }

    #[test]
    fn workshop_press_and_shrine_services() {
        let d = data();
        let cfg = &d.config;
        let mut save = SaveData::default();
        let mut hub = HubState {
            tier: 3,
            blessed: false,
        };
        save.buildings.elevator = true;
        // light-tech I / II / III: 6 / 10 / 14 relics (+2 rich)
        let (ok, ev) = upgrade_light_tech(&d, &mut save, &hub);
        assert!(!ok);
        assert_eq!(ev[0], SimEvent::toast("Needs 6 more relics"));
        save.relics = 30;
        save.rich = 1;
        let (ok, ev) = upgrade_light_tech(&d, &mut save, &hub);
        assert!(ok && save.light_tech == 1 && save.relics == 24);
        assert_eq!(names(&ev), vec!["lightTech", "service", "uiClick", "toast"]);
        assert_eq!(
            ev[3],
            SimEvent::toast("Light-tech I: reach ×1.15, burn ×0.9.")
        );
        let (ok, ev) = upgrade_light_tech(&d, &mut save, &hub);
        assert!(ok && save.light_tech == 2 && save.relics == 14);
        assert_eq!(
            ev.last(),
            Some(&SimEvent::toast(
                "The Ossuary is open. Choose it at the Departure Board."
            ))
        );
        let (ok, ev) = upgrade_light_tech(&d, &mut save, &hub);
        assert!(!ok);
        assert_eq!(ev[0], SimEvent::toast("Needs 1 more rich relics"));
        save.rich = 2;
        let (ok, _) = upgrade_light_tech(&d, &mut save, &hub);
        assert!(ok && save.light_tech == 3 && save.relics == 0 && save.rich == 0);
        let (ok, ev) = upgrade_light_tech(&d, &mut save, &hub);
        assert!(!ok);
        assert_eq!(ev[0], SimEvent::toast("Nothing more to learn here."));
        // press: 30 oil per relic
        let (ok, ev) = press_relics(cfg, &mut save, 1);
        assert!(!ok);
        assert_eq!(ev[0], SimEvent::toast("No relics to press."));
        save.relics = 5;
        let (ok, ev) = press_relics(cfg, &mut save, 1);
        assert!(ok && save.relics == 4 && save.oil == 30);
        assert_eq!(
            ev[0],
            SimEvent::Service(ServiceAction::Press { n: 1, oil: 30 })
        );
        let (ok, ev) = press_relics(cfg, &mut save, 99);
        assert!(ok && save.relics == 0 && save.oil == 150);
        assert_eq!(ev[2], SimEvent::toast("Pressed 4 relics into 120 oil."));
        // reservoir: 5 then 8 relics, +15 each
        let (so, ev) = deepen_reservoir(cfg, &mut save, &hub);
        assert!(so.is_none());
        assert_eq!(ev[0], SimEvent::toast("Needs 5 more relics"));
        save.relics = 13;
        let (so, ev) = deepen_reservoir(cfg, &mut save, &hub);
        assert_eq!(so, Some(70.0));
        assert_eq!(save.reservoir, 1);
        assert_eq!(
            ev[0],
            SimEvent::Service(ServiceAction::Reservoir {
                level: 1,
                start_oil: 70.0
            })
        );
        assert_eq!(
            ev[2],
            SimEvent::toast("Deeper reservoir: the lamp now starts with 70 oil.")
        );
        let (so, _) = deepen_reservoir(cfg, &mut save, &hub);
        assert_eq!(so, Some(85.0));
        assert_eq!(save.relics, 0);
        let (so, ev) = deepen_reservoir(cfg, &mut save, &hub);
        assert!(so.is_none());
        assert_eq!(
            ev[0],
            SimEvent::toast("The reservoir is as deep as it goes.")
        );
        // blessing: toggle, charge 40 at descent, keep half on death
        let ev = toggle_blessing(cfg, &mut save);
        assert!(save.blessing);
        assert_eq!(ev[0], SimEvent::Blessing { on: true });
        assert_eq!(
            ev[2],
            SimEvent::toast("The blessing is lit — 40 oil at each descent.")
        );
        assert!(
            charge_blessing(cfg, &mut save, &mut hub).is_empty(),
            "no shrine yet"
        );
        save.buildings.shrine = true;
        save.oil = 30;
        let ev = charge_blessing(cfg, &mut save, &mut hub);
        assert!(!hub.blessed);
        assert_eq!(
            ev,
            vec![
                SimEvent::toast("No oil for the blessing; the shrine candles stay dark."),
                SimEvent::Service(ServiceAction::Charge { lit: false }),
            ]
        );
        save.oil = 50;
        let ev = charge_blessing(cfg, &mut save, &mut hub);
        assert!(hub.blessed && save.oil == 10);
        assert_eq!(
            ev[1],
            SimEvent::Service(ServiceAction::Charge { lit: true })
        );
        let c = Carried {
            oil: 3,
            relic: 1,
            rich: 2,
            quest: 1,
        };
        let (rest, ev) = apply_blessing(cfg, &mut save, &mut hub, c, "ossuary", 1.0, 2.0);
        assert_eq!(
            rest,
            Carried {
                oil: 2,
                relic: 1,
                rich: 1,
                quest: 1
            }
        );
        assert_eq!(save.oil, 35);
        assert_eq!(save.rich, 1);
        assert_eq!(save.points, 6);
        assert_eq!(save.stats.banked, 6);
        assert!(!hub.blessed, "spent");
        assert_eq!(
            ev[0],
            SimEvent::BlessingKept {
                kept: Carried {
                    oil: 1,
                    relic: 0,
                    rich: 1,
                    quest: 0
                },
                pts: 6,
                zone_id: "ossuary".into(),
                x: 1.0,
                z: 2.0
            }
        );
        assert_eq!(
            ev[1],
            SimEvent::toast("The blessing kept 1 flask, 1 rich relic (+6).")
        );
        // unblessed / nothing to halve: unchanged, silent
        assert_eq!(
            apply_blessing(cfg, &mut save, &mut hub, c, "ossuary", 0.0, 0.0),
            (c, vec![])
        );
        hub.blessed = true;
        let one = Carried {
            oil: 1,
            relic: 1,
            rich: 1,
            quest: 0,
        };
        assert_eq!(
            apply_blessing(cfg, &mut save, &mut hub, one, "ossuary", 0.0, 0.0),
            (one, vec![])
        );
        let ev = toggle_blessing(cfg, &mut save);
        assert_eq!(ev[2], SimEvent::toast("The blessing is snuffed."));
        let hud = hub_hud_text(&d, &save);
        assert_eq!(hud, "Banked: 35 oil · 0 relics · 1 rich\nLight-tech III · reservoir +30 · blessing unlit\nNext descent: The Undercroft");
    }

    #[test]
    fn death_drops_a_bundle_and_keeps_points() {
        let d = data();
        let cfg = &d.config;
        let mut save = SaveData {
            points: 9,
            ..Default::default()
        };
        let mut hub = HubState {
            tier: 2,
            blessed: true,
        };
        let mut carried = Carried {
            oil: 2,
            relic: 1,
            rich: 0,
            quest: 1,
        };
        let out = die(
            cfg,
            &mut save,
            &mut hub,
            &mut carried,
            "undercroft",
            4.0,
            5.0,
            Some(0),
        );
        assert_eq!(
            out.bundle,
            Some(Carried {
                oil: 1,
                relic: 1,
                rich: 0,
                quest: 1
            })
        );
        assert_eq!(out.lost, "1 flask, 1 relic, 1 quest item");
        assert_eq!(names(&out.events), vec!["blessingKept", "toast", "death"]);
        assert_eq!(
            save.points, 10,
            "blessing banked 1 flask; points never lost"
        );
        assert_eq!(save.stats.deaths, 1);
        assert!(carried.is_empty());
        // empty-handed: no bundle
        let out = die(
            cfg,
            &mut save,
            &mut hub,
            &mut carried,
            "undercroft",
            4.0,
            5.0,
            None,
        );
        assert_eq!(out.bundle, None);
        assert_eq!(out.lost, "nothing");
        assert_eq!(
            out.events,
            vec![SimEvent::Death {
                x: 4.0,
                z: 5.0,
                hunter_id: None
            }]
        );
        // the bundle comes back whole on pickup
        let ev = pickup(
            &mut carried,
            ItemKind::Bundle,
            Some(Carried {
                oil: 1,
                relic: 1,
                rich: 0,
                quest: 1,
            }),
            4.0,
            5.0,
        );
        assert_eq!(
            carried,
            Carried {
                oil: 1,
                relic: 1,
                rich: 0,
                quest: 1
            }
        );
        assert_eq!(
            ev[0],
            SimEvent::toast("Recovered your bundle: 1 flask, 1 relic, 1 quest item")
        );
        let ev = pickup(&mut carried, ItemKind::Rich, None, 0.0, 0.0);
        assert_eq!(carried.rich, 1);
        assert_eq!(
            ev,
            vec![SimEvent::Pickup {
                kind: ItemKind::Rich,
                x: 0.0,
                z: 0.0,
                contents: None
            }]
        );
    }

    #[test]
    fn board_selection_and_descend() {
        let d = data();
        let mut save = SaveData::default();
        let hub = HubState::default();
        assert_eq!(selected(&d, &save), "undercroft");
        save.zone_selected = "atlantis".into();
        assert_eq!(selected(&d, &save), "undercroft");
        assert!(!select_zone(&d, &mut save, &hub, "atlantis", true).0);
        let (ok, ev) = select_zone(&d, &mut save, &hub, "cistern", true);
        assert!(!ok);
        assert_eq!(ev[0], SimEvent::toast("The Cistern: Needs the Tram dock"));
        let (ok, ev) = select_zone(&d, &mut save, &hub, "cistern", false);
        assert!(ok && save.zone_selected == "cistern");
        assert_eq!(
            ev,
            vec![SimEvent::ZoneSelected {
                zone_id: "cistern".into()
            }]
        );
        let err = descend(&d, &save, &hub, None).unwrap_err();
        assert_eq!(names(&err), vec!["toast", "uiError"]);
        assert_eq!(
            descend(&d, &save, &hub, Some("cistern")),
            Ok("cistern".into())
        );
        save.buildings.tram = true;
        let (ok, ev) = select_zone(&d, &mut save, &hub, "undercroft", true);
        assert!(ok);
        assert_eq!(names(&ev), vec!["zoneSelected", "uiClick", "toast"]);
        assert_eq!(descend(&d, &save, &hub, None), Ok("undercroft".into()));
        let (ok, ev) = give_tool(&mut save, "prybar");
        assert!(ok);
        assert_eq!(
            ev,
            vec![SimEvent::ToolGained {
                id: "prybar".into()
            }]
        );
        assert_eq!(give_tool(&mut save, "prybar"), (true, vec![]));
        assert_eq!(give_tool(&mut save, "hammer"), (false, vec![]));
    }

    #[test]
    fn endings_availability_lines_and_choice() {
        let d = data();
        let mut save = SaveData::default();
        let info = endgame_info(&d, &save, 2);
        assert_eq!(info.total, 4);
        assert_eq!(available_ending(&d, &save, 2, "cage"), Ok(()));
        assert_eq!(available_ending(&d, &save, 2, "night"), Ok(()));
        assert_eq!(
            available_ending(&d, &save, 2, "dawn"),
            Err("Needs flame tier 4 (now 2) and 3 rescued (now 0)".into())
        );
        assert_eq!(
            available_ending(&d, &save, 2, "x"),
            Err("Unknown ending".into())
        );
        save.rescued.lamplighter = true;
        save.rescued.keeper = true;
        save.rescued.deacon = true;
        assert_eq!(
            available_ending(&d, &save, 3, "dawn"),
            Err("Needs flame tier 4 (now 3)".into())
        );
        assert_eq!(available_ending(&d, &save, 4, "dawn"), Ok(()));
        save.rescued.keeper = false;
        assert_eq!(
            available_ending(&d, &save, 4, "dawn"),
            Err("Needs 3 rescued (now 2)".into())
        );
        assert!(!is_source_unlocked(&d, &save, 3));
        assert_eq!(
            source_lock_reason(&d, &save, 3).as_deref(),
            Some("Needs flame tier 4")
        );
        assert!(is_source_unlocked(&d, &save, 4));
        save.rescued.deacon = false;
        assert_eq!(
            source_lock_reason(&d, &save, 1).as_deref(),
            Some("Needs flame tier 4 and Deacon Maud")
        );
        save.rescued.deacon = true;
        save.stats.runs = 7;
        save.stats.deaths = 2;
        save.points = 33;
        let info = endgame_info(&d, &save, 4);
        assert_eq!(info.names, vec!["Wick the Lamplighter", "Deacon Maud"]);
        assert_eq!(
            stats_line(&info),
            "Runs 7 · Deaths 2 · Rescued 2/4 · Points 33"
        );
        let cage = d.endgame.ending("cage").unwrap();
        let lines = ending_lines(cage, &info);
        assert_eq!(lines.len(), 4);
        assert!(lines[1].starts_with("Far above, the Last Lantern blazes white"));
        assert_eq!(lines[2], "Those you brought up keep the watch with you: Wick the Lamplighter and Deacon Maud. Lamps trimmed, doors barred, eyes on the stairs.");
        let night = d.endgame.ending("night").unwrap();
        assert_eq!(ending_lines(night, &info)[2], "You walk out through the long night with Wick the Lamplighter and Deacon Maud at your side. They know the way; they always did.");
        // the altar flow
        let mut scr = EndingScreen::default();
        assert!(!open_choice(&mut scr, false, 0.0, 0.0, "source").0);
        let (ok, ev) = open_choice(&mut scr, true, 1.0, 1.0, "source");
        assert!(ok && scr.screen == Some(Screen::Choice));
        assert_eq!(names(&ev), vec!["altar", "uiClick"]);
        let (ok, ev) = choose_ending(&d, &mut save, &mut scr, 2, "dawn");
        assert!(!ok);
        assert_eq!(
            ev[0],
            SimEvent::toast("Kindle a new flame: Needs flame tier 4 (now 2) and 3 rescued (now 2)")
        );
        assert!(cancel_choice(&mut scr).0 && scr.screen.is_none());
        assert!(!cancel_choice(&mut scr).0);
        open_choice(&mut scr, true, 1.0, 1.0, "source");
        let (ok, ev) = choose_ending(&d, &mut save, &mut scr, 4, "night");
        assert!(ok && save.endings.night && scr.screen == Some(Screen::End));
        assert_eq!(
            ev,
            vec![SimEvent::Ending {
                id: "night".into(),
                choice: "Free the dark".into(),
                title: "The Long Night".into(),
                tier: 4,
                rescued: 2,
                first: true
            }]
        );
        let mut carried = Carried {
            oil: 1,
            ..Default::default()
        };
        let (id, ev) = continue_to_hub(&mut scr, &mut carried, "source");
        assert_eq!(id.as_deref(), Some("night"));
        assert!(carried.is_empty() && scr.screen.is_none());
        assert_eq!(
            names(&ev),
            vec!["zoneExit", "toast", "endingContinue", "uiClick"]
        );
        assert_eq!(
            ev[1],
            SimEvent::toast("The Lantern gutters. Only embers remain.")
        );
        assert_eq!(begin_night_visit(), vec![ev[1].clone()]);
        assert_eq!(endings_hud_line(&save, true), "Endings: ✧✧✦");
        assert_eq!(endings_hud_line(&save, false), "");
        assert_eq!(endings_hud_line(&SaveData::default(), true), "");
        // a second time is not `first`
        open_choice(&mut scr, true, 1.0, 1.0, "source");
        let (_, ev) = choose_ending(&d, &mut save, &mut scr, 4, "night");
        assert!(matches!(ev[0], SimEvent::Ending { first: false, .. }));
    }

    #[test]
    fn source_laps_stage_the_hunters() {
        let d = data();
        let cfg = &d.config;
        let zone = d.zone("source").unwrap();
        let map = d.parse_zone("source").unwrap().unwrap();
        // the mapped hunters, as the creature lane would list them
        let hunters: Vec<(u32, f32, f32, &str)> = map
            .hunter_spawns
            .iter()
            .zip(zone.hunter_profile_names(cfg))
            .enumerate()
            .map(|(i, (h, prof))| (i as u32, h.x, h.z, prof))
            .collect();
        let mut run = start_source_run(cfg, &map, &hunters);
        let deep: Vec<i32> = hunters
            .iter()
            .map(|&(_, x, z, _)| {
                let (cx, cz) = grid::to_cell(&map, x, z);
                grid::lap_of_map(&map, cx, cz)
            })
            .collect();
        let expect: Vec<Dormant> = hunters
            .iter()
            .zip(&deep)
            .filter(|(_, &l)| l - cfg.endgame.wake_lap_ahead > 0)
            .map(|(&(id, _, _, _), &l)| Dormant {
                id,
                wake_lap: l - cfg.endgame.wake_lap_ahead,
                counts_toward_max: true,
            })
            .collect();
        assert_eq!(run.dormant, expect);
        assert!(!run.dormant.is_empty(), "the Source's hunters spawn deep");
        // walk the laps from the entry
        let entry = zone.anchor("entry").unwrap();
        let (ex, ez) = grid::center(&map, entry[0], entry[1]);
        assert_eq!(update_source_run(&mut run, cfg, 0.1, true, 0), None);
        let mut rng = ScriptedRng::new(vec![0.0]);
        let mut total_spawn = 0;
        let mut woken = Vec::new();
        // a floor cell on each lap, where the player stands when crossing into it
        let on_lap = |lap: i32| -> (f32, f32) {
            let i = (0..map.len())
                .find(|&i| {
                    let (cx, cz) = map.cell_of(i);
                    matches!(map.cells[i], CellKind::Floor | CellKind::Deep)
                        && grid::lap_of_map(&map, cx, cz) == lap
                })
                .expect("a floor cell on the lap");
            grid::center_of(&map, i)
        };
        for lap in 1..=5 {
            let crossed = update_source_run(&mut run, cfg, 0.1, true, lap);
            assert_eq!(crossed, Some((lap, lap - 1)));
            // the base/fast hunters active before this lap: everything woken or spawned so far
            let active = (woken.len() + total_spawn) as u32;
            let out = on_deeper(
                &d,
                &mut run,
                lap,
                lap - 1,
                "source",
                &map,
                on_lap(lap),
                active,
                &mut rng,
            );
            assert_eq!(
                out.events[0],
                SimEvent::Lap {
                    lap,
                    prev: lap - 1,
                    zone_id: "source".into()
                }
            );
            assert_eq!(
                out.events[1],
                SimEvent::toast(d.endgame.lap_lines[lap as usize].clone().unwrap())
            );
            woken.extend(out.woken.iter().copied());
            for w in &out.woken {
                assert!(out.events.contains(&SimEvent::HunterWoken { id: *w, lap }));
            }
            if cfg.endgame.extra_laps.contains(&lap) {
                assert_eq!(out.spawn.len(), 1, "lap {lap} adds one fast hunter");
                assert_eq!(out.spawn[0].profile, "fast");
                assert_eq!(out.spawn[0].lap, lap);
            } else {
                assert!(out.spawn.is_empty());
            }
            total_spawn += out.spawn.len();
        }
        assert_eq!(total_spawn, 2);
        assert_eq!(run.extras, 2);
        assert!(run.dormant.is_empty(), "everything woke by the chamber");
        assert_eq!(woken.len(), expect.len());
        // no re-announce, no re-spawn, max hunters honoured
        let out = on_deeper(&d, &mut run, 5, 4, "source", &map, (ex, ez), 4, &mut rng);
        assert_eq!(names(&out.events), vec!["lap"]);
        assert!(out.spawn.is_empty());
        let mut fresh = SourceRun::default();
        let out = on_deeper(&d, &mut fresh, 3, 2, "source", &map, (ex, ez), 4, &mut rng);
        assert!(out.spawn.is_empty(), "maxHunters reached: none");
        assert_eq!(fresh.spawned_laps, vec![3]);
        // a hunter woken in the same call counts toward maxHunters (the JS recounts after waking) …
        let mut staged = SourceRun {
            dormant: vec![Dormant {
                id: 9,
                wake_lap: 3,
                counts_toward_max: true,
            }],
            ..Default::default()
        };
        let out = on_deeper(
            &d,
            &mut staged,
            3,
            2,
            "source",
            &map,
            on_lap(3),
            3,
            &mut rng,
        );
        assert_eq!(out.woken, vec![9]);
        assert!(
            out.spawn.is_empty(),
            "3 active + the woken hunter = maxHunters"
        );
        // … but a woken creature does not
        let mut staged = SourceRun {
            dormant: vec![Dormant {
                id: 9,
                wake_lap: 3,
                counts_toward_max: false,
            }],
            ..Default::default()
        };
        let out = on_deeper(
            &d,
            &mut staged,
            3,
            2,
            "source",
            &map,
            on_lap(3),
            3,
            &mut rng,
        );
        assert_eq!(out.woken, vec![9]);
        assert_eq!(out.spawn.len(), 1);
        let creature_run = start_source_run(cfg, &map, &[(7, ex, ez, "warden")]);
        assert!(creature_run.dormant.iter().all(|d| !d.counts_toward_max));
        // the spawn window: 12–34 BFS cells, this lap or the next, floor/deep
        let f = grid::bfs_solid(&map, entry[0], entry[1]);
        let cell =
            pick_spawn_cell(cfg, &map, (ex, ez), 0, &mut rng).expect("a cell near the entry");
        let di = f.dist_to(grid::idx(&map, cell.0, cell.1)).unwrap() as i32;
        assert!((cfg.endgame.spawn_min_cells..=cfg.endgame.spawn_max_cells).contains(&di));
        let l = grid::lap_of_map(&map, cell.0, cell.1);
        assert!(l == 0 || l == 1);
        assert!(grid::in_bounds(&map, cell.0, cell.1));
        assert_eq!(pick_spawn_cell(cfg, &map, (-5.0, -5.0), 0, &mut rng), None);
        // tint numbers
        let (fog, k, amb) = lap_tint(cfg, 5.0);
        assert!((fog - 1.5).abs() < 1e-6 && (k - 0.6).abs() < 1e-6 && (amb - 0.35).abs() < 1e-6);
        assert_eq!(lap_tint(cfg, 10.0).2, 0.25);
        // altar reach
        let a = map.altar.expect("altar");
        assert!(altar_in_reach(cfg, &map, a.x + 1.0, a.z));
        assert!(!altar_in_reach(cfg, &map, a.x + 2.0, a.z));
    }

    /// `endgame.js:inSource` — the Source is one, the ordinary zones are not.
    #[test]
    fn in_source_is_the_endgame_zone() {
        let d = data();
        let src = d.zone("source").expect("source zone");
        let smap = d.parse_zone("source").unwrap().unwrap();
        assert!(in_source(src, &smap));
        let u = d.zone("undercroft").expect("undercroft zone");
        let umap = d.parse_zone("undercroft").unwrap().unwrap();
        assert!(!in_source(u, &umap));
        // an altar on the map is enough on its own (`c.zone.altar`)
        let mut fake = u.clone();
        fake.no_bank = false;
        assert!(in_source(&fake, &smap));
    }

    #[test]
    fn ride_up_confirms_when_carrying() {
        let d = data();
        let cfg = &d.config;
        let mut run = SourceRun::default();
        let mut carried = Carried {
            relic: 2,
            ..Default::default()
        };
        assert!(!ride_up(cfg, &mut run, &mut carried, false, "source").0);
        let (ok, ev) = ride_up(cfg, &mut run, &mut carried, true, "source");
        assert!(ok);
        assert_eq!(run.ride_confirm_t, 3.0);
        assert_eq!(names(&ev), vec!["toast", "uiError"]);
        assert_eq!(carried.relic, 2, "still carried");
        update_source_run(&mut run, cfg, 1.0, true, 0);
        assert_eq!(run.ride_confirm_t, 2.0);
        let (ok, ev) = ride_up(cfg, &mut run, &mut carried, true, "source");
        assert!(ok && carried.is_empty());
        assert_eq!(
            ev[0],
            SimEvent::SourceAbandoned {
                carried: Carried {
                    relic: 2,
                    ..Default::default()
                },
                zone_id: "source".into()
            }
        );
        assert_eq!(
            ev[1],
            SimEvent::toast("You ride up out of the Source. 2 relics stays below.")
        );
        assert_eq!(
            ev[2],
            SimEvent::ZoneExit {
                zone_id: "source".into()
            }
        );
        // empty-handed: straight out
        let (ok, ev) = ride_up(cfg, &mut run, &mut carried, true, "source");
        assert!(ok);
        assert_eq!(
            ev[1],
            SimEvent::toast("You ride up out of the Source. The altar keeps its light.")
        );
        // the confirmation expires
        carried.oil = 1;
        ride_up(cfg, &mut run, &mut carried, true, "source");
        update_source_run(&mut run, cfg, 5.0, false, 0);
        assert_eq!(run.ride_confirm_t, 0.0);
        let (_, ev) = ride_up(cfg, &mut run, &mut carried, true, "source");
        assert_eq!(names(&ev), vec!["toast", "uiError"], "armed again, not out");
    }

    #[test]
    fn lamp_snuff_burns_oil_and_locks_the_wick() {
        let d = data();
        let cfg = &d.config;
        let mut lamp = LampState {
            oil: 20.0,
            lamp_on: true,
            ..Default::default()
        };
        // hub: nothing happens
        on_lamp_snuffed(&mut lamp, cfg, None, None, false);
        assert!(lamp.lamp_on && lamp.oil == 20.0 && lamp.lamp_lock == 0.0);
        on_lamp_snuffed(&mut lamp, cfg, None, None, true);
        assert!(!lamp.lamp_on);
        assert_eq!(lamp.oil, 8.0);
        assert_eq!(lamp.lamp_lock, 2.0);
        let (ok, ev) = toggle_lamp(&mut lamp, true);
        assert!(!ok);
        assert_eq!(
            ev,
            vec![SimEvent::toast("The wick is cold"), SimEvent::ui_error()]
        );
        // the payload overrides the defaults; a shorter lockout never shortens the running one; oil floors at 0
        on_lamp_snuffed(&mut lamp, cfg, Some(10.0), Some(1.0), true);
        assert_eq!(lamp.oil, 0.0);
        assert_eq!(lamp.lamp_lock, 2.0);
        lamp.lamp_lock = 0.0;
        lamp.oil = 5.0;
        on_lamp_snuffed(&mut lamp, cfg, Some(1.0), Some(3.5), true);
        assert_eq!((lamp.oil, lamp.lamp_lock), (4.0, 3.5));
    }

    #[test]
    fn hub_guidance_toasts() {
        let d = data();
        let mut save = SaveData::default();
        assert_eq!(
            on_npc_rescued_hub(&d, &save, "lamplighter"),
            vec![SimEvent::toast(
                "Wick the Lamplighter could raise a Workshop at the Lantern."
            )]
        );
        save.buildings.workshop = true;
        assert!(on_npc_rescued_hub(&d, &save, "lamplighter").is_empty());
        assert!(on_npc_rescued_hub(&d, &save, "nobody").is_empty());
        // tier-gated ghosts announce themselves once, with their cost, in BUILD_ORDER
        assert_eq!(
            on_flame_tier_hub(&d, &save, 1, 2),
            vec![SimEvent::toast(
                "The flame is strong enough for a Tram dock (120 oil)."
            )]
        );
        assert_eq!(
            on_flame_tier_hub(&d, &save, 1, 3),
            vec![
                SimEvent::toast("The flame is strong enough for a Tram dock (120 oil)."),
                SimEvent::toast("The flame is strong enough for a Elevator (250 oil + 4 relics).")
            ]
        );
        assert_eq!(
            on_flame_tier_hub(&d, &save, 2, 3),
            vec![SimEvent::toast(
                "The flame is strong enough for a Elevator (250 oil + 4 relics)."
            )]
        );
        assert!(on_flame_tier_hub(&d, &save, 2, 2).is_empty());
        save.buildings.tram = true;
        assert!(
            on_flame_tier_hub(&d, &save, 1, 2).is_empty(),
            "already built"
        );
    }

    #[test]
    fn hub_interact_target_reach_rules() {
        let d = data();
        let m = d.parse_hub().expect("hub");
        let mut save = SaveData::default();
        let hub = HubState::default();
        // the board (anchor 5 → (75.5, 9.5)) is always built: strictly inside 1.8
        assert_eq!(
            hub_interact_target(&d, &save, &hub, &m, 74.0, 9.5),
            Some(HubInteract::Building {
                id: "board".into(),
                label: "[E] Departure Board".into()
            })
        );
        assert_eq!(
            hub_interact_target(&d, &save, &hub, &m, 73.7, 9.5),
            None,
            "1.8 is not < 1.8, and the stairs are out of reach"
        );
        // the stairs (S → (70.5, 10.5)) within CFG.interactR 1.6, inclusive
        assert_eq!(
            hub_interact_target(&d, &save, &hub, &m, 69.0, 10.5),
            Some(HubInteract::Descend {
                zone_id: "undercroft".into(),
                label: "[E] Descend — The Undercroft".into(),
                lock: None
            })
        );
        assert_eq!(hub_interact_target(&d, &save, &hub, &m, 68.8, 10.5), None);
        save.zone_selected = "cistern".into();
        assert_eq!(
            hub_interact_target(&d, &save, &hub, &m, 70.5, 10.5),
            Some(HubInteract::Descend {
                zone_id: "cistern".into(),
                label: "[E] Descend — The Cistern (locked: Needs the Tram dock)".into(),
                lock: Some("Needs the Tram dock".into())
            })
        );
        // the workshop (anchor 1 → (63.5, 1.5)) is 'none' until Wick is rescued, then a ghost, then built
        assert_eq!(hub_interact_target(&d, &save, &hub, &m, 63.5, 1.5), None);
        save.rescued.lamplighter = true;
        assert_eq!(
            hub_interact_target(&d, &save, &hub, &m, 63.5, 1.5),
            Some(HubInteract::Build {
                id: "workshop".into(),
                label: "[E] Build Workshop — 6 relics".into()
            })
        );
        save.buildings.workshop = true;
        assert_eq!(
            hub_interact_target(&d, &save, &hub, &m, 63.5, 1.5),
            Some(HubInteract::Building {
                id: "workshop".into(),
                label: "[E] Workshop".into()
            })
        );
        // the tram ghost (anchor 6 → (62.5, 9.5)) needs tier 2
        assert_eq!(hub_interact_target(&d, &save, &hub, &m, 62.5, 9.5), None);
        let hub2 = HubState {
            tier: 2,
            blessed: false,
        };
        assert!(matches!(
            hub_interact_target(&d, &save, &hub2, &m, 62.5, 9.5),
            Some(HubInteract::Build { id, .. }) if id == "tram"
        ));
    }

    #[test]
    fn explore_tick_marks_lit_cells_and_their_walls() {
        use crate::save::bit_set;
        use undercroft_data::map::parse_map;
        let d = data();
        let cfg = &d.config;
        let rows: Vec<String> = ["#####", "#...#", "#.P.#", "#...#", "#####"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m = parse_map(&rows, 0, "t", None, None).expect("parse");
        let mut bits = SaveData::default().explored_bits("t", m.len());
        assert_eq!(explored_pct(&m, &bits), 0);
        // dark at (1,1): radius 2 → (1,1) (2,1) (3,1) (1,2) (1,3), the pillar and 13 walls around them
        assert!(explore_tick(cfg, &m, &mut bits, 1.5, 1.5, false, 16.0));
        let expect = [
            (1, 1),
            (2, 1),
            (3, 1),
            (1, 2),
            (1, 3),
            (2, 2),
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (0, 1),
            (4, 1),
            (0, 2),
            (4, 2),
            (0, 3),
            (0, 4),
            (1, 4),
            (2, 4),
        ];
        for cz in 0..5 {
            for cx in 0..5 {
                assert_eq!(
                    bit_set(&bits, m.idx(cx, cz)),
                    expect.contains(&(cx, cz)),
                    "cell ({cx},{cz})"
                );
            }
        }
        // 6 of the 9 non-wall cells (8 floor + the pillar)
        assert_eq!(explored_pct(&m, &bits), 67);
        // lamp on but the pillar blocks the diagonal cells: nothing new
        assert!(!explore_tick(cfg, &m, &mut bits, 1.5, 1.5, true, 2.3));
        assert!(!bit_set(&bits, m.idx(3, 2)) && !bit_set(&bits, m.idx(3, 3)));
        // from the opposite corner everything is seen
        assert!(explore_tick(cfg, &m, &mut bits, 3.5, 3.5, false, 16.0));
        assert_eq!(explored_pct(&m, &bits), 100);
        assert!(bit_set(&bits, m.idx(4, 4)), "the corner wall beside (3,3)");
        // the bitset round-trips through the save
        let mut save = SaveData::default();
        save.set_explored_bits("t", &bits);
        assert_eq!(save.explored_bits("t", m.len()), bits);
        // a lit lamp reaches min(exploreMaxR, reach); out of the grid nothing is marked
        let mut none = vec![0u8; bits.len()];
        assert!(!explore_tick(cfg, &m, &mut none, -9.0, -9.0, true, 1.0));
        assert!(none.iter().all(|&b| b == 0));
    }
}
