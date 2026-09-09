# Phase 2, skeleton step: contract and work split

Written 2026-09-08. This is the plan for the "skeleton" that HANDOFF.md §6 asks for before the
five rendering/UI lanes fan out. It fixes the shared types every lane will touch. Read
HANDOFF.md §2, §6 and §8 first; the conventions there apply.

## 0. Ground rules for every agent on this step

- Work only in the files your stage assigns you. Never edit `reference/prototype/` or `assets/data/*.ron`.
- Bevy is pinned to 0.19.1. Do not guess API names from memory: the source is unpacked at
  `~/.cargo/registry/src/*/bevy_*-0.19*/`; grep it (`bevy_ecs` for messages/observers/states,
  `bevy_asset` for `AssetLoader`/`LoadContext`, `bevy_time` for `TimeUpdateStrategy`). Bevy ≥ 0.17
  calls buffered events `Message` (`MessageWriter`/`MessageReader`, `add_message`) and reserves
  `Event` for observers. States live in `bevy_state`.
- Build commands. Always export `PATH="$HOME/.cargo/bin:$PATH"` and run from `undercroft/`.
  - `cargo check -p undercroft --features dev` (fast inner loop, ~1 min incremental)
  - `cargo test -p undercroft --features dev` (headless tests; the crate links Bevy dynamically)
  - `cargo clippy -p undercroft --all-targets --features dev -- -D warnings`
  - `cargo check -p undercroft --target wasm32-unknown-unknown` (no `dev`; catches `std::fs`,
    threads and file-watcher uses that break the web build). Use
    `CARGO_TARGET_DIR=target-wasm` for this one.
  - Never run `trunk build` or a full native `cargo run` unless asked; the reviewer does that.
- Work in a git worktree if told to, with `CARGO_TARGET_DIR=/home/agent/repos/game-spec/undercroft/target`
  exported so Bevy is not rebuilt (the dylib and all deps are reused; only our crates recompile).
  Two cargo processes on one target dir serialise on the lock; that is expected.
- Every public item gets a doc comment naming the JS origin (`main.js:startRun`), as in the sim.
- The sim is authoritative for game logic. If a piece of logic you need is missing from the sim
  and is pure (no rendering, no time source, no side effect), add it to the sim crate in a new
  private helper or a clearly named new `pub fn` in the relevant module, with a test, and say so
  in your report. Do not restructure sim modules.
- Nothing in the app crate may use `std::fs`, `std::thread`, `std::time::Instant` or `Instant::now`
  outside `#[cfg(not(target_arch = "wasm32"))]` blocks. Time comes from `Res<Time>`.
- Report at the end: files touched, what you verified with which command and its output summary,
  anything you could not do, and every place you deviated from this document.

## 1. Crate layout after the skeleton (`crates/undercroft/src/`)

| File | Stage | Owner | Contents |
|---|---|---|---|
| `main.rs` | 1 | foundation | thin: window config (native/wasm canvas), `DefaultPlugins`, `UndercroftPlugin`, run |
| `lib.rs` | 1 | foundation | `pub mod` list, `UndercroftPlugin` (adds every skeleton plugin), re-exports |
| `state.rs` | 1 | foundation | `GameMode` states enum + `PrevMode`, `MenuKind`, mode helpers |
| `assets.rs` | 1 | foundation | `GameDataAsset`, its `AssetLoader`, `GameDataHandle`, `Loading → Title` handoff |
| `resources.rs` | 1 | foundation | every shared resource and component (§3) |
| `messages.rs` | 1 | foundation | `SimMessage`, `emit()`, `EventLog` |
| `debug.rs` | 1 | foundation | `DebugCommand` enum, `DebugQueue`, dispatch system set (handlers live in `run.rs`/`player.rs`) |
| `tick.rs` | 1 | foundation | fixed timestep, `SimSet` ordering, `Clock` |
| `headless.rs` | 1 | foundation | `headless_app()` test harness (§5) |
| `run.rs` | 2 | run | `main.js` lifecycle: begin, hub, descend/start run, zone load, bank, dying/dead, return, menus, source run |
| `player.rs` | 2 | player | `main.js` player: movement + collision, lamp, flash, lantern, pickup, interact targets, `PlayerView` |
| `tests/skeleton.rs` | 3 | tests | headless acceptance tests through `DebugCommand` |

