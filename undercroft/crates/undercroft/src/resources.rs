//! Every shared resource the lanes read and `run.rs` / `player.rs` write. The field types come from
//! `undercroft_sim` and `undercroft_data`; nothing here re-implements game logic. The JS origin of
//! each item is in its doc comment (`main.js:53 ctx.state`, `ctx.player`, `ctx.zone`, …).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use undercroft_data::{GameData, ItemKind, ParsedMap};
use undercroft_sim::collision::BlockMask;
use undercroft_sim::contracts::QuestSpawn;
use undercroft_sim::creature::{Hunter, Tuning};
use undercroft_sim::economy::{ExtraSpawn, HubState, LampState, SourceRun};
use undercroft_sim::player::{Carried, FollowerView};
use undercroft_sim::pool::Lantern;
use undercroft_sim::save::SaveData;
use undercroft_sim::world::ZoneDoors;
use undercroft_sim::{follower, PlayerView, Pool, SimRng};

use crate::assets::{GameDataAsset, GameDataHandle};

/* ============================================================
Game data access
============================================================ */

/// The loaded data behind a handle, or `None` while `GameMode::Loading`.
pub fn try_data<'a>(
    assets: &'a Assets<GameDataAsset>,
    h: &GameDataHandle,
) -> Option<&'a GameDataAsset> {
    assets.get(&h.0)
}

/// `MAPS`/`CFG` — the loaded game data. Panics outside `GameMode::Loading` would mean the asset was
/// dropped, which cannot happen while [`GameDataHandle`] is alive.
pub fn data<'a>(assets: &'a Assets<GameDataAsset>, h: &GameDataHandle) -> &'a GameData {
    &try_data(assets, h)
        .expect("game data asset is loaded outside GameMode::Loading")
        .data
}

/// Read-only access to the game data for any system: `fn sys(game: Game) { game.config().cfg.walk }`.
#[derive(SystemParam)]
pub struct Game<'w> {
    assets: Res<'w, Assets<GameDataAsset>>,
    handle: Res<'w, GameDataHandle>,
}

impl Game<'_> {
    /// `None` while the asset is still loading.
    pub fn get(&self) -> Option<&GameDataAsset> {
        try_data(&self.assets, &self.handle)
    }

    /// The whole asset; only call after `GameMode::Loading`.
    pub fn asset(&self) -> &GameDataAsset {
        self.get().expect("game data asset is loaded")
    }

    /// `MAPS` + the tables (`config.ron`, `zones.ron`, …).
    pub fn data(&self) -> &GameData {
        &self.asset().data
    }

    /// `CFG` — `config.js`'s top-level tunables.
    pub fn config(&self) -> &undercroft_data::Config {
        &self.asset().data.config
    }

    /// The creature tuning derived from `config.ron` (`creature::tuning`), rebuilt on hot reload.
    pub fn tuning(&self) -> &Tuning {
        &self.asset().tuning
    }
}

/* ============================================================
Save, hub, lamp
============================================================ */

/// `ctx.save` — the persisted profile (`save.js`).
#[derive(Resource, Debug, Clone, Default)]
pub struct SaveRes(pub SaveData);

/// `hub.js` module state that is not in the save: the current flame tier and whether the blessing is
/// charged for this descent.
#[derive(Resource, Debug, Clone, Default)]
pub struct HubRes(pub HubState);

/// `ctx.player`'s lamp half (`oil`, `lampOn`, `flashCd`, `lanternCd`, `flashT`, `lampLock`).
#[derive(Resource, Debug, Clone, Default)]
pub struct LampRes(pub LampState);

/// The sim's only randomness (HANDOFF §8). Seeded from entropy natively and on the web; tests use
/// [`crate::headless::headless_app_seeded`].
#[derive(Resource, Debug)]
pub struct RngRes(pub SimRng);

impl Default for RngRes {
    fn default() -> Self {
        RngRes(SimRng::from_entropy())
    }
}

/* ============================================================
World: zone, hub map, items, NPCs
============================================================ */

/// One item lying in the world (`ctx.items[i]`, `main.js:spawnAt`): the map's `oil`/`relic`/`rich`
/// spawns plus contract quest items and death bundles.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldItem {
    pub kind: ItemKind,
    pub x: f32,
    pub z: f32,
    /// `it.contents` — only a death bundle carries loot.
    pub contents: Option<Carried>,
    /// `it.questId` — the contract a `quest` item belongs to.
    pub quest_id: Option<String>,
    /// `it.label` — the contract's item name, shown on the interact prompt.
    pub label: Option<String>,
}

