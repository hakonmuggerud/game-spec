//! World lane: the camera, keyboard/mouse input and 3D rendering of the map. Ports
//! `world.js` (map geometry, gates, shortcuts, water, items, planted lanterns, per-zone atmosphere)
//! and the camera / input / pointer-lock / render-size / `fx.shake` / death-camera parts of
//! `main.js`.
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).
//!
//! # What lives where
//!
//! | module | JS origin |
//! |---|---|
//! | [`palette`] | `maps.js:PALETTES`, the `0xRRGGBB` → colour conversion, the light-unit calibration |
//! | [`camera`] | `main.js` camera + handlamp + `resize` + `fx.shake` + the death camera, and the ⅓-res render rig |
//! | [`input`] | `main.js` pointer lock, `mousemove`, `keydown`, `updatePlayer`'s key block |
//! | [`blocks`] | `world.js:buildBlocks` |
//! | [`water`] | `world.js:buildWater` / `updateWater` |
//! | [`features`] | `world.js:buildStairsMarker`, gates, shortcut doors, the altar |
//! | [`items`] | `world.js:spawnItem` + the bob |
//! | [`lanterns`] | `world.js:spawnLantern` + the flicker and the pool ring |
//! | [`atmosphere`] | `world.js:applyAtmosphere` / `applyHubWarmth`, `endgame.js:applyLapTint` |
//! | [`model`] | the `models.js` factories, rebuilt from `Game.data().models` |
//!
//! The scene holds the hub and the current zone at once, at different `x` (`ParsedMap::ox`), exactly
//! as the JS does; the fog hides whichever one you are not standing in. Nothing toggles by mode.

pub mod atmosphere;
pub mod blocks;
pub mod camera;
pub mod features;
pub mod input;
pub mod items;
pub mod lanterns;
pub mod model;
pub mod palette;
pub mod water;

use bevy::prelude::*;

use crate::resources::{Game, HubMapRes, ZoneRes};
use crate::tick::SimSet;

pub use camera::{Handlamp, RenderRig, ScreenFx, WorldCamera2d, WorldCamera3d, WorldPresenter};
pub use palette::rgb;

/// Everything the current zone owns (`zone.group` of `world.js:loadZone`). Despawned whole when the
/// zone changes; items and lanterns hang off it.
#[derive(Component, Debug, Clone, Copy)]
pub struct ZoneScene;

/// The hub's blocks and its stairwell lamp (`ctx.hub.group`). Built once, never rebuilt.
#[derive(Component, Debug, Clone, Copy)]
pub struct HubScene;

/// Which zone the [`ZoneScene`] was built for, and whether the hub is up.
#[derive(Resource, Debug, Clone, Default)]
struct Built {
    zone: Option<String>,
    hub: bool,
}

/// The world lane's ordering inside `Update`: the scenes are rebuilt before anything diffs against
/// them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldSet {
    /// Zone / hub geometry rebuilt when `ZoneRes.id` or `HubMapRes` changes.
    Scene,
    /// Items, lanterns, doors, atmosphere, lights, animation.
    Sync,
}

/// `world.js:loadZone` / `unloadZone` — the blocks, water and furniture of the current zone.
fn rebuild_zone_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    game: Game,
    zone: Res<ZoneRes>,
    mut built: ResMut<Built>,
    existing: Query<Entity, With<ZoneScene>>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let want = zone.id().map(str::to_string);
    if want == built.zone {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    built.zone = want.clone();
    let (Some(id), Some(z)) = (want, zone.get()) else {
        return;
    };
    let def = asset.data.zones.iter().find(|d| d.id == id);
    let pal = palette::palette_for(&asset.data, &id);
    let bands = def.is_some_and(|d| d.deep_style == undercroft_data::zone::DeepStyle::Bands);

    let root = commands
        .spawn((
            Name::new(format!("zone:{id}")),
            ZoneScene,
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    spawn_blocks(
        &mut commands,
        &mut meshes,
        &mut materials,
        root,
        &z.map,
        pal,
        bands,
        false,
    );
    features::spawn_zone_features(
        &mut commands,
        &mut meshes,
        &mut materials,
        &game,
        root,
        &z.map,
        &z.doors,
    );
    if let Some(mesh) = water::build_water(&z.map) {
        let sheet = commands
            .spawn((
                Name::new("water"),
                water::WaterSheet {
                    glow: pal.water_glow,
                },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: {
                        let c = palette::rgb(pal.water_surface).to_srgba();
                        Color::srgba(c.red, c.green, c.blue, 0.72)
                    },
                    emissive: palette::emissive(pal.water_glow, 0.25),
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    perceptual_roughness: 1.0,
                    reflectance: 0.0,
                    ..default()
                })),
                Transform::IDENTITY,
            ))
            .id();
        commands.entity(root).add_child(sheet);
    }
    info!("world: built zone scene for {id}");
}

