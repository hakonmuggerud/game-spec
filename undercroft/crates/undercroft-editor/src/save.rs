//! Saving a `ZoneDoc`: writes `maps/<id>.txt` (rows joined by `\n`, trailing newline), `zones.ron` through
//! `ron_io`, `npcs.ron` only when an NPC cell changed, the fixtures, then the zone's ASCII map block in
//! `DESIGN.md` (`design_md`); returns the list of paths written (relative) so the page can show them. Refuses a
//! doc whose rows do not parse, and (when the page sends the `base` fingerprint it loaded with) a doc whose
//! zone changed on disk since.
//!
//! Before anything is rendered every `ShortcutDef.saves` of the zone is set to the detour the validator
//! measured for that door (`stats.shortcuts[].detour`), so `zones.ron`, the fixture's `shortcuts[].saves` and
//! the stats agree without a hand fix; a door the validator could not measure keeps its declared value.
//!
//! The save is all-or-nothing: every file is rendered in memory first (so a fixture that cannot be derived —
//! say a map with no spawn — changes nothing), then each one is written through a temp file + rename, and a
//! write that fails rolls the files already replaced back to their previous bytes.

use crate::doc::{self, PreviewResult, ZoneDoc};
use crate::{design_md, fixtures, ron_io};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use undercroft_data::{GameData, NpcTable, ZoneDef};

/// What `/api/save` returns: the fresh preview plus what happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveResult {
    #[serde(flatten)]
    pub preview: PreviewResult,
    pub saved: bool,
    /// Paths written, relative to the parent of the data / fixtures directories
    /// (`data/maps/undercroft.txt`, `data/zones.ron`, `fixtures/undercroft.json`, ...).
    pub written: Vec<String>,
    /// Why nothing (or not everything) was saved.
    pub reason: Option<String>,
    /// The zone changed on disk since the page loaded it (`base` did not match); nothing was written.
    pub stale: bool,
    /// The zone's on-disk fingerprint after the call (`doc::fingerprint`), for the page's next save.
    pub base: Option<String>,
    /// Shortcut `saves` values the save corrected to the validator's detour (the page applies them to its doc).
    #[serde(default)]
    pub synced: Vec<SavesSync>,
    /// What else happened, for the status line: `u_rood saves 152 -> 154`, a `DESIGN.md` that was not there.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// One `ShortcutDef.saves` correction: `from` (declared) -> `to` (measured).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavesSync {
    pub id: String,
    pub from: i32,
    pub to: i32,
}

/// The doc with every shortcut's `saves` set to the detour `preview` measured for it. A door without a
/// measurement (no stats, an id the validator did not report, or a detour of −1 = the flanks are not
/// connected with the door shut) keeps its declared value and gets a note.
fn sync_saves(doc: &ZoneDoc, preview: &PreviewResult) -> (ZoneDoc, Vec<SavesSync>, Vec<String>) {
    let mut doc = doc.clone();
    let mut synced = Vec::new();
    let mut notes = Vec::new();
    let stats = preview.validation.as_ref().and_then(|v| v.stats.as_ref());
    for sc in &mut doc.shortcuts {
        let measured = stats
            .and_then(|st| st.shortcuts.iter().find(|s| s.id == sc.id))
            .map(|s| s.detour);
        match measured {
            Some(detour) if detour >= 0 => {
                if detour != sc.saves {
                    notes.push(format!("{} saves {} -> {}", sc.id, sc.saves, detour));
                    synced.push(SavesSync {
                        id: sc.id.clone(),
                        from: sc.saves,
                        to: detour,
                    });
                    sc.saves = detour;
                }
            }
            Some(_) => notes.push(format!(
                "{} saves {} kept: the validator found no detour around the door (flanks not connected)",
                sc.id, sc.saves
            )),
            None => notes.push(format!(
                "{} saves {} kept: the validator did not measure the door",
                sc.id, sc.saves
            )),
        }
    }
    (doc, synced, notes)
}

