//! Hub lighting that is not the great flame: the alcove sconces (`world.js:303 buildSconces`,
//! `SCONCE_SPOTS_V2`), the hanging lanterns lit per flame tier (`HUB_LANTERNS_V2`,
//! `models.js:hangLanternBoxes`) and the tier rules of `world.js:317 setAlcoveTier`.
//!
//! The prototype kept five real `PointLight`s among the lanterns plus one per sconce and culled
//! them all while a zone ran (`world.js:setHubLights`); Bevy's clustered lighting makes the cull
//! unnecessary (HANDOFF §6), so the lights simply stay where they are and go dark below their tier.

use bevy::prelude::*;
use undercroft_data::models::place_boxes;
use undercroft_sim::grid::{center, in_bounds, is_solid};

use crate::resources::{Game, HubMapRes, HubRes};
use crate::tick::Clock;

use super::LUMENS_PER_JS_INTENSITY;
use crate::model::{self, spawn_box, BoxAssets};
use crate::world::palette;

/* ============================================================
Tables
============================================================ */

/// One `world.js:30 SCONCE_SPOTS_V2` row: an alcove wall sconce.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SconceSpot {
    pub cx: i32,
    pub cz: i32,
    /// Which way the lamp faces off the wall (±1).
    pub side: f32,
    /// The flame tier that lights it.
    pub min_tier: u32,
}

/// `world.js:30 SCONCE_SPOTS_V2` — tier 2 lights the W/E alcoves, tier 3 the N corner rooms.
pub const SCONCE_SPOTS_V2: [SconceSpot; 4] = [
    SconceSpot {
        cx: 1,
        cz: 7,
        side: 1.0,
        min_tier: 2,
    },
    SconceSpot {
        cx: 23,
        cz: 7,
        side: -1.0,
        min_tier: 2,
    },
    SconceSpot {
        cx: 1,
        cz: 2,
        side: 1.0,
        min_tier: 3,
    },
    SconceSpot {
        cx: 23,
        cz: 2,
        side: -1.0,
        min_tier: 3,
    },
];

/// `world.js:29 SCONCE_SPOTS_V1` — the v1 17×9 hub's pair.
pub const SCONCE_SPOTS_V1: [SconceSpot; 2] = [
    SconceSpot {
        cx: 1,
        cz: 3,
        side: 1.0,
        min_tier: 2,
    },
    SconceSpot {
        cx: 15,
        cz: 3,
        side: -1.0,
        min_tier: 2,
    },
];

/// One `world.js:60 HUB_LANTERNS_V2` row: a lantern hung from the ceiling, lit from `tier`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LanternSpot {
    pub cx: i32,
    pub cz: i32,
    pub tier: u32,
    /// Carries a real `PointLight` (five of the twelve do).
    pub light: bool,
}

/// `world.js:60 HUB_LANTERNS_V2` — the stairs/board first, then the alcove mouths, then the
/// corner-room doors and the hearth, then the whole camp.
pub const HUB_LANTERNS_V2: [LanternSpot; 12] = [
    LanternSpot {
        cx: 13,
        cz: 8,
        tier: 1,
        light: true,
    },
    LanternSpot {
        cx: 6,
        cz: 7,
        tier: 2,
        light: true,
    },
    LanternSpot {
        cx: 16,
        cz: 6,
        tier: 2,
        light: true,
    },
    LanternSpot {
        cx: 1,
        cz: 5,
        tier: 3,
        light: true,
    },
    LanternSpot {
        cx: 23,
        cz: 5,
        tier: 3,
        light: true,
    },
    LanternSpot {
        cx: 8,
        cz: 5,
        tier: 3,
        light: false,
    },
    LanternSpot {
        cx: 14,
        cz: 5,
        tier: 3,
        light: false,
    },
    LanternSpot {
        cx: 3,
        cz: 7,
        tier: 4,
        light: false,
    },
    LanternSpot {
        cx: 20,
        cz: 7,
        tier: 4,
        light: false,
    },
    LanternSpot {
        cx: 3,
        cz: 2,
        tier: 4,
        light: false,
    },
    LanternSpot {
        cx: 20,
        cz: 2,
        tier: 4,
        light: false,
    },
    LanternSpot {
        cx: 9,
        cz: 8,
        tier: 4,
        light: false,
    },
];

/// `world.js:317 setAlcoveTier` — which lanterns are visible at a flame tier.
pub fn lit_lanterns(tier: u32) -> Vec<(i32, i32)> {
    HUB_LANTERNS_V2
        .iter()
        .filter(|l| tier >= l.tier)
        .map(|l| (l.cx, l.cz))
        .collect()
}

