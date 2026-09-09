//! The hub buildings (`hub.js:BUILDINGS`, `setMesh`, `refreshBuildings`, `syncBoardPapers`) and
//! the Shrine's blessing glow.
//!
//! One entity per `BuildingDef`, at its anchor cell, in one of three states (`hub.js:370
//! refreshBuildings`):
//!
//! | state | when | look |
//! |---|---|---|
//! | `None` | not unlocked | nothing drawn |
//! | `Ghost` | unlocked, not built | the model in the ghost colour |
//! | `Built` | `economy::is_built` | the model plus the emissive floor pad |
//!
//! `refreshBuildings` was called on `build`, `flameTier`, `npcRescued` and `hubEnter`; the port
//! diffs the desired state against what is drawn every frame instead, which is the same thing for
//! seven buildings and cannot miss an event.

use bevy::prelude::*;
use undercroft_data::models::BoxDef;
use undercroft_sim::economy;

use crate::resources::{Game, HubMapRes, HubRes, SaveRes};
use crate::tick::Clock;

use super::model::{spawn_box, spawn_model, BoxAssets};
use super::{linear_u32, scale_rgb};

/// `hub.js:60 meshes[id].state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildState {
    /// `'none'` — the building is not unlocked yet.
    None,
    /// `'ghost'` — unlocked but unbuilt: a wireframe in the JS.
    Ghost,
    /// `'built'`.
    Built,
}

/// `hub.js:370 refreshBuildings` — the state a building should be in.
pub fn desired_state(
    data: &undercroft_data::GameData,
    save: &undercroft_sim::save::SaveData,
    tier: u32,
    id: &str,
) -> BuildState {
    if economy::is_built(data, save, id) {
        BuildState::Built
    } else if economy::unlocked(data, save, tier, id) {
        BuildState::Ghost
    } else {
        BuildState::None
    }
}

/// One drawn building.
#[derive(Component, Debug)]
pub struct HubBuilding {
    pub id: String,
    pub state: BuildState,
    /// `Math.random() * 6` in the JS; deterministic here (the index in `BUILD_ORDER`).
    pub phase: f32,
    /// The `paper0` … `paper3` boxes of the Departure Board.
    pub papers: Vec<Entity>,
    /// The Shrine's six `candle` boxes and its `halo`.
    pub candles: Vec<Entity>,
    pub halo: Option<Entity>,
    /// The tram's `cart` pivot part (`models.js:tram`).
    pub cart: Option<Entity>,
}

/// The emissive flagstone pad under a built building (`hub.js:290 setMesh` — a pad instead of a
/// `PointLight`, "cheap at any frame rate").
#[derive(Component, Debug)]
pub struct BuildingPad {
    pub phase: f32,
}

/// The zone-locked state of each board paper, as `hub.js:381 syncBoardPapers` computes it.
pub fn board_locked(
    data: &undercroft_data::GameData,
    save: &undercroft_sim::save::SaveData,
    tier: u32,
) -> Vec<bool> {
    data.zones
        .iter()
        .map(|z| economy::zone_locked(data, save, tier, &z.id).is_some())
        .collect()
}

