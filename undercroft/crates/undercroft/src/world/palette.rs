//! Colours and light units. Ports `maps.js:PALETTES` access (`world.js:paletteFor`) and fixes the one
//! conversion every lane needs: the RON stores `0xRRGGBB` sRGB integers, exactly what
//! `THREE.Color.set(hex)` took.
//!
//! # Light units
//!
//! The prototype ran three r160 with physically-correct lights: a `PointLight`'s `intensity` is
//! **candela** and its `decay = 2` gives `irradiance = intensity / d²`, windowed by `distance`.
//! Bevy's [`PointLight::intensity`] is **lumens**, divided by `4π` to get candela
//! (`bevy_pbr-0.19.1/src/render/light.rs:537`) and then multiplied by the camera's
//! [`bevy::camera::Exposure`] (`pbr_functions.wgsl:864`).
//!
//! So a light contributes `intensity / 4π · exposure`, where the default exposure (`EV100 9.7`,
//! Blender's) is about `1 / 1000` — hence Bevy's habit of five-digit lumen values.
//!
//! Rather than scatter a magic factor over five lanes, the world lane sets the 3D camera's exposure
//! to exactly `4π` ([`EXPOSURE_EV100`] = `−log2(4π · 1.2)` ≈ −3.9146). The two factors cancel and
//! **`PointLight::intensity` is then numerically the JS candela value** — `CFG.lampInt` 6.0 is
//! `intensity: 6.0`, a planted lantern `2.0`, the Warden's spot its `light.int`. Ambient needs the
//! matching `1 / (π · exposure) = 1 / 4π²` ([`AMBIENT_BRIGHTNESS`]), because three's Lambert adds
//! `ambientColor · albedo / π`.
//!
//! One consequence for the other lanes: `emissive` is *not* multiplied by exposure when its alpha is
//! 0 (`pbr_functions.wgsl:841`), which is what we want (three added `emissive · emissiveIntensity`
//! straight into the fragment). Always build emissive with alpha 0 — [`emissive`] does.

use bevy::color::LinearRgba;
use bevy::prelude::*;
use undercroft_data::zone::Palette;
use undercroft_data::GameData;

/// `0xRRGGBB` (sRGB, as `THREE.Color.set(hex)` reads it) → a Bevy [`Color`].
///
/// Other lanes may call this, or keep a private identical `rgb_u32` while the lanes run in parallel
/// (PHASE2_LANES §1).
pub fn rgb(hex: u32) -> Color {
    Color::srgb_u8(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// [`rgb`] in linear space — what a vertex-colour attribute and `StandardMaterial::emissive` want.
pub fn linear(hex: u32) -> LinearRgba {
    LinearRgba::from(rgb(hex))
}

/// `emissive(mesh, hex, k)` of `models.js`: the colour times its `emissiveIntensity`, with **alpha 0**
/// so Bevy leaves it out of the exposure scaling (see the module docs).
pub fn emissive(hex: u32, k: f32) -> LinearRgba {
    let c = linear(hex);
    LinearRgba::new(c.red * k, c.green * k, c.blue * k, 0.0)
}

/// Camera exposure that makes [`PointLight::intensity`] equal to the JS candela value:
/// `Exposure::exposure() = 2^-ev100 / 1.2 = 4π`, i.e. `ev100 = −log2(4π · 1.2)`.
pub const EXPOSURE_EV100: f32 = -3.914_561;

/// [`AmbientLight::brightness`] that reproduces three's `AmbientLight(color, 1)` under
/// [`EXPOSURE_EV100`]: `1 / (π · exposure) = 1 / 4π²`.
pub const AMBIENT_BRIGHTNESS: f32 = 0.025_330_296;

/// The Lambert look every lane uses (PHASE2_LANES §1): rough, no specular, no shadows.
pub fn lambert(base: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: base,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        ..default()
    }
}

/// `world.js:paletteFor(id)` — the zone palette, falling back to `PALETTES.hub`.
pub fn palette_for<'a>(data: &'a GameData, id: &str) -> &'a Palette {
    if let Some(z) = data.zones.iter().find(|z| z.id == id) {
        return &z.palette;
    }
    data.palettes
        .get(id)
        .or_else(|| data.palettes.get("hub"))
        .expect("palettes.ron always has `hub`")
}

/// A deterministic ±`amp` jitter for cell `(cx, cz)` of kind `k` — the port of `buildBlocks`'s
/// `1 + (Math.random() * 2 - 1) * 0.06`. Hashed instead of random so the mesh builders are pure and
/// testable (PHASE2_LANES §0).
pub fn jitter(cx: i32, cz: i32, k: u32, amp: f32) -> f32 {
    // xorshift-ish integer hash; only its low 16 bits are used.
    let mut h = (cx as u32)
        .wrapping_mul(0x9e37_79b9)
        .wrapping_add((cz as u32).wrapping_mul(0x85eb_ca6b))
        .wrapping_add(k.wrapping_mul(0xc2b2_ae35));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_f491);
    h ^= h >> 13;
    let u = (h & 0xffff) as f32 / 65535.0; // 0..1
    1.0 + (u * 2.0 - 1.0) * amp
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::Exposure;
    use core::f32::consts::PI;

    /// `EXPOSURE_EV100` really does cancel Bevy's `lumens / 4π`, so `intensity` is candela.
    #[test]
    fn exposure_makes_intensity_candela() {
        let e = Exposure {
            ev100: EXPOSURE_EV100,
        };
        assert!(
            (e.exposure() - 4.0 * PI).abs() < 1e-3,
            "exposure {} != 4pi {}",
            e.exposure(),
            4.0 * PI
        );
        // lumens -> candela -> exposure: a light of `intensity` ends up at `intensity` candela.
        let intensity = 6.0f32;
        assert!(((intensity / (4.0 * PI)) * e.exposure() - intensity).abs() < 1e-3);
    }

    /// Ambient: `exposure · brightness = 1/π`, three's `BRDF_Lambert`.
    #[test]
    fn ambient_brightness_matches_three() {
        let e = Exposure {
            ev100: EXPOSURE_EV100,
        };
        assert!((e.exposure() * AMBIENT_BRIGHTNESS - 1.0 / PI).abs() < 1e-4);
    }

    #[test]
    fn rgb_reads_hex_as_srgb() {
        let c = rgb(0x6e675e).to_srgba();
        assert_eq!(
            (
                (c.red * 255.0).round() as u8,
                (c.green * 255.0).round() as u8,
                (c.blue * 255.0).round() as u8
            ),
            (0x6e, 0x67, 0x5e)
        );
    }

    #[test]
    fn emissive_has_zero_alpha_and_scales_by_k() {
        let e = emissive(0xffc070, 0.5);
        assert_eq!(e.alpha, 0.0);
        let full = linear(0xffc070);
        assert!((e.red - full.red * 0.5).abs() < 1e-6);
    }

    #[test]
    fn jitter_is_deterministic_and_bounded() {
        for cx in 0..8 {
            for cz in 0..8 {
                let j = jitter(cx, cz, 3, 0.06);
                assert!((0.94..=1.06).contains(&j), "{j}");
                assert_eq!(j, jitter(cx, cz, 3, 0.06));
            }
        }
        assert_ne!(jitter(1, 2, 0, 0.06), jitter(1, 2, 1, 0.06));
    }
}