impl WorldItem {
    /// A plain map spawn (`spawnAt(kind, cx, cz)`).
    pub fn new(kind: ItemKind, x: f32, z: f32) -> WorldItem {
        WorldItem {
            kind,
            x,
            z,
            contents: None,
            quest_id: None,
            label: None,
        }
    }
}

/// `ctx.zone` plus the per-run lists `main.js` kept beside it (`ctx.lanterns`, `ctx.items`,
/// `ctx.hunters`). `hunters` is the authoritative creature list: the creatures lane spawns one
/// entity per record, indexed by `Hunter.id`, and mirrors it — it never owns creature state.
#[derive(Debug, Clone)]
pub struct Zone {
    /// `ctx.zone.id`.
    pub id: String,
    /// `ctx.zone.map`.
    pub map: ParsedMap,
    /// Gate/shortcut open flags (`world::apply_saved_openings`).
    pub doors: ZoneDoors,
    /// Lantern light pools (`pool::recompute`).
    pub pool: Pool,
    /// `ctx.lanterns`.
    pub lanterns: Vec<Lantern>,
    /// `ctx.hunters` — hunters and creatures alike.
    pub hunters: Vec<Hunter>,
    /// `ctx.items`.
    pub items: Vec<WorldItem>,
    /// `endgame.js S.run`, only in the Source.
    pub source: Option<SourceRun>,
}

/// The loaded zone, or `None` in the title screen before the first `loadZoneInactive`.
#[derive(Resource, Debug, Default)]
pub struct ZoneRes(pub Option<Zone>);

impl ZoneRes {
    /// The loaded zone.
    pub fn get(&self) -> Option<&Zone> {
        self.0.as_ref()
    }

    /// The loaded zone, mutably.
    pub fn get_mut(&mut self) -> Option<&mut Zone> {
        self.0.as_mut()
    }

    /// `ctx.zone.id`.
    pub fn id(&self) -> Option<&str> {
        self.0.as_ref().map(|z| z.id.as_str())
    }
}

/// `ctx.hub.map` — the parsed hub map and its collision mask. The hub lane fills prop footprints
/// into `mask` with `BlockMask::mark_box_cells`.
#[derive(Debug, Clone)]
pub struct HubMap {
    pub map: ParsedMap,
    pub mask: BlockMask,
}

/// The hub map, built from `GameData::parse_hub` when the game starts.
#[derive(Resource, Debug, Default)]
pub struct HubMapRes(pub Option<HubMap>);

/// `ctx.npcs` — the follower/resident state machine (`npc.js`).
#[derive(Resource, Debug, Default)]
pub struct Npcs(pub follower::Npcs);

/* ============================================================
Player
============================================================ */

/// `ctx.player` minus the lamp fields, which live in [`LampRes`] so there is one owner for `oil`,
/// `lampOn`, `flashT` and `lampLock` (the JS keeps them on the same object; splitting them avoids
/// two copies drifting apart). The world lane owns the camera entity and copies from here.
#[derive(Resource, Debug, Clone, Default)]
pub struct Player {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub sprinting: bool,
    pub moving: bool,
    pub in_water: bool,
    pub in_pool: bool,
    pub on_deep: bool,
    /// `player.lap` — the Source band the player stands in, 0 elsewhere.
    pub lap: i32,
    pub carried: Carried,
    /// `state.dyingT` — seconds left of the death camera.
    pub dying_t: f32,
}

impl Player {
    /// `main.js:startRun` — drop the player at the zone entry.
    pub fn reset_at(&mut self, x: f32, z: f32, yaw: f32) {
        self.x = x;
        self.z = z;
        self.yaw = yaw;
        self.pitch = 0.0;
        self.sprinting = false;
        self.moving = false;
        self.in_water = false;
        self.in_pool = false;
        self.on_deep = false;
        self.lap = 0;
        self.dying_t = 0.0;
    }

