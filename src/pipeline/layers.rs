use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MaskShape {
    LinearGradient {
        start_x: f32, // 0.0 to 1.0 (normalized image coords)
        start_y: f32,
        end_x: f32,
        end_y: f32,
    },
    RadialGradient {
        center_x: f32, // 0.0 to 1.0
        center_y: f32,
        radius_x: f32, // 0.0 to 1.0
        radius_y: f32,
        feather: f32,  // 0.0 to 1.0 (Default: 0.5)
    },
    LumaRange {
        min_luma: f32, // 0.0 to 1.0
        max_luma: f32, // 0.0 to 1.0
        falloff: f32,  // 0.0 to 0.5
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdjustmentLayer {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub invert: bool,
    pub opacity: f32, // 0.0 to 1.0
    pub shape: MaskShape,

    // Targeted Adjustments applied through this mask
    pub exposure: f32,   // -5.0 to +5.0 EV
    pub contrast: f32,   // -100.0 to +100.0
    pub highlights: f32, // -100.0 to +100.0
    pub shadows: f32,    // -100.0 to +100.0
    pub whites: f32,     // -100.0 to +100.0
    pub blacks: f32,     // -100.0 to +100.0
    pub clarity: f32,    // -100.0 to +100.0
    pub saturation: f32, // -100.0 to +100.0
    pub tint_r: f32,     // -1.0 to 1.0
    pub tint_g: f32,     // -1.0 to 1.0
    pub tint_b: f32,     // -1.0 to 1.0
}

impl Default for AdjustmentLayer {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "New Layer".to_string(),
            enabled: true,
            invert: false,
            opacity: 1.0,
            shape: MaskShape::LinearGradient {
                start_x: 0.5,
                start_y: 0.0,
                end_x: 0.5,
                end_y: 0.5,
            },
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            clarity: 0.0,
            saturation: 0.0,
            tint_r: 0.0,
            tint_g: 0.0,
            tint_b: 0.0,
        }
    }
}

/// Evaluates mask weight (0.0 to 1.0) for a given pixel at normalized coordinates (u, v) and RGB luminance
#[inline]
pub fn sample_mask_weight(
    shape: &MaskShape,
    invert: bool,
    opacity: f32,
    u: f32,
    v: f32,
    luma: f32,
) -> f32 {
    let raw_weight = match shape {
        MaskShape::LinearGradient { start_x, start_y, end_x, end_y } => {
            let dx = end_x - start_x;
            let dy = end_y - start_y;
            let len_sq = dx * dx + dy * dy;
            if len_sq <= 1e-6 {
                1.0
            } else {
                let px = u - start_x;
                let py = v - start_y;
                let proj = (px * dx + py * dy) / len_sq;
                // Raised-cosine / smoothstep transition along the vector
                let clamped = proj.clamp(0.0, 1.0);
                clamped * clamped * (3.0 - 2.0 * clamped)
            }
        }
        MaskShape::RadialGradient { center_x, center_y, radius_x, radius_y, feather } => {
            let rx = radius_x.max(1e-4);
            let ry = radius_y.max(1e-4);
            let f = feather.clamp(0.01, 1.0);

            let nx = (u - center_x) / rx;
            let ny = (v - center_y) / ry;
            let dist = (nx * nx + ny * ny).sqrt();

            if dist >= 1.0 {
                0.0
            } else if dist <= (1.0 - f) {
                1.0
            } else {
                // Smooth falloff in the feathered margin
                let t = (1.0 - dist) / f;
                t * t * (3.0 - 2.0 * t)
            }
        }
        MaskShape::LumaRange { min_luma, max_luma, falloff } => {
            let f = falloff.clamp(0.001, 0.5);
            let min_in = (min_luma - f).max(0.0);
            let max_in = (max_luma + f).min(1.0);

            if luma < min_in || luma > max_in {
                0.0
            } else if luma >= *min_luma && luma <= *max_luma {
                1.0
            } else if luma < *min_luma {
                let t = (luma - min_in) / f;
                t * t * (3.0 - 2.0 * t)
            } else {
                let t = (max_in - luma) / f;
                t * t * (3.0 - 2.0 * t)
            }
        }
    };

    let w = if invert { 1.0 - raw_weight } else { raw_weight };
    (w * opacity).clamp(0.0, 1.0)
}