/// `hub.js:290 setMesh` — build (or rebuild) one building's entity in `state`.
#[allow(clippy::too_many_arguments)]
fn set_mesh(
    commands: &mut Commands,
    assets: &mut BoxAssets,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    data: &undercroft_data::GameData,
    map: &undercroft_data::ParsedMap,
    id: &str,
    state: BuildState,
    phase: f32,
) {
    let Some(b) = data.buildings.buildings.get(id) else {
        return;
    };
    let Some(a) = b
        .anchor
        .parse::<u8>()
        .ok()
        .and_then(|d| map.anchors.get(&d))
    else {
        return;
    };
    let (ax, az) = (a.x + b.dx, a.z + b.dz);
    if state == BuildState::None {
        commands.spawn((
            HubBuilding {
                id: id.to_string(),
                state,
                phase,
                papers: Vec::new(),
                candles: Vec::new(),
                halo: None,
                cart: None,
            },
            Name::new(format!("hub:none:{id}")),
            Transform::from_xyz(ax, 0.0, az),
            Visibility::Hidden,
        ));
        return;
    }
    let Some(def) = data.models.get(&b.model) else {
        error!("hub: models.ron has no {:?} for building {id}", b.model);
        return;
    };
    let at = Transform::from_xyz(ax, 0.0, az).with_rotation(Quat::from_rotation_y(b.rot));
    let built = spawn_model(commands, assets, meshes, mats, def, at);
    commands
        .entity(built.root)
        .insert(Name::new(format!("hub:{}:{id}", state_name(state))));

    if state == BuildState::Ghost {
        // The JS swapped every material for one shared wireframe (`ghostMat`). Bevy has no
        // wireframe without the `bevy_pbr/wireframe` feature, which PHASE2_LANES §0 forbids
        // adding, so a ghost is the same silhouette in `HUB_CFG.ghostColor`, unlit and
        // translucent — the "not built yet" reading the wireframe carried.
        let ghost = mats.add(StandardMaterial {
            base_color: super::rgb_u32(data.buildings.cfg.ghost_color).with_alpha(0.35),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        for (_, e) in &built.boxes {
            commands.entity(*e).insert(MeshMaterial3d(ghost.clone()));
        }
        // Unnamed boxes too: re-materialise every mesh child below the root.
        commands
            .entity(built.root)
            .insert(GhostMaterial(ghost.clone()));
    } else {
        let pad = BoxDef {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.8,
            h: 0.02,
            d: 1.8,
            color: 0x000000,
            emissive: Some(0xffc070),
            emissive_k: data.buildings.cfg.pad_glow,
            name: Some("pad".to_string()),
            ry: 0.0,
            part: None,
            hidden: false,
        };
        let root = commands
            .spawn((
                Name::new(format!("hub:pad:{id}")),
                Transform::from_xyz(ax, 0.011, az),
                Visibility::Inherited,
                BuildingPad { phase },
            ))
            .id();
        // Its own material, so the pad can pulse without dragging every emissive box with it.
        // `hub.js:290` builds it `transparent: true, opacity: 0.55`, so the flagstones read
        // through the glow.
        let e = spawn_box(commands, assets, meshes, mats, root, &pad);
        let mut pad_mat = super::model::box_material(pad.color, pad.emissive, pad.emissive_k);
        pad_mat.base_color = pad_mat.base_color.with_alpha(0.55);
        pad_mat.alpha_mode = AlphaMode::Blend;
        let h = mats.add(pad_mat);
        commands.entity(e).insert(MeshMaterial3d(h));
    }

    let mut papers = Vec::new();
    if id == "board" && state == BuildState::Built {
        // `hub.js:308` — own paper materials, plus a candle on the cap so the notices read.
        for i in 0..4 {
            if let Some(e) = built.named(&format!("paper{i}")) {
                let h = mats.add(super::model::box_material(0xe0d0a0, Some(0xe0d0a0), 0.3));
                commands.entity(e).insert(MeshMaterial3d(h));
                papers.push(e);
            }
        }
        let candle = BoxDef {
            x: 0.55,
            y: 2.14 - 0.08,
            z: 0.1,
            w: 0.08,
            h: 0.16,
            d: 0.08,
            color: 0x000000,
            emissive: Some(0xffd080),
            emissive_k: 1.0,
            name: Some("candle".to_string()),
            ry: 0.0,
            part: None,
            hidden: false,
        };
        spawn_box(commands, assets, meshes, mats, built.root, &candle);
    }

    let mut candles = Vec::new();
    let mut halo = None;
    if id == "shrine" && state == BuildState::Built {
        for i in 0..6 {
            if let Some(e) = built.named(&format!("candle{i}")) {
                let h = mats.add(super::model::box_material(0x000000, Some(0xffc070), 1.0));
                commands.entity(e).insert(MeshMaterial3d(h));
                candles.push(e);
            }
        }
        if let Some(e) = built.named("halo") {
            let h = mats.add(super::model::box_material(0x000000, Some(0x6060ff), 0.9));
            commands.entity(e).insert(MeshMaterial3d(h));
            halo = Some(e);
        }
    }

    commands.entity(built.root).insert(HubBuilding {
        id: id.to_string(),
        state,
        phase,
        papers,
        candles,
        halo,
        cart: built.part("cart"),
    });
}

/// The ghost material, kept on the root so [`paint_ghosts`] can reach the boxes the exporter left
/// unnamed (a `Name`-less box never lands in `SpawnedModel::boxes`).
#[derive(Component, Debug)]
struct GhostMaterial(Handle<StandardMaterial>);

fn state_name(s: BuildState) -> &'static str {
    match s {
        BuildState::None => "none",
        BuildState::Ghost => "ghost",
        BuildState::Built => "built",
    }
}

/// Repaint every mesh under a freshly spawned ghost, including the unnamed boxes.
fn paint_ghosts(
    mut commands: Commands,
    ghosts: Query<(Entity, &GhostMaterial), Added<GhostMaterial>>,
    children: Query<&Children>,
    meshes: Query<Entity, With<Mesh3d>>,
) {
    for (root, g) in ghosts.iter() {
        for e in children.iter_descendants(root) {
            if meshes.contains(e) {
                commands.entity(e).insert(MeshMaterial3d(g.0.clone()));
            }
        }
    }
}

/// `hub.js:370 refreshBuildings` — make every building's entity match its state.
#[allow(clippy::too_many_arguments)]
fn sync_buildings(
    mut commands: Commands,
    game: Game,
    save: Res<SaveRes>,
    hub: Res<HubRes>,
    hub_map: Res<HubMapRes>,
    mut assets: ResMut<BoxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    drawn: Query<(Entity, &HubBuilding)>,
    pads: Query<(Entity, &Name), With<BuildingPad>>,
) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let Some(hm) = hub_map.0.as_ref() else {
        return;
    };
    for (i, id) in data.buildings.order.iter().enumerate() {
        let want = desired_state(data, &save.0, hub.0.tier, id);
        let cur = drawn.iter().find(|(_, b)| &b.id == id);
        if let Some((_, b)) = cur {
            if b.state == want {
                continue;
            }
        }
        // `hub.js:284 removeMesh` — the old group and its pad go first.
        if let Some((e, _)) = cur {
            commands.entity(e).despawn();
        }
        let pad_name = format!("hub:pad:{id}");
        for (e, n) in pads.iter() {
            if n.as_str() == pad_name {
                commands.entity(e).despawn();
            }
        }
        set_mesh(
            &mut commands,
            &mut assets,
            &mut meshes,
            &mut mats,
            data,
            &hm.map,
            id,
            want,
            i as f32 * 1.3,
        );
    }
}

