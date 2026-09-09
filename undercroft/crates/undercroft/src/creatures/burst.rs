//! The Brute's lantern smash (`hunter.js:bruteNearLantern` → `fireBurst`, `models.js:emberBurst`
//! and `lanternDebris`): a short-lived group of particles at the lantern's position.
//!
//! The sim reports the smash as animation data — `Anim::burst_fired` for the one frame it happens,
//! `burst_x` / `burst_z` where, and `burst_t` counting the life down — so this module only spawns
//! and steps entities. The JS used a `THREE.Points` cloud for the embers plus the eight recorded
//! `lanternDebris` boxes; here both are small cuboids: 24 emissive embers that rise and drift, and
//! the debris boxes thrown out under gravity, bouncing once off the floor. The whole burst lives in
//! world space, not under the creature (`hunter.js:clear`: "a smash on the last frame of a run must
//! not follow the player into the hub").

use bevy::prelude::*;
use undercroft_data::{Config, ModelTable};
use undercroft_sim::SimRng;

use crate::model::box_material;
use crate::world::palette;

/// One burst: the parent of its particles, holding the fade timer and the materials it fades.
#[derive(Component, Debug)]
pub struct Burst {
    /// Seconds since the smash.
    pub t: f32,
    /// `BR.embers.life` (0.8 s) or the debris' 0.9 s, whichever is longer.
    pub life: f32,
    /// `(material, base colour, unit emissive)` — the JS faded `material.opacity`.
    pub mats: Vec<(Handle<StandardMaterial>, Color, LinearRgba)>,
}

/// One flying piece: `models.js`'s per-mesh velocity/spin record.
#[derive(Component, Debug, Default)]
pub struct BurstParticle {
    pub vel: Vec3,
    /// Radians per second about local X / Z (`lanternDebris`'s tumble).
    pub spin: Vec2,
    /// Falls and bounces (debris) rather than rising (embers).
    pub gravity: bool,
}

/// The randomness the burst is seeded with. `hunter.js` used `Math.random()`; the sim's RNG is
/// reused here so the app crate needs no extra dependency and the web build has no entropy source
/// to reach for. Purely cosmetic: the sim never reads it.
#[derive(Resource, Debug)]
pub struct BurstRng(pub SimRng);

impl Default for BurstRng {
    fn default() -> Self {
        BurstRng(SimRng::seed(0xbee5_0f1a))
    }
}

/// `lanternDebris` life (`models.js`: 0.9 s, not in `config.ron`).
const DEBRIS_LIFE: f32 = 0.9;
/// Ember cube edge (`PointsMaterial.size` 0.12 in `models.js:emberBurst`).
const EMBER_SIZE: f32 = 0.1;
/// Gravity on the debris (`models.js:lanternDebris`).
const GRAVITY: f32 = 9.8;

/// `fireBurst(b, x, z)` — park a fresh burst at the smashed lantern and start it.
#[allow(clippy::too_many_arguments)]
pub fn fire(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    models: &ModelTable,
    config: &Config,
    rng: &mut SimRng,
    x: f32,
    z: f32,
) {
    let em = &config.creature.brute.embers;
    let mut mats: Vec<(Handle<StandardMaterial>, Color, LinearRgba)> = Vec::new();
    let root = commands
        .spawn((
            Name::new("emberBurst"),
            Transform::from_xyz(x, 0.0, z),
            Visibility::Inherited,
        ))
        .id();

    // --- embers (`models.js:emberBurst`): rise 0.8 u/s with a little drift, fading out ---
    let ember_mat = materials.add(StandardMaterial {
        alpha_mode: AlphaMode::Blend,
        ..box_material(0x000000, Some(em.color), 1.0)
    });
    mats.push((
        ember_mat.clone(),
        Color::BLACK,
        palette::rgb(em.color).to_linear(),
    ));
    let ember_mesh = meshes.add(Cuboid::from_length(EMBER_SIZE));
    for _ in 0..em.n {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let r = rng.range(0.0, 0.25);
        commands.spawn((
            Mesh3d(ember_mesh.clone()),
            MeshMaterial3d(ember_mat.clone()),
            Transform::from_xyz(a.cos() * r, rng.range(0.9, 1.4), a.sin() * r),
            BurstParticle {
                vel: Vec3::new(
                    rng.range(-0.6, 0.6),
                    em.rise * rng.range(0.7, 1.3),
                    rng.range(-0.6, 0.6),
                ),
                ..default()
            },
            ChildOf(root),
        ));
    }

    // --- debris (`models.js:lanternDebris`): the lantern itself coming apart ---
    if let Some(def) = models.get("lanternDebris") {
        let n = def.boxes.len().max(1) as f32;
        for (i, b) in def.boxes.iter().enumerate() {
            let material = materials.add(StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                ..box_material(b.color, b.emissive, b.emissive_k)
            });
            mats.push((
                material.clone(),
                palette::rgb(b.color),
                b.emissive
                    .map_or(LinearRgba::BLACK, |e| palette::emissive(e, b.emissive_k)),
            ));
            let a = i as f32 / n * std::f32::consts::TAU + rng.range(0.0, 0.8);
            let s = rng.range(1.6, 3.2);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(b.w, b.h, b.d))),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, b.y + b.h * 0.5, 0.0),
                BurstParticle {
                    vel: Vec3::new(a.cos() * s, rng.range(2.2, 4.0), a.sin() * s),
                    spin: Vec2::new(rng.range(-6.0, 6.0), rng.range(-6.0, 6.0)),
                    gravity: true,
                },
                ChildOf(root),
            ));
        }
    }

    commands.entity(root).insert(Burst {
        t: 0.0,
        life: em.life.max(DEBRIS_LIFE),
        mats,
    });
}

/// `stepBurst` + the per-piece integration of `models.js`'s `step(dt)`: ballistic motion, the
/// bounce, the fade, and the despawn once `life` has elapsed (`b.group.visible = false` in the JS,
/// which kept one parked group per Brute; entities are cheap enough to throw away instead).
pub fn update_bursts(
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bursts: Query<(Entity, &mut Burst)>,
    mut parts: Query<(&mut Transform, &mut BurstParticle)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let drag = 0.96f32.powf(dt * 60.0); // the JS multiplied by 0.96 per frame at 60 fps
    for (mut t, mut p) in &mut parts {
        if p.gravity {
            p.vel.y -= GRAVITY * dt;
        } else {
            p.vel.x *= drag;
            p.vel.z *= drag;
        }
        let v = p.vel;
        t.translation += v * dt;
        if p.gravity {
            if t.translation.y < 0.03 {
                t.translation.y = 0.03;
                p.vel.y = -p.vel.y * 0.25;
                p.vel.x *= 0.6;
                p.vel.z *= 0.6;
            }
            let spin = p.spin;
            t.rotate_local_x(spin.x * dt);
            t.rotate_local_z(spin.y * dt);
        }
    }
    for (entity, mut burst) in &mut bursts {
        burst.t += dt;
        let fade = (1.0 - burst.t / burst.life).clamp(0.0, 1.0);
        for (handle, base, emissive) in &burst.mats {
            let Some(mut m) = materials.get_mut(handle) else {
                continue;
            };
            m.base_color = base.with_alpha(fade);
            m.emissive = *emissive * fade;
        }
        if burst.t >= burst.life {
            commands.entity(entity).despawn();
        }
    }
}
