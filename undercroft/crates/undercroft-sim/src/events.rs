//! `SimEvent` — one variant per name on the prototype's event bus (DESIGN.md §12 "Main events" plus the §5.8
//! creature events and the UI/menu events `main.js` emits). Payload fields are the ones the JS `emit()` calls
//! carry (every `events.emit(name, {...})` in `prototype/src/*.js`); `SimEvent::name()` is the JS name, so a
//! Bevy event log or a test can compare against the prototype's traces.
//!
//! Lanes return `Vec<SimEvent>` from every state change; the ECS shell fans them out (audio, UI, save) the way
//! `main.js:makeEvents` did with its synchronous listeners.

use crate::player::Carried;
use undercroft_data::tables::ItemKind;

/// Who a hunter caught (`hunterCatch.target`, `death.target`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatchTarget {
    Player,
    /// The follower, by NPC id.
    Npc(String),
}

/// A contract reward as emitted (`contracts.js` copies `CONTRACTS[id].reward`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewardPayload {
    pub tool: Option<String>,
    pub pts: u32,
    pub oil: u32,
}

/// What a `build` cost (`hub.js:build` — zeros when built free by a debug action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildSpend {
    pub oil: u32,
    pub relics: u32,
    pub rich: u32,
}

/// `hub.js` `service` event payloads (`{id, action, …}`).
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceAction {
    /// Workshop: `{id:'workshop', action:'lightTech', tier}`.
    LightTech { tier: u32 },
    /// Oil Press: `{id:'press', action:'press', n, oil}` — `n` relics pressed for `oil`.
    Press { n: u32, oil: u32 },
    /// Oil Press: `{id:'press', action:'reservoir', level, startOil}`.
    Reservoir { level: u32, start_oil: f32 },
    /// Shrine: `{id:'shrine', action:'charge', lit}` — the blessing charged (or not) on descent.
    Charge { lit: bool },
}

impl ServiceAction {
    /// The building id the JS puts in `service.id`.
    pub fn building(&self) -> &'static str {
        match self {
            ServiceAction::LightTech { .. } => "workshop",
            ServiceAction::Press { .. } | ServiceAction::Reservoir { .. } => "press",
            ServiceAction::Charge { .. } => "shrine",
        }
    }
}

/// The interaction target kinds `main.js:interactTarget` returns (`interact.target.type`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractTarget {
    Item,
    Bank,
    Descend,
    Gate,
    Shortcut,
    Npc,
    Building,
    Altar,
    Elevator,
    /// Any other `{run()}` target (`type` string kept).
    Other(String),
}

