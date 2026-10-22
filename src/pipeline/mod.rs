pub mod aces;
pub mod color_grading;
pub mod detail;
pub mod film_grain;
pub mod film_sim;
pub mod histogram;
pub mod layers;
pub mod lut;
pub mod presence;
pub mod rgb_mixer;
pub mod skin_tone;
pub mod tone;
pub mod transfer;
pub mod white_balance;

use crate::recipe::Recipe;
use histogram::{compute_histogram_with_dimensions, compute_histogram_16_with_dimensions, HistogramData};
use rayon::prelude::*;
use transfer::Transfer;

/// Resolves active 3D LUT (built-in cinematic profile or external .cube file)
pub fn resolve_recipe_lut(recipe: &Recipe) -> Option<lut::Lut3D> {
    if let Some(ref path) = recipe.lut_path
        && !path.is_empty()
        && let Ok(l) = lut::read_cube_file(path)
    {
        return Some(l);
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
    let film_sim = film_sim::FilmSimulation::from_id(&recipe.film_simulation);
    let film_lut = film_sim::calibrated_lut(film_sim);
    let color_chrome = recipe.color_chrome > 0.0 || recipe.color_chrome_blue > 0.0;
    let working_space = aces::WorkingColorSpace::from_str_name(&recipe.color_space);

    // Intermediate float buffer for multi-stage 16-bit precision processing
    let mut f32_buf = vec![0.0f32; num_pixels * 3];

    // PASS 1: Point Operations (White Balance, Exposure, Tone, Color Wheels, HSL Mixer, 3D LUT, Film Sim, ACES)
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

                // 2. DaVinci Resolve RGB Primary Matrix Mixer
                let (r1_mix, g1_mix, b1_mix) = rgb_mixer::apply_rgb_mixer(r1, g1, b1, recipe);

                // 3. Capture One Pro Style Skin Tone Uniformity Engine
                let (r1_skin, g1_skin, b1_skin) = skin_tone::apply_skin_tone_uniformity(r1_mix, g1_mix, b1_mix, recipe);

                // 4. DaVinci Resolve 3-Way Color Wheels
                let (r2, g2, b2) = color_grading::apply_color_wheels(r1_skin, g1_skin, b1_skin, recipe);

                // 5. 8-Band HSL Color Mixer (Perceptual Luminance Anchor)
                let (r3, g3, b3) = color_grading::apply_hsl_mixer(r2, g2, b2, recipe);

                // 6. DaVinci 3D LUT
                let (r4, g4, b4) = if let Some(ref active_lut) = maybe_lut {
                    lut::apply_lut_pixel(r3, g3, b3, active_lut, recipe.lut_intensity)
                } else {
                    (r3, g3, b3)
                };

                // 7. Authentic Film Simulations (Fujifilm X-Trans / GFX & Hasselblad HNCS / XPan)
                let (r5, g5, b5) = if film_sim != film_sim::FilmSimulation::None {
                    film_sim::apply(film_sim, film_lut.as_deref(), r4, g4, b4, recipe.film_sim_intensity)
                } else {
                    (r4, g4, b4)
                };
                let (r5, g5, b5) = if color_chrome {
                    film_sim::apply_color_chrome(r5, g5, b5, recipe.color_chrome, recipe.color_chrome_blue)
                } else {
                    (r5, g5, b5)
                };

                // 8. ACES 1.3 Gamut Compression if ACEScg or wide-gamut mode active
                let (r6, g6, b6) = if working_space == aces::WorkingColorSpace::AcesCg || recipe.aces_tonemap {
                    aces::apply_aces_gamut_compression(r5, g5, b5)
                } else {
                    (r5, g5, b5)
                };

                row_f32[px_out] = r6;
                row_f32[px_out + 1] = g6;
                row_f32[px_out + 2] = b6;
            }
        });

    // PASS 1.5: Local Layered Mask Adjustments (Linear, Radial, Luma Range)
    if !recipe.layers.is_empty() {
        let inv_w = 1.0 / (w as f32).max(1.0);
        let inv_h = 1.0 / (h as f32).max(1.0);
        f32_buf
            .par_chunks_mut(w * 3)
            .enumerate()
            .for_each(|(y_idx, row_f32)| {
                let v = (y_idx as f32 + 0.5) * inv_h;
                for x in 0..w {
                    let u = (x as f32 + 0.5) * inv_w;
                    let px = x * 3;
                    let (lr, lg, lb) = layers::apply_layers_pixel(
                        row_f32[px],
                        row_f32[px + 1],
                        row_f32[px + 2],
                        &recipe.layers,
                        u,
                        v,
                    );
                    row_f32[px] = lr;
                    row_f32[px + 1] = lg;
                    row_f32[px + 2] = lb;
                }
            });
    }

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

    // PASS 4: Photochemical Silver-Halide Film Grain
    if recipe.grain_amount > 0.001 {
        film_grain::apply_film_grain(
            &mut f32_buf,
            width,
            height,
            recipe.grain_amount,
            recipe.grain_size,
            recipe.grain_roughness,
        );
    }

    // PASS 5: Lens Distortion Correction (if enabled)
    if recipe.lens_distortion.abs() >= 0.1 {
        f32_buf = detail::apply_lens_distortion(&f32_buf, w, h, 3, recipe.lens_distortion);
    }

    // PASS 6: Defringe, Vignette & Final Quantization to 8-bit output
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

