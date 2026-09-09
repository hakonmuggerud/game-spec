//! Visual check for the creatures lane, kept in the tree as the lane's own smoke test.
//!
//! The world lane owns the game's camera and lighting, so a creature spawned by
//! `UndercroftPlugin` alone renders into a black window until the two lanes merge. This example
//! stands the lane up on its own: `DefaultPlugins` + `creatures::plugin`, a fixed camera, an
//! ambient light and one point light, and a fake `ZoneRes` holding one `Hunter` record per
//! profile, side by side, with `Anim` posed by hand (the sim is not running here). It takes a
//! screenshot after two seconds and exits.
//!
//! ```sh
//! export PATH="$HOME/.cargo/bin:$PATH"
//! Xvfb :92 -screen 0 1280x800x24 &
//! DISPLAY=:92 WINIT_UNIX_BACKEND=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!   WGPU_BACKEND=vulkan UNDERCROFT_SHOT=/tmp/creatures.png \
//!   cargo run -p undercroft --features dev --example creature_preview
//! ```
//!
//! `UNDERCROFT_SHOT` sets the PNG path; `UNDERCROFT_POSE=alt` flips the poses (the false light
//! goes dark, the drowner shuts its jaw, the legs swing the other way) so the animated parts can be
//! compared between two runs.

use bevy::camera::Exposure;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use undercroft::assets::{GameDataAsset, GameDataHandle};
use undercroft::creatures;
use undercroft::resources::{Zone, ZoneRes};
use undercroft_data::GameData;
use undercroft_sim::creature::{Hunter, ProfileKind};
use undercroft_sim::world::ZoneDoors;
use undercroft_sim::Pool;

/// Where the profiles stand, in world units.
const SPACING: f32 = 2.0;

fn main() {
    let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
    let asset = GameDataAsset::from_data(data).expect("creature tuning");
    let zone = preview_zone(&asset);

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "creature preview".into(),
            resolution: UVec2::new(1280, 800).into(),
            ..default()
        }),
        ..default()
    }))
    .init_asset::<GameDataAsset>();

    let handle = app
        .world_mut()
        .resource_mut::<Assets<GameDataAsset>>()
        .add(asset);
    app.insert_resource(GameDataHandle(handle))
        .insert_resource(ZoneRes(Some(zone)))
        .add_plugins(creatures::plugin)
        .add_systems(Startup, setup)
        .add_systems(Update, shoot_and_quit)
        .run();
}

/// A zone that exists only to carry the hunter records; the map is the hub's, since nothing in this
/// lane reads it.
fn preview_zone(asset: &GameDataAsset) -> Zone {
    let map = asset.data.parse_hub().expect("hub map parses");
    let alt = std::env::var("UNDERCROFT_POSE").as_deref() == Ok("alt");
    let n = ProfileKind::ALL.len() as f32;
    let hunters = ProfileKind::ALL
        .into_iter()
        .enumerate()
        .map(|(i, profile)| {
            let prof = asset.tuning.prof(profile);
            let mut h = Hunter::new(i as u32, profile, prof.initial, 0.0);
            h.active = true;
            h.x = (i as f32 - (n - 1.0) * 0.5) * SPACING;
            h.z = 0.0;
            pose(&mut h, alt);
            h
        })
        .collect();
    Zone {
        id: "preview".to_string(),
        pool: Pool::empty(map.cells.len()),
        doors: ZoneDoors::closed(&map),
        map,
        lanterns: Vec::new(),
        hunters,
        items: Vec::new(),
        source: None,
    }
}

/// What the sim's `anim` hooks would have written, by hand: enough of each profile's animation to
/// see that the parts, lights and emissives are wired up.
fn pose(h: &mut Hunter, alt: bool) {
    h.anim.eye_k = 1.2;
    h.anim.leg_phase = if alt { 4.2 } else { 1.2 };
    match h.profile {
        ProfileKind::Lampwight => {
            h.y = 0.05;
            h.anim.ember_k = if alt { 0.0 } else { 1.5 };
        }
        ProfileKind::Warden => {
            h.anim.light_k = 1.2;
            h.anim.light_on = true;
            h.anim.plinth = true;
            h.anim.leg_swing = 0.3;
        }
        ProfileKind::Drowner => {
            // in the game it sits at `DR.ySurf` (−0.15) with the ripple on the water surface at
            // −0.1; here it stands on the floor so the ring is not buried in it
            h.anim.body_shown = true;
            h.anim.jaw = if alt { 0.0 } else { -0.45 };
            h.anim.ripple_scale = 1.2;
            h.anim.ripple_k = 0.4;
            h.anim.ring_y = 0.02;
        }
        ProfileKind::FalseLight => {
            h.anim.posed_dark = alt;
            h.anim.glass_k = if alt { 0.0 } else { 1.0 };
            h.anim.light_k = if alt { 0.0 } else { 2.0 };
            h.anim.light_on = !alt;
        }
        ProfileKind::Brute => {
            h.anim.leg_swing = 0.35;
            h.anim.sway = 0.06;
        }
        ProfileKind::Base | ProfileKind::Fast => {}
    }
}

/// The camera, at eye height in front of the row, a floor for the Warden's cone to land on, and
/// one point light off to the side. The world lane owns all of this in the real game.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Name::new("floor"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40.0, 40.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb_u8(0x1a, 0x18, 0x15),
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    commands.spawn((
        Camera3d::default(),
        // `AmbientLight` is a per-camera component in Bevy 0.19, not a resource.
        AmbientLight {
            color: Color::srgb(0.7, 0.75, 0.85),
            brightness: 3.0,
            ..default()
        },
        // `ev100 = -0.263` makes `exp2(-ev100) / 1.2 == 1`, i.e. Bevy's exposure is the identity
        // and light intensities in `config.ron` (three.js candela) read as they did in the
        // prototype. See `creatures::LUMENS_PER_CANDELA`.
        Exposure { ev100: -0.263 },
        // the models face −Z, so the camera stands in front of the row and looks back at it
        Transform::from_xyz(0.0, 1.7, -11.0).looking_at(Vec3::new(0.0, 1.1, 0.0), Vec3::Y),
    ));
    commands.spawn((
        PointLight {
            color: Color::srgb(1.0, 0.93, 0.82),
            intensity: 300.0 * creatures::LUMENS_PER_CANDELA,
            range: 40.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(3.0, 4.0, -6.0),
    ));
}

/// Screenshot at 4 s, exit at 6 s. Software rendering (`llvmpipe` on Xvfb) needs a few seconds to
/// have drawn a frame; capturing earlier sometimes saves an empty swapchain image.
fn shoot_and_quit(
    time: Res<Time>,
    creatures: Query<&creatures::Creature>,
    mut commands: Commands,
    mut shot: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let t = time.elapsed_secs();
    if t >= 4.0 && !*shot {
        *shot = true;
        let path = std::env::var("UNDERCROFT_SHOT")
            .unwrap_or_else(|_| "/tmp/creature_preview.png".to_string());
        info!("creature_preview: screenshot -> {path}");
        info!(
            "creature_preview: {} creatures spawned",
            creatures.iter().count()
        );
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if t >= 6.0 {
        exit.write(AppExit::Success);
    }
}
