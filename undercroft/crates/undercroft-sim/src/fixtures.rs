//! Readers for the parity fixtures in `assets/fixtures/` (frozen golden output recorded from the Three.js
//! prototype during the port; format in `assets/fixtures/README.md`). Test-support code shared by every lane:
//! the grid tests use the map fixtures, the world lane's validator uses `validate_all.json`.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use undercroft_data::GameData;

/// The map fixtures: the four zones, the v2 hub and the v1 hub.
pub const MAP_FIXTURES: [&str; 6] = [
    "undercroft",
    "cistern",
    "ossuary",
    "source",
    "hub",
    "hub_v1",
];

/// A marker record as `maps.js:parseMap` writes it (`{cx, cz, idx, x, z, …}`); extra fields are optional.
#[derive(Debug, Clone, Deserialize)]
pub struct MarkerFixture {
    pub cx: i32,
    pub cz: i32,
    #[serde(default)]
    pub idx: usize,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub z: f32,
    #[serde(default)]
    pub kind: Option<String>,
    /// A string for gates / shortcuts, an integer for spots.
    #[serde(default)]
    pub id: Option<serde_json::Value>,
}

impl MarkerFixture {
    /// The id when it is a string (gate / shortcut ids).
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().and_then(|v| v.as_str().map(String::from))
    }
}

/// An item spawn `{kind, cx, cz}`.
#[derive(Debug, Clone, Deserialize)]
pub struct ItemFixture {
    pub kind: String,
    pub cx: i32,
    pub cz: i32,
}

/// The `markers` block of a map fixture.
#[derive(Debug, Clone, Deserialize)]
pub struct MarkersFixture {
    pub items: Vec<ItemFixture>,
    pub stairs: Option<MarkerFixture>,
    pub hunter_spawns: Vec<MarkerFixture>,
    pub npc_cells: Vec<MarkerFixture>,
    pub spots: Vec<MarkerFixture>,
    pub gates: Vec<MarkerFixture>,
    pub shortcuts: Vec<MarkerFixture>,
    pub flame: Option<MarkerFixture>,
    pub anchors: BTreeMap<String, MarkerFixture>,
    pub altar: Option<MarkerFixture>,
    pub creatures: Vec<MarkerFixture>,
}

/// One `pathTo` probe.
#[derive(Debug, Clone, Deserialize)]
pub struct PathFixture {
    pub from: String,
    pub to: String,
    pub target: [i32; 2],
    pub cells: Vec<[i32; 2]>,
}

/// One `los` probe (both directions).
#[derive(Debug, Clone, Deserialize)]
pub struct LosFixture {
    pub from: String,
    pub to: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub ab: bool,
    pub ba: bool,
}

/// One `nearestReachable` probe (`idx` −1 = none).
#[derive(Debug, Clone, Deserialize)]
pub struct NearestFixture {
    pub at: [f32; 2],
    pub idx: i64,
}

/// One `routeCells` result.
#[derive(Debug, Clone, Deserialize)]
pub struct RouteFixture {
    pub cells: i32,
    pub unreachable: i32,
}

/// The three `routeCells` variants.
#[derive(Debug, Clone, Deserialize)]
pub struct RoutesFixture {
    pub closed: RouteFixture,
    pub open: RouteFixture,
    pub gates_closed: RouteFixture,
}

/// `assets/fixtures/<map>.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct MapFixture {
    pub id: String,
    pub w: i32,
    pub h: i32,
    pub ox: i32,
    pub entry: [i32; 2],
    pub cells: Vec<u8>,
    pub counts: BTreeMap<String, usize>,
    pub walkable: usize,
    pub bfs_dist: Vec<i16>,
    pub bfs_parent: Vec<i32>,
    pub paths: Vec<PathFixture>,
    pub los: Vec<LosFixture>,
    pub nearest: Vec<NearestFixture>,
    pub laps: Option<Vec<i32>>,
    pub route: Option<RoutesFixture>,
    pub markers: MarkersFixture,
    /// Only `hub_v1` carries its rows (the v1 hub has no map file).
    #[serde(default)]
    pub rows: Option<Vec<String>>,
}

impl MapFixture {
    /// Resolve a path/LOS probe's anchor name to its cell. Names are anchor paths from the zone's `ANCHORS`
    /// (`entry`, `shortcuts.u_rood`, `hunters[1]`, with a trailing `+` for the off-centre LOS probes); the hub
    /// uses `entry`, `flame` and `anchorN`.
    pub fn anchor(&self, name: &str) -> (String, [i32; 2]) {
        let key = name.trim_end_matches('+');
        if key == "entry" {
            return (key.to_string(), self.entry);
        }
        if key == "flame" {
            let f = self.markers.flame.as_ref().expect("flame marker");
            return (key.to_string(), [f.cx, f.cz]);
        }
        if let Some(d) = key.strip_prefix("anchor") {
            let a = &self.markers.anchors[d];
            return (key.to_string(), [a.cx, a.cz]);
        }
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        let zone = data.zone(&self.id).expect("zone fixture names a zone");
        let cell = zone
            .anchor(key)
            .unwrap_or_else(|| panic!("{}: unknown anchor {key}", self.id));
        (key.to_string(), cell)
    }
}

/// `assets/fixtures/lap_legacy.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct LapLegacyFixture {
    pub size: i32,
    pub band: i32,
    pub max_lap: i32,
    pub laps: Vec<i32>,
}

fn fixture_path(name: &str) -> PathBuf {
    GameData::workspace_fixtures_dir().join(format!("{name}.json"))
}

/// Read any fixture file as JSON.
pub fn load_fixture_json(name: &str) -> serde_json::Value {
    let path = fixture_path(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Read a map fixture (`undercroft`, `cistern`, `ossuary`, `source`, `hub`, `hub_v1`).
pub fn load_map_fixture(name: &str) -> MapFixture {
    serde_json::from_value(load_fixture_json(name))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// Read `lap_legacy.json`.
pub fn load_lap_legacy() -> LapLegacyFixture {
    serde_json::from_value(load_fixture_json("lap_legacy")).expect("lap_legacy fixture")
}

/// Read `validate_all.json` (the raw `maps.validateAll()` result; the world lane types it).
pub fn load_validate_all() -> serde_json::Value {
    load_fixture_json("validate_all")
}
