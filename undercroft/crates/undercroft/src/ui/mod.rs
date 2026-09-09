//! UI lane: the HUD, menus and screens. Ports `ui.js` in full, plus the HUD/menu/screen portions
//! of `main.js` (fade overlay, prompts, the title/menu key routing), `hub.js` (departure board,
//! build/service panels, the minimap) and `endgame.js` (the altar choice and ending screens).
//!
//! Registered only by [`crate::UndercroftPlugin`]: this lane needs a window and a renderer, and
//! must stay out of the headless harness (`crate::headless`).
//!
//! Layout of the module:
//!
//! | file | `prototype/src` origin |
//! |---|---|
//! | [`style`] | the stylesheet in `index.html` |
//! | [`menu`] | `ui.js:makeListMenu` + the main/pause/controls/sound/confirm panels |
//! | [`screens`] | `hub.js` board/build/service, `npc.js` dialogue, `endgame.js`, `ui.js:showDeath` |
//! | [`render`] | `.screen > .box` markup for either kind of panel |
//! | [`hud`] | `ui.js:updateHUD` / `hintText` and the HUD lines other modules appended |
//! | [`toasts`] | `ui.js` toast queue and timing |
//! | [`fade`] | `#vignette` / `#flashfx` / `#fade` / `#toast` |
//! | [`minimap`] | `hub.js:drawMinimap` |
//! | [`keys`] | the `keydown` branches `player.rs` does not own |
//!
//! Nothing here mutates game state: selections are pushed as [`crate::debug::DebugCommand`]s. The two
//! exceptions are documented in the lane report — the toast queue ([`crate::resources::Toasts`]), which
//! this lane owns outright, and the `minimap {on}` event, which has no other owner.

pub mod fade;
pub mod hud;
pub mod keys;
pub mod menu;
pub mod minimap;
pub mod render;
pub mod screens;
pub mod style;
pub mod toasts;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use undercroft_sim::economy;
use undercroft_sim::player::Carried;
use undercroft_sim::{contracts, SimEvent};

use crate::messages::SimMessage;
use crate::resources::{Game, HubMapRes, HubRes, Player, SaveRes, Toasts, ZoneRes};
use crate::run::EndingScreenRes;
use crate::state::{GameMode, MenuKind, Mode, PrevMode};
use crate::tick::SimSet;
use menu::{MenuCtx, MenuState, PanelKind};
use minimap::{MiniCanvas, MiniItem, MiniMarks, MiniShortcut};
use render::{PanelView, ScreenRoot};
use screens::TextScreen;
use style::UiFont;
use toasts::ToastState;

/// `ui.js:VERSION`.
pub const VERSION: &str = "bevy port";

/// The minimap texture is the prototype's `<canvas id="minimap" width=200 height=200>`.
pub const MINIMAP_PX: u32 = 200;

/// Everything `ui.js`'s module-scope `ui` object held, plus the panel bookkeeping the DOM did for it.
#[derive(Resource, Debug, Default)]
pub struct UiState {
    /// `ui.js` `mainMenu` / `pauseMenu` — only one is ever open, so one machine serves both.
    pub menu: MenuState,
    /// Which root the machine currently holds, so a mode change opens/closes it exactly once.
    pub menu_root: Option<PanelKind>,
    /// The panels' inputs, rebuilt each frame.
    pub menu_ctx: MenuCtx,
    /// The `#menu`-style screen for this frame, when one is up.
    pub text_screen: Option<TextScreen>,
    /// What is currently drawn; a change rebuilds the node tree.
    pub view: Option<PanelView>,
    /// `ui.toastQ` timing (the queue itself is [`Toasts`]).
    pub toast: ToastState,
    /// `ui.seenT`.
    pub seen_t: f32,
    /// `ui.flashFx`.
    pub flash_fx: f32,
    /// `#vignette.red`.
    pub death_red: bool,
    /// `ctx.state.hintOverride`.
    pub hint_override: String,
    /// `state.lostLoot` / `state.lostAny` for the death screen.
    pub lost: String,
    pub lost_any: bool,
    /// `ctx.player.carried` as of the top of the current fixed tick, so the death screen can name what
    /// was dropped (`economy::die` clears it and `DeathOutcome.lost` is not stored anywhere).
    pub carried_at_tick: Carried,
    /// `blessingKept.kept` seen this tick, subtracted from the snapshot.
    pub blessing_kept: Option<Carried>,
    /// `#minimap.hidden`.
    pub minimap_on: bool,
    /// A Tab press or the Cartographer's `[1]` this frame.
    pub minimap_toggle_requested: bool,
    /// `HUB_CFG.minimapHz` throttle.
    pub minimap_t: f32,
    /// `main.js:resetArmedT`.
    pub reset_armed_t: f32,
    /// "Save wiped." waits for `saveReset`'s flush to land first (`main.js:562`).
    pub wipe_toast_pending: bool,
    /// The building a `build` / `service` menu was opened on (`hub.js:openBuilding`).
    pub menu_building: Option<(String, bool)>,
    /// The resident a `dialog` menu was opened on (`npcTalk`).
    pub menu_npc: Option<String>,
}