/// Every event the simulation can emit. Field names are the snake_case of the JS payload keys.
#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    // ---- lifecycle / mode (main.js) ----
    /// `begin {zoneId: null}` — a game started from the title.
    Begin,
    /// `title {}` — back on the main menu.
    Title,
    /// `hubEnter {zoneId: null}`.
    HubEnter,
    /// `zoneEnter {zoneId}`.
    ZoneEnter {
        zone_id: String,
    },
    /// `zoneExit {zoneId}`.
    ZoneExit {
        zone_id: String,
    },
    /// `zoneIntro {zoneId, text}` — `world.js`'s `zoneEnter` listener, emitted with the matching
    /// `toast` when the zone has an `intro` line.
    ZoneIntro {
        zone_id: String,
        text: String,
    },
    /// `saveReset {}`.
    SaveReset,
    /// `menuOpen {kind}` / `menuClose {kind}` (`board`, `npc`, `building`, `pause`, `main` …).
    MenuOpen {
        kind: String,
    },
    MenuClose {
        kind: String,
    },
    /// `pauseOpen {inZone}`.
    PauseOpen {
        in_zone: bool,
    },
    /// `key {code, mode}` — raw key press with the current mode.
    Key {
        code: String,
        mode: String,
    },
    /// `runAbandoned {zoneId, lost}` — returned to the main menu from a zone; `lost` = describe(carried).
    RunAbandoned {
        zone_id: String,
        lost: String,
    },

    // ---- player (main.js) ----
    /// `pickup {kind, item}` — `contents` only for a death bundle.
    Pickup {
        kind: ItemKind,
        x: f32,
        z: f32,
        contents: Option<Carried>,
    },
    /// `bank {carried, pts, zoneId}`.
    Bank {
        carried: Carried,
        pts: u32,
        zone_id: String,
    },
    /// `death {x, z, hunterId, target:'player'}`.
    Death {
        x: f32,
        z: f32,
        hunter_id: Option<u32>,
    },
    /// `flash {x, z}`.
    Flash {
        x: f32,
        z: f32,
    },
    /// `lantern {x, z}` — a lantern planted (`lanternPlanted` is an alias).
    Lantern {
        x: f32,
        z: f32,
    },
    /// `lanternRemoved {x, z}` — the oldest lantern recycled, or one smashed.
    LanternRemoved {
        x: f32,
        z: f32,
    },
    /// `lampToggle {on}`.
    LampToggle {
        on: bool,
    },
    /// `topUp {oil}` — a flask poured in; `oil` = the lamp's oil afterwards.
    TopUp {
        oil: f32,
    },
    /// `interact {target, handled, x, z}`.
    Interact {
        target: Option<InteractTarget>,
        handled: bool,
        x: f32,
        z: f32,
    },
    /// `waterEnter {x, z}` / `waterExit {x, z}` (world.js).
    WaterEnter {
        x: f32,
        z: f32,
    },
    WaterExit {
        x: f32,
        z: f32,
    },

    // ---- world (world.js) ----
    /// `gateLocked {zoneId, cx, cz, tool, toolName}` — E on a gate without its tool.
    GateLocked {
        zone_id: String,
        cx: i32,
        cz: i32,
        tool: String,
        tool_name: String,
    },
    /// `gateOpened {zoneId, id, cx, cz, idx, tool}`.
    GateOpened {
        zone_id: String,
        id: String,
        cx: i32,
        cz: i32,
        idx: usize,
        tool: String,
    },
    /// `shortcutOpened {zoneId, id, name, cx, cz, idx}`.
    ShortcutOpened {
        zone_id: String,
        id: String,
        name: String,
        cx: i32,
        cz: i32,
        idx: usize,
    },

    // ---- hunters and creatures (hunter.js) ----
    /// `hunterState {id, state, prev}`.
    HunterState {
        id: u32,
        state: String,
        prev: String,
    },
    /// `hunterCatch {x, z, hunterId, target}`.
    HunterCatch {
        x: f32,
        z: f32,
        hunter_id: u32,
        target: CatchTarget,
    },
    /// `lampSnuffed {hunterId, oil, lockout, x, z}` — the Lampwight drank the flame.
    LampSnuffed {
        hunter_id: u32,
        oil: f32,
        lockout: f32,
        x: f32,
        z: f32,
    },
    /// `lanternSmashed {hunterId, x, z}` — the Brute (a `lanternRemoved` precedes it).
    LanternSmashed {
        hunter_id: u32,
        x: f32,
        z: f32,
    },
    /// `wardenAlert {hunterId, x, z}`.
    WardenAlert {
        hunter_id: u32,
        x: f32,
        z: f32,
    },
    /// `wardenReturn {hunterId}`.
    WardenReturn {
        hunter_id: u32,
    },
    /// `drownerSurge {hunterId, x, z}`.
    DrownerSurge {
        hunter_id: u32,
        x: f32,
        z: f32,
    },
    /// `drownerSink {hunterId}`.
    DrownerSink {
        hunter_id: u32,
    },
    /// `falseLightPounce {hunterId, x, z}`.
    FalseLightPounce {
        hunter_id: u32,
        x: f32,
        z: f32,
    },
    /// `falseLightReveal {hunterId, x, z}`.
    FalseLightReveal {
        hunter_id: u32,
        x: f32,
        z: f32,
    },
    /// `flashResisted {hunterId, profile}` — the Brute snorts.
    FlashResisted {
        hunter_id: u32,
        profile: String,
    },
    /// `creatureStep {hunterId, profile, x, z, d}` — a footfall (Brute stomp) at distance `d` from the player.
    CreatureStep {
        hunter_id: u32,
        profile: String,
        x: f32,
        z: f32,
        d: f32,
    },

    // ---- NPCs (npc.js) ----
    /// `npcFreed {id, x, z}`.
    NpcFreed {
        id: String,
        x: f32,
        z: f32,
    },
    /// `npcCaught {id, x, z, hunterId}` — always followed by an identical `npcLost`.
    NpcCaught {
        id: String,
        x: f32,
        z: f32,
        hunter_id: Option<u32>,
    },
    /// `npcLost {id, x, z, hunterId}`.
    NpcLost {
        id: String,
        x: f32,
        z: f32,
        hunter_id: Option<u32>,
    },
    /// `npcRescued {id, zoneId, debug?}`.
    NpcRescued {
        id: String,
        zone_id: String,
        debug: bool,
    },
    /// `npcTalk {id}`.
    NpcTalk {
        id: String,
    },

    // ---- contracts (contracts.js) ----
    /// `contractAccepted {id, reward}`.
    ContractAccepted {
        id: String,
        reward: RewardPayload,
    },
    /// `contractComplete {id, reward}`.
    ContractComplete {
        id: String,
        reward: RewardPayload,
    },
    /// `contractFailed {id, reason, progress?, goal?}` — reasons: `abandoned`, `short`, `death`, `left` …
    ContractFailed {
        id: String,
        reason: String,
        progress: Option<u32>,
        goal: Option<u32>,
    },
    /// `contractProgress {id, progress, goal}`.
    ContractProgress {
        id: String,
        progress: u32,
        goal: u32,
    },
    /// `toolGained {id, reward:{tool}}`.
    ToolGained {
        id: String,
    },

    // ---- hub (hub.js) ----
    /// `flameTier {tier, prev, initial}`.
    FlameTier {
        tier: u32,
        prev: u32,
        initial: bool,
    },
    /// `build {id, cost}`.
    Build {
        id: String,
        cost: BuildSpend,
    },
    /// `lightTech {tier, prev}`.
    LightTech {
        tier: u32,
        prev: u32,
    },
    /// `service {id, action, …}`.
    Service(ServiceAction),
    /// `zoneSelected {zoneId}` — the Departure Board choice.
    ZoneSelected {
        zone_id: String,
    },
    /// `blessing {on}` — bought / already held.
    Blessing {
        on: bool,
    },
    /// `blessingKept {kept, pts, zoneId, x, z}` — on death, half of each carried kind banked anyway.
    BlessingKept {
        kept: Carried,
        pts: u32,
        zone_id: String,
        x: f32,
        z: f32,
    },
    /// `minimap {on}`.
    Minimap {
        on: bool,
    },

    // ---- endgame (endgame.js) ----
    /// `lap {lap, prev, zoneId}` — crossed a Source lap line going deeper.
    Lap {
        lap: i32,
        prev: i32,
        zone_id: String,
    },
    /// `hunterWoken {id, lap}` — a dormant Source hunter woke.
    HunterWoken {
        id: u32,
        lap: i32,
    },
    /// `hunterSpawned {id, x, z, profile, lap}` — an extra Source hunter.
    HunterSpawned {
        id: u32,
        x: f32,
        z: f32,
        profile: String,
        lap: i32,
    },
    /// `sourceAbandoned {carried, zoneId}` — rode up out of the Source; the loot stays below.
    SourceAbandoned {
        carried: Carried,
        zone_id: String,
    },
    /// `altar {x, z, zoneId}` — the altar menu opened.
    Altar {
        x: f32,
        z: f32,
        zone_id: String,
    },
    /// `ending {id, choice, title, tier, rescued, first}`.
    Ending {
        id: String,
        choice: String,
        title: String,
        tier: u32,
        rescued: u32,
        first: bool,
    },
    /// `endingContinue {id}`.
    EndingContinue {
        id: String,
    },

    // ---- UI feedback (every module) ----
    /// `uiClick {}`.
    UiClick,
    /// `uiError {reason?, id?}` (`reason: 'shortcutBarred'` from the barred side of a door).
    UiError {
        reason: Option<String>,
        id: Option<String>,
    },
    /// `toast {msg}`.
    Toast {
        msg: String,
    },
}

