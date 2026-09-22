//! The parts of `DESIGN.md` a map save keeps in step with the data, each replaced in place so everything else
//! (prose, spacing, the hub block, the other zones) comes through byte for byte:
//!
//! * `rewrite` — the zone's ASCII map block (§3.2): a heading `#### The Undercroft — 62×62 (`maps/undercroft.txt`)`,
//!   then a fenced block of two column-index header lines (tens digits, units digits; four-space prefix) and one
//!   line per row formatted as `%3d ` + the row. Exactly the row lines are replaced.
//! * `rewrite_tables` — the numbers and `(x,y)` cells the tables quote for the zone: its §3.2 zone-table row
//!   (entry cell, walkable, captive cells, gate cell), its segment of the "Contract spots:" line, its rows of the
//!   shortcut table (cells, opens from, detour removed, the route pair) and its §3.3 row (walkable, walls %,
//!   pillars, water, route shut → open and the percentage). Only the tokens are spliced; the words around them
//!   stay. What is *not* a token — a captive's name, a renamed door, the region notes of §3.4 — is reported in
//!   a note and left for a hand edit.
//!
//! `undercroft-data`'s `design_md_matches_the_data` test reads the same tables with the same tokenisation, so
//! what this module writes is what that test checks.

/// The default location: `DESIGN.md` next to `assets/` (`<data_dir>/../../DESIGN.md`).
pub fn default_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("DESIGN.md"))
        .unwrap_or_else(|| data_dir.join("..").join("..").join("DESIGN.md"))
}

/// `text` with the row lines of zone `id`'s block replaced by `rows`. Refuses (naming what did not match)
/// rather than guess: no or several headings for the zone, no fence, a row line whose `%3d ` prefix is not
/// the expected index, or a block whose row count differs from `rows.len()`.
pub fn rewrite(text: &str, id: &str, rows: &[String]) -> Result<String, String> {
    let needle = format!("(`maps/{id}.txt`)");
    // line endings kept, so the joined result is byte-identical outside the replaced lines
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let body = |i: usize| lines.get(i).map(|l| l.trim_end_matches('\n'));
    let headings: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("#### ") && l.contains(&needle))
        .map(|(i, _)| i)
        .collect();
    let h = match headings.as_slice() {
        [h] => *h,
        [] => {
            return Err(format!(
                "DESIGN.md: no `#### … {needle}` heading for zone {id}"
            ))
        }
        many => {
            return Err(format!(
                "DESIGN.md: {} headings for zone {id} (lines {:?})",
                many.len(),
                many.iter().map(|i| i + 1).collect::<Vec<_>>()
            ))
        }
    };
    let line_no = |i: usize| i + 1;
    if body(h + 1) != Some("```") {
        return Err(format!(
            "DESIGN.md line {}: expected the ``` fence after the {id} heading",
            line_no(h + 1)
        ));
    }
    for i in [h + 2, h + 3] {
        if !body(i).is_some_and(|l| l.starts_with("    ")) {
            return Err(format!(
                "DESIGN.md line {}: expected a column-index header line (four-space prefix) in the {id} block",
                line_no(i)
            ));
        }
    }
    let first = h + 4;
    for (z, _) in rows.iter().enumerate() {
        let want = format!("{z:3} ");
        let ok = body(first + z).is_some_and(|l| l.starts_with(&want) && l.len() > want.len());
        if !ok {
            return Err(format!(
                "DESIGN.md line {}: expected row {z} of the {id} block (`{want}` + the row); the block has \
                 fewer rows than the map or a different prefix format",
                line_no(first + z)
            ));
        }
    }
    let end = first + rows.len();
    if body(end) != Some("```") {
        return Err(format!(
            "DESIGN.md line {}: expected the closing ``` fence after row {} of the {id} block (the block \
             has more rows than the map)",
            line_no(end),
            rows.len().saturating_sub(1)
        ));
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        if (first..end).contains(&i) {
            out.push_str(&format!("{:3} {}\n", i - first, rows[i - first]));
        } else {
            out.push_str(line);
        }
    }
    Ok(out)
}