/// `world.js:buildHub` — the hub blocks and the stairwell's landing lamp. Props, sconces, buildings
/// and NPCs are the hub lane's.
fn rebuild_hub_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    game: Game,
    hub: Res<HubMapRes>,
    mut built: ResMut<Built>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let Some(h) = hub.0.as_ref() else {
        return;
    };
    if built.hub {
        return;
    }
    built.hub = true;
    let pal = palette::palette_for(&asset.data, "hub");
    let root = commands
        .spawn((
            Name::new("hub"),
            HubScene,
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    spawn_blocks(
        &mut commands,
        &mut meshes,
        &mut materials,
        root,
        &h.map,
        pal,
        false,
        true,
    );
    features::spawn_hub_stairs(&mut commands, root, &h.map);
    info!("world: built hub scene");
}

/// One child mesh per non-empty block kind, all sharing the vertex-colour material.
#[allow(clippy::too_many_arguments)]
fn spawn_blocks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
    map: &undercroft_data::ParsedMap,
    pal: &undercroft_data::zone::Palette,
    bands: bool,
    hole: bool,
) {
    let material = materials.add(palette::lambert(Color::WHITE));
    for (kind, mesh) in blocks::build_blocks(map, pal, bands, hole) {
        let e = commands
            .spawn((
                Name::new(format!("blocks:{}", kind.name())),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
            ))
            .id();
        commands.entity(root).add_child(e);
    }
}

/// The world lane's systems.
///
/// The lane needs a renderer outright — it owns the cameras, `ClearColor` and the offscreen render
/// target, none of which a run condition can stand in for — so it drops out at build time on an
/// `App` without one ([`crate::has_renderer`]; `headless.rs`'s asset-loader test builds exactly
/// such an app).
pub fn plugin(app: &mut App) {
    if !crate::has_renderer(app) {
        debug!("world: no renderer in this App, the lane stays out");
        return;
    }
    app.init_resource::<Built>()
        .init_resource::<atmosphere::Atmosphere>()
        .init_resource::<camera::ScreenFx>()
        .init_resource::<input::LookAccum>()
        .init_resource::<input::PointerLock>()
        .add_systems(Startup, camera::spawn_rig)
        .configure_sets(Update, (WorldSet::Scene, WorldSet::Sync).chain())
        .add_systems(
            Update,
            (rebuild_zone_scene, rebuild_hub_scene).in_set(WorldSet::Scene),
        )
        .add_systems(
            Update,
            (
                camera::on_resize,
                camera::on_sim_events,
                camera::update_lamp_light,
                atmosphere::update_atmosphere,
                features::sync_doors,
                items::sync_items,
                items::animate_items,
                lanterns::sync_lanterns,
                lanterns::flicker_lanterns,
                water::animate_water,
                input::accumulate_look,
                input::forward_key_presses,
                input::pointer_lock,
            )
                .in_set(WorldSet::Sync),
        )
        .add_systems(FixedUpdate, input::write_move_intent.in_set(SimSet::Input))
        .add_systems(
            PostUpdate,
            // before the propagation step, so the camera's own `GlobalTransform` and the
            // handlamp's (it is a child) are up to date in the same frame the extraction runs
            camera::follow_player.before(bevy::transform::TransformSystems::Propagate),
        );
}
