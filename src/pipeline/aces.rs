//! ACES 1.3 Color Management and Chromatic Adaptation Engine
//!
//! Provides mathematically rigorous scene-referred color transformations:
//! - ACEScg (AP1, D60 linear) and ACEScc (AP1, D60 logarithmic)
//! - Bradford-adapted 3x3 matrices for sRGB (D65), Display P3 (D65), and Rec.2020 (D65)
//! - ACES 1.3 Reference Gamut Compression (RGC) to prevent clipping artifacts
//! - A fitted approximation of the ACES filmic tone scale

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WorkingColorSpace {
    #[default]
    Srgb,
    DisplayP3,
    Rec2020,
    AcesCg,
}

impl WorkingColorSpace {
    pub fn from_str_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "displayp3" | "display_p3" | "p3" => Self::DisplayP3,
            "rec2020" | "bt2020" | "bt.2020" => Self::Rec2020,
            "acescg" | "aces" | "ap1" => Self::AcesCg,
            _ => Self::Srgb,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "DisplayP3",
            Self::Rec2020 => "Rec2020",
            Self::AcesCg => "ACEScg",
        }
    }
}

// ============================================================================
// ACEScg (AP1 Linear) <-> ACEScc (AP1 Logarithmic)
// ============================================================================

/// ACEScg (Linear AP1) -> ACEScc (Logarithmic AP1, quasi-uniform perceptual grading space)
#[inline(always)]
pub fn acescg_to_acescc_pixel(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let to_cc = |v: f32| -> f32 {
        if v <= 0.000030517578125 {
            // Smooth continuous linear extension for deep shadows below 2^-15
            ((v * 0.5 + 0.0000152587890625).max(1e-12).log2() + 9.72) / 17.52
        } else {
            (v.log2() + 9.72) / 17.52
        }
    };
    (to_cc(r), to_cc(g), to_cc(b))
}

/// ACEScc (Logarithmic AP1) -> ACEScg (Linear AP1)
#[inline(always)]
pub fn acescc_to_acescg_pixel(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let to_cg = |v: f32| -> f32 {
        if v <= -0.301369863 {
            (2.0f32.powf(v * 17.52 - 9.72) - 0.0000152587890625) * 2.0
        } else {
            2.0f32.powf(v * 17.52 - 9.72)
        }
    };
    (to_cg(r), to_cg(g), to_cg(b))
}

// ============================================================================
// 3x3 Color Transformation Matrices (with Bradford Chromatic Adaptation)
// ============================================================================

#[inline(always)]
pub fn apply_matrix3x3(r: f32, g: f32, b: f32, m: &[f32; 9]) -> (f32, f32, f32) {
    (
        m[0] * r + m[1] * g + m[2] * b,
        m[3] * r + m[4] * g + m[5] * b,
        m[6] * r + m[7] * g + m[8] * b,
    )
}

// Linear sRGB (D65) <-> ACEScg (AP1, D60)
pub const MAT_SRGB_TO_ACESCG: [f32; 9] = [
    0.613097, 0.339523, 0.047379,
    0.070194, 0.916354, 0.013452,
    0.020616, 0.109570, 0.913844,
];

pub const MAT_ACESCG_TO_SRGB: [f32; 9] = [
    1.704969, -0.622242, -0.079236,
   -0.130268,  1.140748, -0.010038,
   -0.022844, -0.122738,  1.097270,
];

// Linear Rec.2020 (D65) <-> ACEScg (AP1, D60)
pub const MAT_REC2020_TO_ACESCG: [f32; 9] = [
    0.695452, 0.141549, 0.163000,
    0.045124, 0.859665, 0.095211,
   -0.000553, 0.005187, 1.000366,
];

pub const MAT_ACESCG_TO_REC2020: [f32; 9] = [
    1.453185, -0.237984, -0.214132,
   -0.076411,  1.176426, -0.099517,
    0.001200, -0.006231,  1.000032,
];

// Linear Display P3 (D65) <-> ACEScg (AP1, D60)
pub const MAT_DISPLAY_P3_TO_ACESCG: [f32; 9] = [
    0.695287, 0.244302, 0.060411,
    0.060647, 0.932788, 0.006565,
    0.018090, 0.084180, 0.897730,
];

pub const MAT_ACESCG_TO_DISPLAY_P3: [f32; 9] = [
    1.473670, -0.377261, -0.096409,
   -0.095668,  1.097254, -0.001586,
   -0.020725, -0.095287,  1.116012,
];

