use crate::recipe::Recipe;

/// Luma-guided Highlight Reconstruction (Inpainting):
/// Repairs clipped color channels (e.g. green or blue clipping ahead of red)
/// using chromatic gradient preservation from unclipped channels.
#[inline(always)]
pub fn reconstruct_clipped_highlights(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let clip_thresh = 0.94;
    let r_clip = r > clip_thresh;
    let g_clip = g > clip_thresh;
    let b_clip = b > clip_thresh;

    if !r_clip && !g_clip && !b_clip {
        return (r, g, b);
    }

    // If 1 or 2 channels clip, estimate the missing energy from the unclipped channel(s)
    let max_val = r.max(g).max(b);
    let min_val = r.min(g).min(b);
    let unclipped_luma = 0.2126 * (if r_clip { min_val } else { r })
                       + 0.7152 * (if g_clip { min_val } else { g })
                       + 0.0722 * (if b_clip { min_val } else { b });

    let blend_weight = ((max_val - clip_thresh) / (1.20 - clip_thresh)).clamp(0.0, 1.0);
    let desat = blend_weight * 0.45;

    let target_energy = max_val.max(unclipped_luma * 1.15);

    let r_out = if r_clip { r.max(target_energy) * (1.0 - desat) + target_energy * desat } else { r };
    let g_out = if g_clip { g.max(target_energy) * (1.0 - desat) + target_energy * desat } else { g };
    let b_out = if b_clip { b.max(target_energy) * (1.0 - desat) + target_energy * desat } else { b };

    (r_out, g_out, b_out)
}

/// AgX / Filmic Sigmoidal Dynamic Range Tone Curve
/// Provides smooth photochemical highlight shoulder and deep shadow toe without hue shifts
#[inline(always)]
pub fn apply_agx_filmic_curve(val: f32) -> f32 {
    if val <= 0.0 {
        return 0.0;
    }
    // Logarithmic encoding
    let min_ev = -10.0f32;
    let max_ev = 4.0f32;
    let log_val = (val.log2().clamp(min_ev, max_ev) - min_ev) / (max_ev - min_ev);

    // Sigmoidal S-Curve: f(x) = x^2 * (3 - 2x) with filmic shoulder extension
    let s = log_val * log_val * (3.0 - 2.0 * log_val);
    s.clamp(0.0, 1.0)
}

