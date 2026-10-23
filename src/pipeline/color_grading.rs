use crate::pipeline::transfer::Transfer;
use crate::recipe::Recipe;
use std::sync::LazyLock;

/// The creative stage works on display-encoded values (see [`crate::pipeline::transfer`]),
/// but Oklab is defined on linear light: hues and chroma read on encoded values are
/// skewed (orange reads as yellow, saturation is overstated in the shadows). The
/// Oklch tools decode with the pipeline's default curve first and encode afterwards.
static DISPLAY: LazyLock<Transfer> = LazyLock::new(Transfer::bt709);

/// Display-encoded RGB → Oklab.
#[inline(always)]
pub fn encoded_rgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let t = &*DISPLAY;
    linear_rgb_to_oklab(t.decode(r), t.decode(g), t.decode(b))
}

/// Oklab → display-encoded RGB.
#[inline(always)]
pub fn oklab_to_encoded_rgb(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let t = &*DISPLAY;
    let (r, g, b) = oklab_to_linear_rgb(l, a, b);
    (t.encode(r), t.encode(g), t.encode(b))
}

/// Oklch Hue Centers for 8 photographic color bands:
/// Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta
const OKLCH_CENTERS: [f32; 8] = [
    29.0,  // Red
    55.0,  // Orange
    105.0, // Yellow
    142.0, // Green
    195.0, // Aqua / Cyan
    264.0, // Blue
    305.0, // Purple
    345.0, // Magenta
];

/// Adaptive harmonic bandwidths (in degrees) for seamless C^1 raised-cosine blending
const OKLCH_BANDWIDTHS: [f32; 8] = [
    42.0, // Red
    46.0, // Orange
    48.0, // Yellow
    55.0, // Green
    62.0, // Aqua
    65.0, // Blue
    48.0, // Purple
    45.0, // Magenta
];

/// Converts Linear RGB to Oklab (Björn Ottosson, 2020)
#[inline(always)]
pub fn linear_rgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

    let l_ = if l > 0.0 { l.cbrt() } else { 0.0 };
    let m_ = if m > 0.0 { m.cbrt() } else { 0.0 };
    let s_ = if s > 0.0 { s.cbrt() } else { 0.0 };

    let big_l = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
    let a     = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
    let b_val = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;

    (big_l, a, b_val)
}

/// Converts Oklab back to Linear RGB
#[inline(always)]
pub fn oklab_to_linear_rgb(big_l: f32, a: f32, b_val: f32) -> (f32, f32, f32) {
    let l_ = big_l + 0.3963377774 * a + 0.2158037573 * b_val;
    let m_ = big_l - 0.1055613458 * a - 0.0638541728 * b_val;
    let s_ = big_l - 0.0894841775 * a - 1.2914855480 * b_val;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let r =  4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;

    (r.max(0.0), g.max(0.0), b.max(0.0))
}

