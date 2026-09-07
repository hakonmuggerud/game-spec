//! LANE: world. Gates (`X`) and shortcut doors (`=`) — the gameplay rules of `world.js:gateStatus` / `openGate`
//! / `shortcutSide` / `shortcutStatus` / `openShortcut` and the saved-state restore of `world.js:loadZone`
//! (DESIGN.md §3.6), without the meshes.
//!
//! A closed gate or barred door is a solid, sight-blocking cell (`CellKind::Gate` / `CellKind::Shortcut`).
//! Opening rewrites the cell — every cell of a door group — to `Floor` in the [`ParsedMap`], so `is_solid`,
//! `los` and the BFS fields agree with no extra state, and records the door's stable id in the save so it stays
//! open across runs. The per-run "already open" flags the JS keeps on the gate / shortcut records live in
//! [`ZoneDoors`], parallel to `ParsedMap::gates` / `ParsedMap::shortcuts`.

use crate::events::SimEvent;
use crate::grid::center;
use crate::save::SaveData;
use std::collections::BTreeMap;
use undercroft_data::map::ShortcutCell;
use undercroft_data::zone::{Facing, ZoneDef};
use undercroft_data::{CellKind, ParsedMap};

/// Which side of a shortcut door a world point is on (`world.js:shortcutSide`): `Far` is the `openFrom` side,
/// the only one the bars can be lifted from; `Near` is the barred side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Near,
    Far,
}

/// The per-run open flags of a zone's gates and shortcuts (`g.open` / `s.open` on the JS records), indexed
/// like `ParsedMap::gates` / `ParsedMap::shortcuts`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ZoneDoors {
    pub gates: Vec<bool>,
    pub shortcuts: Vec<bool>,
}

impl ZoneDoors {
    /// Everything closed, sized for the map.
    pub fn closed(m: &ParsedMap) -> ZoneDoors {
        ZoneDoors {
            gates: vec![false; m.gates.len()],
            shortcuts: vec![false; m.shortcuts.len()],
        }
    }

    /// `g.open` for the gate at that index.
    pub fn gate_open(&self, i: usize) -> bool {
        self.gates.get(i).copied().unwrap_or(false)
    }

    /// `s.open` for the shortcut at that index.
    pub fn shortcut_open(&self, i: usize) -> bool {
        self.shortcuts.get(i).copied().unwrap_or(false)
    }
}

/// `world.js:gateStatus(g)` — `{locked, tool, toolName, open}` for a gate in a zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateStatus {
    pub open: bool,
    /// Closed and the save lacks the zone's gate tool.
    pub locked: bool,
    /// `zone.meta.gate.tool`.
    pub tool: Option<String>,
    /// `TOOLS[tool] || tool`.
    pub tool_name: Option<String>,
}

/// `world.js:shortcutStatus(s, wx, wz)` — `{open, id, name, openFrom, side, canOpen}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutStatus {
    pub open: bool,
    pub id: Option<String>,
    pub name: Option<String>,
    pub open_from: Option<Facing>,
    pub side: Side,
    /// Closed and looked at from the far side.
    pub can_open: bool,
}

/// `world.js:loadZone` — restore the doors this save has already opened in `zone_id`: every gate whose id is in
/// `save.gatesOpened[zone]` and every bound shortcut whose id is in `save.shortcuts[zone]` becomes `Floor` and
/// is flagged open in the returned [`ZoneDoors`]. A shortcut with no `SHORTCUTS` entry is never restored (the JS
/// tests `!!s.id && scOpen.includes(s.id)`), nor is a gate without an id. Call it on the freshly parsed map of a
/// run, before anything routes over it.
pub fn apply_saved_openings(m: &mut ParsedMap, zone_id: &str, save: &SaveData) -> ZoneDoors {
    let mut doors = ZoneDoors::closed(m);
    for (i, g) in m.gates.iter().enumerate() {
        let open =
            g.id.as_deref()
                .is_some_and(|id| save.gate_opened(zone_id, id));
        if open {
            doors.gates[i] = true;
            m.cells[g.marker.idx] = CellKind::Floor;
        }
    }
    for (i, s) in m.shortcuts.iter().enumerate() {
        let open = s.id().is_some_and(|id| save.shortcut_opened(zone_id, id));
        if open {
            doors.shortcuts[i] = true;
            m.cells[s.marker.idx] = CellKind::Floor;
        }
    }
    doors
}

