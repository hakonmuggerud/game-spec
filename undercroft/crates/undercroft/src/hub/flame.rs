//! The great flame (`hub.js:buildFlame` / `applyTier` / `update`, `models.js:flameBase`) and its
//! ember cloud (`hub.js:buildEmbers` / `spawnEmber` / `updateEmbers`, `config.js:EMBERS`).
//!
//! The `flameBase` model is built from `models.ron`; its `fire` part carries the four tongues, so
//! the tier scale and the sway animation are a transform on that one entity, exactly as three's
//! `fire` group was.

use bevy::prelude::*;
use undercroft_data::models::BoxDef;
use undercroft_sim::grid::center;
use undercroft_sim::SimRng;

use crate::resources::{Game, HubMapRes, HubRes};
use crate::tick::Clock;

use super::model::{spawn_box, BoxAssets, SpawnedModel};
use super::{lerp_linear, linear_u32, rgb_u32, scale_rgb, LUMENS_PER_JS_INTENSITY};

/// The flame root (`hub.js` `ctx.hub.flame.group`, three name `flame`).
#[derive(Component, Debug)]
pub struct Flame {
    /// The `fire` pivot: the four tongues hang off it.
    pub fire: Entity,
    /// The `base` and `rim` boxes, whose emissive intensity grows with the tier.
    pub base: Option<Entity>,
    pub rim: Option<Entity>,
    /// The tongue boxes, outermost first (`tongue0` … `tongue3`).
    pub tongues: Vec<Entity>,
    /// The tier the materials were last built for, so a change re-tints them.
    pub tier: u32,
}

/// The flame's `PointLight` (`hub.js:buildFlame` — `0xffa040`, intensity `TIERS[t].int`, distance
/// `TIERS[t].dist`, at `lightY` 1.2).
#[derive(Component, Debug)]
pub struct FlameLight;

/// One ember of the cloud (`hub.js:buildEmbers` — a `THREE.Points` in the JS, a tiny emissive
/// cuboid here; `EMBERS.perTier[tier - 1]` of them are alive at a time).
#[derive(Component, Debug)]
pub struct Ember {
    /// Index in the cloud, as `updateEmbers` uses it (`i < want` gates respawning).
    pub i: usize,
    /// Seconds of life left; `<= 0` means dead and counting up to its next birth.
    pub life: f32,
    pub vel: Vec3,
}

/// The ember cloud's own randomness. `RngRes` is the *sim's* single stream (HANDOFF §8) and lanes
/// never mutate shared resources, so the visual-only jitter gets its own deterministic generator.
#[derive(Resource, Debug)]
pub struct EmberRng(pub SimRng);

impl Default for EmberRng {
    fn default() -> Self {
        EmberRng(SimRng::seed(0xE3BE_0000))
    }
}

/// `models.js:flameBase setTier` — the tongue gradient for a tier: `0xff6020 → 0xffd080` across
/// the tiers, then `→ 0xfff0c0` up the stack, scaled `0.7 + 0.1 * i`.
pub fn tongue_emissive(tier: u32, i: usize) -> LinearRgba {
    let t = ((tier as f32 - 1.0) / 3.0).clamp(0.0, 1.0);
    let c = lerp_linear(linear_u32(0xff6020), linear_u32(0xffd080), t);
    scale_rgb(
        lerp_linear(c, linear_u32(0xfff0c0), i as f32 * 0.22),
        0.7 + 0.1 * i as f32,
    )
}

/// `models.js:flameBase setTier` — `fire.scale.setScalar(0.6 + 0.4 * (t - 1))`.
pub fn fire_scale(tier: u32) -> f32 {
    0.6 + 0.4 * (tier as f32 - 1.0)
}

/// `hub.js:update` — the flame's light flicker, 0.80 … 1.0 of the tier intensity.
pub fn flicker(time: f32) -> f32 {
    0.9 + 0.05 * (time * 13.0).sin()
        + 0.035 * (time * 7.3 + 1.1).sin()
        + 0.015 * (time * 31.0).sin()
}