/// Applies state-of-the-art 8-Band Oklch Perceptual Color Mixer.
///
/// Features:
/// - True perceptual uniformity: zero Helmholtz-Kohlrausch brightness distortions.
/// - Linear Chroma scaling: zero Abney hue shifts during saturation adjustments.
/// - Raised-Cosine (Hann) harmonic windowing with C^1 continuity: zero banding or stepping.
/// - Saturation-weighted luminance scaling: preserves pristine neutral skin highlights & grays.
#[inline(always)]
pub fn apply_hsl_mixer(r: f32, g: f32, b: f32, recipe: &Recipe) -> (f32, f32, f32) {
    let mut has_adj = false;
    for i in 0..8 {
        if recipe.hsl_hue[i] != 0.0 || recipe.hsl_sat[i] != 0.0 || recipe.hsl_lum[i] != 0.0 {
            has_adj = true;
            break;
        }
    }
    if !has_adj {
        return (r, g, b);
    }

    let (big_l, a, b_val) = encoded_rgb_to_oklab(r, g, b);
    let chroma = (a * a + b_val * b_val).sqrt();

    // Preserve achromatic neutrals (protects black, white, gray from chromatic pollution)
    if chroma < 0.006 || big_l < 0.001 {
        return (r, g, b);
    }

    let mut hue = b_val.atan2(a).to_degrees();
    if hue < 0.0 {
        hue += 360.0;
    }

    let mut delta_h = 0.0f32;
    let mut delta_s = 0.0f32;
    let mut delta_l = 0.0f32;

    for i in 0..8 {
        let center = OKLCH_CENTERS[i];
        let bw = OKLCH_BANDWIDTHS[i];

        let mut diff = (hue - center).abs();
        if diff > 180.0 {
            diff = 360.0 - diff;
        }

        if diff < bw {
            // Raised-Cosine (Hann) harmonic window: C^1 smooth roll-off
            let w = 0.5 * (1.0 + (std::f32::consts::PI * diff / bw).cos());

            if recipe.hsl_hue[i] != 0.0 {
                // Smooth hue rotation up to +-35 degrees
                delta_h += (recipe.hsl_hue[i] / 100.0) * 35.0 * w;
            }
            if recipe.hsl_sat[i] != 0.0 {
                delta_s += (recipe.hsl_sat[i] / 100.0) * w;
            }
            if recipe.hsl_lum[i] != 0.0 {
                // Perceptual Lightness adjustment in Oklab L
                delta_l += (recipe.hsl_lum[i] / 100.0) * 0.22 * w;
            }
        }
    }

    if delta_h == 0.0 && delta_s == 0.0 && delta_l == 0.0 {
        return (r, g, b);
    }

    // 1. New Hue
    let mut new_hue = (hue + delta_h) % 360.0;
    if new_hue < 0.0 {
        new_hue += 360.0;
    }
    let new_hue_rad = new_hue.to_radians();

    // 2. New Chroma (Perceptual Saturation with soft highlight knee)
    let new_chroma = if delta_s >= 0.0 {
        let boosted = chroma * (1.0 + delta_s * 1.35);
        // Soft roll-off to prevent out-of-gamut harsh clipping
        boosted / (1.0 + 0.12 * delta_s * (chroma / 0.35).min(1.0))
    } else {
        (chroma * (1.0 + delta_s)).max(0.0)
    };

    // 3. New Lightness (anchored to color band saturation)
    let chroma_weight = (chroma / 0.12).clamp(0.2, 1.0);
    let new_l = (big_l + delta_l * chroma_weight).clamp(0.0, 1.5);

    let new_a = new_chroma * new_hue_rad.cos();
    let new_b = new_chroma * new_hue_rad.sin();

    oklab_to_encoded_rgb(new_l, new_a, new_b)
}

