//! Headless scenarios for DESIGN.md §5.9: for each of the seven profiles the stimulus that wakes it, the escape
//! that works, the flash reaction, the pool rule and the catch / kill rule — on tiny synthetic maps through the
//! data crate's parser — plus one smoke run per real zone.

use super::*;
use crate::events::SimEvent;
use crate::grid::{bfs_solid, cell_type, center, dist2d, idx, in_bounds, los, to_cell};
use crate::player::{FollowerView, PlayerView};
use crate::pool::Pool;
use crate::rng::SimRng;
use undercroft_data::config::Config;
use undercroft_data::map::parse_map;
use undercroft_data::zone::Facing;
use undercroft_data::{CellKind, GameData, ParsedMap};

const DT: f32 = 1.0 / 60.0;

fn config() -> Config {
    let path = GameData::workspace_data_dir().join("config.ron");
    undercroft_data::load_ron(&path).expect("config.ron loads")
}

/// `hunter.js:recomputePools` (the world lane owns the real one; this is the test-local mirror).
fn recompute_pool(m: &ParsedMap, lanterns: &[(f32, f32)], pool_r: f32) -> Pool {
    let mut pool = Pool::empty(m.len());
    for &(lx, lz) in lanterns {
        let (cx0, cz0) = to_cell(m, lx, lz);
        for dz in -3..=3 {
            for dx in -3..=3 {
                let (cx, cz) = (cx0 + dx, cz0 + dz);
                if !in_bounds(m, cx, cz) {
                    continue;
                }
                let (px, pz) = center(m, cx, cz);
                if dist2d(px, pz, lx, lz) <= pool_r {
                    pool.mask[idx(m, cx, cz)] = 1;
                }
            }
        }
    }
    pool
}

/// A headless zone: map + pool + player + lanterns + records + the event log.
struct World {
    tuning: Tuning,
    pool_r: f32,
    map: ParsedMap,
    pool: Pool,
    player: PlayerView,
    lanterns: Vec<(f32, f32)>,
    items: Vec<(f32, f32)>,
    hunters: Vec<Hunter>,
    events: Vec<SimEvent>,
    rng: SimRng,
    time: f32,
    in_zone: bool,
}

impl World {
    fn from_map(map: ParsedMap) -> World {
        let cfg = config();
        let tuning = Tuning::from_config(&cfg).expect("profiles build");
        let n = map.len();
        World {
            tuning,
            pool_r: cfg.cfg.pool_r,
            map,
            pool: Pool::empty(n),
            player: PlayerView {
                lamp_on: false,
                ..Default::default()
            },
            lanterns: Vec::new(),
            items: Vec::new(),
            hunters: Vec::new(),
            events: Vec::new(),
            rng: SimRng::seed(7),
            time: 0.0,
            in_zone: true,
        }
    }

    fn synthetic(rows: &[&str]) -> World {
        let rows: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
        World::from_map(parse_map(&rows, 0, "test", None, None).expect("synthetic map parses"))
    }

    fn with_ctx<R>(&mut self, f: impl FnOnce(&mut Ctx, &mut Vec<Hunter>) -> R) -> R {
        let env = Env {
            map: &self.map,
            pool: &self.pool,
            player: &self.player,
            lanterns: &self.lanterns,
            items: &self.items,
            in_zone: self.in_zone,
            time: self.time,
        };
        let mut ctx = Ctx::new(&env, &self.tuning, &mut self.rng, &mut self.events);
        f(&mut ctx, &mut self.hunters)
    }

    fn spawn(&mut self, profile: ProfileKind, cx: i32, cz: i32) -> usize {
        self.spawn_opts(profile, cx, cz, SpawnOpts::default())
    }

    fn spawn_opts(&mut self, profile: ProfileKind, cx: i32, cz: i32, opts: SpawnOpts) -> usize {
        self.with_ctx(|ctx, hs| spawn_creature(ctx, hs, profile, cx, cz, opts))
            .expect("spawn cell is walkable")
    }

    /// Keep the derived player flags honest (`inWater`, `inPool`) the way `main.js` does each frame.
    fn sync_player(&mut self) {
        let (cx, cz) = to_cell(&self.map, self.player.x, self.player.z);
        self.player.in_water = cell_type(&self.map, cx, cz) == CellKind::Water;
        let (px, pz) = (self.player.x, self.player.z);
        let r = self.pool_r;
        self.player.in_pool = self
            .lanterns
            .iter()
            .any(|&(lx, lz)| dist2d(lx, lz, px, pz) <= r);
    }

    fn step(&mut self, dt: f32) {
        self.sync_player();
        self.with_ctx(|ctx, hs| update(ctx, hs, dt));
        self.time += dt;
    }

    fn run(&mut self, secs: f32) {
        let n = (secs / DT).round() as usize;
        for _ in 0..n {
            self.step(DT);
        }
    }

    /// Run until the predicate holds (or the time runs out); returns whether it held.
    fn run_until(&mut self, secs: f32, mut pred: impl FnMut(&World) -> bool) -> bool {
        let n = (secs / DT).round() as usize;
        for _ in 0..n {
            if pred(self) {
                return true;
            }
            self.step(DT);
        }
        pred(self)
    }

    fn plant(&mut self, x: f32, z: f32) {
        self.lanterns.push((x, z));
        self.pool = recompute_pool(&self.map, &self.lanterns, self.pool_r);
        self.sync_player();
    }

    fn remove_lantern(&mut self, x: f32, z: f32) {
        self.lanterns.retain(|&(lx, lz)| lx != x || lz != z);
        self.pool = recompute_pool(&self.map, &self.lanterns, self.pool_r);
        self.sync_player();
    }

    fn place(&mut self, x: f32, z: f32) {
        self.player.x = x;
        self.player.z = z;
        self.sync_player();
    }

    /// Face the player at a point (forward = `(-sin yaw, -cos yaw)`).
    fn face(&mut self, x: f32, z: f32) {
        self.player.yaw = (-(x - self.player.x)).atan2(-(z - self.player.z));
    }

    fn flash(&mut self) -> Vec<(u32, FlashResult)> {
        self.with_ctx(|ctx, hs| flash(ctx, hs))
    }

    fn h(&self, i: usize) -> &Hunter {
        &self.hunters[i]
    }

    fn count(&self, name: &str) -> usize {
        self.events.iter().filter(|e| e.name() == name).count()
    }

    fn states(&self, id: u32) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| match e {
                SimEvent::HunterState { id: i, state, .. } if *i == id => Some(state.clone()),
                _ => None,
            })
            .collect()
    }

    fn hunter_dist(&self, i: usize) -> f32 {
        let h = self.h(i);
        dist2d(h.x, h.z, self.player.x, self.player.z)
    }

    fn hunter_in_pool(&self, i: usize) -> bool {
        let h = self.h(i);
        let (cx, cz) = to_cell(&self.map, h.x, h.z);
        in_bounds(&self.map, cx, cz) && self.pool.is_pool(idx(&self.map, cx, cz))
    }
}