    /// The read-only snapshot every sim call takes (`player.js:PlayerView`). `lamp_reach` is the
    /// handlamp light distance for this zone (`economy::zone_mul`).
    pub fn view(
        &self,
        lamp: &LampState,
        lamp_reach: f32,
        follower: Option<FollowerView>,
    ) -> PlayerView {
        PlayerView {
            x: self.x,
            z: self.z,
            yaw: self.yaw,
            lamp_on: lamp.lamp_on,
            flash_t: lamp.flash_t,
            lamp_lock: lamp.lamp_lock,
            sprinting: self.sprinting,
            moving: self.moving,
            in_water: self.in_water,
            in_pool: self.in_pool,
            on_deep: self.on_deep,
            lap: self.lap,
            oil: lamp.oil,
            lamp_reach,
            carried: self.carried,
            follower,
        }
    }
}

/// What the input side asks for this tick (`main.js:updatePlayer` reads `keys` and the mouse
/// deltas). The world lane writes it in [`crate::tick::SimSet::Input`]; `player.rs` consumes it in
/// [`crate::tick::SimSet::Player`] and clears the look deltas.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveIntent {
    /// −1 back … +1 forward.
    pub forward: f32,
    /// −1 left … +1 right.
    pub strafe: f32,
    pub sprint: bool,
    /// Mouse delta this tick, radians.
    pub look_dx: f32,
    pub look_dy: f32,
}

/// The `PlayerView` built once per fixed tick and passed to every sim call (`run.rs` reads it).
#[derive(Resource, Debug, Clone, Default)]
pub struct PlayerViewRes(pub PlayerView);

/* ============================================================
Fade / transitions
============================================================ */

/// `main.js:209 fade.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FadeMode {
    /// `fade.mode === null` — no fade running.
    #[default]
    Idle,
    /// Fading to black; the pending transition fires at `a >= 1`.
    Out,
    /// Fading back in.
    In,
}

/// The `transition(cb)` callbacks of `main.js`, as data (a closure cannot live in a resource that
/// tests and the UI lane inspect).
#[derive(Debug, Clone, PartialEq)]
pub enum PendingTransition {
    /// `descend()` — load the zone if needed and `startRun()`.
    StartRun { zone_id: String },
    /// `bank()` — bank the loot, `zoneExit`, `enterHub()`.
    Bank,
    /// `returnToHub()` / `rideUp()` — `zoneExit` then `enterHub()`.
    EnterHub,
    /// The death screen (`showDeathScreen`).
    Dead,
    /// `toMainMenu()`.
    Title,
    /// `endgame.js:continueToHub` — `zoneExit`, `enterHub()`, `endingContinue`.
    EndingContinue { id: String },
}

/// `main.js:202 const fade` — the black overlay. The UI lane draws `a`; `run.rs` drives it.
#[derive(Resource, Debug, Clone, Default)]
pub struct Fade {
    /// Opacity 0..1.
    pub a: f32,
    pub mode: FadeMode,
    /// `fade.cb`.
    pub pending: Option<PendingTransition>,
}

impl Fade {
    /// `main.js:209 transition(cb)` — starts a fade-out; refused while one is already running.
    pub fn start(&mut self, next: PendingTransition) -> bool {
        if self.mode == FadeMode::Out {
            return false;
        }
        self.mode = FadeMode::Out;
        self.pending = Some(next);
        true
    }

    /// `actions.gotoZone` — cancel the pending callback and fade straight back in.
    pub fn cut_in(&mut self, at_least: f32) {
        self.pending = None;
        self.mode = FadeMode::In;
        self.a = self.a.max(at_least);
    }
}

/* ============================================================
Spawn requests the sim returns as data (HANDOFF §6)
============================================================ */

/// Side effects the sim hands back as spawn requests; `run.rs` drains them each tick.
#[derive(Resource, Debug, Default)]
pub struct Spawns {
    /// `contracts::quests_to_spawn` — quest items to place in the loaded zone.
    pub pending_quest_items: Vec<QuestSpawn>,
    /// `economy::DeathOutcome.bundle` — the death bundle to drop, at `(x, z)`.
    pub pending_bundles: Vec<(Carried, f32, f32)>,
    /// `economy::ExtraSpawn` — extra Source hunters (`endgame.js`).
    pub pending_extra_hunters: Vec<ExtraSpawn>,
}

impl Spawns {
    /// Nothing pending.
    pub fn is_empty(&self) -> bool {
        self.pending_quest_items.is_empty()
            && self.pending_bundles.is_empty()
            && self.pending_extra_hunters.is_empty()
    }
}

/* ============================================================
Persistence
============================================================ */

