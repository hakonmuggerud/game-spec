//! The render rig and the first-person camera. Ports `main.js`'s camera block (`camera` +
//! `lamp` at `main.js:46–75`, `updatePlayer`'s camera tail at `main.js:118–125`, `updateLamp`'s
//! light numbers at `main.js:150–152`, `resize()` at `main.js:595–600`, `fx.shake` /`applyFx` at
//! `main.js:605–617` and the death camera in `die()` at `main.js:383–394`).
//!
//! # The rig
//!
//! ```text
//! WorldCamera3d  Camera3d, order 0, RenderTarget::Image(target)   ← the whole 3D scene
//!   └ Handlamp   PointLight at (0.25, −0.2, 0), CFG.lampColor
//! WorldCamera2d  Camera2d, order 1, IsDefaultUiCamera             ← presents the image
//!   (the sprite is a separate entity at the origin)
//! WorldPresenter Sprite { image: target, custom_size: window }
//! ```
//!
//! `renderer.setSize(w / 3, h / 3)` plus the canvas's `image-rendering: pixelated` becomes an
//! offscreen `Image` of `(w / 3, h / 3)` with [`ImageSampler::nearest`], blown back up by a
//! full-screen sprite on the 2D camera. `bevy_ui` attaches to that camera
//! ([`IsDefaultUiCamera`]) and therefore draws at full resolution on top, as `ui.js`'s DOM did.

use bevy::camera::ImageRenderTarget;
use bevy::camera::{Exposure, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::ImageSampler;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::render::view::Msaa;
use bevy::ui::IsDefaultUiCamera;
use bevy::window::{PrimaryWindow, WindowResized};
use undercroft_data::Config;
use undercroft_sim::{economy, SimEvent};

use crate::messages::SimMessage;
use crate::resources::{Game, LampRes, Player, PlayerViewRes, ZoneRes};
use crate::state::GameMode;
use crate::tick::Clock;

use super::palette::{rgb, AMBIENT_BRIGHTNESS, EXPOSURE_EV100};

/// The 3D camera (`main.js:46 camera`). The creatures / hub / ui lanes may query it, but only the
/// world lane spawns or moves it.
#[derive(Component, Debug, Clone, Copy)]
pub struct WorldCamera3d;

/// The 2D camera that presents the ⅓-resolution image and carries [`IsDefaultUiCamera`].
#[derive(Component, Debug, Clone, Copy)]
pub struct WorldCamera2d;

/// The full-screen sprite showing the render target.
#[derive(Component, Debug, Clone, Copy)]
pub struct WorldPresenter;

/// The handlamp (`main.js:73 lamp`), a child of [`WorldCamera3d`].
#[derive(Component, Debug, Clone, Copy)]
pub struct Handlamp;

/// The offscreen render target, so the resize system can find it.
#[derive(Resource, Debug, Clone)]
pub struct RenderRig {
    pub target: Handle<Image>,
    /// Logical window size the target was last sized for.
    pub window: UVec2,
}

/// `main.js:608 const fx` — the screen shake, and the death camera's frozen yaw.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ScreenFx {
    /// `fx.amp`.
    pub amp: f32,
    /// `fx.t` — seconds left of the 0.25 s decay.
    pub t: f32,
    /// `die()`'s `player.yaw = atan2(-ux, -uz)`, held for the whole `DYING` mode.
    pub death_yaw: Option<f32>,
}

impl ScreenFx {
    /// `fx.dur`.
    pub const DUR: f32 = 0.25;

    /// `fx.shake(amount)` — the strongest shake in flight wins.
    pub fn shake(&mut self, amount: f32) {
        if amount > 0.0 {
            self.amp = self.amp.max(amount);
            self.t = Self::DUR;
        }
    }
}

/// The ⅓-resolution target for a logical window size, never smaller than 1×1
/// (`main.js:597 Math.max(1, Math.floor(w / 3))`).
pub fn target_size(window: UVec2) -> UVec2 {
    UVec2::new((window.x / 3).max(1), (window.y / 3).max(1))
}

/// A fresh render-target image with nearest filtering.
fn make_target(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::nearest();
    image
}