fn hall(w: usize, h: usize) -> Vec<String> {
    let mut rows = vec!["#".repeat(w)];
    for _ in 0..h - 2 {
        rows.push(format!("#{}#", ".".repeat(w - 2)));
    }
    rows.push("#".repeat(w));
    rows
}

fn hall_world(w: usize, h: usize) -> World {
    let rows = hall(w, h);
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    World::synthetic(&refs)
}

// ------------------------------------------------------------------ profile table

#[test]
fn profile_table_reads_config() {
    let w = hall_world(10, 6);
    let t = &w.tuning;
    assert_eq!(t.profiles().len(), 7);
    let lw = t.prof(ProfileKind::Lampwight);
    assert_eq!(lw.initial, HState::Drift);
    assert!(!lw.kills);
    assert_eq!(lw.senses.lamp, 24.0);
    assert_eq!(lw.senses.sprint, 0.0);
    assert_eq!(lw.speed_of(HState::Drawn), 2.2);
    let base = t.prof(ProfileKind::Base);
    assert!(!base.creature);
    assert_eq!(base.model, "hunter");
    assert_eq!(base.senses.lamp, 12.0);
    assert!(base.senses.follower);
    assert_eq!(base.speed_of(HState::Chase), 3.6);
    assert_eq!(base.eye_of(HState::Chase), 1.5);
    assert_eq!(base.eye_of(HState::Lit), 0.3);
    assert_eq!(base.water_mul, 0.85);
    let fast = t.prof(ProfileKind::Fast);
    assert_eq!(fast.speed_of(HState::Chase), 4.4);
    assert_eq!(fast.lose_t, 4.0);
    let br = t.prof(ProfileKind::Brute);
    assert_eq!(br.pool_mul, 0.5);
    assert!(br.catch_in_pool);
    assert_eq!(br.leash, Some(14));
    assert_eq!(br.step_of(HState::Chase), 0.45);
    assert_eq!(br.step_of(HState::Wander), 0.6);
    assert!(br.row(HState::Staggered).is_none());
    let dr = t.prof(ProfileKind::Drowner);
    assert_eq!(dr.water_mul, 1.0);
    assert_eq!(dr.initial, HState::Submerged);
    assert_eq!(t.prof(ProfileKind::Warden).territory, Some(8.0));
    assert_eq!(t.prof(ProfileKind::FalseLight).initial, HState::Lit);
    assert_eq!(t.flash_range, 7.0);
    assert_eq!(t.flash_dot, 0.4);
}

#[test]
fn unknown_state_resets_to_initial() {
    let mut w = hall_world(20, 8);
    let i = w.spawn(ProfileKind::Warden, 5, 3);
    w.hunters[i].state = HState::Wander; // the endgame writing a base state onto a creature
    w.step(DT);
    assert_eq!(w.h(i).state, HState::Sentry);
    assert_eq!(w.states(w.h(i).id), vec!["SENTRY"]);
}

// ------------------------------------------------------------------ base / fast

#[test]
fn base_wakes_on_lit_lamp_within_12_with_los() {
    let mut w = hall_world(30, 12);
    let i = w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(15.5, 5.5); // 10 u
    w.run(0.4);
    assert_eq!(w.h(i).state, HState::Chase);
    assert_eq!(w.states(0), vec!["CHASE"]);

    let mut w = hall_world(30, 12);
    let i = w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(20.5, 5.5); // 15 u: out of range
    w.run(2.0);
    assert_eq!(w.h(i).state, HState::Wander);

    // dark and still at 5 u: nothing (still range is 1 u)
    let mut w = hall_world(30, 12);
    let i = w.spawn(ProfileKind::Base, 5, 5);
    w.place(10.5, 5.5);
    w.run(2.0);
    assert_eq!(w.h(i).state, HState::Wander);
    // a wall between: no LOS, no chase even when lit
    let rows = [
        "##############################",
        "#............................#",
        "#............................#",
        "#........#...................#",
        "#........#...................#",
        "#........#...................#",
        "#........#...................#",
        "#........#...................#",
        "#........#...................#",
        "#........#...................#",
        "#........#...................#",
        "##############################",
    ];
    let mut w = World::synthetic(&rows);
    let i = w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(12.5, 5.5);
    w.run(1.0);
    assert_eq!(w.h(i).state, HState::Wander);
}

#[test]
fn base_catches_and_kills() {
    let mut w = hall_world(30, 12);
    w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(9.5, 5.5);
    assert!(w.run_until(6.0, |w| w.count("hunterCatch") > 0));
    let e = w
        .events
        .iter()
        .find(|e| e.name() == "hunterCatch")
        .expect("catch");
    match e {
        SimEvent::HunterCatch {
            hunter_id, target, ..
        } => {
            assert_eq!(*hunter_id, 0);
            assert_eq!(*target, crate::events::CatchTarget::Player);
        }
        _ => unreachable!(),
    }
    // no catch outside a zone
    let mut w = hall_world(30, 12);
    w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(6.0, 5.5);
    w.in_zone = false;
    w.run(2.0);
    assert_eq!(w.count("hunterCatch"), 0);
}

#[test]
fn base_pool_escape_douse_and_wait() {
    let mut w = hall_world(30, 12);
    let i = w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(14.5, 5.5);
    w.run(0.4);
    assert_eq!(w.h(i).state, HState::Chase);
    // plant, step in, douse, wait
    w.plant(14.5, 5.5);
    w.player.lamp_on = false;
    assert!(w.player.in_pool);
    let mut never_in_pool = true;
    for _ in 0..(10.0 / DT) as usize {
        w.step(DT);
        never_in_pool &= !w.hunter_in_pool(i);
    }
    assert!(never_in_pool, "a base hunter never enters a pool cell");
    assert_eq!(w.count("hunterCatch"), 0);
    assert!(
        w.states(0).iter().any(|s| s == "INVESTIGATE"),
        "it lost the target: {:?}",
        w.states(0)
    );
    assert_ne!(w.h(i).state, HState::Chase);
}

