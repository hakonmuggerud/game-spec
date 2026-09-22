//! The map editor binary: a local web UI (served by `server`) that edits `assets/data/maps/<id>.txt`,
//! keeps `zones.ron` / `npcs.ron` in sync (shortcut `saves` set to the measured detour), regenerates the
//! fixtures and rewrites the zone's ASCII map block in `DESIGN.md` on save.
//!
//! Environment: `UNDERCROFT_EDITOR_ADDR` (default `0.0.0.0:8790`), `UNDERCROFT_DATA_DIR` (default the
//! workspace `assets/data`), `UNDERCROFT_FIXTURES_DIR` (default the workspace `assets/fixtures`),
//! `UNDERCROFT_DESIGN_MD` (default `<data dir>/../../DESIGN.md`; skipped when missing).

mod design_md;
mod doc;
mod fixtures;
mod ron_io;
mod save;
mod server;

fn main() {
    let config = server::Config::from_env();
    if let Err(e) = server::run(&config) {
        eprintln!("undercroft-editor: {e}");
        std::process::exit(1);
    }
}
