//! LANE: economy. Contract state machine — `contracts.js` (DESIGN-v2 §4, DESIGN.md §7).
//!
//! Each rescued NPC posts one contract at a time (`ContractTable::order`; done → next); up to
//! `CONTRACT_CFG.maxActive` active. Per contract: `locked` (poster not rescued) → `available` → `active` → `done`.
//! Progress lives in `save.contracts` ([`crate::save::ContractSave`]); a death, an early bank or leaving the
//! zone resets run-scoped progress (fetch / survive / recover) but never removes the contract — reported as
//! `contractFailed {id, reason}`.
//!
//! The JS listeners (`pickup`, `bank`, `lantern`, `death`, `zoneEnter`, `zoneExit`, `npcRescued`) are the
//! `on_*` functions here, `update()` is [`tick`]; every one takes the [`ContractEnv`] (tables + where the player
//! is), the save and the hub tier, and returns the events the JS emitted (toasts included). Quest items for
//! `recover` contracts are world objects: [`quests_to_spawn`] says what to place, the shell places it.

use crate::economy::{self, HubState};
use crate::events::{RewardPayload, SimEvent};
use crate::grid::{center, dist2d};
use crate::player::{Carried, PlayerView};
use crate::save::SaveData;
use undercroft_data::config::Config;
use undercroft_data::tables::{ContractDef, ContractKind, ItemKind, Reward};
use undercroft_data::zone::EntryKind;
use undercroft_data::{GameData, ParsedMap};

/// Where the player is, as `contracts.js` reads it off `ctx` (`ctx.state.mode`, `ctx.zone`).
#[derive(Debug, Clone, Copy)]
pub struct ContractEnv<'a> {
    pub data: &'a GameData,
    /// `ctx.zone.id` — the loaded zone (also set in the hub/title when a zone stays loaded).
    pub zone: Option<&'a str>,
    /// `ctx.zone.map` — the parsed map of that zone (spot positions come from it when present).
    pub map: Option<&'a ParsedMap>,
    /// `ctx.state.mode === 'ZONE'`.
    pub in_zone_mode: bool,
}

impl<'a> ContractEnv<'a> {
    /// Outside any zone (hub / title): no zone, no map.
    pub fn hub(data: &'a GameData) -> ContractEnv<'a> {
        ContractEnv {
            data,
            zone: None,
            map: None,
            in_zone_mode: false,
        }
    }

    /// In a zone run.
    pub fn zone(data: &'a GameData, zone: &'a str, map: &'a ParsedMap) -> ContractEnv<'a> {
        ContractEnv {
            data,
            zone: Some(zone),
            map: Some(map),
            in_zone_mode: true,
        }
    }

    fn def(&self, id: &str) -> Option<&'a ContractDef> {
        self.data.contracts.contracts.get(id)
    }

    /// `contracts.js:inZone(c)` — in ZONE mode and the loaded zone is the contract's.
    fn in_zone(&self, c: &ContractDef) -> bool {
        self.in_zone_mode && self.zone == Some(c.zone.as_str())
    }

    /// `contracts.js:exitName` — "cage" for an elevator zone, else "stairs".
    fn exit_name(&self) -> &'static str {
        match self.map.and_then(|m| m.stairs.as_ref()) {
            Some(s) if s.kind == EntryKind::Elevator => "cage",
            _ => "stairs",
        }
    }
}

/// `contracts.js:status(id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractStatus {
    /// Poster not rescued.
    Locked,
    /// The poster's next contract.
    Available,
    /// Rescued, but not next in the poster's list.
    Queued,
    Active,
    Done,
}

/// `contracts.js:available(npcId)` — the offer an NPC makes in dialogue.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub id: String,
    pub title: String,
    pub text: String,
    pub objective: String,
    pub reward: Reward,
    pub reward_text: String,
    pub zone: String,
    pub kind: ContractKind,
}

/// `contracts.js:targets()` entry — an active contract's spot for the board / minimap.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub id: String,
    pub title: String,
    pub zone: String,
    pub kind: ContractKind,
    /// The spot cell, when the contract has one.
    pub cell: Option<[i32; 2]>,
    /// The spot's world position (zone maps sit at `ox` 0).
    pub x: Option<f32>,
    pub z: Option<f32>,
    pub progress: f64,
    pub goal: u32,
}

/// `contracts.js:list()` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ListEntry {
    pub id: String,
    pub title: String,
    pub poster: String,
    pub zone: String,
    pub kind: ContractKind,
    pub status: ContractStatus,
    pub progress: f64,
    pub goal: u32,
}

/// What `contracts.js:spawnQuest` checks against the world before placing a quest item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuestWorld {
    /// Contract ids of the quest items lying in the zone (`it.kind === 'quest' && it.questId`).
    pub quest_items: Vec<String>,
    /// A death bundle in the zone holds a quest item.
    pub bundle_holds_quest: bool,
    /// `ctx.player.carried.quest`.
    pub carried_quest: u32,
}

/// A quest item to place (`contracts.js:spawnQuest`): a `quest` item at the spot, `questId = contract`, labelled
/// with the contract's `item` name.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestSpawn {
    pub contract_id: String,
    pub label: String,
    pub x: f32,
    pub z: f32,
}

/* ============================================================
Helpers
============================================================ */

/// `contracts.js:goalOf(c)` — `n` for fetch, `seconds` for survive, 1 otherwise.
pub fn goal_of(c: &ContractDef) -> u32 {
    match c.kind {
        ContractKind::Fetch => c.n.unwrap_or(1),
        ContractKind::Survive => c.seconds.unwrap_or(0.0) as u32,
        _ => 1,
    }
}

/// `contracts.js:spotPos(id)` — world position of a contract's spot: the loaded zone's parsed map when it is
/// that zone, else the zone file's `spots[i].cell` centre. `None` for fetch contracts and unknown ids.
pub fn spot_pos(env: &ContractEnv, id: &str) -> Option<(f32, f32)> {
    let c = env.def(id)?;
    let spot = c.spot? as usize;
    if env.zone == Some(c.zone.as_str()) {
        if let Some(m) = env.map {
            if let Some(s) = m.spots.get(spot) {
                return Some(center(m, s.marker.cx, s.marker.cz));
            }
        }
    }
    let d = env.data.zone(&c.zone)?.spots.get(spot)?;
    Some((d.cell[0] as f32 + 0.5, d.cell[1] as f32 + 0.5))
}