/// `<dir name>/<rel>` for the `written` list.
fn label(dir: &Path, rel: &str) -> String {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string());
    format!("{name}/{rel}")
}

/// Write the doc to the data directory, regenerate the fixtures and rewrite the zone's map block in
/// `design_md` (skipped with a note when that file does not exist). `base` is the fingerprint the page
/// loaded the zone with (`None` skips the check).
pub fn save(
    doc: &ZoneDoc,
    base: Option<&str>,
    data_dir: &Path,
    fixtures_dir: &Path,
    design_md: &Path,
) -> SaveResult {
    let refuse = |preview: PreviewResult, reason: String| SaveResult {
        preview,
        saved: false,
        written: Vec::new(),
        reason: Some(reason),
        stale: false,
        base: None,
        synced: Vec::new(),
        notes: Vec::new(),
    };
    let data = match GameData::from_dir(data_dir) {
        Ok(d) => d,
        Err(e) => {
            return refuse(
                PreviewResult {
                    parse_error: Some(e.to_string()),
                    kinds: None,
                    validation: None,
                },
                format!("the data directory does not load: {e}"),
            )
        }
    };
    let on_disk = data.zone(&doc.id).map(|z| doc::fingerprint(&data, z));
    let preview = doc::preview(&data, doc);
    if let Some(err) = &preview.parse_error {
        return refuse(
            preview.clone(),
            format!("not saved: the map does not parse ({err})"),
        );
    }
    if let (Some(base), Some(now)) = (base, &on_disk) {
        if base != now {
            return SaveResult {
                preview,
                saved: false,
                written: Vec::new(),
                reason: Some(format!(
                    "not saved: {} changed on disk since this page loaded it (a hand edit or another \
                     editor tab); reload the page to pick it up, or save again to overwrite it",
                    doc.id
                )),
                stale: true,
                base: on_disk,
                synced: Vec::new(),
                notes: Vec::new(),
            };
        }
    }
    let (doc, synced, mut notes) = sync_saves(doc, &preview);
    let doc = &doc;
    let files = match plan(doc, &data, data_dir, fixtures_dir, design_md, &mut notes) {
        Ok(f) => f,
        Err(e) => return refuse(preview, format!("save failed, nothing changed: {e}")),
    };
    let written = match commit(&files) {
        Ok(()) => files.iter().map(|f| f.label.clone()).collect(),
        Err(e) => return refuse(preview, e),
    };
    // reload so the response reflects what is on disk
    match GameData::from_dir(data_dir) {
        Ok(fresh) => SaveResult {
            preview: doc::preview_applied(&fresh, &doc.id),
            saved: true,
            written,
            reason: None,
            stale: false,
            base: fresh.zone(&doc.id).map(|z| doc::fingerprint(&fresh, z)),
            synced,
            notes,
        },
        Err(e) => SaveResult {
            preview,
            saved: false,
            written,
            reason: Some(format!(
                "saved, but the data directory no longer loads: {e}"
            )),
            stale: false,
            base: None,
            synced,
            notes,
        },
    }
}

/// One file the save will replace.
struct Planned {
    path: PathBuf,
    label: String,
    text: String,
}