impl SimEvent {
    /// The prototype's event name (`events.emit(name, …)`).
    pub fn name(&self) -> &'static str {
        match self {
            SimEvent::Begin => "begin",
            SimEvent::Title => "title",
            SimEvent::HubEnter => "hubEnter",
            SimEvent::ZoneEnter { .. } => "zoneEnter",
            SimEvent::ZoneExit { .. } => "zoneExit",
            SimEvent::ZoneIntro { .. } => "zoneIntro",
            SimEvent::SaveReset => "saveReset",
            SimEvent::MenuOpen { .. } => "menuOpen",
            SimEvent::MenuClose { .. } => "menuClose",
            SimEvent::PauseOpen { .. } => "pauseOpen",
            SimEvent::Key { .. } => "key",
            SimEvent::RunAbandoned { .. } => "runAbandoned",
            SimEvent::Pickup { .. } => "pickup",
            SimEvent::Bank { .. } => "bank",
            SimEvent::Death { .. } => "death",
            SimEvent::Flash { .. } => "flash",
            SimEvent::Lantern { .. } => "lantern",
            SimEvent::LanternRemoved { .. } => "lanternRemoved",
            SimEvent::LampToggle { .. } => "lampToggle",
            SimEvent::TopUp { .. } => "topUp",
            SimEvent::Interact { .. } => "interact",
            SimEvent::WaterEnter { .. } => "waterEnter",
            SimEvent::WaterExit { .. } => "waterExit",
            SimEvent::GateLocked { .. } => "gateLocked",
            SimEvent::GateOpened { .. } => "gateOpened",
            SimEvent::ShortcutOpened { .. } => "shortcutOpened",
            SimEvent::HunterState { .. } => "hunterState",
            SimEvent::HunterCatch { .. } => "hunterCatch",
            SimEvent::LampSnuffed { .. } => "lampSnuffed",
            SimEvent::LanternSmashed { .. } => "lanternSmashed",
            SimEvent::WardenAlert { .. } => "wardenAlert",
            SimEvent::WardenReturn { .. } => "wardenReturn",
            SimEvent::DrownerSurge { .. } => "drownerSurge",
            SimEvent::DrownerSink { .. } => "drownerSink",
            SimEvent::FalseLightPounce { .. } => "falseLightPounce",
            SimEvent::FalseLightReveal { .. } => "falseLightReveal",
            SimEvent::FlashResisted { .. } => "flashResisted",
            SimEvent::CreatureStep { .. } => "creatureStep",
            SimEvent::NpcFreed { .. } => "npcFreed",
            SimEvent::NpcCaught { .. } => "npcCaught",
            SimEvent::NpcLost { .. } => "npcLost",
            SimEvent::NpcRescued { .. } => "npcRescued",
            SimEvent::NpcTalk { .. } => "npcTalk",
            SimEvent::ContractAccepted { .. } => "contractAccepted",
            SimEvent::ContractComplete { .. } => "contractComplete",
            SimEvent::ContractFailed { .. } => "contractFailed",
            SimEvent::ContractProgress { .. } => "contractProgress",
            SimEvent::ToolGained { .. } => "toolGained",
            SimEvent::FlameTier { .. } => "flameTier",
            SimEvent::Build { .. } => "build",
            SimEvent::LightTech { .. } => "lightTech",
            SimEvent::Service(_) => "service",
            SimEvent::ZoneSelected { .. } => "zoneSelected",
            SimEvent::Blessing { .. } => "blessing",
            SimEvent::BlessingKept { .. } => "blessingKept",
            SimEvent::Minimap { .. } => "minimap",
            SimEvent::Lap { .. } => "lap",
            SimEvent::HunterWoken { .. } => "hunterWoken",
            SimEvent::HunterSpawned { .. } => "hunterSpawned",
            SimEvent::SourceAbandoned { .. } => "sourceAbandoned",
            SimEvent::Altar { .. } => "altar",
            SimEvent::Ending { .. } => "ending",
            SimEvent::EndingContinue { .. } => "endingContinue",
            SimEvent::UiClick => "uiClick",
            SimEvent::UiError { .. } => "uiError",
            SimEvent::Toast { .. } => "toast",
        }
    }

    /// A plain toast.
    pub fn toast(msg: impl Into<String>) -> SimEvent {
        SimEvent::Toast { msg: msg.into() }
    }

    /// `uiError {}` with no reason.
    pub fn ui_error() -> SimEvent {
        SimEvent::UiError {
            reason: None,
            id: None,
        }
    }
}

