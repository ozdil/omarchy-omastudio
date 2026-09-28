use rayon::prelude::*;

/// Fast O(1) separable horizontal and vertical box blur on single-channel f32 buffers
pub fn fast_separable_blur(
    input: &[f32],
    width: usize,
    height: usize,
    radius: usize,
) -> Vec<f32> {
    if radius == 0 || width == 0 || height == 0 {
        return input.to_vec();
    }

    let r = radius.min(width / 2).min(height / 2).max(1);
    let mut temp = vec![0.0f32; width * height];
    let mut output = vec![0.0f32; width * height];

    // 1. Horizontal Pass (Parallelized over rows with Rayon)
    temp.par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, out_row)| {
            let in_row = &input[y * width..(y + 1) * width];
            let in_ptr = in_row.as_ptr();
            let out_ptr = out_row.as_mut_ptr();
            let mut sum = 0.0f32;

            // Initialize window
            for x in 0..=r {
                sum += unsafe { *in_ptr.add(x.min(width - 1)) };
            }
            sum += unsafe { *in_ptr } * (r as f32);

            let win_size = (2 * r + 1) as f32;
            let inv_win = 1.0 / win_size;

            for x in 0..width {
                let left = x.saturating_sub(r);
                let right = (x + r + 1).min(width - 1);
                unsafe {
                    *out_ptr.add(x) = sum * inv_win;
                    sum += *in_ptr.add(right) - *in_ptr.add(left);
                }
            }
        });

    // 2. Vertical Pass (Parallelized over column chunks with Rayon for cache-friendly O(1) rolling sum)
    let win_size_v = (2 * r + 1) as f32;
    let inv_win_v = 1.0 / win_size_v;

    let num_threads = rayon::current_num_threads().max(1);
    let chunk_size = width.div_ceil(num_threads).max(16);
    let col_chunks: Vec<(usize, usize)> = (0..width)
        .step_by(chunk_size)
        .map(|start_x| (start_x, (start_x + chunk_size).min(width)))
        .collect();

    #[derive(Copy, Clone)]
    struct SendSyncPtr(*mut f32);
    unsafe impl Send for SendSyncPtr {}
    unsafe impl Sync for SendSyncPtr {}

    impl SendSyncPtr {
        #[inline(always)]
        unsafe fn add(self, offset: usize) -> *mut f32 {
            self.0.add(offset)
        }
    }

    let out_ptr = SendSyncPtr(output.as_mut_ptr());
    let temp_slice = temp.as_slice();

    col_chunks.into_par_iter().for_each(move |(start_x, end_x)| {
        let chunk_w = end_x - start_x;
        let mut col_sums = vec![0.0f32; chunk_w];

        // Initialize window for this band of columns
        for dy in 0..=r {
            let row_idx = dy.min(height - 1);
            let row = &temp_slice[row_idx * width + start_x..row_idx * width + end_x];
            for (i, &val) in row.iter().enumerate() {
                col_sums[i] += val;
            }
        }
        let top_row = &temp_slice[start_x..end_x];
        let r_f32 = r as f32;
        for (i, &val) in top_row.iter().enumerate() {
            col_sums[i] += val * r_f32;
        }

        // Rolling sum down the columns
        for y in 0..height {
            let top_y = y.saturating_sub(r);
            let bot_y = (y + r + 1).min(height - 1);
            let top_offset = top_y * width + start_x;
            let bot_offset = bot_y * width + start_x;
            let row_offset = y * width + start_x;

            unsafe {
                let out = out_ptr.add(row_offset);

                for (i, sum) in col_sums.iter_mut().enumerate() {
                    *out.add(i) = *sum * inv_win_v;
                    let bot_val = *temp_slice.get_unchecked(bot_offset + i);
                    let top_val = *temp_slice.get_unchecked(top_offset + i);
                    *sum += bot_val - top_val;
                }
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
        let fine = fast_separable_blur(&luma, width, height, 2);
        let med = fast_separable_blur(&luma, width, height, 6);
        ((texture / 100.0) * 1.6, Some(fine), Some(med))
    } else {
        (0.0, None, None)
    };

    // --- 2. CLARITY ENGINE (Midtone Local Contrast) ---
    let (c_factor, blur_broad) = if clarity.abs() >= 0.5 {
        let broad = fast_separable_blur(&luma, width, height, 16);
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
        let veil = fast_separable_blur(&dark, width, height, 12);
        (dehaze / 100.0, Some(veil))
    } else {
        (0.0, None)
    };

    // --- 4. DAVINCI MIDTONE DETAIL ENGINE ---
    let (md_factor, blur_md_fine, blur_md_broad) = if midtone_detail.abs() >= 0.5 {
        let fine = fast_separable_blur(&luma, width, height, 3);
        let broad = fast_separable_blur(&luma, width, height, 9);
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

            if let (Some(ref fine), Some(ref med)) = (&blur_fine, &blur_med) {
                let detail = fine[i] - med[i];
                luma_shift += detail * t_factor;
            }

            if let Some(ref broad) = blur_broad {
                let midtone_mask = (4.0 * cur_luma * (1.0 - cur_luma)).clamp(0.0, 1.0);
                let local_contrast = cur_luma - broad[i];
                luma_shift += local_contrast * c_factor * midtone_mask;
            }

            if let (Some(ref fine), Some(ref broad)) = (&blur_md_fine, &blur_md_broad) {
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
