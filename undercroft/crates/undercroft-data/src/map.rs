//! The map parser: `maps.js:parseMap` / `bindShortcuts` / `bindGates` / `parseZone` / `parseHub` /
//! `deepNeighbourhood`. Pure: rows in, a `ParsedMap` out. The `pool` bitmap of the JS map is runtime state and
//! lives in `undercroft_sim::pool`; the `open` flags of gates and shortcuts are runtime state too.

use crate::cell::{legend, CellKind, CreatureKind, Legend};
use crate::tables::ItemKind;
use crate::zone::{Bands, CellXY, EntryKind, Facing, GateDef, ShortcutDef, ZoneDef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Parse failure — the JS parser only warns on these; the loader treats them as data errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MapError {
    #[error("map {name}: no rows")]
    Empty { name: String },
    #[error("map {name}: row {row} has length {len}, expected {expected}")]
    Ragged {
        name: String,
        row: usize,
        len: usize,
        expected: usize,
    },
    #[error("map {name}: unknown char {ch:?} at ({x},{z})")]
    UnknownChar {
        name: String,
        ch: char,
        x: usize,
        z: usize,
    },
}

/// A marker cell with its world centre (`maps.js:parseMap` `mark()`): `x = ox + cx + 0.5`, `z = cz + 0.5`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub cx: i32,
    pub cz: i32,
    pub idx: usize,
    pub x: f32,
    pub z: f32,
}

impl Marker {
    /// The cell as `[cx, cz]`.
    pub fn cell(&self) -> CellXY {
        [self.cx, self.cz]
    }
}

/// An item to spawn (`o` / `r` / `R`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSpawn {
    pub kind: ItemKind,
    pub cx: i32,
    pub cz: i32,
}

/// The spawn / extraction cell (`S` or `V`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Extraction {
    pub marker: Marker,
    pub kind: EntryKind,
}

/// A contract spot (`C`), `id` = row-major index.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpotCell {
    pub marker: Marker,
    pub id: u32,
}

/// A tool gate cell (`X`). `id` is the stable save id (`bindGates`): the tool name, or `tool0`, `tool1`… when a
/// zone has several gate cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateCell {
    pub marker: Marker,
    pub id: Option<String>,
}

/// A shortcut door cell (`=`) with its `SHORTCUTS` entry bound (`bindShortcuts`); `def` is `None` when no
/// entry lists the cell (the validator reports it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShortcutCell {
    pub marker: Marker,
    pub def: Option<ShortcutDef>,
}

impl ShortcutCell {
    /// The door id when bound.
    pub fn id(&self) -> Option<&str> {
        self.def.as_ref().map(|d| d.id.as_str())
    }

    /// The side the door opens from when bound.
    pub fn open_from(&self) -> Option<Facing> {
        self.def.as_ref().map(|d| d.open_from)
    }
}

/// A creature spawn cell (`L` `G` `Y` `B` `w`), row-major.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CreatureCell {
    pub marker: Marker,
    pub kind: CreatureKind,
}

/// `maps.js:parseMap` output (minus `pool`, minus `hunterSpawn` which is `hunter_spawns[0]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedMap {
    pub name: String,
    pub w: i32,
    pub h: i32,
    /// World x offset of column 0 (0 for zones, `HUB_OX` for the hub).
    pub ox: i32,
    pub bands: Option<Bands>,
    /// Row-major, `idx = cz * w + cx`.
    pub cells: Vec<CellKind>,
    pub items: Vec<ItemSpawn>,
    pub stairs: Option<Extraction>,
    /// One per `H`, row-major.
    pub hunter_spawns: Vec<Marker>,
    /// One per `N`, row-major.
    pub npc_cells: Vec<Marker>,
    pub spots: Vec<SpotCell>,
    pub gates: Vec<GateCell>,
    pub shortcuts: Vec<ShortcutCell>,
    pub flame: Option<Marker>,
    /// Hub building anchors by digit.
    pub anchors: BTreeMap<u8, Marker>,
    pub altar: Option<Marker>,
    pub creatures: Vec<CreatureCell>,
}

impl ParsedMap {
    /// `maps.js:idx`.
    pub fn idx(&self, cx: i32, cz: i32) -> usize {
        (cz * self.w + cx) as usize
    }

    /// `maps.js:inBounds`.
    pub fn in_bounds(&self, cx: i32, cz: i32) -> bool {
        cx >= 0 && cz >= 0 && cx < self.w && cz < self.h
    }

    /// `maps.js:cellType` — `Wall` outside the grid.
    pub fn cell_type(&self, cx: i32, cz: i32) -> CellKind {
        if self.in_bounds(cx, cz) {
            self.cells[self.idx(cx, cz)]
        } else {
            CellKind::Wall
        }
    }

