use rayon::prelude::*;

/// Fast O(1) separable horizontal and vertical box blur on single-channel f32 buffers.
pub fn fast_separable_blur(
    input: &[f32],
    width: usize,
    height: usize,
    radius: usize,
) -> Vec<f32> {
    if radius == 0 || width == 0 || height == 0 || input.len() < width * height {
        return input.to_vec();
    }

    let r = radius.min(width / 2).min(height / 2).max(1);
    let mut temp = vec![0.0f32; width * height];
    let mut output = vec![0.0f32; width * height];
    let win_size = (2 * r + 1) as f32;
    let inv_win = 1.0 / win_size;

    // 1. Horizontal Pass (Parallelized over rows with Rayon)
    temp.par_chunks_exact_mut(width)
        .enumerate()
        .for_each(|(y, out_row)| {
            let in_row = &input[y * width..(y + 1) * width];
            let mut sum = in_row[0] * (r as f32);
            for x in 0..=r {
                sum += in_row[x.min(width - 1)];
            }

            for (x, dst) in out_row.iter_mut().enumerate() {
                let left = x.saturating_sub(r);
                let right = (x + r + 1).min(width - 1);
                *dst = sum * inv_win;
                sum += in_row[right] - in_row[left];
            }
        });

    // 2. Vertical Pass (Parallelized over disjoint row bands for safe, contiguous SIMD writes)
    let num_threads = rayon::current_num_threads().max(1);
    let band_rows = height.div_ceil(num_threads).max(16);
    let temp_slice = temp.as_slice();
    let max_y = (height - 1) as isize;

    output
        .par_chunks_mut(band_rows * width)
        .enumerate()
        .for_each(|(band_idx, band_out)| {
            let y_start = band_idx * band_rows;
            let mut col_sums = vec![0.0f32; width];

            // Initialize the vertical window centered at y_start with clamped borders.
            for d in -(r as isize)..=(r as isize) {
                let ry = (y_start as isize + d).clamp(0, max_y) as usize;
                let src_row = &temp_slice[ry * width..(ry + 1) * width];
                for (acc, &val) in col_sums.iter_mut().zip(src_row.iter()) {
                    *acc += val;
                }
            }

            // Rolling sum down the rows owned by this band.
            for (local_y, out_row) in band_out.chunks_exact_mut(width).enumerate() {
                let y = y_start + local_y;
                let top_y = y.saturating_sub(r);
                let bot_y = (y + r + 1).min(height - 1);
                let top_row = &temp_slice[top_y * width..(top_y + 1) * width];
                let bot_row = &temp_slice[bot_y * width..(bot_y + 1) * width];

                for (((dst, sum), &bot_val), &top_val) in out_row
                    .iter_mut()
                    .zip(col_sums.iter_mut())
                    .zip(bot_row.iter())
                    .zip(top_row.iter())
                {
                    *dst = *sum * inv_win;
                    *sum += bot_val - top_val;
                }
            }
        });

    output
}

/// Applies Presence adjustments: Texture, Clarity, Dehaze, and DaVinci Midtone Detail
///
/// - **Texture**: Dual-scale bandpass frequency separation (fine vs medium frequencies).
/// - **Clarity**: Midtone local contrast enhancement via guided local luminance average.
/// - **Dehaze**: Atmospheric scattering correction via dark-channel transmission veil removal.
/// - **Midtone Detail (DaVinci MD)**: Targeted midtone frequency contrast boost or beauty skin softening
///   isolated from shadows and specular highlights.
pub fn apply_presence(
    rgb_buffer: &mut [f32],
    width: usize,
    height: usize,
    clarity: f32,
    texture: f32,
    dehaze: f32,
) {
    apply_presence_ex(rgb_buffer, width, height, clarity, texture, dehaze, 0.0);
}

pub fn apply_presence_ex(
    rgb_buffer: &mut [f32],
    width: usize,
    height: usize,
    clarity: f32,
    texture: f32,
    dehaze: f32,
    midtone_detail: f32,
) {
    apply_presence_scaled(rgb_buffer, width, height, clarity, texture, dehaze, midtone_detail, 1.0);
}

