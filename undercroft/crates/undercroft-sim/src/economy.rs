//! LANE: economy. The light economy and the hub ledgers — `main.js` bank / pickup / death-bundle / oil burn,
//! `hub.js` flame tiers, buildings, services, blessing, `endgame.js` altar / endings / laps (DESIGN.md §4, §6, §8, §9).
//!
//! Must contain: `Carried` (oil / relic / rich / quest) with `describe`, the per-tick lamp burn
//! (`main.js:updateLamp` + `zoneMul`: deep / band / zone / light-tech multipliers), `bank` (points, resources,
//! `bank` + `zoneExit`), death and the bundle drop, `tierFor(points)` / `checkTier` (`flameTier`), `buildStatus`
//! / `build` (`BUILD_COSTS`, `build`), the Workshop / Press / Shrine services (`lightTech`, `service`, `blessing`,
//! `blessingKept`), `zoneLocked` is in `undercroft_data::GameData::zone_locked`, and the endgame: `lapOf` driven
//! `lap` events with `LAP_LINES`, hunter staging (`hunterWoken` / `hunterSpawned` windows), the altar
//! (`altar`, ending availability via `EndingDef::requires`, `ending` / `endingContinue`, `sourceAbandoned`).
//! Every function takes the state it needs and returns `Vec<SimEvent>`. Nothing else.