/// Build the rig. Runs once at `Startup`; the config-driven numbers (fov, lamp colour) are patched
/// in [`follow_player`] / [`update_lamp_light`] as soon as the data asset is loaded.
pub(super) fn spawn_rig(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let window = windows
        .single()
        .map(|w| UVec2::new(w.resolution.width() as u32, w.resolution.height() as u32))
        .unwrap_or(UVec2::new(1280, 720));
    let target = images.add(make_target(target_size(window)));
    commands.insert_resource(RenderRig {
        target: target.clone(),
        window,
    });

    let cam = commands
        .spawn((
            Name::new("world:camera3d"),
            WorldCamera3d,
            Camera3d::default(),
            Camera {
                order: 0,
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: target.clone(),
                scale_factor: 1.0,
            }),
            Projection::Perspective(PerspectiveProjection {
                fov: 75f32.to_radians(),
                near: 0.05,
                far: 40.0,
                ..default()
            }),
            // three had no tone mapping and no MSAA (`antialias: false`); the pixels must stay hard.
            Tonemapping::None,
            Msaa::Off,
            Exposure {
                ev100: EXPOSURE_EV100,
            },
            DistanceFog {
                color: Color::BLACK,
                falloff: FogFalloff::ExponentialSquared { density: 0.11 },
                ..default()
            },
            AmbientLight {
                color: rgb(0x0b0a14),
                brightness: AMBIENT_BRIGHTNESS,
                affects_lightmapped_meshes: true,
            },
            Transform::from_xyz(0.0, 1.6, 0.0),
        ))
        .id();
    commands.entity(cam).with_children(|p| {
        p.spawn((
            Name::new("world:handlamp"),
            Handlamp,
            PointLight {
                color: rgb(0xffb265),
                intensity: 0.0,
                range: 11.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.25, -0.2, 0.0),
            Visibility::Hidden,
        ));
    });

    commands.spawn((
        Name::new("world:camera2d"),
        WorldCamera2d,
        Camera2d,
        Camera {
            order: 1,
            ..default()
        },
        IsDefaultUiCamera,
        Msaa::Off,
    ));
    commands.spawn((
        Name::new("world:presenter"),
        WorldPresenter,
        Sprite {
            image: target,
            custom_size: Some(window.as_vec2()),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

/// `main.js:595 resize()` — the target follows the window at a third of its size.
pub(super) fn on_resize(
    mut resized: MessageReader<WindowResized>,
    mut rig: ResMut<RenderRig>,
    mut images: ResMut<Assets<Image>>,
    mut sprites: Query<&mut Sprite, With<WorldPresenter>>,
) {
    let Some(last) = resized.read().last() else {
        return;
    };
    let window = UVec2::new((last.width as u32).max(1), (last.height as u32).max(1));
    if window == rig.window {
        return;
    }
    rig.window = window;
    let size = target_size(window);
    if let Some(mut image) = images.get_mut(&rig.target) {
        image.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    for mut sprite in &mut sprites {
        sprite.custom_size = Some(window.as_vec2());
    }
}

/// `main.js:118` — the camera *is* the player: position `(x, CFG.eye, z)`, rotation `YXZ(yaw, pitch)`,
/// the sprint fov ease, then `applyFx`'s shake on top. Runs in `PostUpdate` so it sees this frame's
/// fixed-tick results.
pub(super) fn follow_player(
    time: Res<Time>,
    game: Game,
    player: Res<Player>,
    clock: Res<Clock>,
    mode: Res<State<GameMode>>,
    mut fx: ResMut<ScreenFx>,
    mut cam: Query<(&mut Transform, &mut Projection), With<WorldCamera3d>>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let cfg = &asset.data.config.cfg;
    let dt = time.delta_secs();
    let Ok((mut tf, mut proj)) = cam.single_mut() else {
        return;
    };

    // `die()` turned the camera to face what took you and neither the player nor the creatures run
    // while DYING, so the yaw stays frozen for the whole death camera.
    let dying = *mode.get() == GameMode::Dying;
    if !dying {
        fx.death_yaw = None;
    }
    let yaw = fx.death_yaw.filter(|_| dying).unwrap_or(player.yaw);
    let pitch = if dying && fx.death_yaw.is_some() {
        0.0
    } else {
        player.pitch
    };

    // `applyFx(dt)`: a pitch jitter `amp · k · sin(28 t)` decaying over 0.25 s plus a 0.01 u eye dip.
    let mut shake_pitch = 0.0;
    let mut dip = 0.0;
    if fx.t > 0.0 {
        fx.t = (fx.t - dt).max(0.0);
        let k = fx.t / ScreenFx::DUR;
        shake_pitch = fx.amp * k * (28.0 * clock.time).sin();
        dip = 0.01 * k;
    } else {
        fx.amp = 0.0;
    }

    tf.translation = Vec3::new(player.x, cfg.eye - dip, player.z);
    tf.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch + shake_pitch, 0.0);

    if let Projection::Perspective(p) = &mut *proj {
        let target = if player.sprinting {
            cfg.fov_sprint
        } else {
            cfg.fov
        }
        .to_radians();
        p.fov += (target - p.fov) * (dt * 8.0).min(1.0);
        p.near = 0.05;
        p.far = 40.0;
    }
}

/// `main.js:150` — the handlamp's intensity (`economy::lamp_intensity`, i.e. the flicker, the
/// low-oil triple amplitude and the flash override), its reach (`PlayerView::lamp_reach`, already
/// `CFG.lampDist × zoneMul().dist`) and `lamp.visible = inten > 0`.
pub(super) fn update_lamp_light(
    game: Game,
    lamp_state: Res<LampRes>,
    view: Res<PlayerViewRes>,
    clock: Res<Clock>,
    mut q: Query<(&mut PointLight, &mut Visibility), With<Handlamp>>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let cfg: &Config = &asset.data.config;
    let inten = economy::lamp_intensity(&lamp_state.0, cfg, clock.time);
    for (mut light, mut vis) in &mut q {
        light.color = rgb(cfg.cfg.lamp_color);
        light.intensity = inten;
        light.range = view.0.lamp_reach;
        *vis = if inten > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// `main.js:756` — a Brute's stride within `CREATURE.brute.shakeR` shakes the screen, and
/// `die()`'s death camera turns to face the killer.
pub(super) fn on_sim_events(
    mut msgs: MessageReader<SimMessage>,
    game: Game,
    zone: Res<ZoneRes>,
    player: Res<Player>,
    mode: Res<State<GameMode>>,
    mut fx: ResMut<ScreenFx>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let brute = &asset.data.config.creature.brute;
    for m in msgs.read() {
        match &m.0 {
            SimEvent::CreatureStep { d, .. } => {
                if *mode.get() == GameMode::Zone && *d <= brute.shake_r {
                    fx.shake(brute.shake_amp * (1.0 - d / brute.shake_r));
                }
            }
            SimEvent::Death { hunter_id, .. } => {
                fx.death_yaw = hunter_id
                    .and_then(|id| {
                        zone.get()
                            .and_then(|z| z.hunters.iter().find(|h| h.id == id))
                    })
                    .map(|h| {
                        let (dx, dz) = (h.x - player.x, h.z - player.z);
                        let d = dx.hypot(dz).max(1e-4);
                        // `player.yaw = Math.atan2(-ux, -uz)` — look straight at it.
                        (-dx / d).atan2(-dz / d)
                    });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_a_third_of_the_window_and_never_zero() {
        assert_eq!(target_size(UVec2::new(1280, 800)), UVec2::new(426, 266));
        assert_eq!(target_size(UVec2::new(2, 2)), UVec2::new(1, 1));
        assert_eq!(target_size(UVec2::ZERO), UVec2::new(1, 1));
    }

    /// The camera transform is `(x, CFG.eye, z)` with `YXZ(yaw, pitch)` — forward is `−Z` rotated by
    /// yaw, which is what `collision::move_player` assumes (`forward = (−sin yaw, −cos yaw)`).
    #[test]
    fn camera_transform_matches_the_player() {
        let (x, z, yaw, pitch, eye) = (3.5, 7.25, 0.9f32, -0.3f32, 1.6);
        let tf = Transform {
            translation: Vec3::new(x, eye, z),
            rotation: Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0),
            scale: Vec3::ONE,
        };
        assert_eq!(tf.translation, Vec3::new(3.5, 1.6, 7.25));
        // With pitch 0 the forward vector is the JS `(−sin yaw, −cos yaw)`.
        let flat = Transform {
            rotation: Quat::from_euler(EulerRot::YXZ, yaw, 0.0, 0.0),
            ..tf
        };
        let f = flat.forward();
        assert!((f.x - -yaw.sin()).abs() < 1e-5, "{f:?}");
        assert!((f.z - -yaw.cos()).abs() < 1e-5, "{f:?}");
    }

    /// `fx.shake` keeps the strongest amplitude and restarts the 0.25 s decay.
    #[test]
    fn shake_keeps_the_strongest_and_ignores_zero() {
        let mut fx = ScreenFx::default();
        fx.shake(0.0);
        assert_eq!(fx.t, 0.0);
        fx.shake(0.004);
        fx.t = 0.1;
        fx.shake(0.002);
        assert_eq!(
            fx.amp, 0.004,
            "the weaker shake does not lower the amplitude"
        );
        assert_eq!(fx.t, ScreenFx::DUR, "but it does restart the decay");
    }

    /// The death camera looks straight at the killer: yaw `atan2(−ux, −uz)`.
    #[test]
    fn death_yaw_faces_the_killer() {
        // killer due north (−z): forward must be −z, i.e. yaw 0.
        let (dx, dz) = (0.0f32, -2.0f32);
        let d = dx.hypot(dz);
        let yaw = (-dx / d).atan2(-dz / d);
        assert!(yaw.abs() < 1e-6, "{yaw}");
        // killer due east (+x): forward −z rotated by yaw must point +x, i.e. yaw = −π/2.
        let (dx, dz) = (2.0f32, 0.0f32);
        let d = dx.hypot(dz);
        let yaw = (-dx / d).atan2(-dz / d);
        let f = Quat::from_euler(EulerRot::YXZ, yaw, 0.0, 0.0) * Vec3::NEG_Z;
        assert!((f.x - 1.0).abs() < 1e-5, "{f:?}");
    }
}
