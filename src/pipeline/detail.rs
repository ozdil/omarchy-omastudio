#![allow(clippy::too_many_arguments)]
use rayon::prelude::*;

/// Applies radial vignetting effect
#[inline(always)]
pub fn apply_vignette(
    r: f32,
    g: f32,
    b: f32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    vignette_amount: f32,
) -> (f32, f32, f32) {
    if vignette_amount.abs() < 0.01 {
        return (r, g, b);
    }

    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let max_dist = (cx * cx + cy * cy).sqrt();

    let dx = x as f32 - cx;
    let dy = y as f32 - cy;
    let dist = (dx * dx + dy * dy).sqrt() / max_dist;

    // Smooth cosine falloff from 0.4 radius to outer edge
    let falloff = ((dist - 0.4) / 0.6).clamp(0.0, 1.0);
    let smooth = falloff * falloff * (3.0 - 2.0 * falloff);

    let factor = 1.0 + (vignette_amount / 100.0) * smooth;
    let clamped_factor = factor.max(0.0);

    (r * clamped_factor, g * clamped_factor, b * clamped_factor)
}

/// Reduces chromatic aberration fringing (purple and green edges)
#[inline(always)]
pub fn apply_defringe(r: f32, g: f32, b: f32, amount: f32) -> (f32, f32, f32) {
    if amount <= 0.01 {
        return (r, g, b);
    }
    let factor = (amount / 100.0).clamp(0.0, 1.0);
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

    // Detect purple fringing: R and B much higher than G
    let purple = (r.min(b) - g).max(0.0);
    // Detect green fringing: G much higher than (R + B) / 2
    let green = (g - (r + b) * 0.5).max(0.0);

    let fringe = (purple + green).clamp(0.0, 1.0) * factor * 0.6;
    if fringe > 0.001 {
        let nr = r * (1.0 - fringe) + luma * fringe;
        let ng = g * (1.0 - fringe) + luma * fringe;
        let nb = b * (1.0 - fringe) + luma * fringe;
        return (nr, ng, nb);
    }
    (r, g, b)
}

/// Applies high-precision multi-threaded Detail enhancements:
/// - **Sharpness**: High-frequency edge enhancement with noise-gate thresholding.
/// - **Luma Denoise**: Bilateral edge-preserving filter suppressing grain while locking sharp contours.
/// - **Chroma Denoise**: Low-frequency chrominance smoothing wiping out shadow color blotches without loss of sharpness.
pub fn apply_detail(
    rgb_buffer: &mut [f32],
    width: usize,
    height: usize,
    sharpness: f32,
    denoise_lum: f32,
    denoise_col: f32,
) {
    if sharpness <= 0.1 && denoise_lum <= 0.1 && denoise_col <= 0.1 {
        return;
    }

    let num_pixels = width * height;
    if rgb_buffer.len() < num_pixels * 3 || width < 3 || height < 3 {
        return;
    }

    // Extract Luminance and Chrominance (Cr, Cb) channels
    let mut luma = vec![0.0f32; num_pixels];
    let mut cr = vec![0.0f32; num_pixels];
    let mut cb = vec![0.0f32; num_pixels];

    luma.par_iter_mut()
        .zip(cr.par_iter_mut())
        .zip(cb.par_iter_mut())
        .enumerate()
        .for_each(|(i, ((l, r_diff), b_diff))| {
            let px = i * 3;
            let y = 0.2126 * rgb_buffer[px] + 0.7152 * rgb_buffer[px + 1] + 0.0722 * rgb_buffer[px + 2];
            *l = y;
            *r_diff = rgb_buffer[px] - y;
            *b_diff = rgb_buffer[px + 2] - y;
        });

    let do_chroma_dn = denoise_col > 0.5;
    let do_luma_dn = denoise_lum > 0.5;
    let do_sharp = sharpness > 0.5;

    // 1. Chroma Denoise: Separable multi-pixel color average (eliminates color blotches with zero loss of sharpness)
    let (cr_smooth, cb_smooth) = if do_chroma_dn {
        let weight = (denoise_col / 100.0).clamp(0.0, 1.0);
        let radius = if denoise_col > 40.0 { 2 } else { 1 };
        let blur_cr = crate::pipeline::presence::fast_separable_blur(&cr, width, height, radius);
        let blur_cb = crate::pipeline::presence::fast_separable_blur(&cb, width, height, radius);

        let mut out_cr = vec![0.0f32; num_pixels];
        let mut out_cb = vec![0.0f32; num_pixels];

        out_cr.par_iter_mut().zip(out_cb.par_iter_mut()).enumerate().for_each(|(i, (dst_r, dst_b))| {
            *dst_r = cr[i] * (1.0 - weight) + blur_cr[i] * weight;
            *dst_b = cb[i] * (1.0 - weight) + blur_cb[i] * weight;
        });

        (Some(out_cr), Some(out_cb))
    } else {
        (None, None)
    };

    // 2. Luma Denoise & Sharpness on Luminance Channel
    let mut luma_out = vec![0.0f32; num_pixels];
    let dn_strength = if do_luma_dn { (denoise_lum / 100.0) * 0.75 } else { 0.0 };
    let sh_strength = if do_sharp { (sharpness / 100.0) * 0.80 } else { 0.0 };

    luma_out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let y_prev = if y > 0 { y - 1 } else { 0 };
        let y_next = if y + 1 < height { y + 1 } else { height - 1 };

        for x in 0..width {
            let x_prev = if x > 0 { x - 1 } else { 0 };
            let x_next = if x + 1 < width { x + 1 } else { width - 1 };

            let center = luma[y * width + x];
            let n_up = luma[y_prev * width + x];
            let n_dn = luma[y_next * width + x];
            let n_lt = luma[y * width + x_prev];
            let n_rt = luma[y * width + x_next];

            let mut final_l = center;

            // Bilateral-style noise reduction on flat areas
            if do_luma_dn {
                let sig = 0.06f32;
                let inv_sig = 1.0 / (2.0 * sig * sig);

                let w_up = (-((n_up - center) * (n_up - center)) * inv_sig).exp();
                let w_dn = (-((n_dn - center) * (n_dn - center)) * inv_sig).exp();
                let w_lt = (-((n_lt - center) * (n_lt - center)) * inv_sig).exp();
                let w_rt = (-((n_rt - center) * (n_rt - center)) * inv_sig).exp();
                let w_c = 1.0f32;

                let w_sum = w_up + w_dn + w_lt + w_rt + w_c;
                let blurred = (n_up * w_up + n_dn * w_dn + n_lt * w_lt + n_rt * w_rt + center * w_c) / w_sum;
                final_l = center * (1.0 - dn_strength) + blurred * dn_strength;
            }

            // Adaptive unsharp masking (boosts in-focus edges with noise gate)
            if do_sharp {
                let laplacian = 4.0 * center - (n_up + n_dn + n_lt + n_rt);
                let edge_mag = laplacian.abs();

                // Noise-gate threshold: do not sharpen tiny noise fluctuations (< 0.012)
                let noise_gate = ((edge_mag - 0.012) / 0.03).clamp(0.0, 1.0);
                let boost = laplacian * sh_strength * noise_gate;
                final_l = (final_l + boost).clamp(0.0, 1.5);
            }

            row[x] = final_l;
        }
    });

    // Reconstruct RGB with filtered luminance and chrominance
    rgb_buffer.par_chunks_mut(3).enumerate().for_each(|(i, rgb)| {
        let l = luma_out[i];
        let r_diff = if let Some(ref c) = cr_smooth { c[i] } else { cr[i] };
        let b_diff = if let Some(ref c) = cb_smooth { c[i] } else { cb[i] };

        // Since L = 0.2126*R + 0.7152*G + 0.0722*B
        // G = (L - 0.2126*(L + r_diff) - 0.0722*(L + b_diff)) / 0.7152
        let r = l + r_diff;
        let b = l + b_diff;
        let g = (l - 0.2126 * r - 0.0722 * b) / 0.7152;

        rgb[0] = r;
        rgb[1] = g;
        rgb[2] = b;
    });
}