Stage 1 is one agent; stage 2 is two agents in parallel on disjoint files; stage 3 is one agent
writing tests, then one independent verifier; then the session owner reviews.

## 2. States (`state.rs`)

```rust
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameMode { #[default] Loading, Title, Hub, Zone, Dying, Dead, Menu, Ending }
```

`Loading` is new (the JS has no async data); everything else mirrors `state.mode` in `main.js:53`.
`PrevMode(Option<GameMode>)` and `MenuKind(Option<String>)` resources mirror `state.prevMode` /
`state.menuKind` (`main.js:426`). Keep the JS semantics: `Menu` is entered from Hub or Zone and
returns to `PrevMode`; the lamp is forced off outside `Zone | Dying | Menu | Ending`
(`main.js:135`). Provide `fn in_run(mode) -> bool` = `Zone | Dying` and `fn sim_runs(mode)` =
`Zone` (creatures and player update only in Zone; `main.js:385` says neither runs while DYING).

## 3. Shared resources and components (`resources.rs`)

These are the only places game state lives. Lanes read them; only `run.rs`/`player.rs` write
them. The foundation agent defines them; field types come from the sim crate, not new copies.

- `Res<GameDataHandle>` and access helper `fn data<'a>(assets: &'a Assets<GameDataAsset>, h: &GameDataHandle) -> &'a GameData`. Prefer a `SystemParam` `Game<'w>` exposing `.data()`, `.tuning()`.
- `Tuning` (sim `creature::Tuning`, built once from `Config` via `creature::tuning`) stored inside the asset or a derived resource that is rebuilt whenever the asset changes (hot reload).
- `SaveRes(pub SaveData)`; `HubRes(pub HubState)`; `LampRes(pub LampState)`.
- `Clock { time: f32 }` — `state.time`, the game clock, advanced every fixed tick in every mode (`main.js:655`).
- `Zone` resource (`Option`-al until a zone is loaded): `id: String`, `map: ParsedMap`, `doors: ZoneDoors`, `pool: Pool`, `lanterns: Vec<pool::Lantern>`, `hunters: Vec<creature::Hunter>`, `items: Vec<WorldItem>` (`kind: ItemKind, x, z, contents: Option<Carried>`; quest and bundle items included), `source: Option<economy::SourceRun>`. `hunters` is the authoritative creature list; the creatures lane spawns one entity per record indexed by `Hunter.id` and mirrors, never owns.
- `HubMap` resource: `map: ParsedMap` from `GameData::parse_hub` plus `mask: collision::BlockMask` (the hub lane fills prop footprints into it later).
- `Npcs` resource wrapping sim `follower::Npcs`.
- `Player` resource (not a component; the world lane owns the camera entity and copies from it): `x, z, yaw, pitch, lamp_on, flash_t, lamp_lock, sprinting, moving, in_water, in_pool, on_deep, oil, carried: Carried, dying_t`, plus `fn view(&self, ...) -> PlayerView`. `MoveIntent { forward: f32, strafe: f32, sprint: bool, look_dx: f32, look_dy: f32 }` written by the input side each frame and consumed by `player.rs` in `FixedUpdate`.
- `Fade { a: f32, mode: FadeMode, pending: Option<PendingTransition> }` mirroring `main.js:209 transition()`; the UI lane draws it, `run.rs` drives it. `PendingTransition` is an enum of the JS callbacks (`EnterHub`, `StartRun(zone)`, `Dead`, `Title`, …), not a closure.
- `Spawns` message-less queue resource for the "side effects the sim returns as data" (HANDOFF §6 list): `pending_quest_items`, `pending_bundles`, `pending_extra_hunters`. `run.rs` drains it.
- `SaveStore` trait object for persistence: native = a file under the platform data dir (`directories`-style path is fine; use `std::env::var("UNDERCROFT_SAVE")` override for tests), wasm = `localStorage` via `web-sys` (declare `web-sys` with the `Storage`, `Window` features in the root `Cargo.toml` as a wasm32-only dep of the app crate). Headless tests use an in-memory store.