    /// `maps.js:parseMap` `hunterSpawn` — the first `H`.
    pub fn hunter_spawn(&self) -> Option<&Marker> {
        self.hunter_spawns.first()
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// True when the grid is empty.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// `(cx, cz)` of a cell index.
    pub fn cell_of(&self, idx: usize) -> (i32, i32) {
        let i = idx as i32;
        (i % self.w, i / self.w)
    }
}

/// `maps.js:deepNeighbourhood` — does a marker cell sit in a deep pocket? The majority of its open
/// 4-neighbours are `D` / `R` (walls and pillars do not count; at least one open neighbour).
pub fn deep_neighbourhood(rows: &[Vec<u8>], x: usize, z: usize) -> bool {
    let (mut deep, mut open) = (0, 0);
    for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
        let (nx, nz) = (x as i32 + dx, z as i32 + dz);
        if nx < 0 || nz < 0 {
            continue;
        }
        let ch = match rows.get(nz as usize).and_then(|r| r.get(nx as usize)) {
            Some(&c) => c,
            None => continue,
        };
        if ch == b'#' || ch == b'P' {
            continue;
        }
        open += 1;
        if ch == b'D' || ch == b'R' {
            deep += 1;
        }
    }
    open > 0 && deep * 2 >= open
}

/// Rows as byte grids (the legend is ASCII).
pub fn row_bytes(rows: &[String]) -> Vec<Vec<u8>> {
    rows.iter().map(|r| r.as_bytes().to_vec()).collect()
}

/// `maps.js:parseMap(rows, ox, name, {bands, shortcuts})`.
pub fn parse_map(
    rows: &[String],
    ox: i32,
    name: &str,
    bands: Option<Bands>,
    shortcut_meta: Option<&[ShortcutDef]>,
) -> Result<ParsedMap, MapError> {
    let grid = row_bytes(rows);
    let h = grid.len();
    let w = grid
        .first()
        .map(|r| r.len())
        .ok_or_else(|| MapError::Empty {
            name: name.to_string(),
        })?;
    for (z, r) in grid.iter().enumerate() {
        if r.len() != w {
            return Err(MapError::Ragged {
                name: name.to_string(),
                row: z,
                len: r.len(),
                expected: w,
            });
        }
    }
    let mut m = ParsedMap {
        name: name.to_string(),
        w: w as i32,
        h: h as i32,
        ox,
        bands,
        cells: vec![CellKind::Floor; w * h],
        items: Vec::new(),
        stairs: None,
        hunter_spawns: Vec::new(),
        npc_cells: Vec::new(),
        spots: Vec::new(),
        gates: Vec::new(),
        shortcuts: Vec::new(),
        flame: None,
        anchors: BTreeMap::new(),
        altar: None,
        creatures: Vec::new(),
    };
    let mark = |x: usize, z: usize| Marker {
        cx: x as i32,
        cz: z as i32,
        idx: z * w + x,
        x: (ox + x as i32) as f32 + 0.5,
        z: z as f32 + 0.5,
    };
    for z in 0..h {
        for x in 0..w {
            let ch = grid[z][x] as char;
            let deep_if_pocket = || {
                if deep_neighbourhood(&grid, x, z) {
                    CellKind::Deep
                } else {
                    CellKind::Floor
                }
            };
            let t = match legend(ch).ok_or(MapError::UnknownChar {
                name: name.to_string(),
                ch,
                x,
                z,
            })? {
                Legend::Wall => CellKind::Wall,
                Legend::Pillar => CellKind::Pillar,
                Legend::Floor => CellKind::Floor,
                Legend::Deep => CellKind::Deep,
                Legend::Water => CellKind::Water,
                Legend::Gate => {
                    m.gates.push(GateCell {
                        marker: mark(x, z),
                        id: None,
                    });
                    CellKind::Gate
                }
                Legend::Shortcut => {
                    m.shortcuts.push(ShortcutCell {
                        marker: mark(x, z),
                        def: None,
                    });
                    CellKind::Shortcut
                }
                Legend::Stairs => {
                    m.stairs = Some(Extraction {
                        marker: mark(x, z),
                        kind: EntryKind::Stairs,
                    });
                    CellKind::Stairs
                }
                Legend::Elevator => {
                    m.stairs = Some(Extraction {
                        marker: mark(x, z),
                        kind: EntryKind::Elevator,
                    });
                    CellKind::Elevator
                }
                Legend::Altar => {
                    m.altar = Some(mark(x, z));
                    CellKind::Altar
                }
                Legend::ItemOil => {
                    m.items.push(ItemSpawn {
                        kind: ItemKind::Oil,
                        cx: x as i32,
                        cz: z as i32,
                    });
                    CellKind::Floor
                }
                Legend::ItemRelic => {
                    m.items.push(ItemSpawn {
                        kind: ItemKind::Relic,
                        cx: x as i32,
                        cz: z as i32,
                    });
                    CellKind::Floor
                }
                Legend::DeepItemRich => {
                    m.items.push(ItemSpawn {
                        kind: ItemKind::Rich,
                        cx: x as i32,
                        cz: z as i32,
                    });
                    CellKind::Deep
                }
                Legend::Hunter => {
                    m.hunter_spawns.push(mark(x, z));
                    deep_if_pocket()
                }
                Legend::Npc => {
                    m.npc_cells.push(mark(x, z));
                    deep_if_pocket()
                }
                Legend::Spot => {
                    let id = m.spots.len() as u32;
                    m.spots.push(SpotCell {
                        marker: mark(x, z),
                        id,
                    });
                    deep_if_pocket()
                }
                Legend::Flame => {
                    m.flame = Some(mark(x, z));
                    CellKind::Floor
                }
                Legend::Creature(kind) => {
                    m.creatures.push(CreatureCell {
                        marker: mark(x, z),
                        kind,
                    });
                    deep_if_pocket()
                }
                Legend::WaterDrowner => {
                    m.creatures.push(CreatureCell {
                        marker: mark(x, z),
                        kind: CreatureKind::Drowner,
                    });
                    CellKind::Water
                }
                Legend::Anchor(d) => {
                    m.anchors.insert(d, mark(x, z));
                    CellKind::Floor
                }
            };
            m.cells[z * w + x] = t;
        }
    }
    if let Some(meta) = shortcut_meta {
        bind_shortcuts(&mut m, meta);
    }
    Ok(m)
}