/// Blur radius `base` (in pixels of a frame whose long edge is
/// [`crate::pipeline::REFERENCE_EDGE`]) for a frame `scale` times as large.
#[inline]
pub fn scaled_radius(base: usize, scale: f32) -> usize {
    ((base as f32 * scale).round() as usize).max(1)
}

/// [`apply_presence_ex`] with the neighbourhoods scaled to the frame, so clarity,
/// texture, dehaze and midtone detail look the same in the preview and in the export.
#[allow(clippy::too_many_arguments)]
pub fn apply_presence_scaled(
    rgb_buffer: &mut [f32],
    width: usize,
    height: usize,
    clarity: f32,
    texture: f32,
    dehaze: f32,
    midtone_detail: f32,
    scale: f32,
) {
    if clarity.abs() < 0.01 && texture.abs() < 0.01 && dehaze.abs() < 0.01 && midtone_detail.abs() < 0.01 {
        return;
    }

    let num_pixels = width * height;
    if rgb_buffer.len() < num_pixels * 3 {
        return;
    }

    // Extract luminance channel
    let mut luma = vec![0.0f32; num_pixels];
    luma.par_iter_mut().enumerate().for_each(|(i, l)| {
        let px = i * 3;
        *l = 0.2126 * rgb_buffer[px] + 0.7152 * rgb_buffer[px + 1] + 0.0722 * rgb_buffer[px + 2];
    });

    // --- 1. TEXTURE ENGINE (Bandpass Micro-Contrast) ---
    let (t_factor, blur_fine, blur_med) = if texture.abs() >= 0.5 {
        let fine = fast_separable_blur(&luma, width, height, scaled_radius(2, scale));
        let med = fast_separable_blur(&luma, width, height, scaled_radius(6, scale));
        ((texture / 100.0) * 1.6, Some(fine), Some(med))
    } else {
        (0.0, None, None)
    };

    // --- 2. CLARITY ENGINE (Midtone Local Contrast) ---
    let (c_factor, blur_broad) = if clarity.abs() >= 0.5 {
        let broad = fast_separable_blur(&luma, width, height, scaled_radius(16, scale));
        ((clarity / 100.0) * 0.85, Some(broad))
    } else {
        (0.0, None)
    };

    // --- 3. DEHAZE ENGINE (Dark-Channel Atmospheric Veil) ---
    let (dehaze_t_factor, dehaze_veil) = if dehaze.abs() >= 0.5 {
        // Compute dark channel min(R, G, B)
        let mut dark = vec![0.0f32; num_pixels];
        dark.par_iter_mut().enumerate().for_each(|(i, d)| {
            let px = i * 3;
            *d = rgb_buffer[px].min(rgb_buffer[px + 1]).min(rgb_buffer[px + 2]);
        });
        let veil = fast_separable_blur(&dark, width, height, scaled_radius(12, scale));
        (dehaze / 100.0, Some(veil))
    } else {
        (0.0, None)
    };

    // --- 4. DAVINCI MIDTONE DETAIL ENGINE ---
    let (md_factor, blur_md_fine, blur_md_broad) = if midtone_detail.abs() >= 0.5 {
        let fine = fast_separable_blur(&luma, width, height, scaled_radius(3, scale));
        let broad = fast_separable_blur(&luma, width, height, scaled_radius(9, scale));
        ((midtone_detail / 100.0) * 1.25, Some(fine), Some(broad))
    } else {
        (0.0, None, None)
    };

    // Combine deltas and apply to RGB buffer in parallel
    rgb_buffer
        .par_chunks_mut(3)
        .enumerate()
        .for_each(|(i, rgb)| {
            let mut r = rgb[0];
            let mut g = rgb[1];
            let mut b = rgb[2];
            let cur_luma = luma[i];

            let mut luma_shift = 0.0f32;

            if let (Some(fine), Some(med)) = (&blur_fine, &blur_med) {
                let detail = fine[i] - med[i];
                luma_shift += detail * t_factor;
            }

            if let Some(ref broad) = blur_broad {
                let midtone_mask = (4.0 * cur_luma * (1.0 - cur_luma)).clamp(0.0, 1.0);
                let local_contrast = cur_luma - broad[i];
                luma_shift += local_contrast * c_factor * midtone_mask;
            }

            if let (Some(fine), Some(broad)) = (&blur_md_fine, &blur_md_broad) {
                let md_detail = fine[i] - broad[i];
                let dist = cur_luma.clamp(0.0, 1.0) - 0.5;
                let midtone_bell = (-dist * dist / 0.06).exp();
                luma_shift += md_detail * md_factor * midtone_bell;
            }

            // Apply Dehaze atmospheric contrast recovery
            if let Some(ref veil) = dehaze_veil {
                let v = veil[i].clamp(0.0, 0.90);
                if dehaze_t_factor > 0.0 {
                    // Positive Dehaze: remove haze transmission veil
                    let atmospheric_light = 0.88f32;
                    let strength = dehaze_t_factor * 0.65;
                    let restore = (v * strength).clamp(0.0, 0.5);
                    r = (r - atmospheric_light * restore) / (1.0 - restore).max(0.1);
                    g = (g - atmospheric_light * restore) / (1.0 - restore).max(0.1);
                    b = (b - atmospheric_light * restore) / (1.0 - restore).max(0.1);

                    // Dehaze color saturation boost
                    let new_luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let sat_boost = 1.0 + dehaze_t_factor * 0.35 * v;
                    r = new_luma + (r - new_luma) * sat_boost;
                    g = new_luma + (g - new_luma) * sat_boost;
                    b = new_luma + (b - new_luma) * sat_boost;
                } else {
                    // Negative Dehaze: add soft atmospheric mist / glow
                    let mist = (-dehaze_t_factor * 0.40) * (1.0 - v * 0.5);
                    r = r + (0.85 - r) * mist;
                    g = g + (0.85 - g) * mist;
                    b = b + (0.90 - b) * mist;
                }
            }

            // Apply texture and clarity luminance shift while preserving chromatic purity
            if luma_shift.abs() > 1e-5 {
                let new_l = (cur_luma + luma_shift).max(0.0);
                let cr = r - cur_luma;
                let cg = g - cur_luma;
                let cb = b - cur_luma;

                r = new_l + cr;
                g = new_l + cg;
                b = new_l + cb;
            }

            rgb[0] = r;
            rgb[1] = g;
            rgb[2] = b;
        });
}