#[test]
fn base_flash_staggers_then_dazed_wander() {
    let mut w = hall_world(30, 12);
    let i = w.spawn(ProfileKind::Base, 10, 5);
    w.player.lamp_on = true;
    w.place(15.5, 5.5);
    w.face(10.5, 5.5);
    w.run(0.4);
    assert_eq!(w.h(i).state, HState::Chase);
    let hits = w.flash();
    assert_eq!(hits, vec![(0, FlashResult::Stagger)]);
    assert_eq!(w.h(i).state, HState::Staggered);
    assert_eq!(w.h(i).stagger_t, 3.0);
    let (x, z) = (w.h(i).x, w.h(i).z);
    w.player.lamp_on = false;
    w.run(2.5);
    assert_eq!(w.h(i).state, HState::Staggered);
    assert_eq!((w.h(i).x, w.h(i).z), (x, z), "frozen while staggered");
    w.run(0.8);
    assert_eq!(w.h(i).state, HState::Wander);
    assert!(w.h(i).daze_t > 3.0 && w.h(i).daze_t <= 4.0);
    // a second flash while staggered does nothing
    let mut w = hall_world(30, 12);
    let i = w.spawn(ProfileKind::Base, 10, 5);
    w.place(15.5, 5.5);
    w.face(10.5, 5.5);
    w.flash();
    assert_eq!(w.h(i).state, HState::Staggered);
    assert_eq!(w.flash(), vec![(0, FlashResult::None)]);
}

#[test]
fn fast_chases_at_4_4() {
    let mut w = hall_world(40, 12);
    let i = w.spawn(ProfileKind::Fast, 3, 5);
    w.player.lamp_on = true;
    w.place(14.5, 5.5);
    w.run(0.4);
    assert_eq!(w.h(i).state, HState::Chase);
    let (x0, z0) = (w.h(i).x, w.h(i).z);
    w.run(1.0);
    let moved = dist2d(x0, z0, w.h(i).x, w.h(i).z);
    assert!((moved - 4.4).abs() < 0.1, "fast chase speed sample {moved}");
    assert_eq!(w.h(i).anim.eye_k, 1.5);
}

#[test]
fn base_trapped_by_a_lantern_walks_out() {
    let mut w = hall_world(30, 12);
    let i = w.spawn(ProfileKind::Base, 10, 5);
    w.place(25.5, 5.5);
    w.plant(10.5, 5.5); // on top of it
    assert!(w.hunter_in_pool(i));
    assert!(w.run_until(6.0, |w| !w.hunter_in_pool(i)));
    assert!(w.h(i).x > 12.0 || w.h(i).x < 9.0 || (w.h(i).z - 5.5).abs() > 2.0);
}

// ------------------------------------------------------------------ lampwight

#[test]
fn lampwight_drawn_at_15_where_base_is_not() {
    let mut w = hall_world(30, 12);
    let l = w.spawn(ProfileKind::Lampwight, 5, 5);
    w.player.lamp_on = true;
    w.place(20.5, 5.5); // 15 u > 12, < 24
    w.run(0.4);
    assert_eq!(w.h(l).state, HState::Drawn);
    assert_eq!(w.count("hunterState"), 1);
    // lamp off → DRIFT within 2.2 s, and it walks past a sprinting dark player at 0.5 u without reacting
    w.player.lamp_on = false;
    w.run(2.2);
    assert_eq!(w.h(l).state, HState::Drift);
    let (hx, hz) = (w.h(l).x, w.h(l).z);
    w.place(hx + 0.5, hz);
    w.player.sprinting = true;
    w.player.moving = true;
    w.run(2.0);
    assert_eq!(w.h(l).state, HState::Drift);
    assert_eq!(w.count("hunterCatch"), 0);
    assert_eq!(w.count("lampSnuffed"), 0);
}

#[test]
fn lampwight_snuffs_instead_of_killing() {
    let mut w = hall_world(30, 12);
    let l = w.spawn(ProfileKind::Lampwight, 5, 5);
    w.player.lamp_on = true;
    w.place(9.5, 5.5);
    assert!(w.run_until(8.0, |w| w.count("lampSnuffed") > 0));
    match w
        .events
        .iter()
        .find(|e| e.name() == "lampSnuffed")
        .expect("snuff")
    {
        SimEvent::LampSnuffed {
            hunter_id,
            oil,
            lockout,
            ..
        } => {
            assert_eq!(*hunter_id, 0);
            assert_eq!(*oil, 12.0);
            assert_eq!(*lockout, 2.0);
        }
        _ => unreachable!(),
    }
    assert_eq!(w.h(l).state, HState::Snuff);
    assert!(w.h(l).snuffed);
    assert_eq!(w.count("hunterCatch"), 0);
    // main.js douses the lamp on lampSnuffed; it goes SATED and wanders ≥ 8 cells off, ignoring everything
    w.player.lamp_on = false;
    w.run(1.0);
    assert_eq!(w.h(l).state, HState::Sated);
    w.player.lamp_on = true; // relit early (the lockout is the player's): still ignored while sated
    w.run(4.3);
    assert_eq!(w.h(l).state, HState::Sated);
    w.player.lamp_on = false;
    w.run(1.0);
    assert_eq!(w.h(l).state, HState::Drift, "{:?}", w.states(0));
    assert_eq!(w.count("hunterCatch"), 0);
    assert_eq!(w.count("lampSnuffed"), 1);
    // it returns only if you relight
    w.player.lamp_on = true;
    w.run(0.4);
    assert_eq!(w.h(l).state, HState::Drawn);
}

#[test]
fn lampwight_dark_touch_does_not_snuff() {
    let mut w = hall_world(30, 12);
    let l = w.spawn(ProfileKind::Lampwight, 5, 5);
    w.hunters[l].state = HState::Drawn; // pretend it is already on us
    w.hunters[l].stim = true;
    w.place(5.6, 5.5);
    w.run(1.0);
    assert_eq!(w.count("lampSnuffed"), 0, "only a lit lamp can be snuffed");
}

#[test]
fn lampwight_flash_staggers_3s() {
    let mut w = hall_world(30, 12);
    let l = w.spawn(ProfileKind::Lampwight, 10, 5);
    w.player.lamp_on = true;
    w.place(15.5, 5.5);
    w.face(10.5, 5.5);
    w.run(0.4);
    assert_eq!(w.h(l).state, HState::Drawn);
    assert_eq!(w.flash(), vec![(0, FlashResult::Stagger)]);
    assert_eq!(w.h(l).state, HState::Staggered);
    assert_eq!(w.h(l).stagger_t, 3.0);
    w.run(2.8);
    assert_eq!(w.h(l).state, HState::Staggered);
    w.run(0.4);
    assert_eq!(w.h(l).state, HState::Drift);
    assert!(w.h(l).daze_t > 3.0);
}

#[test]
fn lampwight_never_enters_a_pool() {
    let mut w = hall_world(30, 12);
    let l = w.spawn(ProfileKind::Lampwight, 5, 5);
    w.player.lamp_on = true;
    w.place(14.5, 5.5);
    w.plant(14.5, 5.5);
    let mut never = true;
    for _ in 0..(10.0 / DT) as usize {
        w.step(DT);
        never &= !w.hunter_in_pool(l);
    }
    assert!(never);
    assert_eq!(w.count("lampSnuffed"), 0);
    assert!(w.hunter_dist(l) > 1.5);
}

// ------------------------------------------------------------------ warden