/// Processes a 16-bit linear RGB buffer (0..65535) and outputs an 8-bit RGB buffer for high-speed viewport rendering.
/// The output uses the BT.709 curve; use [`process_buffer_16_to_8_ex`] for another colour space.
pub fn process_buffer_16_to_8(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
) -> (Vec<u8>, HistogramData) {
    process_buffer_16_to_8_ex(input, width, height, channels, recipe, Transfer::bt709(), false, false)
}

/// Extended 16-to-8 processing supporting zebra highlight & shadow clipping mask overlays
#[allow(clippy::too_many_arguments)]
pub fn process_buffer_16_to_8_ex(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    transfer: Transfer,
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
    let film_sim = film_sim::FilmSimulation::from_id(&recipe.film_simulation);
    let film_lut = film_sim::calibrated_lut(film_sim);
    let color_chrome = recipe.color_chrome > 0.0 || recipe.color_chrome_blue > 0.0;
    let working_space = aces::WorkingColorSpace::from_str_name(&recipe.color_space);

    // Intermediate float buffer for multi-stage 16-bit precision processing
    let mut f32_buf = vec![0.0f32; num_pixels * 3];

    // PASS 1: Point Operations (White Balance, Exposure, Tone, Color Wheels, HSL Mixer, 3D LUT, Film Sim, ACES)
    f32_buf
        .par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y_idx, row_f32)| {
            let in_row = &input[y_idx * w * ch..(y_idx + 1) * w * ch];

            for x in 0..w {
                let px_in = x * ch;
                let px_out = x * 3;

                // Scene-linear stage: white balance and exposure act on linear light,
                // then the buffer's display curve is applied once for the creative stage.
                let r_norm = transfer.encode((in_row[px_in] as f32 / 65535.0) * wb_r * exp_factor);
                let g_norm = transfer.encode((in_row[px_in + 1] as f32 / 65535.0) * wb_g * exp_factor);
                let b_norm = transfer.encode((in_row[px_in + 2] as f32 / 65535.0) * wb_b * exp_factor);

                // 1. Tone & Dynamic range (exposure was already applied in linear light)
                let (r1, g1, b1) = tone::apply_tone_pixel(r_norm, g_norm, b_norm, recipe, 1.0);

                // 2. DaVinci Resolve RGB Primary Matrix Mixer
                let (r1_mix, g1_mix, b1_mix) = rgb_mixer::apply_rgb_mixer(r1, g1, b1, recipe);

                // 3. Capture One Pro Style Skin Tone Uniformity Engine
                let (r1_skin, g1_skin, b1_skin) = skin_tone::apply_skin_tone_uniformity(r1_mix, g1_mix, b1_mix, recipe);

                // 4. DaVinci Resolve 3-Way Color Wheels
                let (r2, g2, b2) = color_grading::apply_color_wheels(r1_skin, g1_skin, b1_skin, recipe);

                // 5. 8-Band HSL Color Mixer
                let (r3, g3, b3) = color_grading::apply_hsl_mixer(r2, g2, b2, recipe);

                // 6. DaVinci 3D LUT
                let (r4, g4, b4) = if let Some(ref active_lut) = maybe_lut {
                    lut::apply_lut_pixel(r3, g3, b3, active_lut, recipe.lut_intensity)
                } else {
                    (r3, g3, b3)
                };

                // 7. Authentic Film Simulations (Fujifilm X-Trans / GFX & Hasselblad HNCS / XPan)
                let (r5, g5, b5) = if film_sim != film_sim::FilmSimulation::None {
                    film_sim::apply(film_sim, film_lut.as_deref(), r4, g4, b4, recipe.film_sim_intensity)
                } else {
                    (r4, g4, b4)
                };
                let (r5, g5, b5) = if color_chrome {
                    film_sim::apply_color_chrome(r5, g5, b5, recipe.color_chrome, recipe.color_chrome_blue)
                } else {
                    (r5, g5, b5)
                };

                // 8. ACES 1.3 Gamut Compression if ACEScg or wide-gamut mode active
                let (r6, g6, b6) = if working_space == aces::WorkingColorSpace::AcesCg || recipe.aces_tonemap {
                    aces::apply_aces_gamut_compression(r5, g5, b5)
                } else {
                    (r5, g5, b5)
                };

                row_f32[px_out] = r6;
                row_f32[px_out + 1] = g6;
                row_f32[px_out + 2] = b6;
            }
        });

    // PASS 1.5: Local Layered Mask Adjustments (Linear, Radial, Luma Range)
    if !recipe.layers.is_empty() {
        let inv_w = 1.0 / (w as f32).max(1.0);
        let inv_h = 1.0 / (h as f32).max(1.0);
        f32_buf
            .par_chunks_mut(w * 3)
            .enumerate()
            .for_each(|(y_idx, row_f32)| {
                let v = (y_idx as f32 + 0.5) * inv_h;
                for x in 0..w {
                    let u = (x as f32 + 0.5) * inv_w;
                    let px = x * 3;
                    let (lr, lg, lb) = layers::apply_layers_pixel(
                        row_f32[px],
                        row_f32[px + 1],
                        row_f32[px + 2],
                        &recipe.layers,
                        u,
                        v,
                    );
                    row_f32[px] = lr;
                    row_f32[px + 1] = lg;
                    row_f32[px + 2] = lb;
                }
            });
    }

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

    // PASS 4: Photochemical Silver-Halide Film Grain
    if recipe.grain_amount > 0.001 {
        film_grain::apply_film_grain(
            &mut f32_buf,
            width,
            height,
            recipe.grain_amount,
            recipe.grain_size,
            recipe.grain_roughness,
        );
    }

    // PASS 5: Lens Distortion Correction (if enabled)
    if recipe.lens_distortion.abs() >= 0.1 {
        f32_buf = detail::apply_lens_distortion(&f32_buf, w, h, 3, recipe.lens_distortion);
    }

    // PASS 6: Defringe, Vignette & Final Quantization to 8-bit viewport output
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

