//! `DESIGN.md` against the data. The map editor keeps the doc's map blocks and table numbers in step with what it
//! saves (`undercroft-editor`'s `design_md`, which tokenises the tables the way this file does); a hand edit to a
//! map, a table or `zones.ron` does not, so this test reads the doc's §3.2 zone table, contract-spot line,
//! shortcut table, §3.3 size/route table, the ASCII map blocks and the §3.8 hub block and compares every cell,
//! number and row with `GameData` plus `assets/fixtures/validate_all.json`. Every mismatch is reported with the
//! DESIGN.md line number, the zone (or shortcut), the documented value and the data value, all of them at once.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::PathBuf;
use undercroft_data::zone::CellXY;
use undercroft_data::{CellKind, DeepStyle, EntryKind, GameData, ParsedMap, ZoneDef, ZONE_ORDER};

// ---------------------------------------------------------------------------------------------------------------
// The fixture (`validate_all.json`), only the fields the doc quotes.

#[derive(Deserialize)]
struct Fixture {
    zones: BTreeMap<String, ZoneReport>,
}

#[derive(Deserialize)]
struct ZoneReport {
    stats: Stats,
}

#[derive(Deserialize)]
struct Stats {
    walkable: i64,
    #[serde(rename = "wallShare")]
    wall_share: f64,
    #[serde(rename = "P")]
    pillars: i64,
    shortcuts: Vec<ShortcutStat>,
    route: Route,
}

#[derive(Deserialize)]
struct ShortcutStat {
    id: String,
    detour: i64,
}

#[derive(Deserialize)]
struct Route {
    closed: i64,
    open: i64,
}

// ---------------------------------------------------------------------------------------------------------------
// Lenient markdown access.

struct Doc {
    lines: Vec<String>,
}

impl Doc {
    fn path() -> PathBuf {
        GameData::workspace_data_dir().join("../../DESIGN.md")
    }

    fn load() -> Doc {
        let path = Doc::path();
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Doc {
            lines: text.lines().map(str::to_string).collect(),
        }
    }

    /// Index of the first line at or after `from` that satisfies `pred`.
    fn find(&self, from: usize, pred: impl Fn(&str) -> bool) -> Option<usize> {
        (from..self.lines.len()).find(|&i| pred(&self.lines[i]))
    }

    /// Index of the line that starts with `prefix`; the doc must have it.
    fn heading(&self, prefix: &str) -> usize {
        self.find(0, |l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("DESIGN.md has no line starting with {prefix:?}"))
    }

    /// The body rows of the first markdown table at or after `from`, as `(1-based line, cells)`; the header and
    /// separator rows are skipped, cells are trimmed and stripped of backticks and bold markers.
    fn table(&self, from: usize) -> Vec<(usize, Vec<String>)> {
        let start = self
            .find(from, |l| l.starts_with('|'))
            .unwrap_or_else(|| panic!("DESIGN.md has no table after line {}", from + 1));
        (start..self.lines.len())
            .take_while(|&i| self.lines[i].starts_with('|'))
            .skip(2)
            .map(|i| {
                let mut cells: Vec<String> = self.lines[i]
                    .split('|')
                    .map(|c| c.trim().replace(['`', '*'], ""))
                    .collect();
                // the leading and trailing `|` produce an empty cell each
                cells.remove(0);
                cells.pop();
                (i + 1, cells)
            })
            .collect()
    }

    /// The lines of the fenced block that starts right after `heading` (the ``` fence itself excluded).
    fn fenced(&self, heading: usize) -> (usize, Vec<&str>) {
        assert_eq!(
            self.lines.get(heading + 1).map(String::as_str),
            Some("```"),
            "DESIGN.md:{}: expected a ``` fence after the heading",
            heading + 2
        );
        let first = heading + 2;
        let rows = (first..self.lines.len())
            .take_while(|&i| self.lines[i] != "```")
            .map(|i| self.lines[i].as_str())
            .collect();
        (first, rows)
    }
}

/// Every decimal number in `s`, in order ("**952 → 638** (67 %)" → `[952, 638, 67]`).
fn nums(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() || (ch == '.' && !cur.is_empty()) {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse() {
                out.push(n);
            }
            cur.clear();
        }
    }
    out
}