/// Render every file the save writes, in write order, without touching the disk. A `design_md` that does not
/// exist is skipped with a note (temp-dir copies); one that exists must hold the zone's block.
fn plan(
    doc: &ZoneDoc,
    data: &GameData,
    data_dir: &Path,
    fixtures_dir: &Path,
    design_md: &Path,
    notes: &mut Vec<String>,
) -> Result<Vec<Planned>, String> {
    let mut files = Vec::new();

    // maps/<id>.txt
    let rel = format!("maps/{}.txt", doc.id);
    let mut text = doc.rows.join("\n");
    text.push('\n');
    files.push(Planned {
        path: data_dir.join(&rel),
        label: label(data_dir, &rel),
        text,
    });

    // zones.ron: the whole table, with the doc applied to its entry
    let path = data_dir.join("zones.ron");
    let (header, mut zones): (String, Vec<ZoneDef>) = ron_io::read(&path)?;
    let entry = zones
        .iter_mut()
        .find(|z| z.id == doc.id)
        .ok_or_else(|| format!("zones.ron has no zone {:?}", doc.id))?;
    doc.apply(entry)?;
    let text = ron_io::render(&header, &zones).map_err(|e| format!("{}: {e}", path.display()))?;
    files.push(Planned {
        path,
        label: label(data_dir, "zones.ron"),
        text,
    });

    // npcs.ron only when a cell moved
    let path = data_dir.join("npcs.ron");
    let (header, mut npcs): (String, NpcTable) = ron_io::read(&path)?;
    if doc.apply_npcs(&mut npcs) {
        let text =
            ron_io::render(&header, &npcs).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(Planned {
            path,
            label: label(data_dir, "npcs.ron"),
            text,
        });
    }

    // fixtures from the applied data (the in-memory copy equals what is about to be written)
    let applied = doc::applied(data, doc)?;
    for (path, text) in fixtures::render_all(&applied, fixtures_dir)? {
        let rel = path
            .strip_prefix(fixtures_dir)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.display().to_string());
        files.push(Planned {
            label: label(fixtures_dir, &rel),
            path,
            text,
        });
    }

    // DESIGN.md: the zone's ASCII map block, then its numbers and cells in the §3.2 / §3.3 tables
    let name = design_md
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| design_md.display().to_string());
    if design_md.is_file() {
        let text = std::fs::read_to_string(design_md)
            .map_err(|e| format!("{}: {e}", design_md.display()))?;
        let text = design_md::rewrite(&text, &doc.id, &doc.rows)?;
        let zone = applied
            .zone(&doc.id)
            .ok_or_else(|| format!("the applied data has no zone {:?}", doc.id))?;
        let parsed = undercroft_data::parse_zone(zone).map_err(|e| e.to_string())?;
        let stats = doc::preview_applied(&applied, &doc.id)
            .validation
            .and_then(|v| v.stats)
            .ok_or_else(|| format!("{}: the validator produced no stats for the tables", doc.id))?;
        let facts = design_md::ZoneFacts {
            zone,
            entry: parsed.stairs.map(|s| s.marker.cell()),
            water: parsed
                .cells
                .iter()
                .filter(|&&k| k == undercroft_data::CellKind::Water)
                .count() as i64,
            npcs: &applied.npcs,
            stats: &stats,
        };
        let text = design_md::rewrite_tables(&text, &facts, notes)?;
        files.push(Planned {
            path: design_md.to_path_buf(),
            label: name,
            text,
        });
    } else {
        notes.push(format!(
            "{name} not found at {}: the map block was not rewritten",
            design_md.display()
        ));
    }
    Ok(files)
}

/// Replace `path` with `text` through a temp file in the same directory + rename, so a reader never sees a
/// truncated file and an interrupted write leaves the old one in place.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("{}: no parent directory", path.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let done = std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| format!("{}: {e}", path.display()));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// Write every planned file; on a failure, put back the previous bytes of the files already replaced.
fn commit(files: &[Planned]) -> Result<(), String> {
    let mut replaced: Vec<(&Path, Option<Vec<u8>>)> = Vec::new();
    for f in files {
        let before = match std::fs::read(&f.path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return rollback(&replaced, format!("{}: {e}", f.path.display())),
        };
        if let Err(e) = write_atomic(&f.path, f.text.as_bytes()) {
            return rollback(&replaced, e);
        }
        replaced.push((&f.path, before));
    }
    Ok(())
}