/// The minimap's texture handle.
#[derive(Resource, Debug)]
pub struct MinimapImage(pub Handle<Image>);

/// `#minimap`.
#[derive(Component)]
pub struct MinimapNode;

/// Load the font, then build the HUD, the overlays and the minimap panel.
///
/// `Assets<Font>` / `Assets<Image>` are optional because `UndercroftPlugin` is also added by the
/// asset-loader test in `headless.rs`, which runs on `MinimalPlugins`: with no `bevy_text` there is
/// nothing to draw at all, and with no renderer there is no minimap texture.
fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    fonts: Option<Res<Assets<Font>>>,
    images: Option<ResMut<Assets<Image>>>,
) {
    if fonts.is_none() {
        return;
    }
    let font = UiFont(server.load(style::FONT_PATH));
    hud::spawn(&mut commands, &font);
    fade::spawn(&mut commands, &font);
    commands.insert_resource(font);
    let Some(mut images) = images else {
        return;
    };

    let mut image = Image::new_fill(
        Extent3d {
            width: MINIMAP_PX,
            height: MINIMAP_PX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // `image-rendering: pixelated` on `#minimap`.
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);
    commands.spawn((
        MinimapNode,
        ImageNode::new(handle.clone()),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(16.0),
            bottom: Val::Px(16.0),
            width: Val::Px(MINIMAP_PX as f32),
            height: Val::Px(MINIMAP_PX as f32),
            border: UiRect::all(Val::Px(1.0)),
            display: Display::None,
            ..default()
        },
        BorderColor::all(style::BORDER),
        BackgroundColor(style::MINIMAP_BG),
        GlobalZIndex(2),
    ));
    commands.insert_resource(MinimapImage(handle));
}

/// `ui.js:init`'s listeners, plus the two the HUD needs from `hub.js` / `endgame.js`.
#[allow(clippy::too_many_arguments)]
fn read_events(
    mut r: MessageReader<SimMessage>,
    game: Game,
    mut ui: ResMut<UiState>,
    mut toasts: ResMut<Toasts>,
    save: Res<SaveRes>,
    hub: Res<HubRes>,
    hub_map: Res<HubMapRes>,
    player: Res<Player>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let cfg = &asset.data.config;
    for m in r.read() {
        match &m.0 {
            SimEvent::Toast { msg } => {
                if ui.menu.is_open() && toasts::suppressed_while_menu_open(msg) {
                    continue;
                }
                toasts.0.push_back(msg.clone());
            }
            SimEvent::FlameTier { tier, initial, .. } => {
                if !initial {
                    if let Some(t) = cfg.tiers.get((*tier as usize).saturating_sub(1)) {
                        toasts.0.push_back(t.msg.clone());
                    }
                }
            }
            SimEvent::Flash { .. } => ui.flash_fx = cfg.cfg.flash_fx,
            SimEvent::HunterState { state, prev, .. } => {
                if state != prev && matches!(state.as_str(), "CHASE" | "POUNCE" | "SURGE") {
                    ui.seen_t = 2.0;
                }
            }
            SimEvent::BlessingKept { kept, .. } => ui.blessing_kept = Some(*kept),
            SimEvent::Death { .. } => {
                ui.death_red = true;
                // `main.js:368` — the hint line while the death camera runs.
                ui.hint_override = "The dark took you.".to_string();
                let mut lost = ui.carried_at_tick;
                if let Some(k) = ui.blessing_kept.take() {
                    lost.oil = lost.oil.saturating_sub(k.oil);
                    lost.relic = lost.relic.saturating_sub(k.relic);
                    lost.rich = lost.rich.saturating_sub(k.rich);
                    lost.quest = lost.quest.saturating_sub(k.quest);
                }
                ui.lost_any = lost.total() > 0;
                ui.lost = economy::describe(&lost);
            }
            SimEvent::HubEnter => {
                ui.death_red = false;
                ui.hint_override.clear();
            }
            SimEvent::Title => {
                // `main.js:477` clears the override with the mode.
                ui.hint_override.clear();
                ui.toast.flush(&mut toasts.0);
            }
            SimEvent::SaveReset => {
                ui.toast.flush(&mut toasts.0);
                if ui.wipe_toast_pending {
                    ui.wipe_toast_pending = false;
                    toasts.0.push_back("Save wiped.".to_string());
                }
            }
            SimEvent::Minimap { on } => ui.minimap_on = *on,
            SimEvent::NpcTalk { id } => ui.menu_npc = Some(id.clone()),
            SimEvent::MenuOpen { kind } => {
                // `hub.js:openBuilding(id)` — which building the panel is for. `player.rs` pushes
                // `OpenMenu(kind)` without the id, so it is resolved from the same sim call it used.
                if matches!(kind.as_str(), "build" | "service") {
                    ui.menu_building = hub_map.0.as_ref().and_then(|h| {
                        economy::hub_interact_target(
                            &asset.data,
                            &save.0,
                            &hub.0,
                            &h.map,
                            player.x,
                            player.z,
                        )
                        .and_then(|t| match t {
                            economy::HubInteract::Build { id, .. } => Some((id, false)),
                            economy::HubInteract::Building { id, .. } => Some((id, true)),
                            economy::HubInteract::Descend { .. } => None,
                        })
                    });
                }
            }
            SimEvent::MenuClose { .. } => {
                ui.menu_building = None;
                ui.menu_npc = None;
            }
            _ => {}
        }
    }
}