fn warden_world() -> (World, usize) {
    let mut w = hall_world(32, 32);
    let g = w.spawn_opts(
        ProfileKind::Warden,
        15,
        15,
        SpawnOpts {
            facing: Some(Facing::N),
            ..Default::default()
        },
    );
    (w, g)
}

#[test]
fn warden_alerts_in_the_cone_and_chases_inside_its_ground() {
    let (mut w, g) = warden_world();
    assert_eq!(w.h(g).state, HState::Sentry);
    assert_eq!(w.h(g).post, (15.5, 15.5));
    assert_eq!(w.h(g).yaw, 0.0);
    w.player.lamp_on = true;
    w.place(15.5, 9.5); // 6 u north, dead ahead
    assert!(w.run_until(8.0, |w| w.count("wardenAlert") > 0));
    assert_eq!(w.h(g).state, HState::Alert);
    assert!(w.run_until(1.0, |w| w.h(g).state == HState::Chase));
    let post = w.h(g).post;
    let mut inside = true;
    for _ in 0..(1.0 / DT) as usize {
        w.step(DT);
        inside &= dist2d(w.h(g).x, w.h(g).z, post.0, post.1) <= 8.0 + 1e-3;
    }
    assert!(
        inside,
        "its cell stays within 8 u of the post while chasing"
    );
    assert_eq!(w.states(0), vec!["ALERT", "CHASE"]);
    // step out of the territory → RETURN within 0.4 s, then SENTRY at the post
    w.place(15.5, 5.5); // 10 u from the post
    assert!(w.run_until(0.4, |w| w.h(g).state == HState::Return));
    assert_eq!(w.count("wardenReturn"), 1);
    assert!(w.run_until(10.0, |w| w.h(g).state == HState::Sentry));
    assert_eq!((w.h(g).x, w.h(g).z), post);
    assert_eq!(w.count("hunterCatch"), 0);
}

#[test]
fn warden_ignores_dark_and_behind() {
    let (mut w, g) = warden_world();
    w.place(15.5, 9.5); // lamp off in the cone
    w.run(10.0);
    assert_eq!(w.h(g).state, HState::Sentry);
    assert_eq!(w.count("wardenAlert"), 0);
    // lit but behind it (the sweep is ±75°)
    let (mut w, g) = warden_world();
    w.player.lamp_on = true;
    w.place(15.5, 21.5);
    w.run(8.0);
    assert_eq!(w.h(g).state, HState::Sentry);
    // lit within `near` at any angle: it feels the heat
    let (mut w, g) = warden_world();
    w.player.lamp_on = true;
    w.place(15.5, 16.5);
    w.run(0.4);
    assert_eq!(w.h(g).state, HState::Alert);
}

#[test]
fn warden_douse_and_stand_still_6s_returns() {
    let (mut w, g) = warden_world();
    w.player.lamp_on = true;
    w.place(15.5, 9.5);
    assert!(w.run_until(2.0, |w| w.h(g).state == HState::Chase));
    w.player.lamp_on = false;
    w.place(12.5, 9.5); // step aside inside its ground, dark and still (it walks to lastKnown, 3 u away)
    w.run(5.5);
    assert_eq!(w.h(g).state, HState::Chase, "not yet");
    assert!(w.run_until(1.0, |w| w.h(g).state == HState::Return));
    assert_eq!(w.count("hunterCatch"), 0);
}

#[test]
fn warden_flash_flinches_then_resumes_chase() {
    let (mut w, g) = warden_world();
    w.player.lamp_on = true;
    w.place(15.5, 9.5);
    w.face(15.5, 15.5);
    assert!(w.run_until(2.0, |w| w.h(g).state == HState::Chase));
    assert_eq!(w.flash(), vec![(0, FlashResult::Flinch)]);
    assert_eq!(w.h(g).state, HState::Flinch);
    assert_eq!(w.h(g).prev_state, Some(HState::Chase));
    w.run(0.4);
    assert_eq!(w.h(g).state, HState::Flinch);
    w.run(0.4);
    assert_eq!(w.h(g).state, HState::Chase);
    assert!(!w.states(0).iter().any(|s| s == "STAGGERED"));
}

#[test]
fn warden_pools_block_and_chase_catches() {
    let (mut w, g) = warden_world();
    w.player.lamp_on = true;
    w.place(15.5, 9.5);
    w.plant(15.5, 12.5); // a pool between it and the player
    let mut never = true;
    for _ in 0..(6.0 / DT) as usize {
        w.step(DT);
        never &= !w.hunter_in_pool(g);
    }
    assert!(never, "its path never enters pool cells");
    // no pool: caught in CHASE
    let (mut w, g) = warden_world();
    w.player.lamp_on = true;
    w.place(15.5, 10.5);
    assert!(w.run_until(6.0, |w| w.count("hunterCatch") > 0));
    assert_eq!(w.h(g).state, HState::Chase);
    // a dark player brushing past a SENTRY is never caught
    let (mut w, g) = warden_world();
    w.place(15.5, 16.0);
    w.run(3.0);
    assert_eq!(w.h(g).state, HState::Sentry);
    assert_eq!(w.count("hunterCatch"), 0);
}

#[test]
fn warden_sweeps_at_its_post() {
    let (mut w, g) = warden_world();
    w.place(2.5, 30.5);
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for _ in 0..(12.0 / DT) as usize {
        w.step(DT);
        min = min.min(w.h(g).yaw);
        max = max.max(w.h(g).yaw);
    }
    let lim = 75.0f32.to_radians();
    assert!(
        (max - lim).abs() < 0.05 && (min + lim).abs() < 0.05,
        "sweep {min}..{max}"
    );
    assert_eq!(w.h(g).anim.light_k, 1.2);
    assert!(w.h(g).anim.plinth);
}

// ------------------------------------------------------------------ drowner

fn lake_world() -> (World, usize) {
    let rows = [
        "####################",
        "#..................#",
        "#..................#",
        "#.....WWWWWWWW.....#",
        "#.....WWWWWWWW.....#",
        "#.....WWWwWWWW.....#",
        "#.....WWWWWWWW.....#",
        "#.....WWWWWWWW.....#",
        "#..................#",
        "#..................#",
        "####################",
    ];
    let mut w = World::synthetic(&rows);
    assert_eq!(w.map.creatures.len(), 1);
    assert_eq!(cell_type(&w.map, 9, 5), CellKind::Water);
    let d = w.spawn(ProfileKind::Drowner, 9, 5);
    (w, d)
}

fn on_own_water(w: &World, i: usize) -> bool {
    let h = w.h(i);
    let (cx, cz) = to_cell(&w.map, h.x, h.z);
    cell_type(&w.map, cx, cz) == CellKind::Water && h.body[idx(&w.map, cx, cz)] == 1
}

