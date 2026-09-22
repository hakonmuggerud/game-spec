//! Fixture regeneration: rewrites `assets/fixtures/<zone>.json` for the four zones and
//! `validate_all.json` from the Rust sim (`data.parse_zone` + `undercroft_sim::grid` / `validate`), keeping
//! the probe definitions of the existing files and matching their `JSON.stringify(v, null, 1)` layout
//! byte-for-byte when the data is unchanged (`regenerate_is_byte_identical` below pins that against the
//! committed files). The hub fixtures (`hub.json`, `hub_v1.json`, `lap_legacy.json`) are never touched.
//!
//! The layout mirrors `reference/tools/export/export.mjs:fixtureFor`: every output struct below lists its
//! fields in the JS insertion order, `counts` keeps the order kinds first occur in the grid, and numbers print
//! the way JS prints them (integral floats without a fraction).

use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use undercroft_data::{CreatureKind, EntryKind, GameData, ItemKind, Marker, ParsedMap, ZoneDef};
use undercroft_sim::grid::{
    bfs_solid, idx, in_bounds, is_solid, lap_of_map, nearest_reachable, path_cells, route_cells,
    Field,
};
use undercroft_sim::validate::{validate_all, CreatureLap, RouteStat, ShortcutStat, Validation};

/// Rewrite the zone fixtures and `validate_all.json` under `dir`; returns the paths written. The save path
/// uses `render_all` and writes the files itself (atomically, with the data files); this plain form is what
/// the fixture tests exercise.
///
/// Probe definitions (`paths` from/to, `los` from/to, `nearest` at) come from the fixture already in `dir`,
/// falling back to the workspace copy, and a probe whose anchor no longer exists is dropped. Without any
/// existing file the exporter's default probe set is derived from the zone's anchors. A fixture that exists
/// but does not parse is an error (not a silent fall-back): fixing it by hand is the only safe answer.
#[cfg(test)]
pub fn regenerate_all(data: &GameData, dir: &Path) -> Result<Vec<PathBuf>, String> {
    let files = render_all(data, dir)?;
    let mut written = Vec::new();
    for (path, text) in files {
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// Everything `regenerate_all` would write, as `(path, text)` in write order, without touching the disk
/// (the save path renders every file first so a failure here changes nothing).
pub fn render_all(data: &GameData, dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out = Vec::new();
    for zone in &data.zones {
        let m = undercroft_data::parse_zone(zone).map_err(|e| e.to_string())?;
        let probes = load_probes(dir, &zone.id)?;
        let fixture = zone_fixture(zone, &m, probes)?;
        let path = dir.join(format!("{}.json", zone.id));
        let text = render_json(&fixture).map_err(|e| format!("{}: {e}", path.display()))?;
        out.push((path, text));
    }
    let all = validate_all(data, None);
    let out_all = ValidateAllOut {
        ok: all.ok,
        errors: all.errors.clone(),
        warnings: all.warnings.clone(),
        zones: OrderedZones(
            all.zones
                .iter()
                .map(|(id, v)| (id.clone(), ValidationOut::from(v)))
                .collect(),
        ),
        hub: ValidationOut::from(&all.hub_v1),
        hub_v2: ValidationOut::from(&all.hub),
    };
    let path = dir.join("validate_all.json");
    let text = render_json(&out_all).map_err(|e| format!("{}: {e}", path.display()))?;
    out.push((path, text));
    Ok(out)
}

/// `JSON.stringify(value, null, 1) + '\n'`.
pub fn render_json<T: Serialize>(value: &T) -> Result<String, String> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser).map_err(|e| e.to_string())?;
    buf.push(b'\n');
    String::from_utf8(buf).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------------------
// Probe definitions kept from the existing fixture
// ---------------------------------------------------------------------------------------------------------

/// The parts of an existing `<zone>.json` that are authored rather than derived.
#[derive(Debug, Clone, Default, Deserialize)]
struct Probes {
    #[serde(default)]
    paths: Vec<ProbePair>,
    #[serde(default)]
    los: Vec<ProbePair>,
    #[serde(default)]
    nearest: Vec<NearestDef>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProbePair {
    from: String,
    to: String,
}

#[derive(Debug, Clone, Deserialize)]
struct NearestDef {
    at: [f64; 2],
}

/// The existing fixture's probes: from `dir` when present, else the workspace copy, else `None`.
/// A file that exists but is not valid JSON for the probe shape is an error, so a broken fixture cannot
/// silently regenerate from another copy (or from nothing).
fn load_probes(dir: &Path, id: &str) -> Result<Option<Probes>, String> {
    let name = format!("{id}.json");
    for path in [
        dir.join(&name),
        GameData::workspace_fixtures_dir().join(&name),
    ] {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let probes: Probes = serde_json::from_str(&text)
            .map_err(|e| format!("{}: not a fixture: {e}", path.display()))?;
        return Ok(Some(probes));
    }
    Ok(None)
}

/// The exporter's default probe set (`fixtureFor` with `named` = the zone's anchors in map order) for a zone
/// with no fixture to inherit from.
fn default_probes(zone: &ZoneDef, m: &ParsedMap) -> Probes {
    let named: Vec<(String, [i32; 2])> = zone
        .anchor_list()
        .into_iter()
        .filter(|(_, c)| in_bounds(m, c[0], c[1]))
        .collect();
    let n = named.len();
    let mut paths = Vec::new();
    for (k, _) in named.iter().take(12) {
        paths.push(ProbePair {
            from: "entry".into(),
            to: k.clone(),
        });
    }
    for i in 1..n.min(6) {
        paths.push(ProbePair {
            from: named[i].0.clone(),
            to: named[(i * 3) % n].0.clone(),
        });
    }
    let mut los = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            if los.len() >= 60 {
                break;
            }
            los.push(ProbePair {
                from: named[i].0.clone(),
                to: named[j].0.clone(),
            });
        }
    }
    for i in 0..n.min(10) {
        los.push(ProbePair {
            from: format!("{}+", named[i].0),
            to: format!("{}+", named[(i + 7) % n].0),
        });
    }
    let (w, h, ox) = (m.w as f64, m.h as f64, m.ox as f64);
    let nearest = [
        [0.5, 0.5],
        [ox + w / 2.0, h / 2.0],
        [ox + 3.3, 7.7],
        [ox + w - 1.5, h - 1.5],
        [ox + 10.1, 10.9],
    ]
    .into_iter()
    .map(|at| NearestDef { at })
    .collect();
    Probes {
        paths,
        los,
        nearest,
    }
}