use undercroft_data::zone::CellXY;
use undercroft_data::{NpcTable, ZoneDef};
use undercroft_sim::validate::Stats;

/// What the §3.2 / §3.3 tables quote for one zone, taken from the data about to be written.
pub struct ZoneFacts<'a> {
    pub zone: &'a ZoneDef,
    /// The parsed `S` / `V` cell.
    pub entry: Option<CellXY>,
    /// Water cells in the parsed map.
    pub water: i64,
    /// For the captives column: the word before each `(x,y)` must be part of that NPC's `npcs.ron` name.
    pub npcs: &'a NpcTable,
    /// The validator's stats for the zone (what `validate_all.json` will hold).
    pub stats: &'a Stats,
}

/// `text` with every number and cell the tables quote for `facts.zone` replaced by the data's value. Fails
/// (naming the DESIGN.md line) when a table, the zone's row, or a token the data needs is not there; a cell
/// whose words do not identify the data (a captive name no NPC carries, a door id not in `zones.ron`) is left
/// alone with a note in `notes`.
pub fn rewrite_tables(
    text: &str,
    facts: &ZoneFacts,
    notes: &mut Vec<String>,
) -> Result<String, String> {
    let mut lines = Lines(text.split_inclusive('\n').map(str::to_string).collect());
    zone_table(&mut lines, facts, notes)?;
    contract_spots(&mut lines, facts, notes)?;
    shortcut_table(&mut lines, facts, notes)?;
    scale_table(&mut lines, facts)?;
    Ok(lines.0.concat())
}

/// The document as lines, each with its line ending.
struct Lines(Vec<String>);

impl Lines {
    fn body(&self, i: usize) -> &str {
        self.0[i].trim_end_matches(['\n', '\r'])
    }

    /// Index of the line that starts with `prefix`.
    fn heading(&self, prefix: &str) -> Result<usize, String> {
        (0..self.0.len())
            .find(|&i| self.body(i).starts_with(prefix))
            .ok_or_else(|| format!("DESIGN.md: no line starting with {prefix:?}"))
    }

    /// The body rows of the first markdown table at or after `from` (header and separator skipped).
    fn table_rows(&self, from: usize) -> Vec<usize> {
        let Some(start) = (from..self.0.len()).find(|&i| self.body(i).starts_with('|')) else {
            return Vec::new();
        };
        (start..self.0.len())
            .take_while(|&i| self.body(i).starts_with('|'))
            .skip(2)
            .collect()
    }

    /// The body row at or after `from` whose first cell (backticks / bold stripped) is `key`.
    fn row_keyed(&self, from: usize, key: &str) -> Option<usize> {
        self.table_rows(from)
            .into_iter()
            .find(|&i| pieces(&self.0[i]).get(1).is_some_and(|c| clean(c) == key))
    }
}

/// The `|`-separated pieces of a table row, spacing and line ending intact: `pieces.join("|")` is the row and
/// the drift test's cell `k` is `pieces[k + 1]`.
fn pieces(line: &str) -> Vec<String> {
    line.split('|').map(str::to_string).collect()
}

/// A cell as the drift test compares it: trimmed, backticks and bold markers removed.
fn clean(piece: &str) -> String {
    piece.trim().replace(['`', '*'], "")
}

fn cell_mut(row: &mut [String], k: usize, line_no: usize) -> Result<&mut String, String> {
    row.get_mut(k + 1)
        .ok_or_else(|| format!("DESIGN.md line {line_no}: the table row has no column {k}"))
}

/// `piece` with its trimmed content replaced by `new` (the surrounding spaces and line ending kept).
fn replace_trimmed(piece: &str, new: &str) -> String {
    let start = piece.len() - piece.trim_start().len();
    let end = start + piece.trim().len();
    format!("{}{new}{}", &piece[..start], &piece[end..])
}