#[test]
fn drowner_surges_at_a_lit_lamp_on_the_shore_but_cannot_leave_the_water() {
    let (mut w, d) = lake_world();
    assert_eq!(w.h(d).body.iter().map(|&b| b as u32).sum::<u32>(), 40);
    assert_eq!(w.h(d).state, HState::Submerged);
    w.player.lamp_on = true;
    w.place(9.5, 8.5); // the shore cell south of the lake, adjacent to water
    assert!(w.run_until(1.0, |w| w.h(d).state == HState::Surfacing));
    assert_eq!(w.count("drownerSurge"), 1);
    assert!(w.run_until(1.0, |w| w.h(d).state == HState::Surge));
    let mut on_water = true;
    for _ in 0..(6.0 / DT) as usize {
        w.step(DT);
        on_water &= on_own_water(&w, d);
    }
    assert!(on_water, "every sample is on a water cell of its body");
    assert_eq!(
        w.count("hunterCatch"),
        0,
        "one cell centre back from the water is safe"
    );
    assert!(w.hunter_dist(d) >= 0.95);
}

#[test]
fn drowner_loses_a_dark_still_player_then_sinks() {
    let (mut w, d) = lake_world();
    w.player.lamp_on = true;
    w.place(9.5, 8.5);
    assert!(w.run_until(2.0, |w| w.h(d).state == HState::Surge));
    w.player.lamp_on = false;
    assert!(w.run_until(4.6, |w| w.h(d).state == HState::Lurk));
    assert!(w.run_until(5.0, |w| w.h(d).state == HState::Submerged));
    assert_eq!(w.count("drownerSink"), 1);
    assert_eq!(w.count("hunterCatch"), 0);
}

#[test]
fn drowner_triggers_on_dark_wading_and_kills() {
    let (mut w, d) = lake_world();
    w.place(7.5, 4.5);
    w.player.moving = true; // wading in the dark, 2 u from it
    assert!(w.player.in_water);
    assert!(w.run_until(1.0, |w| w.h(d).state == HState::Surfacing));
    assert!(w.run_until(6.0, |w| w.count("hunterCatch") > 0));
    assert_eq!(w.h(d).state, HState::Surge);
}

#[test]
fn drowner_flash_forces_a_dive_and_dazes_it() {
    let (mut w, d) = lake_world();
    w.player.lamp_on = true;
    w.place(9.5, 8.5);
    w.face(9.5, 5.5);
    assert!(w.run_until(2.0, |w| w.h(d).state == HState::Surge));
    assert_eq!(w.flash(), vec![(0, FlashResult::Sink)]);
    assert_eq!(w.h(d).state, HState::Sink);
    assert_eq!(w.count("drownerSink"), 1);
    // lit and wading beside it for 4 s: no re-surface while dazed
    w.place(8.5, 7.5);
    w.player.moving = true;
    let mut resurfaced = false;
    for _ in 0..(4.4 / DT) as usize {
        w.step(DT);
        resurfaced |= w.h(d).state == HState::Surfacing;
    }
    assert!(!resurfaced);
    assert_eq!(w.h(d).state, HState::Submerged);
    // submerged: a flash does nothing
    assert_eq!(
        w.with_ctx(|ctx, hs| on_flash(ctx, &mut hs[d])),
        FlashResult::None
    );
}

#[test]
fn drowner_respects_a_pool_island_and_its_body() {
    let (mut w, d) = lake_world();
    w.player.lamp_on = true;
    w.place(12.5, 3.5);
    w.player.moving = true;
    w.plant(12.5, 3.5); // a lantern in the shallows: a safe island
    assert!(w.player.in_pool && w.player.in_water);
    assert!(!w.hunter_in_pool(d));
    assert!(w.run_until(1.0, |w| w.h(d).state == HState::Surfacing));
    let mut never = true;
    for _ in 0..(6.0 / DT) as usize {
        w.step(DT);
        never &= !w.hunter_in_pool(d);
    }
    assert!(never);
    assert_eq!(w.count("hunterCatch"), 0);
    // teleported off its body: the guard snaps it back
    w.hunters[d].x = 2.5;
    w.hunters[d].z = 1.5;
    w.step(0.25);
    assert!(on_own_water(&w, d));
    assert!(dist2d(w.h(d).x, w.h(d).z, 9.5, 5.5) < 1.5);
    // a second, separate pond is not its body
    let rows = [
        "############",
        "#WW......WW#",
        "#Ww......WW#",
        "#WW......WW#",
        "############",
    ];
    let mut w2 = World::synthetic(&rows);
    let d2 = w2.spawn(ProfileKind::Drowner, 1, 2);
    assert_eq!(w2.h(d2).body.iter().map(|&b| b as u32).sum::<u32>(), 6);
    assert_eq!(w2.h(d2).body[idx(&w2.map, 9, 1)], 0);
}

// ------------------------------------------------------------------ false light

fn false_light_world() -> (World, usize) {
    let rows = [
        "##############################",
        "#............................#",
        "#............................#",
        "#.....Y......................#",
        "#............................#",
        "#............................#",
        "#######.###########.##########",
        "#............................#",
        "#.........o..................#",
        "#............................#",
        "#............................#",
        "##############################",
    ];
    let mut w = World::synthetic(&rows);
    w.items = w
        .map
        .items
        .iter()
        .map(|it| center(&w.map, it.cx, it.cz))
        .collect();
    let y = w.spawn(ProfileKind::FalseLight, 6, 3);
    (w, y)
}

#[test]
fn false_light_is_a_proximity_trap_lamp_or_not() {
    let (mut w, y) = false_light_world();
    assert_eq!(w.h(y).state, HState::Lit);
    w.step(DT);
    assert!(w.h(y).anim.light_on && w.h(y).anim.light_k > 1.8);
    assert_eq!(w.h(y).anim.eye_k, 0.0);
    assert!(w.lanterns.is_empty(), "never a pool");
    w.place(10.5, 3.5); // 4 u: nothing, lamp off
    w.run(2.0);
    assert_eq!(w.h(y).state, HState::Lit);
    w.place(9.4, 3.5); // 2.9 u, lamp still off
    assert!(w.run_until(0.4, |w| w.h(y).state == HState::Dark));
    assert_eq!(w.count("falseLightPounce"), 1);
    w.step(DT);
    assert!(!w.h(y).anim.light_on && w.h(y).anim.posed_dark);
    assert!(w.run_until(0.6, |w| w.h(y).state == HState::Pounce));
    assert_eq!(w.h(y).lunge, (9.4, 3.5));
    assert!(w.run_until(1.5, |w| w.count("hunterCatch") > 0));
    // then it retreats out of sight and re-lights ≥ 8 cells away
    w.player.lamp_on = true;
    w.place(9.4, 3.5);
    assert!(w.run_until(3.0, |w| w.h(y).state == HState::Retreat));
    let rest = w.h(y).rest_idx.expect("a rest spot");
    let (rx, rz) = center(&w.map, (rest as i32) % w.map.w, (rest as i32) / w.map.w);
    assert!(!los(&w.map, w.player.x, w.player.z, rx, rz));
    let from_player = bfs_solid(&w.map, 9, 3);
    assert!(from_player.dist[rest] >= 8 && from_player.dist[rest] <= 30);
    assert!(w.run_until(30.0, |w| w.h(y).state == HState::Lit));
    assert!(dist2d(w.h(y).x, w.h(y).z, rx, rz) < 0.05);
}

