//! Creatures lane: the visual side of hunters and other creatures. Ports the rendering half of
//! `hunter.js` (mesh, eye glow, animation hooks) and `models.js` (the per-profile meshes). The sim
//! side (`undercroft_sim::creature`) is authoritative; this lane only mirrors `Zone.hunters` onto
//! entities, indexed by `Hunter.id`, and never owns creature state.
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).
//!
//! Layout:
//!
//! - [`model`] — `models.js:build`: a `ModelDef` becomes a root → parts → boxes entity hierarchy.
//! - [`sync`] — `hunter.js:syncMesh` plus every per-profile `anim` hook, reading `Hunter::anim`.
//! - [`burst`] — the Brute's lantern smash: `models.js:emberBurst` / `lanternDebris`.
//! - this file — the roster: one entity per `Zone.hunters[i]`, spawned and despawned by diffing
//!   `ZoneRes` every frame (`hunter.js:makeHunter` / `disposeMesh` / `spawnAll`).

pub mod burst;
pub mod model;
pub mod sync;

use std::collections::HashMap;

use bevy::prelude::*;
use undercroft_data::ModelDef;
use undercroft_sim::creature::{Hunter, ProfileKind};

use crate::resources::{Game, ZoneRes};
use model::{fallback_def, spawn_model, ModelEntities};

/// three.js (r155+) light intensity is in candela; Bevy's `PointLight`/`SpotLight` intensity is
/// luminous power in lumens, `lm = cd · 4π`. A camera `Exposure { ev100: -0.263 }`
/// (`exp2(-ev100) / 1.2 == 1`) reproduces three's unexposed output — the world lane owns the
/// camera, so the two lanes must agree on this constant; see the lane report.
pub const LUMENS_PER_CANDELA: f32 = 4.0 * std::f32::consts::PI;

/// The entity mirroring one `Zone.hunters[i]` (`hunter.js`'s `h.group`, named `hunter:<id>`).
#[derive(Component, Debug)]
pub struct Creature {
    /// `Hunter::id` — the key `ZoneRes.hunters` is diffed by.
    pub id: u32,
    /// The profile the model was built for; a record whose profile changed is rebuilt
    /// (`spawnAll`: `if (h.profile !== sp.profile) { disposeMesh(h); h = makeHunter(...) }`).
    pub profile: ProfileKind,
    /// Parts, named boxes and materials from [`model::spawn_model`].
    pub model: ModelEntities,
    /// The Warden's cone `SpotLight` on `conePivot` (`hunter.js:wardenBuild`).
    pub spot: Option<Entity>,
    /// The false light's `PointLight` in its glass (`hunter.js:falseLightBuild`).
    pub point: Option<Entity>,
    /// A burst is playing for this creature, so `Anim::burst_fired` staying set across several
    /// rendered frames of one fixed tick cannot fire it twice.
    pub burst_active: bool,
}

/// `ctx.hunters` → entities, by `Hunter::id`. The zone owns the records; this is only the mirror.
#[derive(Resource, Debug, Default)]
pub struct HunterEntities {
    /// `Hunter::id` → root entity.
    pub map: HashMap<u32, Entity>,
    /// The zone the entities belong to; a change means `ZoneRes` replaced the whole vector
    /// (`zoneExit` + `zoneEnter` → `clear()` + `spawnAll`), so everything is rebuilt.
    zone: Option<String>,
}

impl HunterEntities {
    /// The entity mirroring that record, if it is spawned.
    pub fn get(&self, id: u32) -> Option<Entity> {
        self.map.get(&id).copied()
    }
}

/// `hunter.js:buildMesh` — the `models.js` factory a profile is drawn with. `base` and `fast` share
/// the `hunter` factory (`prof.model`); the exporter recorded the `scaleY = 1.15` variant `fast`
/// gets as its own entry, `hunterFast`. Everything else is the profile's own name
/// (`ProfileKind::js_name`).
pub fn model_key(profile: ProfileKind) -> &'static str {
    match profile {
        ProfileKind::Base => "hunter",
        ProfileKind::Fast => "hunterFast",
        other => other.js_name(),
    }
}

/// `hunter.js:buildProfiles` — `fallback` / `fallbackSize`, the colour and size of the box a
/// profile falls back to when `models.js` has no factory for it (default size `[0.5, 1.8]`).
pub fn fallback_look(profile: ProfileKind) -> (u32, (f32, f32)) {
    match profile {
        ProfileKind::Base | ProfileKind::Fast => (0x08080a, (0.5, 1.8)),
        ProfileKind::Lampwight => (0x6a7488, (0.4, 2.2)),
        ProfileKind::Warden => (0x3a2f22, (0.7, 2.5)),
        ProfileKind::Drowner => (0x06080a, (0.6, 0.55)),
        ProfileKind::FalseLight => (0x3a2a1a, (0.3, 1.6)),
        ProfileKind::Brute => (0x141210, (1.3, 2.6)),
    }
}

