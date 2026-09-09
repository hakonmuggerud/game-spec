//! The full-screen overlays of `index.html`: `#vignette` (oil-low darkening, red while dying),
//! `#flashfx` (the lamp flash), `#fade` (`main.js`'s black transition) and `#toast`.
//!
//! `run.rs` drives `Fade.a`; this module only paints it.

use bevy::prelude::*;

use crate::resources::Fade;
use crate::resources::{Game, LampRes};
use crate::state::{GameMode, Mode};
use crate::ui::hud::vignette_alpha;
use crate::ui::style::*;
use crate::ui::UiState;

/// `#vignette` — a flat dark wash (the DOM used a radial gradient; a `bevy_ui` node cannot, so the
/// alpha is the same curve at a lower ceiling).
#[derive(Component)]
pub struct Vignette;

/// `#flashfx`.
#[derive(Component)]
pub struct FlashFx;

/// `#fade`.
#[derive(Component)]
pub struct FadeOverlay;

/// `#toast`.
#[derive(Component)]
pub struct ToastLine;

/// Spawn the four overlays, back to front, at the `index.html` z-indices.
pub fn spawn(commands: &mut Commands, font: &UiFont) {
    let full = || Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    };
    commands.spawn((
        Vignette,
        full(),
        BackgroundColor(Color::NONE),
        GlobalZIndex(1),
    ));
    commands.spawn((
        FlashFx,
        full(),
        BackgroundColor(Color::NONE),
        GlobalZIndex(3),
    ));
    commands.spawn((
        FadeOverlay,
        full(),
        BackgroundColor(Color::NONE),
        GlobalZIndex(4),
    ));
    commands.spawn((
        ToastLine,
        Text::new(""),
        font.at(FS_TOAST),
        TextColor(Color::NONE),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            bottom: Val::Percent(6.0),
            width: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(6),
    ));
}

/// A full-screen overlay's colour, isolated from the other two.
type OverlayQuery<'w, 's, T, A, B> =
    Query<'w, 's, &'static mut BackgroundColor, (With<T>, Without<A>, Without<B>)>;

/// `ui.js:updateHUD`'s vignette/flash lines, `ui.js:setFade` and the toast line.
#[allow(clippy::too_many_arguments)]
pub fn update_overlays(
    game: Game,
    mode: Mode,
    ui: Res<UiState>,
    fade: Res<Fade>,
    lamp: Res<LampRes>,
    mut vignette: OverlayQuery<Vignette, FlashFx, FadeOverlay>,
    mut flash: OverlayQuery<FlashFx, Vignette, FadeOverlay>,
    mut fade_q: OverlayQuery<FadeOverlay, Vignette, FlashFx>,
    mut toast: Query<(&mut Text, &mut TextColor), With<ToastLine>>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let cfg = &asset.data.config;
    let m = mode.get();
    if let Ok(mut c) = vignette.single_mut() {
        // `#vignette.red` while dying or dead, otherwise the low-oil wash
        let a = vignette_alpha(cfg, m, lamp.0.oil);
        let want = if ui.death_red && matches!(m, GameMode::Dying | GameMode::Dead) {
            Color::srgba(0.38, 0.0, 0.0, 0.55)
        } else {
            Color::srgba(0.0, 0.0, 0.0, a * 0.85)
        };
        set(&mut c, want);
    }
    if let Ok(mut c) = flash.single_mut() {
        let a = (ui.flash_fx / cfg.cfg.flash_fx.max(0.0001)).clamp(0.0, 1.0);
        set(&mut c, Color::srgba(1.0, 1.0, 1.0, a));
    }
    if let Ok(mut c) = fade_q.single_mut() {
        set(&mut c, Color::srgba(0.0, 0.0, 0.0, fade.a.clamp(0.0, 1.0)));
    }
    if let Ok((mut t, mut colour)) = toast.single_mut() {
        if t.0 != ui.toast.text {
            t.0 = ui.toast.text.clone();
        }
        let want = if ui.toast.visible { TOAST } else { Color::NONE };
        if colour.0 != want {
            colour.0 = want;
        }
    }
}

/// Only write through the `Mut` when the value really changed, so the UI layout is not re-run.
fn set(c: &mut Mut<BackgroundColor>, want: Color) {
    if c.0 != want {
        c.0 = want;
    }
}
