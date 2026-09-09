//! NPC visuals (`npc.js:ensureMesh` / `show` / `sync`, and the resident idle of `hub.js:833
//! update`). Residents standing in the hub, captives waiting in a zone and the follower walking
//! behind the player are all the same `NpcRecord`; the sim owns their position, yaw and bob
//! (`follower::Npcs::update_hub` / `update_zone`), and this module mirrors them onto entities.
//!
//! One entity per record, keyed by id and diffed against `Npcs::present()` every frame: a record
//! that leaves the list is hidden, not despawned, exactly as `npc.js:hideAll` hid the group.

use bevy::prelude::*;
use undercroft_data::models::BoxDef;
use undercroft_data::tables::NpcDef;
use undercroft_sim::follower::NpcState;

use crate::resources::{Game, Npcs};
use crate::tick::Clock;

use crate::model::{self, spawn_model, BoxAssets};

/// One NPC's mesh (`npc.js:ensureMesh` — `models.npc(id)`, or a body/head/hat box fallback).
#[derive(Component, Debug)]
pub struct NpcVisual {
    pub id: String,
    /// `r.phase` at spawn, so the idle stays in step with the sim's bob.
    pub phase: f32,
    /// The animated boxes of `hub.js`'s resident idle.
    pub head: Option<Entity>,
    pub arm_l: Option<Entity>,
    pub arm_r: Option<Entity>,
    pub coat: Option<Entity>,
    /// Their transforms as built, so the idle composes rather than accumulates.
    pub head_base: Transform,
    pub arm_l_base: Transform,
    pub arm_r_base: Transform,
    pub coat_base: Transform,
}

/// `npc.js:64 ensureMesh` fallback — a coat-coloured body, a head and a hat.
fn fallback_boxes(def: &NpcDef) -> Vec<BoxDef> {
    let mk = |y: f32, w: f32, h: f32, d: f32, color: u32, name: &str| BoxDef {
        x: 0.0,
        y,
        z: 0.0,
        w,
        h,
        d,
        color,
        emissive: None,
        emissive_k: 0.0,
        name: Some(name.to_string()),
        ry: 0.0,
        part: None,
        hidden: false,
    };
    vec![
        mk(0.0, 0.44, 0.9, 0.3, def.coat, "coat"),
        mk(0.9, 0.3, 0.3, 0.3, 0xd9b08c, "head"),
        mk(1.2, 0.34, 0.12, 0.34, def.hat, "hat"),
    ]
}

/// `npc.js:78 show` / `sync` — spawn what is missing, place and reveal what is present, hide the
/// rest.
#[allow(clippy::too_many_arguments)]
fn sync_npcs(
    mut commands: Commands,
    game: Game,
    npcs: Res<Npcs>,
    mut assets: ResMut<BoxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut drawn: Query<(Entity, &NpcVisual, &mut Transform, &mut Visibility)>,
) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let present = npcs.0.present();

    // Hide anything that left the list; move and show what is still there.
    for (_, v, mut t, mut vis) in drawn.iter_mut() {
        match present.iter().find(|r| r.id == v.id) {
            Some(r) if r.place.is_some() && r.state != NpcState::Idle => {
                t.translation = Vec3::new(r.x, r.y, r.z);
                t.rotation = Quat::from_rotation_y(r.yaw);
                *vis = Visibility::Inherited;
            }
            _ => *vis = Visibility::Hidden,
        }
    }

    for r in &present {
        if drawn.iter().any(|(_, v, _, _)| v.id == r.id) {
            continue;
        }
        let Some(def) = data.npcs.npcs.get(&r.id) else {
            continue;
        };
        let at = Transform::from_xyz(r.x, r.y, r.z).with_rotation(Quat::from_rotation_y(r.yaw));
        // `models.js` has no factory for some ids: the JS box fallback, built through the same
        // path as a recorded model so the lookups below work either way.
        let fallback: Option<undercroft_data::ModelDef> =
            data.models.get(&r.id).is_none().then(|| {
                warn!("hub: models.ron has no npc model {:?}; using boxes", r.id);
                undercroft_data::ModelDef {
                    name: r.id.clone(),
                    height: 1.7,
                    box_count: 0,
                    scale: [1.0, 1.0, 1.0],
                    parts: Vec::new(),
                    boxes: fallback_boxes(def),
                    extras: Vec::new(),
                }
            });
        let model = fallback
            .as_ref()
            .or_else(|| data.models.get(&r.id))
            .expect("either the recorded model or the fallback");
        let built = spawn_model(
            &mut commands,
            Some(&mut assets),
            &mut meshes,
            &mut mats,
            model,
            at,
        );
        // The base transform of an animated box comes from the same box list it was built
        // from, so the idle composes onto the pose instead of replacing it.
        let bases: Vec<BoxDef> = match data.models.get(&r.id) {
            Some(model) => model.boxes.clone(),
            None => fallback_boxes(def),
        };
        let base_of = |name: &str| -> Transform {
            bases
                .iter()
                .find(|b| b.name.as_deref() == Some(name))
                .map(model::box_transform)
                .unwrap_or(Transform::IDENTITY)
        };
        let head = built.named_entity("head");
        let arm_l = built.named_entity("armL");
        let arm_r = built.named_entity("armR");
        let coat = built.named_entity("coat");
        info!(
            "hub: npc {} at ({:.1}, {:.1}), {:?}",
            r.id, r.x, r.z, r.state
        );
        commands.entity(built.root).insert((
            NpcVisual {
                id: r.id.clone(),
                phase: r.phase,
                head,
                arm_l,
                arm_r,
                coat,
                head_base: base_of("head"),
                arm_l_base: base_of("armL"),
                arm_r_base: base_of("armR"),
                coat_base: base_of("coat"),
            },
            Name::new(format!("npc:{}", r.id)),
        ));
    }
}

