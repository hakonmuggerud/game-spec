//! LANE: world. The map validator — `maps.js:validateMap` / `validateZone` / `validateAll` (DESIGN.md §3.7).
//!
//! A faithful port: every check of the JS runs in the same order and produces the same message text, so the
//! `errors` / `warnings` lists and the `stats` block can be compared with `assets/fixtures/validate_all.json`
//! (the prototype's own `validateAll()` output). The v1 checks are always errors; the DESIGN.md §3.3/§3.4/§3.6/§3.7
//! contract checks are `[v2] …` warnings while a zone is still on a legacy grid and errors once the grid reaches
//! `targets.size` (or when `strict` is forced). Nothing here panics on bad data: a map that cannot be parsed
//! reports the parse failure as an error and returns.
//!
//! The JS `meta` argument is either a `ZONES[id]` record or `{name, entry: 'F', hub: true}` for the hub;
//! [`MapMeta`] models both (every zone field optional so the hub can leave them out).

use crate::grid::{
    bfs_field, cell_type, idx, in_bounds, is_solid, lap_of_map, los_f64, pass_pred, route_cells,
    Field, DIRS4,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use undercroft_data::cell::{is_legend_char, CreatureKind};
use undercroft_data::map::{deep_neighbourhood, parse_map, row_bytes};
use undercroft_data::tables::ItemKind;
use undercroft_data::zone::{
    Anchor, Bands, CellXY, CreatureSpawn, DeepStyle, EntryKind, Facing, GateDef, Loot, Region,
    ShortcutDef, SpotDef, Targets, ZoneDef,
};
use undercroft_data::{CellKind, GameData, ParsedMap};

/// `maps.js:HUB_ROWS` — the v1 hub (17×9), kept for `validateAll` and tests (the v2 rows are
/// `GameData::hub_rows` / `assets/data/maps/hub.txt`).
pub const HUB_ROWS_V1: [&str; 9] = [
    "#################",
    "#....#.....#....#",
    "#....#.....#....#",
    "#.......F.......#",
    "#....#.....#....#",
    "#....#.....#....#",
    "######.....######",
    "######..S..######",
    "#################",
];

/// The `meta` the validator reads — a `ZONES[id]` record ([`MapMeta::zone`]) or the hub stub
/// ([`MapMeta::hub`]). Optional fields mirror the JS `if (meta.x)` guards.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MapMeta {
    /// `meta.name` (falls back to `id`, then `'map'`, for message prefixes).
    pub name: Option<String>,
    /// `meta.id` — the zone id (`'source'` enables the altar rule).
    pub id: Option<String>,
    /// `meta.hub` / `meta.entry === 'F'`.
    pub hub: bool,
    /// `meta.entry` for zones.
    pub entry: Option<EntryKind>,
    /// `META.size`.
    pub size: Option<i32>,
    pub targets: Option<Targets>,
    pub bands: Option<Bands>,
    pub deep_style: Option<DeepStyle>,
    /// `META.shortcuts` (`SHORTCUTS`).
    pub shortcuts: Vec<ShortcutDef>,
    /// `META.regions` (`REGIONS`).
    pub regions: Vec<Region>,
    /// `META.creatures` (`None` = the key is absent).
    pub creatures: Option<Vec<CreatureSpawn>>,
    /// `META.hunters`.
    pub hunters: Option<Vec<String>>,
    /// `META.npcs` id → cell.
    pub npcs: Option<BTreeMap<String, CellXY>>,
    pub gate: Option<GateDef>,
    pub spots: Option<Vec<SpotDef>>,
    pub loot: Option<Loot>,
    pub anchors: Option<BTreeMap<String, Anchor>>,
}

impl MapMeta {
    /// The `ZONES[id]` record as meta.
    pub fn zone(z: &ZoneDef) -> MapMeta {
        MapMeta {
            name: Some(z.name.clone()),
            id: Some(z.id.clone()),
            hub: false,
            entry: Some(z.entry),
            size: Some(z.size),
            targets: Some(z.targets.clone()),
            bands: z.bands,
            deep_style: Some(z.deep_style),
            shortcuts: z.shortcuts.clone(),
            regions: z.regions.clone(),
            creatures: Some(z.creatures.clone()),
            hunters: Some(z.hunters.clone()),
            npcs: Some(z.npcs.clone()),
            gate: z.gate.clone(),
            spots: Some(z.spots.clone()),
            loot: Some(z.loot),
            anchors: Some(z.anchors.clone()),
        }
    }

    /// `{name, entry: 'F', hub: true}` — the hub stub `validateAll` passes.
    pub fn hub(name: &str) -> MapMeta {
        MapMeta {
            name: Some(name.to_string()),
            hub: true,
            ..MapMeta::default()
        }
    }
}

/// `validateMap`'s options `{size, strict}` plus the `TOOLS` table the gate-tool check reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidateOpts {
    /// Expected grid size for zones (the JS default 40; `validateZone` passes `ZONES[id].size`).
    pub size: i32,
    /// `None` = strict when the grid has reached `targets.size`; `Some(b)` forces it.
    pub strict: Option<bool>,
    /// Known gate tool ids (`config.js:TOOLS` keys).
    pub tools: BTreeSet<String>,
}

impl Default for ValidateOpts {
    fn default() -> Self {
        ValidateOpts {
            size: 40,
            strict: None,
            tools: BTreeSet::new(),
        }
    }
}

impl ValidateOpts {
    /// `validateZone(id, opts)`: `size = ZONES[id].size`, tools from the config.
    pub fn for_zone(z: &ZoneDef, data: &GameData, strict: Option<bool>) -> ValidateOpts {
        ValidateOpts {
            size: z.size,
            strict,
            tools: data.config.tools.keys().cloned().collect(),
        }
    }
}

/// Legend character counts (`stats.S` … `stats.w`), serialised under the JS keys. `w` is the Drowner char
/// count (the JS overwrote its grid-width `w` with it); the grid size is `Stats::width` / `height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CharCounts {
    #[serde(rename = "S")]
    pub s: i32,
    #[serde(rename = "V")]
    pub v: i32,
    #[serde(rename = "A")]
    pub a: i32,
    #[serde(rename = "H")]
    pub h: i32,
    #[serde(rename = "N")]
    pub n: i32,
    #[serde(rename = "C")]
    pub c: i32,
    #[serde(rename = "X")]
    pub x: i32,
    #[serde(rename = "W")]
    pub water: i32,
    #[serde(rename = "o")]
    pub o: i32,
    #[serde(rename = "r")]
    pub r: i32,
    #[serde(rename = "R")]
    pub rich: i32,
    #[serde(rename = "D")]
    pub d: i32,
    #[serde(rename = "P")]
    pub p: i32,
    #[serde(rename = "F")]
    pub f: i32,
    #[serde(rename = "=")]
    pub shortcut: i32,
    #[serde(rename = "L")]
    pub l: i32,
    #[serde(rename = "G")]
    pub g: i32,
    #[serde(rename = "Y")]
    pub y: i32,
    #[serde(rename = "B")]
    pub b: i32,
    #[serde(rename = "w")]
    pub drowner: i32,
}

/// One `stats.shortcuts[]` entry: the door's meta plus the measured detour and the two flank distances.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutStat {
    pub id: String,
    pub name: String,
    pub cells: Vec<CellXY>,
    #[serde(rename = "openFrom")]
    pub open_from: Facing,
    pub saves: i32,
    /// BFS cells from the barred flank to the far flank with the door shut (−1 = not connected).
    pub detour: i32,
    /// BFS cells from the entry to the barred flank (`dBarred`) and to the far flank (`dOpen`), shortcuts shut.
    #[serde(rename = "dBarred")]
    pub d_barred: i32,
    #[serde(rename = "dOpen")]
    pub d_open: i32,
}

