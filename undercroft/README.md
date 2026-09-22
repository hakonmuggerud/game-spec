# The Undercroft

A lamp-lit dungeon crawler about a light economy: descend from a dying hub into pitch-black ruins,
survive the hunters your lamp draws, bank what you carry, and watch the flame grow. Rust + Bevy,
native (Linux) and web (wasm). The design, with every rule and number, is in [`DESIGN.md`](DESIGN.md);
the numbers themselves live in `assets/data/*.ron`, which is the source of truth.

| Crate | Depends on | Owns |
|---|---|---|
| `crates/undercroft-data` | serde, ron | the types every `assets/data/*.ron` table deserialises into, the RON / text-map loaders, the map parser |
| `crates/undercroft-sim` | data | pure game logic: grid, BFS, LOS, collision, map validator, creature FSMs, follower, contracts, economy, save schema. Returns `Vec<SimEvent>`; randomness through `SimRng` |
| `crates/undercroft` | data, sim, bevy | the app: skeleton (states, resources, messages, debug commands, fixed tick, headless harness) + one plugin per lane (`world`, `creatures`, `ui`, `hub`, `audio`) |
| `crates/undercroft-editor` | data, sim, tiny_http | the map editor: a native-only dev server that serves `web/index.html` (one page, canvas, no build step), previews edits through the validator and writes maps, `zones.ron`, `npcs.ron` and the fixtures on save |

"Functional core, ECS shell": the sim crate exposes plain structs and pure functions, Bevy owns
entities and lifetimes and calls into the sim from `FixedUpdate` at 60 Hz. DESIGN.md §12 is the map
of the code.

## Layout

```
Cargo.toml            workspace; default-members = data + sim + editor, so a bare `cargo test` is cheap (no Bevy)
crates/               the four crates above
assets/data/          the game data: eight RON tables + maps/*.txt (SOURCE OF TRUTH; edit directly or with the map editor)
assets/fixtures/      frozen parity fixtures the sim tests compare against (README there)
assets/audio/         59 one-shot WAVs + manifest.ron (source; the continuous layers are synthesised)
assets/ui/            Adwaita Mono (OFL)
web/index.html, Trunk.toml   the wasm shell
tools/qa/             headless-Chromium driver for the frozen prototype (side-by-side screenshots)
docs/history/         how the port was run (Phase 2 contracts, the old handoff)
../reference/         the frozen Three.js prototype the port was verified against, and its export tools
```

## Build and test

Toolchain: `rustup` stable (see `rust-toolchain.toml`) with the `wasm32-unknown-unknown` target,
`clang` + `mold` (see `.cargo/config.toml`), Bevy's Linux deps (alsa, udev, wayland, xkbcommon, x11,
xcursor, xrandr, xi, mesa), and `trunk` for the web build. A cold Bevy build takes 15–25 minutes and
wants 8 GB+; after that our crates rebuild in seconds (the `dev` feature links Bevy dynamically).

Always `export PATH="$HOME/.cargo/bin:$PATH"` and work from this directory.

```sh
cargo test                                                     # data + sim + editor, 162 tests, ~1 min cold
cargo test -p undercroft --features dev                        # app: 164 unit + 10 integration, headless
cargo clippy --workspace --all-targets --features undercroft/dev -- -D warnings
cargo fmt --all -- --check
CARGO_TARGET_DIR=target-wasm cargo check -p undercroft --target wasm32-unknown-unknown
CARGO_TARGET_DIR=target-wasm trunk build                       # dist/, ~80 s incremental, 106 MB dev wasm
```

Those six are the definition of green; run them before a commit that touches code or data.
`dev` (dynamic linking, asset file watcher) must never be enabled for the wasm target, which is why
the wasm commands use their own target dir.

## Run

```sh
cargo run -p undercroft --features dev          # native; saves to ./undercroft-save.json (UNDERCROFT_SAVE overrides)
```

Web: `trunk build` writes `dist/` (`trunk build --release` for a small binary, slow). Serve the repo
root and open `/undercroft/dist/`; the frozen prototype is next to it for a side-by-side:

```sh
python3 -m http.server 8765 --bind 0.0.0.0 --directory /home/agent/repos/game-spec
# http://100.114.229.118:8765/undercroft/dist/                  (Bevy)
# http://100.114.229.118:8765/reference/prototype/index.html    (Three.js reference)
```

### Map editor

```sh
cargo run -p undercroft-editor                  # http://0.0.0.0:8790/ (UNDERCROFT_EDITOR_ADDR overrides)
```

A local web page (`crates/undercroft-editor/web/index.html`, vanilla JS, no internet needed) that
draws each zone's grid and lets you move entities and doors, paint or erase cells, and drag anchors,
with the Rust validator (`undercroft_sim::validate`) re-run on every change. `UNDERCROFT_DATA_DIR` and
`UNDERCROFT_FIXTURES_DIR` point it at copies. Every request reloads the data from disk; the page keeps
the zone it loaded, so if the files change under it (a hand edit, another tab) the save is refused
as stale and the page asks before overwriting — reload the page to pick the disk version up.
Ctrl+Z / Ctrl+Shift+Z undo, Ctrl+S saves (after a confirmation when the validator reports errors or
warnings; a map that cannot parse is never written). A save is all-or-nothing: every file is
rendered first, then written by temp-file + rename, and a failure rolls the rest back. Zones only —
the hub is not editable here.

