//! Phase 0 smoke app: a lit cube and a camera, enough to prove the native and wasm
//! toolchains work end to end. Phase 2's skeleton step replaces this.
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "The Undercroft".into(),
                // On the web, size the canvas to the page and let CSS own it.
                fit_canvas_to_parent: true,
                #[cfg(target_arch = "wasm32")]
                canvas: Some("#undercroft".into()),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, spin)
        .run();
}

#[derive(Component)]
struct Spinner;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.45, 0.35),
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.5, 0.0),
        Spinner,
    ));
    commands.spawn((
        PointLight {
            intensity: 20_000.0,
            range: 12.0,
            color: Color::srgb(1.0, 0.8, 0.5),
            ..default()
        },
        Transform::from_xyz(1.5, 2.0, 1.5),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.5, 1.6, 2.5).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
    ));
}

fn spin(time: Res<Time>, mut q: Query<&mut Transform, With<Spinner>>) {
    for mut t in &mut q {
        t.rotate_y(time.delta_secs() * 0.7);
    }
}