/// Byte spans of every decimal number in `s`, tokenised as the drift test's `nums` does (digits, with `.`
/// absorbed after a digit; a token that is not a number, like `1.2.3`, is skipped).
fn num_spans(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if s[start..i].parse::<f64>().is_ok() {
                out.push((start, i));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Byte spans of every `(x,y)` / `(x, y)` cell in `s`, with the cell (the drift test's `cells`).
fn cell_spans(s: &str) -> Vec<(usize, usize, CellXY)> {
    let b = s.as_bytes();
    let number = |i: &mut usize| -> Option<i32> {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        s[start..*i].parse().ok()
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
                out.push((i, end, cell));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `s` with the k-th span replaced by `values[k]` (spans ascending and non-overlapping; extra spans stay).
fn splice(s: &str, spans: &[(usize, usize)], values: &[String]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    for (&(start, end), v) in spans.iter().zip(values) {
        out.push_str(&s[at..start]);
        out.push_str(v);
        at = end;
    }
    out.push_str(&s[at..]);
    out
}

/// `piece` with its first `values.len()` numbers replaced; fails when it holds fewer.
fn replace_nums(piece: &str, values: &[String], what: &str) -> Result<String, String> {
    let spans = num_spans(piece);
    if spans.len() < values.len() {
        return Err(format!(
            "{what}: expected {} number(s) in {:?}, found {}",
            values.len(),
            piece.trim(),
            spans.len()
        ));
    }
    Ok(splice(piece, &spans, values))
}

fn fmt_cell(c: CellXY) -> String {
    format!("({},{})", c[0], c[1])
}

/// `piece` with its `(x,y)` cells replaced by `cells` in order — all of them when `exact`, else the first
/// `cells.len()`; fails when it holds fewer (or, when `exact`, a different number).
fn replace_cells(piece: &str, cells: &[CellXY], exact: bool, what: &str) -> Result<String, String> {
    let spans: Vec<(usize, usize)> = cell_spans(piece).iter().map(|&(s, e, _)| (s, e)).collect();
    if spans.len() < cells.len() || (exact && spans.len() != cells.len()) {
        return Err(format!(
            "{what}: expected {} cell(s) in {:?}, found {}",
            cells.len(),
            piece.trim(),
            spans.len()
        ));
    }
    let values: Vec<String> = cells.iter().map(|&c| fmt_cell(c)).collect();
    Ok(splice(piece, &spans, &values))
}

fn word_before(s: &str, at: usize) -> &str {
    s[..at].split_whitespace().last().unwrap_or("")
}

/// The §3.2 zone table: the zone's row — entry cell, walkable, captive cells, gate cell.
fn zone_table(lines: &mut Lines, f: &ZoneFacts, notes: &mut Vec<String>) -> Result<(), String> {
    let id = &f.zone.id;
    let h = lines.heading("### 3.2 ")?;
    let row = lines
        .row_keyed(h, id)
        .ok_or_else(|| format!("DESIGN.md: the §3.2 zone table has no row for {id}"))?;
    let no = row + 1;
    let mut p = pieces(&lines.0[row]);
    let what = |col: &str| format!("DESIGN.md line {no}: §3.2 {id} {col}");
    if let Some(entry) = f.entry {
        let c = cell_mut(&mut p, 3, no)?;
        *c = replace_cells(c, &[entry], true, &what("entry"))?;
    }
    let c = cell_mut(&mut p, 4, no)?;
    *c = replace_nums(c, &[f.stats.walkable.to_string()], &what("walkable"))?;
    // captives: "Wick (4,47), Deacon (4,5)" — the word before each cell names the NPC
    let c = cell_mut(&mut p, 8, no)?;
    let text = c.clone();
    let spans = cell_spans(&text);
    if spans.len() != f.zone.npcs.len() {
        notes.push(format!(
            "DESIGN.md:{no}: §3.2 lists {} captive cell(s) for {id}, zones.ron has {} NPC(s); check it by hand",
            spans.len(),
            f.zone.npcs.len()
        ));
    }
    let mut values = Vec::new();
    for &(start, end, old) in &spans {
        let word = word_before(&text, start);
        let named = f.zone.npcs.iter().find(|(npc, _)| {
            f.npcs
                .npcs
                .get(*npc)
                .is_some_and(|n| n.name.split_whitespace().any(|part| part == word))
        });
        match named {
            Some((_, &cell)) => values.push(fmt_cell(cell)),
            None => {
                notes.push(format!(
                    "DESIGN.md:{no}: §3.2 captive {word:?} at {} names no NPC of {id}; left as is",
                    fmt_cell(old)
                ));
                values.push(text[start..end].to_string());
            }
        }
    }
    let spans: Vec<(usize, usize)> = spans.iter().map(|&(s, e, _)| (s, e)).collect();
    *c = splice(&text, &spans, &values);
    if let Some(first) = f.zone.gate.as_ref().and_then(|g| g.cells.first()) {
        let c = cell_mut(&mut p, 9, no)?;
        *c = replace_cells(c, &[*first], false, &what("gate cell"))?;
    }
    lines.0[row] = p.join("|");
    Ok(())
}

/// The "Contract spots:" line (which may wrap): the zone's `·`-separated segment, its cells in spot order.
fn contract_spots(lines: &mut Lines, f: &ZoneFacts, notes: &mut Vec<String>) -> Result<(), String> {
    let zone = f.zone;
    let h = lines.heading("Items respawn each expedition. Contract spots:")?;
    let end = (h..lines.0.len())
        .find(|&i| lines.body(i).trim().is_empty())
        .unwrap_or(lines.0.len());
    let joined: String = lines.0[h..end].concat();
    let marker = "Contract spots:";
    let after = joined.find(marker).map_or(0, |i| i + marker.len());
    let mut segments = Vec::new();
    let mut seg_start = after;
    for (i, sep) in joined[after..].match_indices('·') {
        segments.push((seg_start, after + i));
        seg_start = after + i + sep.len();
    }
    segments.push((seg_start, joined.len()));
    let short = zone.name.strip_prefix("The ").unwrap_or(&zone.name);
    let segment = segments
        .into_iter()
        .find(|&(a, b)| joined[a..b].split_whitespace().next() == Some(short));
    let Some((a, b)) = segment else {
        if !zone.spots.is_empty() {
            notes.push(format!(
                "DESIGN.md:{}: the contract-spot line has no segment for {short}; check it by hand",
                h + 1
            ));
        }
        return Ok(());
    };
    let cells: Vec<CellXY> = zone.spots.iter().map(|s| s.cell).collect();
    let new = replace_cells(
        &joined[a..b],
        &cells,
        true,
        &format!("DESIGN.md line {}: contract spots of {}", h + 1, zone.id),
    )?;
    let out = format!("{}{new}{}", &joined[..a], &joined[b..]);
    let parts: Vec<String> = out.split_inclusive('\n').map(str::to_string).collect();
    lines.0.splice(h..end, parts);
    Ok(())
}

/// The shortcut table: the zone's rows — cells, opens from, detour removed, and the route pair on its first row.
fn shortcut_table(lines: &mut Lines, f: &ZoneFacts, notes: &mut Vec<String>) -> Result<(), String> {
    let zone = f.zone;
    let id = &zone.id;
    let h = lines.heading("Shortcuts (`=`, §3.6")?;
    let rows: Vec<usize> = lines
        .table_rows(h)
        .into_iter()
        .filter(|&i| pieces(&lines.0[i]).get(1).is_some_and(|c| &clean(c) == id))
        .collect();
    let mut documented = Vec::new();
    for (n, &row) in rows.iter().enumerate() {
        let no = row + 1;
        let mut p = pieces(&lines.0[row]);
        let sc_id = clean(cell_mut(&mut p, 1, no)?)
            .split(' ')
            .next()
            .unwrap_or("")
            .to_string();
        documented.push(sc_id.clone());
        let Some(def) = zone.shortcuts.iter().find(|s| s.id == sc_id) else {
            notes.push(format!(
                "DESIGN.md:{no}: shortcut `{sc_id}` is not in zones.ron for {id}; left as is"
            ));
            continue;
        };
        let what = |col: &str| format!("DESIGN.md line {no}: shortcut {sc_id} {col}");
        let c = cell_mut(&mut p, 2, no)?;
        *c = replace_cells(c, &def.cells, true, &what("cells"))?;
        let c = cell_mut(&mut p, 3, no)?;
        *c = replace_trimmed(c, &format!("{:?}", def.open_from));
        if let Some(stat) = f.stats.shortcuts.iter().find(|s| s.id == sc_id) {
            let c = cell_mut(&mut p, 5, no)?;
            *c = replace_nums(c, &[stat.detour.to_string()], &what("detour removed"))?;
        }
        if n == 0 {
            if let Some(route) = &f.stats.route {
                let c = cell_mut(&mut p, 6, no)?;
                *c = replace_nums(
                    c,
                    &[route.closed.to_string(), route.open.to_string()],
                    &what("full-clear route"),
                )?;
            }
        }
        lines.0[row] = p.join("|");
    }
    let ids: Vec<String> = zone.shortcuts.iter().map(|s| s.id.clone()).collect();
    if documented != ids {
        notes.push(format!(
            "DESIGN.md:{}: the shortcut table lists {documented:?} for {id}, zones.ron has {ids:?}; check it by hand",
            rows.first().map_or(h, |r| r + 1)
        ));
    }
    Ok(())
}

/// The §3.3 scale table: the zone's row — walkable (target), walls % (target), pillars, water, route.
fn scale_table(lines: &mut Lines, f: &ZoneFacts) -> Result<(), String> {
    let zone = f.zone;
    let id = &zone.id;
    let h = lines.heading("### 3.3 ")?;
    let row = lines
        .row_keyed(h, id)
        .ok_or_else(|| format!("DESIGN.md: the §3.3 table has no row for {id}"))?;
    let no = row + 1;
    let mut p = pieces(&lines.0[row]);
    let what = |col: &str| format!("DESIGN.md line {no}: §3.3 {id} {col}");
    let pct = |share: f64| format!("{:.1}", share * 100.0);
    let c = cell_mut(&mut p, 3, no)?;
    *c = replace_nums(
        c,
        &[
            f.stats.walkable.to_string(),
            zone.targets.walkable.to_string(),
        ],
        &what("walkable (target)"),
    )?;
    let c = cell_mut(&mut p, 4, no)?;
    *c = replace_nums(
        c,
        &[pct(f.stats.wall_share), pct(zone.targets.wall_share as f64)],
        &what("walls % (target)"),
    )?;
    let c = cell_mut(&mut p, 5, no)?;
    *c = replace_nums(c, &[f.stats.counts.p.to_string()], &what("pillars"))?;
    let c = cell_mut(&mut p, 6, no)?;
    if num_spans(c).is_empty() {
        if f.water > 0 {
            *c = replace_trimmed(c, &f.water.to_string());
        }
    } else {
        *c = replace_nums(c, &[f.water.to_string()], &what("water"))?;
    }
    if let Some(route) = &f.stats.route {
        let ratio = ((route.open as f64) * 100.0 / (route.closed as f64)).round() as i64;
        let c = cell_mut(&mut p, 7, no)?;
        *c = replace_nums(
            c,
            &[
                route.closed.to_string(),
                route.open.to_string(),
                ratio.to_string(),
            ],
            &what("route shut → open"),
        )?;
    }
    lines.0[row] = p.join("|");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::GameData;

    fn workspace_design_md() -> String {
        std::fs::read_to_string(default_path(&GameData::workspace_data_dir())).expect("DESIGN.md")
    }

    #[test]
    fn every_zone_block_round_trips_from_the_data() {
        let text = workspace_design_md();
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("loads");
        for zone in &data.zones {
            let same = rewrite(&text, &zone.id, &zone.rows).expect(&zone.id);
            assert_eq!(
                same, text,
                "{}: DESIGN.md already matches maps/{}.txt",
                zone.id, zone.id
            );
        }
    }

    #[test]
    fn one_changed_row_changes_exactly_that_line() {
        let text = workspace_design_md();
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("loads");
        let zone = data.zone("cistern").expect("cistern");
        let mut rows = zone.rows.clone();
        rows[7] = "X".repeat(zone.size as usize);
        let out = rewrite(&text, "cistern", &rows).expect("rewrites");
        let changed: Vec<(usize, &str, &str)> = text
            .lines()
            .zip(out.lines())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| (i + 1, a, b))
            .collect();
        assert_eq!(changed.len(), 1, "{changed:?}");
        let (line, before, after) = changed[0];
        assert_eq!(after, format!("  7 {}", rows[7]));
        assert_eq!(before, format!("  7 {}", zone.rows[7]));
        let heading = text
            .lines()
            .position(|l| l.starts_with("#### ") && l.contains("(`maps/cistern.txt`)"))
            .expect("heading")
            + 1;
        assert_eq!(
            line,
            heading + 4 + 7,
            "the row line sits 4 lines under the heading + z"
        );
        assert_eq!(text.lines().count(), out.lines().count());
        assert_eq!(text.ends_with('\n'), out.ends_with('\n'));
    }

    #[test]
    fn a_block_that_does_not_match_is_refused() {
        let text = workspace_design_md();
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("loads");
        let u = data.zone("undercroft").expect("undercroft");
        let err = rewrite(&text, "nowhere", &u.rows).unwrap_err();
        assert!(
            err.contains("no `#### …") && err.contains("nowhere"),
            "{err}"
        );
        let short = &u.rows[..u.rows.len() - 1];
        let err = rewrite(&text, "undercroft", short).unwrap_err();
        assert!(err.contains("more rows than the map"), "{err}");
        let mut long = u.rows.clone();
        long.push(u.rows[0].clone());
        let err = rewrite(&text, "undercroft", &long).unwrap_err();
        assert!(err.contains("fewer rows than the map"), "{err}");
        let broken = text.replacen("```\n    0         1", "~~~\n    0         1", 1);
        assert_ne!(broken, text);
        let err = rewrite(&broken, "undercroft", &u.rows).unwrap_err();
        assert!(err.contains("fence"), "{err}");
        // the hub block is not a zone block: no `maps/hub.txt` heading is addressable through a zone id
        assert!(rewrite(&text, "hub", &u.rows).is_err());
    }

    use crate::doc;
    use undercroft_data::CellKind;

    /// Facts for a zone from the workspace data (stats from the validator, entry / water from the parser).
    fn facts_for<'a>(data: &'a GameData, id: &str, stats: &'a Stats) -> ZoneFacts<'a> {
        let zone = data.zone(id).expect("zone");
        let parsed = undercroft_data::parse_zone(zone).expect("parses");
        ZoneFacts {
            zone,
            entry: parsed.stairs.map(|s| s.marker.cell()),
            water: parsed
                .cells
                .iter()
                .filter(|&&k| k == CellKind::Water)
                .count() as i64,
            npcs: &data.npcs,
            stats,
        }
    }

    fn stats_for(data: &GameData, id: &str) -> Stats {
        doc::preview_applied(data, id)
            .validation
            .and_then(|v| v.stats)
            .expect("stats")
    }

    #[test]
    fn every_zone_table_round_trips_from_the_data() {
        let text = workspace_design_md();
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("loads");
        for zone in &data.zones {
            let stats = stats_for(&data, &zone.id);
            let mut notes = Vec::new();
            let same = rewrite_tables(&text, &facts_for(&data, &zone.id, &stats), &mut notes)
                .expect(&zone.id);
            assert_eq!(
                same, text,
                "{}: DESIGN.md already matches the data",
                zone.id
            );
            assert!(notes.is_empty(), "{}: {notes:?}", zone.id);
        }
    }

    #[test]
    fn changed_facts_change_exactly_the_zone_s_table_cells() {
        let text = workspace_design_md();
        let mut data = GameData::from_dir(&GameData::workspace_data_dir()).expect("loads");
        let mut stats = stats_for(&data, "undercroft");
        // the data behind every kind of token: an NPC, a spot, the gate, a door, the entry, and the numbers
        let zone = data
            .zones
            .iter_mut()
            .find(|z| z.id == "undercroft")
            .expect("undercroft");
        *zone.npcs.get_mut("lamplighter").expect("wick") = [5, 47];
        zone.spots[1].cell = [26, 44];
        zone.gate.as_mut().expect("gate").cells[0] = [15, 7];
        zone.shortcuts[0].cells = vec![[37, 54], [37, 55]];
        zone.shortcuts[2].open_from = undercroft_data::Facing::S;
        stats.walkable += 1;
        stats.counts.p += 1;
        stats.shortcuts[0].detour = 66;
        stats.route = Some(undercroft_sim::validate::RouteStat {
            closed: 948,
            open: 656,
        });
        let mut facts = facts_for(&data, "undercroft", &stats);
        facts.entry = Some([31, 57]);
        facts.water = 3;
        let mut notes = Vec::new();
        let out = rewrite_tables(&text, &facts, &mut notes).expect("rewrites");
        assert!(notes.is_empty(), "{notes:?}");
        let changed: Vec<(usize, &str)> = text
            .lines()
            .zip(out.lines())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (_, b))| (i + 1, b))
            .collect();
        let after: Vec<&str> = changed.iter().map(|c| c.1).collect();
        assert_eq!(changed.len(), 5, "{changed:#?}");
        // §3.2 row: entry, walkable, Wick, gate; the prose around them intact
        assert!(
            after[0].starts_with("| `undercroft` | The Undercroft | 62×62 | S (31,57) | "),
            "{}",
            after[0]
        );
        assert!(
            after[0].contains("Wick (5,47), Deacon (4,5)")
                && after[0].contains("Pry Bar → X (15,7), NW crypt"),
            "{}",
            after[0]
        );
        assert!(
            after[0].contains(&format!("| {} | ×1 |", stats.walkable)),
            "{}",
            after[0]
        );
        // the contract-spot line
        assert!(
            after[1].contains("Undercroft (48,10) NE crypt, (26,44) great hall · Cistern (33,45)"),
            "{}",
            after[1]
        );
        // the shortcut rows: cells + detour + route on the first, `opens from` on the third
        assert!(
            after[2].contains("| (37,54) (37,55) | E |")
                && after[2].contains("| 66 | 948 → 656 cells |"),
            "{}",
            after[2]
        );
        assert!(
            after[3].contains("| `u_rood` The Rood Door | (53,36) | S |"),
            "{}",
            after[3]
        );
        // §3.3 row: walkable, pillars, water, route with the recomputed percentage; the target numbers intact
        assert!(
            after[4].contains(&format!(
                "| {} (2380) | 35.4 % (35.7) | {} | 3 | **948 → 656** (69 %) |",
                stats.walkable, stats.counts.p
            )),
            "{}",
            after[4]
        );
        assert_eq!(text.lines().count(), out.lines().count());
        assert_eq!(text.ends_with('\n'), out.ends_with('\n'));
        // the second pass is a fixed point
        let again = rewrite_tables(&out, &facts, &mut notes).expect("rewrites");
        assert_eq!(again, out);
    }

    #[test]
    fn words_the_data_cannot_place_are_noted_not_rewritten() {
        let text = workspace_design_md();
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("loads");
        let stats = stats_for(&data, "undercroft");
        let facts = facts_for(&data, "undercroft", &stats);
        // a captive renamed in the doc, and a door the doc names that zones.ron does not have
        let wick = fmt_cell(data.zone("undercroft").expect("undercroft").npcs["lamplighter"]);
        let renamed = text
            .replacen(&format!("Wick {wick}"), &format!("Tallow {wick}"), 1)
            .replacen("| undercroft | `u_rood` ", "| undercroft | `u_rude` ", 1);
        assert_ne!(renamed, text);
        let mut notes = Vec::new();
        let out = rewrite_tables(&renamed, &facts, &mut notes).expect("rewrites");
        assert_eq!(out, renamed, "nothing to change, nothing changed");
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(
            notes[0].contains("captive \"Tallow\"") && notes[0].contains(&wick),
            "{}",
            notes[0]
        );
        assert!(
            notes[1].contains("`u_rude` is not in zones.ron"),
            "{}",
            notes[1]
        );
        assert!(notes[2].contains("shortcut table lists"), "{}", notes[2]);
        // a table without the zone's row is refused
        let gone = text.replacen(
            "| `undercroft` | The Undercroft |",
            "| `nowhere` | The Undercroft |",
            1,
        );
        let err = rewrite_tables(&gone, &facts, &mut Vec::new()).unwrap_err();
        assert!(
            err.contains("§3.2") && err.contains("no row for undercroft"),
            "{err}"
        );
    }
}
