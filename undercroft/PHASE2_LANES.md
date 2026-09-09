# Phase 2, lane step: the five rendering / UI / audio plugins

Written 2026-09-09. The skeleton (`PHASE2_SKELETON.md`, all of it still binding, §8 especially) is
in; this document splits the rest of Phase 2 into five lanes that run in parallel, one plugin each,
in separate git worktrees on disjoint files. HANDOFF.md §6 lists the open items each lane inherits.

## 0. Ground rules (in addition to PHASE2_SKELETON.md §0)

- You own `crates/undercroft/src/<lane>/` (the `mod.rs` stub exists; add submodules freely) and
  nothing else in `crates/undercroft/src/`. Do not edit `lib.rs`, `main.rs`, the skeleton files,
  another lane's directory, `reference/prototype/`, or `assets/data/*.ron`. New assets go under
  `assets/<lane>/` (e.g. `assets/audio/`). Pure helpers may be added to the sim/data crates per
  PHASE2_SKELETON §0; report them.
- New third-party crates: allowed only if no Bevy feature covers the need. Add them to the root
  `Cargo.toml` `[workspace.dependencies]` and reference with `workspace = true`; expect a trivial
  merge. Never enable extra Bevy features without saying so in the report (the list in the root
  manifest is deliberately trimmed; adding one forces a full Bevy rebuild for everyone).
- Read game state only through the skeleton's resources: `Game` (data, config, tuning),
  `ZoneRes`, `HubMapRes`, `Npcs`, `Player`, `LampRes`, `SaveRes`, `HubRes`, `Clock`, `Fade`,
  `state::Mode`, `PrevMode`, `MenuKind`, `Toasts`, `EventLog`, and `MessageReader<SimMessage>`.
  Lanes never mutate them, with two exceptions named below (`MoveIntent` for world,
  `HubMap.mask` for hub). Anything a lane needs the game to *do* goes through
  `DebugQueue::push(DebugCommand::…)` — the same path the tests use.
- Everything visual reads `Player`/`LampRes`/`Zone.hunters` etc. in `Update` (render rate) and the
  sim state in `FixedUpdate` is the truth; interpolation is not required in this step.
- Keep mesh/material/colour construction in pure functions (`fn build_x(&ParsedMap, &Palette) -> Mesh`)
  with unit tests that run without a GPU (`Mesh`, `Color`, `Transform` need no render device).
