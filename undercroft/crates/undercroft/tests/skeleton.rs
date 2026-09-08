//! Stage-3 acceptance tests (PHASE2_SKELETON.md §7): end-to-end behaviour driven only through the
//! `DebugCommand` console (the Rust `window.__game.actions`), exactly the way a real client would
//! start a run, die, bank and so on. Each test builds its own [`headless_app`] (a fresh in-memory
//! save store per app — see `headless.rs`) and is independent of the others.
//!
//! Only `undercroft::headless::*`, `undercroft::debug::DebugCommand`, `undercroft::state::GameMode`,
//! resources read with `app.world().resource::<T>()`, and `undercroft_sim`/`undercroft_data` types
//! are used — the same surface a black-box parity trace would have.

use bevy::prelude::*;

use undercroft::debug::DebugCommand;
use undercroft::headless::*;
use undercroft::state::GameMode;
use undercroft::{GameDataAsset, GameDataHandle, LampRes, Player, PrevMode, SaveRes, ZoneRes};

use undercroft_data::{CellKind, GameData};
use undercroft_sim::{economy, grid, SimEvent};

/// The loaded `GameData`, cloned out from behind the asset handle (any app; the data is the same
/// `assets/data/` directory for all of them).
fn game_data(app: &App) -> GameData {
    let handle = app.world().resource::<GameDataHandle>().0.clone();
    let assets = app.world().resource::<Assets<GameDataAsset>>();
    assets.get(&handle).expect("data loaded").data.clone()
}

/// Start a real run the way `actions.gotoZone(id)` does and wait for `Zone` to apply. `headless_app`
/// gives every app its own fresh in-memory save store, so nothing leaks between tests.
fn app_in_zone(id: &str) -> App {
    let mut app = headless_app();
    send(&mut app, DebugCommand::GotoZone(id.into()));
    step(&mut app, 0.5);
    // `NextState` is applied once per frame, before `FixedUpdate` — one more tick makes sure the
    // mode this test reads has actually caught up (PHASE2_SKELETON.md §5 / the run lane's note).
    step(&mut app, 1.0 / 60.0);
    assert_eq!(
        mode(&app),
        GameMode::Zone,
        "gotoZone starts the run immediately"
    );
    app
}

/// Test 1 — `main.js`'s module-scope init block runs once at load, well before the player can click
/// Begin, and announces the flame tier (`hub.js:init` → `flameTier {initial:true}`); `begin()`
/// (`main.js:begin`) then fades into the hub, `enterHub` (`main.js:enterHub`) logging `hubEnter`
/// after it — so the log order is `flameTier`, `begin`, `hubEnter`.
#[test]
fn boot_then_begin_enters_the_hub_in_order() {
    let mut app = headless_app();
    step(&mut app, 1.0 / 60.0);
    assert_eq!(mode(&app), GameMode::Title);
    let data = game_data(&app);
    assert!(!data.zones.is_empty(), "GameData loaded");

    send(&mut app, DebugCommand::Begin);
    step(&mut app, 2.0);
    assert_eq!(mode(&app), GameMode::Hub);

    let names = log(&app).names();
    let tier_at = names
        .iter()
        .position(|n| *n == "flameTier")
        .expect("the boot-time flameTier{initial:true}");
    let begin_at = names.iter().position(|n| *n == "begin").expect("begin");
    let hub_at = names
        .iter()
        .position(|n| *n == "hubEnter")
        .expect("hubEnter");
    assert!(tier_at < begin_at, "flameTier before begin: {names:?}");
    assert!(begin_at < hub_at, "begin before hubEnter: {names:?}");

    // §7.1 "hub HUD text non-empty": there is no UI lane in this skeleton, so this calls the same
    // pure sim function it will (`hub.js:updateHud`).
    let save = app.world().resource::<SaveRes>().0.clone();
    let hud = economy::hub_hud_text(&data, &save);
    assert!(!hud.is_empty(), "hub HUD text");
}

/// Test 2 — `actions.gotoZone(id)` (`main.js:actions.gotoZone`) spawns one hunter/creature record per map
/// spawn cell (`hunter.js:spawnAll`) and drops the player at the map's entry marker
/// (`main.js:spawnAt`).
#[test]
fn goto_zone_spawns_every_marker_and_places_the_player_at_the_entry() {
    let app = app_in_zone("undercroft");
    let data = game_data(&app);
    let map = data
        .parse_zone("undercroft")
        .expect("zone")
        .expect("parses");
    let expected_hunters = map.hunter_spawns.len() + map.creatures.len();

    {
        let z = app.world().resource::<ZoneRes>();
        let zone = z.get().expect("zone loaded");
        assert_eq!(zone.id, "undercroft");
        assert_eq!(
            zone.hunters.len(),
            expected_hunters,
            "one record per spawn marker"
        );
        assert!(
            zone.hunters.iter().all(|h| h.active),
            "spawnAll activates everything"
        );
    }
    assert_eq!(log(&app).count("zoneEnter"), 1);

    let entry = map.stairs.expect("every zone has an entry").marker;
    let player = app.world().resource::<Player>();
    assert!(
        (player.x - entry.x).abs() < 1e-4 && (player.z - entry.z).abs() < 1e-4,
        "player at ({}, {}), entry at ({}, {})",
        player.x,
        player.z,
        entry.x,
        entry.z
    );
}

