pub mod color_grading;
pub mod detail;
pub mod histogram;
pub mod lut;
pub mod presence;
pub mod tone;
pub mod white_balance;

use crate::recipe::Recipe;
use histogram::{compute_histogram_with_dimensions, compute_histogram_16_with_dimensions, HistogramData};
use rayon::prelude::*;

/// Resolves active 3D LUT (built-in cinematic profile or external .cube file)
pub fn resolve_recipe_lut(recipe: &Recipe) -> Option<lut::Lut3D> {
    if let Some(ref path) = recipe.lut_path {
        if !path.is_empty() {
            if let Ok(l) = lut::read_cube_file(path) {
                return Some(l);
            }
        }
    }
    if let Some(ref name) = recipe.lut_name {
        match name.as_str() {
            "Kodak 2383" => Some(lut::Lut3D::kodak_2383()),
            "Teal & Orange" => Some(lut::Lut3D::teal_and_orange()),
            "Fuji Eterna" => Some(lut::Lut3D::fuji_eterna()),
            "Silver Nitrate" => Some(lut::Lut3D::silver_nitrate()),
            _ => None,
        }
    } else {
        None
    }
}

/// Processes an 8-bit linear RGB buffer according to the given Recipe using parallel multi-threading
pub fn process_buffer(
    input: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
) -> (Vec<u8>, HistogramData) {
    let ch = channels as usize;
    let w = width as usize;
    let h = height as usize;
    let num_pixels = w * h;

    let (wb_r, wb_g, wb_b) = white_balance::kelvin_to_rgb_multipliers(
        recipe.wb_temperature,
        recipe.wb_tint,
    );
    let exp_factor = 2.0f32.powf(recipe.exposure);

    // Resolve active 3D LUT if requested
    let maybe_lut = resolve_recipe_lut(recipe);

    // Intermediate float buffer for multi-stage 16-bit precision processing
    let mut f32_buf = vec![0.0f32; num_pixels * 3];

    // PASS 1: Point Operations (White Balance, Exposure, Tone, Color Wheels, HSL Mixer, 3D LUT)
    f32_buf
        .par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y_idx, row_f32)| {
            let in_row = &input[y_idx * w * ch..(y_idx + 1) * w * ch];

            for x in 0..w {
                let px_in = x * ch;
                let px_out = x * 3;

                let r_norm = (in_row[px_in] as f32 / 255.0) * wb_r;
                let g_norm = (in_row[px_in + 1] as f32 / 255.0) * wb_g;
                let b_norm = (in_row[px_in + 2] as f32 / 255.0) * wb_b;

                // 1. Tone & Dynamic range (Chroma-preserving, smooth highlight knee & shadow toe)
                let (r1, g1, b1) = tone::apply_tone_pixel(r_norm, g_norm, b_norm, recipe, exp_factor);

                // 2. DaVinci Resolve 3-Way Color Wheels
                let (r2, g2, b2) = color_grading::apply_color_wheels(r1, g1, b1, recipe);

                // 3. 8-Band HSL Color Mixer (Perceptual Luminance Anchor)
                let (r3, g3, b3) = color_grading::apply_hsl_mixer(r2, g2, b2, recipe);

                // 4. DaVinci 3D LUT
                let (r4, g4, b4) = if let Some(ref active_lut) = maybe_lut {
                    lut::apply_lut_pixel(r3, g3, b3, active_lut, recipe.lut_intensity)
                } else {
                    (r3, g3, b3)
                };

                row_f32[px_out] = r4;
                row_f32[px_out + 1] = g4;
                row_f32[px_out + 2] = b4;
            }
        });

    // PASS 2: Presence Engine (Texture, Clarity, Dehaze, DaVinci Midtone Detail)
    presence::apply_presence_ex(
        &mut f32_buf,
        w,
        h,
        recipe.clarity,
        recipe.texture,
        recipe.dehaze,
        recipe.midtone_detail,
    );

    // PASS 3: Detail & Optics Engine (Sharpness, Luma Denoise, Chroma Denoise)
    detail::apply_detail(
        &mut f32_buf,
        w,
        h,
        recipe.sharpness,
        recipe.denoise_lum,
        recipe.denoise_col,
    );

    // PASS 4: Lens Distortion Correction (if enabled)
    if recipe.lens_distortion.abs() >= 0.1 {
        f32_buf = detail::apply_lens_distortion(&f32_buf, w, h, 3, recipe.lens_distortion);
    }

    // PASS 5: Defringe, Vignette & Final Quantization to 8-bit output
    let mut output = vec![0u8; num_pixels * ch];
    output
        .par_chunks_mut(w * ch)
        .enumerate()
        .for_each(|(y_idx, row)| {
            let in_f32 = &f32_buf[y_idx * w * 3..(y_idx + 1) * w * 3];
            let y = y_idx as u32;

            for x in 0..w {
                let px_out = x * ch;
                let px_f32 = x * 3;

                let (r4, g4, b4) = detail::apply_defringe(
                    in_f32[px_f32],
                    in_f32[px_f32 + 1],
                    in_f32[px_f32 + 2],
                    recipe.defringe,
                );

                let (r5, g5, b5) = detail::apply_vignette(
                    r4,
                    g4,
                    b4,
                    x as u32,
                    y,
                    width,
                    height,
                    recipe.vignette,
                );

                row[px_out] = (r5.clamp(0.0, 1.0) * 255.0) as u8;
                row[px_out + 1] = (g5.clamp(0.0, 1.0) * 255.0) as u8;
                row[px_out + 2] = (b5.clamp(0.0, 1.0) * 255.0) as u8;

                if ch == 4 {
                    let orig_alpha = input[y_idx * w * ch + px_out + 3];
                    row[px_out + 3] = orig_alpha;
                }
            }
        });

    let hist = compute_histogram_with_dimensions(&output, w, h, ch);
    (output, hist)
}