/// Processes a 16-bit linear RGB buffer and outputs a 16-bit RGB buffer (0..65535) for master export.
/// The output uses the BT.709 curve; use [`process_buffer_16_to_16_ex`] for another colour space.
pub fn process_buffer_16_to_16(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
) -> (Vec<u16>, HistogramData) {
    process_buffer_16_to_16_ex(input, width, height, channels, recipe, Transfer::bt709())
}

/// Rows per band of the master export. Each band is processed on its own with
/// [`BAND_HALO`] extra rows above and below, so working memory no longer grows with
/// the size of the photo (a 100 MP GFX frame needs the same scratch space as a 24 MP one).
const BAND_ROWS: usize = 384;
/// Context rows around a band: the widest neighbourhood any pass reads (clarity's
/// 16-pixel blur), plus detail's 3 rows that run on presence's result.
const BAND_HALO: usize = 24;
/// Pre-distortion rows are kept as u16 covering 0..2, so values pushed above white by
/// sharpening or clarity survive until the final clamp.
const PRE_DISTORTION_SCALE: f32 = 32767.5;

/// [`process_buffer_16_to_16`] for a buffer whose colour space uses `transfer`.
///
/// The photo is processed in bands of rows. The result is the same as processing it in
/// one piece; only the scratch memory is bounded.
pub fn process_buffer_16_to_16_ex(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    transfer: Transfer,
) -> (Vec<u16>, HistogramData) {
    process_16_to_16_banded(input, width, height, channels, recipe, transfer, BAND_ROWS)
}