/// `world.js:gateAt(cx, cz)` — the index of the gate record at that cell (open or not).
pub fn gate_at(m: &ParsedMap, cx: i32, cz: i32) -> Option<usize> {
    m.gates
        .iter()
        .position(|g| g.marker.cx == cx && g.marker.cz == cz)
}

/// `world.js:shortcutAt(cx, cz)` — the index of the shortcut record at that cell (open or not).
pub fn shortcut_at(m: &ParsedMap, cx: i32, cz: i32) -> Option<usize> {
    m.shortcuts
        .iter()
        .position(|s| s.marker.cx == cx && s.marker.cz == cz)
}

/// `TOOLS[tool] || tool` — the display name of a tool id.
fn tool_display(zone_tools: &BTreeMap<String, String>, tool: &str) -> String {
    zone_tools
        .get(tool)
        .cloned()
        .unwrap_or_else(|| tool.to_string())
}

/// `world.js:gateStatus(g)` — `open` is the gate's run flag (`None` gate → not open, locked when the save lacks
/// the tool, exactly as the JS reads `!!g && !g.open`). `tools` is `Config::tools` (`TOOLS` in the JS).
pub fn gate_status(
    zone: &ZoneDef,
    save: &SaveData,
    tools: &BTreeMap<String, String>,
    open: Option<bool>,
) -> GateStatus {
    let tool = zone.gate.as_ref().map(|g| g.tool.clone());
    let has = tool.as_deref().is_some_and(|t| save.tools.get(t));
    let is_open = open.unwrap_or(false);
    GateStatus {
        open: is_open,
        locked: open.is_some() && !is_open && !has,
        tool_name: tool.as_deref().map(|t| tool_display(tools, t)),
        tool,
    }
}

/// `world.js:openGate(cx, cz, {force})` — E on a gate cell. Refused (with `gateLocked`) when the save lacks the
/// zone's gate tool and `force` is off; a cell with no unopened gate is a silent `false`. Opening turns the cell
/// to `Floor`, flags it in `doors`, records `g.id || tool || 'gate'` in `save.gatesOpened[zone]` and emits
/// `gateOpened` (the hunter lane drops its paths on it). `tools` is `Config::tools`.
///
/// A zone with `X` cells but no `META.gate` (a validator error) yields a `gateLocked` whose `tool` /
/// `tool_name` are empty, since the JS emits `null` there.
#[allow(clippy::too_many_arguments)]
pub fn open_gate(
    m: &mut ParsedMap,
    doors: &mut ZoneDoors,
    save: &mut SaveData,
    zone: &ZoneDef,
    tools: &BTreeMap<String, String>,
    cx: i32,
    cz: i32,
    force: bool,
) -> (bool, Vec<SimEvent>) {
    let Some(gi) = m
        .gates
        .iter()
        .enumerate()
        .position(|(i, g)| g.marker.cx == cx && g.marker.cz == cz && !doors.gate_open(i))
    else {
        return (false, vec![]);
    };
    let st = gate_status(zone, save, tools, Some(false));
    if st.locked && !force {
        return (
            false,
            vec![SimEvent::GateLocked {
                zone_id: zone.id.clone(),
                cx,
                cz,
                tool: st.tool.unwrap_or_default(),
                tool_name: st.tool_name.unwrap_or_default(),
            }],
        );
    }
    let g = &m.gates[gi];
    let idx = g.marker.idx;
    let gid =
        g.id.clone()
            .or_else(|| st.tool.clone())
            .unwrap_or_else(|| "gate".to_string());
    if gi >= doors.gates.len() {
        doors.gates.resize(m.gates.len(), false);
    }
    doors.gates[gi] = true;
    m.cells[idx] = CellKind::Floor;
    save.open_gate(&zone.id, &gid);
    (
        true,
        vec![SimEvent::GateOpened {
            zone_id: zone.id.clone(),
            id: gid,
            cx,
            cz,
            idx,
            tool: st.tool.unwrap_or_default(),
        }],
    )
}

/// `world.js:shortcutSide(s, wx, wz)` — `Far` when the point is on the door's `openFrom` side of its cell
/// centre, else `Near` (also `Near` for an unbound door: the JS reads `SHORTCUT_YAW[undefined] == null → 0`).
/// Row 0 is north, so `N` means `p.z − wz > 0`.
pub fn shortcut_side(m: &ParsedMap, s: &ShortcutCell, wx: f32, wz: f32) -> Side {
    let (px, pz) = center(m, s.marker.cx, s.marker.cz);
    let d = match s.open_from() {
        None => 0.0,
        Some(Facing::N) => pz - wz,
        Some(Facing::S) => wz - pz,
        Some(Facing::E) => wx - px,
        Some(Facing::W) => px - wx,
    };
    if d > 0.0 {
        Side::Far
    } else {
        Side::Near
    }
}