/// Test 3 — `hunter.js`'s catch (`driver.rs::dist2d <= catch_r` while `st.catch`) fires `hunterCatch
/// {target: Player}`; `main.js:747`'s listener calls `die()`, which enters `DYING` for
/// `CFG.dyingT` seconds and then `DEAD`; `actions.returnToHub()` fades back to the hub.
#[test]
fn a_hunter_catch_kills_the_player_and_returns_to_the_hub() {
    let mut app = app_in_zone("undercroft");
    // `startRun` leaves the handlamp lit, so the hunter's sight sense registers the player at once.
    assert!(
        app.world().resource::<LampRes>().0.lamp_on,
        "startRun lights the handlamp"
    );
    let map = game_data(&app)
        .parse_zone("undercroft")
        .expect("zone")
        .expect("parses");

    // Two adjacent floor cells: the player stands on one, the hunter spawns on the other.
    let (px, pz, hcx, hcz) = 'search: {
        for cz in 0..map.h {
            for cx in 0..map.w {
                if map.cell_type(cx, cz) != CellKind::Floor {
                    continue;
                }
                for (dx, dz) in grid::DIRS4 {
                    if map.cell_type(cx + dx, cz + dz) == CellKind::Floor {
                        let (x, z) = grid::center(&map, cx, cz);
                        break 'search (x, z, cx + dx, cz + dz);
                    }
                }
            }
        }
        panic!("no two adjacent floor cells in undercroft");
    };

    send(
        &mut app,
        DebugCommand::Teleport {
            x: px,
            z: pz,
            yaw: Some(0.0),
        },
    );
    send(
        &mut app,
        DebugCommand::SpawnHunter {
            cx: hcx,
            cz: hcz,
            profile: "base".into(),
        },
    );
    step(&mut app, 1.0 / 60.0);

    let mut caught = false;
    for _ in 0..20 {
        // up to 10 s in 0.5 s slices
        step(&mut app, 0.5);
        if log(&app).count("hunterCatch") > 0 {
            caught = true;
            break;
        }
    }
    assert!(caught, "no hunterCatch within 10s: {:?}", log(&app).names());
    assert_eq!(log(&app).count("death"), 1);
    step(&mut app, 1.0 / 60.0);
    assert_eq!(mode(&app), GameMode::Dying);

    let dying_t = game_data(&app).config.cfg.dying_t;
    step(&mut app, dying_t + 2.0 / 60.0);
    assert_eq!(mode(&app), GameMode::Dead);

    send(&mut app, DebugCommand::ReturnToHub);
    step(&mut app, 2.0);
    assert_eq!(mode(&app), GameMode::Hub);
}

/// Test 4 — `main.js:pickup()`/`interact()` picks up a map item within reach; `bank()` (`main.js:bank`)
/// fades out, credits the ledger (`hub.js`'s `bank` listener, inside `economy::bank`) and enters
/// the hub.
#[test]
fn pickup_then_bank_credits_the_ledger_and_returns_to_the_hub() {
    let mut app = app_in_zone("undercroft");
    let (ix, iz, kind) = {
        let z = app.world().resource::<ZoneRes>();
        let it = z.get().expect("zone").items.first().expect("an item");
        (it.x, it.z, it.kind)
    };
    // stand half a unit south of the item, facing north: forward = (0, -1) at yaw 0.
    send(
        &mut app,
        DebugCommand::Teleport {
            x: ix,
            z: iz + 0.5,
            yaw: Some(0.0),
        },
    );
    step(&mut app, 1.0 / 60.0);
    send(&mut app, DebugCommand::Interact);
    step(&mut app, 1.0 / 60.0);
    assert_eq!(log(&app).count("pickup"), 1);
    let carried = app.world().resource::<Player>().carried;
    assert!(!carried.is_empty(), "carried changed");
    assert_eq!(carried.get(kind), 1);

    let before_points = app.world().resource::<SaveRes>().0.points;

    let (sx, sz) = {
        let z = app.world().resource::<ZoneRes>();
        let s = z
            .get()
            .expect("zone")
            .map
            .stairs
            .expect("stairs/bank marker");
        (s.marker.x, s.marker.z)
    };
    send(
        &mut app,
        DebugCommand::Teleport {
            x: sx,
            z: sz,
            yaw: Some(0.0),
        },
    );
    step(&mut app, 1.0 / 60.0);
    send(&mut app, DebugCommand::Bank);
    step(&mut app, 2.0);

    assert_eq!(log(&app).count("bank"), 1);
    let after_points = app.world().resource::<SaveRes>().0.points;
    assert!(
        after_points > before_points,
        "banking increased save points: {before_points} -> {after_points}"
    );
    assert!(app.world().resource::<Player>().carried.is_empty());
    assert_eq!(mode(&app), GameMode::Hub, "the fade lands in the hub");
}