/// The DESIGN.md §12 event list plus the §5.8 creature events — what `SimEvent::name` must cover.
pub const DESIGN_EVENT_NAMES: [&str; 45] = [
    "begin",
    "hubEnter",
    "zoneEnter",
    "zoneExit",
    "pickup",
    "bank",
    "flameTier",
    "death",
    "hunterCatch",
    "hunterState",
    "flash",
    "lantern",
    "lanternRemoved",
    "gateOpened",
    "shortcutOpened",
    "npcFreed",
    "npcCaught",
    "npcRescued",
    "contractAccepted",
    "contractComplete",
    "contractFailed",
    "toolGained",
    "build",
    "lightTech",
    "service",
    "zoneSelected",
    "blessing",
    "lap",
    "ending",
    "uiClick",
    "uiError",
    "toast",
    "lampSnuffed",
    "lanternSmashed",
    "wardenAlert",
    "wardenReturn",
    "drownerSurge",
    "drownerSink",
    "falseLightPounce",
    "falseLightReveal",
    "flashResisted",
    "creatureStep",
    "hunterWoken",
    "hunterSpawned",
    "altar",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<SimEvent> {
        let s = || String::new();
        vec![
            SimEvent::Begin,
            SimEvent::Title,
            SimEvent::HubEnter,
            SimEvent::ZoneEnter { zone_id: s() },
            SimEvent::ZoneExit { zone_id: s() },
            SimEvent::ZoneIntro {
                zone_id: s(),
                text: s(),
            },
            SimEvent::SaveReset,
            SimEvent::MenuOpen { kind: s() },
            SimEvent::MenuClose { kind: s() },
            SimEvent::PauseOpen { in_zone: true },
            SimEvent::Key {
                code: s(),
                mode: s(),
            },
            SimEvent::RunAbandoned {
                zone_id: s(),
                lost: s(),
            },
            SimEvent::Pickup {
                kind: ItemKind::Oil,
                x: 0.0,
                z: 0.0,
                contents: None,
            },
            SimEvent::Bank {
                carried: Carried::default(),
                pts: 0,
                zone_id: s(),
            },
            SimEvent::Death {
                x: 0.0,
                z: 0.0,
                hunter_id: None,
            },
            SimEvent::Flash { x: 0.0, z: 0.0 },
            SimEvent::Lantern { x: 0.0, z: 0.0 },
            SimEvent::LanternRemoved { x: 0.0, z: 0.0 },
            SimEvent::LampToggle { on: true },
            SimEvent::TopUp { oil: 0.0 },
            SimEvent::Interact {
                target: None,
                handled: false,
                x: 0.0,
                z: 0.0,
            },
            SimEvent::WaterEnter { x: 0.0, z: 0.0 },
            SimEvent::WaterExit { x: 0.0, z: 0.0 },
            SimEvent::GateLocked {
                zone_id: s(),
                cx: 0,
                cz: 0,
                tool: s(),
                tool_name: s(),
            },
            SimEvent::GateOpened {
                zone_id: s(),
                id: s(),
                cx: 0,
                cz: 0,
                idx: 0,
                tool: s(),
            },
            SimEvent::ShortcutOpened {
                zone_id: s(),
                id: s(),
                name: s(),
                cx: 0,
                cz: 0,
                idx: 0,
            },
            SimEvent::HunterState {
                id: 0,
                state: s(),
                prev: s(),
            },
            SimEvent::HunterCatch {
                x: 0.0,
                z: 0.0,
                hunter_id: 0,
                target: CatchTarget::Player,
            },
            SimEvent::LampSnuffed {
                hunter_id: 0,
                oil: 0.0,
                lockout: 0.0,
                x: 0.0,
                z: 0.0,
            },
            SimEvent::LanternSmashed {
                hunter_id: 0,
                x: 0.0,
                z: 0.0,
            },
            SimEvent::WardenAlert {
                hunter_id: 0,
                x: 0.0,
                z: 0.0,
            },
            SimEvent::WardenReturn { hunter_id: 0 },
            SimEvent::DrownerSurge {
                hunter_id: 0,
                x: 0.0,
                z: 0.0,
            },
            SimEvent::DrownerSink { hunter_id: 0 },
            SimEvent::FalseLightPounce {
                hunter_id: 0,
                x: 0.0,
                z: 0.0,
            },
            SimEvent::FalseLightReveal {
                hunter_id: 0,
                x: 0.0,
                z: 0.0,
            },
            SimEvent::FlashResisted {
                hunter_id: 0,
                profile: s(),
            },
            SimEvent::CreatureStep {
                hunter_id: 0,
                profile: s(),
                x: 0.0,
                z: 0.0,
                d: 0.0,
            },
            SimEvent::NpcFreed {
                id: s(),
                x: 0.0,
                z: 0.0,
            },
            SimEvent::NpcCaught {
                id: s(),
                x: 0.0,
                z: 0.0,
                hunter_id: None,
            },
            SimEvent::NpcLost {
                id: s(),
                x: 0.0,
                z: 0.0,
                hunter_id: None,
            },
            SimEvent::NpcRescued {
                id: s(),
                zone_id: s(),
                debug: false,
            },
            SimEvent::NpcTalk { id: s() },
            SimEvent::ContractAccepted {
                id: s(),
                reward: RewardPayload::default(),
            },
            SimEvent::ContractComplete {
                id: s(),
                reward: RewardPayload::default(),
            },
            SimEvent::ContractFailed {
                id: s(),
                reason: s(),
                progress: None,
                goal: None,
            },
            SimEvent::ContractProgress {
                id: s(),
                progress: 0,
                goal: 0,
            },
            SimEvent::ToolGained { id: s() },
            SimEvent::FlameTier {
                tier: 1,
                prev: 0,
                initial: true,
            },
            SimEvent::Build {
                id: s(),
                cost: BuildSpend::default(),
            },
            SimEvent::LightTech { tier: 1, prev: 0 },
            SimEvent::Service(ServiceAction::Charge { lit: true }),
            SimEvent::ZoneSelected { zone_id: s() },
            SimEvent::Blessing { on: true },
            SimEvent::BlessingKept {
                kept: Carried::default(),
                pts: 0,
                zone_id: s(),
                x: 0.0,
                z: 0.0,
            },
            SimEvent::Minimap { on: true },
            SimEvent::Lap {
                lap: 1,
                prev: 0,
                zone_id: s(),
            },
            SimEvent::HunterWoken { id: 0, lap: 1 },
            SimEvent::HunterSpawned {
                id: 0,
                x: 0.0,
                z: 0.0,
                profile: s(),
                lap: 3,
            },
            SimEvent::SourceAbandoned {
                carried: Carried::default(),
                zone_id: s(),
            },
            SimEvent::Altar {
                x: 0.0,
                z: 0.0,
                zone_id: s(),
            },
            SimEvent::Ending {
                id: s(),
                choice: s(),
                title: s(),
                tier: 1,
                rescued: 0,
                first: true,
            },
            SimEvent::EndingContinue { id: s() },
            SimEvent::UiClick,
            SimEvent::ui_error(),
            SimEvent::toast("x"),
        ]
    }

    #[test]
    fn every_design_event_has_a_variant() {
        let names: Vec<&str> = sample().iter().map(|e| e.name()).collect();
        for want in DESIGN_EVENT_NAMES {
            assert!(names.contains(&want), "missing event {want}");
        }
        assert_eq!(ServiceAction::Press { n: 1, oil: 30 }.building(), "press");
    }
}
