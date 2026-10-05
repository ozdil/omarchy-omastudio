//! Capture One Pro Style Skin Tone Uniformity & Color Editor
//!
//! Provides perceptual Oklch-based skin tone targeted processing:
//! - Optical skin tone isolation around the 123° vectorscope flesh line.
//! - Capture One style Hue, Saturation, and Lightness Uniformity (homogenization).
//! - Target color matching without destroying micro-texture or facial highlights.

use crate::pipeline::color_grading::{linear_rgb_to_oklab, oklab_to_linear_rgb};
use crate::recipe::Recipe;

/// Standard photographic vectorscope skin line angle in Oklch space (approx 45° to 55° in Oklab,
/// corresponding to 123° in traditional Rec.709 CbCr vectorscope).
pub const DEFAULT_SKIN_HUE_OKLCH: f32 = 50.0;
pub const DEFAULT_SKIN_HUE_TOLERANCE: f32 = 32.0;

/// Evaluates whether a pixel falls within the human skin color volume in Oklab / Oklch space.
/// Returns a soft membership weight w in [0.0, 1.0].
#[inline(always)]
pub fn calculate_skin_weight(
    hue: f32,
    chroma: f32,
    luma: f32,
    target_hue: f32,
    hue_tolerance: f32,
) -> f32 {
    // 1. Luminance gating: Skin requires reasonable light (reject deep shadows and pure specular blowouts)
    if luma < 0.08 || luma > 0.98 {
        return 0.0;
    }
    let luma_gate = if luma < 0.20 {
        (luma - 0.08) / 0.12
    } else if luma > 0.90 {
        (0.98 - luma) / 0.08
    } else {
        1.0
    };

    // 2. Chroma gating: Neutral grays or super-saturated neons are not natural skin
    if chroma < 0.025 || chroma > 0.28 {
        return 0.0;
    }
    let chroma_gate = if chroma < 0.045 {
        (chroma - 0.025) / 0.020
    } else if chroma > 0.22 {
        (0.28 - chroma) / 0.060
    } else {
        1.0
    };

    // 3. Hue distance around target skin hue
    let mut diff = (hue - target_hue).abs();
    if diff > 180.0 {
        diff = 360.0 - diff;
    }

    if diff >= hue_tolerance {
        return 0.0;
    }

    // Cosine roll-off for smooth transition (zero stepping or boundary artifacts)
    let hue_gate = 0.5 * (1.0 + (std::f32::consts::PI * diff / hue_tolerance).cos());

    (luma_gate * chroma_gate * hue_gate).clamp(0.0, 1.0)
}

/// Applies Capture One style Skin Tone Uniformity & Color Transformation
#[inline(always)]
pub fn apply_skin_tone_uniformity(
    r: f32,
    g: f32,
    b: f32,
    recipe: &Recipe,
) -> (f32, f32, f32) {
    // Fast exit if skin tone module is inactive
    if !recipe.skin_tone_enabled {
        return (r, g, b);
    }

    let target_hue = if recipe.skin_target_hue > 0.0 {
        recipe.skin_target_hue
    } else {
        DEFAULT_SKIN_HUE_OKLCH
    };
    let hue_tol = if recipe.skin_hue_range > 0.0 {
        recipe.skin_hue_range
    } else {
        DEFAULT_SKIN_HUE_TOLERANCE
    };

    let (l, a, b_val) = linear_rgb_to_oklab(r, g, b);
    let chroma = (a * a + b_val * b_val).sqrt();
    let mut hue = b_val.atan2(a).to_degrees();
    if hue < 0.0 {
        hue += 360.0;
    }

    let skin_weight = calculate_skin_weight(hue, chroma, l, target_hue, hue_tol);
    if skin_weight <= 0.001 {
        return (r, g, b);
    }

    // Capture One Uniformity Sliders (0..100)
    let u_hue = (recipe.skin_uniformity_hue.clamp(0.0, 100.0) / 100.0) * skin_weight;
    let u_sat = (recipe.skin_uniformity_sat.clamp(0.0, 100.0) / 100.0) * skin_weight;

    // Capture One Amount Sliders (-100..+100)
    let delta_hue = (recipe.skin_amount_hue.clamp(-100.0, 100.0) / 100.0) * 15.0; // +-15 deg
    let delta_sat = (recipe.skin_amount_sat.clamp(-100.0, 100.0) / 100.0) * 0.40; // +-40% sat

    // 1. Hue Uniformity: pull current hue toward target skin hue
    let mut diff = target_hue - hue;
    if diff > 180.0 {
        diff -= 360.0;
    } else if diff < -180.0 {
        diff += 360.0;
    }
    let mut new_hue = hue + (diff * u_hue) + (delta_hue * skin_weight);
    if new_hue < 0.0 {
        new_hue += 360.0;
    } else if new_hue >= 360.0 {
        new_hue -= 360.0;
    }

    // 2. Saturation Uniformity: pull chroma toward target nominal skin chroma (~0.12 Oklab)
    let nominal_skin_chroma = 0.115f32;
    let new_chroma = (chroma + (nominal_skin_chroma - chroma) * u_sat + chroma * delta_sat * skin_weight)
        .max(0.0);

    let rad = new_hue.to_radians();
    let new_a = new_chroma * rad.cos();
    let new_b = new_chroma * rad.sin();

    oklab_to_linear_rgb(l, new_a, new_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skin_detection_and_uniformity() {
        let mut recipe = Recipe::default();
        recipe.skin_tone_enabled = true;
        recipe.skin_uniformity_hue = 80.0;
        recipe.skin_target_hue = 50.0;

        // Warm reddish skin pixel in linear RGB
        let (r, g, b) = (0.75, 0.55, 0.45);
        let (r2, g2, b2) = apply_skin_tone_uniformity(r, g, b, &recipe);

        // Should modify the skin tone smoothly
        assert!(r2 > 0.0 && g2 > 0.0 && b2 > 0.0);
        assert!((r - r2).abs() < 0.25);

        // Pure blue sky pixel should be untouched
        let (sky_r, sky_g, sky_b) = (0.1, 0.4, 0.95);
        let (sr2, sg2, sb2) = apply_skin_tone_uniformity(sky_r, sky_g, sky_b, &recipe);
        assert!((sky_r - sr2).abs() < 1e-4);
        assert!((sky_g - sg2).abs() < 1e-4);
        assert!((sky_b - sb2).abs() < 1e-4);
    }
}
