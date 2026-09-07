//! LANE: economy. Contract state machine — `contracts.js` (DESIGN-v2 §4).
//!
//! Must contain: the per-save contract store (`active`, `done`, `progress` — `save.contracts`), `available(npc)`,
//! `accept(id)`, the listeners on `pickup` / `bank` / `lantern` / `death` / `zoneEnter` / `zoneExit` /
//! `npcRescued` / `flameTier` as pure functions `fn(&mut ContractState, &SimEvent, ...) -> Vec<SimEvent>`, the
//! per-frame `tick(dt, &PlayerView)` for survive contracts, quest-item spawning for recover contracts
//! (`spotPos`), `rewardText`, the HUD lines, and the emitted `contractAccepted` / `contractComplete` /
//! `contractFailed` / `contractProgress` / `toolGained` events. Definitions come from
//! `undercroft_data::ContractTable`; numbers from `Config::contract_cfg`. Nothing else.