/// Applies photometric exposure, dynamic range tone adjustments, and presence
#[inline(always)]
pub fn apply_tone_pixel(
    mut r: f32,
    mut g: f32,
    mut b: f32,
    recipe: &Recipe,
    exp_factor: f32,
) -> (f32, f32, f32) {
    // 0. SOTA Highlight Reconstruction (Clipped channel inpainting)
    if recipe.highlight_reconstruct {
        let (rc, gc, bc) = reconstruct_clipped_highlights(r, g, b);
        r = rc;
        g = gc;
        b = bc;
    }

    // 1. Photometric Exposure: I_out = I_in * 2^EV
    r *= exp_factor;
    g *= exp_factor;
    b *= exp_factor;

    // Calculate luminance
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if luma <= 0.0 {
        return (0.0, 0.0, 0.0);
    }

    let mut luma_adj = luma;

    // 2. Highlights recovery / compression
    // Smooth quadratic rolloff in the top 65% of luminance
    if recipe.highlights != 0.0 {
        let h_norm = ((luma - 0.35).max(0.0) / 0.65).min(1.0);
        let h_weight = h_norm * h_norm * (3.0 - 2.0 * h_norm); // Smoothstep
        let h_adj = (recipe.highlights / 100.0) * h_weight * 0.35;
        luma_adj += h_adj;
    }

    // 3. Shadows lift / crush
    // Natural photographic toe response:
    // - Anchors true black (0.0 remains 0.0, avoiding milky washed-out blacks or lifted noise)
    // - Delivers maximum lift in the shadow detail region (0.05 to 0.25)
    // - Smoothly tapers to 0 at midtones (0.60)
    if recipe.shadows != 0.0 {
        let s_norm = (luma / 0.60).clamp(0.0, 1.0);
        let falloff = (1.0 - s_norm) * (1.0 - s_norm);
        let toe = (luma / (luma + 0.03)).clamp(0.0, 1.0);
        let s_weight = falloff * toe;
        let s_adj = (recipe.shadows / 100.0) * s_weight * 0.35;
        luma_adj += s_adj;
    }

    // 4. Whites & Blacks anchor points
    if recipe.whites != 0.0 {
        let w_norm = ((luma - 0.60).max(0.0) / 0.40).min(1.0);
        let w_weight = w_norm * w_norm;
        luma_adj += (recipe.whites / 100.0) * w_weight * 0.25;
    }
    if recipe.blacks != 0.0 {
        let b_norm = ((0.30 - luma).max(0.0) / 0.30).min(1.0);
        let b_weight = b_norm * b_norm;
        luma_adj += (recipe.blacks / 100.0) * b_weight * 0.20;
    }

    // 5. Contrast (S-curve centered around DaVinci Pivot point)
    if recipe.contrast != 0.0 {
        let c = recipe.contrast / 100.0;
        let pivot = recipe.contrast_pivot.clamp(0.05, 0.95);
        let diff = luma_adj - pivot;
        luma_adj = pivot + diff * (1.0 + c * 0.5) + (diff * diff * diff) * c * 0.3;
    }

    // Tone Curve 4-zone parametric offsets
    if recipe.curve_highlights != 0.0 || recipe.curve_lights != 0.0 || recipe.curve_darks != 0.0 || recipe.curve_shadows != 0.0 {
        let ch = (recipe.curve_highlights / 100.0) * (luma_adj - 0.75).max(0.0) / 0.25;
        let cl = (recipe.curve_lights / 100.0) * ((luma_adj - 0.5).max(0.0) * (0.75 - luma_adj).max(0.0) * 4.0);
        let cd = (recipe.curve_darks / 100.0) * ((luma_adj - 0.25).max(0.0) * (0.5 - luma_adj).max(0.0) * 4.0);
        let cs = (recipe.curve_shadows / 100.0) * (0.25 - luma_adj).max(0.0) / 0.25;
        luma_adj += (ch + cl + cd + cs) * 0.25;
    }

    luma_adj = luma_adj.clamp(0.0, 1.5);

    // Apply AgX Filmic Tone Curve if requested
    if recipe.filmic_agx {
        luma_adj = apply_agx_filmic_curve(luma_adj);
    }

    // =========================================================================
    // COLOR-PRESERVING LUMINANCE ADJUSTMENT (Stevens / Hunt Perceptual Constancy)
    // Decomposes into luminance and chrominance vectors (cr, cg, cb).
    // Instead of naive multiplicative scaling (r *= luma_adj / luma) which multiplies
    // chromaticity by up to 50x and causes extreme neon oversaturation / clipping,
    // chroma scales gently with the 4th-root of luminance gain.
    // =========================================================================
    let ratio = if luma > 1e-5 { luma_adj / luma } else { 1.0 };

    let chroma_scale = if ratio > 1.0 {
        // Shadows lift: gentle perceptual colorfulness scaling (capped at 1.35x)
        ratio.powf(0.20).min(1.35)
    } else {
        // Highlights compression: preserve sky/window color saturation
        ratio.powf(0.40)
    };

    let cr = r - luma;
    let cg = g - luma;
    let cb = b - luma;

    let mut r_adj = luma_adj + cr * chroma_scale;
    let mut g_adj = luma_adj + cg * chroma_scale;
    let mut b_adj = luma_adj + cb * chroma_scale;

    // Soft highlight gamut roll-off: desaturate gently towards white near clipping
    // preventing hard single-channel clipping from causing harsh neon hue shifts.
    let max_comp = r_adj.max(g_adj).max(b_adj);
    if max_comp > 0.88 && luma_adj > 0.65 {
        let excess = ((max_comp - 0.88) / 0.30).min(1.0);
        let desat = excess * 0.35;
        r_adj = r_adj * (1.0 - desat) + luma_adj * desat;
        g_adj = g_adj * (1.0 - desat) + luma_adj * desat;
        b_adj = b_adj * (1.0 - desat) + luma_adj * desat;
    }

    // 6. Presence: Vibrance, DaVinci Color Boost and Saturation
    let max_c = r_adj.max(g_adj).max(b_adj);
    let min_c = r_adj.min(g_adj).min(b_adj);
    let current_sat = if max_c > 0.0001 { (max_c - min_c) / max_c } else { 0.0 };

    let mut sat_delta = recipe.saturation / 100.0;

    // Vibrance boosts less saturated colors more, protecting skin tones
    if recipe.vibrance != 0.0 {
        let vib_factor = (1.0 - current_sat) * (recipe.vibrance / 100.0);
        sat_delta += vib_factor;
    }

    // DaVinci Resolve Color Boost: Natural perceptual boost prioritizing desaturated tones
    if recipe.color_boost != 0.0 {
        let boost_weight = (1.0 - current_sat).powf(1.4);
        let boost_delta = (recipe.color_boost / 100.0) * boost_weight;
        sat_delta += boost_delta;
    }

    if sat_delta != 0.0 {
        let cur_luma = 0.2126 * r_adj + 0.7152 * g_adj + 0.0722 * b_adj;
        let sat_mult = (1.0 + sat_delta).max(0.0);
        r_adj = cur_luma + (r_adj - cur_luma) * sat_mult;
        g_adj = cur_luma + (g_adj - cur_luma) * sat_mult;
        b_adj = cur_luma + (b_adj - cur_luma) * sat_mult;
    }

    (r_adj, g_adj, b_adj)
}
