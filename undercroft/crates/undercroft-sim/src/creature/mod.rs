//! LANE: creatures. Every creature in the ruin — `hunter.js` (DESIGN.md §5, §5.8).
//!
//! Must contain, as submodules of this directory: the hunter record (`hunter.js:makeHunter` — one stable shape
//! for every profile), the profile table (`hunter.js:PROFILES`: senses, movement predicate, FSM states, flash /
//! catch reactions, per `base` / `fast` / `lampwight` / `warden` / `drowner` / `falseLight` / `brute`), the
//! senses (`genericSense`, `wardenSense`, `drownerSense`, `falseLightSense`, `stimAt`, `playerLit`) reading
//! [`crate::player::PlayerView`], the shared FSM driver (`hunter.js:update` — tick / repath / move / catch, BFS
//! only on repath ticks) and the per-creature state machines (`BASE_FSM`, `LAMPWIGHT_FSM`, `WARDEN_FSM`,
//! `DROWNER_FSM`, `FALSELIGHT_FSM`, `BRUTE_FSM`), `spawnAll` / `spawnHunter` / `spawnCreature`, `onFlash`,
//! `stagger`, `investigate`, `pickWander` (random choices through [`crate::rng::SimRng`]) and `recomputePools`
//! is NOT here (see [`crate::pool`]). Every state change returns `Vec<SimEvent>` (`hunterState`, `hunterCatch`
//! and the §5.8 creature events in [`crate::events`]). Grid queries go through [`crate::grid`]. Nothing else.