/// `contracts.js:rewardText(r)` — "Pry Bar, 4 flame points" / "60 oil" / "nothing".
pub fn reward_text(cfg: &Config, r: &Reward) -> String {
    let mut parts = Vec::new();
    if let Some(t) = &r.tool {
        parts.push(cfg.tools.get(t).cloned().unwrap_or_else(|| t.clone()));
    }
    if r.pts > 0 {
        parts.push(format!(
            "{} flame point{}",
            r.pts,
            if r.pts == 1 { "" } else { "s" }
        ));
    }
    if r.oil > 0 {
        parts.push(format!("{} oil", r.oil));
    }
    if parts.is_empty() {
        "nothing".to_string()
    } else {
        parts.join(", ")
    }
}

/// `contracts.js:kindLabel(kind, n)` — `LABEL[kind]`, pluralised.
fn kind_label(cfg: &Config, kind: ItemKind, n: u32) -> String {
    let base = match kind {
        ItemKind::Oil => &cfg.label.oil,
        ItemKind::Relic => &cfg.label.relic,
        ItemKind::Rich => &cfg.label.rich,
        ItemKind::Bundle => &cfg.label.bundle,
        ItemKind::Quest => &cfg.label.quest,
    };
    if n == 1 {
        base.clone()
    } else {
        format!("{base}s")
    }
}

fn zone_name<'a>(data: &'a GameData, id: &'a str) -> &'a str {
    economy::zone_name(data, id)
}

/// `contracts.js:objectiveText(id)` — the one-line objective.
pub fn objective_text(data: &GameData, id: &str) -> String {
    let c = match data.contracts.contracts.get(id) {
        Some(c) => c,
        None => return String::new(),
    };
    let spot = c
        .spot
        .and_then(|s| data.zone(&c.zone).and_then(|z| z.spots.get(s as usize)))
        .map(|s| s.label.as_str())
        .unwrap_or("the spot");
    let zone = zone_name(data, &c.zone);
    match c.kind {
        ContractKind::Plant => format!("Plant a lantern in {spot} of {zone}."),
        ContractKind::Fetch => {
            let n = c.n.unwrap_or(1);
            let kind = c.item_kind.unwrap_or(ItemKind::Oil);
            format!(
                "Bank {n} {} from {zone} in one run.",
                kind_label(&data.config, kind, n)
            )
        }
        ContractKind::Recover => format!(
            "Recover the {} from {spot} of {zone} and bank it.",
            c.item.as_deref().unwrap_or("item")
        ),
        ContractKind::Survive => format!(
            "Stay {} s in {spot} of {zone}{}.",
            c.seconds.unwrap_or(0.0),
            if c.lamp_off {
                " with the lamp doused"
            } else {
                ""
            }
        ),
    }
}

/* ============================================================
State machine
============================================================ */

/// `contracts.js:progress(id)`.
pub fn progress(save: &SaveData, id: &str) -> f64 {
    save.contracts.progress.get(id).copied().unwrap_or(0.0)
}

/// `contracts.js:status(id)` — `None` for an unknown id.
pub fn status(data: &GameData, save: &SaveData, id: &str) -> Option<ContractStatus> {
    let c = data.contracts.contracts.get(id)?;
    let s = &save.contracts;
    if s.done.iter().any(|d| d == id) {
        return Some(ContractStatus::Done);
    }
    if s.active.iter().any(|a| a == id) {
        return Some(ContractStatus::Active);
    }
    if !save.rescued.get(&c.poster) {
        return Some(ContractStatus::Locked);
    }
    match available(data, save, &c.poster) {
        Some(o) if o.id == id => Some(ContractStatus::Available),
        _ => Some(ContractStatus::Queued),
    }
}

/// `contracts.js:available(npcId)` — the next contract this NPC can post, or `None` (not rescued, one already
/// active for this NPC, or list exhausted).
pub fn available(data: &GameData, save: &SaveData, npc: &str) -> Option<Offer> {
    if !save.rescued.get(npc) {
        return None;
    }
    let s = &save.contracts;
    for id in &data.contracts.order {
        let c = data.contracts.contracts.get(id)?;
        if c.poster != npc || s.done.iter().any(|d| d == id) {
            continue;
        }
        if s.active.iter().any(|a| a == id) {
            return None;
        }
        return Some(Offer {
            id: id.clone(),
            title: c.title.clone(),
            text: c.text.clone(),
            objective: objective_text(data, id),
            reward: c.reward.clone(),
            reward_text: reward_text(&data.config, &c.reward),
            zone: c.zone.clone(),
            kind: c.kind,
        });
    }
    None
}

fn payload(r: &Reward) -> RewardPayload {
    RewardPayload {
        tool: r.tool.clone(),
        pts: r.pts,
        oil: r.oil,
    }
}

/// `contracts.js:accept(id, {force})` — true when accepted (poster rescued, next in its list, fewer than
/// `maxActive` active). Emits `contractAccepted`, `uiClick` and the toast; refused with a toast + `uiError`
/// when the player already carries `maxActive`. For a `recover` contract accepted inside its zone the shell
/// should then place its quest item ([`quests_to_spawn`]).
pub fn accept(
    env: &ContractEnv,
    save: &mut SaveData,
    id: &str,
    force: bool,
) -> (bool, Vec<SimEvent>) {
    let c = match env.def(id) {
        Some(c) => c,
        None => return (false, vec![]),
    };
    let s = &save.contracts;
    if s.done.iter().any(|d| d == id) || s.active.iter().any(|a| a == id) {
        return (false, vec![]);
    }
    if !force {
        match available(env.data, save, &c.poster) {
            Some(o) if o.id == id => {}
            _ => return (false, vec![]),
        }
    }
    if s.active.len() as u32 >= env.data.contracts.cfg.max_active {
        return (
            false,
            vec![
                SimEvent::toast("You already carry enough contracts."),
                SimEvent::ui_error(),
            ],
        );
    }
    save.contracts.active.push(id.to_string());
    save.contracts.progress.insert(id.to_string(), 0.0);
    (
        true,
        vec![
            SimEvent::ContractAccepted {
                id: id.to_string(),
                reward: payload(&c.reward),
            },
            SimEvent::UiClick,
            SimEvent::toast(format!("Contract accepted: {}", c.title)),
        ],
    )
}

