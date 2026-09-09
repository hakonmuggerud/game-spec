//! Loose items (`world.js:spawnItem` / `resetItems` / the `update` bob) — one entity per
//! `Zone.items` record, diffed by index every frame so a pickup or a death bundle needs no rebuild.
//!
//! Item meshes are the `models.js` factories, feet-origin, hovering at `ITEM_Y[kind]` and bobbing
//! `±0.08` at `sin(time·2 + phase)` while spinning `dt · 0.8` (`world.js:update`).

use bevy::prelude::*;
use undercroft_data::ItemKind;

use crate::resources::{Game, ZoneRes};
use crate::tick::Clock;

use super::model::{build_model, spawn_groups};

/// One spawned item, keyed by its index in `Zone.items`.
#[derive(Component, Debug, Clone, Copy)]
pub struct WorldItemView {
    pub index: usize,
    pub kind: ItemKind,
    pub x: f32,
    pub z: f32,
    /// `it.baseY` — `ITEM_Y[kind]`.
    pub base_y: f32,
    /// `it.phase` — the bob offset; hashed from the index and position so it is reproducible.
    pub phase: f32,
}

/// `world.js:ITEM_Y` — the height each kind hovers at (the bob adds ±0.08).
pub fn item_y(kind: ItemKind) -> f32 {
    match kind {
        ItemKind::Oil => 0.12,
        ItemKind::Relic => 0.14,
        ItemKind::Rich => 0.14,
        ItemKind::Bundle => 0.02,
        ItemKind::Quest => 0.1,
    }
}

/// `models.makeModel(kind)` — the registry name behind an item kind (`oil` → `flask`,
/// `rich` → `richRelic`).
pub fn item_model(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Oil => "flask",
        ItemKind::Relic => "relic",
        ItemKind::Rich => "richRelic",
        ItemKind::Bundle => "bundle",
        ItemKind::Quest => "quest",
    }
}

/// A stable stand-in for `Math.random() * 6.28`.
fn phase_of(index: usize, x: f32, z: f32) -> f32 {
    // `jitter(.., 1.0)` spreads over 0 … 2, so a half turn each makes a full 0 … 2π phase.
    super::palette::jitter(
        (x * 16.0) as i32,
        (z * 16.0) as i32,
        index as u32 + 977,
        1.0,
    ) * std::f32::consts::PI
}

/// Spawn / despawn item entities so they mirror `Zone.items` exactly.
pub(super) fn sync_items(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    game: Game,
    zone: Res<ZoneRes>,
    existing: Query<(Entity, &WorldItemView)>,
    scene: Query<Entity, With<super::ZoneScene>>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let items: &[crate::resources::WorldItem] = match zone.get() {
        Some(z) => &z.items,
        None => &[],
    };
    let Ok(root) = scene.single() else {
        // no zone scene: drop whatever is left over
        for (e, _) in &existing {
            commands.entity(e).despawn();
        }
        return;
    };

    let mut seen = vec![false; items.len()];
    for (e, view) in &existing {
        match items.get(view.index) {
            Some(it)
                if it.kind == view.kind
                    && (it.x - view.x).abs() < 1e-4
                    && (it.z - view.z).abs() < 1e-4 =>
            {
                seen[view.index] = true;
            }
            _ => commands.entity(e).despawn(),
        }
    }
    for (i, it) in items.iter().enumerate() {
        if seen[i] {
            continue;
        }
        let base_y = item_y(it.kind);
        let e = commands
            .spawn((
                Name::new(format!("item:{}", item_model(it.kind))),
                WorldItemView {
                    index: i,
                    kind: it.kind,
                    x: it.x,
                    z: it.z,
                    base_y,
                    phase: phase_of(i, it.x, it.z),
                },
                Transform::from_xyz(it.x, base_y, it.z),
                Visibility::default(),
            ))
            .id();
        commands.entity(root).add_child(e);
        if let Some(def) = asset.data.models.get(item_model(it.kind)) {
            spawn_groups(
                &mut commands,
                &mut meshes,
                &mut materials,
                e,
                build_model(def, &[]),
            );
        }
    }
}

/// `world.js:update` — bob and spin.
pub(super) fn animate_items(
    time: Res<Time>,
    clock: Res<Clock>,
    mut q: Query<(&mut Transform, &WorldItemView)>,
) {
    for (mut tf, it) in &mut q {
        tf.translation.y = it.base_y + 0.08 * (clock.time * 2.0 + it.phase).sin();
        tf.rotate_y(time.delta_secs() * 0.8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_heights_and_models_match_the_js_tables() {
        assert_eq!(item_y(ItemKind::Oil), 0.12);
        assert_eq!(item_y(ItemKind::Relic), 0.14);
        assert_eq!(item_y(ItemKind::Rich), 0.14);
        assert_eq!(item_y(ItemKind::Bundle), 0.02);
        assert_eq!(item_y(ItemKind::Quest), 0.1);
        assert_eq!(item_model(ItemKind::Oil), "flask");
        assert_eq!(item_model(ItemKind::Rich), "richRelic");
        assert_eq!(item_model(ItemKind::Quest), "quest");
    }

    #[test]
    fn phases_are_stable_and_spread() {
        assert_eq!(phase_of(0, 1.5, 2.5), phase_of(0, 1.5, 2.5));
        assert_ne!(phase_of(0, 1.5, 2.5), phase_of(1, 1.5, 2.5));
        for i in 0..16 {
            let p = phase_of(i, 3.5, 4.5);
            assert!((0.0..=6.29).contains(&p), "{p}");
        }
    }
}
