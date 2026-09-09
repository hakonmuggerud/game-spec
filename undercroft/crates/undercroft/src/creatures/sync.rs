//! `hunter.js:syncMesh` (line ~934) plus every per-profile `anim` hook, applied to the entities
//! [`super::sync_roster`] built.
//!
//! The sim already ran the hooks: `undercroft_sim::creature::record::Anim` holds exactly what the
//! JS wrote onto THREE objects (eye intensity, leg phase and swing, jaw angle, ember, light,
//! glass, ripple, poses, sway, burst). This module is the other half — it never computes animation,
//! it only writes `Anim` onto transforms, visibilities, materials and lights:
//!
//! | `Anim` field | JS | here |
//! |---|---|---|
//! | `eye_k` | `syncMesh`: `eyes[i].material.emissiveIntensity` | `eyeL` / `eyeR` / `visor` emissive |
//! | `leg_phase`, `leg_swing` | `wardenAnim` / `bruteAnim`: `legs[i].rotation.x = ±a` | `legs0` / `legs1` parts |
//! | `jaw` | `drownerAnim`: `jaw.rotation.x` | the `jaw` part |
//! | `ember_k` | `lampwight.anim`: `ember.material.emissiveIntensity` | the `ember` box |
//! | `light_k`, `light_on` | Warden cone / false light `PointLight` | `SpotLight` / `PointLight` |
//! | `glass_k` | `falseLightAnim`: `glass.material.emissiveIntensity` | the `glass` box |
//! | `ripple_*`, `ring_y`, `body_shown` | `drownerAnim` | the `ripple` ring and the `body` part |
//! | `posed_dark` | `models.js:falseLight.setDark` | legs splayed, jaw down, glass cold, eyes shown |
//! | `plinth` | `wardenAnim` | the `plinth` box |
//! | `sway` | `bruteAnim`: `body.position.x` | the `body` part |
//! | `burst_*` | `fireBurst` / `stepBurst` | [`super::burst`] |
//!
//! One JS quirk is reproduced rather than fixed: `updateOne` runs the profile's `anim` hook *then*
//! `syncMesh`, so the Warden's `glass_k` (which `wardenAnim` writes onto the visor as
//! `light ÷ 2`) is overwritten by `eye_k` the same frame — its visor tracks `prof.eye[state]`, and
//! `glass_k` only really drives the false light's glass.

use bevy::prelude::*;
use undercroft_sim::creature::{Hunter, ProfileKind};

use super::burst::{self, BurstRng};
use super::model::{emissive_rgba, rgb_u32, BoxRef, ModelEntities, RIPPLE_COLOR};
use super::{Creature, HunterEntities, LUMENS_PER_CANDELA};
use crate::resources::{Game, ZoneRes};

/// The boxes `hunter.js` collects into `userData.eyes` — the Warden's "eyes" are its visor slit.
const EYE_BOXES: [&str; 3] = ["eyeL", "eyeR", "visor"];

/// The false light's glass while dark: "the glass goes cold, not just unlit"
/// (`models.js:falseLight.setDark`).
const GLASS_COLD: u32 = 0x1c1610;
/// `setDark`: the legs splay by this much (radians about Z) and the jaw drops by 0.9.
const SPLAY: f32 = 0.25;
const JAW_DROP: f32 = 0.9;