## 4. Messages (`messages.rs`) and debug commands (`debug.rs`)

- `#[derive(Message)] pub struct SimMessage(pub SimEvent);` plus `pub fn emit(w: &mut MessageWriter<SimMessage>, evs: Vec<SimEvent>)`.
- `EventLog` resource: ring buffer of `(tick: u64, SimEvent)` capped at 4096, filled by a system in `SimSet::Fanout`; `names()`, `clear()`, `count(name)`. Tests and the future Phase 3 parity checks use it. It exists in native/wasm builds too (cheap, and the pause menu can show it later).
- Per-variant observers are *not* part of the skeleton; lanes read `MessageReader<SimMessage>` and match.
- `DebugCommand` mirrors `main.js:660–740 ctx.actions`, one variant per action that changes state (queries like `saveInfo`, `mainMenu`, `los` are omitted; tests read resources directly). Variants, with the JS name in the doc comment: `Begin, Descend, Bank, Flash, PlantLantern, Interact, TopUp, ToggleLamp, ReturnToHub, Die, EnterHub, OpenMenu(kind), CloseMenu, OpenMainMenu, OpenPause, ClosePause, ClearSave, NewGame, LoadZone(id), SelectZone(id), FreeNpc(id), Accept(id), Build{id, free}, Choose(id), GiveTool(id), SetPoints(n), SetResources{oil, relics, rich}, Rescue(id), UnlockAll, GotoZone(id), Teleport{x, z, yaw: Option<f32>}, SpawnHunter{cx, cz, profile}, SpawnCreature{profile, cx, cz, opts: SpawnOpts}, RideUp, OpenChoice, ContinueEnding, Reset, Key(code)`. `Key` feeds the same key handler the real input uses so tests can press `KeyE`, `Escape`, `Digit1`.
- `DebugQueue(VecDeque<DebugCommand>)` resource + `SimSet::Debug` set that runs first in `FixedUpdate`. `run.rs` and `player.rs` each add a system in that set that drains only the variants they own (peek, take matching, leave the rest); the foundation adds a final system that logs and drops anything left.

## 5. Tick and headless harness (`tick.rs`, `headless.rs`)

- `Time<Fixed>` at 60 Hz. `SimSet` order inside `FixedUpdate`: `Debug → Input → Player → Creatures → Follower → Contracts → Economy → Fanout`. `Input` is where the world lane will turn keyboard/mouse into `MoveIntent`; the skeleton leaves it empty. Cadences (0.2 s sense tick, 0.3 s repath, 0.25 s follower) are already inside the sim; just pass `dt = time.delta_secs()` of the fixed clock.
- `headless_app() -> App`: `MinimalPlugins` + `StatesPlugin` + `AssetPlugin` is *not* used; instead load `GameData::from_dir(GameData::workspace_data_dir())` synchronously, insert it into `Assets<GameDataAsset>` (register the asset type manually) and start in `GameMode::Title`. Add `UndercroftPlugin::headless()` (or a `SkeletonPlugins` group without window/render/audio) so the very same systems run. Provide `fn step(app: &mut App, secs: f32)` that sets `TimeUpdateStrategy::ManualDuration` and calls `app.update()` enough times for `FixedUpdate` to run `round(secs * 60)` ticks, and `fn send(app, DebugCommand)`, `fn log(app) -> &EventLog`, `fn mode(app) -> GameMode`.
- Foundation test (in `headless.rs` under `#[cfg(test)]`): the harness boots, reaches `Title`,
  `GameData` is loaded (`zones.len() == 4`), a test system that writes one `SimMessage` shows up in
  `EventLog`, a `DebugCommand` nobody handles is dropped with a warning and the app keeps stepping.
  The `Begin → Hub` path belongs to stage 2.