/// `contracts.js:abandon(id)` — drop an active contract; it becomes available again from its poster.
pub fn abandon(save: &mut SaveData, id: &str) -> (bool, Vec<SimEvent>) {
    let s = &mut save.contracts;
    let i = match s.active.iter().position(|a| a == id) {
        Some(i) => i,
        None => return (false, vec![]),
    };
    s.active.remove(i);
    s.progress.remove(id);
    (
        true,
        vec![SimEvent::ContractFailed {
            id: id.to_string(),
            reason: "abandoned".to_string(),
            progress: None,
            goal: None,
        }],
    )
}

/// `contracts.js:grant(c)` — points (re-tiering the flame), oil, the tool (`toolGained` once).
fn grant(
    env: &ContractEnv,
    save: &mut SaveData,
    hub: &mut HubState,
    c: &ContractDef,
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    let r = &c.reward;
    if r.pts > 0 {
        let n = save.points + r.pts;
        out.extend(economy::set_points(&env.data.config, save, hub, n));
    }
    if r.oil > 0 {
        save.oil += r.oil;
    }
    if let Some(t) = &r.tool {
        if crate::save::Tools::known(t) {
            out.extend(economy::give_tool(save, t).1);
        } else {
            // the JS writes the unknown key and emits toolGained anyway
            out.push(SimEvent::ToolGained { id: t.clone() });
        }
    }
    out
}

/// `contracts.js:complete(id)` — active → done, progress = goal, the reward, the toast, `contractComplete`.
fn complete(env: &ContractEnv, save: &mut SaveData, hub: &mut HubState, id: &str) -> Vec<SimEvent> {
    let c = match env.def(id) {
        Some(c) => c,
        None => return vec![],
    };
    let i = match save.contracts.active.iter().position(|a| a == id) {
        Some(i) => i,
        None => return vec![],
    };
    save.contracts.active.remove(i);
    if !save.contracts.done.iter().any(|d| d == id) {
        save.contracts.done.push(id.to_string());
    }
    save.contracts
        .progress
        .insert(id.to_string(), goal_of(c) as f64);
    let mut out = grant(env, save, hub, c);
    out.push(SimEvent::toast(format!(
        "Contract complete: {} — {}",
        c.title,
        reward_text(&env.data.config, &c.reward)
    )));
    out.push(SimEvent::ContractComplete {
        id: id.to_string(),
        reward: payload(&c.reward),
    });
    out
}

/// `contracts.js:setProgress(id, v)` — store and emit `contractProgress` when it changed.
fn set_progress(env: &ContractEnv, save: &mut SaveData, id: &str, v: f64) -> Vec<SimEvent> {
    let old = progress(save, id);
    if old == v {
        return vec![];
    }
    save.contracts.progress.insert(id.to_string(), v);
    let goal = env.def(id).map(goal_of).unwrap_or(1);
    vec![SimEvent::ContractProgress {
        id: id.to_string(),
        progress: v.floor() as u32,
        goal,
    }]
}

/// `contracts.js:resetRun(reason, zoneId)` — reset run-scoped progress (never `plant`; only that zone's when
/// given). Emits `contractFailed` for each contract that had progress, plus the toast on death.
pub fn reset_run(
    env: &ContractEnv,
    save: &mut SaveData,
    reason: &str,
    zone_id: Option<&str>,
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    for id in save.contracts.active.clone() {
        let c = match env.def(&id) {
            Some(c) => c,
            None => continue,
        };
        if c.kind == ContractKind::Plant {
            continue;
        }
        if let Some(z) = zone_id {
            if c.zone != z {
                continue;
            }
        }
        let had = progress(save, &id);
        if had > 0.0 {
            save.contracts.progress.insert(id.clone(), 0.0);
            out.push(SimEvent::ContractFailed {
                id: id.clone(),
                reason: reason.to_string(),
                progress: Some(had.floor() as u32),
                goal: Some(goal_of(c)),
            });
            if reason == "death" {
                out.push(SimEvent::toast(format!(
                    "Contract progress lost: {}",
                    c.title
                )));
            }
        }
    }
    out
}

/* ============================================================
Quest items (recover)
============================================================ */

/// `contracts.js:spawnQuest(c)` for one contract: the item to place unless one already lies there, a bundle
/// holds it, or the player carries it.
pub fn quest_to_spawn(env: &ContractEnv, id: &str, world: &QuestWorld) -> Option<QuestSpawn> {
    let c = env.def(id)?;
    if c.kind != ContractKind::Recover {
        return None;
    }
    if world.quest_items.iter().any(|q| q == id)
        || world.bundle_holds_quest
        || world.carried_quest > 0
    {
        return None;
    }
    let (x, z) = spot_pos(env, id)?;
    Some(QuestSpawn {
        contract_id: id.to_string(),
        label: c.item.clone().unwrap_or_default(),
        x,
        z,
    })
}

/// `contracts.js:spawnQuestsFor(zoneId)` — quest items for every active `recover` contract of `zone_id`.
pub fn quests_to_spawn(
    env: &ContractEnv,
    save: &SaveData,
    zone_id: &str,
    world: &QuestWorld,
) -> Vec<QuestSpawn> {
    save.contracts
        .active
        .iter()
        .filter(|id| {
            env.def(id)
                .map(|c| c.kind == ContractKind::Recover && c.zone == zone_id)
                .unwrap_or(false)
        })
        .filter_map(|id| quest_to_spawn(env, id, world))
        .collect()
}

/* ============================================================
HUD / board
============================================================ */