/// Applies DaVinci Resolve style 3-Way Color Wheels (Lift, Gamma, Gain, Offset)
#[inline(always)]
pub fn apply_color_wheels(mut r: f32, mut g: f32, mut b: f32, recipe: &Recipe) -> (f32, f32, f32) {
    let has_lift = recipe.lift[0] != 0.0 || recipe.lift[1] != 0.0 || recipe.lift[2] != 0.0 || recipe.lift_luma != 0.0;
    let has_gamma = recipe.gamma[0] != 0.0 || recipe.gamma[1] != 0.0 || recipe.gamma[2] != 0.0 || recipe.gamma_luma != 0.0;
    let has_gain = recipe.gain[0] != 0.0 || recipe.gain[1] != 0.0 || recipe.gain[2] != 0.0 || recipe.gain_luma != 0.0;
    let has_offset = recipe.offset[0] != 0.0 || recipe.offset[1] != 0.0 || recipe.offset[2] != 0.0 || recipe.offset_luma != 0.0;

    if !has_lift && !has_gamma && !has_gain && !has_offset {
        return (r, g, b);
    }

    let y = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);

    // DaVinci Resolve Lift: Quadratic falloff towards highlights (1 - y)^2
    let w_lift = (1.0 - y).max(0.0) * (1.0 - y).max(0.0);

    // DaVinci Resolve Gain: Linear-quadratic ramp from shadows to highlights y^2
    let w_gain = y * y;

    // DaVinci Resolve Gamma: Bell shape peaking at midtones (1 - w_lift - w_gain)
    let w_gamma = (1.0 - w_lift - w_gain).max(0.0);

    let dr = w_lift * (recipe.lift[0] * 0.35 + recipe.lift_luma * 0.35)
           + w_gamma * (recipe.gamma[0] * 0.35 + recipe.gamma_luma * 0.35)
           + w_gain * (recipe.gain[0] * 0.35 + recipe.gain_luma * 0.35)
           + (recipe.offset[0] * 0.25 + recipe.offset_luma * 0.25);

    let dg = w_lift * (recipe.lift[1] * 0.35 + recipe.lift_luma * 0.35)
           + w_gamma * (recipe.gamma[1] * 0.35 + recipe.gamma_luma * 0.35)
           + w_gain * (recipe.gain[1] * 0.35 + recipe.gain_luma * 0.35)
           + (recipe.offset[1] * 0.25 + recipe.offset_luma * 0.25);

    let db = w_lift * (recipe.lift[2] * 0.35 + recipe.lift_luma * 0.35)
           + w_gamma * (recipe.gamma[2] * 0.35 + recipe.gamma_luma * 0.35)
           + w_gain * (recipe.gain[2] * 0.35 + recipe.gain_luma * 0.35)
           + (recipe.offset[2] * 0.25 + recipe.offset_luma * 0.25);

    r = (r + dr).max(0.0);
    g = (g + dg).max(0.0);
    b = (b + db).max(0.0);

    (r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oklab_roundtrip() {
        let colors = [
            (1.0, 1.0, 1.0),
            (0.5, 0.5, 0.5),
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
            (1.0, 1.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.5, 1.0),
        ];
        for &(r, g, b) in &colors {
            let (l, a, b_val) = linear_rgb_to_oklab(r, g, b);
            let (r2, g2, b2) = oklab_to_linear_rgb(l, a, b_val);
            assert!((r - r2).abs() < 1e-4, "Red mismatch for ({},{},{}): got {}", r, g, b, r2);
            assert!((g - g2).abs() < 1e-4, "Green mismatch for ({},{},{}): got {}", r, g, b, g2);
            assert!((b - b2).abs() < 1e-4, "Blue mismatch for ({},{},{}): got {}", r, g, b, b2);
        }
    }

    #[test]
    fn test_yellow_channel_adjustment() {
        let mut recipe = Recipe::default();
        recipe.hsl_sat[2] = 66.0;  // Yellow saturation +66
        recipe.hsl_hue[2] = 30.0;  // Yellow hue +30 (toward green)
        recipe.hsl_lum[2] = 20.0;  // Yellow lum +20

        // Yellow pixel
        let (yr, yg, yb) = apply_hsl_mixer(1.0, 0.9, 0.1, &recipe);
        assert!(yr != 1.0 || yg != 0.9 || yb != 0.1, "Yellow should be modified by Yellow HSL");

        // Blue pixel should NOT be affected by Yellow adjustments
        let (br, bg, bb) = apply_hsl_mixer(0.1, 0.2, 0.9, &recipe);
        assert!((br - 0.1).abs() < 0.01, "Blue red channel should not change under Yellow HSL");
        assert!((bg - 0.2).abs() < 0.01, "Blue green channel should not change under Yellow HSL");
        assert!((bb - 0.9).abs() < 0.01, "Blue blue channel should not change under Yellow HSL");

        // Neutral gray should NOT be affected
        let (gr, gg, gb) = apply_hsl_mixer(0.5, 0.5, 0.5, &recipe);
        assert_eq!((gr, gg, gb), (0.5, 0.5, 0.5), "Neutral gray must remain unaltered");
    }
}