#[test]
fn false_light_flash_reveals_when_lit_and_aborts_when_dark() {
    let (mut w, y) = false_light_world();
    w.place(11.5, 3.5); // 5 u
    w.face(6.5, 3.5);
    assert_eq!(w.flash(), vec![(0, FlashResult::Reveal)]);
    assert_eq!(w.h(y).state, HState::Revealed);
    assert_eq!(w.count("falseLightReveal"), 1);
    assert!(w.run_until(1.5, |w| w.h(y).state == HState::Retreat));
    assert_eq!(w.count("falseLightPounce"), 0, "no pounce after a reveal");
    assert!(w.h(y).rest_idx.is_some());
    // flash while RETREAT: nothing
    assert_eq!(
        w.with_ctx(|ctx, hs| on_flash(ctx, &mut hs[y])),
        FlashResult::None
    );
    // abort: flash in DARK
    let (mut w, y) = false_light_world();
    w.place(9.4, 3.5);
    w.face(6.5, 3.5);
    assert!(w.run_until(0.4, |w| w.h(y).state == HState::Dark));
    assert_eq!(w.flash(), vec![(0, FlashResult::Abort)]);
    assert_eq!(w.h(y).state, HState::Staggered);
    w.run(3.0);
    assert_eq!(w.count("hunterCatch"), 0);
    assert!(matches!(w.h(y).state, HState::Retreat | HState::Relight));
}

#[test]
fn false_light_lunge_stops_at_a_pool_edge() {
    let (mut w, y) = false_light_world();
    w.place(9.4, 3.5);
    w.plant(8.0, 5.9); // pools cells (7,3) and (8,3), not the player nor the false light
    assert!(!w.player.in_pool);
    assert!(!w.hunter_in_pool(y));
    assert!(w.pool.is_pool(idx(&w.map, 7, 3)) && w.pool.is_pool(idx(&w.map, 8, 3)));
    assert!(w.run_until(1.0, |w| w.h(y).state == HState::Pounce));
    let mut never = true;
    let mut max_x = 0.0f32;
    while w.h(y).state == HState::Pounce {
        w.step(DT);
        never &= !w.hunter_in_pool(y);
        max_x = max_x.max(w.h(y).x);
    }
    assert!(never, "no pool cell entered");
    assert!(max_x < 7.0, "the lunge stopped at the pool edge ({max_x})");
    assert_eq!(w.count("hunterCatch"), 0);
    assert!(w.h(y).lunge_done);
}

// ------------------------------------------------------------------ brute

#[test]
fn brute_sees_a_lamp_at_7_not_10_and_chases_at_walk_speed() {
    let mut w = hall_world(40, 20);
    let b = w.spawn(ProfileKind::Brute, 5, 10);
    let base = w.spawn(ProfileKind::Base, 5, 12);
    w.player.lamp_on = true;
    w.place(15.5, 10.5); // 10 u from the Brute, 10.2 from the base hunter
    w.run(0.8);
    assert_eq!(w.h(b).state, HState::Wander);
    assert_eq!(w.h(base).state, HState::Chase, "base sees 10 u");
    let mut w = hall_world(40, 20);
    let b = w.spawn(ProfileKind::Brute, 5, 10);
    w.player.lamp_on = true;
    w.place(12.5, 10.5); // 7 u
    assert!(w.run_until(0.4, |w| w.h(b).state == HState::Chase));
    let (x0, z0) = (w.h(b).x, w.h(b).z);
    w.run(1.0);
    let moved = dist2d(x0, z0, w.h(b).x, w.h(b).z);
    assert!(
        (moved - 2.6).abs() < 0.1,
        "brute chase speed sample {moved}"
    );
    // creatureStep every 0.45 s in CHASE
    let before = w.count("creatureStep");
    w.run(1.8);
    let steps = w.count("creatureStep") - before;
    assert!((3..=5).contains(&steps), "{steps} steps in 1.8 s");
    assert!(w.events.iter().any(|e| matches!(e, SimEvent::CreatureStep { profile, d, .. } if profile == "brute" && *d < 10.0)));
}

#[test]
fn brute_ignores_the_flash() {
    let mut w = hall_world(40, 20);
    let b = w.spawn(ProfileKind::Brute, 10, 10);
    w.player.lamp_on = true;
    w.place(13.5, 10.5);
    w.face(10.5, 10.5);
    assert!(w.run_until(0.4, |w| w.h(b).state == HState::Chase));
    assert_eq!(w.flash(), vec![(0, FlashResult::None)]);
    assert_eq!(w.h(b).state, HState::Chase);
    assert_eq!(w.count("flashResisted"), 1);
    assert!(!w.states(0).iter().any(|s| s == "STAGGERED"));
}

#[test]
fn brute_wades_into_the_pool_smashes_the_lantern_and_catches_inside() {
    let mut w = hall_world(40, 20);
    let b = w.spawn(ProfileKind::Brute, 16, 10);
    w.player.lamp_on = true;
    w.place(20.5, 10.5);
    w.plant(20.5, 10.5);
    assert!(w.player.in_pool);
    assert!(w.run_until(0.4, |w| w.h(b).state == HState::Chase));
    let mut entered_pool = false;
    let mut pool_speed = None;
    let mut smashed = false;
    for _ in 0..(12.0 / DT) as usize {
        let (x0, z0) = (w.h(b).x, w.h(b).z);
        w.step(DT);
        if !smashed && w.hunter_in_pool(b) {
            entered_pool = true;
            let v = dist2d(x0, z0, w.h(b).x, w.h(b).z) / DT;
            if v > 0.5 {
                pool_speed = Some(v);
            }
        }
        if !smashed && w.count("lanternSmashed") > 0 {
            smashed = true;
            // the shell's lanternRemoved listener: the lantern goes, the pools are recomputed
            assert_eq!(w.count("lanternRemoved"), 1);
            assert!(w.h(b).anim.burst_fired);
            w.remove_lantern(20.5, 10.5);
            assert!(w.lanterns.is_empty());
            assert!(!w.player.in_pool);
        }
        if w.count("hunterCatch") > 0 {
            break;
        }
    }
    assert!(entered_pool, "it wades in");
    let v = pool_speed.expect("moved inside the pool");
    assert!((v - 1.3).abs() < 0.15, "half speed inside the pool: {v}");
    assert!(smashed);
    assert_eq!(w.count("hunterCatch"), 1);
    assert_eq!(w.count("lanternSmashed"), 1, "one smash");
}

