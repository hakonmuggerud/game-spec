//! Zone definitions: everything a `prototype/src/maps/<id>.js` file exports (`ID SIZE REGIONS SHORTCUTS ANCHORS
//! META`) merged with the non-map-shaped `maps.js:TUNING` entry and its `maps.js:PALETTES` palette — i.e. one
//! `maps.js:ZONES[id]` record minus the rows. The rows live in `assets/data/maps/<id>.txt` and are attached by
//! `GameData::from_dir` (`ZoneDef::rows`).

use crate::cell::CreatureKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A grid cell `[cx, cz]` as the zone files write it.
pub type CellXY = [i32; 2];

/// Per-zone look (`maps.js:PALETTES[id]`); every colour is `0xRRGGBB`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub floor: u32,
    pub wall: u32,
    pub pillar: u32,
    pub ceil: u32,
    pub deep: u32,
    pub water: u32,
    pub water_surface: u32,
    pub water_glow: u32,
    pub fog: Fog,
    pub ambient: u32,
    pub sky: u32,
}

/// `PALETTES[id].fog` — exp2 fog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fog {
    pub color: u32,
    pub density: f32,
}

/// `META.entry`: which spawn/extraction marker the map uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    #[serde(rename = "S")]
    Stairs,
    #[serde(rename = "V")]
    Elevator,
}

/// `META.deepStyle`: flat deep pockets (×0.6 lamp, ×1.5 burn) or the Source's per-lap bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeepStyle {
    Flat,
    Bands,
}

/// `META.bands` — `maps.js:lapOf` parameters (DESIGN.md §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Bands {
    pub band: i32,
    pub max_lap: i32,
}

/// Compass side (`SHORTCUTS[].openFrom`, `META.creatures[].facing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Facing {
    N,
    E,
    S,
    W,
}

impl Facing {
    /// `maps.js:DIR_OF` — the unit step `[dx, dz]` for this side (row 0 is north, so N = `[0, -1]`).
    pub fn delta(self) -> CellXY {
        match self {
            Facing::N => [0, -1],
            Facing::S => [0, 1],
            Facing::E => [1, 0],
            Facing::W => [-1, 0],
        }
    }

    /// Yaw in radians with the prototype's convention forward = `(-sin yaw, -cos yaw)`: N = 0, E = -π/2,
    /// S = π, W = π/2 (`hunter.js` Warden facing).
    pub fn yaw(self) -> f32 {
        match self {
            Facing::N => 0.0,
            Facing::E => -std::f32::consts::FRAC_PI_2,
            Facing::S => std::f32::consts::PI,
            Facing::W => std::f32::consts::FRAC_PI_2,
        }
    }
}

/// One `META.creatures[i]` entry (row-major creature cell order, DESIGN.md §5.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatureSpawn {
    pub kind: CreatureKind,
    /// Warden: the side it faces at its post.
    pub facing: Option<Facing>,
    /// Warden sweep half-angle (degrees), reach and territory overrides (`CREATURE.warden` otherwise).
    pub sweep: Option<f32>,
    pub reach: Option<f32>,
    pub territory: Option<f32>,
    /// Brute leash (BFS cells from its home).
    pub leash: Option<f32>,
    /// The creature may sit behind the zone's tool gate (validator).
    pub gate_ok: bool,
}

/// `META.gate` — the zone's `X` cells and the tool that opens them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateDef {
    pub tool: String,
    pub cells: Vec<CellXY>,
    pub opens: String,
}

/// `META.spots[i]` — a contract spot (`C`, row-major; `id` = the `spot` field in contracts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpotDef {
    pub id: u32,
    pub cell: CellXY,
    pub label: String,
}

/// Expected loot counts (`META.loot`, `REGIONS[].loot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Loot {
    pub oil: u32,
    pub relic: u32,
    pub rich: u32,
}

/// `REGIONS[i]` — an inclusive extent that partitions the non-solid cells (DESIGN.md §3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    pub name: String,
    /// Inclusive `[x0, x1]`.
    pub x: CellXY,
    /// Inclusive `[z0, z1]`.
    pub z: CellXY,
    pub deep: bool,
    pub loot: Option<Loot>,
}

impl Region {
    /// Is the cell inside this region's extent?
    pub fn contains(&self, cx: i32, cz: i32) -> bool {
        cx >= self.x[0] && cx <= self.x[1] && cz >= self.z[0] && cz <= self.z[1]
    }
}

/// `SHORTCUTS[i]` — one barred door (DESIGN.md §3.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShortcutDef {
    pub id: String,
    pub name: String,
    /// The `=` cells of one door (1, or 2 adjacent).
    pub cells: Vec<CellXY>,
    /// The side the player must stand on to lift the bars (the far side).
    pub open_from: Facing,
    /// Region ids on the barred / open side.
    pub from: String,
    pub to: String,
    /// The BFS detour the door removes (measured; the validator recomputes it as `detour`).
    pub saves: i32,
}

