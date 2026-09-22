//! `ZoneDoc`: the editable state of one zone, exactly the JSON the page holds and sends back
//! (`id`, `rows`, flattened `anchors`, `npcs`, `gate`, `spots`, `shortcuts`, `regions`). Converts
//! `undercroft_data::ZoneDef` -> `ZoneDoc`, applies a doc back onto a `ZoneDef` (rows, npcs, gate, spots,
//! shortcuts, regions replaced wholesale; anchors rebuilt from the flattened paths) and onto the NPC table
//! (`npcs.npcs[id].cell`), and runs the parser + `undercroft_sim::validate::validate_zone` for previews.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use undercroft_data::zone::{CellXY, GateDef, SpotDef};
use undercroft_data::{Anchor, GameData, NpcTable, Region, ShortcutDef, ZoneDef};
use undercroft_sim::validate::{validate_zone, Validation};

/// The editable state of one zone (the API contract's `ZoneDoc`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneDoc {
    pub id: String,
    /// `size` strings of `size` chars.
    pub rows: Vec<String>,
    /// Flattened anchor paths (`entry`, `hunters[1]`, `shortcuts.u_rood`) -> `[x, z]`.
    pub anchors: BTreeMap<String, CellXY>,
    pub npcs: BTreeMap<String, CellXY>,
    pub gate: Option<GateDef>,
    pub spots: Vec<SpotDef>,
    pub shortcuts: Vec<ShortcutDef>,
    pub regions: Vec<Region>,
}

/// What `/api/preview` returns: either a parse error or the parsed kinds plus the validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewResult {
    pub parse_error: Option<String>,
    /// `CellKind as u8`, row-major.
    pub kinds: Option<Vec<u8>>,
    pub validation: Option<Validation>,
}

impl PreviewResult {
    fn error(msg: String) -> PreviewResult {
        PreviewResult {
            parse_error: Some(msg),
            kinds: None,
            validation: None,
        }
    }
}

impl ZoneDoc {
    /// The doc for a loaded zone.
    pub fn from_zone(zone: &ZoneDef) -> ZoneDoc {
        ZoneDoc {
            id: zone.id.clone(),
            rows: zone.rows.clone(),
            anchors: zone.anchor_list().into_iter().collect(),
            npcs: zone.npcs.clone(),
            gate: zone.gate.clone(),
            spots: zone.spots.clone(),
            shortcuts: zone.shortcuts.clone(),
            regions: zone.regions.clone(),
        }
    }

    /// Apply the doc onto `zone`: rows, npcs, gate, spots, shortcuts and regions replaced wholesale, anchors
    /// rebuilt from the flattened paths. Fails (leaving `zone` untouched) on a wrong row count, a ragged row or
    /// an anchor path that cannot be rebuilt.
    pub fn apply(&self, zone: &mut ZoneDef) -> Result<(), String> {
        if self.id != zone.id {
            return Err(format!("doc is for zone {:?}, not {:?}", self.id, zone.id));
        }
        let size = zone.size as usize;
        if self.rows.len() != size {
            return Err(format!(
                "map {}: {} rows but the zone size is {}",
                zone.id,
                self.rows.len(),
                size
            ));
        }
        for (z, row) in self.rows.iter().enumerate() {
            if row.chars().count() != size {
                return Err(format!(
                    "map {}: row {} has length {}, expected {}",
                    zone.id,
                    z,
                    row.chars().count(),
                    size
                ));
            }
        }
        let anchors = rebuild_anchors(&self.anchors, &zone.anchors)?;
        zone.rows = self.rows.clone();
        zone.npcs = self.npcs.clone();
        zone.gate = self.gate.clone();
        zone.spots = self.spots.clone();
        zone.shortcuts = self.shortcuts.clone();
        zone.regions = self.regions.clone();
        zone.anchors = anchors;
        Ok(())
    }

    /// Sync the NPC table: `npcs.npcs[id].cell = cell` for every doc NPC that exists in the table.
    /// Returns whether any cell changed.
    pub fn apply_npcs(&self, npcs: &mut NpcTable) -> bool {
        let mut changed = false;
        for (id, cell) in &self.npcs {
            if let Some(def) = npcs.npcs.get_mut(id) {
                if def.cell != *cell {
                    def.cell = *cell;
                    changed = true;
                }
            }
        }
        changed
    }
}