/// `hub.js:846` — the resident idle: the head turns, the arms sway, the coat breathes. The body
/// bob (`r.y`) and the turn-to-player (`r.yaw`) are the sim's, applied in [`sync_npcs`].
fn animate_npcs(
    clock: Res<Clock>,
    npcs: Res<Npcs>,
    visuals: Query<&NpcVisual>,
    mut xf: Query<&mut Transform>,
) {
    let time = clock.time;
    for v in visuals.iter() {
        let Some(r) = npcs.0.get(&v.id) else { continue };
        if r.place != Some(undercroft_sim::follower::Place::Hub) {
            continue;
        }
        let ph = v.phase;
        if let Some(e) = v.head {
            if let Ok(mut t) = xf.get_mut(e) {
                *t = v.head_base;
                t.rotation *= Quat::from_rotation_y(
                    0.14 * (time * 0.7 + ph).sin() + 0.05 * (time * 2.3 + ph).sin(),
                );
            }
        }
        if let Some(e) = v.arm_l {
            if let Ok(mut t) = xf.get_mut(e) {
                *t = v.arm_l_base;
                t.rotation *= Quat::from_rotation_x(0.06 * (time * 1.1 + ph).sin());
            }
        }
        if let Some(e) = v.arm_r {
            if let Ok(mut t) = xf.get_mut(e) {
                *t = v.arm_r_base;
                t.rotation *= Quat::from_rotation_x(-0.06 * (time * 1.1 + ph + 0.4).sin());
            }
        }
        if let Some(e) = v.coat {
            if let Ok(mut t) = xf.get_mut(e) {
                *t = v.coat_base;
                t.scale.z = 1.0 + 0.03 * (time * 1.6 + ph).sin();
            }
        }
    }
}

/// The NPC meshes and their idle.
pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (sync_npcs, animate_npcs).chain().in_set(super::HubSet),
    );
}

#[cfg(test)]
mod tests {
    use undercroft_data::GameData;

    /// Every NPC in `npcs.ron` has its own `models.js` factory in `models.ron`, so the box
    /// fallback of `npc.js:ensureMesh` never fires.
    #[test]
    fn every_npc_has_a_model() {
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        for id in &data.npcs.order {
            let m = data
                .models
                .get(id)
                .unwrap_or_else(|| panic!("models.ron has no {id}"));
            assert!(
                m.named("coat").is_some() && m.named("head").is_some(),
                "{id} needs a coat and a head to animate"
            );
        }
    }

    /// `npcs.ron` keeps the coat colour `npc.js:ensureMesh` falls back to.
    #[test]
    fn coats_are_distinct() {
        let data = GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads");
        let coats: Vec<u32> = data
            .npcs
            .order
            .iter()
            .filter_map(|id| data.npcs.npcs.get(id))
            .map(|d| d.coat)
            .collect();
        let mut sorted = coats.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), coats.len(), "each resident reads differently");
    }
}