/// `contracts.js:hudLines()` — "◇ Title — p/g" per active contract (" s" suffix for survive).
pub fn hud_lines(data: &GameData, save: &SaveData) -> Vec<String> {
    let mut out = Vec::new();
    for id in &save.contracts.active {
        let c = match data.contracts.contracts.get(id) {
            Some(c) => c,
            None => continue,
        };
        let g = goal_of(c);
        let p = (progress(save, id).floor() as u32).min(g);
        out.push(format!(
            "◇ {} — {p}/{g}{}",
            c.title,
            if c.kind == ContractKind::Survive {
                " s"
            } else {
                ""
            }
        ));
    }
    out
}

/// `contracts.js:targets(zoneId?)` — active contract targets for the board / minimap.
pub fn targets(data: &GameData, save: &SaveData, zone_id: Option<&str>) -> Vec<Target> {
    let mut out = Vec::new();
    for id in &save.contracts.active {
        let c = match data.contracts.contracts.get(id) {
            Some(c) => c,
            None => continue,
        };
        if let Some(z) = zone_id {
            if c.zone != z {
                continue;
            }
        }
        let cell = c
            .spot
            .and_then(|s| data.zone(&c.zone).and_then(|z| z.spots.get(s as usize)))
            .map(|s| s.cell);
        out.push(Target {
            id: id.clone(),
            title: c.title.clone(),
            zone: c.zone.clone(),
            kind: c.kind,
            cell,
            x: cell.map(|c| c[0] as f32 + 0.5),
            z: cell.map(|c| c[1] as f32 + 0.5),
            progress: progress(save, id),
            goal: goal_of(c),
        });
    }
    out
}

/// `contracts.js:list()` — every contract with its status, in posting order.
pub fn list(data: &GameData, save: &SaveData) -> Vec<ListEntry> {
    data.contracts
        .order
        .iter()
        .filter_map(|id| {
            let c = data.contracts.contracts.get(id)?;
            Some(ListEntry {
                id: id.clone(),
                title: c.title.clone(),
                poster: c.poster.clone(),
                zone: c.zone.clone(),
                kind: c.kind,
                status: status(data, save, id)?,
                progress: progress(save, id),
                goal: goal_of(c),
            })
        })
        .collect()
}

/* ============================================================
Event handlers
============================================================ */

/// `contracts.js:onLantern({x, z})` — a lantern within `plantR` of an active `plant` contract's spot (in its
/// zone) completes it.
pub fn on_lantern(
    env: &ContractEnv,
    save: &mut SaveData,
    hub: &mut HubState,
    x: f32,
    z: f32,
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    let plant_r = env.data.contracts.cfg.plant_r;
    for id in save.contracts.active.clone() {
        let c = match env.def(&id) {
            Some(c) => c,
            None => continue,
        };
        if c.kind != ContractKind::Plant || !env.in_zone(c) {
            continue;
        }
        let (px, pz) = match spot_pos(env, &id) {
            Some(p) => p,
            None => continue,
        };
        if dist2d(x, z, px, pz) <= plant_r {
            out.extend(complete(env, save, hub, &id));
        }
    }
    out
}