/// Test 5 — `main.js:toggleLamp()` / `flash()` / `plantLantern()`.
#[test]
fn lamp_flash_and_lantern_are_logged() {
    let mut app = app_in_zone("undercroft");
    assert!(
        app.world().resource::<LampRes>().0.lamp_on,
        "startRun lights the handlamp"
    );

    send(&mut app, DebugCommand::ToggleLamp);
    step(&mut app, 1.0 / 60.0);
    send(&mut app, DebugCommand::ToggleLamp);
    step(&mut app, 1.0 / 60.0);
    let toggles: Vec<bool> = log(&app)
        .all("lampToggle")
        .iter()
        .filter_map(|e| match e {
            SimEvent::LampToggle { on } => Some(*on),
            _ => None,
        })
        .collect();
    assert_eq!(toggles, vec![false, true]);

    send(&mut app, DebugCommand::Flash);
    step(&mut app, 1.0 / 60.0);
    assert_eq!(log(&app).count("flash"), 1);

    send(&mut app, DebugCommand::PlantLantern);
    step(&mut app, 1.0 / 60.0);
    assert_eq!(log(&app).count("lantern"), 1);
    let z = app.world().resource::<ZoneRes>();
    assert!(
        z.get().expect("zone").pool.count() > 0,
        "Zone.pool non-empty"
    );
}

/// Test 6 — `main.js:openPause`/`closePause` (`state.prevMode`) and `toMainMenu()` abandoning a run.
#[test]
fn menus_round_trip_and_the_main_menu_abandons_a_run() {
    let mut app = headless_app();
    send(&mut app, DebugCommand::Begin);
    step(&mut app, 0.5);
    assert_eq!(mode(&app), GameMode::Hub);

    send(&mut app, DebugCommand::OpenPause);
    step(&mut app, 1.0 / 30.0);
    assert_eq!(mode(&app), GameMode::Menu);
    assert_eq!(app.world().resource::<PrevMode>().0, Some(GameMode::Hub));

    send(&mut app, DebugCommand::ClosePause);
    step(&mut app, 1.0 / 30.0);
    assert_eq!(mode(&app), GameMode::Hub);

    send(&mut app, DebugCommand::GotoZone("undercroft".into()));
    step(&mut app, 0.5);
    assert_eq!(mode(&app), GameMode::Zone);

    send(&mut app, DebugCommand::OpenMainMenu);
    step(&mut app, 0.5);
    assert_eq!(mode(&app), GameMode::Title);
    assert_eq!(log(&app).count("runAbandoned"), 1);
}

/// Test 7 — `actions.unlockAll()` then `actions.setPoints(0)` re-derives and announces the flame tier
/// (`hub.js:checkTier`); `actions.giveTool(id)` is idempotent (`toolGained` fires once).
#[test]
fn unlock_all_then_zero_points_retiers_and_give_tool_is_idempotent() {
    let mut app = headless_app();
    send(&mut app, DebugCommand::Begin);
    step(&mut app, 0.5);

    // Before `unlockAll` (which already sets every tool), so `toolGained` has something to do.
    send(&mut app, DebugCommand::GiveTool("prybar".into()));
    send(&mut app, DebugCommand::GiveTool("prybar".into()));
    step(&mut app, 0.5);
    assert!(app.world().resource::<SaveRes>().0.tools.prybar);
    assert_eq!(log(&app).count("toolGained"), 1);

    send(&mut app, DebugCommand::UnlockAll);
    step(&mut app, 1.0 / 60.0);
    let after_unlock = app.world().resource::<SaveRes>().0.points;
    assert!(after_unlock > 0, "unlockAll raises points to the top tier");

    send(&mut app, DebugCommand::SetPoints(0));
    step(&mut app, 1.0 / 60.0);
    assert_eq!(app.world().resource::<SaveRes>().0.points, 0);
    let retier: Vec<u32> = log(&app)
        .all("flameTier")
        .iter()
        .filter_map(|e| match e {
            SimEvent::FlameTier {
                tier,
                initial: false,
                ..
            } => Some(*tier),
            _ => None,
        })
        .collect();
    assert_eq!(
        retier,
        vec![1],
        "0 points re-tiers to 1: {:?}",
        log(&app).names()
    );
}