// ---------------------------------------------------------------------------------------------------------
// Output shapes (field order = the JS insertion order)
// ---------------------------------------------------------------------------------------------------------

/// A number printed the way JS prints it: no fraction when integral.
#[derive(Debug, Clone, Copy)]
struct JsNum(f64);

impl Serialize for JsNum {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        if v.fract() == 0.0 && v.is_finite() && v.abs() < 1e15 {
            s.serialize_i64(v as i64)
        } else {
            s.serialize_f64(v)
        }
    }
}

/// `counts` — kind name → cells, keyed in first-occurrence order.
struct Counts(Vec<(&'static str, usize)>);

impl Serialize for Counts {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

#[derive(Serialize)]
struct ZoneFixture {
    id: String,
    w: i32,
    h: i32,
    ox: i32,
    entry: [i32; 2],
    cells: Vec<u8>,
    counts: Counts,
    walkable: usize,
    bfs_dist: Vec<i16>,
    bfs_parent: Vec<i32>,
    paths: Vec<PathOut>,
    los: Vec<LosOut>,
    nearest: Vec<NearestOut>,
    laps: Option<Vec<i32>>,
    route: Option<RoutesOut>,
    markers: MarkersOut,
}

#[derive(Serialize)]
struct PathOut {
    from: String,
    to: String,
    target: [i32; 2],
    cells: Vec<[i32; 2]>,
}

#[derive(Serialize)]
struct LosOut {
    from: String,
    to: String,
    a: [f64; 2],
    b: [f64; 2],
    ab: bool,
    ba: bool,
}

#[derive(Serialize)]
struct NearestOut {
    at: [JsNum; 2],
    idx: i64,
}

#[derive(Serialize)]
struct RouteOut {
    cells: i32,
    unreachable: i32,
}

#[derive(Serialize)]
struct RoutesOut {
    closed: RouteOut,
    open: RouteOut,
    gates_closed: RouteOut,
}

#[derive(Serialize)]
struct MarkersOut {
    items: Vec<ItemOut>,
    stairs: Option<StairsOut>,
    hunter_spawns: Vec<MarkerOut>,
    npc_cells: Vec<MarkerOut>,
    spots: Vec<SpotOut>,
    gates: Vec<GateOut>,
    shortcuts: Vec<ShortcutOut>,
    flame: Option<MarkerOut>,
    anchors: BTreeMap<String, MarkerOut>,
    altar: Option<MarkerOut>,
    creatures: Vec<CreatureOut>,
}

#[derive(Serialize)]
struct ItemOut {
    kind: ItemKind,
    cx: i32,
    cz: i32,
}

/// `maps.js:parseMap` `mark(x, z)`.
#[derive(Serialize)]
struct MarkerOut {
    cx: i32,
    cz: i32,
    idx: usize,
    x: f32,
    z: f32,
}

impl From<&Marker> for MarkerOut {
    fn from(m: &Marker) -> Self {
        MarkerOut {
            cx: m.cx,
            cz: m.cz,
            idx: m.idx,
            x: m.x,
            z: m.z,
        }
    }
}

#[derive(Serialize)]
struct StairsOut {
    #[serde(flatten)]
    marker: MarkerOut,
    kind: &'static str,
}

#[derive(Serialize)]
struct SpotOut {
    #[serde(flatten)]
    marker: MarkerOut,
    id: u32,
}

#[derive(Serialize)]
struct GateOut {
    #[serde(flatten)]
    marker: MarkerOut,
    open: bool,
    id: Option<String>,
}

/// A `=` cell: the marker, `open`, and its `SHORTCUTS` entry when bound (`id: null` and nothing else otherwise).
#[derive(Serialize)]
struct ShortcutOut {
    #[serde(flatten)]
    marker: MarkerOut,
    open: bool,
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(rename = "openFrom", skip_serializing_if = "Option::is_none")]
    open_from: Option<undercroft_data::Facing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    saves: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cells: Option<Vec<[i32; 2]>>,
}

#[derive(Serialize)]
struct CreatureOut {
    #[serde(flatten)]
    marker: MarkerOut,
    kind: CreatureKind,
}

/// `validate_all.json`: `maps.validateAll()` with the JS field names (`hub` is the v1 hub, `hubV2` the v2).
#[derive(Serialize)]
struct ValidateAllOut {
    ok: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
    zones: OrderedZones,
    hub: ValidationOut,
    #[serde(rename = "hubV2")]
    hub_v2: ValidationOut,
}

/// `zones: { id: result }` in `ZONE_ORDER`.
struct OrderedZones(Vec<(String, ValidationOut)>);

impl Serialize for OrderedZones {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (id, v) in &self.0 {
            map.serialize_entry(id, v)?;
        }
        map.end()
    }
}

/// `validateMap` result `{ok, errors, warnings, stats}`.
#[derive(Serialize)]
struct ValidationOut {
    ok: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
    stats: Option<StatsOut>,
}

/// `stats` in the JS insertion order (`maps.js:validateMap`): the `w` key was the grid width and was
/// overwritten with the Drowner count, so it keeps its early slot; `unassigned` is only set once a cell
/// belongs to no region and lands after `shortcuts`.
#[derive(Serialize)]
struct StatsOut {
    width: i32,
    height: i32,
    w: i32,
    h: i32,
    strict: bool,
    #[serde(rename = "S")]
    s: i32,
    #[serde(rename = "V")]
    v: i32,
    #[serde(rename = "A")]
    a: i32,
    #[serde(rename = "H")]
    hunters: i32,
    #[serde(rename = "N")]
    n: i32,
    #[serde(rename = "C")]
    c: i32,
    #[serde(rename = "X")]
    x: i32,
    #[serde(rename = "W")]
    water: i32,
    o: i32,
    r: i32,
    #[serde(rename = "R")]
    rich: i32,
    #[serde(rename = "D")]
    d: i32,
    #[serde(rename = "P")]
    p: i32,
    #[serde(rename = "F")]
    f: i32,
    #[serde(rename = "=")]
    shortcut: i32,
    #[serde(rename = "L")]
    l: i32,
    #[serde(rename = "G")]
    g: i32,
    #[serde(rename = "Y")]
    y: i32,
    #[serde(rename = "B")]
    b: i32,
    walkable: i32,
    walls: i32,
    #[serde(rename = "wallShare")]
    wall_share: JsNum,
    reachable: i32,
    #[serde(rename = "gatedCells")]
    gated_cells: i32,
    #[serde(rename = "shortcutOnlyCells")]
    shortcut_only_cells: i32,
    #[serde(rename = "behindGate")]
    behind_gate: Vec<String>,
    shortcuts: Vec<ShortcutStat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unassigned: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<RouteStat>,
    #[serde(rename = "altarLap", skip_serializing_if = "Option::is_none")]
    altar_lap: Option<i32>,
    #[serde(rename = "creatureLaps", skip_serializing_if = "Option::is_none")]
    creature_laps: Option<Vec<CreatureLap>>,
    #[serde(rename = "hunterLaps", skip_serializing_if = "Option::is_none")]
    hunter_laps: Option<Vec<i32>>,
}

impl From<&Validation> for ValidationOut {
    fn from(v: &Validation) -> Self {
        ValidationOut {
            ok: v.ok,
            errors: v.errors.clone(),
            warnings: v.warnings.clone(),
            stats: v.stats.as_ref().map(|st| {
                let k = &st.counts;
                StatsOut {
                    width: st.width,
                    height: st.height,
                    w: k.drowner,
                    h: st.h,
                    strict: st.strict,
                    s: k.s,
                    v: k.v,
                    a: k.a,
                    hunters: k.h,
                    n: k.n,
                    c: k.c,
                    x: k.x,
                    water: k.water,
                    o: k.o,
                    r: k.r,
                    rich: k.rich,
                    d: k.d,
                    p: k.p,
                    f: k.f,
                    shortcut: k.shortcut,
                    l: k.l,
                    g: k.g,
                    y: k.y,
                    b: k.b,
                    walkable: st.walkable,
                    walls: st.walls,
                    wall_share: JsNum(st.wall_share),
                    reachable: st.reachable,
                    gated_cells: st.gated_cells,
                    shortcut_only_cells: st.shortcut_only_cells,
                    behind_gate: st.behind_gate.clone(),
                    shortcuts: st.shortcuts.clone(),
                    unassigned: st.unassigned,
                    route: st.route,
                    altar_lap: st.altar_lap,
                    creature_laps: st.creature_laps.clone(),
                    hunter_laps: st.hunter_laps.clone(),
                }
            }),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Derivation
// ---------------------------------------------------------------------------------------------------------

/// Resolve a probe name (`entry`, an anchor path, either with a trailing `+`) to a cell inside the grid.
fn resolve(zone: &ZoneDef, m: &ParsedMap, entry: [i32; 2], name: &str) -> Option<[i32; 2]> {
    let key = name.trim_end_matches('+');
    let cell = if key == "entry" {
        entry
    } else {
        zone.anchor(key)?
    };
    in_bounds(m, cell[0], cell[1]).then_some(cell)
}

fn zone_fixture(
    zone: &ZoneDef,
    m: &ParsedMap,
    probes: Option<Probes>,
) -> Result<ZoneFixture, String> {
    let stairs = m.stairs.as_ref().ok_or_else(|| {
        format!(
            "zone {}: no S/V cell, cannot build the entry field",
            zone.id
        )
    })?;
    let entry = [stairs.marker.cx, stairs.marker.cz];
    let probes = probes.unwrap_or_else(|| default_probes(zone, m));
    let field = bfs_solid(m, entry[0], entry[1]);

    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for k in &m.cells {
        match counts.iter_mut().find(|(n, _)| *n == k.name()) {
            Some((_, c)) => *c += 1,
            None => counts.push((k.name(), 1)),
        }
    }
    let walkable = (0..m.len())
        .filter(|&i| {
            let (cx, cz) = m.cell_of(i);
            !is_solid(m, cx, cz)
        })
        .count();

    let cells_of = |f: &Field, ti: usize| -> Vec<[i32; 2]> {
        path_cells(f, ti)
            .into_iter()
            .map(|i| {
                let (cx, cz) = m.cell_of(i);
                [cx, cz]
            })
            .collect()
    };
    let mut paths = Vec::new();
    for p in &probes.paths {
        let (Some(from), Some(target)) = (
            resolve(zone, m, entry, &p.from),
            resolve(zone, m, entry, &p.to),
        ) else {
            continue;
        };
        let own;
        let f = if p.from == "entry" {
            &field
        } else {
            own = bfs_solid(m, from[0], from[1]);
            &own
        };
        paths.push(PathOut {
            from: p.from.clone(),
            to: p.to.clone(),
            target,
            cells: cells_of(f, idx(m, target[0], target[1])),
        });
    }

    let ox = m.ox as f64;
    let mut los = Vec::new();
    for p in &probes.los {
        let (Some(a), Some(b)) = (
            resolve(zone, m, entry, &p.from),
            resolve(zone, m, entry, &p.to),
        ) else {
            continue;
        };
        // the exporter's off-centre probes: `+0.13/+0.87` at a `from+`, `+0.71/+0.29` at a `to+`
        let (adx, adz) = if p.from.ends_with('+') {
            (0.13, 0.87)
        } else {
            (0.5, 0.5)
        };
        let (bdx, bdz) = if p.to.ends_with('+') {
            (0.71, 0.29)
        } else {
            (0.5, 0.5)
        };
        let (ax, az) = (ox + a[0] as f64 + adx, a[1] as f64 + adz);
        let (bx, bz) = (ox + b[0] as f64 + bdx, b[1] as f64 + bdz);
        los.push(LosOut {
            from: p.from.clone(),
            to: p.to.clone(),
            a: [ax, az],
            b: [bx, bz],
            ab: undercroft_sim::grid::los_f64(m, ax, az, bx, bz),
            ba: undercroft_sim::grid::los_f64(m, bx, bz, ax, az),
        });
    }

    let nearest = probes
        .nearest
        .iter()
        .map(|n| NearestOut {
            at: [JsNum(n.at[0]), JsNum(n.at[1])],
            idx: nearest_reachable(m, &field, n.at[0] as f32, n.at[1] as f32)
                .map(|i| i as i64)
                .unwrap_or(-1),
        })
        .collect();

    let laps = m.bands.map(|_| {
        (0..m.len())
            .map(|i| {
                let (cx, cz) = m.cell_of(i);
                lap_of_map(m, cx, cz)
            })
            .collect()
    });
    let route_of = |gates_open: bool, sc_open: bool| {
        let r = route_cells(m, gates_open, sc_open);
        RouteOut {
            cells: r.cells,
            unreachable: r.unreachable,
        }
    };
    let route = Some(RoutesOut {
        closed: route_of(true, false),
        open: route_of(true, true),
        gates_closed: route_of(false, false),
    });

    let markers = MarkersOut {
        items: m
            .items
            .iter()
            .map(|i| ItemOut {
                kind: i.kind,
                cx: i.cx,
                cz: i.cz,
            })
            .collect(),
        stairs: Some(StairsOut {
            marker: (&stairs.marker).into(),
            kind: match stairs.kind {
                EntryKind::Stairs => "stairs",
                EntryKind::Elevator => "elevator",
            },
        }),
        hunter_spawns: m.hunter_spawns.iter().map(MarkerOut::from).collect(),
        npc_cells: m.npc_cells.iter().map(MarkerOut::from).collect(),
        spots: m
            .spots
            .iter()
            .map(|s| SpotOut {
                marker: (&s.marker).into(),
                id: s.id,
            })
            .collect(),
        gates: m
            .gates
            .iter()
            .map(|g| GateOut {
                marker: (&g.marker).into(),
                open: false,
                id: g.id.clone(),
            })
            .collect(),
        shortcuts: m
            .shortcuts
            .iter()
            .map(|s| ShortcutOut {
                marker: (&s.marker).into(),
                open: false,
                id: s.def.as_ref().map(|d| d.id.clone()),
                name: s.def.as_ref().map(|d| d.name.clone()),
                open_from: s.def.as_ref().map(|d| d.open_from),
                from: s.def.as_ref().map(|d| d.from.clone()),
                to: s.def.as_ref().map(|d| d.to.clone()),
                saves: s.def.as_ref().map(|d| d.saves),
                cells: s.def.as_ref().map(|d| d.cells.clone()),
            })
            .collect(),
        flame: m.flame.as_ref().map(MarkerOut::from),
        anchors: m
            .anchors
            .iter()
            .map(|(d, a)| (d.to_string(), MarkerOut::from(a)))
            .collect(),
        altar: m.altar.as_ref().map(MarkerOut::from),
        creatures: m
            .creatures
            .iter()
            .map(|c| CreatureOut {
                marker: (&c.marker).into(),
                kind: c.kind,
            })
            .collect(),
    };

    Ok(ZoneFixture {
        id: zone.id.clone(),
        w: m.w,
        h: m.h,
        ox: m.ox,
        entry,
        cells: m.cells.iter().map(|&k| k as u8).collect(),
        counts: Counts(counts),
        walkable,
        bfs_dist: field.dist.clone(),
        bfs_parent: field.parent.clone(),
        paths,
        los,
        nearest,
        laps,
        route,
        markers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::CellKind;
    use undercroft_sim::fixtures::MapFixture;

    const ZONES: [&str; 4] = ["undercroft", "cistern", "ossuary", "source"];

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    /// A fresh, empty scratch directory for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("undercroft-editor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// A unified-diff-like excerpt around the first differing line.
    fn first_mismatch(want: &str, got: &str) -> String {
        let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
        let n = w.len().max(g.len());
        let Some(at) = (0..n).find(|&i| w.get(i) != g.get(i)) else {
            return "no line differs (trailing whitespace?)".into();
        };
        let lo = at.saturating_sub(3);
        let mut out = format!(
            "@@ line {} (want {} lines, got {})\n",
            at + 1,
            w.len(),
            g.len()
        );
        for i in lo..(at + 4).min(n) {
            match (w.get(i), g.get(i)) {
                (Some(a), Some(b)) if a == b => out.push_str(&format!("  {a}\n")),
                (a, b) => {
                    if let Some(a) = a {
                        out.push_str(&format!("- {a}\n"));
                    }
                    if let Some(b) = b {
                        out.push_str(&format!("+ {b}\n"));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn regenerate_is_byte_identical() {
        let d = data();
        let dir = temp_dir("identical");
        let written = regenerate_all(&d, &dir).expect("regenerate");
        let names: Vec<String> = written
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let want_names: Vec<String> = ZONES
            .iter()
            .map(|z| format!("{z}.json"))
            .chain(std::iter::once("validate_all.json".to_string()))
            .collect();
        assert_eq!(names, want_names, "files written, in order");
        for name in &want_names {
            let want = std::fs::read_to_string(GameData::workspace_fixtures_dir().join(name))
                .expect("committed fixture");
            let got = std::fs::read_to_string(dir.join(name)).expect("regenerated fixture");
            assert_eq!(
                got,
                want,
                "{name} differs from the committed fixture:\n{}",
                first_mismatch(&want, &got)
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regenerate_reacts_to_a_map_change() {
        let mut d = data();
        // wall up a floor cell next to the Undercroft entry, in memory only
        let zone = d
            .zones
            .iter_mut()
            .find(|z| z.id == "undercroft")
            .expect("zone");
        let (ex, ez) = {
            let m = undercroft_data::parse_zone(zone).expect("parses");
            let s = m.stairs.expect("entry");
            (s.marker.cx as usize, s.marker.cz as usize)
        };
        let target = [(ex + 1, ez), (ex - 1, ez), (ex, ez + 1), (ex, ez - 1)]
            .into_iter()
            .find(|&(x, z)| zone.rows[z].as_bytes()[x] == b'.')
            .expect("a floor cell next to the entry");
        let mut row = zone.rows[target.1].clone().into_bytes();
        row[target.0] = b'#';
        zone.rows[target.1] = String::from_utf8(row).unwrap();

        let dir = temp_dir("mutated");
        regenerate_all(&d, &dir).expect("regenerate");
        let committed =
            std::fs::read_to_string(GameData::workspace_fixtures_dir().join("undercroft.json"))
                .expect("committed fixture");
        let text = std::fs::read_to_string(dir.join("undercroft.json")).expect("regenerated");
        let want: MapFixture = serde_json::from_str(&committed).expect("committed parses");
        let got: MapFixture =
            serde_json::from_str(&text).expect("the sim's reader parses the output");
        assert_eq!(got.walkable, want.walkable - 1, "one fewer walkable cell");
        assert_ne!(got.bfs_dist, want.bfs_dist, "the entry field changed");
        assert_eq!(
            got.cells[target.1 * got.w as usize + target.0],
            CellKind::Wall as u8
        );
        assert_eq!(
            got.paths.len(),
            want.paths.len(),
            "probe definitions are kept"
        );
        assert_eq!(got.los.len(), want.los.len());
        assert_eq!(got.nearest.len(), want.nearest.len());
        // validate_all.json follows too (`stats.walkable` counts non-wall chars)
        let all: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("validate_all.json")).unwrap())
                .unwrap();
        let committed_all = undercroft_sim::fixtures::load_validate_all();
        assert_ne!(
            all["zones"]["undercroft"]["stats"]["walkable"],
            committed_all["zones"]["undercroft"]["stats"]["walkable"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_probes_match_the_exporter_counts() {
        // with no fixture to inherit from, the exporter's probe set is derived: 12 + 5 paths, 60 + 10 LOS, 5 nearest
        let d = data();
        for id in ZONES {
            let zone = d.zone(id).unwrap();
            let m = undercroft_data::parse_zone(zone).unwrap();
            let p = default_probes(zone, &m);
            assert_eq!(
                (p.paths.len(), p.los.len(), p.nearest.len()),
                (17, 70, 5),
                "{id}"
            );
            let f = zone_fixture(zone, &m, Some(p)).expect("derives");
            assert_eq!(
                (f.paths.len(), f.los.len(), f.nearest.len()),
                (17, 70, 5),
                "{id}"
            );
        }
    }

    #[test]
    fn a_malformed_fixture_is_an_error_not_a_fallback() {
        let d = data();
        let dir = temp_dir("malformed");
        std::fs::write(dir.join("cistern.json"), "{not json").unwrap();
        let err = render_all(&d, &dir).expect_err("a broken fixture must not regenerate");
        assert!(
            err.contains("cistern.json") && err.contains("not a fixture"),
            "{err}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("cistern.json")).unwrap(),
            "{not json",
            "render_all touches nothing"
        );
        assert!(!dir.join("undercroft.json").exists());
        // an absent fixture still falls back to the workspace copy
        std::fs::remove_file(dir.join("cistern.json")).unwrap();
        assert!(render_all(&d, &dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropped_probes_do_not_panic() {
        // a fixture whose probes name anchors that do not exist yields fewer probes, not an error
        let d = data();
        let dir = temp_dir("dropped");
        let stale = r#"{"paths":[{"from":"entry","to":"nowhere"},{"from":"entry","to":"entry"}],"los":[{"from":"gone+","to":"entry+"}],"nearest":[{"at":[1.5,2]}]}"#;
        std::fs::write(dir.join("cistern.json"), stale).unwrap();
        regenerate_all(&d, &dir).expect("regenerate");
        let got: MapFixture =
            serde_json::from_str(&std::fs::read_to_string(dir.join("cistern.json")).unwrap())
                .unwrap();
        assert_eq!(got.paths.len(), 1);
        assert!(got.los.is_empty());
        assert_eq!(got.nearest.len(), 1);
        assert!(std::fs::read_to_string(dir.join("cistern.json"))
            .unwrap()
            .contains("\"at\": [\n    1.5,\n    2\n   ]"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