/// `hub.js:381 syncBoardPapers` — a paper is dark grey while its zone is locked.
fn sync_board_papers(
    game: Game,
    save: Res<SaveRes>,
    hub: Res<HubRes>,
    boards: Query<&HubBuilding>,
    handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let Some(board) = boards.iter().find(|b| b.id == "board") else {
        return;
    };
    if board.papers.is_empty() {
        return;
    }
    let locked = board_locked(data, &save.0, hub.0.tier);
    for (i, &e) in board.papers.iter().enumerate() {
        let is_locked = locked.get(i).copied().unwrap_or(true);
        let Ok(h) = handles.get(e) else { continue };
        let Some(mut m) = mats.get_mut(&h.0) else {
            continue;
        };
        let (color, k) = if is_locked {
            (0x555555_u32, 0.08_f32)
        } else {
            (0xe0d0a0, 0.3)
        };
        m.base_color = super::rgb_u32(color);
        m.emissive = scale_rgb(linear_u32(color), k);
    }
}

/// The blessing glow: the Shrine's candles are lit while the blessing is charged for this descent
/// (`hub.js:471 onZoneEnter` — "the shrine candles stay dark" when there is no oil), dim when the
/// blessing is on but not yet charged, and out when it is snuffed.
fn update_blessing_glow(
    save: Res<SaveRes>,
    hub: Res<HubRes>,
    clock: Res<Clock>,
    shrines: Query<&HubBuilding>,
    handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(shrine) = shrines.iter().find(|b| b.id == "shrine") else {
        return;
    };
    let lit = hub.0.blessed;
    let on = save.0.blessing;
    let base = if lit {
        1.0
    } else if on {
        0.35
    } else {
        0.06
    };
    let f = base * (0.92 + 0.08 * (clock.time * 6.0).sin());
    for &e in &shrine.candles {
        if let Ok(h) = handles.get(e) {
            if let Some(mut m) = mats.get_mut(&h.0) {
                m.emissive = scale_rgb(linear_u32(0xffc070), f);
            }
        }
    }
    if let Some(e) = shrine.halo {
        if let Ok(h) = handles.get(e) {
            if let Some(mut m) = mats.get_mut(&h.0) {
                m.emissive = scale_rgb(linear_u32(0x6060ff), if lit { 1.6 * f } else { 0.9 });
            }
        }
    }
}