/// Test 8 — `endgame.js:startRun()` dormancy (PHASE2_SKELETON.md §6/§8: `spawn_all` activates everything,
/// the shell puts the deep `L Y B` creatures back to sleep) and `endgame.js:onDeeper` (crossing a
/// lap line logs `lap` and wakes any dormant hunter whose `wakeLap ≤ lap`).
#[test]
fn source_keeps_deep_creatures_dormant_and_crossing_a_lap_wakes_them() {
    let mut app = headless_app();
    send(&mut app, DebugCommand::UnlockAll);
    send(&mut app, DebugCommand::GotoZone("source".into()));
    step(&mut app, 0.5);
    step(&mut app, 1.0 / 60.0);
    assert_eq!(mode(&app), GameMode::Zone);

    let entry_lap = {
        let z = app.world().resource::<ZoneRes>();
        let zone = z.get().expect("source loaded");
        let run = zone.source.as_ref().expect("endgame.js S.run");
        assert!(
            !run.dormant.is_empty(),
            "the Source spawns hunters deeper than lap 1"
        );
        for d in &run.dormant {
            let h = zone.hunters.iter().find(|h| h.id == d.id).expect("record");
            assert!(!h.active, "dormant {} is asleep", h.profile.js_name());
        }
        assert!(
            zone.hunters.iter().any(|h| h.active),
            "the shallow ones still walk"
        );
        app.world().resource::<Player>().lap
    };

    let map = game_data(&app)
        .parse_zone("source")
        .expect("zone")
        .expect("parses");
    let (tx, tz, target_lap) = {
        let mut best: Option<(f32, f32, i32)> = None;
        for cz in 0..map.h {
            for cx in 0..map.w {
                if map.cell_type(cx, cz) != CellKind::Floor {
                    continue;
                }
                let lap = grid::lap_of_map(&map, cx, cz);
                if best.is_none_or(|(_, _, b)| lap > b) {
                    let (x, z) = grid::center(&map, cx, cz);
                    best = Some((x, z, lap));
                }
            }
        }
        best.expect("a floor cell in the source")
    };
    assert!(
        target_lap > entry_lap,
        "need a deeper cell than the entry: {target_lap} vs {entry_lap}"
    );

    send(
        &mut app,
        DebugCommand::Teleport {
            x: tx,
            z: tz,
            yaw: Some(0.0),
        },
    );
    step(&mut app, 0.5);

    assert_eq!(log(&app).count("lap"), 1, "{:?}", log(&app).names());
    assert!(
        log(&app).count("hunterWoken") > 0,
        "at least one dormant hunter wakes: {:?}",
        log(&app).names()
    );
}

/// Test 9 — `save.js` — the store is the only channel between two sessions.
#[test]
fn banked_save_survives_into_a_fresh_app_sharing_the_store() {
    let shared = undercroft::resources::MemoryStore::new();
    let mut a = headless_app_with_store(shared.clone());
    send(&mut a, DebugCommand::Begin);
    step(&mut a, 0.5);
    send(&mut a, DebugCommand::SetPoints(9));
    step(&mut a, 0.5);
    assert_eq!(a.world().resource::<SaveRes>().0.points, 9);
    assert!(shared.peek().is_some(), "the store was written");

    let mut b = headless_app_with_store(shared.clone());
    step(&mut b, 1.0 / 60.0); // one tick lets `boot` load the save
    assert_eq!(b.world().resource::<SaveRes>().0.points, 9);
}

/// Test 10 — HANDOFF §8 / PHASE2_SKELETON §5: the sim's only randomness is `RngRes`, seeded the same way
/// in both apps, so the same command script produces the same `EventLog`.
#[test]
fn seeded_apps_produce_identical_event_logs_for_the_same_script() {
    fn run_script(app: &mut App) {
        send(app, DebugCommand::Begin);
        step(app, 1.0 / 60.0);
        send(app, DebugCommand::GotoZone("undercroft".into()));
        step(app, 3.0);
        send(app, DebugCommand::Flash);
        step(app, 3.0);
    }
    let mut a = headless_app_seeded(7);
    let mut b = headless_app_seeded(7);
    run_script(&mut a);
    run_script(&mut b);
    assert_eq!(log(&a).names(), log(&b).names());
}