/// One frame of `spawnAll` / `disposeMesh`: spawn an entity for every record that has none, rebuild
/// one whose profile changed, and despawn the entities of records that are gone (or of the whole
/// zone when `ZoneRes` swapped it — `hunter.js` cleared the list on `zoneExit` / `hubEnter`).
#[allow(clippy::too_many_arguments)]
pub fn sync_roster(
    mut commands: Commands,
    zone: Res<ZoneRes>,
    game: Game,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roster: ResMut<HunterEntities>,
    creatures: Query<&Creature>,
    bursts: Query<Entity, With<burst::Burst>>,
) {
    let Some(asset) = game.get() else {
        return; // still GameMode::Loading
    };
    let zone_id = zone.id().map(str::to_string);
    if roster.zone != zone_id {
        for (_, e) in roster.map.drain() {
            commands.entity(e).despawn();
        }
        // one-shot bursts live in the scene, not under the group: a smash on the last frame of a
        // run must not follow the player into the hub (`hunter.js:clear`)
        for e in &bursts {
            commands.entity(e).despawn();
        }
        roster.zone = zone_id;
    }
    let Some(hunters) = zone.get().map(|z| &z.hunters) else {
        return;
    };

    for h in hunters {
        match roster.get(h.id) {
            Some(e) if creatures.get(e).map(|c| c.profile) == Ok(h.profile) => continue,
            Some(e) => commands.entity(e).despawn(),
            None => {}
        }
        let entity = spawn_creature(
            &mut commands,
            &mut meshes,
            &mut materials,
            &asset.data.models,
            &asset.data.config,
            h,
            asset.tuning.prof(h.profile).eye_color,
        );
        roster.map.insert(h.id, entity);
    }
    roster.map.retain(|id, entity| {
        if hunters.iter().any(|h| h.id == *id) {
            return true;
        }
        commands.entity(*entity).despawn();
        false
    });
}

