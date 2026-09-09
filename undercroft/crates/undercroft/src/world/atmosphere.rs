//! `world.js:applyAtmosphere(id)` / `applyHubWarmth(tier)` and `endgame.js:applyLapTint` — fog,
//! ambient light and the clear colour, per zone and per flame tier.
//!
//! three kept these on the scene (`scene.fog`, `scene.background`, one `AmbientLight`); Bevy keeps
//! fog and ambient on the *camera* ([`DistanceFog`], [`AmbientLight`] as a component) and the clear
//! colour in the [`ClearColor`] resource. `FogExp2` of three is
//! `1 − exp(−(d·density)²)`, which is exactly [`FogFalloff::ExponentialSquared`]
//! (`bevy_pbr-0.19.1/src/render/fog.wgsl:53`).

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use undercroft_data::zone::Palette;
use undercroft_sim::economy;
use undercroft_sim::SimEvent;

use crate::messages::SimMessage;
use crate::resources::{Game, HubRes, ZoneRes};

use super::camera::WorldCamera3d;
use super::palette::{palette_for, rgb, AMBIENT_BRIGHTNESS};

/// `world.js:atmos.current` — which palette is showing, and the hub tier it was warmed to.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Atmosphere {
    /// `hub` or a zone id.
    pub current: String,
    /// The flame tier the hub warmth was applied at (`HUB_WARMTH`), 0 outside the hub.
    pub tier: u32,
    /// The Source's eased lap the tint was last applied at.
    pub v_lap: f32,
}

/// The three numbers a palette (plus hub warmth, plus the lap tint) resolves to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atmos {
    pub fog_color: Color,
    pub fog_density: f32,
    pub ambient: Color,
    pub sky: Color,
}

/// `applyAtmosphere` + `applyHubWarmth`: the palette's own numbers, replaced in the hub by
/// `HUB_WARMTH`'s per-tier ambient and fog (`tier` is 1-based; zones pass 0).
pub fn atmos_of(
    pal: &Palette,
    warmth: Option<(&undercroft_data::config::HubWarmth, u32)>,
) -> Atmos {
    let mut a = Atmos {
        fog_color: rgb(pal.fog.color),
        fog_density: pal.fog.density,
        ambient: rgb(pal.ambient),
        sky: rgb(pal.sky),
    };
    if let Some((w, tier)) = warmth {
        let i = (tier.max(1) - 1).min(3) as usize;
        a.ambient = rgb(w.ambient[i]);
        a.fog_color = rgb(w.fog.color[i]);
        a.fog_density = w.fog.density;
    }
    a
}

/// `endgame.js:applyLapTint(vLap)` — the Source only: fog thickens, its colour bleeds toward
/// `0x08001a` and the ambient dims, all from [`economy::lap_tint`].
pub fn apply_lap_tint(a: Atmos, cfg: &undercroft_data::Config, v_lap: f32) -> Atmos {
    let (density_mul, tint_t, ambient_mul) = economy::lap_tint(cfg, v_lap);
    let tint = rgb(0x08_001a).to_linear();
    let fog = a.fog_color.to_linear();
    Atmos {
        fog_color: Color::LinearRgba(LinearRgba::new(
            fog.red + (tint.red - fog.red) * tint_t,
            fog.green + (tint.green - fog.green) * tint_t,
            fog.blue + (tint.blue - fog.blue) * tint_t,
            1.0,
        )),
        fog_density: a.fog_density * density_mul,
        ambient: {
            let c = a.ambient.to_linear();
            Color::LinearRgba(LinearRgba::new(
                c.red * ambient_mul,
                c.green * ambient_mul,
                c.blue * ambient_mul,
                1.0,
            ))
        },
        sky: a.sky,
    }
}