/// One frame of `syncMesh` for every record in `ZoneRes.hunters`.
#[allow(clippy::too_many_arguments)]
pub fn sync_creatures(
    zone: Res<ZoneRes>,
    game: Game,
    roster: Res<HunterEntities>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<BurstRng>,
    mut creatures: Query<&mut Creature>,
    mut transforms: Query<&mut Transform>,
    mut visibilities: Query<&mut Visibility>,
    mut spots: Query<&mut SpotLight>,
    mut points: Query<&mut PointLight>,
) {
    let (Some(zone), Some(asset)) = (zone.get(), game.get()) else {
        return;
    };
    for h in &zone.hunters {
        let Some(entity) = roster.get(h.id) else {
            continue;
        };
        let Ok(creature) = creatures.get_mut(entity) else {
            continue;
        };
        // disjoint borrows: the burst flag is written while the model is read
        let Creature {
            model,
            spot,
            point,
            burst_active,
            ..
        } = creature.into_inner();

        // `group.position.set(h.x, h.y, h.z)` / `rotation.y` — the Brute renders its turn-rate
        // limited yaw, everyone else the yaw motion gave them (`moveToward`: `atan2(-dx, -dz)`).
        let yaw = if h.profile == ProfileKind::Brute {
            h.yaw_vis
        } else {
            h.yaw
        };
        if let Ok(mut t) = transforms.get_mut(entity) {
            t.translation = Vec3::new(h.x, h.y, h.z);
            t.rotation = Quat::from_rotation_y(yaw);
            t.scale = model.scale;
        }
        set_visible(&mut visibilities, entity, h.active);

        // `syncMesh`: every eye gets `prof.eye[state]` (0.3 for an unlisted state), in the
        // profile's own colour — `buildMesh` passes `eyeColor` into the factory.
        let eye_color = asset.tuning.prof(h.profile).eye_color;
        for name in EYE_BOXES {
            if let Some(b) = model.named(name) {
                set_emissive(&mut materials, b, eye_color, h.anim.eye_k);
            }
        }
        set_legs(&mut transforms, model, h.anim.leg_swing, h.anim.leg_phase);

        match h.profile {
            ProfileKind::Base | ProfileKind::Fast => {}
            // the stolen flame in its chest, decaying through SATED
            ProfileKind::Lampwight => {
                if let Some(b) = model.named("ember") {
                    let color = b.emissive.unwrap_or(0xffb265);
                    set_emissive(&mut materials, b, color, h.anim.ember_k);
                }
            }
            ProfileKind::Warden => {
                if let Some(e) = *spot {
                    if let Ok(mut l) = spots.get_mut(e) {
                        l.intensity = h.anim.light_k * LUMENS_PER_CANDELA;
                    }
                    set_visible(&mut visibilities, e, h.anim.light_on);
                }
                // the plinth is the post's stone, not the Warden's
                if let Some(b) = model.named("plinth") {
                    set_visible(&mut visibilities, b.entity, h.anim.plinth);
                }
            }
            ProfileKind::Drowner => {
                sync_drowner(&mut transforms, &mut visibilities, &mut materials, model, h)
            }
            ProfileKind::FalseLight => {
                if let Some(e) = *point {
                    if let Ok(mut l) = points.get_mut(e) {
                        l.intensity = h.anim.light_k * LUMENS_PER_CANDELA;
                    }
                    set_visible(&mut visibilities, e, h.anim.light_on);
                }
                sync_false_light(&mut transforms, &mut visibilities, &mut materials, model, h);
            }
            ProfileKind::Brute => {
                // `bruteAnim`: the body sways ±0.06 u per stride, from its build-time pivot
                if let Some(p) = model.part("body") {
                    if let Ok(mut t) = transforms.get_mut(p.entity) {
                        t.translation.x = p.pivot.x + h.anim.sway;
                    }
                }
                // one burst per smash: `burst_fired` is set for one *fixed* tick, which can be
                // seen by several rendered frames
                if h.anim.burst_fired && !*burst_active {
                    *burst_active = true;
                    burst::fire(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &asset.data.models,
                        &asset.data.config,
                        &mut rng.0,
                        h.anim.burst_x,
                        h.anim.burst_z,
                    );
                } else if h.anim.burst_t <= 0.0 {
                    *burst_active = false;
                }
            }
        }
    }
}

/// `drownerAnim`: the gape, the sink (the root `y` the sim already wrote), the body hidden while
/// submerged, and the ripple ring left on the water surface.
fn sync_drowner(
    transforms: &mut Query<&mut Transform>,
    visibilities: &mut Query<&mut Visibility>,
    materials: &mut Assets<StandardMaterial>,
    model: &ModelEntities,
    h: &Hunter,
) {
    if let Some(p) = model.part("jaw") {
        if let Ok(mut t) = transforms.get_mut(p.entity) {
            t.rotation = p.rotation * Quat::from_rotation_x(h.anim.jaw);
        }
    }
    // `for (const c of h.group.children) if (c !== h.ring) c.visible = shown` — every box of the
    // drowner hangs off its `body` part.
    if let Some(p) = model.part("body") {
        set_visible(visibilities, p.entity, h.anim.body_shown);
    }
    let Some(ring) = model.named("ripple") else {
        return;
    };
    if let Ok(mut t) = transforms.get_mut(ring.entity) {
        t.translation.y = h.anim.ring_y;
        // the ring mesh is an annulus in its own XY plane, laid flat by the build rotation, so the
        // radius scales on local X/Y
        t.scale = Vec3::new(h.anim.ripple_scale, h.anim.ripple_scale, 1.0);
    }
    set_emissive(materials, ring, RIPPLE_COLOR, h.anim.ripple_k);
}