/// Radial polynomial lens distortion: r_source = r * (1 + k1 * (r / max_r)^2).
#[derive(Clone, Copy)]
pub struct LensDistortion {
    cx: f32,
    cy: f32,
    inv_max_r2: f32,
    k1: f32,
}

impl LensDistortion {
    /// None when `distortion` (-100..100) is too small to have an effect.
    pub fn new(width: usize, height: usize, distortion: f32) -> Option<Self> {
        if distortion.abs() < 0.1 || width == 0 || height == 0 {
            return None;
        }
        let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
        Some(Self { cx, cy, inv_max_r2: 1.0 / (cx * cx + cy * cy), k1: (distortion / 100.0) * 0.15 })
    }

    /// Where the output pixel (x, y) samples the source image.
    #[inline(always)]
    pub fn source(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let factor = 1.0 + self.k1 * (dx * dx + dy * dy) * self.inv_max_r2;
        (self.cx + dx * factor, self.cy + dy * factor)
    }
}

/// Corrects optical barrel / pincushion lens distortion via radial polynomial mapping
pub fn apply_lens_distortion(
    input: &[f32],
    width: usize,
    height: usize,
    channels: usize,
    distortion: f32,
) -> Vec<f32> {
    let Some(lens) = LensDistortion::new(width, height, distortion) else {
        return input.to_vec();
    };

    let mut output = vec![0.0f32; input.len()];
    output
        .par_chunks_mut(width * channels)
        .enumerate()
        .for_each(|(y_idx, row)| {
            for x_idx in 0..width {
                let (src_x, src_y) = lens.source(x_idx as f32, y_idx as f32);
                let px = x_idx * channels;

                if src_x >= 0.0 && src_x < (width - 1) as f32 && src_y >= 0.0 && src_y < (height - 1) as f32 {
                    let x0 = src_x.floor() as usize;
                    let y0 = src_y.floor() as usize;
                    let x1 = (x0 + 1).min(width - 1);
                    let y1 = (y0 + 1).min(height - 1);

                    let fx = src_x - (x0 as f32);
                    let fy = src_y - (y0 as f32);

                    let idx00 = (y0 * width + x0) * channels;
                    let idx10 = (y0 * width + x1) * channels;
                    let idx01 = (y1 * width + x0) * channels;
                    let idx11 = (y1 * width + x1) * channels;

                    for c in 0..channels.min(3) {
                        let v00 = input[idx00 + c];
                        let v10 = input[idx10 + c];
                        let v01 = input[idx01 + c];
                        let v11 = input[idx11 + c];

                        let top = v00 * (1.0 - fx) + v10 * fx;
                        let bottom = v01 * (1.0 - fx) + v11 * fx;
                        row[px + c] = top * (1.0 - fy) + bottom * fy;
                    }
                    if channels == 4 {
                        row[px + 3] = input[idx00 + 3];
                    }
                } else {
                    for c in 0..channels {
                        row[px + c] = 0.0;
                    }
                }
            }
        });

    output
}