/// `ui.js:update(c, dt)`'s two decays and the toast timer.
fn tick_ui(time: Res<Time>, game: Game, mut ui: ResMut<UiState>, mut toasts: ResMut<Toasts>) {
    let Some(asset) = game.get() else {
        return;
    };
    let dt = time.delta_secs();
    ui.seen_t = (ui.seen_t - dt).max(0.0);
    ui.flash_fx = (ui.flash_fx - dt).max(0.0);
    let toast_t = asset.data.config.cfg.toast_t;
    let mut state = std::mem::take(&mut ui.toast);
    state.update(dt, &mut toasts.0, toast_t);
    ui.toast = state;
}

/// Everything the screens read.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ScreenSource<'w> {
    pub game: Game<'w>,
    pub save: Res<'w, SaveRes>,
    pub hub: Res<'w, HubRes>,
    pub player: Res<'w, Player>,
    pub zone: Res<'w, ZoneRes>,
    pub kind: Res<'w, MenuKind>,
    pub ending: Res<'w, EndingScreenRes>,
    /// `ui.js:showPause(state.prevMode === 'ZONE')`.
    pub prev: Res<'w, PrevMode>,
}

/// `ui.js:showTitle` / `showPause` / `showMenu` / `showDeath` / `endgame.render*` in one place: decide
/// which panel the current mode wants, then rebuild the node tree only when it changed.
fn update_screens(
    mut commands: Commands,
    src: ScreenSource,
    mode: Mode,
    font: Option<Res<UiFont>>,
    mut ui: ResMut<UiState>,
    roots: Query<Entity, With<ScreenRoot>>,
) {
    let (Some(font), Some(asset)) = (font, src.game.get()) else {
        return;
    };
    let data = &asset.data;
    let m = mode.get();
    let tier = src.hub.0.tier;
    // `main.js:openPause` passes `prevMode === 'ZONE'`: the pause menu is drawn while the mode is MENU.
    let behind = match m {
        GameMode::Menu => src.prev.0.unwrap_or(GameMode::Hub),
        other => other,
    };

    ui.menu_ctx = MenuCtx {
        has_progress: src.save.0.has_progress(),
        save_text: src.save.0.summary(&data.config.tiers).text,
        in_zone: behind == GameMode::Zone,
        loot: economy::describe(&src.player.carried),
        has_loot: src.player.carried.total() > 0,
        volume: src.save.0.audio.vol,
        muted: src.save.0.audio.muted,
        version: VERSION.to_string(),
    };

    // `main.js` owns the mode; the list menu follows it.
    let want_root = match m {
        GameMode::Title => Some(PanelKind::MainRoot),
        GameMode::Menu if src.kind.0.as_deref() == Some("pause") => Some(PanelKind::PauseRoot),
        _ => None,
    };
    if want_root != ui.menu_root {
        let ctx = ui.menu_ctx.clone();
        match &want_root {
            Some(k) => ui.menu.open(k.clone(), &ctx),
            None => ui.menu.close(),
        }
        ui.menu_root = want_root;
    }

    ui.text_screen = text_screen(&src, &ui, m, tier);

    let ctx = ui.menu_ctx.clone();
    let view = match ui.menu.view(&ctx) {
        Some(v) => Some(PanelView::from_menu(&v)),
        None => ui.text_screen.as_ref().map(PanelView::from_text),
    };
    if view == ui.view {
        return;
    }
    ui.view = view;
    for e in &roots {
        commands.entity(e).despawn();
    }
    if let Some(v) = &ui.view {
        render::spawn_panel(&mut commands, &font, v);
    }
}

