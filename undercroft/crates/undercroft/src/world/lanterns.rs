//! Planted lanterns (`world.js:spawnLantern` / `removeLantern` and the flicker in `world.js:update`):
//! the `lantern` model, its `PointLight` at `userData.lightY = 1.28` and the pool ring that shows
//! `CFG.poolR` on the floor. One entity per `Zone.lanterns` record, diffed by index.

use bevy::prelude::*;
use undercroft_sim::pool::Lantern;

use crate::resources::{Game, ZoneRes};
use crate::state::GameMode;
use crate::tick::Clock;

use super::model::{build_model, spawn_groups};
use super::palette::{emissive, lambert, rgb};

/// One planted lantern, keyed by its index in `Zone.lanterns`.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlantedLantern {
    pub index: usize,
    pub x: f32,
    pub z: f32,
    /// `l.phase` — `Math.random() * 6` in the JS, hashed here.
    pub phase: f32,
}

/// `models.lantern().userData.lightY`.
const LIGHT_Y: f32 = 1.28;
/// `new THREE.PointLight(0xffc070, 2.0, 6, 2)`.
const LIGHT_INT: f32 = 2.0;
const LIGHT_DIST: f32 = 6.0;
const LIGHT_COLOR: u32 = 0xffc070;

/// The lantern's own light, so the flicker can find it.
#[derive(Component, Debug, Clone, Copy)]
pub struct LanternLight;

/// `ringGeo` — `RingGeometry(poolR − 0.15, poolR, 40)` laid flat at `y = 0.02`.
pub fn pool_ring(pool_r: f32) -> Mesh {
    Mesh::from(Annulus::new((pool_r - 0.15).max(0.01), pool_r))
        .rotated_by(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
}

/// Mirror `Zone.lanterns` into entities.
pub(super) fn sync_lanterns(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    game: Game,
    zone: Res<ZoneRes>,
    existing: Query<(Entity, &PlantedLantern)>,
    scene: Query<Entity, With<super::ZoneScene>>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let lanterns: &[Lantern] = match zone.get() {
        Some(z) => &z.lanterns,
        None => &[],
    };
    let Ok(root) = scene.single() else {
        for (e, _) in &existing {
            commands.entity(e).despawn();
        }
        return;
    };

    let mut seen = vec![false; lanterns.len()];
    for (e, view) in &existing {
        match lanterns.get(view.index) {
            Some(l) if (l.x - view.x).abs() < 1e-4 && (l.z - view.z).abs() < 1e-4 => {
                seen[view.index] = true;
            }
            _ => commands.entity(e).despawn(),
        }
    }
    for (i, l) in lanterns.iter().enumerate() {
        if seen[i] {
            continue;
        }
        let e = commands
            .spawn((
                Name::new("lantern"),
                PlantedLantern {
                    index: i,
                    x: l.x,
                    z: l.z,
                    phase: super::palette::jitter(
                        (l.x * 16.0) as i32,
                        (l.z * 16.0) as i32,
                        i as u32 + 31,
                        1.0,
                    ) * 3.0,
                },
                Transform::from_xyz(l.x, 0.0, l.z),
                Visibility::default(),
            ))
            .id();
        commands.entity(root).add_child(e);
        if let Some(def) = asset.data.models.get("lantern") {
            spawn_groups(
                &mut commands,
                &mut meshes,
                &mut materials,
                e,
                build_model(def, &["glass"]),
            );
        }
        let pool_r = asset.data.config.cfg.pool_r;
        commands.entity(e).with_children(|p| {
            p.spawn((
                Name::new("poolRing"),
                Mesh3d(meshes.add(pool_ring(pool_r))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    emissive: emissive(0xffc070, 0.6),
                    double_sided: true,
                    cull_mode: None,
                    ..lambert(Color::BLACK)
                })),
                Transform::from_xyz(0.0, 0.02, 0.0),
            ));
            p.spawn((
                Name::new("lantern:light"),
                LanternLight,
                PointLight {
                    color: rgb(LIGHT_COLOR),
                    intensity: LIGHT_INT,
                    range: LIGHT_DIST,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, LIGHT_Y, 0.0),
            ));
        });
    }
}

/// `world.js:update` — `f = 0.93 + 0.07 · sin(time · 13 + phase)`, applied to the light's intensity
/// and the glass's `emissiveIntensity`, in `ZONE` only.
pub(super) fn flicker_lanterns(
    clock: Res<Clock>,
    mode: Res<State<GameMode>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lanterns: Query<(&PlantedLantern, &Children)>,
    mut lights: Query<&mut PointLight, With<LanternLight>>,
    glass: Query<(&Name, &MeshMaterial3d<StandardMaterial>)>,
) {
    if *mode.get() != GameMode::Zone {
        return;
    }
    for (l, children) in &lanterns {
        let f = 0.93 + 0.07 * (clock.time * 13.0 + l.phase).sin();
        for child in children.iter() {
            if let Ok(mut light) = lights.get_mut(child) {
                light.intensity = LIGHT_INT * f;
            }
            if let Ok((name, mat)) = glass.get(child) {
                if name.as_str() == "glass" {
                    if let Some(mut m) = materials.get_mut(&mat.0) {
                        m.emissive = emissive(LIGHT_COLOR, f);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ring is flat (all vertices at y ≈ 0) and spans `poolR − 0.15 … poolR`.
    #[test]
    fn pool_ring_is_flat_and_the_right_size() {
        let m = pool_ring(2.5);
        match m.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x3(v) => {
                assert!(v.iter().all(|p| p[1].abs() < 1e-5), "flat");
                let r: Vec<f32> = v.iter().map(|p| p[0].hypot(p[2])).collect();
                let min = r.iter().cloned().fold(f32::INFINITY, f32::min);
                let max = r.iter().cloned().fold(0.0f32, f32::max);
                assert!((min - 2.35).abs() < 1e-3, "inner {min}");
                assert!((max - 2.5).abs() < 1e-3, "outer {max}");
            }
            _ => panic!("positions"),
        }
    }

    /// The flicker stays inside the JS band.
    #[test]
    fn flicker_band() {
        for i in 0..64 {
            let f = 0.93 + 0.07 * ((i as f32) * 0.31).sin();
            assert!((0.86..=1.0).contains(&f), "{f}");
        }
    }
}