/// Processes a 16-bit linear RGB buffer (0..65535) and outputs an 8-bit RGB buffer for high-speed viewport rendering
pub fn process_buffer_16_to_8(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
) -> (Vec<u8>, HistogramData) {
    process_buffer_16_to_8_ex(input, width, height, channels, recipe, false, false)
}

/// Extended 16-to-8 processing supporting zebra highlight & shadow clipping mask overlays
pub fn process_buffer_16_to_8_ex(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    highlight_mask: bool,
    shadow_mask: bool,
) -> (Vec<u8>, HistogramData) {
    let ch = channels as usize;
    let w = width as usize;
    let h = height as usize;
    let num_pixels = w * h;

    let (wb_r, wb_g, wb_b) = white_balance::kelvin_to_rgb_multipliers(
        recipe.wb_temperature,
        recipe.wb_tint,
    );
    let exp_factor = 2.0f32.powf(recipe.exposure);

    // Resolve active 3D LUT if requested
    let maybe_lut = resolve_recipe_lut(recipe);

    // Intermediate float buffer for multi-stage 16-bit precision processing
    let mut f32_buf = vec![0.0f32; num_pixels * 3];

    // PASS 1: Point Operations (White Balance, Exposure, Tone, Color Wheels, HSL Mixer, 3D LUT)
    f32_buf
        .par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y_idx, row_f32)| {
            let in_row = &input[y_idx * w * ch..(y_idx + 1) * w * ch];

            for x in 0..w {
                let px_in = x * ch;
                let px_out = x * 3;

                let r_norm = (in_row[px_in] as f32 / 65535.0) * wb_r;
                let g_norm = (in_row[px_in + 1] as f32 / 65535.0) * wb_g;
                let b_norm = (in_row[px_in + 2] as f32 / 65535.0) * wb_b;

                // 1. Tone & Dynamic range
                let (r1, g1, b1) = tone::apply_tone_pixel(r_norm, g_norm, b_norm, recipe, exp_factor);

                // 2. DaVinci Resolve 3-Way Color Wheels
                let (r2, g2, b2) = color_grading::apply_color_wheels(r1, g1, b1, recipe);

                // 3. 8-Band HSL Color Mixer
                let (r3, g3, b3) = color_grading::apply_hsl_mixer(r2, g2, b2, recipe);

                // 4. DaVinci 3D LUT
                let (r4, g4, b4) = if let Some(ref active_lut) = maybe_lut {
                    lut::apply_lut_pixel(r3, g3, b3, active_lut, recipe.lut_intensity)
                } else {
                    (r3, g3, b3)
                };

                row_f32[px_out] = r4;
                row_f32[px_out + 1] = g4;
                row_f32[px_out + 2] = b4;
            }
        });

    // PASS 2: Presence Engine (Texture, Clarity, Dehaze, DaVinci Midtone Detail)
    presence::apply_presence_ex(
        &mut f32_buf,
        w,
        h,
        recipe.clarity,
        recipe.texture,
        recipe.dehaze,
        recipe.midtone_detail,
    );

    // PASS 3: Detail & Optics Engine (Sharpness, Luma Denoise, Chroma Denoise)
    detail::apply_detail(
        &mut f32_buf,
        w,
        h,
        recipe.sharpness,
        recipe.denoise_lum,
        recipe.denoise_col,
    );

    // PASS 4: Lens Distortion Correction (if enabled)
    if recipe.lens_distortion.abs() >= 0.1 {
        f32_buf = detail::apply_lens_distortion(&f32_buf, w, h, 3, recipe.lens_distortion);
    }

    // PASS 5: Defringe, Vignette & Final Quantization to 8-bit viewport output
    let mut output = vec![0u8; num_pixels * ch];
    output
        .par_chunks_mut(w * ch)
        .enumerate()
        .for_each(|(y_idx, row)| {
            let in_f32 = &f32_buf[y_idx * w * 3..(y_idx + 1) * w * 3];
            let y = y_idx as u32;

            for x in 0..w {
                let px_out = x * ch;
                let px_f32 = x * 3;

                let (r4, g4, b4) = detail::apply_defringe(
                    in_f32[px_f32],
                    in_f32[px_f32 + 1],
                    in_f32[px_f32 + 2],
                    recipe.defringe,
                );

                let (r5, g5, b5) = detail::apply_vignette(
                    r4,
                    g4,
                    b4,
                    x as u32,
                    y,
                    width,
                    height,
                    recipe.vignette,
                );

                row[px_out] = (r5.clamp(0.0, 1.0) * 255.0) as u8;
                row[px_out + 1] = (g5.clamp(0.0, 1.0) * 255.0) as u8;
                row[px_out + 2] = (b5.clamp(0.0, 1.0) * 255.0) as u8;

                if ch == 4 {
                    let orig_alpha = input[y_idx * w * ch + px_out + 3];
                    row[px_out + 3] = (orig_alpha >> 8) as u8;
                }
            }
        });

    let hist = compute_histogram_with_dimensions(&output, w, h, ch);

    // Visual clipping overlay: Red for highlights (>= 254), Blue for shadows (<= 1)
    if highlight_mask || shadow_mask {
        output
            .par_chunks_mut(ch)
            .for_each(|px| {
                let r = px[0];
                let g = px[1];
                let b = px[2];
                if highlight_mask && (r >= 254 || g >= 254 || b >= 254) {
                    px[0] = 255;
                    px[1] = 0;
                    px[2] = 0;
                } else if shadow_mask && (r <= 1 && g <= 1 && b <= 1) {
                    px[0] = 0;
                    px[1] = 80;
                    px[2] = 255;
                }
            });
    }

    (output, hist)
}

