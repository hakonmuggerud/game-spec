//! Zone furniture: gates (`X`), shortcut doors (`=`), the stairs / elevator marker (`S` / `V`) and
//! the altar (`A`). Ports `world.js:buildStairsMarker`, the gate and shortcut blocks of `loadZone`
//! and `addShortcutMesh` / `openShortcut`'s mesh swap.
//!
//! Gates and shortcuts are the only zone furniture that changes during a run, so they are diffed
//! against `Zone.doors` every frame: an opened gate's bars are despawned, an opened shortcut's
//! barred model is swapped for `shortcutOpen`.

use bevy::prelude::*;
use undercroft_data::zone::Facing;
use undercroft_data::ParsedMap;

use crate::resources::{Game, ZoneRes};

use super::model::{build_model, spawn_groups};
use super::palette::{emissive, lambert, rgb};

/// One gate's bars, by index into `ParsedMap::gates`.
#[derive(Component, Debug, Clone, Copy)]
pub struct GateBars(pub usize);

/// One shortcut door, by index into `ParsedMap::shortcuts`; `open` is the state the mesh shows.
#[derive(Component, Debug, Clone, Copy)]
pub struct ShortcutDoor {
    pub index: usize,
    pub open: bool,
}

/// `SHORTCUT_YAW` — the model's front (−Z) faces the `openFrom` side.
pub fn shortcut_yaw(from: Facing) -> f32 {
    match from {
        Facing::N => 0.0,
        Facing::S => std::f32::consts::PI,
        Facing::E => -std::f32::consts::FRAC_PI_2,
        Facing::W => std::f32::consts::FRAC_PI_2,
    }
}

/// `loadZone`'s gate rule — the bars face across the narrower opening, so a wall running
/// north–south turns them a quarter turn.
pub fn gate_yaw(map: &ParsedMap, cx: i32, cz: i32) -> f32 {
    let solid = |dz: i32| map.cell_type(cx, cz + dz).is_solid();
    if solid(-1) && solid(1) {
        std::f32::consts::FRAC_PI_2
    } else {
        0.0
    }
}

/// Everything `loadZone` puts in the scene besides the blocks and the water: the closed gates, the
/// shortcut doors, the stairs / elevator marker and the altar.
pub(super) fn spawn_zone_features(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &Game,
    root: Entity,
    map: &ParsedMap,
    doors: &undercroft_sim::world::ZoneDoors,
) {
    let models = &game.data().models;

    for (i, g) in map.gates.iter().enumerate() {
        if doors.gate_open(i) {
            continue;
        }
        let bars = commands
            .spawn((
                Name::new(format!("gate:{},{}", g.marker.cx, g.marker.cz)),
                GateBars(i),
                Transform::from_xyz(g.marker.x, 0.0, g.marker.z).with_rotation(
                    Quat::from_rotation_y(gate_yaw(map, g.marker.cx, g.marker.cz)),
                ),
                Visibility::default(),
            ))
            .id();
        commands.entity(root).add_child(bars);
        if let Some(def) = models.get("gate") {
            spawn_groups(commands, meshes, materials, bars, build_model(def, &[]));
        }
    }

    for (i, s) in map.shortcuts.iter().enumerate() {
        let open = doors.shortcut_open(i);
        let door = commands
            .spawn((
                Name::new(format!(
                    "shortcut:{}:{},{}",
                    s.id().unwrap_or("?"),
                    s.marker.cx,
                    s.marker.cz
                )),
                ShortcutDoor { index: i, open },
                Transform::from_xyz(s.marker.x, 0.0, s.marker.z).with_rotation(
                    Quat::from_rotation_y(s.open_from().map(shortcut_yaw).unwrap_or(0.0)),
                ),
                Visibility::default(),
            ))
            .id();
        commands.entity(root).add_child(door);
        let name = if open {
            "shortcutOpen"
        } else {
            "shortcutBarred"
        };
        if let Some(def) = models.get(name) {
            spawn_groups(commands, meshes, materials, door, build_model(def, &[]));
        }
    }

    if let Some(st) = map.stairs {
        // the flat glowing sill under the player's feet at spawn
        let sill = commands
            .spawn((
                Name::new("stairsMarker"),
                Mesh3d(meshes.add(Cuboid::new(0.9, 0.06, 0.9))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    emissive: emissive(0x2a4a9a, 0.55),
                    ..lambert(rgb(0x1a2230))
                })),
                Transform::from_xyz(st.marker.x, 0.03, st.marker.z),
            ))
            .id();
        commands.entity(root).add_child(sill);

        if st.kind == undercroft_data::zone::EntryKind::Elevator {
            let cage = commands
                .spawn((
                    Name::new("elevator"),
                    Transform::from_xyz(st.marker.x, 0.0, st.marker.z),
                    Visibility::default(),
                ))
                .id();
            commands.entity(root).add_child(cage);
            if let Some(def) = models.get("elevator") {
                spawn_groups(
                    commands,
                    meshes,
                    materials,
                    cage,
                    build_model(def, &["lamp"]),
                );
            }
            let light = commands
                .spawn((
                    Name::new("elevator:light"),
                    PointLight {
                        color: rgb(0xffc070),
                        intensity: 1.2,
                        range: 5.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    // `models.elevator().userData.lightY`
                    Transform::from_xyz(st.marker.x, 2.4, st.marker.z),
                ))
                .id();
            commands.entity(root).add_child(light);
        } else {
            let stairs = commands
                .spawn((
                    Name::new("stairs"),
                    Transform::from_xyz(st.marker.x, 0.0, st.marker.z).with_scale(Vec3::splat(0.8)),
                    Visibility::default(),
                ))
                .id();
            commands.entity(root).add_child(stairs);
            if let Some(def) = models.get("stairs") {
                spawn_groups(
                    commands,
                    meshes,
                    materials,
                    stairs,
                    build_model(def, &["rune"]),
                );
            }
        }
    }

    if let Some(a) = map.altar {
        let altar = commands
            .spawn((
                Name::new("altar"),
                Transform::from_xyz(a.x, 0.0, a.z),
                Visibility::default(),
            ))
            .id();
        commands.entity(root).add_child(altar);
        if let Some(def) = models.get("altar") {
            spawn_groups(commands, meshes, materials, altar, build_model(def, &[]));
        }
    }
}