fn process_16_to_16_banded(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    transfer: Transfer,
    max_band_rows: usize,
) -> (Vec<u16>, HistogramData) {
    let ch = channels as usize;
    let w = width as usize;
    let h = height as usize;
    let num_pixels = w * h;

    let stage = PointStage::new(recipe, transfer);
    let lens = detail::LensDistortion::new(w, h, recipe.lens_distortion);

    // Bands of equal height, so the last one is never a sliver
    let bands = h.div_ceil(max_band_rows).max(1);
    let band_rows = h.div_ceil(bands).max(1);

    let mut output = vec![0u16; num_pixels * ch];
    // With lens distortion the remap can read rows of any band, so the bands are
    // collected first and the remap, defringe and vignette run on the whole frame
    let mut pre = if lens.is_some() { vec![0u16; num_pixels * 3] } else { Vec::new() };

    for y0 in (0..h).step_by(band_rows) {
        let y1 = (y0 + band_rows).min(h);
        let (by0, by1) = (y0.saturating_sub(BAND_HALO), (y1 + BAND_HALO).min(h));
        let band = process_band(input, w, ch, by0, by1, Window::whole(w, h), recipe, &stage);
        let inner = &band[(y0 - by0) * w * 3..(y1 - by0) * w * 3];

        if lens.is_some() {
            pre[y0 * w * 3..y1 * w * 3].par_iter_mut().zip(inner.par_iter()).for_each(|(o, &v)| {
                *o = (v.clamp(0.0, 2.0) * PRE_DISTORTION_SCALE + 0.5) as u16;
            });
            continue;
        }
        output[y0 * w * ch..y1 * w * ch]
            .par_chunks_mut(w * ch)
            .zip(inner.par_chunks(w * 3))
            .enumerate()
            .for_each(|(i, (row, src))| {
                let y = y0 + i;
                for x in 0..w {
                    let px = (src[x * 3], src[x * 3 + 1], src[x * 3 + 2]);
                    finish_pixel(&mut row[x * ch..x * ch + 3], px, x, y, width, height, recipe);
                    if ch == 4 {
                        row[x * ch + 3] = input[(y * w + x) * ch + 3];
                    }
                }
            });
    }

    if let Some(lens) = lens {
        output.par_chunks_mut(w * ch).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let (sx, sy) = lens.source(x as f32, y as f32);
                let px = if sx >= 0.0 && sx < (w - 1) as f32 && sy >= 0.0 && sy < (h - 1) as f32 {
                    let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
                    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                    let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
                    let at = |yy: usize, xx: usize, c: usize| pre[(yy * w + xx) * 3 + c] as f32 / PRE_DISTORTION_SCALE;
                    let sample = |c: usize| {
                        let top = at(y0, x0, c) * (1.0 - fx) + at(y0, x1, c) * fx;
                        let bottom = at(y1, x0, c) * (1.0 - fx) + at(y1, x1, c) * fx;
                        top * (1.0 - fy) + bottom * fy
                    };
                    (sample(0), sample(1), sample(2))
                } else {
                    (0.0, 0.0, 0.0)
                };
                finish_pixel(&mut row[x * ch..x * ch + 3], px, x, y, width, height, recipe);
                if ch == 4 {
                    row[x * ch + 3] = input[(y * w + x) * ch + 3];
                }
            }
        });
    }

    let hist = compute_histogram_16_with_dimensions(&output, w, h, ch);
    (output, hist)
}