/// Converts linear RGB from specified space to ACEScg
#[inline(always)]
pub fn linear_to_acescg(r: f32, g: f32, b: f32, space: WorkingColorSpace) -> (f32, f32, f32) {
    match space {
        WorkingColorSpace::Srgb => apply_matrix3x3(r, g, b, &MAT_SRGB_TO_ACESCG),
        WorkingColorSpace::DisplayP3 => apply_matrix3x3(r, g, b, &MAT_DISPLAY_P3_TO_ACESCG),
        WorkingColorSpace::Rec2020 => apply_matrix3x3(r, g, b, &MAT_REC2020_TO_ACESCG),
        WorkingColorSpace::AcesCg => (r, g, b),
    }
}

/// Converts linear ACEScg to target display color space
#[inline(always)]
pub fn acesccg_to_linear(r: f32, g: f32, b: f32, space: WorkingColorSpace) -> (f32, f32, f32) {
    match space {
        WorkingColorSpace::Srgb => apply_matrix3x3(r, g, b, &MAT_ACESCG_TO_SRGB),
        WorkingColorSpace::DisplayP3 => apply_matrix3x3(r, g, b, &MAT_ACESCG_TO_DISPLAY_P3),
        WorkingColorSpace::Rec2020 => apply_matrix3x3(r, g, b, &MAT_ACESCG_TO_REC2020),
        WorkingColorSpace::AcesCg => (r, g, b),
    }
}

// ============================================================================
// ACES 1.3 Reference Gamut Compression (RGC)
// ============================================================================

/// Per-channel distance thresholds (cyan, magenta, yellow): distances below these
/// are left untouched.
const RGC_THRESHOLD: [f32; 3] = [0.815, 0.803, 0.880];
/// Distances that are mapped onto the gamut boundary (distance 1.0).
const RGC_LIMIT: [f32; 3] = [1.147, 1.264, 1.312];
const RGC_POWER: f32 = 1.2;

/// Compresses one distance with the RGC power curve: identity below the threshold,
/// `limit` lands exactly on 1.0, larger distances approach `threshold + scale`.
#[inline(always)]
fn rgc_compress(dist: f32, thr: f32, lim: f32) -> f32 {
    if dist < thr {
        return dist;
    }
    let scale = (lim - thr) / (((1.0 - thr) / (lim - thr)).powf(-RGC_POWER) - 1.0).powf(1.0 / RGC_POWER);
    let nd = (dist - thr) / scale;
    thr + scale * nd / (1.0 + nd.powf(RGC_POWER)).powf(1.0 / RGC_POWER)
}

/// ACES 1.3 Reference Gamut Compression on linear RGB.
///
/// Each channel's distance from the achromatic axis (the largest channel) is
/// compressed, so colours outside the RGB gamut (negative components) are brought
/// back inside smoothly instead of being clipped. Colours well inside the gamut are
/// unchanged; only the most saturated ones near the boundary move slightly.
#[inline(always)]
pub fn apply_aces_gamut_compression(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let ach = r.max(g).max(b);
    if !ach.is_finite() || ach <= 0.0 {
        return (r, g, b);
    }
    let c = [r, g, b];
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let d = (ach - c[i]) / ach;
        out[i] = ach - rgc_compress(d, RGC_THRESHOLD[i], RGC_LIMIT[i]) * ach;
    }
    (out[0], out[1], out[2])
}

/// Gamut compression for display-encoded values (the creative stage of the
/// pipeline). Distances are taken on the encoded values, which spreads the
/// protected region evenly in lightness: fully saturated colours keep most of
/// their saturation, and colours pushed outside the gamut by grading come back
/// smoothly instead of clipping.
#[inline(always)]
pub fn apply_gamut_compression_encoded(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max <= 0.0 || (max - min) / max < RGC_THRESHOLD[1] {
        return (r, g, b);
    }
    apply_aces_gamut_compression(r, g, b)
}

// ============================================================================
// ACES filmic tone curve (fitted approximation)
// ============================================================================