/// `hub.js:166 buildFlame` — the brazier, once the hub map is there.
#[allow(clippy::too_many_arguments)]
fn spawn_flame(
    mut commands: Commands,
    game: Game,
    hub_map: Res<HubMapRes>,
    mut assets: ResMut<BoxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<Flame>>,
) {
    if !existing.is_empty() {
        return;
    }
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let Some(hm) = hub_map.0.as_ref() else {
        return;
    };
    let Some(f) = hm.map.flame else {
        return;
    };
    let Some(def) = data.models.get("flameBase") else {
        error!("hub: models.ron has no flameBase");
        return;
    };
    let (x, z) = center(&hm.map, f.cx, f.cz);
    let built: SpawnedModel = super::model::spawn_model(
        &mut commands,
        &mut assets,
        &mut meshes,
        &mut mats,
        def,
        Transform::from_xyz(x, 0.0, z),
    );
    let Some(fire) = built.part("fire") else {
        error!("hub: flameBase has no `fire` part");
        return;
    };
    let tongues: Vec<Entity> = (0..4)
        .filter_map(|i| built.named(&format!("tongue{i}")))
        .collect();
    commands.entity(built.root).insert(Flame {
        fire,
        base: built.named("base"),
        rim: built.named("rim"),
        tongues,
        tier: 0,
    });
    // The one real light of the hub centre.
    commands.spawn((
        FlameLight,
        Name::new("hub:flameLight"),
        PointLight {
            color: rgb_u32(0xffa040),
            intensity: 2.0 * LUMENS_PER_JS_INTENSITY,
            range: 7.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(x, 1.2, z),
        ChildOf(built.root),
    ));
    // `hub.js:178 buildEmbers` — EMBERS.max points, all dead, waiting in the coal bed.
    let e_cfg = &data.config.embers;
    let s = e_cfg.size * 0.5; // a Points sprite of `size` reads as a cube about half as wide
    let proto = BoxDef {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: s,
        h: s,
        d: s,
        color: 0x000000,
        emissive: Some(0xffa040),
        emissive_k: 1.0,
        name: None,
        ry: 0.0,
        part: None,
        hidden: true,
    };
    for i in 0..e_cfg.max as usize {
        let e = spawn_box(
            &mut commands,
            &mut assets,
            &mut meshes,
            &mut mats,
            built.root,
            &proto,
        );
        commands.entity(e).insert(Ember {
            i,
            life: 0.0,
            vel: Vec3::ZERO,
        });
    }
    info!("hub: great flame at ({x:.1}, {z:.1}), {} embers", e_cfg.max);
}

/// `hub.js:248 applyTier` + `hub.js:833 update` — retint on a tier change, then the per-frame
/// flicker, fire pulse and tongue sway.
#[allow(clippy::too_many_arguments)]
fn update_flame(
    game: Game,
    hub: Res<HubRes>,
    clock: Res<Clock>,
    mut flames: Query<(&mut Flame, &Children)>,
    mut light: Query<&mut PointLight, With<FlameLight>>,
    mut xf: Query<&mut Transform>,
    handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let Ok((mut flame, _)) = flames.single_mut() else {
        return;
    };
    let tier = hub.0.tier.clamp(1, data.config.tiers.len() as u32);
    let t = &data.config.tiers[(tier - 1) as usize];
    let time = clock.time;

    if flame.tier != tier {
        flame.tier = tier;
        // The tongues and the brazier get their own materials (one per tongue index), so the tint
        // is a per-entity edit rather than a shared-handle surprise.
        for (i, &e) in flame.tongues.iter().enumerate() {
            if let Ok(h) = handles.get(e) {
                if let Some(mut m) = mats.get_mut(&h.0) {
                    m.emissive = tongue_emissive(tier, i);
                }
            }
        }
        for (e, base_k, per_tier, color) in [
            (flame.base, 0.08_f32, 0.05_f32, 0xff6a28_u32),
            (flame.rim, 0.35, 0.12, 0xff7a30),
        ] {
            let Some(e) = e else { continue };
            if let Ok(h) = handles.get(e) {
                if let Some(mut m) = mats.get_mut(&h.0) {
                    m.emissive =
                        scale_rgb(linear_u32(color), base_k + per_tier * (tier as f32 - 1.0));
                }
            }
        }
    }

    if let Ok(mut l) = light.single_mut() {
        l.intensity = t.int * flicker(time) * LUMENS_PER_JS_INTENSITY;
        l.range = t.dist;
    }
    // `models.js:flameBase update(time)` — the stack breathes, each tongue twists and sways.
    let s = fire_scale(tier);
    if let Ok(mut fire_t) = xf.get_mut(flame.fire) {
        fire_t.scale = Vec3::new(s, s * (0.95 + 0.08 * (time * 9.7).sin()), s);
    }
    for (i, &e) in flame.tongues.iter().enumerate() {
        let fi = i as f32;
        let Ok(mut tt) = xf.get_mut(e) else { continue };
        tt.rotation = Quat::from_rotation_y(fi * 0.7 + 0.35 * (time * (2.1 + fi * 0.6) + fi).sin());
        tt.translation.x = 0.04 * fi * (time * 3.3 + fi * 1.7).sin();
        tt.translation.z = 0.04 * fi * (time * 2.7 + fi * 0.9).cos();
    }
}

/// `hub.js:195 updateEmbers` — rise, drift, fade and respawn, `EMBERS.perTier[tier - 1]` alive.
fn update_embers(
    game: Game,
    hub: Res<HubRes>,
    clock: Res<Clock>,
    time: Res<Time>,
    mut rng: ResMut<EmberRng>,
    mut q: Query<(&mut Ember, &mut Transform, &mut Visibility)>,
) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let cfg = &data.config.embers;
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let t = clock.time;
    let want = cfg.per_tier[(hub.0.tier.clamp(1, 4) - 1) as usize] as usize;
    let mut alive = 0usize;
    // Deterministic order so the "first `want` embers" rule of the JS holds.
    let mut list: Vec<(Mut<Ember>, Mut<Transform>, Mut<Visibility>)> = q.iter_mut().collect();
    list.sort_by_key(|(e, _, _)| e.i);
    for (ember, tr, vis) in list.iter_mut() {
        let i = ember.i;
        if ember.life > 0.0 {
            ember.life -= dt;
            tr.translation.x += (ember.vel.x + 0.15 * (t * 3.0 + i as f32).sin()) * dt;
            tr.translation.y += ember.vel.y * dt;
            tr.translation.z += (ember.vel.z + 0.15 * (t * 2.6 + i as f32).cos()) * dt;
            if ember.life <= 0.0 || tr.translation.y > 2.9 {
                ember.life = -0.2 - rng.0.range(0.0, 1.0) * 0.6;
                tr.translation.y = -5.0;
                **vis = Visibility::Hidden;
            } else {
                alive += 1;
            }
        } else if alive < want && i < want {
            ember.life += dt;
            if ember.life >= 0.0 {
                let a = rng.0.range(0.0, 1.0) * std::f32::consts::TAU;
                let r = rng.0.range(0.0, 1.0) * 0.28;
                tr.translation = Vec3::new(
                    a.cos() * r,
                    0.55 + rng.0.range(0.0, 1.0) * 0.3 * hub.0.tier as f32,
                    a.sin() * r,
                );
                ember.vel = Vec3::new(
                    (rng.0.range(0.0, 1.0) - 0.5) * cfg.drift,
                    cfg.rise[0] + rng.0.range(0.0, 1.0) * (cfg.rise[1] - cfg.rise[0]),
                    (rng.0.range(0.0, 1.0) - 0.5) * cfg.drift,
                );
                ember.life = cfg.life[0] + rng.0.range(0.0, 1.0) * (cfg.life[1] - cfg.life[0]);
                **vis = Visibility::Inherited;
                alive += 1;
            }
        }
    }
}