/// `world.js:317 setAlcoveTier` — a sconce's `PointLight` intensity at a flame tier: its own
/// `SCONCE_INT[tier - 1]` when lit, the banked-ember `SCONCE_INT[0]` below its alcove's tier.
pub fn sconce_intensity(sconce_int: &[f32; 4], min_tier: u32, tier: u32) -> f32 {
    if tier >= min_tier {
        sconce_int[(tier.clamp(1, 4) - 1) as usize]
    } else {
        sconce_int[0]
    }
}

/// `world.js:317` — the sconce cup's `emissiveIntensity`.
pub fn sconce_cup_k(min_tier: u32, tier: u32) -> f32 {
    if tier >= min_tier {
        0.3 + 0.2 * (tier as f32 - 2.0)
    } else {
        0.08
    }
}

/* ============================================================
Components
============================================================ */

/// A wall sconce (`world.js:buildSconces`): the emissive cup plus its light.
#[derive(Component, Debug)]
pub struct Sconce {
    pub min_tier: u32,
    /// `Math.random() * 6` in the JS; deterministic here (the spot index).
    pub phase: f32,
    pub cup: Entity,
    pub light: Entity,
}

/// A hanging lantern (`world.js:buildProps`' per-tier merged groups, one entity per lantern here).
#[derive(Component, Debug)]
pub struct HangLantern {
    pub tier: u32,
    pub phase: f32,
    /// The `glass` box — its own material, so the group flicker is per-lantern.
    pub glass: Option<Entity>,
    /// The `PointLight`, on the five keyed lanterns.
    pub light: Option<Entity>,
}

/* ============================================================
Systems
============================================================ */

