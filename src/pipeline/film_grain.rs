//! Photochemical Silver-Halide Film Grain Engine
//!
//! Simulates realistic analog film grain:
//! - Luminance-dependent grain distribution (strongest in Zone V-VI midtones, decaying in deep shadows and highlights).
//! - Adjustable grain size (coarseness) and roughness (organic clumping).
//! - High-speed deterministic PRNG per-pixel to ensure reproducibility without memory allocations.

use rayon::prelude::*;

/// Fast deterministic hash to generate pseudo-random float in [-1.0, 1.0] from pixel coordinate and seed
#[inline(always)]
fn hash_noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut n = (x.wrapping_mul(374761393))
        ^ (y.wrapping_mul(668265263))
        ^ (seed.wrapping_mul(314159265));
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    let f = (n & 0x007FFFFF) as f32 / 8388607.0; // 0.0 to 1.0
    f * 2.0 - 1.0
}

/// Applies photochemical film grain to an f32 RGB image buffer in-place
pub fn apply_film_grain(
    buffer: &mut [f32],
    width: u32,
    _height: u32,
    amount: f32,    // 0.0 to 100.0 (Default: 0.0)
    size: f32,      // 1.0 to 3.0 (Default: 1.0)
    roughness: f32, // 0.0 to 100.0 (Default: 50.0)
) {
    if amount <= 0.001 {
        return;
    }

    let w = width as usize;
    let norm_amount = (amount / 100.0) * 0.12; // Scaled to photographic realistic contrast
    let step = size.max(1.0);
    let rough_weight = roughness / 100.0;

    buffer
        .par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y_idx, row)| {
            let y = y_idx as u32;
            let sample_y = (y as f32 / step) as u32;

            for x in 0..w {
                let px = x * 3;
                let sample_x = (x as f32 / step) as u32;

                let r = row[px];
                let g = row[px + 1];
                let b = row[px + 2];

                // Relative Photometric Luminance (Rec.709)
                let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);

                // Physical Silver-Halide grain bell curve:
                // Peak grain visibility around 18% gray (Zone V ~ 0.40 - 0.50 linear),
                // rapidly tapering off at pure black (0.0) and specular highlights (1.0).
                let luma_response = (4.0 * luma * (1.0 - luma)).powf(1.15).max(0.0);

                // Multi-scale noise: Base grain + Fine particulate roughness
                let n1 = hash_noise(sample_x, sample_y, 42);
                let n2 = hash_noise(x as u32, y, 1337);
                let combined_noise = n1 * (1.0 - rough_weight * 0.4) + n2 * (rough_weight * 0.4);

                let delta = combined_noise * norm_amount * luma_response;

                // Additive grain in linear space
                row[px] = (r + delta).max(0.0);
                row[px + 1] = (g + delta).max(0.0);
                row[px + 2] = (b + delta).max(0.0);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_film_grain_zero_amount() {
        let mut buf = vec![0.5f32; 100 * 3];
        let copy = buf.clone();
        apply_film_grain(&mut buf, 10, 10, 0.0, 1.0, 50.0);
        assert_eq!(buf, copy);
    }

    #[test]
    fn test_film_grain_preserves_pure_black_and_white() {
        let mut buf = vec![
            0.0, 0.0, 0.0, // Pure black
            1.0, 1.0, 1.0, // Pure white
            0.5, 0.5, 0.5, // Mid gray
        ];
        apply_film_grain(&mut buf, 3, 1, 50.0, 1.0, 50.0);

        // Black (luma=0) has zero luma_response -> unchanged
        assert_eq!(buf[0], 0.0);
        assert_eq!(buf[1], 0.0);
        assert_eq!(buf[2], 0.0);

        // White (luma=1) has zero luma_response -> unchanged
        assert_eq!(buf[3], 1.0);
        assert_eq!(buf[4], 1.0);
        assert_eq!(buf[5], 1.0);

        // Mid gray should have received grain
        assert!((buf[6] - 0.5).abs() > 0.0001);
    }
}