/// `hub.js:833 update` — the pads breathe and the tram cart rocks.
fn animate_buildings(
    game: Game,
    clock: Res<Clock>,
    pads: Query<(&BuildingPad, &Children)>,
    handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    buildings: Query<&HubBuilding>,
    mut xf: Query<&mut Transform>,
) {
    let Some(data) = game.get().map(|a| &a.data) else {
        return;
    };
    let time = clock.time;
    let glow = data.buildings.cfg.pad_glow;
    for (pad, children) in pads.iter() {
        let k = glow * (0.9 + 0.1 * (time * 9.0 + pad.phase).sin());
        for c in children.iter() {
            if let Ok(h) = handles.get(c) {
                if let Some(mut m) = mats.get_mut(&h.0) {
                    m.emissive = scale_rgb(linear_u32(0xffc070), k);
                }
            }
        }
    }
    if let Some(cart) = buildings
        .iter()
        .find(|b| b.id == "tram")
        .and_then(|b| b.cart)
    {
        if let Ok(mut t) = xf.get_mut(cart) {
            t.translation.z = 0.15 * (time * 0.5).sin();
        }
    }
}

/// The buildings, their pads, the board papers and the blessing glow.
pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            sync_buildings,
            paint_ghosts,
            sync_board_papers,
            update_blessing_glow,
            animate_buildings,
        )
            .chain()
            .in_set(super::HubSet),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::GameData;
    use undercroft_sim::save::SaveData;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    /// `hub.js:370 refreshBuildings` — the board is always built, an NPC building needs its NPC,
    /// and the tram needs the flame at tier 2.
    #[test]
    fn built_unbuilt_state_selection() {
        let data = data();
        let mut save = SaveData::default();
        assert_eq!(
            desired_state(&data, &save, 1, "board"),
            BuildState::Built,
            "the Departure Board is `always`"
        );
        assert_eq!(desired_state(&data, &save, 1, "workshop"), BuildState::None);
        assert_eq!(
            desired_state(&data, &save, 1, "tram"),
            BuildState::None,
            "tier 1 does not unlock the tram"
        );
        assert_eq!(desired_state(&data, &save, 2, "tram"), BuildState::Ghost);

        save.rescued.set("lamplighter", true);
        assert_eq!(
            desired_state(&data, &save, 1, "workshop"),
            BuildState::Ghost,
            "rescuing Wick raises the Workshop ghost"
        );
        save.buildings.set("workshop", true);
        assert_eq!(
            desired_state(&data, &save, 1, "workshop"),
            BuildState::Built
        );
    }

    /// `hub.js:381 syncBoardPapers` — one paper per zone, the first unlocked from the start.
    #[test]
    fn board_papers_follow_zone_locks() {
        let data = data();
        let save = SaveData::default();
        let locked = board_locked(&data, &save, 1);
        assert_eq!(locked.len(), data.zones.len());
        assert!(!locked[0], "the first zone is always open");
        assert!(locked[1..].iter().all(|&l| l), "the rest start locked");
    }
}