/// `contracts.js:onPickup({kind, item})` — a quest item (`quest_id` = its contract) marks that contract 1/1 with
/// a "bring it to the stairs/cage" toast; a bundle holding a quest item marks every active `recover` contract
/// of the current zone.
pub fn on_pickup(
    env: &ContractEnv,
    save: &mut SaveData,
    kind: ItemKind,
    quest_id: Option<&str>,
    bundle_holds_quest: bool,
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    match kind {
        ItemKind::Quest => {
            if let Some(c) = quest_id.and_then(|q| env.def(q)) {
                out.push(SimEvent::toast(format!(
                    "You found the {}. Bring it to the {}.",
                    c.item.as_deref().unwrap_or("item"),
                    env.exit_name()
                )));
                out.extend(set_progress(env, save, &c.id, 1.0));
            }
        }
        ItemKind::Bundle if bundle_holds_quest => {
            for id in save.contracts.active.clone() {
                if let Some(c) = env.def(&id) {
                    if c.kind == ContractKind::Recover && env.in_zone(c) {
                        out.extend(set_progress(env, save, &id, 1.0));
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// `contracts.js:onBank({carried, zoneId})` — `fetch`: complete when `carried[kind] ≥ n`, else fail `short`
/// with a toast; `recover`: complete when a quest item is carried. Then the run's progress in that zone resets
/// (`bank`).
pub fn on_bank(
    env: &ContractEnv,
    save: &mut SaveData,
    hub: &mut HubState,
    carried: &Carried,
    zone_id: &str,
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    for id in save.contracts.active.clone() {
        let c = match env.def(&id) {
            Some(c) => c,
            None => continue,
        };
        if c.zone != zone_id {
            continue;
        }
        match c.kind {
            ContractKind::Fetch => {
                let n = c.n.unwrap_or(1);
                let have = c.item_kind.map(|k| carried.get(k)).unwrap_or(0);
                if have >= n {
                    out.extend(complete(env, save, hub, &id));
                } else if have > 0 {
                    save.contracts.progress.insert(id.clone(), 0.0);
                    out.push(SimEvent::ContractFailed {
                        id: id.clone(),
                        reason: "short".to_string(),
                        progress: Some(have),
                        goal: Some(n),
                    });
                    out.push(SimEvent::toast(format!(
                        "{}: {have}/{n} — not enough in one run.",
                        c.title
                    )));
                }
            }
            ContractKind::Recover if carried.quest > 0 => {
                out.extend(complete(env, save, hub, &id));
            }
            _ => {}
        }
    }
    out.extend(reset_run(env, save, "bank", Some(zone_id)));
    out
}

/// `contracts.js:onDeath()` — every run-scoped contract loses its progress.
pub fn on_death(env: &ContractEnv, save: &mut SaveData) -> Vec<SimEvent> {
    reset_run(env, save, "death", None)
}

/// `contracts.js:onZoneEnter({zoneId})` — reset that zone's run progress (`enter`); the shell then places the
/// zone's quest items ([`quests_to_spawn`]).
pub fn on_zone_enter(env: &ContractEnv, save: &mut SaveData, zone_id: &str) -> Vec<SimEvent> {
    reset_run(env, save, "enter", Some(zone_id))
}

/// `contracts.js:onZoneExit({zoneId})`.
pub fn on_zone_exit(env: &ContractEnv, save: &mut SaveData, zone_id: &str) -> Vec<SimEvent> {
    reset_run(env, save, "exit", Some(zone_id))
}

/// `contracts.js:onRescued({id})` — a toast naming the contract the rescued NPC now offers.
pub fn on_rescued(data: &GameData, save: &SaveData, id: &str) -> Vec<SimEvent> {
    match available(data, save, id) {
        Some(o) => vec![SimEvent::toast(format!(
            "{} — a contract from the Lantern. Talk to them at the flame.",
            o.title
        ))],
        None => vec![],
    }
}

/// `contracts.js:update(dt)` — per frame in ZONE mode (unpaused): `fetch` progress = carried of that kind,
/// `recover` = 1 while the quest item is carried, `survive` accumulates contiguous seconds within `spotR` of
/// the spot (lamp doused for `lampOff` ones) and completes at `seconds`; leaving resets the timer.
pub fn tick(
    env: &ContractEnv,
    save: &mut SaveData,
    hub: &mut HubState,
    dt: f32,
    player: &PlayerView,
    paused: bool,
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    let zone = match (env.in_zone_mode, paused, env.zone) {
        (true, false, Some(z)) => z,
        _ => return out,
    };
    let spot_r = env.data.contracts.cfg.spot_r;
    for id in save.contracts.active.clone() {
        let c = match env.def(&id) {
            Some(c) => c,
            None => continue,
        };
        if c.zone != zone {
            continue;
        }
        match c.kind {
            ContractKind::Fetch => {
                let have = c.item_kind.map(|k| player.carried.get(k)).unwrap_or(0);
                out.extend(set_progress(env, save, &id, have as f64));
            }
            ContractKind::Recover => {
                let v = if player.carried.quest > 0 { 1.0 } else { 0.0 };
                out.extend(set_progress(env, save, &id, v));
            }
            ContractKind::Survive => {
                let (sx, sz) = match spot_pos(env, &id) {
                    Some(p) => p,
                    None => continue,
                };
                let near = dist2d(player.x, player.z, sx, sz) <= spot_r;
                let lamp_ok = !c.lamp_off || !player.lamp_on;
                if near && lamp_ok {
                    let v = progress(save, &id) + dt as f64;
                    save.contracts.progress.insert(id.clone(), v);
                    if v >= c.seconds.unwrap_or(0.0) as f64 {
                        out.extend(complete(env, save, hub, &id));
                    }
                } else if progress(save, &id) > 0.0 {
                    save.contracts.progress.insert(id.clone(), 0.0);
                }
            }
            ContractKind::Plant => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::SaveData;
    use undercroft_data::GameData;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    fn names(ev: &[SimEvent]) -> Vec<&'static str> {
        ev.iter().map(SimEvent::name).collect()
    }

    fn all_rescued() -> SaveData {
        let mut s = SaveData::default();
        for id in crate::save::Rescued::IDS {
            s.rescued.set(id, true);
        }
        s
    }

    fn spot_of(data: &GameData, zone: &str, spot: usize) -> (f32, f32) {
        let c = data.zone(zone).unwrap().spots[spot].cell;
        (c[0] as f32 + 0.5, c[1] as f32 + 0.5)
    }

    #[test]
    fn texts_match_the_prototype() {
        let d = data();
        let cfg = &d.config;
        assert_eq!(
            objective_text(&d, "c_relight"),
            "Plant a lantern in the great hall of The Undercroft."
        );
        assert_eq!(
            objective_text(&d, "c_wick"),
            "Bank 4 oil flasks from The Undercroft in one run."
        );
        assert_eq!(
            objective_text(&d, "c_sound"),
            "Stay 30 s in the drowned hall of The Cistern."
        );
        assert_eq!(
            objective_text(&d, "c_chart"),
            "Recover the lost chart from the pump room of The Cistern and bank it."
        );
        assert_eq!(
            objective_text(&d, "c_censer"),
            "Bank 2 rich relics from The Ossuary in one run."
        );
        assert_eq!(
            objective_text(&d, "c_ledger"),
            "Recover the ledger from the east bone-pit of The Ossuary and bank it."
        );
        assert_eq!(
            objective_text(&d, "c_vigil"),
            "Stay 45 s in the south vault of The Ossuary with the lamp doused."
        );
        assert_eq!(
            objective_text(&d, "c_bones"),
            "Bank 3 relics from The Ossuary in one run."
        );
        assert_eq!(objective_text(&d, "nope"), "");
        let r = |id: &str| reward_text(cfg, &d.contracts.contracts[id].reward);
        assert_eq!(r("c_relight"), "Pry Bar, 4 flame points");
        assert_eq!(r("c_wick"), "60 oil");
        assert_eq!(r("c_sound"), "Sluice Key");
        assert_eq!(r("c_censer"), "Censer");
        assert_eq!(
            reward_text(
                cfg,
                &Reward {
                    tool: None,
                    pts: 1,
                    oil: 0
                }
            ),
            "1 flame point"
        );
        assert_eq!(
            reward_text(
                cfg,
                &Reward {
                    tool: None,
                    pts: 0,
                    oil: 0
                }
            ),
            "nothing"
        );
        for id in &d.contracts.order {
            let c = &d.contracts.contracts[id];
            let g = goal_of(c);
            match c.kind {
                ContractKind::Fetch => assert_eq!(g, c.n.unwrap()),
                ContractKind::Survive => assert_eq!(g, c.seconds.unwrap() as u32),
                _ => assert_eq!(g, 1),
            }
        }
    }

    #[test]
    fn availability_status_and_offers() {
        let d = data();
        let mut save = SaveData::default();
        assert_eq!(status(&d, &save, "c_relight"), Some(ContractStatus::Locked));
        assert_eq!(status(&d, &save, "x"), None);
        assert!(available(&d, &save, "lamplighter").is_none());
        save.rescued.lamplighter = true;
        let o = available(&d, &save, "lamplighter").expect("offer");
        assert_eq!(o.id, "c_relight");
        assert_eq!(o.title, "Relight the Great Hall");
        assert_eq!(o.reward_text, "Pry Bar, 4 flame points");
        assert_eq!(o.zone, "undercroft");
        assert_eq!(o.kind, ContractKind::Plant);
        assert_eq!(
            status(&d, &save, "c_relight"),
            Some(ContractStatus::Available)
        );
        assert_eq!(status(&d, &save, "c_wick"), Some(ContractStatus::Queued));
        let ev = on_rescued(&d, &save, "lamplighter");
        assert_eq!(
            ev,
            vec![SimEvent::toast(
                "Relight the Great Hall — a contract from the Lantern. Talk to them at the flame."
            )]
        );
        assert!(on_rescued(&d, &save, "keeper").is_empty());
        let env = ContractEnv::hub(&d);
        // the queued one cannot be accepted, the offered one can
        assert!(!accept(&env, &mut save, "c_wick", false).0);
        let (ok, ev) = accept(&env, &mut save, "c_relight", false);
        assert!(ok);
        assert_eq!(names(&ev), vec!["contractAccepted", "uiClick", "toast"]);
        assert_eq!(
            ev[0],
            SimEvent::ContractAccepted {
                id: "c_relight".into(),
                reward: RewardPayload {
                    tool: Some("prybar".into()),
                    pts: 4,
                    oil: 0
                }
            }
        );
        assert_eq!(status(&d, &save, "c_relight"), Some(ContractStatus::Active));
        assert!(
            available(&d, &save, "lamplighter").is_none(),
            "one per NPC at a time"
        );
        assert!(
            !accept(&env, &mut save, "c_relight", false).0,
            "already active"
        );
        assert!(
            accept(&env, &mut save, "c_wick", true).0,
            "force skips the queue"
        );
        assert_eq!(
            hud_lines(&d, &save),
            vec!["◇ Relight the Great Hall — 0/1", "◇ Wick's Reserve — 0/4"]
        );
        // abandon
        let (ok, ev) = abandon(&mut save, "c_wick");
        assert!(ok);
        assert_eq!(
            ev[0],
            SimEvent::ContractFailed {
                id: "c_wick".into(),
                reason: "abandoned".into(),
                progress: None,
                goal: None
            }
        );
        assert!(!abandon(&mut save, "c_wick").0);
        let l = list(&d, &save);
        assert_eq!(l.len(), 8);
        assert_eq!(l[0].status, ContractStatus::Active);
        assert_eq!(l[1].status, ContractStatus::Queued);
        assert_eq!(l[2].status, ContractStatus::Locked);
        let t = targets(&d, &save, None);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].cell, Some([31, 47]));
        assert_eq!((t[0].x, t[0].z), (Some(31.5), Some(47.5)));
        assert!(targets(&d, &save, Some("cistern")).is_empty());
    }

    #[test]
    fn max_two_active() {
        let d = data();
        let mut save = all_rescued();
        let env = ContractEnv::hub(&d);
        assert!(accept(&env, &mut save, "c_relight", false).0);
        assert!(accept(&env, &mut save, "c_sound", false).0);
        let (ok, ev) = accept(&env, &mut save, "c_censer", false);
        assert!(!ok);
        assert_eq!(
            ev,
            vec![
                SimEvent::toast("You already carry enough contracts."),
                SimEvent::ui_error()
            ]
        );
        assert_eq!(save.contracts.active.len(), 2);
        abandon(&mut save, "c_sound");
        assert!(accept(&env, &mut save, "c_censer", false).0);
    }

    #[test]
    fn plant_completes_on_a_lantern_at_the_spot() {
        let d = data();
        let mut save = all_rescued();
        let mut hub = HubState::default();
        let map = d.parse_zone("undercroft").unwrap().unwrap();
        let env = ContractEnv::zone(&d, "undercroft", &map);
        accept(&env, &mut save, "c_relight", false);
        let (sx, sz) = spot_pos(&env, "c_relight").unwrap();
        assert_eq!((sx, sz), spot_of(&d, "undercroft", 1));
        assert!(
            on_lantern(&env, &mut save, &mut hub, sx + 2.0, sz).is_empty(),
            "too far"
        );
        // wrong zone / hub: nothing
        let hub_env = ContractEnv::hub(&d);
        assert!(on_lantern(&hub_env, &mut save, &mut hub, sx, sz).is_empty());
        let ev = on_lantern(&env, &mut save, &mut hub, sx + 1.0, sz + 1.0);
        assert_eq!(names(&ev), vec!["toolGained", "toast", "contractComplete"]);
        assert_eq!(save.points, 4);
        assert!(save.tools.prybar);
        assert_eq!(status(&d, &save, "c_relight"), Some(ContractStatus::Done));
        assert_eq!(progress(&save, "c_relight"), 1.0);
        assert_eq!(
            ev[1],
            SimEvent::toast("Contract complete: Relight the Great Hall — Pry Bar, 4 flame points")
        );
        // Wick now offers the next one
        assert_eq!(
            available(&d, &save, "lamplighter").map(|o| o.id),
            Some("c_wick".into())
        );
        // a death never resets a plant contract (it has no run progress)
        assert!(accept(&env, &mut save, "c_wick", true).0);
        assert!(on_death(&env, &mut save).is_empty());
    }

    #[test]
    fn fetch_progress_short_and_complete() {
        let d = data();
        let mut save = all_rescued();
        let mut hub = HubState::default();
        let map = d.parse_zone("undercroft").unwrap().unwrap();
        let env = ContractEnv::zone(&d, "undercroft", &map);
        assert!(accept(&env, &mut save, "c_wick", true).0);
        let mut p = PlayerView::default();
        p.carried.oil = 2;
        let ev = tick(&env, &mut save, &mut hub, 0.1, &p, false);
        assert_eq!(
            ev,
            vec![SimEvent::ContractProgress {
                id: "c_wick".into(),
                progress: 2,
                goal: 4
            }]
        );
        assert!(
            tick(&env, &mut save, &mut hub, 0.1, &p, false).is_empty(),
            "unchanged: silent"
        );
        assert!(
            tick(&env, &mut save, &mut hub, 0.1, &p, true).is_empty(),
            "paused"
        );
        assert_eq!(hud_lines(&d, &save), vec!["◇ Wick's Reserve — 2/4"]);
        // bank short: progress lost, contract kept
        let ev = on_bank(&env, &mut save, &mut hub, &p.carried, "undercroft");
        assert_eq!(
            ev,
            vec![
                SimEvent::ContractFailed {
                    id: "c_wick".into(),
                    reason: "short".into(),
                    progress: Some(2),
                    goal: Some(4)
                },
                SimEvent::toast("Wick's Reserve: 2/4 — not enough in one run."),
            ]
        );
        assert_eq!(status(&d, &save, "c_wick"), Some(ContractStatus::Active));
        // death resets
        p.carried.oil = 3;
        tick(&env, &mut save, &mut hub, 0.1, &p, false);
        let ev = on_death(&env, &mut save);
        assert_eq!(
            ev,
            vec![
                SimEvent::ContractFailed {
                    id: "c_wick".into(),
                    reason: "death".into(),
                    progress: Some(3),
                    goal: Some(4)
                },
                SimEvent::toast("Contract progress lost: Wick's Reserve"),
            ]
        );
        assert_eq!(progress(&save, "c_wick"), 0.0);
        assert!(on_death(&env, &mut save).is_empty(), "nothing left to lose");
        // leaving / entering resets only that zone
        tick(&env, &mut save, &mut hub, 0.1, &p, false);
        assert!(on_zone_exit(&env, &mut save, "cistern").is_empty());
        assert_eq!(
            names(&on_zone_exit(&env, &mut save, "undercroft")),
            vec!["contractFailed"]
        );
        tick(&env, &mut save, &mut hub, 0.1, &p, false);
        assert_eq!(
            names(&on_zone_enter(&env, &mut save, "undercroft")),
            vec!["contractFailed"]
        );
        // bank with enough: complete, +60 oil
        p.carried.oil = 4;
        let ev = on_bank(&env, &mut save, &mut hub, &p.carried, "undercroft");
        assert_eq!(names(&ev), vec!["toast", "contractComplete"]);
        assert_eq!(save.oil, 60);
        assert_eq!(progress(&save, "c_wick"), 4.0);
        // the two Ossuary fetches
        let map = d.parse_zone("ossuary").unwrap().unwrap();
        let env = ContractEnv::zone(&d, "ossuary", &map);
        assert!(accept(&env, &mut save, "c_censer", true).0);
        assert!(accept(&env, &mut save, "c_bones", true).0);
        let c = Carried {
            relic: 3,
            rich: 2,
            ..Default::default()
        };
        let ev = on_bank(&env, &mut save, &mut hub, &c, "ossuary");
        assert_eq!(
            names(&ev),
            vec![
                "toolGained",
                "toast",
                "contractComplete",
                "flameTier",
                "toast",
                "contractComplete"
            ]
        );
        assert!(save.tools.censer);
        assert_eq!(save.points, 6);
        assert_eq!(hub.tier, 2, "the reward re-tiered the flame");
        assert_eq!(save.contracts.done.len(), 3);
    }

    #[test]
    fn recover_spawns_carries_and_banks() {
        let d = data();
        let mut save = all_rescued();
        let mut hub = HubState::default();
        let map = d.parse_zone("cistern").unwrap().unwrap();
        let env = ContractEnv::zone(&d, "cistern", &map);
        assert!(accept(&env, &mut save, "c_chart", true).0);
        let world = QuestWorld::default();
        let q = quests_to_spawn(&env, &save, "cistern", &world);
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].contract_id, "c_chart");
        assert_eq!(q[0].label, "lost chart");
        assert_eq!((q[0].x, q[0].z), spot_of(&d, "cistern", 1));
        assert!(quests_to_spawn(&env, &save, "ossuary", &world).is_empty());
        let placed = QuestWorld {
            quest_items: vec!["c_chart".into()],
            ..Default::default()
        };
        assert!(
            quests_to_spawn(&env, &save, "cistern", &placed).is_empty(),
            "already lying there"
        );
        let held = QuestWorld {
            carried_quest: 1,
            ..Default::default()
        };
        assert!(quest_to_spawn(&env, "c_chart", &held).is_none());
        let bundled = QuestWorld {
            bundle_holds_quest: true,
            ..Default::default()
        };
        assert!(quest_to_spawn(&env, "c_chart", &bundled).is_none());
        assert!(
            quest_to_spawn(&env, "c_wick", &world).is_none(),
            "not a recover contract"
        );
        // pick it up
        let ev = on_pickup(&env, &mut save, ItemKind::Quest, Some("c_chart"), false);
        assert_eq!(
            ev,
            vec![
                SimEvent::toast("You found the lost chart. Bring it to the stairs."),
                SimEvent::ContractProgress {
                    id: "c_chart".into(),
                    progress: 1,
                    goal: 1
                },
            ]
        );
        assert!(on_pickup(&env, &mut save, ItemKind::Oil, None, false).is_empty());
        // dropped on death, recovered from the bundle
        let ev = on_death(&env, &mut save);
        assert_eq!(names(&ev), vec!["contractFailed", "toast"]);
        let ev = on_pickup(&env, &mut save, ItemKind::Bundle, None, true);
        assert_eq!(
            ev,
            vec![SimEvent::ContractProgress {
                id: "c_chart".into(),
                progress: 1,
                goal: 1
            }]
        );
        // the per-frame mirror of carried.quest
        let mut p = PlayerView::default();
        tick(&env, &mut save, &mut hub, 0.1, &p, false);
        assert_eq!(progress(&save, "c_chart"), 0.0);
        p.carried.quest = 1;
        tick(&env, &mut save, &mut hub, 0.1, &p, false);
        assert_eq!(progress(&save, "c_chart"), 1.0);
        // bank it: +8 points → tier 2
        let ev = on_bank(&env, &mut save, &mut hub, &p.carried, "cistern");
        assert_eq!(names(&ev), vec!["flameTier", "toast", "contractComplete"]);
        assert_eq!(save.points, 8);
        assert_eq!(status(&d, &save, "c_chart"), Some(ContractStatus::Done));
        // the Ossuary ledger: an elevator zone says "cage"
        let map = d.parse_zone("ossuary").unwrap().unwrap();
        let env = ContractEnv::zone(&d, "ossuary", &map);
        assert!(accept(&env, &mut save, "c_ledger", true).0);
        let ev = on_pickup(&env, &mut save, ItemKind::Quest, Some("c_ledger"), false);
        assert_eq!(
            ev[0],
            SimEvent::toast("You found the ledger. Bring it to the cage.")
        );
        let c = Carried {
            quest: 1,
            ..Default::default()
        };
        let ev = on_bank(&env, &mut save, &mut hub, &c, "ossuary");
        assert_eq!(names(&ev), vec!["toast", "contractComplete"]);
        assert_eq!(save.oil, 80);
    }

    #[test]
    fn survive_counts_contiguous_seconds() {
        let d = data();
        let mut save = all_rescued();
        let mut hub = HubState::default();
        let map = d.parse_zone("cistern").unwrap().unwrap();
        let env = ContractEnv::zone(&d, "cistern", &map);
        accept(&env, &mut save, "c_sound", false);
        let (sx, sz) = spot_of(&d, "cistern", 0);
        let mut p = PlayerView {
            x: sx + 1.0,
            z: sz,
            lamp_on: true,
            ..Default::default()
        };
        for _ in 0..100 {
            assert!(tick(&env, &mut save, &mut hub, 0.1, &p, false).is_empty());
        }
        assert!((progress(&save, "c_sound") - 10.0).abs() < 1e-6);
        assert_eq!(
            hud_lines(&d, &save),
            vec!["◇ Sound the Drowned Hall — 10/30 s"]
        );
        // step out: the timer resets
        p.x = sx + 4.0;
        tick(&env, &mut save, &mut hub, 0.1, &p, false);
        assert_eq!(progress(&save, "c_sound"), 0.0);
        p.x = sx + 2.9;
        let mut done = Vec::new();
        for _ in 0..301 {
            done.extend(tick(&env, &mut save, &mut hub, 0.1, &p, false));
        }
        assert_eq!(
            names(&done),
            vec!["toolGained", "toast", "contractComplete"]
        );
        assert!(save.tools.sluice);
        assert_eq!(progress(&save, "c_sound"), 30.0);
        // the vigil needs the lamp doused
        let map = d.parse_zone("ossuary").unwrap().unwrap();
        let env = ContractEnv::zone(&d, "ossuary", &map);
        assert!(accept(&env, &mut save, "c_vigil", true).0);
        let (sx, sz) = spot_of(&d, "ossuary", 1);
        let mut p = PlayerView {
            x: sx,
            z: sz,
            lamp_on: true,
            ..Default::default()
        };
        for _ in 0..20 {
            tick(&env, &mut save, &mut hub, 0.5, &p, false);
        }
        assert_eq!(progress(&save, "c_vigil"), 0.0, "lamp on: no vigil");
        p.lamp_on = false;
        let mut done = Vec::new();
        for _ in 0..90 {
            done.extend(tick(&env, &mut save, &mut hub, 0.5, &p, false));
        }
        assert_eq!(names(&done), vec!["flameTier", "toast", "contractComplete"]);
        assert_eq!(save.points, 10);
        assert_eq!(hub.tier, 2);
    }

    #[test]
    fn all_eight_complete_by_events() {
        let d = data();
        let mut save = all_rescued();
        let mut hub = HubState::default();
        let mut done = 0;
        for id in d.contracts.order.clone() {
            let c = d.contracts.contracts[&id].clone();
            let map = d.parse_zone(&c.zone).unwrap().unwrap();
            let env = ContractEnv::zone(&d, &c.zone, &map);
            assert_eq!(
                status(&d, &save, &id),
                Some(ContractStatus::Available),
                "{id} offered in order"
            );
            assert!(accept(&env, &mut save, &id, false).0, "{id}");
            let ev = match c.kind {
                ContractKind::Plant => {
                    let (x, z) = spot_pos(&env, &id).unwrap();
                    on_lantern(&env, &mut save, &mut hub, x, z)
                }
                ContractKind::Fetch => {
                    let mut carried = Carried::default();
                    match c.item_kind.unwrap() {
                        ItemKind::Oil => carried.oil = c.n.unwrap(),
                        ItemKind::Relic => carried.relic = c.n.unwrap(),
                        ItemKind::Rich => carried.rich = c.n.unwrap(),
                        _ => unreachable!(),
                    }
                    on_bank(&env, &mut save, &mut hub, &carried, &c.zone)
                }
                ContractKind::Recover => {
                    let q = quest_to_spawn(&env, &id, &QuestWorld::default()).expect("quest item");
                    assert_eq!(q.label, c.item.clone().unwrap());
                    on_pickup(&env, &mut save, ItemKind::Quest, Some(&id), false);
                    let carried = Carried {
                        quest: 1,
                        ..Default::default()
                    };
                    on_bank(&env, &mut save, &mut hub, &carried, &c.zone)
                }
                ContractKind::Survive => {
                    let (x, z) = spot_pos(&env, &id).unwrap();
                    let p = PlayerView {
                        x,
                        z,
                        lamp_on: !c.lamp_off,
                        ..Default::default()
                    };
                    let mut ev = Vec::new();
                    let mut t = 0.0;
                    while t < c.seconds.unwrap() + 1.0 {
                        ev.extend(tick(&env, &mut save, &mut hub, 0.25, &p, false));
                        t += 0.25;
                    }
                    ev
                }
            };
            assert!(
                ev.iter()
                    .any(|e| matches!(e, SimEvent::ContractComplete { id: cid, .. } if *cid == id)),
                "{id} completes"
            );
            assert_eq!(status(&d, &save, &id), Some(ContractStatus::Done));
            done += 1;
        }
        assert_eq!(done, 8);
        assert_eq!(save.contracts.done.len(), 8);
        assert!(save.contracts.active.is_empty());
        assert_eq!(save.points, 4 + 8 + 10 + 6);
        assert_eq!(save.oil, 60 + 80);
        assert!(save.tools.prybar && save.tools.sluice && save.tools.censer);
        assert_eq!(hub.tier, 3);
        for npc in crate::save::Rescued::IDS {
            assert!(
                available(&d, &save, npc).is_none(),
                "{npc}'s list is exhausted"
            );
        }
        assert!(hud_lines(&d, &save).is_empty());
    }
}