/// One `ANCHORS` value: a cell, a list of cells, or a nested group (`ANCHORS.shortcuts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    Cell(CellXY),
    Cells(Vec<CellXY>),
    Group(BTreeMap<String, Anchor>),
}

impl Anchor {
    /// The single cell, if this anchor is one.
    pub fn cell(&self) -> Option<CellXY> {
        match self {
            Anchor::Cell(c) => Some(*c),
            _ => None,
        }
    }

    /// Flatten to `(path, cell)` pairs; lists become `name[i]`, groups `group.name`.
    pub fn flatten(&self, prefix: &str, out: &mut Vec<(String, CellXY)>) {
        match self {
            Anchor::Cell(c) => out.push((prefix.to_string(), *c)),
            Anchor::Cells(v) => {
                for (i, c) in v.iter().enumerate() {
                    out.push((format!("{prefix}[{i}]"), *c));
                }
            }
            Anchor::Group(g) => {
                for (k, v) in g {
                    v.flatten(&format!("{prefix}.{k}"), out);
                }
            }
        }
    }
}

/// `META.targets` — the DESIGN.md §3.3 size/route contract the validator checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Targets {
    pub size: i32,
    pub walkable: i32,
    pub wall_share: f32,
    pub route: i32,
}

/// `TUNING[id].requires` — access rule (`maps.js:zoneLocked`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Requires {
    pub building: Option<String>,
    pub light_tech: Option<u32>,
    pub tier: Option<u32>,
    pub rescued: Option<String>,
}

/// One zone: `maps.js:ZONES[id]` without the rows (see the module doc).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneDef {
    pub id: String,
    /// `META.name`.
    pub name: String,
    /// `SIZE` — the grid is `size × size`.
    pub size: i32,
    pub entry: EntryKind,
    /// `META.exit` — `"stairs"` or `"elevator"`.
    pub exit: String,
    pub burn_mul: f32,
    pub lamp_mul: f32,
    pub deep_style: DeepStyle,
    pub bands: Option<Bands>,
    /// One speed-profile name per `H`, row-major (`base` / `fast`).
    pub hunters: Vec<String>,
    /// One entry per creature cell, row-major.
    pub creatures: Vec<CreatureSpawn>,
    /// The captive named on the Departure Board.
    pub npc: Option<String>,
    /// NPC id → cell for every `N`.
    pub npcs: BTreeMap<String, CellXY>,
    pub gate: Option<GateDef>,
    pub spots: Vec<SpotDef>,
    pub loot: Loot,
    /// `META.points` — total flame points on the map.
    pub points: u32,
    pub regions: Vec<Region>,
    pub shortcuts: Vec<ShortcutDef>,
    pub anchors: BTreeMap<String, Anchor>,
    pub targets: Targets,
    /// `TUNING.source.noBank` — no banking in this zone.
    pub no_bank: bool,
    pub requires: Option<Requires>,
    pub lock_reason: Option<String>,
    pub ambience: String,
    pub palette: Palette,
    pub intro: String,
    pub threat: String,
    /// The ASCII rows (`ROWS`), attached by the loader from `maps/<id>.txt`; not part of the RON.
    #[serde(skip)]
    pub rows: Vec<String>,
}

impl ZoneDef {
    /// Look up an anchor by dotted path (`"entry"`, `"shortcuts.u_rood"`, `"hunters[1]"`).
    pub fn anchor(&self, path: &str) -> Option<CellXY> {
        let mut all = Vec::new();
        for (k, v) in &self.anchors {
            v.flatten(k, &mut all);
        }
        all.into_iter().find(|(k, _)| k == path).map(|(_, c)| c)
    }

    /// Every anchor as `(path, cell)` in map order.
    pub fn anchor_list(&self) -> Vec<(String, CellXY)> {
        let mut all = Vec::new();
        for (k, v) in &self.anchors {
            v.flatten(k, &mut all);
        }
        all
    }

    /// `maps.js:ZONES[id].hunterSpeeds` — the `(profile name, speed table)` per `H` is derived from
    /// `Config::hunter_profiles`; this is the profile name list with the JS fallback to `base`.
    pub fn hunter_profile_names(&self) -> Vec<&str> {
        self.hunters.iter().map(String::as_str).collect()
    }
}

/// Zone order on the Departure Board (`maps.js:ZONE_ORDER`).
pub const ZONE_ORDER: [&str; 4] = ["undercroft", "cistern", "ossuary", "source"];