- Build isolation: your worktree has its own private copy of `target/` and `target-wasm/` (copied
  from the main tree, so Bevy is already built; only our crates recompile). Do NOT set
  `CARGO_TARGET_DIR`; plain `cargo` from your worktree's `undercroft/` uses the local copy. For the
  wasm check use `CARGO_TARGET_DIR=target-wasm` (relative, i.e. your worktree's copy). Sharing one
  target dir across worktrees bakes another tree's `CARGO_MANIFEST_DIR` into `undercroft-data`
  and breaks the headless tests, which is why the copies exist.
- Visual check: each lane runs the real app on its own Xvfb display with the debug script and
  reads the PNGs it produces. Display numbers: world `:91`, creatures `:92`, ui `:93`, hub `:94`,
  audio `:95`. Recipe (from your worktree's `undercroft/`, `PATH` exported):

  ```sh
  Xvfb :9N -screen 0 1280x800x24 >/dev/null 2>&1 &   # your lane's N only
  DISPLAY=:9N WINIT_UNIX_BACKEND=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan \
  UNDERCROFT_SCRIPT="wait 1; begin; wait 2; gotoZone undercroft; wait 1; screenshot /abs/path/a.png; wait 1; quit" \
  timeout 180 cargo run -p undercroft --features dev 2>&1 | tail -30
  ```
  Software Vulkan is slow: keep runs short, expect a few FPS, and never judge performance from it.
  Kill your Xvfb when done. Prefer `teleport x z yaw` steps to frame what you want to see.
- Commit on your lane branch when done (`git commit`, identity is configured), message starting
  "Phase 2 <lane> lane". Report as PHASE2_SKELETON §0 says, plus the PNGs you looked at and what
  they showed.

## 1. Ownership

| Lane | Directory | Ports | Reads | May write |
|---|---|---|---|---|
| world | `src/world/` | `world.js` (blocks, water, gates, shortcuts, items, lanterns, stairs marker, elevator, atmosphere, `setHubLights` → not needed), `main.js` camera/input/pointer lock/render size/`fx.shake`/death camera, `models.js` item/gate/lantern factories | `ZoneRes`, `HubMapRes`, `Player`, `LampRes`, `Game`, `Mode`, `SimMessage` | `MoveIntent` (the only writer), the camera entities |
| creatures | `src/creatures/` | `hunter.js` mesh/anim/`syncMesh`/spotlight/ember burst, `models.js` creature factories | `ZoneRes.hunters`, `Game.data().models`, `Clock`, `SimMessage` | its own entities only |
| ui | `src/ui/` | `ui.js` (HUD, toasts, list menu, title/pause/death/ending/confirm/sound panels, minimap), the menu *content* built in `hub.js` (board, build, service, dialog) and `endgame.js` (altar choice, ending), `main.js` key routing for Title/Dead/Menu | everything read-only, `Toasts`, `Fade`, `MenuKind`, `PrevMode`, `SaveRes`, `Npcs`, `EventLog` | none; pushes `DebugCommand`s |
| hub | `src/hub/` | `hub.js` visuals (buildings, flame tiers, warmth, embers, blessing glow, board papers), `world.js` `buildProps`/`buildSconces`/`setAlcoveTier`/hub lanterns, `npc.js` visuals (residents *and* the in-zone follower), `models.js` prop/building/npc factories | `HubMapRes`, `HubRes`, `SaveRes`, `Npcs`, `Player`, `Clock`, `SimMessage` | `HubMapRes.mask` (prop footprints, once, when the hub map is built) |
| audio | `src/audio/` | `audio.js` entirely | `SimMessage`, `Player`, `LampRes`, `ZoneRes.hunters`, `Mode`, `SaveRes.audio` | its own resources |

Cross-lane rules that must hold on merge:

- Only world spawns cameras, sets `ClearColor`, fog, ambient light, MSAA, or the render target.
  Every other lane spawns ordinary 3D entities (`Mesh3d` + `MeshMaterial3d<StandardMaterial>`,
  `PointLight`, `SpotLight`) in world space; they render through world's camera automatically.
- World coordinates are the sim's: x east, z south, y up, 1 cell = 1 unit, cell `(cx, cz)` spans
  `x ∈ [cx + ox, cx + ox + 1)`, `z ∈ [cz, cz + 1)` where `ox` is `ParsedMap.ox` (0 for zones,
  `HUB_OX` for the hub). The hub and the current zone coexist in one scene at different x, as in
  the JS; fog hides the other one. Nothing toggles scenes by mode. Ceiling height and eye height
  come from `Config` (`CFG.eye`, block sizes); grep the JS for the exact constants.
- Lambert look everywhere: `StandardMaterial { perceptual_roughness: 1.0, reflectance: 0.0,
  unlit: false, ..}`; emissive boxes use `emissive` with the recorded `emissive_k`. Shadows off.
- The 3D camera renders to an offscreen `Image` of size `(w/3, h/3)`, presented full-screen by a
  2D camera with nearest filtering (`main.js:599 renderer.setSize(w/3, h/3)` + CSS `pixelated`).
  World owns this. The 2D camera carries `IsDefaultUiCamera` so `bevy_ui` nodes attach to it and
  draw at full resolution on top. UI must not add cameras; it may rely on the default UI camera.
- Colours in the RON are `0xRRGGBB` `u32`s; convert with one shared helper. World provides
  `pub fn rgb(u32) -> Color` in `src/world/palette.rs`; other lanes may call it but, to avoid a
  compile-time dependency during the parallel step, each lane may also keep a private identical
  copy named `rgb_u32` and the reviewer will dedupe on merge.
- Models: `Game.data().models` (`ModelTable`) holds every `models.js` factory as boxes + parts with
  pivots (`undercroft-data/src/models.rs` doc comment has the conventions: feet origin, front −Z,
  `y` is the box bottom). Creatures and hub both need a "build this `ModelDef` into a
  `Mesh3d` hierarchy with named part entities" routine. To stay independent, **each writes its
  own** in its lane directory (`creatures/model.rs`, `hub/model.rs`) following the same contract:
  root entity → one child entity per `PartDef` (named with `Name`) with `Transform` from
  pivot/rotation/scale → boxes as children of their part (or root) as `Cuboid` meshes, one
  `StandardMaterial` per distinct `(color, emissive, emissive_k)`. The reviewer will merge the two
  into one shared module after the lanes land; keep them small and identical in spirit.

## 2. Lane briefs

### world

1. Camera rig: `Camera3d` at `(Player.x, CFG.eye, Player.z)`, yaw/pitch from `Player`, updated in
   `PostUpdate` (after `FixedUpdate` results) plus `fx.shake` (`main.js`, on `creatureStep` within
   `brute.shakeR`) and the death camera (`main.js:383–394`: looks at the killer, sinks). The handlamp
   `PointLight` parented to the camera with `economy::lamp_intensity` and `Player`/`LampRes` reach
   and colour from `CFG`. Offscreen render target as in §1, resized on `WindowResized`.
2. Input → `MoveIntent` in `Update`, plus the pointer-lock flow (`main.js:539–550`: click to lock in
   Hub/Zone/Dying, release on Escape/menus; `state.locked`, `mouseFree`), keyboard look
   (`CFG.lookKeys × dt`, pre-divided by `mouse_sens` because `player.rs` multiplies), `Shift`
   sprint, WASD/arrows, and key presses forwarded as `DebugCommand::Key(code)` using the JS
   `KeyboardEvent.code` names (`KeyE`, `Escape`, `Digit1`, `Enter`, `Space`, `Backspace`, `Tab`).
3. Zone geometry rebuilt whenever `ZoneRes.id` changes: one merged vertex-coloured mesh per block
   kind (floor, wall, pillar, ceiling, deep, water bed) from the `Palette`, water sheet with the
   `updateWater` bob as a vertex-colour/transform animation (a shader is optional), gates `X`,
   shortcuts `=`, stairs marker, elevator `V`, items (`Zone.items`, bobbing at `ITEM_Y`), planted
   lanterns (`Zone.lanterns`, with their point lights and the pool ring), all diffed against the
   resources each frame (spawn/despawn by index or position, no full rebuilds per frame).
4. Hub *blocks* (walls/floor/ceil/pillars) from `HubMapRes.map` once it is present; props etc. are
   hub's.
5. Atmosphere: fog colour/density, ambient, clear colour from `palettes.ron` on `HubEnter` /
   `ZoneEnter` (`world.js:applyAtmosphere`), lap tint in the Source (`economy::lap_tint`).
6. Tests: mesh builders (vertex counts per block kind for a tiny map, colours), `MoveIntent`
   mapping, camera transform from a `Player`.

### creatures

1. One entity per `Zone.hunters[i]`, keyed by `Hunter.id`, spawned/despawned by diffing each
   frame; model from `ModelTable[profile js name]` (`ProfileKind::js_name`, `hunter.js` picks the
   factory by profile; fallback box `fallback`/`fallbackSize`).
2. `syncMesh` port: transform from `x, z, yaw` (hunters face their motion; check `hunter.js`),
   `Anim` fields → eye emissive `eye_k`, legs `leg_phase`/`leg_swing`, jaw, ember, Warden spot
   light `light_k`/`light_on` on the `conePivot` part, visor `glass_k`, Drowner ring
   `ripple_scale`/`ripple_k`/`ring_y`/`body_shown`, false light `posed_dark`, Warden `plinth`,
   Brute `sway` and the ember/debris burst (`burst_t`, `burst_x/z`, `burst_fired`) as a small
   particle mesh. `active == false` → hidden.
3. Tests: model build from a `ModelDef` (entity count, part names), anim → transform mapping.

### ui

1. `bevy_ui` only (no egui). Full-resolution overlay on the default UI camera. Fonts: the default
   font feature is enabled; a pixel/monospace TTF may be added under `assets/ui/` if wanted.
2. HUD (`ui.js:updateHUD` + `hintText` + `targetLabel`): oil, carried, hint line, contract lines
   (`contracts::hud_lines`), hub HUD (`economy::hub_hud_text`, `endings_hud_line`), follower line
   (`Npcs::hud_line`). Toasts (`Toasts` resource + `SimEvent::Toast`, JS queue timing, flushed on
   `title`). Fade overlay from `Fade.a`. Minimap: an `Image` rewritten at 8 Hz from the save's
   explored bitset and `ZoneRes.map` (`hub.js` minimap drawing), toggled by `Tab`
   (`SimEvent::Minimap`).
3. List-menu widget (`ui.js:makeListMenu`: title, items, selection, depth/stack, key handling
   Up/Down/Enter/Escape/digits) and every screen built on it: main menu (`mainRoot`, incl.
   controls, sound, confirm-new-game with the Backspace×2 wipe), pause (`pauseRoot`, in-zone
   variant), death (`showDeath`), ending (`showEnding`, "Click to continue"), hub menus for
   `MenuKind` `board`/`build`/`service`/`dialog` (content from `contracts::list`,
   `economy::build_status`/`cost_text`, `upgrade_light_tech`/`press_relics`/`deepen_reservoir`
   status text, `Npcs::talk` dialogue via `follower::Dialogue`), altar choice
   (`economy::open_choice` state in `run.rs`'s `EndingScreenRes`). Selections push
   `DebugCommand`s (`SelectZone`, `Accept`, `Build`, `Choose`, `ClosePause`, `NewGame`,
   `ClearSave`, `ReturnToHub`, `ContinueEnding`, `ToMainMenu` → `OpenMainMenu`…). Keys arrive as
   `SimEvent::Key {code, mode}` on the bus and via `DebugCommand::Key` (player.rs forwards some);
   read the `Key` messages for menu navigation.
4. Tests: menu widget state machine (open/close/select/depth) headless; HUD text functions.

### hub

1. Hub props (`world.js:HUB_PROPS_V2` + `PROP_BOXES`), sconces + alcove tiers, hanging lanterns per
   tier, buildings (`hub.js` build meshes per `BuildingDef`, built/unbuilt states, papers on the
   board), the flame (`flameBase` + tier scaling, embers), warmth lights (`HUB_WARMTH`), blessing
   glow; all keyed on `SaveRes`/`HubRes` and `SimEvent::FlameTier`/`Build`/`Blessing`.
2. Prop footprints into `HubMapRes.mask` via `collision::BlockMask::mark_box_cells` +
   `Aabb::of_yawed_box` once when the hub map appears (before that, the player walks through
   props; `run.rs` builds the map at boot, so do it in the first frame `HubMapRes` is `Some`).
3. NPC visuals (`npc.js`): residents at their hub spots (`Npcs::at_hub`), captives in zones, the
   follower (`Npcs::follower`) with walk bob and facing; `NpcDef.coat` colours.
4. Tests: mask footprint for a known prop, model build, tier → lantern visibility table.

### audio

1. Port `audio.js` faithfully. Two halves:
   - One-shots (`SOUNDS` table, ~30 entries, plus creature stings/deaths): render each to a WAV
     under `assets/audio/` with a Node script in `tools/export/audio/` using the `node-web-audio-api`
     npm package (OfflineAudioContext; Rust-backed, no browser). If that package does not install
     or diverges, synthesise them in Rust instead (same synth as below) and say so. Play them with
     `bevy_audio` (`AudioPlayer` + `PlaybackSettings`, volume from `SaveRes.audio` and the JS
     `out(peak, pan)` panning approximated with `SpatialAudio` or plain stereo gain).
   - Continuous layers (drone, lamp crackle, footsteps, hub flame, water wash, drips, hunter
     presence voices `growl/whistle/grind/wash/lure/breath`): a small real-time synth implemented
     as a custom `bevy_audio` `Decodable` source (rodio `Source`) — oscillators (sine/triangle/
     square/saw), the 2 s looped white-noise buffer, a biquad, gain with `setTargetAtTime`-style
     smoothing (`TUNE.tau`), parameters pushed at 30 Hz from a `FixedUpdate` system reading the
     game state exactly as `audio.js:tick` and `presenceTick` do. No extra crate unless truly
     needed (`fundsp` is acceptable; say why).
2. Mute/volume from `SaveRes.audio` and the `SimEvent`s the sound panel emits; the "first
   gesture" unlock is a browser rule: on native start immediately, on wasm start on first input.
3. Tests: the synth as pure DSP (a sine block's RMS, biquad response), the event → sound name
   table covering every `SOUNDS` key, presence `stateMul` selection.
4. This lane cannot see its result in a screenshot; instead render 2 s of the drone and one
   one-shot to WAV from a unit test into the scratchpad and report the file sizes and RMS.

## 3. After the lanes

Merge order: world, creatures, hub, ui, audio (world first because the others' screenshots only
mean something with its camera; all five branch from the same commit, so the order is for
conflict resolution only). Then: a verifier pass (fresh Opus) that runs the full script
`begin → gotoZone → walk → flash → plant lantern → bank → menus → death` on Xvfb with screenshots at
each step and compares against the prototype served at `http://100.114.229.118:8765/reference/prototype/`
in the sandbox container's Firefox for the same script through `window.__game.actions`; then the
owner's review; then Phase 3 (HANDOFF §7).
