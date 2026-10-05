//! DaVinci Resolve Style RGB Primary Matrix Mixer & Cross-Talk Engine
//!
//! Allows creative colorists to blend R, G, B channel contributions:
//! - Preserves photonic linearity and energy conservation.
//! - Enables classic Hollywood film looks (Technicolor 3-Strip, bleach bypass, teal/orange separation).
//! - Clean monochrome matrix calculation with customized spectral weights.

use crate::recipe::Recipe;

/// Applies 3x3 RGB Mixer matrix transformations to an RGB pixel
#[inline(always)]
pub fn apply_rgb_mixer(r: f32, g: f32, b: f32, recipe: &Recipe) -> (f32, f32, f32) {
    if !recipe.rgb_mixer_enabled {
        return (r, g, b);
    }

    if recipe.rgb_mixer_monochrome {
        // Monochromatic mix based on user channel ratios
        // Normalized so default sum is 1.0
        let total = (recipe.mixer_red_in_r + recipe.mixer_green_in_r + recipe.mixer_blue_in_r).abs();
        let norm = if total > 1e-4 { total } else { 1.0 };

        let mono = (r * recipe.mixer_red_in_r + g * recipe.mixer_green_in_r + b * recipe.mixer_blue_in_r) / norm;
        let clamped = mono.max(0.0);
        return (clamped, clamped, clamped);
    }

    // 3x3 Matrix Multiplication:
    // Out_R = R*rr + G*gr + B*br
    // Out_G = R*rg + G*gg + B*bg
    // Out_B = R*rb + G*gb + B*bb
    let out_r = (r * recipe.mixer_red_in_r + g * recipe.mixer_green_in_r + b * recipe.mixer_blue_in_r).max(0.0);
    let out_g = (r * recipe.mixer_red_in_g + g * recipe.mixer_green_in_g + b * recipe.mixer_blue_in_g).max(0.0);
    let out_b = (r * recipe.mixer_red_in_b + g * recipe.mixer_green_in_b + b * recipe.mixer_blue_in_b).max(0.0);

    (out_r, out_g, out_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_rgb_mixer() {
        let recipe = Recipe::default();
        let (r, g, b) = (0.35, 0.65, 0.85);
        let (r2, g2, b2) = apply_rgb_mixer(r, g, b, &recipe);
        assert_eq!((r, g, b), (r2, g2, b2));
    }

    #[test]
    fn test_monochrome_rgb_mixer() {
        let mut recipe = Recipe::default();
        recipe.rgb_mixer_enabled = true;
        recipe.rgb_mixer_monochrome = true;
        recipe.mixer_red_in_r = 0.3;
        recipe.mixer_green_in_r = 0.6;
        recipe.mixer_blue_in_r = 0.1;

        let (r, g, b) = apply_rgb_mixer(1.0, 0.5, 0.0, &recipe);
        assert!((r - g).abs() < 1e-5);
        assert!((g - b).abs() < 1e-5);
        assert!(r > 0.0);
    }
}