/// `stats.route` — the greedy full-clear walk with every shortcut shut (`closed`) and open (`open`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteStat {
    pub closed: i32,
    pub open: i32,
}

/// `stats.creatureLaps[]` (Source only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatureLap {
    pub kind: CreatureKind,
    pub cell: CellXY,
    pub lap: i32,
}

/// The `stats` block of a validation result. Serialises to the JS shape (see `assets/fixtures/README.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub width: i32,
    pub height: i32,
    /// The JS `stats.h` (= `height`; its `w` twin holds the Drowner count, see [`CharCounts`]).
    pub h: i32,
    pub strict: bool,
    #[serde(flatten)]
    pub counts: CharCounts,
    /// Non-`#`/`P` cells (`X` and `=` included — larger than the fixtures' `walkable` by that count).
    #[serde(default)]
    pub walkable: i32,
    #[serde(default)]
    pub walls: i32,
    /// `walls / (w*h)` to 3 decimals (`toFixed(3)`).
    #[serde(rename = "wallShare", default)]
    pub wall_share: f64,
    /// Cells reachable from the spawn with gates and shortcuts open (`w*h − orphans`).
    #[serde(default)]
    pub reachable: i32,
    /// Non-gate cells reachable only once the gate is open.
    #[serde(rename = "gatedCells", default)]
    pub gated_cells: i32,
    /// Cells reachable only through a shortcut (§3.7 check 3 violations).
    #[serde(rename = "shortcutOnlyCells", default)]
    pub shortcut_only_cells: i32,
    /// `kind@(cx,cz)` / `npc@(cx,cz)` for every item and NPC behind the gate.
    #[serde(rename = "behindGate", default)]
    pub behind_gate: Vec<String>,
    /// Non-solid cells in no region (only present when > 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unassigned: Option<i32>,
    #[serde(default)]
    pub shortcuts: Vec<ShortcutStat>,
    /// Zones only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<RouteStat>,
    /// Source only.
    #[serde(rename = "altarLap", default, skip_serializing_if = "Option::is_none")]
    pub altar_lap: Option<i32>,
    #[serde(
        rename = "creatureLaps",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub creature_laps: Option<Vec<CreatureLap>>,
    #[serde(
        rename = "hunterLaps",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub hunter_laps: Option<Vec<i32>>,
}

/// `validateMap` result `{ok, errors, warnings, stats}`; `stats` is `None` when the rows failed the shape checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Validation {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub stats: Option<Stats>,
}

/// `validateAll` result. `hub` is the v2 hub (`maps.js` `hubV2`, message prefix `hub-v2`) and `hub_v1` the
/// legacy rows (`maps.js` `hub`); `errors` / `warnings` aggregate zones, then v1 hub, then v2 hub like the JS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationAll {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// In `ZONE_ORDER`.
    pub zones: Vec<(String, Validation)>,
    pub hub: Validation,
    pub hub_v1: Validation,
}

impl ValidationAll {
    /// A zone's result by id.
    pub fn zone(&self, id: &str) -> Option<&Validation> {
        self.zones.iter().find(|(z, _)| z == id).map(|(_, v)| v)
    }
}

/* ============================================================
Helpers
============================================================ */

/// `solidType(t)` — wall or pillar only (gates and shortcuts are handled by `passPred`).
fn solid_type(t: CellKind) -> bool {
    matches!(t, CellKind::Wall | CellKind::Pillar)
}

/// `rows[z][x]` with the JS `undefined` for anything off the grid (negative included).
fn ch_at(grid: &[Vec<u8>], x: i32, z: i32) -> Option<u8> {
    if x < 0 || z < 0 {
        return None;
    }
    grid.get(z as usize)
        .and_then(|r| r.get(x as usize))
        .copied()
}

/// `flood(m, sx, sz, gatesOpen, scOpen)` as a reachability bitmap (the JS uses a DFS; the set is the same).
fn flood(m: &ParsedMap, sx: i32, sz: i32, gates_open: bool, sc_open: bool) -> Vec<bool> {
    dist_field(m, sx, sz, gates_open, sc_open)
        .dist
        .iter()
        .map(|&d| d >= 0)
        .collect()
}

/// `distField(m, cx, cz, gatesOpen, scOpen)`.
fn dist_field(m: &ParsedMap, cx: i32, cz: i32, gates_open: bool, sc_open: bool) -> Field {
    let blocked = pass_pred(gates_open, sc_open);
    bfs_field(m, cx, cz, |x, z| blocked(m, x, z))
}

/// `field.dist[idx]` with −1 for a cell outside the grid.
fn dist_at(m: &ParsedMap, f: &Field, cx: i32, cz: i32) -> i32 {
    if in_bounds(m, cx, cz) {
        f.dist[idx(m, cx, cz)] as i32
    } else {
        -1
    }
}

/// `+(x).toFixed(3)` — round to 3 decimals the way JS does (exact decimal expansion, ties away from zero for
/// positive values) and re-parse, so the value equals the JSON fixture's.
fn to_fixed3(x: f64) -> f64 {
    let long = format!("{x:.40}");
    let rounded = match long.split_once('.') {
        Some((int, frac))
            if frac.len() > 3
                && frac.as_bytes()[3] == b'5'
                && frac[4..].bytes().all(|b| b == b'0') =>
        {
            // exact tie: JS picks the larger n
            let n: u64 = frac[..3].parse().unwrap_or(0) + 1;
            let int: u64 = int.parse().unwrap_or(0);
            let (int, n) = if n == 1000 { (int + 1, 0) } else { (int, n) };
            format!("{int}.{n:03}")
        }
        _ => format!("{x:.3}"),
    };
    rounded.parse().unwrap_or(x)
}

/// `Facing` as the JS letter.
fn facing_letter(f: Facing) -> &'static str {
    match f {
        Facing::N => "N",
        Facing::E => "E",
        Facing::S => "S",
        Facing::W => "W",
    }
}

/* ============================================================
validateMap
============================================================ */