/// Every "(x,y)" / "(x, y)" cell in `s` with its byte offset, in order.
fn cells(s: &str) -> Vec<(usize, CellXY)> {
    let b = s.as_bytes();
    let number = |i: &mut usize| -> Option<i32> {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        std::str::from_utf8(&b[start..*i]).ok()?.parse().ok()
    };
    let spaces = |i: &mut usize| {
        while *i < b.len() && b[*i] == b' ' {
            *i += 1;
        }
    };
    let cell_at = |start: usize| -> Option<(CellXY, usize)> {
        let mut i = start + 1;
        let x = number(&mut i)?;
        spaces(&mut i);
        if b.get(i) != Some(&b',') {
            return None;
        }
        i += 1;
        spaces(&mut i);
        let z = number(&mut i)?;
        if b.get(i) != Some(&b')') {
            return None;
        }
        Some(([x, z], i + 1))
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'(' {
            if let Some((cell, end)) = cell_at(i) {
                out.push((i, cell));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn cell_list(s: &str) -> Vec<CellXY> {
    cells(s).into_iter().map(|(_, c)| c).collect()
}

/// The whitespace-separated word just before byte offset `at`.
fn word_before(s: &str, at: usize) -> &str {
    s[..at].split_whitespace().last().unwrap_or("")
}

// ---------------------------------------------------------------------------------------------------------------
// Mismatch collection.

#[derive(Default)]
struct Mismatches(Vec<String>);

impl Mismatches {
    fn check<T: PartialEq + Debug>(&mut self, line: usize, what: &str, documented: T, data: T) {
        if documented != data {
            self.0.push(format!(
                "DESIGN.md:{line}: {what}: documented {documented:?}, data {data:?}"
            ));
        }
    }

    fn note(&mut self, line: usize, message: String) {
        self.0.push(format!("DESIGN.md:{line}: {message}"));
    }
}

struct World {
    data: GameData,
    parsed: BTreeMap<String, ParsedMap>,
    fixture: Fixture,
}

impl World {
    fn load() -> World {
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        let parsed = data
            .zones
            .iter()
            .map(|z| {
                (
                    z.id.clone(),
                    undercroft_data::parse_zone(z).expect("zone parses"),
                )
            })
            .collect();
        let path = GameData::workspace_fixtures_dir().join("validate_all.json");
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let fixture =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        World {
            data,
            parsed,
            fixture,
        }
    }

    fn zone(&self, id: &str) -> Option<&ZoneDef> {
        self.data.zone(id)
    }

    fn stats(&self, id: &str) -> &Stats {
        &self.fixture.zones[id].stats
    }

    fn water_cells(&self, id: &str) -> i64 {
        self.parsed[id]
            .cells
            .iter()
            .filter(|&&k| k == CellKind::Water)
            .count() as i64
    }
}

fn grid(size: i32) -> Vec<f64> {
    vec![size as f64, size as f64]
}

// ---------------------------------------------------------------------------------------------------------------
// §3.2 zone table.

fn check_zone_table(doc: &Doc, w: &World, m: &mut Mismatches) {
    let rows = doc.table(doc.heading("### 3.2 "));
    let mut seen = Vec::new();
    for (line, c) in &rows {
        let id = c[0].as_str();
        let Some(zone) = w.zone(id) else {
            m.note(
                *line,
                format!("zone table row `{id}` names no zone in zones.ron"),
            );
            continue;
        };
        seen.push(id.to_string());
        let stats = w.stats(id);
        let tag = |what: &str| format!("{id} {what}");
        m.check(*line, &tag("name"), c[1].clone(), zone.name.clone());
        m.check(*line, &tag("grid"), nums(&c[2]), grid(zone.size));
        // entry: "S (31,58)" — the marker letter and the S/V cell of the parsed map
        let entry_kind = match c[3].chars().next() {
            Some('S') => Some(EntryKind::Stairs),
            Some('V') => Some(EntryKind::Elevator),
            _ => None,
        };
        m.check(*line, &tag("entry marker"), entry_kind, Some(zone.entry));
        m.check(
            *line,
            &tag("entry cell"),
            cell_list(&c[3]),
            w.parsed[id]
                .stairs
                .iter()
                .map(|s| s.marker.cell())
                .collect(),
        );
        m.check(
            *line,
            &tag("walkable"),
            nums(&c[4]),
            vec![stats.walkable as f64],
        );
        if c[5] == "bands" || c[6] == "bands" {
            m.check(
                *line,
                &tag("burn/lamp `bands`"),
                DeepStyle::Bands,
                zone.deep_style,
            );
        } else {
            m.check(
                *line,
                &tag("burn"),
                c[5].clone(),
                format!("×{}", zone.burn_mul),
            );
            m.check(
                *line,
                &tag("lamp"),
                c[6].clone(),
                format!("×{}", zone.lamp_mul),
            );
        }
        // captives: "Wick (4,47), Deacon (4,5)" — each cell is an NPC cell and the word before it is in that NPC's name
        let captives = cells(&c[8]);
        m.check(
            *line,
            &tag("captive count"),
            captives.len(),
            zone.npcs.len(),
        );
        for (at, cell) in captives {
            let word = word_before(&c[8], at);
            let named = zone
                .npcs
                .iter()
                .find(|(_, &npc_cell)| npc_cell == cell)
                .map(|(npc, _)| w.data.npcs.npcs[npc].name.clone());
            match named {
                Some(name) if name.split_whitespace().any(|part| part == word) => {}
                Some(name) => m.note(
                    *line,
                    format!("{id} captive at {cell:?}: documented as {word:?}, data names it {name:?}"),
                ),
                None => m.note(
                    *line,
                    format!("{id} captive {word} {cell:?}: no NPC at that cell in zones.ron (data {:?})", zone.npcs),
                ),
            }
        }
        // gate: "Pry Bar → X (15,6), NW crypt" or "—"
        match &zone.gate {
            None => m.check(*line, &tag("gate"), c[9].clone(), "—".to_string()),
            Some(gate) => {
                let tool = c[9].split('→').next().unwrap_or("").trim().to_string();
                m.check(
                    *line,
                    &tag("gate tool"),
                    tool,
                    w.data.config.tools[&gate.tool].clone(),
                );
                m.check(
                    *line,
                    &tag("gate cell"),
                    cell_list(&c[9]).first().copied(),
                    gate.cells.first().copied(),
                );
            }
        }
    }
    let expected: Vec<String> = ZONE_ORDER.iter().map(|s| s.to_string()).collect();
    m.check(
        rows.first().map_or(0, |r| r.0),
        "zone table rows",
        seen,
        expected,
    );
}

// ---------------------------------------------------------------------------------------------------------------
// The "Contract spots:" line (may wrap onto the following lines up to a blank one).

fn check_contract_spots(doc: &Doc, w: &World, m: &mut Mismatches) {
    let start = doc.heading("Items respawn each expedition. Contract spots:");
    let line = start + 1;
    let joined: String = (start..doc.lines.len())
        .take_while(|&i| !doc.lines[i].trim().is_empty())
        .map(|i| doc.lines[i].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let text = joined.split("Contract spots:").nth(1).unwrap_or("");
    let mut seen = Vec::new();
    for segment in text.split('·') {
        let segment = segment.trim().trim_end_matches('.');
        let word = segment.split_whitespace().next().unwrap_or("");
        let Some(zone) = w
            .data
            .zones
            .iter()
            .find(|z| z.name.strip_prefix("The ").unwrap_or(&z.name) == word)
        else {
            m.note(line, format!("contract spots: {word:?} names no zone"));
            continue;
        };
        seen.push(zone.id.clone());
        m.check(
            line,
            &format!("{} contract spots", zone.id),
            cell_list(segment),
            zone.spots.iter().map(|s| s.cell).collect(),
        );
    }
    let expected: Vec<String> = w
        .data
        .zones
        .iter()
        .filter(|z| !z.spots.is_empty())
        .map(|z| z.id.clone())
        .collect();
    m.check(line, "zones with contract spots", seen, expected);
}

// ---------------------------------------------------------------------------------------------------------------
// The shortcuts table.

fn check_shortcut_table(doc: &Doc, w: &World, m: &mut Mismatches) {
    let rows = doc.table(doc.heading("Shortcuts (`=`, §3.6"));
    let mut per_zone: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (line, c) in &rows {
        let zone_id = c[0].as_str();
        let Some(zone) = w.zone(zone_id) else {
            m.note(
                *line,
                format!("shortcut table row names unknown zone `{zone_id}`"),
            );
            continue;
        };
        let mut words = c[1].splitn(2, ' ');
        let id = words.next().unwrap_or("").to_string();
        let name = words.next().unwrap_or("").trim().to_string();
        let first_of_zone = !per_zone.contains_key(zone_id);
        per_zone
            .entry(zone_id.to_string())
            .or_default()
            .push(id.clone());
        let tag = |what: &str| format!("{zone_id} {id} {what}");
        let Some(def) = zone.shortcuts.iter().find(|s| s.id == id) else {
            m.note(
                *line,
                format!("{zone_id}: shortcut `{id}` is not in zones.ron"),
            );
            continue;
        };
        m.check(*line, &tag("name"), name, def.name.clone());
        m.check(*line, &tag("cells"), cell_list(&c[2]), def.cells.clone());
        m.check(
            *line,
            &tag("opens from"),
            c[3].clone(),
            format!("{:?}", def.open_from),
        );
        let stats = w.stats(zone_id);
        match stats.shortcuts.iter().find(|s| s.id == id) {
            Some(stat) => {
                m.check(
                    *line,
                    &tag("detour removed"),
                    nums(&c[5]),
                    vec![stat.detour as f64],
                );
                m.check(
                    *line,
                    &tag("zones.ron `saves` vs the validator's detour"),
                    def.saves as i64,
                    stat.detour,
                );
            }
            None => m.note(
                *line,
                format!("{zone_id}: validate_all.json has no shortcut `{id}`"),
            ),
        }
        if first_of_zone {
            m.check(
                *line,
                &tag("full-clear route shut → open"),
                nums(&c[6]),
                vec![stats.route.closed as f64, stats.route.open as f64],
            );
        } else if !c[6].is_empty() {
            m.note(
                *line,
                format!("{zone_id}: the route cell should only be on the zone's first row"),
            );
        }
    }
    for zone in &w.data.zones {
        let documented = per_zone.remove(&zone.id).unwrap_or_default();
        let ids: Vec<String> = zone.shortcuts.iter().map(|s| s.id.clone()).collect();
        m.check(
            rows.first().map_or(0, |r| r.0),
            &format!("{} shortcut rows", zone.id),
            documented,
            ids,
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// §3.3 size/route table.

fn check_scale_table(doc: &Doc, w: &World, m: &mut Mismatches) {
    let rows = doc.table(doc.heading("### 3.3 "));
    let mut seen = Vec::new();
    for (line, c) in &rows {
        let id = c[0].as_str();
        let Some(zone) = w.zone(id) else {
            m.note(*line, format!("§3.3 row names unknown zone `{id}`"));
            continue;
        };
        seen.push(id.to_string());
        let stats = w.stats(id);
        let tag = |what: &str| format!("{id} {what}");
        m.check(*line, &tag("grid"), nums(&c[1]), grid(zone.size));
        m.check(
            *line,
            &tag("cells"),
            nums(&c[2]),
            vec![(zone.size * zone.size) as f64],
        );
        m.check(
            *line,
            &tag("walkable (target)"),
            nums(&c[3]),
            vec![stats.walkable as f64, zone.targets.walkable as f64],
        );
        let pct = |share: f64| format!("{:.1}", share * 100.0);
        let documented: Vec<String> = nums(&c[4]).iter().map(|n| pct(n / 100.0)).collect();
        m.check(
            *line,
            &tag("walls % (target)"),
            documented,
            vec![pct(stats.wall_share), pct(zone.targets.wall_share as f64)],
        );
        m.check(
            *line,
            &tag("pillars"),
            nums(&c[5]),
            vec![stats.pillars as f64],
        );
        let water = if c[6] == "—" {
            0
        } else {
            nums(&c[6]).first().copied().unwrap_or(-1.0) as i64
        };
        m.check(*line, &tag("water"), water, w.water_cells(id));
        let route = nums(&c[7]);
        let ratio = ((stats.route.open as f64) * 100.0 / (stats.route.closed as f64)).round();
        m.check(
            *line,
            &tag("route shut → open (open %)"),
            route,
            vec![stats.route.closed as f64, stats.route.open as f64, ratio],
        );
    }
    let expected: Vec<String> = ZONE_ORDER.iter().map(|s| s.to_string()).collect();
    m.check(
        rows.first().map_or(0, |r| r.0),
        "§3.3 table rows",
        seen,
        expected,
    );
}

// ---------------------------------------------------------------------------------------------------------------
// The ASCII map blocks: "#### <name> — N×N (`maps/<id>.txt`)" + a fenced block of two column-index header lines
// and one "%3d <row>" line per row.

fn check_map_blocks(doc: &Doc, w: &World, m: &mut Mismatches) {
    for zone in &w.data.zones {
        let marker = format!("(`maps/{}.txt`)", zone.id);
        let Some(heading) = doc.find(0, |l| l.starts_with("#### ") && l.contains(&marker)) else {
            m.note(0, format!("{}: no `#### … {marker}` map block", zone.id));
            continue;
        };
        let title = &doc.lines[heading]["#### ".len()..];
        let (name, rest) = title.split_once(" — ").unwrap_or((title, ""));
        m.check(
            heading + 1,
            &format!("{} map block name", zone.id),
            name.to_string(),
            zone.name.clone(),
        );
        m.check(
            heading + 1,
            &format!("{} map block size", zone.id),
            nums(rest).into_iter().take(2).collect::<Vec<_>>(),
            grid(zone.size),
        );
        let (first, block) = doc.fenced(heading);
        let size = zone.size as usize;
        let tens: String = (0..size)
            .map(|x| {
                if x % 10 == 0 {
                    char::from(b'0' + (x / 10) as u8)
                } else {
                    ' '
                }
            })
            .collect();
        let units: String = (0..size)
            .map(|x| char::from(b'0' + (x % 10) as u8))
            .collect();
        m.check(
            first + 1,
            &format!("{} column header (tens)", zone.id),
            block.first().copied(),
            Some(format!("    {tens}").as_str()),
        );
        m.check(
            first + 2,
            &format!("{} column header (units)", zone.id),
            block.get(1).copied(),
            Some(format!("    {units}").as_str()),
        );
        let rows = &block[block.len().min(2)..];
        m.check(
            first + 3,
            &format!("{} map block row count", zone.id),
            rows.len(),
            zone.rows.len(),
        );
        for (i, (doc_row, data_row)) in rows.iter().zip(&zone.rows).enumerate() {
            let expected = format!("{i:3} {data_row}");
            if *doc_row != expected {
                m.note(
                    first + 3 + i,
                    format!(
                        "{} row {i}: documented {doc_row:?}, data {expected:?}",
                        zone.id
                    ),
                );
            }
        }
    }
    // the hub block: bare rows, no prefix
    let heading = doc.heading("### 3.8 Hub");
    let (first, block) = doc.fenced(heading);
    m.check(
        first + 1,
        "hub rows",
        block,
        w.data.hub_rows.iter().map(String::as_str).collect(),
    );
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn design_md_matches_the_data() {
    let doc = Doc::load();
    let w = World::load();
    let mut m = Mismatches::default();
    check_zone_table(&doc, &w, &mut m);
    check_contract_spots(&doc, &w, &mut m);
    check_shortcut_table(&doc, &w, &mut m);
    check_scale_table(&doc, &w, &mut m);
    check_map_blocks(&doc, &w, &mut m);
    assert!(
        m.0.is_empty(),
        "DESIGN.md disagrees with assets/data + validate_all.json in {} place(s):\n  {}",
        m.0.len(),
        m.0.join("\n  ")
    );
}