/// Applies active adjustment layers to an RGB pixel in linear 0.0..1.0 space
#[inline]
pub fn apply_layers_pixel(
    mut r: f32,
    mut g: f32,
    mut b: f32,
    layers: &[AdjustmentLayer],
    u: f32,
    v: f32,
) -> (f32, f32, f32) {
    if layers.is_empty() {
        return (r, g, b);
    }

    for layer in layers {
        if !layer.enabled || layer.opacity <= 1e-4 {
            continue;
        }

        // Rec.709 luma for range sampling
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let weight = sample_mask_weight(&layer.shape, layer.invert, layer.opacity, u, v, luma);
        if weight <= 1e-4 {
            continue;
        }

        // Layer exposure
        let exp_mul = 2.0f32.powf(layer.exposure * weight);
        let mut lr = r * exp_mul;
        let mut lg = g * exp_mul;
        let mut lb = b * exp_mul;

        // Layer contrast
        if layer.contrast.abs() > 1e-3 {
            let c_factor = 1.0 + (layer.contrast / 100.0) * weight;
            lr = 0.18 + (lr - 0.18) * c_factor;
            lg = 0.18 + (lg - 0.18) * c_factor;
            lb = 0.18 + (lb - 0.18) * c_factor;
        }

        // Layer saturation
        if layer.saturation.abs() > 1e-3 {
            let sat_mul = 1.0 + (layer.saturation / 100.0) * weight;
            let l = 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
            lr = l + (lr - l) * sat_mul;
            lg = l + (lg - l) * sat_mul;
            lb = l + (lb - l) * sat_mul;
        }

        // Layer tint
        if layer.tint_r.abs() > 1e-4 || layer.tint_g.abs() > 1e-4 || layer.tint_b.abs() > 1e-4 {
            lr += layer.tint_r * weight * 0.2;
            lg += layer.tint_g * weight * 0.2;
            lb += layer.tint_b * weight * 0.2;
        }

        r = lr.max(0.0);
        g = lg.max(0.0);
        b = lb.max(0.0);
    }

    (r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_gradient_interpolation() {
        let shape = MaskShape::LinearGradient {
            start_x: 0.0,
            start_y: 0.0,
            end_x: 1.0,
            end_y: 0.0,
        };

        let w_start = sample_mask_weight(&shape, false, 1.0, 0.0, 0.5, 0.5);
        let w_mid = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.5);
        let w_end = sample_mask_weight(&shape, false, 1.0, 1.0, 0.5, 0.5);

        assert!((w_start - 0.0).abs() < 1e-3);
        assert!((w_mid - 0.5).abs() < 1e-2);
        assert!((w_end - 1.0).abs() < 1e-3);
    }

    #[test]
    fn test_radial_gradient_bounds() {
        let shape = MaskShape::RadialGradient {
            center_x: 0.5,
            center_y: 0.5,
            radius_x: 0.2,
            radius_y: 0.2,
            feather: 0.5,
        };

        let w_center = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.5);
        let w_outside = sample_mask_weight(&shape, false, 1.0, 0.9, 0.9, 0.5);

        assert!((w_center - 1.0).abs() < 1e-3);
        assert_eq!(w_outside, 0.0);
    }

    #[test]
    fn test_luma_range_mask() {
        let shape = MaskShape::LumaRange {
            min_luma: 0.6,
            max_luma: 1.0,
            falloff: 0.1,
        };

        let w_shadow = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.2);
        let w_highlight = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.8);

        assert_eq!(w_shadow, 0.0);
        assert!((w_highlight - 1.0).abs() < 1e-3);
    }
}