/// Defringe, vignette and 16-bit quantisation of one output pixel.
#[inline(always)]
fn finish_pixel(out: &mut [u16], (r, g, b): (f32, f32, f32), x: usize, y: usize, width: u32, height: u32, recipe: &Recipe) {
    let (r, g, b) = detail::apply_defringe(r, g, b, recipe.defringe);
    let (r, g, b) = detail::apply_vignette(r, g, b, x as u32, y as u32, width, height, recipe.vignette);
    out[0] = (r.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    out[1] = (g.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    out[2] = (b.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
}

/// Per-recipe state of the point operations (pass 1).
struct PointStage {
    wb: (f32, f32, f32),
    exp_factor: f32,
    lut: Option<lut::Lut3D>,
    film_sim: film_sim::FilmSimulation,
    film_lut: Option<std::sync::Arc<lut::Lut3D>>,
    color_chrome: bool,
    gamut_compress: bool,
    transfer: Transfer,
}

impl PointStage {
    fn new(recipe: &Recipe, transfer: Transfer) -> Self {
        let film_sim = film_sim::FilmSimulation::from_id(&recipe.film_simulation);
        let working_space = aces::WorkingColorSpace::from_str_name(&recipe.color_space);
        Self {
            wb: white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint),
            exp_factor: 2.0f32.powf(recipe.exposure),
            lut: resolve_recipe_lut(recipe),
            film_sim,
            film_lut: film_sim::calibrated_lut(film_sim),
            color_chrome: recipe.color_chrome > 0.0 || recipe.color_chrome_blue > 0.0,
            gamut_compress: working_space == aces::WorkingColorSpace::AcesCg || recipe.aces_tonemap,
            transfer,
        }
    }

    /// White balance, exposure, tone, mixer, skin, wheels, HSL, LUT, film simulation,
    /// Color Chrome and gamut compression for one linear 16-bit pixel.
    #[inline(always)]
    fn apply(&self, recipe: &Recipe, r: u16, g: u16, b: u16) -> (f32, f32, f32) {
        let (wb_r, wb_g, wb_b) = self.wb;
        // Scene-linear stage: white balance and exposure act on linear light,
        // then the buffer's display curve is applied once for the creative stage.
        let r = self.transfer.encode((r as f32 / 65535.0) * wb_r * self.exp_factor);
        let g = self.transfer.encode((g as f32 / 65535.0) * wb_g * self.exp_factor);
        let b = self.transfer.encode((b as f32 / 65535.0) * wb_b * self.exp_factor);

        let (r, g, b) = tone::apply_tone_pixel(r, g, b, recipe, 1.0);
        let (r, g, b) = rgb_mixer::apply_rgb_mixer(r, g, b, recipe);
        let (r, g, b) = skin_tone::apply_skin_tone_uniformity(r, g, b, recipe);
        let (r, g, b) = color_grading::apply_color_wheels(r, g, b, recipe);
        let (r, g, b) = color_grading::apply_hsl_mixer(r, g, b, recipe);
        let (r, g, b) = match &self.lut {
            Some(active_lut) => lut::apply_lut_pixel(r, g, b, active_lut, recipe.lut_intensity),
            None => (r, g, b),
        };
        let (r, g, b) = if self.film_sim != film_sim::FilmSimulation::None {
            film_sim::apply(self.film_sim, self.film_lut.as_deref(), r, g, b, recipe.film_sim_intensity)
        } else {
            (r, g, b)
        };
        let (r, g, b) = if self.color_chrome {
            film_sim::apply_color_chrome(r, g, b, recipe.color_chrome, recipe.color_chrome_blue)
        } else {
            (r, g, b)
        };
        if self.gamut_compress { aces::apply_aces_gamut_compression(r, g, b) } else { (r, g, b) }
    }
}

/// Where a buffer sits in the frame it belongs to: position-dependent effects (local
/// masks, grain, vignette, lens distortion) use frame coordinates, so a window of the
/// frame renders the same as that part of the whole.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub x0: usize,
    pub y0: usize,
    pub frame_w: usize,
    pub frame_h: usize,
}

impl Window {
    pub fn whole(w: usize, h: usize) -> Self {
        Self { x0: 0, y0: 0, frame_w: w, frame_h: h }
    }
}

/// Context the neighbourhood passes need around a region (see [`BAND_HALO`]).
pub const REGION_HALO: usize = BAND_HALO;

/// Renders part of a frame to 8-bit RGB, for the viewport's detail view.
///
/// `input` is 3-channel linear RGB, `w` x `h`, placed in a `window.frame_w` x
/// `window.frame_h` frame at (`window.x0`, `window.y0`). The output is the frame
/// rectangle `out` = (x, y, width, height). `input` must cover `out` plus
/// [`REGION_HALO`] pixels (and, with lens distortion, the area the distortion reads
/// from); see [`region_source`].
pub fn render_region_8(
    input: &[u16],
    w: usize,
    h: usize,
    window: Window,
    out: (usize, usize, usize, usize),
    recipe: &Recipe,
    transfer: Transfer,
) -> Vec<u8> {
    let stage = PointStage::new(recipe, transfer);
    let buf = process_band(input, w, 3, 0, h, window, recipe, &stage);
    let lens = detail::LensDistortion::new(window.frame_w, window.frame_h, recipe.lens_distortion);
    let (ox, oy, ow, oh) = out;
    let (fw, fh) = (window.frame_w as f32, window.frame_h as f32);
    let mut output = vec![0u8; ow * oh * 3];
    output.par_chunks_mut(ow * 3).enumerate().for_each(|(j, row)| {
        let gy = oy + j;
        for i in 0..ow {
            let gx = ox + i;
            let (sx, sy) = match &lens {
                Some(l) => l.source(gx as f32, gy as f32),
                None => (gx as f32, gy as f32),
            };
            let px = if sx >= 0.0 && sx < fw - 1.0 && sy >= 0.0 && sy < fh - 1.0 {
                // Source position inside this buffer; clamped at its edges
                let lx = (sx - window.x0 as f32).clamp(0.0, (w - 1) as f32);
                let ly = (sy - window.y0 as f32).clamp(0.0, (h - 1) as f32);
                let (x0, y0) = (lx.floor() as usize, ly.floor() as usize);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let (fx, fy) = (lx - x0 as f32, ly - y0 as f32);
                let at = |yy: usize, xx: usize, c: usize| buf[(yy * w + xx) * 3 + c];
                let sample = |c: usize| {
                    let top = at(y0, x0, c) * (1.0 - fx) + at(y0, x1, c) * fx;
                    let bottom = at(y1, x0, c) * (1.0 - fx) + at(y1, x1, c) * fx;
                    top * (1.0 - fy) + bottom * fy
                };
                (sample(0), sample(1), sample(2))
            } else {
                (0.0, 0.0, 0.0)
            };
            let (r, g, b) = detail::apply_defringe(px.0, px.1, px.2, recipe.defringe);
            let (r, g, b) =
                detail::apply_vignette(r, g, b, gx as u32, gy as u32, window.frame_w as u32, window.frame_h as u32, recipe.vignette);
            row[i * 3] = (r.clamp(0.0, 1.0) * 255.0) as u8;
            row[i * 3 + 1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
            row[i * 3 + 2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
        }
    });
    output
}

/// The frame rectangle (x, y, width, height) [`render_region_8`] needs as input to
/// render `out` of a `frame_w` x `frame_h` frame with `recipe`.
pub fn region_source(out: (usize, usize, usize, usize), frame_w: usize, frame_h: usize, recipe: &Recipe) -> (usize, usize, usize, usize) {
    let (ox, oy, ow, oh) = out;
    let (mut x0, mut y0, mut x1, mut y1) = (ox as f32, oy as f32, (ox + ow) as f32, (oy + oh) as f32);
    if let Some(lens) = detail::LensDistortion::new(frame_w, frame_h, recipe.lens_distortion) {
        // The distortion is radial and monotonic, so the border of the output maps to
        // the border of what it reads
        let steps = 32;
        for k in 0..=steps {
            let t = k as f32 / steps as f32;
            let (ex, ey) = (ox as f32 + t * ow as f32, oy as f32 + t * oh as f32);
            for (x, y) in [(ex, oy as f32), (ex, (oy + oh) as f32), (ox as f32, ey), ((ox + ow) as f32, ey)] {
                let (sx, sy) = lens.source(x, y);
                x0 = x0.min(sx);
                y0 = y0.min(sy);
                x1 = x1.max(sx);
                y1 = y1.max(sy);
            }
        }
    }
    let halo = REGION_HALO as f32 + 2.0;
    let x0 = (x0 - halo).floor().clamp(0.0, frame_w as f32) as usize;
    let y0 = (y0 - halo).floor().clamp(0.0, frame_h as f32) as usize;
    let x1 = (x1 + halo).ceil().clamp(0.0, frame_w as f32) as usize;
    let y1 = (y1 + halo).ceil().clamp(0.0, frame_h as f32) as usize;
    (x0, y0, x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1))
}

/// Passes 1 to 4 (point operations, local masks, presence, detail, grain) for rows
/// `y0..y1` of `input` (`w` pixels wide), which sits in the frame as `window` says. Rows near the band's edges only serve as context for the neighbourhood
/// passes; the caller keeps the rows at least [`BAND_HALO`] away from a cut.
#[allow(clippy::too_many_arguments)]
fn process_band(
    input: &[u16],
    w: usize,
    ch: usize,
    y0: usize,
    y1: usize,
    window: Window,
    recipe: &Recipe,
    stage: &PointStage,
) -> Vec<f32> {
    let rows = y1 - y0;
    let mut buf = vec![0.0f32; rows * w * 3];

    buf.par_chunks_mut(w * 3).enumerate().for_each(|(i, row)| {
        let y = y0 + i;
        let in_row = &input[y * w * ch..(y + 1) * w * ch];
        for x in 0..w {
            let (r, g, b) = stage.apply(recipe, in_row[x * ch], in_row[x * ch + 1], in_row[x * ch + 2]);
            row[x * 3] = r;
            row[x * 3 + 1] = g;
            row[x * 3 + 2] = b;
        }
    });

    // Local layered mask adjustments, positioned in whole-frame coordinates
    if !recipe.layers.is_empty() {
        let inv_w = 1.0 / (window.frame_w as f32).max(1.0);
        let inv_h = 1.0 / (window.frame_h as f32).max(1.0);
        buf.par_chunks_mut(w * 3).enumerate().for_each(|(i, row)| {
            let v = ((window.y0 + y0 + i) as f32 + 0.5) * inv_h;
            for x in 0..w {
                let u = ((window.x0 + x) as f32 + 0.5) * inv_w;
                let px = x * 3;
                let (lr, lg, lb) =
                    layers::apply_layers_pixel(row[px], row[px + 1], row[px + 2], &recipe.layers, u, v);
                row[px] = lr;
                row[px + 1] = lg;
                row[px + 2] = lb;
            }
        });
    }

    presence::apply_presence_ex(
        &mut buf,
        w,
        rows,
        recipe.clarity,
        recipe.texture,
        recipe.dehaze,
        recipe.midtone_detail,
    );
    detail::apply_detail(&mut buf, w, rows, recipe.sharpness, recipe.denoise_lum, recipe.denoise_col);
    if recipe.grain_amount > 0.001 {
        film_grain::apply_film_grain_window(
            &mut buf,
            w as u32,
            window.x0 as u32,
            (window.y0 + y0) as u32,
            recipe.grain_amount,
            recipe.grain_size,
            recipe.grain_roughness,
        );
    }
    buf
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
    process_split_comparison_16_to_8_ex(input, width, height, channels, recipe, Transfer::bt709(), split_ratio, false, false)
}

/// Extended Before / After split comparison buffer with clipping mask overlay support
pub fn process_split_comparison_16_to_8_ex(
    input: &[u16],
    width: u32,
    height: u32,
    channels: u32,
    recipe: &Recipe,
    transfer: Transfer,
    split_ratio: f32,
    highlight_mask: bool,
    shadow_mask: bool,
) -> (Vec<u8>, HistogramData) {
    let (processed, hist) = process_buffer_16_to_8_ex(input, width, height, channels, recipe, transfer, highlight_mask, shadow_mask);
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

            // The source is linear light; BEFORE gets the same single display curve the
            // AFTER path applies, so a neutral recipe looks the same on both halves.
            let to_8bit = |val16: u16| -> u8 { (transfer.encode(val16 as f32 / 65535.0).clamp(0.0, 1.0) * 255.0) as u8 };

            for x in 0..w {
                let px = x * ch;
                if x < split_x {
                    // BEFORE (original decoded RAW, same encoding as the AFTER half)
                    row[px] = to_8bit(orig_row[px]);
                    row[px + 1] = to_8bit(orig_row[px + 1]);
                    row[px + 2] = to_8bit(orig_row[px + 2]);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Banding must not change the result: every neighbourhood pass and the grain are on.
    #[test]
    fn banded_export_matches_single_piece() {
        let (w, h) = (97usize, 260usize);
        let input: Vec<u16> = (0..w * h * 3)
            .map(|i| {
                let (p, c) = (i / 3, i % 3);
                let (x, y) = (p % w, p / w);
                (((x * 37 + y * 53 + c * 101) % 251) as u16) * 200 + ((x ^ y) as u16 & 63) * 100
            })
            .collect();
        let mut recipe = Recipe {
            clarity: 40.0,
            texture: 30.0,
            dehaze: 20.0,
            midtone_detail: 25.0,
            sharpness: 50.0,
            denoise_lum: 30.0,
            denoise_col: 60.0,
            grain_amount: 20.0,
            vignette: -30.0,
            ..Default::default()
        };
        for lens in [0.0, 25.0] {
            recipe.lens_distortion = lens;
            let t = Transfer::bt709();
            let (whole, _) = process_16_to_16_banded(&input, w as u32, h as u32, 3, &recipe, t, h);
            let (banded, _) = process_16_to_16_banded(&input, w as u32, h as u32, 3, &recipe, t, 40);
            let worst = whole.iter().zip(&banded).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            // Rolling blur sums start at a different row, so float rounding can move a
            // value by one step (two with lens distortion's 15-bit intermediate)
            assert!(worst <= 2, "lens {lens}: max difference {worst}");
        }
    }

    /// A region rendered from its source window matches the same pixels of the whole frame.
    #[test]
    fn region_render_matches_whole_frame() {
        let (w, h) = (180usize, 140usize);
        let input: Vec<u16> = (0..w * h * 3)
            .map(|i| {
                let (p, c) = (i / 3, i % 3);
                let (x, y) = (p % w, p / w);
                (((x * 37 + y * 53 + c * 101) % 251) as u16) * 200 + ((x ^ y) as u16 & 63) * 100
            })
            .collect();
        let mut recipe = Recipe {
            clarity: 40.0,
            sharpness: 50.0,
            denoise_col: 60.0,
            grain_amount: 20.0,
            vignette: -30.0,
            ..Default::default()
        };
        let t = Transfer::bt709();
        for lens in [0.0, 25.0, -25.0] {
            recipe.lens_distortion = lens;
            let whole = render_region_8(&input, w, h, Window::whole(w, h), (0, 0, w, h), &recipe, t);
            let out = (110, 9, 50, 40);
            let (sx, sy, sw, sh) = region_source(out, w, h, &recipe);
            let crop: Vec<u16> = (sy..sy + sh).flat_map(|y| input[(y * w + sx) * 3..(y * w + sx + sw) * 3].to_vec()).collect();
            let window = Window { x0: sx, y0: sy, frame_w: w, frame_h: h };
            let part = render_region_8(&crop, sw, sh, window, out, &recipe, t);
            let mut worst = 0;
            for j in 0..out.3 {
                for i in 0..out.2 * 3 {
                    let a = whole[((out.1 + j) * w + out.0) * 3 + i];
                    worst = worst.max(a.abs_diff(part[j * out.2 * 3 + i]));
                }
            }
            assert!(worst <= 1, "lens {lens}: max difference {worst}");
        }
    }
}
