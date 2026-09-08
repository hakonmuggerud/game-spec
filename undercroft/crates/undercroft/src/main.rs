//! Window, plugins, run. Everything else lives in the library (`undercroft::*`).
//!
//! Until the world lane lands, `GameMode::Title` shows the Phase 0 placeholder scene — a lit
//! spinning cube — so `cargo run` and the wasm build still draw something after the data loads.

use bevy::prelude::*;
use undercroft::assets::asset_plugin;
use undercroft::{GameMode, UndercroftPlugin};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "The Undercroft".into(),
                        // On the web, size the canvas to the page and let CSS own it.
                        fit_canvas_to_parent: true,
                        #[cfg(target_arch = "wasm32")]
                        canvas: Some("#undercroft".into()),
                        ..default()
                    }),
                    ..default()
                })
                .set(asset_plugin()),
        )
        .add_plugins(UndercroftPlugin)
        .add_systems(OnEnter(GameMode::Title), spawn_placeholder)
        .add_systems(OnExit(GameMode::Title), despawn_placeholder)
        .add_systems(Update, spin.run_if(in_state(GameMode::Title)))
        .run();
}

/// Everything the placeholder scene spawns, so `OnExit(Title)` can clear it in one query.
#[derive(Component)]
struct TitleScene;

/// The cube itself.
#[derive(Component)]
struct Spinner;

fn spawn_placeholder(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    info!("GameMode::Title — placeholder scene");
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.45, 0.35),
            // The prototype's Lambert look (HANDOFF §6, world lane).
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.5, 0.0),
        Spinner,
        TitleScene,
    ));
    commands.spawn((
        PointLight {
            intensity: 20_000.0,
            range: 12.0,
            color: Color::srgb(1.0, 0.8, 0.5),
            ..default()
        },
        Transform::from_xyz(1.5, 2.0, 1.5),
        TitleScene,
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.5, 1.6, 2.5).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
        TitleScene,
    ));
}

fn despawn_placeholder(mut commands: Commands, q: Query<Entity, With<TitleScene>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn spin(time: Res<Time>, mut q: Query<&mut Transform, With<Spinner>>) {
    for mut t in &mut q {
        t.rotate_y(time.delta_secs() * 0.7);
    }
}
