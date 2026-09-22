//! Saving a `ZoneDoc`: writes `maps/<id>.txt` (rows joined by `\n`, trailing newline), `zones.ron` through
//! `ron_io`, `npcs.ron` only when an NPC cell changed, then the fixtures; returns the list of paths written
//! (relative) so the page can show them. Refuses a doc whose rows do not parse, and (when the page sends the
//! `base` fingerprint it loaded with) a doc whose zone changed on disk since.
//!
//! The save is all-or-nothing: every file is rendered in memory first (so a fixture that cannot be derived —
//! say a map with no spawn — changes nothing), then each one is written through a temp file + rename, and a
//! write that fails rolls the files already replaced back to their previous bytes.

use crate::doc::{self, PreviewResult, ZoneDoc};
use crate::{fixtures, ron_io};
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
}

/// `<dir name>/<rel>` for the `written` list.
fn label(dir: &Path, rel: &str) -> String {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string());
    format!("{name}/{rel}")
}

/// Write the doc to the data directory and regenerate the fixtures. `base` is the fingerprint the page
/// loaded the zone with (`None` skips the check).
pub fn save(doc: &ZoneDoc, base: Option<&str>, data_dir: &Path, fixtures_dir: &Path) -> SaveResult {
    let refuse = |preview: PreviewResult, reason: String| SaveResult {
        preview,
        saved: false,
        written: Vec::new(),
        reason: Some(reason),
        stale: false,
        base: None,
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
            };
        }
    }
    let files = match plan(doc, &data, data_dir, fixtures_dir) {
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
        },
    }
}

/// One file the save will replace.
struct Planned {
    path: PathBuf,
    label: String,
    text: String,
}

/// Render every file the save writes, in write order, without touching the disk.
fn plan(
    doc: &ZoneDoc,
    data: &GameData,
    data_dir: &Path,
    fixtures_dir: &Path,
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
        let r = save(&doc, None, &data_dir, &fixtures_dir);
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
        let r = save(&moved, None, &data_dir, &fixtures_dir);
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
        let r = save(&bad, None, &data_dir, &fixtures_dir);
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
        let r = save(&doc, None, &data_dir, &fixtures_dir);
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
        let r = save(&moved, None, &data_dir, &root.join("nowhere"));
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
        let r = save(&moved, None, &data_dir, &fixtures_dir);
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
        let r = save(&moved, None, &data_dir, &fixtures_dir);
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

        let r = save(&doc, Some(&base), &data_dir, &fixtures_dir);
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
        let r = save(&doc, Some(&now), &data_dir, &fixtures_dir);
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
        assert!(save(&doc, Some(&base), &data_dir, &fixtures_dir).saved);
        assert!(save(&doc, None, &data_dir, &fixtures_dir).saved);

        // npcs.ron cells are part of the fingerprint too
        let npath = data_dir.join("npcs.ron");
        let ntext = std::fs::read_to_string(&npath).unwrap();
        let cell = data.npcs.npcs["deacon"].cell;
        let needle = format!("cell: ({}, {})", cell[0], cell[1]);
        assert!(ntext.contains(&needle));
        std::fs::write(&npath, ntext.replacen(&needle, "cell: (9, 9)", 1)).unwrap();
        let r = save(&doc, Some(&base), &data_dir, &fixtures_dir);
        assert!(r.stale && !r.saved);

        std::fs::remove_dir_all(&root).ok();
    }
}
