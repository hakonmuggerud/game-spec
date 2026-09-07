//! LANE: world. The map validator — `maps.js:validateMap` / `validateZone` / `validateAll` (DESIGN.md §3.7).
//!
//! Must contain: `validate_map(rows, meta, opts) -> Validation { ok, errors, warnings, stats }` with every check
//! of the JS in the same order and the same message text (the v1 errors, the `[v2]` contract checks that are
//! warnings until the grid reaches `targets.size`, the flood / distField helpers, `stats.shortcuts` with
//! `detour / dBarred / dOpen`, `stats.route {closed, open}`, region and loot agreement, entry pocket, Source
//! laps) and `validate_all(data) -> ValidationAll { ok, errors, warnings, zones, hub, hub_v1 }`. Tests must
//! reproduce `assets/fixtures/validate_all.json` (see `assets/fixtures/README.md`): same stats numbers, zero
//! errors and zero warnings on the committed maps. `grid::route_cells` and `grid::pass_pred` already exist.
//! Nothing else.