/// `falseLightAnim` + `models.js:falseLight.setDark` — the pose switch: legs splayed, jaw dropped,
/// glass off *and cold*, eyes shown.
fn sync_false_light(
    transforms: &mut Query<&mut Transform>,
    visibilities: &mut Query<&mut Visibility>,
    materials: &mut Assets<StandardMaterial>,
    model: &ModelEntities,
    h: &Hunter,
) {
    let dark = h.anim.posed_dark;
    for (i, name) in ["legs0", "legs1"].into_iter().enumerate() {
        if let Some(p) = model.part(name) {
            if let Ok(mut t) = transforms.get_mut(p.entity) {
                let splay = if dark { SPLAY } else { 0.0 };
                let sign = if i == 0 { 1.0 } else { -1.0 };
                t.rotation = p.rotation * Quat::from_rotation_z(splay * sign);
            }
        }
    }
    if let Some(p) = model.part("jaw") {
        if let Ok(mut t) = transforms.get_mut(p.entity) {
            let drop = if dark { JAW_DROP } else { 0.0 };
            t.rotation = p.rotation * Quat::from_rotation_x(drop);
        }
    }
    if let Some(b) = model.named("glass") {
        let color = b.emissive.unwrap_or(0xffc070);
        set_emissive(materials, b, color, h.anim.glass_k);
        set_base_color(materials, b, if dark { GLASS_COLD } else { b.color });
    }
    for name in ["eyeL", "eyeR"] {
        if let Some(b) = model.named(name) {
            set_visible(visibilities, b.entity, dark);
        }
    }
}

/// `wardenAnim` / `bruteAnim`: `legs[0].rotation.x = a`, `legs[1].rotation.x = -a` with
/// `a = swing · sin(phase)` — 0 while standing, which returns the legs to the build pose. Models
/// without hip pivots (hunter, lampwight, drowner) have nothing to swing; the false light has the
/// pivots but poses them about Z instead, and [`sync_false_light`] runs after this.
fn set_legs(transforms: &mut Query<&mut Transform>, model: &ModelEntities, swing: f32, phase: f32) {
    let a = swing * phase.sin();
    for (i, name) in ["legs0", "legs1"].into_iter().enumerate() {
        let Some(p) = model.part(name) else { continue };
        let Ok(mut t) = transforms.get_mut(p.entity) else {
            continue;
        };
        let sign = if i == 0 { 1.0 } else { -1.0 };
        t.rotation = p.rotation * Quat::from_rotation_x(a * sign);
    }
}

/// `mesh.visible = on` — `Visibility::Inherited` rather than `Visible`, so hiding a creature root
/// hides everything under it.
fn set_visible(visibilities: &mut Query<&mut Visibility>, entity: Entity, on: bool) {
    let Ok(mut v) = visibilities.get_mut(entity) else {
        return;
    };
    let want = if on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *v != want {
        *v = want;
    }
}

/// `material.emissiveIntensity = k` on a box's own material. Written only when it changes, so an
/// idle creature does not re-upload its materials every frame.
fn set_emissive(materials: &mut Assets<StandardMaterial>, b: &BoxRef, color: u32, k: f32) {
    let want = emissive_rgba(color, k);
    if materials.get(&b.material).map(|m| m.emissive) == Some(want) {
        return;
    }
    if let Some(mut m) = materials.get_mut(&b.material) {
        m.emissive = want;
    }
}

/// `material.color.set(...)` — the false light's glass going cold.
fn set_base_color(materials: &mut Assets<StandardMaterial>, b: &BoxRef, color: u32) {
    let want = rgb_u32(color);
    if materials.get(&b.material).map(|m| m.base_color) == Some(want) {
        return;
    }
    if let Some(mut m) = materials.get_mut(&b.material) {
        m.base_color = want;
    }
}