fn rollback(replaced: &[(&Path, Option<Vec<u8>>)], error: String) -> Result<(), String> {
    let mut stuck = Vec::new();
    for (path, before) in replaced.iter().rev() {
        let restored = match before {
            Some(bytes) => write_atomic(path, bytes),
            None => std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display())),
        };
        if let Err(e) = restored {
            stuck.push(e);
        }
    }
    if stuck.is_empty() {
        Err(format!("save failed, nothing changed: {error}"))
    } else {
        Err(format!(
            "save failed: {error}; and rolling back failed too, check these by hand: {}",
            stuck.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use undercroft_data::zone::CellXY;

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("mkdir");
        for entry in std::fs::read_dir(from).expect("read_dir") {
            let entry = entry.expect("entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("type").is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("copy");
            }
        }
    }

    fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut out = BTreeMap::new();
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).expect("read_dir") {
                let p = entry.expect("entry").path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    let rel = p.strip_prefix(root).expect("rel").to_path_buf();
                    out.insert(rel, std::fs::read(&p).expect("read"));
                }
            }
        }
        walk(dir, dir, &mut out);
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("undercroft-editor-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clean");
        }
        copy_tree(&GameData::workspace_data_dir(), &dir.join("data"));
        copy_tree(&GameData::workspace_fixtures_dir(), &dir.join("fixtures"));
        dir
    }

    fn cell_char(rows: &[String], c: CellXY) -> char {
        rows[c[1] as usize].as_bytes()[c[0] as usize] as char
    }

    fn set_char(rows: &mut [String], c: CellXY, ch: char) {
        let row = &mut rows[c[1] as usize];
        row.replace_range(c[0] as usize..c[0] as usize + 1, &ch.to_string());
    }

    #[test]
    fn noop_save_is_byte_identical_and_a_move_lands_everywhere() {
        let root = scratch("save");
        let data_dir = root.join("data");
        let fixtures_dir = root.join("fixtures");
        let before = snapshot(&root);

        let data = GameData::from_dir(&data_dir).expect("loads");
        let u = data.zone("undercroft").expect("undercroft");
        let doc = ZoneDoc::from_zone(u);
        let r = save(
            &doc,
            None,
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(r.saved, "{:?}", r.reason);
        assert_eq!(r.reason, None);
        assert_eq!(r.preview.parse_error, None);
        assert!(r.preview.validation.as_ref().expect("validation").ok);
        assert_eq!(
            r.written,
            vec![
                "data/maps/undercroft.txt",
                "data/zones.ron",
                "fixtures/undercroft.json",
                "fixtures/cistern.json",
                "fixtures/ossuary.json",
                "fixtures/source.json",
                "fixtures/validate_all.json",
            ]
        );
        assert!(r.synced.is_empty(), "{:?}", r.synced);
        assert_eq!(
            r.notes,
            vec![format!(
                "DESIGN.md not found at {}: the map block was not rewritten",
                root.join("DESIGN.md").display()
            )]
        );
        let after = snapshot(&root);
        for (path, bytes) in &before {
            assert!(
                after.get(path) == Some(bytes),
                "{}: changed by a no-op save",
                path.display()
            );
        }
        assert_eq!(before.len(), after.len(), "no files appeared or vanished");

        // move the deacon (an N that is also a zones.ron npc, an anchor and an npcs.ron cell)
        let from = u.npcs["deacon"];
        assert_eq!(cell_char(&u.rows, from), 'N');
        let to = [[1, 0], [-1, 0], [0, 1], [0, -1]]
            .iter()
            .map(|d| [from[0] + d[0], from[1] + d[1]])
            .find(|c| matches!(cell_char(&u.rows, *c), '.' | 'D'))
            .expect("a floor neighbour");
        let mut moved = doc.clone();
        // swap the pair so the vacated cell keeps the neighbour's floor type
        set_char(&mut moved.rows, from, cell_char(&u.rows, to));
        set_char(&mut moved.rows, to, 'N');
        for v in moved.anchors.values_mut().chain(moved.npcs.values_mut()) {
            if *v == from {
                *v = to;
            }
        }
        let r = save(
            &moved,
            None,
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(r.saved, "{:?}", r.reason);
        assert!(r.written.contains(&"data/npcs.ron".to_string()));
        let after = snapshot(&root);

        let txt = String::from_utf8(after[Path::new("data/maps/undercroft.txt")].clone()).unwrap();
        let old = String::from_utf8(before[Path::new("data/maps/undercroft.txt")].clone()).unwrap();
        let diffs: Vec<(usize, usize)> = old
            .lines()
            .zip(txt.lines())
            .enumerate()
            .flat_map(|(z, (a, b))| {
                a.bytes()
                    .zip(b.bytes())
                    .enumerate()
                    .filter(|(_, (p, q))| p != q)
                    .map(move |(x, _)| (x, z))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut want = vec![
            (from[0] as usize, from[1] as usize),
            (to[0] as usize, to[1] as usize),
        ];
        want.sort();
        let mut got = diffs.clone();
        got.sort();
        assert_eq!(got, want, "the txt differs in exactly the moved pair");
        assert!(txt.ends_with('\n') && old.lines().count() == txt.lines().count());

        let fresh = GameData::from_dir(&data_dir).expect("still loads after the move");
        let fu = fresh.zone("undercroft").expect("undercroft");
        assert_eq!(fu.npcs["deacon"], to);
        assert_eq!(fu.anchor("deacon"), Some(to));
        assert_eq!(fresh.npcs.npcs["deacon"].cell, to);
        assert_ne!(
            after[Path::new("data/zones.ron")],
            before[Path::new("data/zones.ron")]
        );
        assert_ne!(
            after[Path::new("data/npcs.ron")],
            before[Path::new("data/npcs.ron")]
        );
        assert_ne!(
            after[Path::new("fixtures/undercroft.json")],
            before[Path::new("fixtures/undercroft.json")]
        );
        for untouched in [
            "fixtures/hub.json",
            "fixtures/hub_v1.json",
            "data/maps/hub.txt",
        ] {
            assert_eq!(after[Path::new(untouched)], before[Path::new(untouched)]);
        }
        let fixture: serde_json::Value =
            serde_json::from_slice(&after[Path::new("fixtures/undercroft.json")]).unwrap();
        assert_eq!(
            fixture["markers"]["npc_cells"][0]["cx"],
            serde_json::json!(to[0])
        );
        assert_eq!(
            r.preview.validation.expect("validation"),
            undercroft_sim::validate::validate_zone(&fresh, fu, None)
        );

        // a doc that does not parse is refused before anything is written
        let after_move = snapshot(&root);
        let mut bad = moved.clone();
        bad.rows[3].pop();
        let r = save(
            &bad,
            None,
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(!r.saved && r.preview.parse_error.is_some());
        assert!(r.reason.as_deref().unwrap_or("").contains("does not parse"));
        assert!(r.written.is_empty());
        assert_eq!(snapshot(&root), after_move);

        std::fs::remove_dir_all(&root).ok();
    }

    /// The Undercroft doc with its `S` painted over: parses, validates with errors, and the fixtures cannot
    /// be derived from it (no entry field).
    fn doc_without_spawn(data: &GameData) -> ZoneDoc {
        let u = data.zone("undercroft").expect("undercroft");
        let mut doc = ZoneDoc::from_zone(u);
        let s = doc.anchors["entry"];
        assert_eq!(cell_char(&doc.rows, s), 'S');
        set_char(&mut doc.rows, s, '.');
        doc
    }

    #[test]
    fn a_failed_save_leaves_every_file_as_it_was() {
        let root = scratch("atomic");
        let data_dir = root.join("data");
        let fixtures_dir = root.join("fixtures");
        let before = snapshot(&root);
        let data = GameData::from_dir(&data_dir).expect("loads");

        // the fixtures cannot be derived: nothing is written, not even the txt / zones.ron
        let doc = doc_without_spawn(&data);
        let r = save(
            &doc,
            None,
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(!r.saved && !r.stale);
        assert_eq!(r.preview.parse_error, None, "the map itself parses");
        assert!(!r.preview.validation.as_ref().expect("validation").ok);
        let reason = r.reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("no S/V") && reason.contains("nothing changed"),
            "{reason}"
        );
        assert!(r.written.is_empty());
        assert_eq!(
            snapshot(&root),
            before,
            "no file changed and no temp file was left behind"
        );

        // a fixtures directory that does not exist: the data files already replaced are put back
        let good = ZoneDoc::from_zone(data.zone("undercroft").expect("undercroft"));
        let mut moved = good.clone();
        let from = moved.anchors["hunter"];
        assert_eq!(cell_char(&moved.rows, from), 'H');
        let to = [[1, 0], [-1, 0], [0, 1], [0, -1]]
            .iter()
            .map(|d| [from[0] + d[0], from[1] + d[1]])
            .find(|c| cell_char(&moved.rows, *c) == '.')
            .expect("a floor neighbour");
        set_char(&mut moved.rows, from, '.');
        set_char(&mut moved.rows, to, 'H');
        moved.anchors.insert("hunter".into(), to);
        let r = save(
            &moved,
            None,
            &data_dir,
            &root.join("nowhere"),
            &root.join("DESIGN.md"),
        );
        assert!(!r.saved, "{:?}", r.reason);
        let reason = r.reason.as_deref().unwrap_or("");
        assert!(reason.contains("nothing changed"), "{reason}");
        assert!(r.written.is_empty());
        assert_eq!(
            snapshot(&root),
            before,
            "txt and zones.ron were rolled back"
        );

        // a fixture that does not parse refuses the save instead of regenerating from the workspace copy
        std::fs::write(fixtures_dir.join("ossuary.json"), "{not json").unwrap();
        let r = save(
            &moved,
            None,
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(!r.saved);
        let reason = r.reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("ossuary.json") && reason.contains("not a fixture"),
            "{reason}"
        );
        assert_eq!(
            snapshot(&root)[Path::new("data/maps/undercroft.txt")],
            before[Path::new("data/maps/undercroft.txt")]
        );
        std::fs::write(
            fixtures_dir.join("ossuary.json"),
            &before[Path::new("fixtures/ossuary.json")],
        )
        .unwrap();

        // and the same move saves once everything is in place
        let r = save(
            &moved,
            None,
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(r.saved, "{:?}", r.reason);
        let fresh = GameData::from_dir(&data_dir).expect("loads");
        assert_eq!(fresh.zone("undercroft").unwrap().anchor("hunter"), Some(to));
        let after = snapshot(&root);
        assert_eq!(before.len(), after.len(), "no temp files left behind");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_zone_changed_on_disk_is_not_overwritten_blindly() {
        let root = scratch("stale");
        let data_dir = root.join("data");
        let fixtures_dir = root.join("fixtures");
        let data = GameData::from_dir(&data_dir).expect("loads");
        let u = data.zone("undercroft").expect("undercroft");
        let base = doc::fingerprint(&data, u);
        let doc = ZoneDoc::from_zone(u);

        // hand edit on disk: a region name in zones.ron (a doc-owned field the page still holds the old value of)
        let path = data_dir.join("zones.ron");
        let text = std::fs::read_to_string(&path).unwrap();
        let old_name = &doc.regions[0].name;
        let edited = text.replacen(old_name.as_str(), "Renamed By Hand", 1);
        assert_ne!(edited, text);
        std::fs::write(&path, &edited).unwrap();
        let before = snapshot(&root);

        let r = save(
            &doc,
            Some(&base),
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(!r.saved && r.stale, "{:?}", r.reason);
        assert!(r
            .reason
            .as_deref()
            .unwrap_or("")
            .contains("changed on disk"));
        assert!(r.written.is_empty());
        assert_eq!(snapshot(&root), before, "nothing written");
        let now = r.base.expect("the current fingerprint comes back");
        assert_ne!(now, base);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("Renamed By Hand"));

        // with the fresh base the (now stale) doc saves and overwrites the hand edit, explicitly
        let r = save(
            &doc,
            Some(&now),
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(r.saved, "{:?}", r.reason);
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("Renamed By Hand"));
        assert_eq!(
            r.base.as_deref(),
            Some(base.as_str()),
            "back to the original state"
        );

        // the same doc with the same base saves again (a no-op) and without a base the check is skipped
        assert!(
            save(
                &doc,
                Some(&base),
                &data_dir,
                &fixtures_dir,
                &root.join("DESIGN.md")
            )
            .saved
        );
        assert!(
            save(
                &doc,
                None,
                &data_dir,
                &fixtures_dir,
                &root.join("DESIGN.md")
            )
            .saved
        );

        // npcs.ron cells are part of the fingerprint too
        let npath = data_dir.join("npcs.ron");
        let ntext = std::fs::read_to_string(&npath).unwrap();
        let cell = data.npcs.npcs["deacon"].cell;
        let needle = format!("cell: ({}, {})", cell[0], cell[1]);
        assert!(ntext.contains(&needle));
        std::fs::write(&npath, ntext.replacen(&needle, "cell: (9, 9)", 1)).unwrap();
        let r = save(
            &doc,
            Some(&base),
            &data_dir,
            &fixtures_dir,
            &root.join("DESIGN.md"),
        );
        assert!(r.stale && !r.saved);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_wrong_shortcut_saves_is_corrected_to_the_measured_detour() {
        let root = scratch("saves");
        let data_dir = root.join("data");
        let fixtures_dir = root.join("fixtures");
        let design = root.join("DESIGN.md");
        let data = GameData::from_dir(&data_dir).expect("loads");
        let u = data.zone("undercroft").expect("undercroft");
        let measured: BTreeMap<String, i32> =
            undercroft_sim::validate::validate_zone(&data, u, None)
                .stats
                .expect("stats")
                .shortcuts
                .iter()
                .map(|s| (s.id.clone(), s.detour))
                .collect();
        assert!(measured.len() >= 3);
        let mut doc = ZoneDoc::from_zone(u);
        let id = doc.shortcuts[1].id.clone();
        let right = measured[&id];
        assert_eq!(
            doc.shortcuts[1].saves, right,
            "the workspace data is in sync"
        );
        doc.shortcuts[1].saves = right + 7;

        let r = save(&doc, None, &data_dir, &fixtures_dir, &design);
        assert!(r.saved, "{:?}", r.reason);
        assert_eq!(
            r.synced,
            vec![SavesSync {
                id: id.clone(),
                from: right + 7,
                to: right
            }]
        );
        assert!(
            r.notes
                .contains(&format!("{id} saves {} -> {right}", right + 7)),
            "{:?}",
            r.notes
        );
        let fresh = GameData::from_dir(&data_dir).expect("loads");
        let fu = fresh.zone("undercroft").expect("undercroft");
        for sc in &fu.shortcuts {
            assert_eq!(
                sc.saves, measured[&sc.id],
                "{}: zones.ron agrees with the detour",
                sc.id
            );
        }
        let stats = r
            .preview
            .validation
            .expect("validation")
            .stats
            .expect("stats");
        for s in &stats.shortcuts {
            assert_eq!(s.saves, s.detour, "{}: the response stats agree", s.id);
        }
        let all: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixtures_dir.join("validate_all.json")).unwrap())
                .unwrap();
        for s in all["zones"]["undercroft"]["stats"]["shortcuts"]
            .as_array()
            .unwrap()
        {
            assert_eq!(
                s["saves"], s["detour"],
                "{}: validate_all.json agrees",
                s["id"]
            );
        }
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixtures_dir.join("undercroft.json")).unwrap())
                .unwrap();
        let mut seen = 0;
        for s in fixture["markers"]["shortcuts"].as_array().unwrap() {
            if s["id"] == serde_json::Value::String(id.clone()) {
                assert_eq!(
                    s["saves"],
                    serde_json::json!(right),
                    "undercroft.json agrees"
                );
                seen += 1;
            }
        }
        assert!(seen > 0, "the door is in the fixture");
        // the doc as sent is not what is on disk, but the doc with the corrected saves is a no-op now
        let mut corrected = doc.clone();
        corrected.shortcuts[1].saves = right;
        let r = save(&corrected, None, &data_dir, &fixtures_dir, &design);
        assert!(
            r.saved && r.synced.is_empty(),
            "{:?} {:?}",
            r.reason,
            r.synced
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn design_md_map_block_follows_the_save() {
        let root = scratch("design");
        let data_dir = root.join("data");
        let fixtures_dir = root.join("fixtures");
        let design = root.join("DESIGN.md");
        std::fs::copy(
            design_md::default_path(&GameData::workspace_data_dir()),
            &design,
        )
        .expect("copy DESIGN.md");
        let original = std::fs::read(&design).unwrap();
        let data = GameData::from_dir(&data_dir).expect("loads");
        let u = data.zone("undercroft").expect("undercroft");
        let doc = ZoneDoc::from_zone(u);

        // a no-op save lists DESIGN.md and leaves it byte-identical
        let r = save(&doc, None, &data_dir, &fixtures_dir, &design);
        assert!(r.saved, "{:?}", r.reason);
        assert_eq!(r.written.last().map(String::as_str), Some("DESIGN.md"));
        assert!(r.notes.is_empty(), "{:?}", r.notes);
        assert_eq!(std::fs::read(&design).unwrap(), original);
        assert!(!root.join(".DESIGN.md.tmp").exists());

        // one changed row changes exactly that line of the block
        let mut changed = doc.clone();
        let z = changed
            .rows
            .iter()
            .position(|r| r.contains(".."))
            .expect("a row with floor");
        let x = changed.rows[z].find("..").unwrap();
        set_char(&mut changed.rows, [x as i32, z as i32], 'D');
        let r = save(&changed, None, &data_dir, &fixtures_dir, &design);
        assert!(r.saved, "{:?}", r.reason);
        let now = std::fs::read_to_string(&design).unwrap();
        let old = String::from_utf8(original.clone()).unwrap();
        let diffs: Vec<(usize, &str, &str)> = old
            .lines()
            .zip(now.lines())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| (i + 1, a, b))
            .collect();
        assert_eq!(diffs.len(), 1, "{diffs:?}");
        assert_eq!(diffs[0].1, format!("{z:3} {}", doc.rows[z]));
        assert_eq!(diffs[0].2, format!("{z:3} {}", changed.rows[z]));
        let heading = old
            .lines()
            .position(|l| l.starts_with("#### ") && l.contains("(`maps/undercroft.txt`)"))
            .unwrap()
            + 1;
        assert_eq!(diffs[0].0, heading + 4 + z);
        assert_eq!(old.lines().count(), now.lines().count());

        // a DESIGN.md without the zone's block refuses the whole save and nothing changes
        let before = snapshot(&root);
        std::fs::write(&design, "# no maps here\n").unwrap();
        let before_design = snapshot(&root);
        let mut again = changed.clone();
        set_char(&mut again.rows, [x as i32 + 1, z as i32], 'D');
        let r = save(&again, None, &data_dir, &fixtures_dir, &design);
        assert!(!r.saved, "{:?}", r.reason);
        let reason = r.reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("nothing changed") && reason.contains("DESIGN.md"),
            "{reason}"
        );
        assert!(r.written.is_empty());
        assert_eq!(snapshot(&root), before_design);
        assert_eq!(
            before[Path::new("data/maps/undercroft.txt")],
            before_design[Path::new("data/maps/undercroft.txt")]
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