/// Which `#menu`-style screen the mode asks for.
fn text_screen(src: &ScreenSource, ui: &UiState, m: GameMode, tier: u32) -> Option<TextScreen> {
    let data = &src.game.get()?.data;
    let save = &src.save.0;
    match m {
        GameMode::Dead => Some(screens::death(&ui.lost, ui.lost_any)),
        GameMode::Ending => screens::ending(data, save, tier, &src.ending.0),
        GameMode::Menu => match src.kind.0.as_deref()? {
            "board" => Some(screens::board(data, save, tier)),
            "build" => {
                let (id, _) = ui.menu_building.as_ref()?;
                screens::build_menu(data, save, tier, id)
            }
            "service" => {
                let (id, _) = ui.menu_building.as_ref()?;
                match id.as_str() {
                    "board" => Some(screens::board(data, save, tier)),
                    "workshop" => Some(screens::workshop(data, save)),
                    "press" => Some(screens::press(data, save)),
                    "cart" => Some(screens::cart(data, save, &explored_table(src))),
                    "shrine" => Some(screens::shrine(data, save)),
                    "tram" | "elevator" => screens::ride(data, save, tier, id),
                    _ => None,
                }
            }
            "dialog" => screens::dialog(data, save, ui.menu_npc.as_deref()?),
            _ => None,
        },
        _ => None,
    }
}

/// `hub.js:openCart` — `exploredPct(zone)` per zone, from the save's bitsets.
fn explored_table(src: &ScreenSource) -> Vec<(String, u32)> {
    let Some(asset) = src.game.get() else {
        return Vec::new();
    };
    let data = &asset.data;
    data.zones
        .iter()
        .map(|z| {
            let pct = match loaded_or_parsed(src, &z.id) {
                Some(map) => {
                    let bits = src.save.0.explored_bits(&z.id, (map.w * map.h) as usize);
                    economy::explored_pct(&map, &bits)
                }
                None => 0,
            };
            (z.name.clone(), pct)
        })
        .collect()
}

/// The loaded zone's map, or a fresh parse (`hub.js:zoneMap`). Parsing is only done for the
/// Cartographer's panel, which is opened by hand.
fn loaded_or_parsed(src: &ScreenSource, id: &str) -> Option<undercroft_data::ParsedMap> {
    if let Some(z) = src.zone.get() {
        if z.id == id {
            return Some(z.map.clone());
        }
    }
    src.game.get()?.data.parse_zone(id)?.ok()
}

/// `main.js`'s `KEYS.minimap` branch plus `hub.js:onMinimapToggle` — the Cartographer's Table gate.
fn toggle_minimap(
    mut ui: ResMut<UiState>,
    save: Res<SaveRes>,
    mut toasts: ResMut<Toasts>,
    mut w: MessageWriter<SimMessage>,
) {
    if !ui.minimap_toggle_requested {
        return;
    }
    ui.minimap_toggle_requested = false;
    if !save.0.buildings.cart {
        if ui.minimap_on {
            ui.minimap_on = false;
        }
        toasts.0.push_back(
            "You have no map of this place. Ines could draw one — Cartographer's Table."
                .to_string(),
        );
        w.write(SimMessage(SimEvent::ui_error()));
        return;
    }
    ui.minimap_on = !ui.minimap_on;
    // Nothing else owns `minimap {on}`: `debug.rs` has no toggle command (see the lane report).
    w.write(SimMessage(SimEvent::Minimap { on: ui.minimap_on }));
}