## 6. Stage 2 details

### `run.rs` (agent "run")

Port of `main.js` lifecycle, hub and endgame wiring, everything that changes `GameMode`:
`begin`, `enterHub` (`economy::enter_hub` after the fade, `follower.on_rescued`/`place_hub`),
`startRun`/`descend` (`economy::start_run`, `economy::descend`, `apply_saved_openings`,
`contracts::reset_run`, `quests_to_spawn` → items, `creature::spawn_all` + Source dormancy per
`endgame` lap rules, `follower.spawn_zone`), `loadZoneInactive`, `bank`, `die` → `Dying` timer →
`Dead`, `returnToHub`, `toMainMenu`/`runAbandoned`, `openMenu`/`closeMenu`/pause, `clearSave`,
`newGame`, save load/store through `SaveStore` at the same moments the JS calls `save.store()`,
the `hunterCatch → die` and `npcCaught → busy_until` listeners, `HubRes` tier init, Source run
(`start_source_run`, `update_source_run`, `on_deeper`, `ride_up`, extra spawns), endings
(`open_choice`, `choose_ending`, `continue_to_hub`). Handles the `DebugCommand`s: Begin, Descend,
Bank, Die, EnterHub, menus, ClearSave, NewGame, LoadZone, SelectZone, FreeNpc, Accept, Build,
Choose, GiveTool, SetPoints, SetResources, Rescue, UnlockAll, GotoZone, SpawnHunter,
SpawnCreature, RideUp, OpenChoice, ContinueEnding, Reset. Also runs `creature::update`,
`follower.update_zone/update_hub`, `contracts::tick` in their `SimSet`s and applies the sim's
returned side effects (HANDOFF §6 bullet list) — lantern smash → remove + `Pool::recompute`,
`lampSnuffed` → `economy::on_lamp_snuffed` (Zone only), death bundles, extra hunters.

### `player.rs` (agent "player")

Port of `main.js` `updatePlayer`/`updateLamp`/`flash`/`plantLantern`/`topUp`/`toggleLamp`/
`pickup`/`interactTarget`/`interact` and `world.js` water enter/exit: consume `MoveIntent`,
`collision::move_player` against the current map (hub uses `HubMap.mask`), cell flags
(`in_water`, `on_deep`, `in_pool` via `pool::in_pool`, `lap`), `economy::update_lamp` each tick,
`economy::toggle_lamp`, `top_up`, `flash` (+ `creature::flash`), `plant_lantern` (+
`Pool::recompute`, `contracts::on_lantern`), item pickup within `CFG` reach (`economy::pickup`,
`contracts::on_pickup`), the interact target resolver (items, bank, descend, gate, shortcut, NPC,
building, altar, elevator; `economy::hub_interact_target`, `world::gate_status`/`open_gate`,
`shortcut_status`/`open_shortcut`, `follower.interact_target`/`talk`), the `Key` handler for the
in-run keys (`main.js:560–640`: E, F, Q, R, Shift, Escape, digits), `PlayerView` construction
once per fixed tick into a `Res<PlayerViewRes>` that `run.rs` passes to the sim, and the minimap
exploration tick (`economy::explore_tick` every 0.25 s with LOS, into the save bitset). Handles
the `DebugCommand`s: Flash, PlantLantern, Interact, TopUp, ToggleLamp, Teleport, Key.

Interface between the two: `run.rs` calls `Player::reset_at(x, z, yaw)` (defined in
`resources.rs`) when a run starts; `player.rs` never changes `GameMode` directly, it pushes
`SimEvent`s and, for bank/descend/die, pushes the matching `DebugCommand` into `DebugQueue`
(same as the JS where `interact` calls `actions.bank()`).