/// `maps.js:validateMap(rows, meta, {size, strict})`.
pub fn validate_map(rows: &[String], meta: &MapMeta, opts: &ValidateOpts) -> Validation {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let name = meta
        .name
        .clone()
        .or_else(|| meta.id.clone())
        .unwrap_or_else(|| "map".to_string());
    let grid = row_bytes(rows);
    let h = grid.len() as i32;
    let w = grid.first().map(|r| r.len() as i32).unwrap_or(0);
    let is_hub = meta.hub;
    let targets = meta.targets.as_ref();
    let is_strict = match opts.strict {
        None => targets.is_some_and(|t| h == t.size && w == t.size),
        Some(s) => s,
    };
    // message sinks: err / warn / v2 (error when strict, warning otherwise)
    macro_rules! err {
        ($($arg:tt)*) => { errors.push(format!("{name}: {}", format!($($arg)*))) };
    }
    macro_rules! warn {
        ($($arg:tt)*) => { warnings.push(format!("{name}: {}", format!($($arg)*))) };
    }
    macro_rules! v2 {
        ($($arg:tt)*) => {{
            let s = format!("{name}: [v2] {}", format!($($arg)*));
            if is_strict { errors.push(s) } else { warnings.push(s) }
        }};
    }

    if !is_hub && (w != opts.size || h != opts.size) {
        err!("expected {}×{}, got {w}×{h}", opts.size, opts.size);
    }
    for (z, r) in grid.iter().enumerate() {
        if r.len() as i32 != w {
            err!("row {z} has length {}, expected {w}", r.len());
        }
    }
    for (z, r) in grid.iter().enumerate() {
        for (x, &b) in r.iter().enumerate() {
            if !is_legend_char(b as char) {
                err!("unknown char '{}' at ({x},{z})", b as char);
            }
        }
    }
    for (z, r) in grid.iter().enumerate() {
        for (x, &b) in r.iter().enumerate() {
            let (x, z) = (x as i32, z as i32);
            if (z == 0 || z == h - 1 || x == 0 || x == w - 1) && b != b'#' {
                err!("border not wall at ({x},{z})");
            }
        }
    }
    if !errors.is_empty() {
        return Validation {
            ok: false,
            errors,
            warnings,
            stats: None,
        };
    }

    let m = match parse_map(rows, 0, &name, meta.bands, Some(meta.shortcuts.as_slice())) {
        Ok(m) => m,
        Err(e) => {
            // the JS would throw here (empty rows); report instead
            err!("cannot parse: {e}");
            return Validation {
                ok: false,
                errors,
                warnings,
                stats: None,
            };
        }
    };
    let count = |ch: u8| -> i32 {
        grid.iter()
            .map(|r| r.iter().filter(|&&b| b == ch).count() as i32)
            .sum()
    };
    let counts = CharCounts {
        s: count(b'S'),
        v: count(b'V'),
        a: count(b'A'),
        h: count(b'H'),
        n: count(b'N'),
        c: count(b'C'),
        x: count(b'X'),
        water: count(b'W'),
        o: count(b'o'),
        r: count(b'r'),
        rich: count(b'R'),
        d: count(b'D'),
        p: count(b'P'),
        f: count(b'F'),
        shortcut: count(b'='),
        l: count(b'L'),
        g: count(b'G'),
        y: count(b'Y'),
        b: count(b'B'),
        drowner: count(b'w'),
    };
    let mut stats = Stats {
        width: w,
        height: h,
        h,
        strict: is_strict,
        counts,
        walkable: 0,
        walls: 0,
        wall_share: 0.0,
        reachable: 0,
        gated_cells: 0,
        shortcut_only_cells: 0,
        behind_gate: Vec::new(),
        unassigned: None,
        shortcuts: Vec::new(),
        route: None,
        altar_lap: None,
        creature_laps: None,
        hunter_laps: None,
    };
    let spawns = counts.s + counts.v;
    if is_hub {
        if counts.f != 1 {
            err!("hub needs exactly one F, has {}", counts.f);
        }
        if counts.s != 1 {
            err!("hub needs exactly one S, has {}", counts.s);
        }
    } else {
        if spawns != 1 {
            err!("needs exactly one spawn (S or V), has {spawns}");
        } else if let Some(entry) = meta.entry {
            let bad = match entry {
                EntryKind::Stairs => counts.s != 1,
                EntryKind::Elevator => counts.v != 1,
            };
            if bad {
                err!(
                    "entry should be {}",
                    match entry {
                        EntryKind::Stairs => "S",
                        EntryKind::Elevator => "V",
                    }
                );
            }
        }
        let is_source = meta.id.as_deref() == Some("source");
        if (is_source && counts.a != 1) || (!is_source && counts.a != 0) {
            err!("altar count {} (Source needs 1, others 0)", counts.a);
        }
    }
    let stairs = match &m.stairs {
        Some(s) => s.marker,
        None => {
            err!("no spawn cell");
            return Validation {
                ok: false,
                errors,
                warnings,
                stats: Some(stats),
            };
        }
    };

    /* ---- 1. size / walkable / wall share (DESIGN.md §3.3) ---- */
    let (mut walkable, mut walls) = (0i32, 0i32);
    for r in &grid {
        for &b in r {
            if b == b'#' {
                walls += 1;
            } else if b != b'P' {
                walkable += 1;
            }
        }
    }
    stats.walkable = walkable;
    stats.walls = walls;
    stats.wall_share = to_fixed3(walls as f64 / (w as f64 * h as f64));
    if !is_hub {
        if let Some(size) = meta.size {
            if w != size || h != size {
                err!("meta.size is {size} but the rows are {w}×{h}");
            }
        }
    }
    if let Some(t) = targets {
        if w != t.size || h != t.size {
            v2!(
                "grid is {w}×{h}; §3.3 target {}×{} (walkable {walkable} → {} ±5 %, route {}+)",
                t.size,
                t.size,
                t.walkable,
                t.route
            );
        } else {
            let lo = t.walkable as f64 * 0.95;
            let hi = t.walkable as f64 * 1.05;
            if (walkable as f64) < lo || (walkable as f64) > hi {
                v2!(
                    "walkable {walkable} outside the §3.3 band {}–{}",
                    js_round(lo),
                    js_round(hi)
                );
            }
            if (stats.wall_share - t.wall_share as f64).abs() > 0.05 {
                v2!(
                    "wall share {} % is more than 5 pts off the §3.3 {} %",
                    js_to_fixed1(stats.wall_share * 100.0),
                    js_to_fixed1(t.wall_share as f64 * 100.0)
                );
            }
        }
    }

    /* ---- connectivity: `open` = gates open + every shortcut CLOSED (first-run topology, §3.7 check 3);
    `all` = gates and shortcuts open (full connectivity, check 4); `closed` = nothing open ---- */
    let open = flood(&m, stairs.cx, stairs.cz, true, false);
    let all = flood(&m, stairs.cx, stairs.cz, true, true);
    let closed = flood(&m, stairs.cx, stairs.cz, false, false);
    let (mut orphans, mut gated_cells, mut need_shortcut) = (0i32, 0i32, 0i32);
    let mut orphan_list: Vec<String> = Vec::new();
    for z in 0..h {
        for x in 0..w {
            let i = idx(&m, x, z);
            let t = m.cells[i];
            if solid_type(t) {
                continue;
            }
            if t == CellKind::Shortcut {
                continue; // barred: solid until opened (its flanks are checked below)
            }
            if !all[i] {
                orphans += 1;
                if orphan_list.len() < 8 {
                    orphan_list.push(format!("({x},{z})"));
                }
            } else if !open[i] {
                need_shortcut += 1; // reachable only through a shortcut → check 3 violation
            } else if !closed[i] && t != CellKind::Gate {
                gated_cells += 1;
            }
        }
    }
    if orphans > 0 {
        err!(
            "{orphans} walkable cell(s) unreachable from spawn even with gates and shortcuts open: {}{}",
            orphan_list.join(" "),
            if orphans > 8 { " …" } else { "" }
        );
    }
    if need_shortcut > 0 {
        v2!("{need_shortcut} cell(s) reachable only through a shortcut — nothing in the zone may need one (§3.7 check 3)");
    }
    stats.reachable = w * h - orphans;
    stats.gated_cells = gated_cells;
    stats.shortcut_only_cells = need_shortcut;
    {
        let mut reach = |label: &str, list: &[(i32, i32)], need_closed: bool| {
            for &(cx, cz) in list {
                let i = idx(&m, cx, cz);
                if !open[i] {
                    err!("{label} at ({cx},{cz}) unreachable");
                } else if need_closed && !closed[i] {
                    err!("{label} at ({cx},{cz}) is behind a gate");
                }
            }
        };
        let items: Vec<(i32, i32)> = m.items.iter().map(|i| (i.cx, i.cz)).collect();
        let npcs: Vec<(i32, i32)> = m.npc_cells.iter().map(|n| (n.cx, n.cz)).collect();
        let spots: Vec<(i32, i32)> = m.spots.iter().map(|s| (s.marker.cx, s.marker.cz)).collect();
        let hunters: Vec<(i32, i32)> = m.hunter_spawns.iter().map(|s| (s.cx, s.cz)).collect();
        reach("item", &items, false);
        reach("npc", &npcs, false);
        reach("spot", &spots, false);
        reach("hunter spawn", &hunters, true); // hunters do not open gates
        if let Some(a) = &m.altar {
            reach("altar", &[(a.cx, a.cz)], true);
        }
    }
    // creatures (DESIGN.md §5.7): reachable from the spawn with the gates closed unless the meta says gateOk (a
    // Warden may guard a gated pocket); G in a deep neighbourhood; w in a water body; Y visible from some
    // floor/door cell ≤ 6 u.
    let cmeta: &[CreatureSpawn] = meta.creatures.as_deref().unwrap_or(&[]);
    for (i, c) in m.creatures.iter().enumerate() {
        let (cx, cz, ci) = (c.marker.cx, c.marker.cz, c.marker.idx);
        let gate_ok = cmeta.get(i).is_some_and(|o| o.gate_ok);
        let label = format!("creature {}", c.kind.js_name());
        if !open[ci] {
            err!("{label} at ({cx},{cz}) unreachable");
        } else if !gate_ok && !closed[ci] {
            err!("{label} at ({cx},{cz}) is behind a gate (set gateOk to allow)");
        }
        if c.kind == CreatureKind::Warden && !deep_neighbourhood(&grid, cx as usize, cz as usize) {
            err!("{label} at ({cx},{cz}) is not in a deep pocket");
        }
        if c.kind == CreatureKind::Drowner {
            let wet = DIRS4
                .iter()
                .any(|&(dx, dz)| cell_type(&m, cx + dx, cz + dz) == CellKind::Water);
            if m.cells[ci] != CellKind::Water || !wet {
                err!("{label} at ({cx},{cz}) is not in a water body");
            }
        }
        if c.kind == CreatureKind::FalseLight {
            let mut seen = false;
            'scan: for z in (cz - 6).max(0)..=(cz + 6).min(h - 1) {
                for x in (cx - 6).max(0)..=(cx + 6).min(w - 1) {
                    let ch = grid[z as usize][x as usize];
                    if ch != b'.' && ch != b'D' {
                        continue;
                    }
                    let d = ((x - cx) as f64).hypot((z - cz) as f64);
                    if d <= 6.0
                        && los_f64(
                            &m,
                            x as f64 + 0.5,
                            z as f64 + 0.5,
                            cx as f64 + 0.5,
                            cz as f64 + 0.5,
                        )
                    {
                        seen = true;
                        break 'scan;
                    }
                }
            }
            if !seen {
                err!("{label} at ({cx},{cz}) has no floor/deep cell with LOS within 6 u (it must be seen to work)");
            }
        }
    }
    for g in &m.gates {
        let (gx, gz) = (g.marker.cx, g.marker.cz);
        let ns = is_solid(&m, gx, gz - 1) && is_solid(&m, gx, gz + 1);
        let ew = is_solid(&m, gx - 1, gz) && is_solid(&m, gx + 1, gz);
        if !ns && !ew {
            err!("gate at ({gx},{gz}) is not set in a wall line");
        }
        if ns && (is_solid(&m, gx - 1, gz) || is_solid(&m, gx + 1, gz)) {
            v2!("gate at ({gx},{gz}) has a solid flank on its open axis");
        }
        if ew && (is_solid(&m, gx, gz - 1) || is_solid(&m, gx, gz + 1)) {
            v2!("gate at ({gx},{gz}) has a solid flank on its open axis");
        }
        if !open[g.marker.idx] {
            err!("gate at ({gx},{gz}) unreachable");
        }
    }
    if !m.gates.is_empty() && gated_cells == 0 {
        warn!("gates seal nothing (every cell reachable with them closed)");
    }
    let mut behind: Vec<String> = Vec::new();
    for it in &m.items {
        let i = idx(&m, it.cx, it.cz);
        if open[i] && !closed[i] {
            behind.push(format!("{}@({},{})", item_name(it.kind), it.cx, it.cz));
        }
    }
    for n in &m.npc_cells {
        if open[n.idx] && !closed[n.idx] {
            behind.push(format!("npc@({},{})", n.cx, n.cz));
        }
    }
    stats.behind_gate = behind;

    /* ---- 2. markers on legal cells (§3.7 check 2) ---- */
    let open_nbrs = |x: i32, z: i32| -> Vec<(i32, i32)> {
        DIRS4
            .iter()
            .map(|&(dx, dz)| (x + dx, z + dz))
            .filter(|&(cx, cz)| in_bounds(&m, cx, cz) && !is_solid(&m, cx, cz))
            .collect()
    };
    let marooned = |x: i32, z: i32| -> bool {
        let n = open_nbrs(x, z);
        n.is_empty()
            || n.iter()
                .all(|&(cx, cz)| cell_type(&m, cx, cz) == CellKind::Water)
    };
    for z in 0..h {
        for x in 0..w {
            let ch = grid[z as usize][x as usize];
            if ch == b'o' || ch == b'r' {
                if marooned(x, z) {
                    v2!(
                        "item '{}' at ({x},{z}) is not on dry floor/deep (§3.4: o and r sit on '.' or 'D')",
                        ch as char
                    );
                }
            } else if ch == b'R' {
                let n = open_nbrs(x, z);
                let wet = !n.is_empty()
                    && n.iter()
                        .all(|&(cx, cz)| cell_type(&m, cx, cz) == CellKind::Water);
                if !deep_neighbourhood(&grid, x as usize, z as usize) && !wet {
                    v2!("rich relic 'R' at ({x},{z}) is neither in a deep pocket nor standing in a flooded vault");
                }
            } else if b"NCHLGYB".contains(&ch) {
                if marooned(x, z) {
                    v2!("marker '{}' at ({x},{z}) is not on floor/deep", ch as char);
                }
            } else if ch == b'w'
                && !DIRS4
                    .iter()
                    .any(|&(dx, dz)| cell_type(&m, x + dx, z + dz) == CellKind::Water)
            {
                v2!("'w' at ({x},{z}) is not in a water body");
            }
        }
    }

    /* ---- 5. shortcuts (§3.6 data + §3.7 check 5) ---- */
    let sc_meta = &meta.shortcuts;
    let regions = &meta.regions;
    let mut meta_cells: BTreeSet<CellXY> = BTreeSet::new();
    let mut sc_ids: BTreeSet<&str> = BTreeSet::new();
    let mut sc_stats: Vec<ShortcutStat> = Vec::new();
    let floor_d = if meta.deep_style == Some(DeepStyle::Bands) {
        100
    } else {
        40
    };
    for e in sc_meta {
        if e.id.is_empty() {
            err!("a SHORTCUTS entry has no id");
            continue;
        }
        if sc_ids.contains(e.id.as_str()) {
            err!("duplicate shortcut id '{}'", e.id);
        }
        sc_ids.insert(&e.id);
        if regions.iter().any(|r| r.id == e.id) {
            err!("shortcut id '{}' collides with a region id", e.id);
        }
        if e.cells.is_empty() || e.cells.len() > 2 {
            err!("shortcut '{}' must list 1 or 2 cells", e.id);
            continue;
        }
        // `openFrom` is a `Facing` here, so the JS "is not N/E/S/W" error cannot arise
        if e.cells.len() == 2
            && (e.cells[0][0] - e.cells[1][0]).abs() + (e.cells[0][1] - e.cells[1][1]).abs() != 1
        {
            err!("shortcut '{}' cells are not adjacent", e.id);
        }
        let mut ok = true;
        for c in &e.cells {
            meta_cells.insert(*c);
            if ch_at(&grid, c[0], c[1]) != Some(b'=') {
                err!(
                    "shortcut '{}' lists ({},{}) but there is no '=' there",
                    e.id,
                    c[0],
                    c[1]
                );
                ok = false;
            }
        }
        if !ok {
            continue;
        }
        // geometry: wall line, the two flanks, which side is farther from the entry, and the detour it removes
        let [dx, dz] = e.open_from.delta();
        let c0 = e.cells[0];
        let of_ = (c0[0] + dx, c0[1] + dz);
        let bf = (c0[0] - dx, c0[1] - dz);
        let perp: [(i32, i32); 2] = if dx != 0 {
            [(0, -1), (0, 1)]
        } else {
            [(-1, 0), (1, 0)]
        };
        for c in &e.cells {
            let in_line = perp.iter().all(|&(px, pz)| {
                is_solid(&m, c[0] + px, c[1] + pz)
                    || e.cells
                        .iter()
                        .any(|o| o[0] == c[0] + px && o[1] == c[1] + pz)
            });
            if !in_line {
                v2!(
                    "shortcut '{}' cell ({},{}) is not set in a wall line",
                    e.id,
                    c[0],
                    c[1]
                );
            }
        }
        let open_ok = !is_solid(&m, of_.0, of_.1);
        let barred_ok = !is_solid(&m, bf.0, bf.1);
        if !open_ok || !barred_ok {
            v2!(
                "shortcut '{}' flanks are not both open (open {open_ok}, barred {barred_ok})",
                e.id
            );
        } else {
            let f = dist_field(&m, stairs.cx, stairs.cz, true, false);
            let d_bar = dist_at(&m, &f, bf.0, bf.1);
            let d_open = dist_at(&m, &f, of_.0, of_.1);
            if d_bar < 0 {
                v2!(
                    "shortcut '{}' barred flank ({},{}) is not reachable with every shortcut shut",
                    e.id,
                    bf.0,
                    bf.1
                );
            }
            if d_open < 0 {
                v2!(
                    "shortcut '{}' far flank ({},{}) is not reachable with every shortcut shut (§3.7 check 3)",
                    e.id,
                    of_.0,
                    of_.1
                );
            }
            if d_bar >= 0 && d_open >= 0 && d_open <= d_bar {
                v2!(
                    "shortcut '{}' openFrom '{}' is the NEARER side ({d_open} vs {d_bar} cells from the entry) — bar the near side",
                    e.id,
                    facing_letter(e.open_from)
                );
            }
            let g = dist_field(&m, bf.0, bf.1, true, false);
            let detour = dist_at(&m, &g, of_.0, of_.1);
            if detour < 0 {
                v2!("shortcut '{}' flanks are not connected with it shut", e.id);
            } else if detour < floor_d {
                v2!(
                    "shortcut '{}' removes a detour of only {detour} cells (floor {floor_d})",
                    e.id
                );
            }
            sc_stats.push(ShortcutStat {
                id: e.id.clone(),
                name: e.name.clone(),
                cells: e.cells.clone(),
                open_from: e.open_from,
                saves: e.saves,
                detour,
                d_barred: d_bar,
                d_open,
            });
            if meta.deep_style == Some(DeepStyle::Bands) {
                let lb = lap_of_map(&m, bf.0, bf.1);
                let lo2 = lap_of_map(&m, of_.0, of_.1);
                if (lo2 - lb).abs() != 1 {
                    v2!(
                        "shortcut '{}' joins lap {lb} to lap {lo2} — a fissure must join L to L+1 (§3.6)",
                        e.id
                    );
                }
            }
        }
    }
    for s in &m.shortcuts {
        if !meta_cells.contains(&s.marker.cell()) {
            err!(
                "'=' at ({},{}) is in no SHORTCUTS entry (world.loadZone could not bind it)",
                s.marker.cx,
                s.marker.cz
            );
        }
    }
    stats.shortcuts = sc_stats;

    /* ---- 6/7. regions and deep pockets (§3.7 checks 6, 7) ---- */
    if !is_hub && regions.is_empty() {
        v2!("no REGIONS declared (DESIGN.md §3.4)");
    }
    if !regions.is_empty() {
        let mut owner: Vec<i32> = vec![-1; (w * h) as usize];
        for (ri, r) in regions.iter().enumerate() {
            let (x, z) = (r.x, r.z);
            if !(x[0] >= 0 && x[1] < w && z[0] >= 0 && z[1] < h && x[0] <= x[1] && z[0] <= z[1]) {
                err!(
                    "region '{}' extent x[{},{}] z[{},{}] is not inside the grid",
                    r.id,
                    x[0],
                    x[1],
                    z[0],
                    z[1]
                );
                continue;
            }
            for cz in z[0]..=z[1] {
                for cx in x[0]..=x[1] {
                    let i = idx(&m, cx, cz);
                    if owner[i] >= 0 {
                        err!(
                            "regions '{}' and '{}' overlap at ({cx},{cz})",
                            regions[owner[i] as usize].id,
                            r.id
                        );
                    } else {
                        owner[i] = ri as i32;
                    }
                }
            }
        }
        let mut unassigned_list: Vec<String> = Vec::new();
        let mut unassigned = 0i32;
        for cz in 0..h {
            for cx in 0..w {
                let i = idx(&m, cx, cz);
                if solid_type(m.cells[i]) || owner[i] >= 0 {
                    continue;
                }
                if unassigned_list.len() < 8 {
                    unassigned_list.push(format!("({cx},{cz})"));
                }
                unassigned += 1;
            }
        }
        if unassigned > 0 {
            stats.unassigned = Some(unassigned);
            v2!(
                "{unassigned} non-solid cell(s) belong to no region: {}{}",
                unassigned_list.join(" "),
                if unassigned > 8 { " …" } else { "" }
            );
        }
        for r in regions {
            let (x, z) = (r.x, r.z);
            let mut chars = Loot::default();
            let (mut cells, mut deep, mut entrances) = (0i32, 0i32, 0i32);
            for cz in z[0]..=z[1] {
                for cx in x[0]..=x[1] {
                    let ch = ch_at(&grid, cx, cz);
                    match ch {
                        Some(b'o') => chars.oil += 1,
                        Some(b'r') => chars.relic += 1,
                        Some(b'R') => chars.rich += 1,
                        _ => {}
                    }
                    if ch == Some(b'#') || ch == Some(b'P') {
                        continue;
                    }
                    cells += 1;
                    if ch == Some(b'D') || ch == Some(b'R') {
                        deep += 1;
                    }
                    // an entrance = a non-solid cell of this region with a non-solid neighbour outside it
                    let exit = DIRS4.iter().any(|&(dx, dz)| {
                        let (nx, nz) = (cx + dx, cz + dz);
                        in_bounds(&m, nx, nz)
                            && !is_solid(&m, nx, nz)
                            && (nx < x[0] || nx > x[1] || nz < z[0] || nz > z[1])
                    });
                    if exit {
                        entrances += 1;
                    }
                }
            }
            if let Some(loot) = &r.loot {
                for (k, want, got) in [
                    ("oil", loot.oil, chars.oil),
                    ("relic", loot.relic, chars.relic),
                    ("rich", loot.rich, chars.rich),
                ] {
                    if want != got {
                        v2!(
                            "region '{}' declares {want} {k} but its extent holds {got}",
                            r.id
                        );
                    }
                }
            }
            if r.deep {
                if cells == 0 || (deep as f64) / (cells as f64) < 0.6 {
                    let pct = if cells > 0 {
                        js_round(100.0 * deep as f64 / cells as f64)
                    } else {
                        0
                    };
                    v2!("deep region '{}' is {pct} % D (needs ≥ 60 %)", r.id);
                }
                if entrances == 0 {
                    v2!("deep region '{}' has no non-solid entrance", r.id);
                }
                'deep: for cz in z[0]..=z[1] {
                    for cx in x[0]..=x[1] {
                        let ch = ch_at(&grid, cx, cz);
                        if ch == Some(b'#') || ch == Some(b'P') || ch == Some(b'=') {
                            continue;
                        }
                        let in_deep = cx >= 0
                            && cz >= 0
                            && deep_neighbourhood(&grid, cx as usize, cz as usize);
                        if !in_deep {
                            v2!(
                                "deep region '{}' cell ({cx},{cz}) is not in a deep neighbourhood (the lamp would not shrink)",
                                r.id
                            );
                            break 'deep;
                        }
                    }
                }
            }
        }
    }

    /* ---- 8. route length (§3.3 / §3.7 check 8) ---- */
    if !is_hub {
        let rc = route_cells(&m, true, false);
        let mut route = RouteStat {
            closed: rc.cells,
            open: rc.cells,
        };
        if !m.shortcuts.is_empty() {
            route.open = route_cells(&m, true, true).cells;
        }
        if let Some(t) = targets {
            if t.route > 0 && rc.cells < t.route {
                v2!(
                    "full-clear route is {} cells with every shortcut shut; §3.3 target ≥ {}",
                    rc.cells,
                    t.route
                );
            }
        }
        if !m.shortcuts.is_empty() && (route.open as f64) > rc.cells as f64 * 0.7 {
            v2!(
                "with every shortcut open the route is {} cells = {} % of {} (§4 wants ≤ 70 %)",
                route.open,
                js_round(100.0 * route.open as f64 / rc.cells as f64),
                rc.cells
            );
        }
        stats.route = Some(route);
    }

    /* ---- 9. entry pocket (§3.7 check 9) ---- */
    if !is_hub {
        let free = open_nbrs(stairs.cx, stairs.cz).len();
        if free < 3 {
            v2!(
                "entry ({},{}) has {free} free 4-neighbour(s), needs ≥ 3",
                stairs.cx,
                stairs.cz
            );
        }
        let ef = dist_field(&m, stairs.cx, stairs.cz, true, false);
        let spawns_iter = m
            .hunter_spawns
            .iter()
            .map(|s| ("hunter", s.cx, s.cz))
            .chain(
                m.creatures
                    .iter()
                    .map(|c| (c.kind.js_name(), c.marker.cx, c.marker.cz)),
            );
        for (kind, cx, cz) in spawns_iter {
            let d = dist_at(&m, &ef, cx, cz);
            if (0..10).contains(&d) {
                v2!("{kind} spawn at ({cx},{cz}) is {d} BFS cells from the entry (needs ≥ 10)");
            }
        }
    }

    /* ---- 10. Source laps (§3.4 / §3.7 check 10) ---- */
    if meta.deep_style == Some(DeepStyle::Bands) {
        if let Some(a) = &m.altar {
            let lap = lap_of_map(&m, a.cx, a.cz);
            if lap != 5 {
                v2!("altar at lap {lap}, expected 5");
            }
            stats.altar_lap = Some(lap);
            stats.creature_laps = Some(
                m.creatures
                    .iter()
                    .map(|c| CreatureLap {
                        kind: c.kind,
                        cell: c.marker.cell(),
                        lap: lap_of_map(&m, c.marker.cx, c.marker.cz),
                    })
                    .collect(),
            );
            stats.hunter_laps = Some(
                m.hunter_spawns
                    .iter()
                    .map(|s| lap_of_map(&m, s.cx, s.cz))
                    .collect(),
            );
        }
    }

    /* ---- metadata agreement ---- */
    if let Some(hunters) = &meta.hunters {
        if hunters.len() != m.hunter_spawns.len() {
            err!(
                "meta.hunters has {} entries, map has {} H",
                hunters.len(),
                m.hunter_spawns.len()
            );
        }
    }
    if meta.creatures.is_some() || !m.creatures.is_empty() {
        let want: &[CreatureSpawn] = meta.creatures.as_deref().unwrap_or(&[]);
        if want.len() != m.creatures.len() {
            err!(
                "meta.creatures has {} entries, map has {} creature cells",
                want.len(),
                m.creatures.len()
            );
        }
        for (i, c) in m.creatures.iter().enumerate() {
            if let Some(o) = want.get(i) {
                if o.kind != c.kind {
                    err!(
                        "creature {i} at ({},{}) is {}, meta says {}",
                        c.marker.cx,
                        c.marker.cz,
                        c.kind.js_name(),
                        o.kind.js_name()
                    );
                }
            }
        }
        // `facing` is a `Facing` here, so the JS "warden facing is not N/E/S/W" error cannot arise
    }
    if let Some(npcs) = &meta.npcs {
        if npcs.len() != m.npc_cells.len() {
            err!(
                "meta.npcs names {} NPC(s), map has {} N",
                npcs.len(),
                m.npc_cells.len()
            );
        }
        for (id, c) in npcs {
            if !m.npc_cells.iter().any(|n| n.cell() == *c) {
                err!("npc {id} expected at ({},{}), no N there", c[0], c[1]);
            }
        }
    }
    if let Some(gate) = &meta.gate {
        if gate.cells.len() != m.gates.len() {
            err!(
                "meta.gate has {} cell(s), map has {} X",
                gate.cells.len(),
                m.gates.len()
            );
        }
        for c in &gate.cells {
            if !m.gates.iter().any(|g| g.marker.cell() == *c) {
                err!("gate expected at ({},{}), no X there", c[0], c[1]);
            }
        }
        if !opts.tools.contains(&gate.tool) {
            err!("unknown gate tool '{}'", gate.tool);
        }
    } else if !m.gates.is_empty() {
        err!("map has {} X but meta.gate is null", m.gates.len());
    }
    if let Some(spots) = &meta.spots {
        if spots.len() != m.spots.len() {
            err!(
                "meta.spots has {}, map has {} C",
                spots.len(),
                m.spots.len()
            );
        }
        for (i, s) in spots.iter().enumerate() {
            let c = m.spots.get(i);
            if c.is_none_or(|c| c.marker.cell() != s.cell) {
                err!(
                    "spot {i} expected at ({},{}), map spot {i} is {}",
                    s.cell[0],
                    s.cell[1],
                    match c {
                        Some(c) => format!("({},{})", c.marker.cx, c.marker.cz),
                        None => "missing".to_string(),
                    }
                );
            }
        }
    }
    if let Some(loot) = &meta.loot {
        for (k, want, got) in [
            ("oil", loot.oil as i32, counts.o),
            ("relic", loot.relic as i32, counts.r),
            ("rich", loot.rich as i32, counts.rich),
        ] {
            if want != got {
                err!("loot.{k}: meta says {want}, map has {got}");
            }
        }
    }
    if let Some(anchors) = &meta.anchors {
        // ANCHORS is the test contract: every anchor must be a real cell inside the grid (suites teleport to cx+0.5)
        let mut flat: Vec<(String, CellXY)> = Vec::new();
        for (k, v) in anchors {
            v.flatten(k, &mut flat);
        }
        for (path, c) in &flat {
            if !in_bounds(&m, c[0], c[1]) {
                err!("anchor {path} ({},{}) is outside the grid", c[0], c[1]);
            }
        }
        if let Some(Anchor::Cell(e)) = anchors.get("entry") {
            if *e != stairs.cell() {
                err!(
                    "anchors.entry ({},{}) is not the spawn cell ({},{})",
                    e[0],
                    e[1],
                    stairs.cx,
                    stairs.cz
                );
            }
        }
    }
    Validation {
        ok: errors.is_empty(),
        errors,
        warnings,
        stats: Some(stats),
    }
}