/// The top-level anchor key of a flattened path (`hunters[1]` -> `hunters`, `shortcuts.u_rood` -> `shortcuts`).
fn top_key(path: &str) -> &str {
    let end = path.find(['.', '[']).unwrap_or(path.len());
    &path[..end]
}

/// `name` -> `Cell`, `name[i]` -> `Cells` in index order, `group.name` (optionally `group.name[i]`) -> `Group`.
///
/// A top-level entry of `existing` whose flattened form equals the doc's entries under that key is kept
/// verbatim: that carries an empty `Cells([])` / `Group({})` (which flattens to nothing) and a deeper group
/// (which the flat form cannot express) through an unrelated edit instead of dropping or refusing it.
fn rebuild_anchors(
    flat: &BTreeMap<String, CellXY>,
    existing: &BTreeMap<String, Anchor>,
) -> Result<BTreeMap<String, Anchor>, String> {
    let mut kept: BTreeMap<String, Anchor> = BTreeMap::new();
    for (key, anchor) in existing {
        let mut was = Vec::new();
        anchor.flatten(key, &mut was);
        was.sort();
        let now: Vec<(String, CellXY)> = flat
            .iter()
            .filter(|(p, _)| top_key(p) == key)
            .map(|(p, c)| (p.clone(), *c))
            .collect();
        if was == now {
            kept.insert(key.clone(), anchor.clone());
        }
    }
    // group name -> its own flat map; "" is the top level
    let mut groups: BTreeMap<String, BTreeMap<String, CellXY>> = BTreeMap::new();
    for (path, cell) in flat {
        if kept.contains_key(top_key(path)) {
            continue;
        }
        if path.is_empty() {
            return Err("anchor path is empty".to_string());
        }
        let (group, rest) = match path.split_once('.') {
            Some((g, r)) => (g, r),
            None => ("", path.as_str()),
        };
        if rest.contains('.') {
            return Err(format!(
                "anchor path {path:?}: only one level of grouping is supported"
            ));
        }
        if rest.is_empty() || (path.contains('.') && group.is_empty()) {
            return Err(format!("anchor path {path:?}: empty name"));
        }
        groups
            .entry(group.to_string())
            .or_default()
            .insert(rest.to_string(), *cell);
    }
    let mut out = kept;
    for (group, members) in groups {
        let built = rebuild_level(&members)?;
        let entries: Vec<(String, Anchor)> = if group.is_empty() {
            built.into_iter().collect()
        } else {
            vec![(group, Anchor::Group(built))]
        };
        for (k, v) in entries {
            if out.insert(k.clone(), v).is_some() {
                return Err(format!("anchor {k:?} is both a cell and a group"));
            }
        }
    }
    Ok(out)
}

/// One level: plain names become `Cell`, `name[i]` entries become one `Cells` per name (indices must be 0..n).
fn rebuild_level(flat: &BTreeMap<String, CellXY>) -> Result<BTreeMap<String, Anchor>, String> {
    let mut lists: BTreeMap<String, Vec<(usize, CellXY)>> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (name, cell) in flat {
        if let Some(open) = name.find('[') {
            let base = &name[..open];
            let idx = name[open + 1..]
                .strip_suffix(']')
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or_else(|| format!("anchor path {name:?}: bad list index"))?;
            if base.is_empty() {
                return Err(format!("anchor path {name:?}: empty name"));
            }
            lists
                .entry(base.to_string())
                .or_default()
                .push((idx, *cell));
        } else {
            out.insert(name.clone(), Anchor::Cell(*cell));
        }
    }
    for (base, mut items) in lists {
        if out.contains_key(&base) {
            return Err(format!("anchor {base:?} is both a cell and a list"));
        }
        items.sort_by_key(|(i, _)| *i);
        for (want, (got, _)) in items.iter().enumerate() {
            if *got != want {
                return Err(format!(
                    "anchor list {base:?}: indices are not contiguous from 0 (missing [{want}])"
                ));
            }
        }
        out.insert(
            base,
            Anchor::Cells(items.into_iter().map(|(_, c)| c).collect()),
        );
    }
    Ok(out)
}