A save writes, in order: `assets/data/maps/<id>.txt`; `assets/data/zones.ron` (the whole file,
header comment kept, so a moved NPC / gate / spot / shortcut door / anchor cell follows the grid, and
every shortcut's `saves` is set to the detour the validator measured, so `zones.ron`, the fixture and
the stats agree — the status line reports `u_rood saves 152 -> 154`); `assets/data/npcs.ron` only
when an NPC cell changed; then it regenerates `assets/fixtures/{undercroft,cistern,ossuary,source}.json`
and `assets/fixtures/validate_all.json` from the sim (the hub fixtures are untouched), so the
fixture-driven tests stay green; and last `DESIGN.md` (the file next to `assets/`, `UNDERCROFT_DESIGN_MD`
overrides, skipped with a note when it is missing): the zone's ASCII map block, and every number and
`(x,y)` the tables quote for the zone — its §3.2 row (entry, walkable, captive and gate cells), its
"Contract spots" segment, its shortcut rows (cells, `opens from`, detour, route) and its §3.3 row
(walkable, walls %, pillars, water, route and the percentage) — spliced in place, the prose around them
kept; the hub block is never touched. `design_md_matches_the_data` in `undercroft-data` reads the same
tables and map blocks and fails naming the DESIGN.md line, the documented value and the data value, so
a stale number cannot reach a commit. The tests of the sim and data crates read the maps' cells and the
validator's numbers from the data rather than pinning them, so after a save `cargo test` is green with no
hand edit. What still needs a hand: DESIGN.md prose about a place (the §3.4 region notes, a door's
"links" text, a captive's name) and anything the save reports in a note (a captive the table names that
no NPC carries, a door the table lists that `zones.ron` does not have).

### Scripted runs and screenshots

`UNDERCROFT_SCRIPT` drives the app through `DebugCommand::parse` (`crates/undercroft/src/debug.rs`):
every debug action by its JS name (`begin`, `unlockAll`, `gotoZone id`, `teleport x z yaw`, `key KeyE`,
`openMenu kind id`, `spawnHunter cx cz profile`, …) plus `wait N` (game seconds), `screenshot path`
and `quit`. Keep a `wait` between `screenshot` and `quit`. No window manager is needed:

```sh
Xvfb :90 -screen 0 1280x800x24 >/dev/null 2>&1 &
DISPLAY=:90 WINIT_UNIX_BACKEND=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan \
UNDERCROFT_SAVE=/tmp/save.json \
UNDERCROFT_SCRIPT="wait 1; begin; wait 2; unlockAll; wait 1; screenshot /tmp/hub.png; gotoZone undercroft; wait 2; screenshot /tmp/zone.png; wait 1; quit" \
cargo run -p undercroft --features dev
```

The same step list drives the prototype through `tools/qa/proto.mjs` (`shot path` instead of
`screenshot`; README there), which is how parity was checked. Xvfb + lavapipe occasionally saves an
empty frame; re-run rather than debug.

`headless_app()` in `crates/undercroft/src/headless.rs` is the test harness (no window, in-memory
save store); `crates/undercroft/tests/skeleton.rs` shows the idioms (`send`, `step`, `log`, `mode`).

## Working on the game

- **Data.** Edit `assets/data/*.ron` and `maps/*.txt` directly; each file's first line names the
  Rust type it must parse as, and `every_ron_file_parses` in `undercroft-data` reports a bad edit by
  file and line. For the zone maps the easy way is the map editor (`cargo run -p undercroft-editor`,
  see Run): it keeps `zones.ron` / `npcs.ron` in step with the grid (shortcut `saves` = the measured
  detour), regenerates the fixtures and rewrites the zone's map block and table numbers in DESIGN.md,
  so a map save needs no hand edit beyond prose (§3.4 region notes). A hand edit to a map or a table
  that shifts a number is caught by `design_md_matches_the_data`, which names the stale DESIGN.md line.
  The maps and `config.ron` hot-reload natively under `dev`. The fixtures under
  `assets/fixtures/` pin the prototype's numbers, so a deliberate tuning change that breaks a
  fixture test must update the fixture in the same commit (the editor does this for map changes).
  When a number changes, fix it in DESIGN.md too.
- **Modes.** Inside `FixedUpdate` always read the mode through `state::Mode` (the pending state
  wins), never `State<GameMode>` directly; two bugs came from that.
- **Writers.** Lanes only push `DebugCommand`s to change game state; `run.rs` / `player.rs` are the
  writers. Exceptions on record: world writes `MoveIntent`, hub writes `HubMap.mask`, audio owns
  `save.audio`, ui writes `Toasts`.
- **Light units.** JS candela go straight into `PointLight` / `SpotLight::intensity` and emissives
  carry exposure weight 0, because the world camera runs at `world::palette::EXPOSURE_EV100`.
- **Portability.** The app crate never uses `std::fs` / `std::thread` / `Instant` outside
  `cfg(not(wasm32))`; the wasm `cargo check` above catches it.
- **Doc comments** on public items cite their origin in the prototype
  (`reference/prototype/src/hunter.js:profile`) as history; keep the habit for ported items.
- **Dependencies** are declared once in the root `Cargo.toml` `[workspace.dependencies]`; member
  crates opt in with `{ workspace = true }`.
- **Worktrees.** If you work in a git worktree, give it its own copy of `target/`; sharing one bakes
  another tree's `CARGO_MANIFEST_DIR` into `undercroft-data` and breaks the headless tests. If they
  suddenly fail on a clean tree: `cargo clean -p undercroft-data -p undercroft-sim -p undercroft`.

## Status

The port is functionally complete against the prototype: a 15-step scripted playthrough matches it
frame for frame, and the owner has accepted the wasm build. Known, deliberately unfixed visual
deltas: flat death vignette, menu panels ~65 px narrower, chunkier hub embers, slashed-zero font,
missing hub stairwell steps, a Brute smash applied one tick late. The acceptance checklists in
DESIGN.md §5.9 and §13 are the playtest script when one is wanted.