#[cfg(test)]
mod tests {
    use super::fast_separable_blur;

    fn naive_separable_blur(input: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
        let r = radius.min(width / 2).min(height / 2).max(1);
        let win = (2 * r + 1) as f32;
        let mut horiz = vec![0.0f32; width * height];
        for y in 0..height {
            for x in 0..width {
                let mut s = 0.0f32;
                for dx in -(r as isize)..=(r as isize) {
                    let sx = (x as isize + dx).clamp(0, (width - 1) as isize) as usize;
                    s += input[y * width + sx];
                }
                horiz[y * width + x] = s / win;
            }
        }
        let mut out = vec![0.0f32; width * height];
        for y in 0..height {
            for x in 0..width {
                let mut s = 0.0f32;
                for dy in -(r as isize)..=(r as isize) {
                    let sy = (y as isize + dy).clamp(0, (height - 1) as isize) as usize;
                    s += horiz[sy * width + x];
                }
                out[y * width + x] = s / win;
            }
        }
        out
    }

    #[test]
    fn safe_separable_blur_matches_naive_across_multi_band_sizes() {
        for &(w, h, r) in &[(17, 13, 3), (64, 97, 5), (113, 145, 16)] {
            let input: Vec<f32> = (0..w * h)
                .map(|i| (((i * 37 + 13) % 251) as f32) / 250.0)
                .collect();
            let got = fast_separable_blur(&input, w, h, r);
            let expected = naive_separable_blur(&input, w, h, r);
            assert_eq!(got.len(), expected.len());
            for (idx, (a, b)) in got.iter().zip(expected.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-4,
                    "blur mismatch at {idx} for {w}x{h} r={r}: got {a}, expected {b}"
                );
            }
        }
    }
}