/// A fingerprint of a zone's editable state as it is on disk: the doc JSON plus the `npcs.ron` cells of its
/// NPCs, FNV-1a hashed. `/api/zones` hands it to the page and `/api/save` compares it against a fresh load,
/// so a save cannot silently overwrite a hand edit (or another tab's save) made since the page loaded.
pub fn fingerprint(data: &GameData, zone: &ZoneDef) -> String {
    let doc = ZoneDoc::from_zone(zone);
    let mut text = serde_json::to_string(&doc).unwrap_or_default();
    for id in doc.npcs.keys() {
        if let Some(def) = data.npcs.npcs.get(id) {
            text.push_str(&format!("|{id}:{},{}", def.cell[0], def.cell[1]));
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The zone `doc` targets, cloned from `data`.
pub fn zone_of<'a>(data: &'a GameData, doc: &ZoneDoc) -> Result<&'a ZoneDef, String> {
    data.zone(&doc.id)
        .ok_or_else(|| format!("unknown zone {:?}", doc.id))
}

/// Apply the doc to a copy of `data` (the matching zone and the NPC table). Errors are parse-level
/// (row shape, anchors) or an unknown zone.
pub fn applied(data: &GameData, doc: &ZoneDoc) -> Result<GameData, String> {
    zone_of(data, doc)?;
    let mut copy = data.clone();
    let zone = copy
        .zones
        .iter_mut()
        .find(|z| z.id == doc.id)
        .expect("zone exists (checked above)");
    doc.apply(zone)?;
    doc.apply_npcs(&mut copy.npcs);
    Ok(copy)
}

/// The parser + validator result for `doc` against a fresh `data` (`data` itself is not modified).
pub fn preview(data: &GameData, doc: &ZoneDoc) -> PreviewResult {
    let copy = match applied(data, doc) {
        Ok(c) => c,
        Err(e) => return PreviewResult::error(e),
    };
    preview_applied(&copy, &doc.id)
}

/// The preview for a zone of an already-applied (or freshly loaded) `data`.
pub fn preview_applied(data: &GameData, id: &str) -> PreviewResult {
    let zone = match data.zone(id) {
        Some(z) => z,
        None => return PreviewResult::error(format!("unknown zone {id:?}")),
    };
    let parsed = match undercroft_data::parse_zone(zone) {
        Ok(m) => m,
        Err(e) => return PreviewResult::error(e.to_string()),
    };
    let kinds = parsed.cells.iter().map(|k| *k as u8).collect();
    PreviewResult {
        parse_error: None,
        kinds: Some(kinds),
        validation: Some(validate_zone(data, zone, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    #[test]
    fn from_zone_then_apply_round_trips_every_zone() {
        let d = data();
        for zone in &d.zones {
            let doc = ZoneDoc::from_zone(zone);
            assert_eq!(doc.rows.len(), zone.size as usize);
            let mut blank = zone.clone();
            blank.rows.clear();
            blank.anchors.clear();
            blank.npcs.clear();
            blank.gate = None;
            blank.spots.clear();
            blank.shortcuts.clear();
            blank.regions.clear();
            doc.apply(&mut blank).expect("apply");
            assert_eq!(
                &blank, zone,
                "{}: apply(from_zone) is the identity",
                zone.id
            );
            // and the JSON shape survives a serde round trip
            let json = serde_json::to_string(&doc).expect("json");
            let back: ZoneDoc = serde_json::from_str(&json).expect("parse");
            assert_eq!(back, doc);
        }
    }

    #[test]
    fn round_trip_preview_is_clean() {
        let d = data();
        for zone in &d.zones {
            let p = preview(&d, &ZoneDoc::from_zone(zone));
            assert_eq!(p.parse_error, None, "{}", zone.id);
            let v = p.validation.expect("validation");
            assert!(
                v.ok && v.errors.is_empty() && v.warnings.is_empty(),
                "{}",
                zone.id
            );
            assert_eq!(
                p.kinds.expect("kinds").len(),
                (zone.size * zone.size) as usize
            );
        }
    }

    #[test]
    fn moved_anchor_and_npc_show_up_after_apply() {
        let d = data();
        let u = d.zone("undercroft").expect("undercroft");
        let mut doc = ZoneDoc::from_zone(u);
        doc.anchors.insert("shortcuts.u_rood".into(), [1, 2]);
        doc.anchors.insert("entry".into(), [3, 4]);
        doc.npcs.insert("deacon".into(), [5, 6]);
        let mut zone = u.clone();
        doc.apply(&mut zone).expect("apply");
        assert_eq!(zone.anchor("shortcuts.u_rood"), Some([1, 2]));
        assert_eq!(zone.anchor("entry"), Some([3, 4]));
        assert_eq!(zone.npcs["deacon"], [5, 6]);
        let mut npcs = d.npcs.clone();
        assert!(doc.apply_npcs(&mut npcs));
        assert_eq!(npcs.npcs["deacon"].cell, [5, 6]);
        assert!(!doc.apply_npcs(&mut npcs), "second apply changes nothing");

        // lists rebuild in index order, whatever the map order of the paths
        let c = d.zone("cistern").expect("cistern");
        let mut doc = ZoneDoc::from_zone(c);
        doc.anchors.insert("hunters[1]".into(), [9, 9]);
        let mut zone = c.clone();
        doc.apply(&mut zone).expect("apply");
        assert_eq!(
            zone.anchors["hunters"],
            Anchor::Cells(vec![[20, 17], [9, 9]])
        );
        doc.anchors.remove("hunters[0]");
        assert!(doc.apply(&mut zone).unwrap_err().contains("contiguous"));
        doc.anchors.insert("a.b.c".into(), [0, 0]);
        assert!(doc.apply(&mut zone).is_err());
    }

    #[test]
    fn unchanged_anchor_subtrees_survive_apply() {
        // an empty list / group flattens to nothing and a two-level group cannot be expressed in the flat
        // form; both are kept verbatim as long as the doc does not touch them
        let d = data();
        let u = d.zone("undercroft").expect("undercroft");
        let mut zone = u.clone();
        zone.anchors
            .insert("zz_empty".into(), Anchor::Cells(Vec::new()));
        zone.anchors
            .insert("zz_group".into(), Anchor::Group(BTreeMap::new()));
        let deep = Anchor::Group(BTreeMap::from([(
            "a".to_string(),
            Anchor::Group(BTreeMap::from([("b".to_string(), Anchor::Cell([1, 1]))])),
        )]));
        zone.anchors.insert("zz_deep".into(), deep.clone());
        let mut doc = ZoneDoc::from_zone(&zone);
        assert_eq!(doc.anchors.get("zz_deep.a.b"), Some(&[1, 1]));
        assert!(!doc.anchors.keys().any(|k| k.starts_with("zz_empty")));

        // an unrelated edit keeps all three
        doc.anchors.insert("entry".into(), [3, 4]);
        let mut applied = zone.clone();
        doc.apply(&mut applied).expect("apply");
        assert_eq!(applied.anchor("entry"), Some([3, 4]));
        assert_eq!(applied.anchors["zz_empty"], Anchor::Cells(Vec::new()));
        assert_eq!(applied.anchors["zz_group"], Anchor::Group(BTreeMap::new()));
        assert_eq!(applied.anchors["zz_deep"], deep);
        let mut with = d.clone();
        *with
            .zones
            .iter_mut()
            .find(|z| z.id == "undercroft")
            .expect("undercroft") = zone.clone();
        assert_eq!(preview(&with, &doc).parse_error, None, "no edit needed");

        // moving the deep cell is the one thing the flat form cannot express
        doc.anchors.insert("zz_deep.a.b".into(), [2, 2]);
        let err = doc.apply(&mut applied).unwrap_err();
        assert!(err.contains("one level"), "{err}");
    }

    #[test]
    fn ragged_row_gives_parse_error() {
        let d = data();
        let u = d.zone("undercroft").expect("undercroft");
        let mut doc = ZoneDoc::from_zone(u);
        doc.rows[5].pop();
        let p = preview(&d, &doc);
        let err = p.parse_error.expect("parse_error");
        assert!(err.contains("row 5"), "{err}");
        assert!(p.kinds.is_none() && p.validation.is_none());
        let mut zone = u.clone();
        assert!(doc.apply(&mut zone).is_err());
        assert_eq!(&zone, u, "a failed apply leaves the zone untouched");

        let mut doc = ZoneDoc::from_zone(u);
        doc.rows.pop();
        assert!(preview(&d, &doc).parse_error.is_some());

        let mut doc = ZoneDoc::from_zone(u);
        doc.rows[1].replace_range(1..2, "?");
        let err = preview(&d, &doc).parse_error.expect("unknown char");
        assert!(err.contains("unknown char"), "{err}");
    }
}