/// `world.js:shortcutStatus(s, wx, wz)` for the shortcut at index `i` (`None` → the JS's all-null status,
/// `side: 'near'`), with the door's run flag from `doors`.
pub fn shortcut_status(
    m: &ParsedMap,
    doors: &ZoneDoors,
    i: Option<usize>,
    wx: f32,
    wz: f32,
) -> ShortcutStatus {
    match i.and_then(|i| m.shortcuts.get(i).map(|s| (i, s))) {
        None => ShortcutStatus {
            open: false,
            id: None,
            name: None,
            open_from: None,
            side: Side::Near,
            can_open: false,
        },
        Some((i, s)) => {
            let open = doors.shortcut_open(i);
            let side = shortcut_side(m, s, wx, wz);
            ShortcutStatus {
                open,
                id: s.id().map(str::to_string),
                name: s.def.as_ref().map(|d| d.name.clone()),
                open_from: s.open_from(),
                side,
                can_open: !open && side == Side::Far,
            }
        }
    }
}

/// `world.js:openShortcut(cx, cz, {force, from})` — E on a barred cell. Refused silently from the barred side
/// unless `force` (`from` is the player's position). Opening turns every cell of the door group (all cells
/// bound to the same `SHORTCUTS` id; just this cell when unbound) to `Floor`, flags them in `doors`, records
/// the id in `save.shortcuts[zone]` (bound doors only) and emits `shortcutOpened` (hunter / npc drop their
/// paths). An unbound door emits the event with empty `id` / `name`, where the JS sends `undefined`.
#[allow(clippy::too_many_arguments)]
pub fn open_shortcut(
    m: &mut ParsedMap,
    doors: &mut ZoneDoors,
    save: &mut SaveData,
    zone_id: &str,
    cx: i32,
    cz: i32,
    from: (f32, f32),
    force: bool,
) -> (bool, Vec<SimEvent>) {
    let Some(si) = m
        .shortcuts
        .iter()
        .enumerate()
        .position(|(i, s)| s.marker.cx == cx && s.marker.cz == cz && !doors.shortcut_open(i))
    else {
        return (false, vec![]);
    };
    if !force && shortcut_side(m, &m.shortcuts[si], from.0, from.1) != Side::Far {
        return (false, vec![]);
    }
    if doors.shortcuts.len() < m.shortcuts.len() {
        doors.shortcuts.resize(m.shortcuts.len(), false);
    }
    let id = m.shortcuts[si].id().map(str::to_string);
    let name = m.shortcuts[si]
        .def
        .as_ref()
        .map(|d| d.name.clone())
        .unwrap_or_default();
    let idx = m.shortcuts[si].marker.idx;
    let group: Vec<usize> = match &id {
        Some(id) => m
            .shortcuts
            .iter()
            .enumerate()
            .filter(|(_, o)| o.id() == Some(id.as_str()))
            .map(|(i, _)| i)
            .collect(),
        None => vec![si],
    };
    for i in group {
        doors.shortcuts[i] = true;
        let ci = m.shortcuts[i].marker.idx;
        m.cells[ci] = CellKind::Floor;
    }
    if let Some(id) = &id {
        save.open_shortcut(zone_id, id);
    }
    (
        true,
        vec![SimEvent::ShortcutOpened {
            zone_id: zone_id.to_string(),
            id: id.unwrap_or_default(),
            name,
            cx,
            cz,
            idx,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{bfs_solid, is_solid, los};
    use undercroft_data::map::{bind_gates, parse_map};
    use undercroft_data::zone::{GateDef, ShortcutDef};
    use undercroft_data::GameData;

    /// A 9×7 room split by a wall down column 4 with a two-cell door (`=`) at rows 2–3 and a gate (`X`) at
    /// row 5.
    fn room() -> (ParsedMap, Vec<ShortcutDef>) {
        let rows: Vec<String> = [
            "#########", //
            "#...#...#", //
            "#...=...#", //
            "#...=...#", //
            "#...#...#", //
            "#...X...#", //
            "#########",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let defs = vec![ShortcutDef {
            id: "door".into(),
            name: "The Rood Door".into(),
            cells: vec![[4, 2], [4, 3]],
            open_from: Facing::E,
            from: "west".into(),
            to: "east".into(),
            saves: 4,
        }];
        let mut m = parse_map(&rows, 0, "t", None, Some(&defs)).expect("parse");
        bind_gates(
            &mut m,
            Some(&GateDef {
                tool: "prybar".into(),
                cells: vec![[4, 5]],
                opens: "east".into(),
            }),
        );
        (m, defs)
    }

    fn zone(m: &ParsedMap, defs: Vec<ShortcutDef>) -> ZoneDef {
        let d = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        let mut z = d.zone("undercroft").expect("undercroft").clone();
        z.id = "t".into();
        z.gate = Some(GateDef {
            tool: "prybar".into(),
            cells: m.gates.iter().map(|g| g.marker.cell()).collect(),
            opens: "east".into(),
        });
        z.shortcuts = defs;
        z
    }

    fn tools() -> BTreeMap<String, String> {
        BTreeMap::from([("prybar".to_string(), "Pry Bar".to_string())])
    }

    fn door(open_from: Facing) -> ShortcutCell {
        ShortcutCell {
            marker: undercroft_data::Marker {
                cx: 4,
                cz: 2,
                idx: 2 * 9 + 4,
                x: 4.5,
                z: 2.5,
            },
            def: Some(ShortcutDef {
                id: "d".into(),
                name: "d".into(),
                cells: vec![[4, 2]],
                open_from,
                from: "a".into(),
                to: "b".into(),
                saves: 0,
            }),
        }
    }

    #[test]
    fn side_per_facing() {
        let (m, _) = room();
        // N: far is north of the centre (smaller z); row 0 is north
        assert_eq!(shortcut_side(&m, &door(Facing::N), 4.5, 1.5), Side::Far);
        assert_eq!(shortcut_side(&m, &door(Facing::N), 4.5, 3.5), Side::Near);
        assert_eq!(shortcut_side(&m, &door(Facing::S), 4.5, 3.5), Side::Far);
        assert_eq!(shortcut_side(&m, &door(Facing::S), 4.5, 1.5), Side::Near);
        assert_eq!(shortcut_side(&m, &door(Facing::E), 5.5, 2.5), Side::Far);
        assert_eq!(shortcut_side(&m, &door(Facing::E), 3.5, 2.5), Side::Near);
        assert_eq!(shortcut_side(&m, &door(Facing::W), 3.5, 2.5), Side::Far);
        assert_eq!(shortcut_side(&m, &door(Facing::W), 5.5, 2.5), Side::Near);
        // exactly on the centre line is near (d > 0 is strict)
        assert_eq!(shortcut_side(&m, &door(Facing::E), 4.5, 2.5), Side::Near);
        // unbound → near from anywhere
        let mut unbound = door(Facing::E);
        unbound.def = None;
        assert_eq!(shortcut_side(&m, &unbound, 8.0, 2.5), Side::Near);
    }

    #[test]
    fn two_cell_door_opens_both_cells_from_the_far_side_only() {
        let (mut m, _) = room();
        let mut doors = ZoneDoors::closed(&m);
        let mut save = SaveData::default();
        assert_eq!(m.shortcuts.len(), 2);
        assert!(is_solid(&m, 4, 2) && is_solid(&m, 4, 3));
        assert!(!los(&m, 2.5, 2.5, 6.5, 2.5));
        // from the west (barred) side: refused, nothing changes
        let (ok, ev) = open_shortcut(&mut m, &mut doors, &mut save, "t", 4, 2, (3.5, 2.5), false);
        assert!(!ok && ev.is_empty());
        assert!(is_solid(&m, 4, 2));
        let st = shortcut_status(&m, &doors, shortcut_at(&m, 4, 2), 3.5, 2.5);
        assert_eq!(st.side, Side::Near);
        assert!(!st.can_open && !st.open);
        assert_eq!(st.id.as_deref(), Some("door"));
        // from the east (far) side: both cells open, the save records the id once, one event
        let st = shortcut_status(&m, &doors, shortcut_at(&m, 4, 3), 5.5, 3.5);
        assert!(st.can_open);
        let (ok, ev) = open_shortcut(&mut m, &mut doors, &mut save, "t", 4, 3, (5.5, 3.5), false);
        assert!(ok);
        assert_eq!(
            ev,
            vec![SimEvent::ShortcutOpened {
                zone_id: "t".into(),
                id: "door".into(),
                name: "The Rood Door".into(),
                cx: 4,
                cz: 3,
                idx: m.idx(4, 3),
            }]
        );
        assert!(!is_solid(&m, 4, 2) && !is_solid(&m, 4, 3));
        assert!(los(&m, 2.5, 2.5, 6.5, 2.5));
        assert!(bfs_solid(&m, 1, 1).reachable(m.idx(7, 1)));
        assert_eq!(doors.shortcuts, vec![true, true]);
        assert_eq!(save.shortcuts["t"], vec!["door"]);
        assert!(shortcut_status(&m, &doors, Some(0), 5.5, 2.5).open);
        // already open: a second press does nothing
        let (ok, ev) = open_shortcut(&mut m, &mut doors, &mut save, "t", 4, 2, (5.5, 2.5), false);
        assert!(!ok && ev.is_empty());
        // force from the barred side on a fresh map
        let (mut m2, _) = room();
        let mut d2 = ZoneDoors::closed(&m2);
        let (ok, _) = open_shortcut(&mut m2, &mut d2, &mut save, "t", 4, 2, (3.5, 2.5), true);
        assert!(ok && !is_solid(&m2, 4, 3));
        // the null status
        let st = shortcut_status(&m, &doors, None, 0.0, 0.0);
        assert_eq!(st.side, Side::Near);
        assert!(st.id.is_none() && !st.can_open);
    }

    #[test]
    fn unbound_door_opens_only_its_cell_and_is_not_saved() {
        let (mut m, _) = room();
        for s in &mut m.shortcuts {
            s.def = None;
        }
        let mut doors = ZoneDoors::closed(&m);
        let mut save = SaveData::default();
        // near from everywhere → refused without force
        assert!(!open_shortcut(&mut m, &mut doors, &mut save, "t", 4, 2, (5.5, 2.5), false).0);
        let (ok, ev) = open_shortcut(&mut m, &mut doors, &mut save, "t", 4, 2, (5.5, 2.5), true);
        assert!(ok);
        assert!(
            !is_solid(&m, 4, 2) && is_solid(&m, 4, 3),
            "only the pressed cell"
        );
        assert_eq!(doors.shortcuts, vec![true, false]);
        assert!(save.shortcuts.is_empty());
        assert!(
            matches!(&ev[0], SimEvent::ShortcutOpened { id, name, .. } if id.is_empty() && name.is_empty())
        );
    }

    #[test]
    fn gate_needs_the_tool_unless_forced() {
        let (mut m, defs) = room();
        let z = zone(&m, defs);
        let t = tools();
        let mut doors = ZoneDoors::closed(&m);
        let mut save = SaveData::default();
        assert_eq!(m.gates.len(), 1);
        assert_eq!(m.gates[0].id.as_deref(), Some("prybar"));
        let st = gate_status(&z, &save, &t, Some(false));
        assert_eq!(
            st,
            GateStatus {
                open: false,
                locked: true,
                tool: Some("prybar".into()),
                tool_name: Some("Pry Bar".into()),
            }
        );
        assert!(
            !gate_status(&z, &save, &t, None).locked,
            "no gate → not locked"
        );
        let (ok, ev) = open_gate(&mut m, &mut doors, &mut save, &z, &t, 4, 5, false);
        assert!(!ok);
        assert_eq!(
            ev,
            vec![SimEvent::GateLocked {
                zone_id: "t".into(),
                cx: 4,
                cz: 5,
                tool: "prybar".into(),
                tool_name: "Pry Bar".into(),
            }]
        );
        assert!(is_solid(&m, 4, 5));
        // not a gate cell
        assert!(!open_gate(&mut m, &mut doors, &mut save, &z, &t, 4, 4, true).0);
        // with the tool
        save.tools.prybar = true;
        assert!(!gate_status(&z, &save, &t, Some(false)).locked);
        let (ok, ev) = open_gate(&mut m, &mut doors, &mut save, &z, &t, 4, 5, false);
        assert!(ok);
        assert_eq!(
            ev,
            vec![SimEvent::GateOpened {
                zone_id: "t".into(),
                id: "prybar".into(),
                cx: 4,
                cz: 5,
                idx: m.idx(4, 5),
                tool: "prybar".into(),
            }]
        );
        assert!(!is_solid(&m, 4, 5));
        assert!(doors.gate_open(0));
        assert_eq!(save.gates_opened["t"], vec!["prybar"]);
        assert!(gate_status(&z, &save, &t, Some(doors.gate_open(0))).open);
        // pressed again: nothing
        assert!(!open_gate(&mut m, &mut doors, &mut save, &z, &t, 4, 5, false).0);
        // forced without the tool, and an id-less gate falls back to the tool name
        let (mut m2, _) = room();
        m2.gates[0].id = None;
        let mut d2 = ZoneDoors::closed(&m2);
        let mut s2 = SaveData::default();
        let (ok, ev) = open_gate(&mut m2, &mut d2, &mut s2, &z, &t, 4, 5, true);
        assert!(ok);
        assert!(matches!(&ev[0], SimEvent::GateOpened { id, .. } if id == "prybar"));
        assert_eq!(s2.gates_opened["t"], vec!["prybar"]);
        // no gate def at all: forced open records "gate", a refusal carries empty tool fields
        let mut z2 = z.clone();
        z2.gate = None;
        let (mut m3, _) = room();
        m3.gates[0].id = None;
        let mut d3 = ZoneDoors::closed(&m3);
        let mut s3 = SaveData::default();
        let (ok, ev) = open_gate(&mut m3, &mut d3, &mut s3, &z2, &t, 4, 5, false);
        assert!(!ok);
        assert!(matches!(&ev[0], SimEvent::GateLocked { tool, .. } if tool.is_empty()));
        assert!(open_gate(&mut m3, &mut d3, &mut s3, &z2, &t, 4, 5, true).0);
        assert_eq!(s3.gates_opened["t"], vec!["gate"]);
    }

    #[test]
    fn saved_openings_are_restored_on_load() {
        let (mut m, _) = room();
        let mut save = SaveData::default();
        save.open_gate("t", "prybar");
        save.open_shortcut("t", "door");
        save.open_shortcut("other", "door"); // another zone's list is ignored
        let doors = apply_saved_openings(&mut m, "t", &save);
        assert_eq!(doors.gates, vec![true]);
        assert_eq!(doors.shortcuts, vec![true, true]);
        assert!(!is_solid(&m, 4, 5) && !is_solid(&m, 4, 2) && !is_solid(&m, 4, 3));
        assert!(bfs_solid(&m, 1, 1).reachable(m.idx(7, 5)));
        // a fresh save: everything stays barred
        let (mut m2, _) = room();
        let d2 = apply_saved_openings(&mut m2, "t", &SaveData::default());
        assert_eq!(d2, ZoneDoors::closed(&m2));
        assert!(is_solid(&m2, 4, 5) && is_solid(&m2, 4, 2));
        // an unbound shortcut is never restored, even with a matching id elsewhere in the list
        let (mut m3, _) = room();
        for s in &mut m3.shortcuts {
            s.def = None;
        }
        let d3 = apply_saved_openings(&mut m3, "t", &save);
        assert_eq!(d3.shortcuts, vec![false, false]);
        assert!(is_solid(&m3, 4, 2));
        // a gate with no id is not restored either
        let (mut m4, _) = room();
        m4.gates[0].id = None;
        let d4 = apply_saved_openings(&mut m4, "t", &save);
        assert_eq!(d4.gates, vec![false]);
    }

    #[test]
    fn real_zone_shortcuts_round_trip_through_the_save() {
        let d = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        let z = d.zone("undercroft").expect("undercroft");
        let mut m = d.parse_zone("undercroft").expect("zone").expect("parse");
        assert!(!z.shortcuts.is_empty() && !m.shortcuts.is_empty());
        let mut doors = ZoneDoors::closed(&m);
        let mut save = SaveData::default();
        let def = &z.shortcuts[0];
        let (cx, cz) = (def.cells[0][0], def.cells[0][1]);
        let (px, pz) = center(&m, cx, cz);
        let [dx, dz] = def.open_from.delta();
        let far = (px + dx as f32, pz + dz as f32);
        let near = (px - dx as f32, pz - dz as f32);
        assert!(!open_shortcut(&mut m, &mut doors, &mut save, &z.id, cx, cz, near, false).0);
        assert!(open_shortcut(&mut m, &mut doors, &mut save, &z.id, cx, cz, far, false).0);
        for c in &def.cells {
            assert!(!is_solid(&m, c[0], c[1]));
        }
        assert!(save.shortcut_opened(&z.id, &def.id));
        // reload: the door is floor again from the save alone
        let mut m2 = d.parse_zone("undercroft").expect("zone").expect("parse");
        let d2 = apply_saved_openings(&mut m2, &z.id, &save);
        for c in &def.cells {
            assert!(!is_solid(&m2, c[0], c[1]));
        }
        assert_eq!(d2.shortcuts.iter().filter(|&&o| o).count(), def.cells.len());
    }
}