/// `Math.round` — half away from zero for positives (Rust `round` matches for the values used here).
fn js_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

/// `(x).toFixed(1)` for the wall-share message.
fn js_to_fixed1(x: f64) -> String {
    format!("{x:.1}")
}

/// `ItemKind` as the JS `kind` string (only the three map-spawned kinds occur here).
fn item_name(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Oil => "oil",
        ItemKind::Relic => "relic",
        ItemKind::Rich => "rich",
        ItemKind::Quest => "quest",
        ItemKind::Bundle => "bundle",
    }
}

/// `maps.js:validateZone(id, opts)` — a zone with its own meta and size.
pub fn validate_zone(data: &GameData, zone: &ZoneDef, strict: Option<bool>) -> Validation {
    let opts = ValidateOpts::for_zone(zone, data, strict);
    validate_map(&zone.rows, &MapMeta::zone(zone), &opts)
}

/// `maps.js:validateAll(opts)` — every zone in `ZONE_ORDER`, then the v1 hub and the v2 hub.
pub fn validate_all(data: &GameData, strict: Option<bool>) -> ValidationAll {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut zones = Vec::new();
    for z in &data.zones {
        let r = validate_zone(data, z, strict);
        errors.extend(r.errors.iter().cloned());
        warnings.extend(r.warnings.iter().cloned());
        zones.push((z.id.clone(), r));
    }
    let hub_opts = ValidateOpts::default();
    let v1_rows: Vec<String> = HUB_ROWS_V1.iter().map(|r| r.to_string()).collect();
    let hub_v1 = validate_map(&v1_rows, &MapMeta::hub("hub"), &hub_opts);
    let hub = validate_map(&data.hub_rows, &MapMeta::hub("hub-v2"), &hub_opts);
    for r in [&hub_v1, &hub] {
        errors.extend(r.errors.iter().cloned());
        warnings.extend(r.warnings.iter().cloned());
    }
    ValidationAll {
        ok: errors.is_empty(),
        errors,
        warnings,
        zones,
        hub,
        hub_v1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{load_map_fixture, load_validate_all};

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    fn rows(s: &[&str]) -> Vec<String> {
        s.iter().map(|r| r.to_string()).collect()
    }

    fn fixture_result(v: &serde_json::Value) -> Validation {
        serde_json::from_value(v.clone()).expect("fixture result parses")
    }

    #[test]
    fn to_fixed3_matches_js() {
        assert_eq!(to_fixed3(1354.0 / 3844.0), 0.352);
        assert_eq!(to_fixed3(76.0 / 153.0), 0.497);
        assert_eq!(to_fixed3(0.0625), 0.063); // exact tie → the larger n (Rust's `{:.3}` would give 0.062)
        assert_eq!(to_fixed3(0.1875), 0.188);
        assert_eq!(to_fixed3(0.28125), 0.281);
        assert_eq!(to_fixed3(0.34375), 0.344);
        assert_eq!(to_fixed3(0.3625), 0.362); // the double is just below the tie
        assert_eq!(to_fixed3(0.0), 0.0);
        assert_eq!(to_fixed3(0.9995), 1.0);
    }

    #[test]
    fn hub_v1_rows_match_the_fixture() {
        let f = load_map_fixture("hub_v1");
        let want: Vec<String> = HUB_ROWS_V1.iter().map(|r| r.to_string()).collect();
        assert_eq!(f.rows, Some(want));
    }

    #[test]
    fn validate_all_is_clean_and_reproduces_the_fixture() {
        let d = data();
        let all = validate_all(&d, None);
        assert!(all.errors.is_empty(), "{:?}", all.errors);
        assert!(all.warnings.is_empty(), "{:?}", all.warnings);
        assert!(all.ok);
        let fx = load_validate_all();
        assert_eq!(fx["ok"], serde_json::Value::Bool(true));
        for (id, r) in &all.zones {
            let want = fixture_result(&fx["zones"][id]);
            assert_eq!(r, &want, "zone {id}");
        }
        assert_eq!(all.hub_v1, fixture_result(&fx["hub"]));
        assert_eq!(all.hub, fixture_result(&fx["hubV2"]));
        // and the serialised shape round-trips to the same JSON the exporter wrote
        for (id, r) in &all.zones {
            let json = serde_json::to_value(r).expect("serialise");
            assert_eq!(json, fx["zones"][id], "zone {id} json");
        }
        assert_eq!(serde_json::to_value(&all.hub).expect("hub"), fx["hubV2"]);
    }

    #[test]
    fn design_route_lengths_are_reproduced() {
        // DESIGN.md §3.3: route shut → open per zone, and the ≤ 70 % rule
        let d = data();
        let all = validate_all(&d, None);
        let want = [
            ("undercroft", 952, 640),
            ("cistern", 1002, 678),
            ("ossuary", 1082, 638),
            ("source", 1926, 1086),
        ];
        for (id, closed, open) in want {
            let r = all.zone(id).expect(id);
            let s = r.stats.as_ref().expect("stats");
            let route = s.route.expect("route");
            assert_eq!((route.closed, route.open), (closed, open), "{id}");
            assert!(
                route.open as f64 <= route.closed as f64 * 0.7,
                "{id} open ratio"
            );
            let z = d.zone(id).expect(id);
            assert!(route.closed >= z.targets.route, "{id} route floor");
        }
        // the three Undercroft doors and what they save (DESIGN.md §3.2 table)
        let u = all.zone("undercroft").expect("undercroft");
        let sc = &u.stats.as_ref().expect("stats").shortcuts;
        let saves: Vec<(&str, i32)> = sc.iter().map(|s| (s.id.as_str(), s.detour)).collect();
        assert_eq!(
            saves,
            vec![("u_navedoor", 74), ("u_wingstair", 80), ("u_rood", 152)]
        );
        for s in sc {
            assert_eq!(s.detour, s.saves, "{}: saves is the measured detour", s.id);
            assert!(s.d_open > s.d_barred, "{}: openFrom is the far side", s.id);
        }
    }

    #[test]
    fn every_zone_is_strict_and_the_hubs_are_not() {
        let d = data();
        let all = validate_all(&d, None);
        for (id, r) in &all.zones {
            assert!(r.stats.as_ref().expect("stats").strict, "{id} strict");
        }
        assert!(!all.hub.stats.as_ref().expect("stats").strict);
        // forcing strict off turns nothing into warnings on the committed maps either
        let lax = validate_all(&d, Some(false));
        assert!(lax.ok && lax.warnings.is_empty());
    }

    /* ---- negative controls: the validator must catch a broken map (DESIGN.md §3.7) ---- */

    fn undercroft_rows(d: &GameData) -> (ZoneDef, Vec<Vec<u8>>) {
        let z = d.zone("undercroft").expect("undercroft").clone();
        let grid = row_bytes(&z.rows);
        (z, grid)
    }

    fn with_rows(z: &ZoneDef, grid: &[Vec<u8>]) -> ZoneDef {
        let mut z = z.clone();
        z.rows = grid
            .iter()
            .map(|r| String::from_utf8(r.clone()).expect("ascii"))
            .collect();
        z
    }

    #[test]
    fn negative_control_walled_off_item_is_unreachable() {
        // wall in an item's cell on every side: it becomes an orphan and an unreachable item
        let d = data();
        let (z, mut grid) = undercroft_rows(&d);
        let m = d.parse_zone("undercroft").expect("zone").expect("parses");
        let it = m.items[0];
        for (dx, dz) in DIRS4 {
            let (x, z) = ((it.cx + dx) as usize, (it.cz + dz) as usize);
            grid[z][x] = b'#';
        }
        let broken = with_rows(&z, &grid);
        let r = validate_zone(&d, &broken, None);
        assert!(!r.ok);
        let name = &z.name;
        assert!(
            r.errors.iter().any(|e| e.starts_with(&format!(
                "{name}: 1 walkable cell(s) unreachable from spawn"
            ))),
            "{:?}",
            r.errors
        );
        assert!(r.errors.contains(&format!(
            "{name}: item at ({},{}) unreachable",
            it.cx, it.cz
        )));
        // the walled-in cell also falls out of its region's loot / walkable band only if it changes counts;
        // the loot check still agrees because the item char is untouched
        assert!(!r.errors.iter().any(|e| e.contains("loot.")));
    }

    #[test]
    fn negative_control_shortcut_barred_from_the_near_side() {
        // flip a door's openFrom: the near side becomes the open side → [v2] error on a strict grid
        let d = data();
        let (z, _) = undercroft_rows(&d);
        let mut broken = z.clone();
        let door = &mut broken.shortcuts[2]; // u_rood, openFrom N
        assert_eq!(door.id, "u_rood");
        door.open_from = Facing::S;
        let r = validate_zone(&d, &broken, None);
        assert!(!r.ok);
        let msg = format!(
            "{}: [v2] shortcut 'u_rood' openFrom 'S' is the NEARER side (45 vs 137 cells from the entry) — bar the near side",
            z.name
        );
        assert!(r.errors.contains(&msg), "{:?}", r.errors);
        // the measured door still reports the (swapped) flank distances
        let s = &r.stats.as_ref().expect("stats").shortcuts[2];
        assert_eq!((s.d_barred, s.d_open, s.detour), (137, 45, 152));
        // on a non-strict grid the same fault is only a warning
        let lax = validate_zone(&d, &broken, Some(false));
        assert!(lax.ok);
        assert!(lax.warnings.contains(&msg));
    }

    #[test]
    fn negative_control_meta_disagreement() {
        // drop the gate from the meta, move an NPC and a spot, claim extra loot: the agreement checks fire
        let d = data();
        let (z, _) = undercroft_rows(&d);
        let mut broken = z.clone();
        broken.gate = None;
        broken.loot.oil += 1;
        let (npc_id, npc_cell) = broken
            .npcs
            .iter()
            .next()
            .map(|(k, v)| (k.clone(), *v))
            .expect("an npc");
        broken
            .npcs
            .insert(npc_id.clone(), [npc_cell[0] + 1, npc_cell[1]]);
        broken.spots[0].cell[0] += 1;
        broken.hunters.push("fast".to_string());
        let r = validate_zone(&d, &broken, None);
        assert!(!r.ok);
        let name = &z.name;
        let want = [
            format!("{name}: meta.hunters has 2 entries, map has 1 H"),
            format!(
                "{name}: npc {npc_id} expected at ({},{}), no N there",
                npc_cell[0] + 1,
                npc_cell[1]
            ),
            format!("{name}: map has 1 X but meta.gate is null"),
            format!(
                "{name}: loot.oil: meta says {}, map has {}",
                z.loot.oil + 1,
                z.loot.oil
            ),
        ];
        for w in &want {
            assert!(r.errors.contains(w), "missing {w:?} in {:?}", r.errors);
        }
        assert!(r
            .errors
            .iter()
            .any(|e| e.starts_with(&format!("{name}: spot 0 expected at"))));
        // stats are still measured on a map that only fails meta agreement
        assert!(r.stats.as_ref().expect("stats").route.is_some());
    }

    #[test]
    fn shape_errors_stop_before_parsing() {
        let d = data();
        let opts = ValidateOpts::default();
        let hub = MapMeta::hub("t");
        // ragged row + unknown char (the short row's last char is not at x = w−1, so no border error — as JS)
        let r = validate_map(&rows(&["#####", "#.x.", "#...#", "#####"]), &hub, &opts);
        assert!(!r.ok);
        assert!(r.stats.is_none());
        assert_eq!(
            r.errors,
            vec![
                "t: row 1 has length 4, expected 5".to_string(),
                "t: unknown char 'x' at (2,1)".to_string(),
            ]
        );
        let r = validate_map(&rows(&["#####", "#...#", "#....", "#####"]), &hub, &opts);
        assert_eq!(r.errors, vec!["t: border not wall at (4,2)".to_string()]);
        // a zone that is not the expected size
        let z = MapMeta {
            name: Some("z".into()),
            id: Some("z".into()),
            entry: Some(EntryKind::Stairs),
            ..MapMeta::default()
        };
        let r = validate_map(&rows(&["#####", "#S..#", "#####"]), &z, &opts);
        assert_eq!(r.errors, vec!["z: expected 40×40, got 5×3".to_string()]);
        // empty rows do not panic
        let r = validate_map(&[], &hub, &opts);
        assert!(!r.ok);
        let _ = validate_all(&d, None);
    }

    #[test]
    fn hub_rules_and_missing_spawn() {
        let opts = ValidateOpts::default();
        let hub = MapMeta::hub("h");
        let r = validate_map(&rows(&["#####", "#F..#", "#.F.#", "#####"]), &hub, &opts);
        assert_eq!(
            r.errors,
            vec![
                "h: hub needs exactly one F, has 2".to_string(),
                "h: hub needs exactly one S, has 0".to_string(),
                "h: no spawn cell".to_string(),
            ]
        );
        let s = r.stats.expect("counts are still reported");
        assert_eq!((s.counts.f, s.counts.s, s.walkable), (2, 0, 0));
        // a clean toy hub: warnings only for things the hub does not check
        let r = validate_map(&rows(&["#####", "#F..#", "#.S.#", "#####"]), &hub, &opts);
        assert!(r.ok, "{:?}", r.errors);
        let s = r.stats.expect("stats");
        assert_eq!((s.walkable, s.walls, s.reachable), (6, 14, 20));
        assert_eq!(s.wall_share, 0.7);
        assert!(s.route.is_none());
        assert!(s.unassigned.is_none() && s.shortcuts.is_empty());
    }

    #[test]
    fn toy_zone_contract_checks_warn_on_a_legacy_grid() {
        // a 7×7 "zone" with size 7: no targets → never strict → the contract checks are warnings
        let opts = ValidateOpts {
            size: 7,
            strict: None,
            tools: ["prybar".to_string()].into_iter().collect(),
        };
        let meta = MapMeta {
            name: Some("toy".into()),
            id: Some("toy".into()),
            entry: Some(EntryKind::Stairs),
            size: Some(7),
            hunters: Some(vec!["base".into()]),
            gate: Some(GateDef {
                tool: "prybar".into(),
                cells: vec![[4, 3]],
                opens: "x".into(),
            }),
            loot: Some(Loot {
                oil: 1,
                relic: 0,
                rich: 0,
            }),
            ..MapMeta::default()
        };
        let r = validate_map(
            &rows(&[
                "#######", "#S....#", "#.....#", "#..#X##", "#..#.o#", "#H.#..#", "#######",
            ]),
            &meta,
            &opts,
        );
        assert!(r.ok, "{:?}", r.errors);
        let s = r.stats.as_ref().expect("stats");
        assert_eq!(s.gated_cells, 4);
        assert_eq!(s.behind_gate, vec!["oil@(5,4)".to_string()]);
        assert_eq!(s.route.map(|r| (r.closed, r.open)), Some((14, 14)));
        assert!(r
            .warnings
            .contains(&"toy: [v2] no REGIONS declared (DESIGN.md §3.4)".to_string()));
        assert!(r.warnings.contains(
            &"toy: [v2] hunter spawn at (1,5) is 4 BFS cells from the entry (needs ≥ 10)"
                .to_string()
        ));
        assert!(r
            .warnings
            .contains(&"toy: [v2] entry (1,1) has 2 free 4-neighbour(s), needs ≥ 3".to_string()));
        // forced strict → the same texts become errors
        let strict = validate_map(
            &rows(&[
                "#######", "#S....#", "#.....#", "#..#X##", "#..#.o#", "#H.#..#", "#######",
            ]),
            &meta,
            &ValidateOpts {
                strict: Some(true),
                ..opts.clone()
            },
        );
        assert!(!strict.ok);
        assert_eq!(strict.errors.len(), r.warnings.len());
        // an unknown tool is an error
        let mut bad = meta.clone();
        bad.gate.as_mut().expect("gate").tool = "spoon".into();
        let r = validate_map(
            &rows(&[
                "#######", "#S....#", "#.....#", "#..#X##", "#..#.o#", "#H.#..#", "#######",
            ]),
            &bad,
            &opts,
        );
        assert!(r
            .errors
            .contains(&"toy: unknown gate tool 'spoon'".to_string()));
    }
}