/// `buildStairsMarker(m, group, {hub: true})` — the hub's way down. `models.stairsDown` is not in
/// `models.ron` (the exporter only walked `MODELS`), so the port keeps the cool landing lamp that
/// makes a tier-1 arrival readable and leaves the stairwell itself as the open hole `blocks.rs`
/// cuts.
pub(super) fn spawn_hub_stairs(commands: &mut Commands, root: Entity, map: &ParsedMap) {
    let Some(st) = map.stairs else {
        return;
    };
    let light = commands
        .spawn((
            Name::new("hub:landingLight"),
            PointLight {
                color: rgb(0x9ab0ff),
                intensity: 0.9,
                range: 4.5,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(st.marker.x, 1.2, st.marker.z - 0.4),
        ))
        .id();
    commands.entity(root).add_child(light);
}

/// `openGate` removes the bars for the run; `openShortcut` swaps the barred model for the open one.
pub(super) fn sync_doors(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    game: Game,
    zone: Res<ZoneRes>,
    gates: Query<(Entity, &GateBars)>,
    mut shortcuts: Query<(Entity, &mut ShortcutDoor, &Children)>,
) {
    let (Some(z), Some(asset)) = (zone.get(), game.get()) else {
        return;
    };
    for (e, g) in &gates {
        if z.doors.gate_open(g.0) {
            commands.entity(e).despawn();
        }
    }
    for (e, mut door, children) in &mut shortcuts {
        let open = z.doors.shortcut_open(door.index);
        if open == door.open {
            continue;
        }
        door.open = open;
        for c in children.iter() {
            commands.entity(c).despawn();
        }
        let name = if open {
            "shortcutOpen"
        } else {
            "shortcutBarred"
        };
        if let Some(def) = asset.data.models.get(name) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::map::parse_map;

    fn map(rows: &[&str]) -> ParsedMap {
        parse_map(
            &rows.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
            0,
            "t",
            None,
            None,
        )
        .expect("map")
    }

    /// Bars turn a quarter turn when the wall they sit in runs north–south.
    #[test]
    fn gate_yaw_follows_the_wall_line() {
        // wall above and below the gate: the opening runs east–west, bars turn
        let m = map(&["###", "#X#", "###"]);
        assert!((gate_yaw(&m, 1, 1) - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // open above and below: bars stay put
        let m = map(&["#.#", "#X#", "#.#"]);
        assert_eq!(gate_yaw(&m, 1, 1), 0.0);
    }

    /// `SHORTCUT_YAW` — the model front (−Z) points at the side the door opens from.
    #[test]
    fn shortcut_yaw_table() {
        assert_eq!(shortcut_yaw(Facing::N), 0.0);
        assert_eq!(shortcut_yaw(Facing::S), std::f32::consts::PI);
        assert_eq!(shortcut_yaw(Facing::E), -std::f32::consts::FRAC_PI_2);
        assert_eq!(shortcut_yaw(Facing::W), std::f32::consts::FRAC_PI_2);
    }
}
