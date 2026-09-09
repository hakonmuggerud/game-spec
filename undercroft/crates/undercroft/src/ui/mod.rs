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
//! | [`mouse`] | `Interaction`-driven hover/click on the list-menu rows, hub `[n]` lines and the death/ending screens |
//!
//! Nothing here mutates game state: selections are pushed as [`crate::debug::DebugCommand`]s. The one
//! exception is the toast queue ([`crate::resources::Toasts`]), which this lane owns outright.

pub mod fade;
pub mod hud;
pub mod keys;
pub mod menu;
pub mod minimap;
pub mod mouse;
pub mod render;
pub mod screens;
pub mod style;
pub mod toasts;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use undercroft_sim::economy;

use undercroft_sim::follower::{NpcState, Place};
use undercroft_sim::{contracts, SimEvent};

use crate::hub::buildings::{desired_state, BuildState};
use crate::messages::SimMessage;
use crate::resources::{Game, HubMapRes, HubRes, Npcs, Player, SaveRes, Toasts, ZoneRes};
use crate::run::{EndingScreenRes, LastDeath, MenuTargetRes, MinimapRes};
use crate::state::{GameMode, MenuKind, Mode, PrevMode};
use menu::{MenuCtx, MenuState, PanelKind};
use minimap::{MiniBuilding, MiniCanvas, MiniItem, MiniMarks, MiniNpc, MiniShortcut};
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
    /// `HUB_CFG.minimapHz` throttle.
    pub minimap_t: f32,
    /// `main.js:resetArmedT`.
    pub reset_armed_t: f32,
    /// "Save wiped." waits for `saveReset`'s flush to land first (`main.js:562`).
    pub wipe_toast_pending: bool,
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
fn read_events(
    mut r: MessageReader<SimMessage>,
    game: Game,
    mut ui: ResMut<UiState>,
    mut toasts: ResMut<Toasts>,
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
            SimEvent::Death { .. } => {
                ui.death_red = true;
                // `main.js:368` — the hint line while the death camera runs. What was lost is
                // `run.rs`'s `LastDeath`, straight off the `DeathOutcome`.
                ui.hint_override = "The dark took you.".to_string();
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
    /// `main.js:die`'s outcome (`state.lostLoot` / `state.lostAny`).
    pub death: Res<'w, LastDeath>,
    /// The building / resident the open menu belongs to (`hub.js:openBuilding`, `npc.js:talk`).
    pub menu_target: Res<'w, MenuTargetRes>,
}

/// `ui.js:showTitle` / `showPause` / `showMenu` / `showDeath` / `endgame.render*` in one place:
/// decide which panel the current mode wants and drive the list-menu stack. Pure state — no font,
/// no entities — so the menu machine also runs in a headless app (`ui::tests`).
fn update_screens(src: ScreenSource, mode: Mode, mut ui: ResMut<UiState>) {
    let Some(asset) = src.game.get() else {
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

    ui.text_screen = text_screen(&src, m, tier);

    let ctx = ui.menu_ctx.clone();
    ui.view = match ui.menu.view(&ctx) {
        Some(v) => Some(PanelView::from_menu(&v)),
        None => ui.text_screen.as_ref().map(PanelView::from_text),
    };
}

/// The node half of [`update_screens`]: rebuild the `bevy_ui` tree whenever [`UiState::view`]
/// changes. Needs the font, so it stands down in an app without `bevy_text`.
fn render_panels(
    mut commands: Commands,
    font: Option<Res<UiFont>>,
    ui: Res<UiState>,
    roots: Query<Entity, With<ScreenRoot>>,
    mut drawn: Local<Option<PanelView>>,
) {
    let Some(font) = font else {
        return;
    };
    if *drawn == ui.view {
        return;
    }
    drawn.clone_from(&ui.view);
    for e in &roots {
        commands.entity(e).despawn();
    }
    if let Some(v) = &ui.view {
        render::spawn_panel(&mut commands, &font, v);
    }
}

/// Which `#menu`-style screen the mode asks for.
fn text_screen(src: &ScreenSource, m: GameMode, tier: u32) -> Option<TextScreen> {
    let data = &src.game.get()?.data;
    let save = &src.save.0;
    match m {
        GameMode::Dead => Some(screens::death(&src.death.lost, src.death.lost_any)),
        GameMode::Ending => screens::ending(data, save, tier, &src.ending.0),
        GameMode::Menu => match src.kind.0.as_deref()? {
            "board" => Some(screens::board(data, save, tier)),
            "build" => {
                let id = src.menu_target.0.as_deref()?;
                screens::build_menu(data, save, tier, id)
            }
            "service" => {
                let id = src.menu_target.0.as_deref()?;
                match id {
                    "board" => Some(screens::board(data, save, tier)),
                    "workshop" => Some(screens::workshop(data, save)),
                    "press" => Some(screens::press(data, save)),
                    "cart" => Some(screens::cart(data, save, &explored_table(src))),
                    "shrine" => Some(screens::shrine(data, save)),
                    "tram" | "elevator" => screens::ride(data, save, tier, id),
                    _ => None,
                }
            }
            "dialog" => screens::dialog(data, save, src.menu_target.0.as_deref()?),
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
    hub: Res<HubRes>,
    npcs: Res<Npcs>,
    player: Res<Player>,
    minimap: Res<MinimapRes>,
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
    let showing = minimap.0 && matches!(m, GameMode::Hub | GameMode::Zone);
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
    if hub_mode {
        // `hub.js:779` — one square per `BUILD_ORDER` entry at its anchor cell.
        marks.buildings = asset
            .data
            .buildings
            .order
            .iter()
            .filter_map(|id| {
                let b = asset.data.buildings.buildings.get(id)?;
                let state = desired_state(&asset.data, &save.0, hub.0.tier, id);
                if state == BuildState::None {
                    return None;
                }
                let a = b
                    .anchor
                    .parse::<u8>()
                    .ok()
                    .and_then(|d| map.anchors.get(&d))?;
                Some(MiniBuilding {
                    x: a.x,
                    z: a.z,
                    built: state == BuildState::Built,
                })
            })
            .collect();
    }
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
        // `hub.js:795` — a dot per NPC present in the zone (captive or following).
        marks.npcs = npcs
            .0
            .present()
            .into_iter()
            .filter(|n| n.place == Some(Place::Zone))
            .map(|n| MiniNpc {
                x: n.x,
                z: n.z,
                following: n.state == NpcState::Follow,
            })
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

/// The whole lane.
pub fn plugin(app: &mut App) {
    app.init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                read_events,
                tick_ui,
                update_screens,
                render_panels,
                keys::route_keys,
                update_minimap,
                hud::update_hud,
                fade::update_overlays,
            )
                .chain(),
        );
    // `mouse`'s systems read `ButtonInput<MouseButton>`, `CursorMoved` and the world lane's
    // `PointerLock` — none of which the headless harness (no `InputPlugin`/`WindowPlugin`, no world
    // lane) ever inserts. `crate::has_renderer` is the same gate `world::plugin` itself stands down
    // on, so the two always agree on whether this app is the real one.
    if crate::has_renderer(app) {
        app.add_systems(
            Update,
            (
                mouse::hover_rows,
                mouse::click_rows,
                mouse::click_screen_picks,
                mouse::click_screen_cta,
            )
                .after(keys::route_keys),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::{DebugCommand, DebugQueue};
    use crate::headless::{headless_app, mode, send, step};

    /// The lane on a headless app: no window, no font, so only the state machine half runs
    /// ([`update_screens`]); [`render_panels`] and the minimap stand down on their own.
    fn ui_app() -> App {
        let mut app = headless_app();
        app.add_plugins(plugin);
        app
    }

    fn depth(app: &App) -> usize {
        app.world().resource::<UiState>().menu.depth()
    }

    /// `ui.js:makeListMenu`'s Escape: it pops the panel stack first and only closes the pause menu
    /// at its root (`pauseRoot.onEscape` → `A.closePause()`). `player.rs` must therefore *not*
    /// short-circuit Escape to `ClosePause` while a sub-panel is up.
    #[test]
    fn escape_pops_a_pause_sub_panel_before_it_closes_the_menu() {
        let mut app = ui_app();
        send(&mut app, DebugCommand::Begin);
        step(&mut app, 0.1);
        assert_eq!(mode(&app), GameMode::Hub);

        send(&mut app, DebugCommand::OpenPause);
        step(&mut app, 0.1);
        assert_eq!(mode(&app), GameMode::Menu);
        assert_eq!(depth(&app), 1, "the pause root");

        // `[2] Controls` pushes a sub-panel.
        send(&mut app, DebugCommand::Key("Digit2".into()));
        step(&mut app, 0.1);
        assert_eq!(depth(&app), 2, "Controls is on the stack");
        assert_eq!(
            app.world().resource::<UiState>().menu.kind(),
            Some(&menu::PanelKind::Controls)
        );

        send(&mut app, DebugCommand::Key("Escape".into()));
        step(&mut app, 0.1);
        assert_eq!(mode(&app), GameMode::Menu, "Escape only popped the panel");
        assert_eq!(depth(&app), 1);

        send(&mut app, DebugCommand::Key("Escape".into()));
        step(&mut app, 0.2);
        assert_eq!(mode(&app), GameMode::Hub, "Escape at the root resumes");
    }

    /// The Sound panel's rows queue the audio lane's commands (`AUDIO_COMMANDS_EXIST`).
    #[test]
    fn the_sound_panel_pushes_set_volume_and_toggle_mute() {
        let mut app = ui_app();
        send(&mut app, DebugCommand::OpenPause);
        step(&mut app, 0.1);
        // `[3] Sound`, then `[2] Mute`.
        send(&mut app, DebugCommand::Key("Digit3".into()));
        step(&mut app, 1.0 / 60.0);
        assert_eq!(
            app.world().resource::<UiState>().menu.kind(),
            Some(&menu::PanelKind::Sound)
        );
        send(&mut app, DebugCommand::Key("Digit2".into()));
        step(&mut app, 1.0 / 60.0);
        // Nothing in the skeleton owns them, so they are still queued when `Drain` logs them —
        // read the queue in the same tick they were pushed instead.
        send(&mut app, DebugCommand::Key("ArrowUp".into()));
        send(&mut app, DebugCommand::Key("ArrowRight".into()));
        step(&mut app, 1.0 / 60.0);
        let queued: Vec<DebugCommand> = app
            .world()
            .resource::<DebugQueue>()
            .0
            .iter()
            .cloned()
            .collect();
        assert!(
            queued
                .iter()
                .any(|c| matches!(c, DebugCommand::SetVolume(_))),
            "{queued:?}"
        );
    }
}