/// The flame, its light and the ember cloud.
pub fn plugin(app: &mut App) {
    app.init_resource::<EmberRng>().add_systems(
        Update,
        (spawn_flame, update_flame, update_embers)
            .chain()
            .in_set(super::HubSet),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `models.js:flameBase setTier` — tier 1's innermost tongue keeps the base ember colour,
    /// tier 4's is the pale one, and the stack always brightens upward.
    #[test]
    fn tongue_gradient_warms_with_the_tier() {
        let t1 = tongue_emissive(1, 0);
        let t4 = tongue_emissive(4, 0);
        assert!(t4.green > t1.green && t4.blue > t1.blue, "{t1:?} {t4:?}");
        for tier in 1..=4 {
            let a = tongue_emissive(tier, 0);
            let b = tongue_emissive(tier, 3);
            assert!(b.blue > a.blue, "tier {tier}: the tip is paler");
        }
    }

    #[test]
    fn fire_scale_per_tier() {
        assert!((fire_scale(1) - 0.6).abs() < 1e-6);
        assert!((fire_scale(4) - 1.8).abs() < 1e-6);
    }

    /// `hub.js:836` — the flicker never leaves 0.80 … 1.0.
    #[test]
    fn flicker_stays_in_range() {
        for i in 0..2000 {
            let f = flicker(i as f32 * 0.017);
            assert!((0.79..=1.01).contains(&f), "{f}");
        }
    }
}