/// Where the save JSON lives. `save.js` used `localStorage`; native builds use a file and headless
/// tests an in-memory cell.
pub trait SaveStore {
    /// The stored JSON, or `None` when there is no save.
    fn load(&self) -> Option<String>;
    /// Overwrite the stored JSON.
    fn store(&self, json: &str);
}

/// The active store.
#[derive(Resource)]
pub struct SaveStoreRes(pub Box<dyn SaveStore + Send + Sync>);

impl SaveStoreRes {
    /// Wrap any store.
    pub fn new(store: impl SaveStore + Send + Sync + 'static) -> SaveStoreRes {
        SaveStoreRes(Box::new(store))
    }
}

impl Default for SaveStoreRes {
    fn default() -> Self {
        SaveStoreRes::new(MemoryStore::default())
    }
}

/// An in-memory store shared by clones of the handle — the headless harness's `localStorage`, and
/// the way `tests/skeleton.rs` carries a save between two apps.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore(pub Arc<Mutex<Option<String>>>);

impl MemoryStore {
    /// A fresh empty store.
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    /// The current contents, without going through the trait.
    pub fn peek(&self) -> Option<String> {
        self.0.lock().expect("save store mutex").clone()
    }
}

impl SaveStore for MemoryStore {
    fn load(&self) -> Option<String> {
        self.0.lock().expect("save store mutex").clone()
    }

    fn store(&self, json: &str) {
        *self.0.lock().expect("save store mutex") = Some(json.to_string());
    }
}

/// Native persistence: one JSON file. `UNDERCROFT_SAVE` overrides the path (tests, DESIGN §2).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct FileStore(pub std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl Default for FileStore {
    fn default() -> Self {
        FileStore(
            std::env::var("UNDERCROFT_SAVE")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("./undercroft-save.json")),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SaveStore for FileStore {
    fn load(&self) -> Option<String> {
        match std::fs::read_to_string(&self.0) {
            Ok(s) => Some(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                error!("save: cannot read {}: {e}", self.0.display());
                None
            }
        }
    }

    fn store(&self, json: &str) {
        if let Err(e) = std::fs::write(&self.0, json) {
            error!("save: cannot write {}: {e}", self.0.display());
        }
    }
}

/// Web persistence: `localStorage`, exactly what `save.js` used.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Default)]
pub struct LocalStorageStore;

#[cfg(target_arch = "wasm32")]
impl LocalStorageStore {
    fn storage() -> Option<web_sys::Storage> {
        web_sys::window().and_then(|w| w.local_storage().ok().flatten())
    }
}

#[cfg(target_arch = "wasm32")]
impl SaveStore for LocalStorageStore {
    fn load(&self) -> Option<String> {
        Self::storage().and_then(|s| s.get_item(undercroft_sim::save::SAVE_KEY).ok().flatten())
    }

    fn store(&self, json: &str) {
        if let Some(s) = Self::storage() {
            if s.set_item(undercroft_sim::save::SAVE_KEY, json).is_err() {
                error!("save: localStorage is not writable");
            }
        }
    }
}

/// The platform's default store: a file natively, `localStorage` on the web.
pub fn default_store() -> SaveStoreRes {
    #[cfg(target_arch = "wasm32")]
    {
        SaveStoreRes::new(LocalStorageStore)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        SaveStoreRes::new(FileStore::default())
    }
}

/// Toasts waiting to be shown; the UI lane drains them. `main.js` called `ui.toast` directly, but
/// the sim returns `SimEvent::Toast` and the UI lane may not exist yet.
#[derive(Resource, Debug, Default)]
pub struct Toasts(pub VecDeque<String>);

/// Insert every shared resource with its default value.
pub fn plugin(app: &mut App) {
    app.init_resource::<SaveRes>()
        .init_resource::<HubRes>()
        .init_resource::<LampRes>()
        .init_resource::<ZoneRes>()
        .init_resource::<HubMapRes>()
        .init_resource::<Npcs>()
        .init_resource::<Player>()
        .init_resource::<MoveIntent>()
        .init_resource::<PlayerViewRes>()
        .init_resource::<Fade>()
        .init_resource::<Spawns>()
        .init_resource::<Toasts>();
    if !app.world().contains_resource::<RngRes>() {
        app.init_resource::<RngRes>();
    }
    if !app.world().contains_resource::<SaveStoreRes>() {
        app.insert_resource(default_store());
    }
}