/// Processes a 16-bit linear RGB buffer and outputs a 16-bit RGB buffer (0..65535) for master export
pub fn process_buffer_16_to_16(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
) -> (Vec<u16>, HistogramData) {
    let ch = channels as usize;
    let w = width as usize;
    let h = height as usize;
    let num_pixels = w * h;

    let (wb_r, wb_g, wb_b) = white_balance::kelvin_to_rgb_multipliers(
        recipe.wb_temperature,
        recipe.wb_tint,
    );
    let exp_factor = 2.0f32.powf(recipe.exposure);

    // Resolve active 3D LUT if requested
    let maybe_lut = resolve_recipe_lut(recipe);

    // Intermediate float buffer for multi-stage 16-bit precision processing
    let mut f32_buf = vec![0.0f32; num_pixels * 3];

    // PASS 1: Point Operations (White Balance, Exposure, Tone, Color Wheels, HSL Mixer, 3D LUT)
    f32_buf
        .par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y_idx, row_f32)| {
            let in_row = &input[y_idx * w * ch..(y_idx + 1) * w * ch];

            for x in 0..w {
                let px_in = x * ch;
                let px_out = x * 3;

                let r_norm = (in_row[px_in] as f32 / 65535.0) * wb_r;
                let g_norm = (in_row[px_in + 1] as f32 / 65535.0) * wb_g;
                let b_norm = (in_row[px_in + 2] as f32 / 65535.0) * wb_b;

                // 1. Tone & Dynamic range
                let (r1, g1, b1) = tone::apply_tone_pixel(r_norm, g_norm, b_norm, recipe, exp_factor);

                // 2. DaVinci Resolve 3-Way Color Wheels
                let (r2, g2, b2) = color_grading::apply_color_wheels(r1, g1, b1, recipe);

                // 3. 8-Band HSL Color Mixer
                let (r3, g3, b3) = color_grading::apply_hsl_mixer(r2, g2, b2, recipe);

                // 4. DaVinci 3D LUT
                let (r4, g4, b4) = if let Some(ref active_lut) = maybe_lut {
                    lut::apply_lut_pixel(r3, g3, b3, active_lut, recipe.lut_intensity)
                } else {
                    (r3, g3, b3)
                };

                row_f32[px_out] = r4;
                row_f32[px_out + 1] = g4;
                row_f32[px_out + 2] = b4;
            }
        });

    // PASS 2: Presence Engine (Texture, Clarity, Dehaze, DaVinci Midtone Detail)
    presence::apply_presence_ex(
        &mut f32_buf,
        w,
        h,
        recipe.clarity,
        recipe.texture,
        recipe.dehaze,
        recipe.midtone_detail,
    );

    // PASS 3: Detail & Optics Engine (Sharpness, Luma Denoise, Chroma Denoise)
    detail::apply_detail(
        &mut f32_buf,
        w,
        h,
        recipe.sharpness,
        recipe.denoise_lum,
        recipe.denoise_col,
    );

    // PASS 4: Lens Distortion Correction (if enabled)
    if recipe.lens_distortion.abs() >= 0.1 {
        f32_buf = detail::apply_lens_distortion(&f32_buf, w, h, 3, recipe.lens_distortion);
    }

    // PASS 5: Defringe, Vignette & Final Quantization to 16-bit master output
    let mut output = vec![0u16; num_pixels * ch];
    output
        .par_chunks_mut(w * ch)
        .enumerate()
        .for_each(|(y_idx, row)| {
            let in_f32 = &f32_buf[y_idx * w * 3..(y_idx + 1) * w * 3];
            let y = y_idx as u32;

            for x in 0..w {
                let px_out = x * ch;
                let px_f32 = x * 3;

                let (r4, g4, b4) = detail::apply_defringe(
                    in_f32[px_f32],
                    in_f32[px_f32 + 1],
                    in_f32[px_f32 + 2],
                    recipe.defringe,
                );

                let (r5, g5, b5) = detail::apply_vignette(
                    r4,
                    g4,
                    b4,
                    x as u32,
                    y,
                    width,
                    height,
                    recipe.vignette,
                );

                row[px_out] = (r5.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
                row[px_out + 1] = (g5.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
                row[px_out + 2] = (b5.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;

                if ch == 4 {
                    row[px_out + 3] = input[y_idx * w * ch + px_out + 3];
                }
            }
        });

    let hist = compute_histogram_16_with_dimensions(&output, w, h, ch);
    (output, hist)
}

/// Generates a Before / After split comparison buffer
pub fn process_split_comparison(
    input: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    split_ratio: f32,
) -> (Vec<u8>, HistogramData) {
    let (processed, hist) = process_buffer(input, width, height, channels, recipe);
    let ch = channels as usize;
    let w = width as usize;
    let split_x = ((width as f32) * split_ratio.clamp(0.0, 1.0)) as usize;

    let mut output = vec![0u8; input.len()];
    output
        .par_chunks_mut(w * ch)
        .enumerate()
        .for_each(|(y, row)| {
            let orig_row = &input[y * w * ch..(y + 1) * w * ch];
            let proc_row = &processed[y * w * ch..(y + 1) * w * ch];

            for x in 0..w {
                let px = x * ch;
                if x < split_x {
                    // BEFORE
                    row[px..px + ch].copy_from_slice(&orig_row[px..px + ch]);
                } else if x == split_x || x == split_x + 1 {
                    // Split dividing white line
                    row[px] = 255;
                    row[px + 1] = 255;
                    row[px + 2] = 255;
                    if ch == 4 {
                        row[px + 3] = 255;
                    }
                } else {
                    // AFTER
                    row[px..px + ch].copy_from_slice(&proc_row[px..px + ch]);
                }
            }
        });

    (output, hist)
}

/// Generates a Before / After split comparison buffer from a 16-bit source to an 8-bit viewport buffer
pub fn process_split_comparison_16_to_8(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    split_ratio: f32,
) -> (Vec<u8>, HistogramData) {
    process_split_comparison_16_to_8_ex(input, width, height, channels, recipe, split_ratio, false, false)
}

/// Extended Before / After split comparison buffer with clipping mask overlay support
pub fn process_split_comparison_16_to_8_ex(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    split_ratio: f32,
    highlight_mask: bool,
    shadow_mask: bool,
) -> (Vec<u8>, HistogramData) {
    let (processed, hist) = process_buffer_16_to_8_ex(input, width, height, channels, recipe, highlight_mask, shadow_mask);
    let ch = channels as usize;
    let w = width as usize;
    let split_x = ((width as f32) * split_ratio.clamp(0.0, 1.0)) as usize;

    let mut output = vec![0u8; (width as usize) * (height as usize) * ch];
    output
        .par_chunks_mut(w * ch)
        .enumerate()
        .for_each(|(y, row)| {
            let orig_row = &input[y * w * ch..(y + 1) * w * ch];
            let proc_row = &processed[y * w * ch..(y + 1) * w * ch];

            let to_srgb = |val16: u16| -> u8 {
                let lin = val16 as f32 / 65535.0;
                let srgb = if lin <= 0.0031308 {
                    lin * 12.92
                } else {
                    1.055 * lin.powf(1.0 / 2.4) - 0.055
                };
                (srgb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
            };

            for x in 0..w {
                let px = x * ch;
                if x < split_x {
                    // BEFORE (Original unprocessed RAW with standard sRGB transfer curve)
                    row[px] = to_srgb(orig_row[px]);
                    row[px + 1] = to_srgb(orig_row[px + 1]);
                    row[px + 2] = to_srgb(orig_row[px + 2]);
                    if ch == 4 {
                        row[px + 3] = (orig_row[px + 3] >> 8) as u8;
                    }
                } else if x == split_x || x == split_x + 1 {
                    // White split separator line
                    row[px] = 255;
                    row[px + 1] = 255;
                    row[px + 2] = 255;
                    if ch == 4 {
                        row[px + 3] = 255;
                    }
                } else {
                    // AFTER (Fine-tuned Recipe)
                    row[px..px + ch].copy_from_slice(&proc_row[px..px + ch]);
                }
            }
        });

    (output, hist)
}