## 7. Stage 3: tests and verification

`tests/skeleton.rs` (agent "tests", Sonnet), through the harness only, each test independent:

1. boots to Title with data loaded; `Begin` → Hub, log `begin, hubEnter`; hub HUD text non-empty.
2. `GotoZone("undercroft")` → Zone; `Zone` resource populated; hunters spawned equals the map's
   spawn count; `zoneEnter` logged; player at the zone entry.
3. `Teleport` next to a hunter cell, `SpawnHunter{profile: "base"}` adjacent, step 10 s →
   `hunterCatch` then `death`, mode `Dying` then `Dead` after `CFG.dyingT`; `ReturnToHub` → Hub.
4. pickup: teleport onto an item, `Interact` or step → `pickup` logged, `carried` updated;
   `Teleport` to the bank marker, `Bank` → `bank` logged, save points increased, mode Hub after fade.
5. lamp: `ToggleLamp` twice logs `lampToggle {on:false}` then `{on:true}`; `Flash` logs `flash`;
   `PlantLantern` logs `lantern` and `Zone.pool` is non-empty.
6. menus: `OpenPause` from Hub → Menu with `PrevMode == Hub`; `ClosePause` → Hub; `OpenMainMenu`
   from Zone → Title with `runAbandoned`.
7. contracts + tools: `UnlockAll` then `SetPoints(0)`; `GiveTool` twice returns/logs
   `toolGained` once.
8. Source: `UnlockAll`, `GotoZone("source")`, dormant `L Y B` creatures are `active == false`;
   teleport across the first lap line → `lap` logged and at least one `hunterWoken`.
9. save round trip: bank, then a fresh `headless_app()` sharing the same in-memory `SaveStore`
   handle starts with the same points.
10. determinism: two apps seeded with `SimRng::seed(7)` produce identical `EventLog` names after
    the same command script.

Verifier (Opus, fresh context, no edits except to fix its own findings): reads HANDOFF.md and
this file, diffs `git diff main...HEAD -- crates/undercroft`, runs clippy, the tests, the wasm
check, and `cargo run` natively on Xvfb per HANDOFF §5 to confirm the window still opens at
Title; checks every HANDOFF §6 "side effects" bullet has an implementation site; checks no
sim function was reimplemented in the app crate; reports findings ranked by severity with
file:line, and fixes only clear-cut defects (compile/clippy/test failures, wrong JS number)
itself.

## 8. Stage 1 outcome (what stage 2 must know)

Committed as "Phase 2 skeleton: stage 1 foundation". Differences from §3–§5 above, all deliberate:

- Debug handlers must be in `DebugSet::Handle` (a sub-set of `SimSet::Debug`); the sweep that drops
  unhandled commands runs in `DebugSet::Drain` after it.
- `Player` has no lamp fields; `oil`, `lamp_on`, `flash_t`, `lamp_lock` live only in `LampRes`, and
  `Player::view(&self, &LampState, lamp_reach, follower)` builds the `PlayerView`.
- `ZoneRes(Option<Zone>)` and `HubMapRes(Option<HubMap>)` are always present; nothing inserts or
  removes resources at runtime. `HubMapRes` is empty until `run.rs` fills it (begin / enterHub).
- `Tuning` lives inside `GameDataAsset`; use the `Game` system param (`.data()`, `.config()`, `.tuning()`).
- `TickCount` is the monotone tick counter; `Clock.time` advances every tick in every mode, as `main.js:655` does (the verifier caught the earlier "Zone only" wording contradicting the JS).
- `Fade` is defined but undriven; `run.rs` owns `updateFade` (`main.js:210`). Add `PendingTransition`
  variants as needed, never closures.
- `SaveStore` moves JSON strings; `run.rs` owns (de)serialisation and migration through `sim::save`.
- `headless.rs` is native-only (`cfg(not(wasm32))`) and compiled in every native build.
- The data has 4 zones plus the hub map.