/// Krzysztof Narkowicz's rational fit of the ACES RRT + sRGB ODT tone scale
/// (luminance only, no RRT colour adjustments). Input is scene-linear, output is
/// display-linear in [0, 1]. The 0.6 pre-scale is part of the fit and puts scene
/// 1.0 near display 0.8 as in the reference transform.
#[inline(always)]
pub fn apply_aces_tonemap(val: f32) -> f32 {
    let x = val.max(0.0) * 0.6;
    let a = 2.51f32;
    let b = 0.03f32;
    let c = 2.43f32;
    let d = 0.59f32;
    let e = 0.14f32;
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

/// Applies the ACES tone curve to a display-encoded value: decode (2.2 power),
/// tone map in linear light, encode again.
#[inline(always)]
pub fn apply_aces_tonemap_encoded(val: f32) -> f32 {
    let lin = val.max(0.0).powf(2.2);
    apply_aces_tonemap(lin).powf(1.0 / 2.2)
}

/// Applies the ACES tone curve to each channel of a linear RGB pixel.
#[inline(always)]
pub fn apply_aces_tonemap_pixel(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    (
        apply_aces_tonemap(r),
        apply_aces_tonemap(g),
        apply_aces_tonemap(b),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acescg_acescc_roundtrip() {
        let test_vals = [0.001f32, 0.01, 0.18, 0.5, 1.0, 5.0, 10.0];
        for &v in &test_vals {
            let (cr, cg, cb) = acescg_to_acescc_pixel(v, v, v);
            let (r, g, b) = acescc_to_acescg_pixel(cr, cg, cb);
            assert!((r - v).abs() < 1e-3, "Mismatch for {}: got {}", v, r);
            assert!((g - v).abs() < 1e-3, "Mismatch for {}: got {}", v, g);
            assert!((b - v).abs() < 1e-3, "Mismatch for {}: got {}", v, b);
        }
    }

    #[test]
    fn test_srgb_acescg_roundtrip() {
        let (r, g, b) = (0.75f32, 0.45f32, 0.20f32);
        let (ar, ag, ab) = linear_to_acescg(r, g, b, WorkingColorSpace::Srgb);
        let (r2, g2, b2) = acesccg_to_linear(ar, ag, ab, WorkingColorSpace::Srgb);
        assert!((r - r2).abs() < 1e-4, "Red mismatch: {} vs {}", r, r2);
        assert!((g - g2).abs() < 1e-4, "Green mismatch: {} vs {}", g, g2);
        assert!((b - b2).abs() < 1e-4, "Blue mismatch: {} vs {}", b, b2);
    }

    #[test]
    fn test_aces_tonemap_limits() {
        assert_eq!(apply_aces_tonemap(0.0), 0.0);
        let mid = apply_aces_tonemap(0.18);
        assert!(mid > 0.10 && mid < 0.20, "Scene 0.18 maps near display 0.14: got {}", mid);
        let white = apply_aces_tonemap(1.0);
        assert!(white > 0.60 && white < 0.85, "Scene 1.0 sits on the shoulder: got {}", white);
        let super_white = apply_aces_tonemap(100.0);
        assert!((super_white - 1.0).abs() < 0.03, "Super-white approaches 1.0: got {}", super_white);
    }

    #[test]
    fn gamut_compression_leaves_inner_colours_alone() {
        for &(r, g, b) in &[(0.18f32, 0.18, 0.18), (0.8, 0.5, 0.3), (0.2, 0.6, 0.9), (2.0, 1.5, 1.0)] {
            let (r2, g2, b2) = apply_aces_gamut_compression(r, g, b);
            assert!((r - r2).abs() < 1e-6 && (g - g2).abs() < 1e-6 && (b - b2).abs() < 1e-6);
        }
    }

    #[test]
    fn gamut_compression_brings_negatives_inside() {
        // Each channel at its limit distance lands exactly on the gamut boundary.
        let (r, g, b) = apply_aces_gamut_compression(1.0 - RGC_LIMIT[0], 1.0, 1.0 - RGC_LIMIT[2]);
        assert!(r.abs() < 1e-4 && b.abs() < 1e-4 && (g - 1.0).abs() < 1e-6, "{r} {g} {b}");
        // Mildly out-of-gamut colours come back inside, hue order kept.
        let (r, g, b) = apply_aces_gamut_compression(1.0, -0.08, 0.2);
        assert!(g >= 0.0 && b > g && r == 1.0, "{r} {g} {b}");
        // Monotonic in distance.
        let mut prev = f32::MAX;
        for i in 0..200 {
            let c = 1.0 - i as f32 * 0.01;
            let (_, out, _) = apply_aces_gamut_compression(1.0, c, 1.0);
            assert!(out <= prev + 1e-6);
            prev = out;
        }
    }

    #[test]
    fn encoded_helpers_round_trip_neutral() {
        let (r, g, b) = apply_gamut_compression_encoded(0.5, 0.5, 0.5);
        assert_eq!((r, g, b), (0.5, 0.5, 0.5));
        // Out of gamut comes back inside; an in-gamut channel far from the
        // boundary stays where it was.
        let (r, g, b) = apply_gamut_compression_encoded(0.9, -0.05, 0.5);
        assert!(g >= 0.0 && r == 0.9 && b == 0.5, "{r} {g} {b}");
        // A pure primary keeps most of its saturation.
        let (_, g, b) = apply_gamut_compression_encoded(1.0, 0.0, 0.0);
        assert!(g > 0.0 && g < 0.1 && b > 0.0 && b < 0.15, "{g} {b}");
        let mid = apply_aces_tonemap_encoded(0.18f32.powf(1.0 / 2.2));
        assert!(mid > 0.35 && mid < 0.50, "{mid}");
    }
}
