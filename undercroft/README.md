# The Undercroft — Bevy port

Cargo workspace for the Rust/Bevy port (resuming on a new machine? read [`HANDOFF.md`](HANDOFF.md)) of [`../reference/prototype`](../reference/prototype) (the Three.js prototype
stays untouched as the playable reference until this build passes DESIGN.md §5.9 and §13).

| Crate | Depends on | Owns |
|---|---|---|
| `crates/undercroft-data` | serde, ron | shared types mirroring `reference/prototype/src/*.js`, RON/text-map loaders, the map parser |
| `crates/undercroft-sim` | data | grid, BFS, LOS, collision, map validator, creature FSMs, contracts, economy, save schema |
| `crates/undercroft` | data, sim, bevy | the app: one Bevy plugin per lane (world, creatures, ui, hub, audio) |

"Functional core, ECS shell": the sim crate exposes plain structs and pure functions, Bevy owns entities
and lifetimes and calls into the sim from `FixedUpdate`. Nothing is mirrored between two worlds.

## Build

Toolchain: `rustup` (stable, see `rust-toolchain.toml`), `clang` + `mold` (see `.cargo/config.toml`),
and Bevy's Linux deps (`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libx11-dev ...`).

```sh
cargo test                                   # data + sim tests, no window needed
cargo build -p undercroft --features dev     # native, dynamic-linked Bevy (fast rebuilds)
cargo run   -p undercroft --features dev
```

## Web (wasm)

`trunk` (prebuilt binary in `~/.cargo/bin`) builds `web/index.html` + the wasm into `dist/`:

```sh
trunk build            # dev: no wasm-opt, ~1 minute after the first build
trunk build --release  # small binary, slow
```

Serve the repo root on the tailnet port and open `/undercroft/dist/`:

```sh
python3 -m http.server 8765 --bind 0.0.0.0 --directory /home/agent/repos/game-spec
# http://100.87.43.62:8765/undercroft/dist/       (Bevy)
# http://100.87.43.62:8765/reference/prototype/index.html   (Three.js reference)
```

`dev` (dynamic linking, file watcher) must never be enabled for the wasm target.

## Data pipeline

Nothing in `assets/data/` is typed by hand. `tools/export/export.mjs` (node 22) evaluates the prototype's ES
modules and dumps every table to JSON plus the ASCII maps (`assets/data/maps/*.txt`) and the parity fixtures
(`assets/fixtures/*.json`, see its README); `cargo run -p undercroft-data --bin json2ron` turns the JSON into
`assets/data/*.ron` through the Rust types, so the RON always matches them:

```sh
cd tools/export && npm install && node export.mjs && cd ../..
cargo run -p undercroft-data --bin json2ron
cargo test                                   # data spot-checks + grid parity against the fixtures
```

## Dependencies

All third-party crates are declared once in the root `Cargo.toml` `[workspace.dependencies]`;
member crates opt in with `{ workspace = true }`. Add new ones there, in one place.