/// `zoneEnter` / `hubEnter` / `flameTier` switch the palette; the Source re-tints every frame as
/// `SourceRun::v_lap` eases toward the real lap.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_atmosphere(
    mut msgs: MessageReader<SimMessage>,
    game: Game,
    zone: Res<ZoneRes>,
    hub: Res<HubRes>,
    mut atmos: ResMut<Atmosphere>,
    mut clear: ResMut<ClearColor>,
    mut cam: Query<(&mut DistanceFog, &mut AmbientLight), With<WorldCamera3d>>,
) {
    let Some(asset) = game.get() else {
        return;
    };
    let data = &asset.data;
    let mut want = atmos.current.clone();
    for m in msgs.read() {
        match &m.0 {
            SimEvent::HubEnter => want = "hub".to_string(),
            SimEvent::ZoneEnter { zone_id } => want = zone_id.clone(),
            _ => {}
        }
    }
    // `applyAtmosphere` falls back to the hub palette for an unknown id, and the boot state is the
    // hub (`world.js:init` → `applyAtmosphere('hub')`).
    if want.is_empty() {
        want = "hub".to_string();
    }
    let in_hub = want == "hub" || data.zones.iter().all(|z| z.id != want);
    let tier = if in_hub { hub.0.tier } else { 0 };
    let v_lap = zone
        .get()
        .filter(|z| z.id == want)
        .and_then(|z| z.source.as_ref())
        .map(|r| r.v_lap)
        .unwrap_or(0.0);
    if atmos.current == want && atmos.tier == tier && (atmos.v_lap - v_lap).abs() < 1e-4 {
        return;
    }
    *atmos = Atmosphere {
        current: want.clone(),
        tier,
        v_lap,
    };

    let pal = palette_for(data, &want);
    let warmth = in_hub.then_some((&data.config.hub_warmth, tier));
    let mut a = atmos_of(pal, warmth);
    if v_lap > 0.0 {
        a = apply_lap_tint(a, &data.config, v_lap);
    }
    debug!(
        "world: atmosphere {want} tier={tier} ambient={:?} fog={:?} density={} lap={v_lap}",
        a.ambient.to_srgba(),
        a.fog_color.to_srgba(),
        a.fog_density
    );
    // The title screen looks over the hub (`main.js:767`), so the atmosphere applies in every mode.
    clear.0 = a.sky;
    for (mut fog, mut ambient) in &mut cam {
        fog.color = a.fog_color;
        fog.falloff = FogFalloff::ExponentialSquared {
            density: a.fog_density,
        };
        ambient.color = a.ambient;
        ambient.brightness = AMBIENT_BRIGHTNESS;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::config::{HubFog, HubWarmth, LanternLight};
    use undercroft_data::zone::Fog;

    fn pal() -> Palette {
        Palette {
            floor: 0,
            wall: 0,
            pillar: 0,
            ceil: 0,
            deep: 0,
            water: 0,
            water_surface: 0,
            water_glow: 0,
            fog: Fog {
                color: 0x000000,
                density: 0.11,
            },
            ambient: 0x0b0a14,
            sky: 0x010203,
        }
    }

    fn warmth() -> HubWarmth {
        HubWarmth {
            ambient: [0x1c140d, 0x2a1d12, 0x3a2917, 0x4c3620],
            fog: HubFog {
                color: [0x050302, 0x080504, 0x0b0705, 0x0f0a06],
                density: 0.10,
            },
            lantern_glow: 1.0,
            lantern_light: LanternLight {
                color: 0xffb060,
                int: 1.2,
                dist: 5.5,
            },
        }
    }

    /// A zone takes its palette verbatim.
    #[test]
    fn zone_atmosphere_is_the_palette() {
        let a = atmos_of(&pal(), None);
        assert_eq!(a.fog_density, 0.11);
        assert_eq!(a.ambient, rgb(0x0b0a14));
        assert_eq!(a.sky, rgb(0x010203));
    }

    /// The hub swaps ambient and fog for the tier's `HUB_WARMTH` entry and clamps the index.
    #[test]
    fn hub_warmth_overrides_ambient_and_fog_per_tier() {
        let w = warmth();
        let t1 = atmos_of(&pal(), Some((&w, 1)));
        assert_eq!(t1.ambient, rgb(0x1c140d));
        assert_eq!(t1.fog_color, rgb(0x050302));
        assert_eq!(t1.fog_density, 0.10);
        let t4 = atmos_of(&pal(), Some((&w, 4)));
        assert_eq!(t4.ambient, rgb(0x4c3620));
        assert_eq!(atmos_of(&pal(), Some((&w, 9))), t4, "tier clamps to 4");
        assert_eq!(atmos_of(&pal(), Some((&w, 0))), t1, "tier 0 reads as 1");
    }
}