/// `hub.js:drawMinimap()` at `HUB_CFG.minimapHz`.
#[allow(clippy::too_many_arguments)]
fn update_minimap(
    time: Res<Time>,
    game: Game,
    mode: Mode,
    prev: Res<PrevMode>,
    mut ui: ResMut<UiState>,
    save: Res<SaveRes>,
    zone: Res<ZoneRes>,
    hub_map: Res<HubMapRes>,
    player: Res<Player>,
    image: Option<Res<MinimapImage>>,
    images: Option<ResMut<Assets<Image>>>,
    mut node: Query<&mut Node, With<MinimapNode>>,
) {
    let (Some(asset), Some(image), Some(mut images)) = (game.get(), image, images) else {
        return;
    };
    let m = match mode.get() {
        GameMode::Menu => prev.0.unwrap_or(GameMode::Hub),
        other => other,
    };
    let hub_mode = matches!(m, GameMode::Hub | GameMode::Title);
    let showing = ui.minimap_on && matches!(m, GameMode::Hub | GameMode::Zone);
    if let Ok(mut n) = node.single_mut() {
        let want = if showing {
            Display::Flex
        } else {
            Display::None
        };
        if n.display != want {
            n.display = want;
        }
    }
    if !showing {
        return;
    }
    ui.minimap_t -= time.delta_secs();
    if ui.minimap_t > 0.0 {
        return;
    }
    ui.minimap_t = 1.0 / asset.data.config.hub_cfg.minimap_hz.max(1.0);

    let map = if hub_mode {
        hub_map.0.as_ref().map(|h| &h.map)
    } else {
        zone.get().map(|z| &z.map)
    };
    let Some(map) = map else {
        return;
    };
    let bits = if hub_mode {
        None
    } else {
        zone.get()
            .map(|z| save.0.explored_bits(&z.id, (map.w * map.h) as usize))
    };
    let mut marks = MiniMarks {
        player: Some((player.x, player.z, player.yaw)),
        ..MiniMarks::default()
    };
    if let (false, Some(z)) = (hub_mode, zone.get()) {
        marks.items = z
            .items
            .iter()
            .map(|i| MiniItem {
                kind: i.kind,
                x: i.x,
                z: i.z,
            })
            .collect();
        marks.lanterns = z.lanterns.iter().map(|l| (l.x, l.z)).collect();
        marks.shortcuts = z
            .map
            .shortcuts
            .iter()
            .enumerate()
            .map(|(i, s)| MiniShortcut {
                cx: s.marker.cx,
                cz: s.marker.cz,
                x: s.marker.x,
                z: s.marker.z,
                open: z.doors.shortcut_open(i),
            })
            .collect();
        marks.targets = contracts::targets(&asset.data, &save.0, Some(&z.id))
            .into_iter()
            .filter_map(|t| Some((t.x? + map.ox as f32, t.z?)))
            .collect();
    }
    let mut canvas = MiniCanvas::new(
        MINIMAP_PX,
        MINIMAP_PX,
        map,
        asset.data.config.hub_cfg.minimap_px,
    );
    minimap::draw(&mut canvas, map, bits.as_deref(), &marks);
    let want = (MINIMAP_PX * MINIMAP_PX * 4) as usize;
    let Some(mut img) = images.get_mut(&image.0) else {
        return;
    };
    if let Some(data) = img.data.as_mut() {
        if data.len() == want && canvas.buf.len() == want {
            data.copy_from_slice(&canvas.buf);
        }
    }
}

/// `ctx.player.carried` at the top of the fixed tick, before anything in it can clear the loot.
fn snapshot_carried(player: Res<Player>, mut ui: ResMut<UiState>) {
    if ui.carried_at_tick != player.carried {
        ui.carried_at_tick = player.carried;
    }
}

/// The whole lane.
pub fn plugin(app: &mut App) {
    app.init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, snapshot_carried.before(SimSet::Debug))
        .add_systems(
            Update,
            (
                read_events,
                tick_ui,
                update_screens,
                keys::route_keys,
                toggle_minimap,
                update_minimap,
                hud::update_hud,
                fade::update_overlays,
            )
                .chain(),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The death screen's "Lost:" line is rebuilt from the tick snapshot minus what the blessing kept
    /// — `DeathOutcome.lost` is not stored anywhere the lane can read.
    #[test]
    fn lost_loot_is_the_snapshot_minus_the_blessing() {
        let carried = Carried {
            oil: 4,
            relic: 3,
            rich: 1,
            quest: 0,
        };
        let kept = Carried {
            oil: 2,
            relic: 1,
            rich: 0,
            quest: 0,
        };
        let mut lost = carried;
        lost.oil -= kept.oil;
        lost.relic -= kept.relic;
        assert_eq!(economy::describe(&lost), "2 flasks, 2 relics, 1 rich relic");
        assert!(lost.total() > 0);
        assert_eq!(economy::describe(&Carried::default()), "nothing");
    }
}