#[test]
fn brute_catches_even_in_a_pool_without_a_lantern_to_smash() {
    // a pool that is not a lantern (the shell's pool of another zone state): catchInPool still applies
    let mut w = hall_world(40, 20);
    w.spawn(ProfileKind::Brute, 18, 10);
    w.player.lamp_on = true;
    w.place(20.5, 10.5);
    w.pool = recompute_pool(&w.map, &[(20.5, 10.5)], w.pool_r);
    w.player.in_pool = true;
    let mut caught = false;
    for _ in 0..(6.0 / DT) as usize {
        w.with_ctx(|ctx, hs| update(ctx, hs, DT));
        if w.count("hunterCatch") > 0 {
            caught = true;
            break;
        }
    }
    assert!(caught);
}

#[test]
fn brute_douse_and_walk_breaks_contact() {
    let mut w = hall_world(50, 20);
    let b = w.spawn(ProfileKind::Brute, 10, 10);
    w.player.lamp_on = true;
    w.place(13.5, 10.5);
    assert!(w.run_until(0.4, |w| w.h(b).state == HState::Chase));
    w.player.lamp_on = false;
    w.player.moving = true;
    let mut investigate_at = None;
    for f in 0..(8.0 / DT) as usize {
        let (x, z) = (w.player.x + 2.6 * DT, w.player.z);
        w.place(x, z);
        w.step(DT);
        if investigate_at.is_none() && w.h(b).state == HState::Investigate {
            investigate_at = Some(f as f32 * DT);
        }
    }
    assert_eq!(w.count("hunterCatch"), 0);
    let t = investigate_at.expect("it loses us");
    assert!(t > 3.5 && t < 5.0, "INVESTIGATE after {t} s");
}

#[test]
fn brute_wander_stays_within_its_leash() {
    let mut w = hall_world(60, 20);
    let b = w.spawn(ProfileKind::Brute, 4, 10);
    w.place(56.5, 10.5); // far, dark, still
    let leash = bfs_solid(&w.map, 4, 10).dist;
    let mut max_leash = 0;
    let mut max_x = 0.0f32;
    for _ in 0..(60.0 / DT) as usize {
        w.step(DT);
        let (cx, cz) = to_cell(&w.map, w.h(b).x, w.h(b).z);
        max_leash = max_leash.max(leash[idx(&w.map, cx, cz)]);
        max_x = max_x.max(w.h(b).x);
    }
    assert!(max_leash <= 14, "wandered {max_leash} BFS cells from home");
    assert!(max_x < 20.0);
    assert!(max_leash > 3, "it does wander ({max_leash})");
    assert_eq!(w.h(b).state, HState::Wander);
    assert_eq!(w.count("hunterCatch"), 0);
}

// ------------------------------------------------------------------ flash targeting

#[test]
fn flash_targets_cone_range_and_los() {
    let rows = [
        "########################",
        "#......................#",
        "#......................#",
        "#..........#...........#",
        "#......................#",
        "#......................#",
        "########################",
    ];
    let mut w = World::synthetic(&rows);
    let front = w.spawn(ProfileKind::Base, 6, 1); // 4 u ahead (facing -x)
    let behind = w.spawn(ProfileKind::Base, 14, 1);
    let far = w.spawn(ProfileKind::Base, 2, 1); // 8 u
    w.place(10.5, 1.5);
    w.face(6.5, 1.5);
    let ids = w.with_ctx(|ctx, hs| flash_targets(ctx.env, ctx.tuning, hs));
    assert_eq!(ids, vec![front]);
    assert!(!ids.contains(&behind) && !ids.contains(&far));
    // side of the cone: dot must exceed 0.4
    w.face(10.5, 4.5); // looking south; the hunter at (6.5,1.5) is off to the side
    let ids = w.with_ctx(|ctx, hs| flash_targets(ctx.env, ctx.tuning, hs));
    assert!(ids.is_empty());
    // LOS through the pillar at (11,3): player (12.5,3.5) facing west, hunter at (8.5,3.5)
    let mut w = World::synthetic(&rows);
    let hidden = w.spawn(ProfileKind::Base, 8, 3);
    w.place(12.5, 3.5);
    w.face(8.5, 3.5);
    let ids = w.with_ctx(|ctx, hs| flash_targets(ctx.env, ctx.tuning, hs));
    assert!(!ids.contains(&hidden));
    // an inactive hunter is skipped
    w.hunters[hidden].active = false;
    let hits = w.flash();
    assert!(hits.is_empty());
}

// ------------------------------------------------------------------ follower

fn follower(x: f32, z: f32, lit: bool) -> FollowerView {
    FollowerView {
        id: "wick".into(),
        x,
        z,
        moving: true,
        lit,
        in_pool: false,
    }
}

#[test]
fn creatures_never_target_the_follower_but_hunters_do() {
    let mut w = hall_world(30, 12);
    let l = w.spawn(ProfileKind::Lampwight, 5, 5);
    let g = w.spawn_opts(
        ProfileKind::Warden,
        5,
        8,
        SpawnOpts {
            facing: Some(Facing::E),
            ..Default::default()
        },
    );
    let y = w.spawn(ProfileKind::FalseLight, 5, 2);
    w.place(27.5, 5.5); // dark, far
    w.player.follower = Some(follower(6.5, 5.5, true)); // a lit follower right next to them
    w.run(3.0);
    assert_eq!(w.h(l).state, HState::Drift);
    assert_eq!(w.h(g).state, HState::Sentry);
    assert_eq!(w.h(y).state, HState::Lit);
    assert_eq!(w.count("hunterState"), 0);

    let (mut w, d) = lake_world();
    w.place(17.5, 1.5);
    w.player.follower = Some(follower(9.5, 8.5, true));
    w.run(3.0);
    assert_eq!(w.h(d).state, HState::Submerged);

    // the base hunter chases the nearer stimulated target: the follower
    let mut w = hall_world(30, 12);
    let b = w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(15.5, 5.5);
    w.player.follower = Some(follower(7.5, 5.5, false));
    w.run(0.4);
    assert_eq!(w.h(b).state, HState::Chase);
    assert_eq!(w.h(b).target, Target::Npc);
    assert_eq!(w.h(b).target_id.as_deref(), Some("wick"));
    // a follower standing in a pool is not a target
    let mut w = hall_world(30, 12);
    let b = w.spawn(ProfileKind::Base, 5, 5);
    w.place(27.5, 5.5);
    w.player.follower = Some(FollowerView {
        in_pool: true,
        ..follower(7.5, 5.5, false)
    });
    w.run(1.0);
    assert_eq!(w.h(b).state, HState::Wander);
    // npcCaught: the catcher stands over the spot for 2 s and investigates
    let mut w = hall_world(30, 12);
    let b = w.spawn(ProfileKind::Base, 5, 5);
    w.hunters[b].target = Target::Npc;
    w.with_ctx(|ctx, hs| on_npc_caught(ctx, &mut hs[b]));
    assert_eq!(w.h(b).busy_t, 2.0);
    assert_eq!(w.h(b).target, Target::Player);
    assert_eq!(w.h(b).state, HState::Investigate);
}