/// `world.js:303 buildSconces` + the lantern half of `world.js:253 buildProps`, once.
#[allow(clippy::too_many_arguments)]
fn spawn_lights(
    mut commands: Commands,
    game: Game,
    hub_map: Res<HubMapRes>,
    mut assets: ResMut<BoxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    sconces: Query<Entity, With<Sconce>>,
    lanterns: Query<Entity, With<HangLantern>>,
) {
    if !sconces.is_empty() || !lanterns.is_empty() {
        return;
    }
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let Some(hm) = hub_map.0.as_ref() else {
        return;
    };
    let m = &hm.map;

    // --- sconces ---
    let spots: &[SconceSpot] = if m.w >= 25 {
        &SCONCE_SPOTS_V2
    } else {
        &SCONCE_SPOTS_V1
    };
    for (i, s) in spots.iter().enumerate() {
        if !in_bounds(m, s.cx, s.cz) || is_solid(m, s.cx, s.cz) {
            continue;
        }
        let (px, pz) = center(m, s.cx, s.cz);
        let root = commands
            .spawn((
                Name::new(format!("hub:sconce:{},{}", s.cx, s.cz)),
                Transform::from_xyz(px - s.side * 0.35, 0.0, pz),
                Visibility::Inherited,
            ))
            .id();
        // `new THREE.BoxGeometry(0.22, 0.22, 0.22)` at y 1.7, colour 0x1a1210, emissive 0xff7a30.
        let cup = commands
            .spawn((
                Name::new("cup"),
                Mesh3d(assets.mesh(&mut meshes, 0.22, 0.22, 0.22)),
                MeshMaterial3d(mats.add(model::box_material(0x1a1210, Some(0xff7a30), 0.0))),
                Transform::from_xyz(0.0, 1.7, 0.0),
                ChildOf(root),
            ))
            .id();
        let light = commands
            .spawn((
                Name::new("light"),
                PointLight {
                    color: palette::rgb(0xffa040),
                    intensity: 0.0,
                    range: 7.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(s.side * 0.3, 1.9, 0.0),
                ChildOf(root),
            ))
            .id();
        commands.entity(root).insert(Sconce {
            min_tier: s.min_tier,
            phase: i as f32 * 1.9,
            cup,
            light,
        });
    }

    // --- hanging lanterns ---
    let Some(boxes) = data.models.props.get("hangLantern") else {
        error!("hub: models.ron has no hangLantern prop boxes");
        return;
    };
    let ll = &data.config.hub_warmth.lantern_light;
    for (i, l) in HUB_LANTERNS_V2.iter().enumerate() {
        if !in_bounds(m, l.cx, l.cz) || is_solid(m, l.cx, l.cz) {
            continue;
        }
        let (px, pz) = center(m, l.cx, l.cz);
        let root = commands
            .spawn((
                Name::new(format!("hub:lantern:{},{}", l.cx, l.cz)),
                Transform::from_xyz(px, 0.0, pz),
                Visibility::Hidden,
            ))
            .id();
        let mut glass = None;
        for b in place_boxes(boxes, 0.0, 0.0, 0.0) {
            let e = spawn_box(&mut commands, &mut assets, &mut meshes, &mut mats, root, &b);
            if b.name.as_deref() == Some("glass") {
                // Its own material: `world.js:update` flickers the glow per lantern.
                let h = mats.add(model::box_material(b.color, b.emissive, b.emissive_k));
                commands.entity(e).insert(MeshMaterial3d(h));
                glass = Some(e);
            }
        }
        let light = l.light.then(|| {
            commands
                .spawn((
                    Name::new("light"),
                    PointLight {
                        color: palette::rgb(ll.color),
                        intensity: 0.0,
                        range: ll.dist,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    // The JS light is a sibling of the merged group, always visible, at y 2.2.
                    Transform::from_xyz(px, 2.2, pz),
                ))
                .id()
        });
        commands.entity(root).insert(HangLantern {
            tier: l.tier,
            phase: i as f32 * 0.83,
            glass,
            light,
        });
    }
    info!(
        "hub: {} sconce(s), {} hanging lantern(s)",
        spots.len(),
        HUB_LANTERNS_V2.len()
    );
}

/// `world.js:317 setAlcoveTier` + the sconce/lantern half of `world.js:565 update` — tier
/// visibility and the flicker.
#[allow(clippy::too_many_arguments)]
fn update_lights(
    game: Game,
    hub: Res<HubRes>,
    clock: Res<Clock>,
    sconces: Query<&Sconce>,
    lanterns: Query<(&HangLantern, &mut Visibility)>,
    mut lights: Query<&mut PointLight>,
    handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let tier = hub.0.tier;
    let time = clock.time;
    let w = &data.config.hub_warmth;

    for s in sconces.iter() {
        let base = sconce_intensity(&data.config.sconce_int, s.min_tier, tier);
        if let Ok(mut l) = lights.get_mut(s.light) {
            l.intensity =
                base * (0.93 + 0.07 * (time * 11.0 + s.phase).sin()) * LUMENS_PER_JS_INTENSITY;
        }
        if let Ok(h) = handles.get(s.cup) {
            if let Some(mut m) = mats.get_mut(&h.0) {
                m.emissive = palette::emissive(0xff7a30, sconce_cup_k(s.min_tier, tier));
            }
        }
    }

    for (l, mut vis) in lanterns {
        let on = tier >= l.tier;
        *vis = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let f = 0.9 + 0.08 * (time * 9.0 + l.phase).sin() + 0.02 * (time * 23.0 + l.phase).sin();
        if let Some(g) = l.glass {
            if let Ok(h) = handles.get(g) {
                if let Some(mut m) = mats.get_mut(&h.0) {
                    // `world.js:579` — the merged glow material's colour scales with the flicker.
                    m.emissive = palette::emissive(
                        0xffc070,
                        if on { 0.85 * w.lantern_glow * f } else { 0.0 },
                    );
                }
            }
        }
        if let Some(le) = l.light {
            if let Ok(mut pl) = lights.get_mut(le) {
                pl.intensity = if on {
                    w.lantern_light.int * f * LUMENS_PER_JS_INTENSITY
                } else {
                    0.0
                };
            }
        }
    }
}

/// The sconces and hanging lanterns.
pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (spawn_lights, update_lights).chain().in_set(super::HubSet),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `world.js:60 HUB_LANTERNS_V2` + `setAlcoveTier`: the tier → lit-lantern table.
    #[test]
    fn tier_to_lit_lanterns() {
        assert_eq!(lit_lanterns(0).len(), 0);
        assert_eq!(lit_lanterns(1), vec![(13, 8)]);
        assert_eq!(lit_lanterns(2), vec![(13, 8), (6, 7), (16, 6)]);
        assert_eq!(
            lit_lanterns(3),
            vec![(13, 8), (6, 7), (16, 6), (1, 5), (23, 5), (8, 5), (14, 5)]
        );
        assert_eq!(lit_lanterns(4).len(), 12, "tier 4 lights the whole camp");
        assert_eq!(
            HUB_LANTERNS_V2.iter().filter(|l| l.light).count(),
            5,
            "five keyed lanterns carry a real PointLight"
        );
    }

    /// `world.js:317` — a sconce below its alcove's tier keeps the banked-ember glow.
    #[test]
    fn sconce_intensity_table() {
        let int = [0.45_f32, 1.6, 3.2, 5.0];
        // The W/E alcoves (minTier 2).
        assert_eq!(sconce_intensity(&int, 2, 1), 0.45);
        assert_eq!(sconce_intensity(&int, 2, 2), 1.6);
        assert_eq!(sconce_intensity(&int, 2, 4), 5.0);
        // The N corner rooms (minTier 3) stay banked at tier 2.
        assert_eq!(sconce_intensity(&int, 3, 2), 0.45);
        assert_eq!(sconce_intensity(&int, 3, 3), 3.2);
        assert!((sconce_cup_k(2, 2) - 0.3).abs() < 1e-6);
        assert!((sconce_cup_k(2, 4) - 0.7).abs() < 1e-6);
        assert!((sconce_cup_k(3, 2) - 0.08).abs() < 1e-6);
    }
}