/// `hunter.js:makeHunter` + `buildMesh`: the group, its model and the profile's `onBuild` light.
fn spawn_creature(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    models: &undercroft_data::ModelTable,
    config: &undercroft_data::Config,
    h: &Hunter,
    eye_color: u32,
) -> Entity {
    let key = model_key(h.profile);
    let fallback: Option<ModelDef> = models.get(key).is_none().then(|| {
        let (color, size) = fallback_look(h.profile);
        warn!("creatures: models.{key} is missing, using the fallback box");
        fallback_def(key, color, size, eye_color)
    });
    let def = fallback
        .as_ref()
        .or_else(|| models.get(key))
        .expect("either the recorded model or the fallback");

    // `g.visible = false` until the first `syncMesh` places it (`buildMesh`).
    let root = commands
        .spawn((
            Name::new(format!("hunter:{}", h.id)),
            Transform::from_xyz(h.x, h.y, h.z),
            Visibility::Hidden,
        ))
        .id();
    let model = spawn_model(commands, meshes, materials, def, root);

    let mut spot = None;
    let mut point = None;
    match h.profile {
        // `wardenBuild` — the cone, on the visor pivot (`conePivot` sits at the recorded `spotY`).
        ProfileKind::Warden => {
            let wd = &config.creature.warden;
            let parent = model.part("conePivot").map_or(root, |p| p.entity);
            let deg = std::f32::consts::PI / 180.0;
            spot = Some(
                commands
                    .spawn((
                        Name::new("wardenLight"),
                        SpotLight {
                            color: model::rgb_u32(wd.light.color),
                            intensity: 0.0,
                            range: wd.light.dist,
                            outer_angle: wd.light.angle * deg,
                            // three's `penumbra` softens inwards from the cone edge.
                            inner_angle: wd.light.angle * deg * (1.0 - wd.light.penumbra),
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        // the JS target was `(0, 1.0, −6)` in group space, i.e. the cone points
                        // forward (−Z) and a little down from the visor at y 2.28
                        Transform::from_rotation(Quat::from_rotation_x(-WARDEN_CONE_PITCH)),
                        Visibility::Hidden,
                        ChildOf(parent),
                    ))
                    .id(),
            );
        }
        // `falseLightBuild` — the lure's own point light, inside the glass (`userData.lightY`).
        ProfileKind::FalseLight => {
            let fl = &config.creature.false_light.light;
            point = Some(
                commands
                    .spawn((
                        Name::new("falseLight"),
                        PointLight {
                            color: model::rgb_u32(fl.color),
                            intensity: 0.0,
                            range: fl.dist,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        Transform::from_xyz(0.0, fl.y, 0.0),
                        Visibility::Hidden,
                        ChildOf(root),
                    ))
                    .id(),
            );
        }
        _ => {}
    }

    commands.entity(root).insert(Creature {
        id: h.id,
        profile: h.profile,
        model,
        spot,
        point,
        burst_active: false,
    });
    root
}

/// `atan2(2.28 − 1.0, 6)` — the downward tilt of the Warden's cone, from the JS spotlight at
/// `(0, spotY 2.28, 0)` aimed at its target `(0, 1.0, −6)`.
const WARDEN_CONE_PITCH: f32 = 0.209_29;

/// The lane: the roster diff, the `syncMesh` port and the ember bursts, all at render rate.
///
/// The mesh/material collections are guarded: `UndercroftPlugin` is also built without a renderer
/// in `headless::tests::the_asset_loader_reads_the_data_directory` (`MinimalPlugins` +
/// `AssetPlugin`), where `Assets<Mesh>` does not exist and fetching it would panic.
pub fn plugin(app: &mut App) {
    app.init_resource::<HunterEntities>()
        .init_resource::<burst::BurstRng>()
        .add_systems(
            Update,
            (sync_roster, sync::sync_creatures, burst::update_bursts)
                .chain()
                .run_if(
                    resource_exists::<Assets<Mesh>>
                        .and_then(resource_exists::<Assets<StandardMaterial>>),
                ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use undercroft_data::GameData;
    use undercroft_sim::creature::HState;
    use undercroft_sim::world::ZoneDoors;
    use undercroft_sim::Pool;

    use crate::assets::{GameDataAsset, GameDataHandle};
    use crate::resources::Zone;

    /// A headless app with this lane's systems: no `AssetPlugin` and no renderer, just the asset
    /// collections the builder writes into (`Assets<T>` is `Default` and `add` needs no server).
    fn lane_app() -> App {
        let mut app = App::new();
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        let asset = GameDataAsset::from_data(data).expect("creature tuning");
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<GameDataAsset>>()
            .init_resource::<ZoneRes>();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<GameDataAsset>>()
            .add(asset);
        app.insert_resource(GameDataHandle(handle));
        app.add_plugins(plugin);
        app
    }

    /// The zone the lane mirrors: only `hunters` matters here.
    fn zone_with(app: &App, hunters: Vec<Hunter>) -> Zone {
        let assets = app.world().resource::<Assets<GameDataAsset>>();
        let handle = app.world().resource::<GameDataHandle>();
        let data = &assets.get(&handle.0).expect("loaded").data;
        let map = data.parse_hub().expect("hub map parses");
        Zone {
            id: "test".to_string(),
            pool: Pool::empty(map.cells.len()),
            doors: ZoneDoors::closed(&map),
            map,
            lanterns: Vec::new(),
            hunters,
            items: Vec::new(),
            source: None,
        }
    }

    fn hunter(id: u32, profile: ProfileKind, x: f32, z: f32, active: bool) -> Hunter {
        let mut h = Hunter::new(id, profile, HState::Wander, 0.0);
        h.x = x;
        h.z = z;
        h.active = active;
        h
    }

    #[test]
    fn every_profile_has_a_model() {
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        for p in ProfileKind::ALL {
            let key = model_key(p);
            let def = data
                .models
                .get(key)
                .unwrap_or_else(|| panic!("models.ron has no `{key}` for profile {p:?}"));
            assert!(!def.boxes.is_empty(), "{key} has no boxes");
            // the eyes every profile animates (the Warden's are its visor)
            let eyes = ["eyeL", "eyeR", "visor"];
            assert!(
                eyes.iter().any(|n| def.named(n).is_some()),
                "{key} has none of {eyes:?}"
            );
        }
        assert_eq!(model_key(ProfileKind::Base), "hunter");
        assert_eq!(model_key(ProfileKind::Fast), "hunterFast");
        assert_eq!(model_key(ProfileKind::FalseLight), "falseLight");
    }

    #[test]
    fn fallback_box_matches_the_js() {
        // `hunter.js:fallbackModel` — a body box plus two eyes, sized by `fallbackSize`.
        let (color, size) = fallback_look(ProfileKind::Brute);
        let def = fallback_def("brute", color, size, 0xff6a20);
        assert_eq!(def.boxes.len(), 3);
        assert_eq!(def.box_count, 3);
        let body = def.named("body").expect("body box");
        assert_eq!((body.w, body.h), (1.3, 2.6));
        assert_eq!(body.color, 0x141210);
        let eye = def.named("eyeR").expect("right eye");
        assert_eq!(eye.emissive, Some(0xff6a20));
        assert!((eye.emissive_k - 0.4).abs() < 1e-6);
        assert!((eye.x - 1.3 * 0.2).abs() < 1e-6);
        assert!((eye.y - 2.6 * 0.85).abs() < 1e-6);
    }

    #[test]
    fn spawn_model_builds_the_wardens_parts_and_boxes() {
        let mut app = lane_app();
        let def = {
            let assets = app.world().resource::<Assets<GameDataAsset>>();
            let handle = app.world().resource::<GameDataHandle>();
            assets
                .get(&handle.0)
                .expect("loaded")
                .data
                .models
                .get("warden")
                .expect("models.warden")
                .clone()
        };
        let root = app
            .world_mut()
            .spawn((Transform::default(), Visibility::Inherited))
            .id();
        let boxes = def.boxes.len();
        let built = {
            let def = def.clone();
            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands,
                          mut meshes: ResMut<Assets<Mesh>>,
                          mut materials: ResMut<Assets<StandardMaterial>>| {
                        spawn_model(&mut commands, &mut meshes, &mut materials, &def, root)
                    },
                )
                .expect("one-shot system runs")
        };

        // `models.js:warden` — hip pivots, the neck and the visor pivot the cone hangs on.
        let mut parts: Vec<&str> = built.parts.keys().map(String::as_str).collect();
        parts.sort_unstable();
        assert_eq!(parts, ["conePivot", "head", "legs0", "legs1"]);
        assert_eq!(built.boxes.len(), boxes);
        assert_eq!(boxes, 15);
        for name in ["plinth", "visor", "helm", "halberd"] {
            assert!(built.named(name).is_some(), "no `{name}` box");
        }

        // the cone pivot hangs off the head, and a box sits at `y + h / 2` (three's centre)
        let cone = built.part("conePivot").expect("conePivot");
        let head = built.part("head").expect("head");
        let world = app.world();
        assert_eq!(
            world.get::<ChildOf>(cone.entity).map(ChildOf::parent),
            Some(head.entity)
        );
        let visor = built.named("visor").expect("visor");
        let vb = def.named("visor").expect("visor box");
        let t = world
            .get::<Transform>(visor.entity)
            .expect("visor transform");
        assert!((t.translation.y - (vb.y + vb.h * 0.5)).abs() < 1e-6);
        assert_eq!(
            world
                .get::<Name>(visor.entity)
                .map(|n| n.as_str().to_string()),
            Some("visor".to_string())
        );
        // one material per distinct (color, emissive, emissive_k): the Warden has fewer than boxes
        assert!(world.resource::<Assets<StandardMaterial>>().len() < boxes);
    }

    #[test]
    fn sync_places_active_hunters_and_hides_the_rest() {
        let mut app = lane_app();
        let mut walker = hunter(0, ProfileKind::Base, 3.0, -2.0, true);
        walker.yaw = 0.5;
        walker.anim.eye_k = 1.5;
        let zone = zone_with(
            &app,
            vec![walker, hunter(1, ProfileKind::Warden, 1.0, 1.0, false)],
        );
        app.insert_resource(ZoneRes(Some(zone)));
        app.update();

        let (a, b) = {
            let roster = app.world().resource::<HunterEntities>();
            assert_eq!(roster.map.len(), 2);
            (
                roster.get(0).expect("hunter 0"),
                roster.get(1).expect("hunter 1"),
            )
        };
        let world = app.world();
        let t = world.get::<Transform>(a).expect("transform");
        assert_eq!(t.translation, Vec3::new(3.0, 0.0, -2.0));
        let (yaw, _, _) = t.rotation.to_euler(EulerRot::YXZ);
        assert!((yaw - 0.5).abs() < 1e-5, "yaw {yaw}");
        assert_eq!(world.get::<Visibility>(a), Some(&Visibility::Inherited));
        assert_eq!(world.get::<Visibility>(b), Some(&Visibility::Hidden));

        // the eyes glow with `prof.eye[state]` in the profile's colour
        let creature = world.get::<Creature>(a).expect("creature");
        let eye = creature.model.named("eyeL").expect("eyeL");
        let mat = world
            .resource::<Assets<StandardMaterial>>()
            .get(&eye.material)
            .expect("eye material");
        assert_eq!(mat.emissive, model::emissive_rgba(0xff3a20, 1.5));

        // the Warden brought its cone light
        let warden = world.get::<Creature>(b).expect("warden");
        assert!(warden.spot.is_some() && warden.point.is_none());

        // the record disappearing takes the entity with it (`spawnAll` shrinking the list)
        let zone = zone_with(&app, vec![hunter(0, ProfileKind::Base, 3.0, -2.0, true)]);
        app.insert_resource(ZoneRes(Some(zone)));
        app.update();
        assert_eq!(app.world().resource::<HunterEntities>().map.len(), 1);
        assert!(app.world().get_entity(b).is_err());
    }
}