// ------------------------------------------------------------------ misc surfaces

#[test]
fn hint_lines() {
    let mut w = hall_world(30, 12);
    let l = w.spawn(ProfileKind::Lampwight, 5, 5);
    w.player.lamp_on = true;
    w.place(12.5, 5.5);
    w.run(0.4);
    assert_eq!(w.h(l).state, HState::Drawn);
    assert_eq!(
        w.with_ctx(|ctx, hs| hint(ctx, hs)),
        "Something is drawn to your light"
    );
    w.player.lamp_lock = 1.2;
    assert_eq!(
        w.with_ctx(|ctx, hs| hint(ctx, hs)),
        "Snuffed — [F] relight in 2 s"
    );
    w.player.lamp_lock = 0.0;
    w.in_zone = false;
    assert_eq!(w.with_ctx(|ctx, hs| hint(ctx, hs)), "");
}

#[test]
fn clear_and_clear_paths_and_spawn_rules() {
    let mut w = hall_world(30, 12);
    let b = w.spawn(ProfileKind::Base, 5, 5);
    w.player.lamp_on = true;
    w.place(12.5, 5.5);
    w.run(0.7);
    assert!(!w.h(b).path.is_empty());
    clear_paths(&mut w.hunters);
    assert!(w.h(b).path.is_empty());
    clear(&mut w.hunters);
    assert!(!w.h(b).active);
    w.run(1.0);
    assert_eq!(w.count("hunterCatch"), 0);
    // a solid or off-grid cell refuses a spawn; spawn_hunter routes creatures to spawn_creature
    assert!(w
        .with_ctx(|ctx, hs| spawn_hunter(ctx, hs, 0, 0, ProfileKind::Base))
        .is_none());
    assert!(w
        .with_ctx(|ctx, hs| spawn_hunter(ctx, hs, 99, 1, ProfileKind::Fast))
        .is_none());
    let i = w
        .with_ctx(|ctx, hs| spawn_hunter(ctx, hs, 8, 8, ProfileKind::Brute))
        .expect("spawned");
    assert_eq!(w.h(i).profile, ProfileKind::Brute);
    assert!(w.h(i).extra && w.h(i).active);
    assert!(!w.h(i).leash.is_empty());
    // out of a zone, a creature spawns inactive; a plain hunter gets idleT 0.5
    w.in_zone = false;
    let j = w.spawn(ProfileKind::Lampwight, 9, 9);
    assert!(!w.h(j).active);
    let k = w
        .with_ctx(|ctx, hs| spawn_hunter(ctx, hs, 9, 8, ProfileKind::Base))
        .expect("spawned");
    assert_eq!(w.h(k).idle_t, 0.5);
}

// ------------------------------------------------------------------ real zones

#[test]
fn every_zone_spawns_its_roster_and_runs_30s() {
    let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
    let expected = [
        ("undercroft", 3),
        ("cistern", 4),
        ("ossuary", 4),
        ("source", 5),
    ];
    for (id, n) in expected {
        let zone = data.zone(id).expect("zone");
        let map = data.parse_zone(id).expect("zone").expect("parses");
        let mut w = World::from_map(map);
        w.hunters = w.with_ctx(|ctx, _| spawn_all(ctx, zone));
        assert_eq!(w.hunters.len(), n, "{id} roster");
        let kinds: Vec<ProfileKind> = w.hunters.iter().map(|h| h.profile).collect();
        for (i, h) in w.hunters.iter().enumerate() {
            assert_eq!(h.id as usize, i);
            assert!(h.active);
            assert_eq!(h.state, w.tuning.prof(h.profile).initial);
        }
        match id {
            "undercroft" => assert_eq!(
                kinds,
                vec![ProfileKind::Base, ProfileKind::Warden, ProfileKind::Brute]
            ),
            "cistern" => assert_eq!(
                kinds,
                vec![
                    ProfileKind::Base,
                    ProfileKind::Base,
                    ProfileKind::Drowner,
                    ProfileKind::Lampwight
                ]
            ),
            "ossuary" => assert_eq!(
                kinds,
                vec![
                    ProfileKind::Fast,
                    ProfileKind::Warden,
                    ProfileKind::FalseLight,
                    ProfileKind::FalseLight
                ]
            ),
            "source" => assert_eq!(
                kinds,
                vec![
                    ProfileKind::Fast,
                    ProfileKind::Fast,
                    ProfileKind::Brute,
                    ProfileKind::FalseLight,
                    ProfileKind::Lampwight
                ]
            ),
            _ => unreachable!(),
        }
        if let Some(g) = w.hunters.iter().find(|h| h.profile == ProfileKind::Warden) {
            assert_eq!(g.opts.facing, zone.creatures[0].facing);
            assert_eq!(
                g.yaw,
                zone.creatures[0].facing.map(|f| f.yaw()).unwrap_or(0.0)
            );
        }
        if let Some(d) = w.hunters.iter().find(|h| h.profile == ProfileKind::Drowner) {
            assert_eq!(d.body.iter().map(|&b| b as u32).sum::<u32>(), 765);
        }
        let entry = zone.anchor("entry").expect("entry anchor");
        let (ex, ez) = center(&w.map, entry[0], entry[1]);
        w.player.lamp_on = true;
        w.place(ex, ez);
        w.items = w
            .map
            .items
            .iter()
            .map(|it| center(&w.map, it.cx, it.cz))
            .collect();
        w.player.moving = true;
        let mut lantern_planted = false;
        for f in 0..900 {
            w.step(1.0 / 30.0);
            if f == 300 && !lantern_planted {
                w.plant(w.player.x, w.player.z);
                lantern_planted = true;
            }
            if f == 600 {
                w.flash();
            }
            for h in &w.hunters {
                assert!(
                    h.x.is_finite() && h.z.is_finite(),
                    "{id}: hunter {} left the grid",
                    h.id
                );
                let (cx, cz) = to_cell(&w.map, h.x, h.z);
                assert!(in_bounds(&w.map, cx, cz), "{id}: hunter {} off-grid", h.id);
                if h.profile == ProfileKind::Drowner {
                    assert!(
                        on_own_water(&w, h.id as usize),
                        "{id}: drowner left its water"
                    );
                }
            }
        }
        assert!(lantern_planted);
        for h in &w.hunters {
            if !h.profile.is_creature() {
                assert!(
                    dist2d(h.x, h.z, h.home.x, h.home.z) > 1.0,
                    "{id}: hunter {} never wandered",
                    h.id
                );
            }
        }
        assert!(
            w.count("creatureStep") > 0 || !kinds.contains(&ProfileKind::Brute) || id == "source"
        );
    }
}