/// `maps.js:bindShortcuts` — attach each `SHORTCUTS` entry to the `=` cells it lists.
pub fn bind_shortcuts(m: &mut ParsedMap, meta: &[ShortcutDef]) {
    for s in &mut m.shortcuts {
        let cell = s.marker.cell();
        s.def = meta.iter().find(|e| e.cells.contains(&cell)).cloned();
    }
}

/// `maps.js:bindGates` — stable save ids for the `X` cells: the tool name alone for one gate, `tool0`, `tool1`…
/// for several; `gate` when the zone has no gate meta.
pub fn bind_gates(m: &mut ParsedMap, gate: Option<&GateDef>) {
    let tool = gate.map(|g| g.tool.as_str()).unwrap_or("gate");
    let n = m.gates.len();
    for (i, g) in m.gates.iter_mut().enumerate() {
        g.id = Some(if n > 1 {
            format!("{tool}{i}")
        } else {
            tool.to_string()
        });
    }
}

/// `maps.js:parseZone` — parse a zone's rows with its bands and shortcut meta bound and gate ids assigned.
pub fn parse_zone(zone: &ZoneDef) -> Result<ParsedMap, MapError> {
    let mut m = parse_map(
        &zone.rows,
        0,
        &zone.id,
        zone.bands,
        Some(zone.shortcuts.as_slice()),
    )?;
    bind_gates(&mut m, zone.gate.as_ref());
    Ok(m)
}

/// `maps.js:parseHub` — the hub rows at `HUB_OX`.
pub fn parse_hub(rows: &[String], hub_ox: i32) -> Result<ParsedMap, MapError> {
    parse_map(rows, hub_ox, "hub", None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &[&str]) -> Vec<String> {
        s.iter().map(|r| r.to_string()).collect()
    }

    #[test]
    fn parses_markers_and_deep_pockets() {
        let r = rows(&["#####", "#DHD#", "#.oS#", "#N.C#", "#####"]);
        let m = parse_map(&r, 60, "t", None, None).expect("parse");
        assert_eq!((m.w, m.h), (5, 5));
        assert_eq!(m.cell_type(2, 1), CellKind::Deep); // H between two D
        assert_eq!(m.cell_type(1, 3), CellKind::Floor); // N with one open non-deep neighbour
        assert_eq!(m.hunter_spawns[0].x, 62.5);
        assert_eq!(m.hunter_spawns[0].z, 1.5);
        assert_eq!(m.items[0].kind, ItemKind::Oil);
        assert_eq!(m.stairs.map(|s| s.kind), Some(EntryKind::Stairs));
        assert_eq!(m.spots[0].id, 0);
        assert_eq!(m.cell_type(-1, 0), CellKind::Wall);
    }

    #[test]
    fn rejects_bad_rows() {
        assert!(matches!(
            parse_map(&rows(&["###", "##"]), 0, "t", None, None),
            Err(MapError::Ragged { row: 1, .. })
        ));
        assert!(matches!(
            parse_map(&rows(&["#x#"]), 0, "t", None, None),
            Err(MapError::UnknownChar { ch: 'x', .. })
        ));
        assert_eq!(
            parse_map(&[], 0, "t", None, None),
            Err(MapError::Empty {
                name: "t".to_string()
            })
        );
    }

    #[test]
    fn binds_gates_and_shortcuts() {
        let r = rows(&["#####", "#.X.#", "#=#=#", "#####"]);
        let mut m = parse_map(
            &r,
            0,
            "t",
            None,
            Some(&[ShortcutDef {
                id: "d".into(),
                name: "Door".into(),
                cells: vec![[1, 2]],
                open_from: Facing::S,
                from: "a".into(),
                to: "b".into(),
                saves: 1,
            }]),
        )
        .expect("parse");
        assert_eq!(m.shortcuts[0].id(), Some("d"));
        assert_eq!(m.shortcuts[1].id(), None);
        bind_gates(&mut m, None);
        assert_eq!(m.gates[0].id.as_deref(), Some("gate"));
    }
}
