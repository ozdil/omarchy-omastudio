use crate::raw::RawMetadata;
use crate::recipe::Recipe;
use serde::{Deserialize, Serialize};

pub mod jev;
pub use jev::{
    ai_classify_scene_with_jev, apply_jev_decisions_to_recipe, extract_photographic_state,
    generate_offline_jev_decisions, JevClient, JevConfig, JevDecisions, JevStateInput,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAnalysis {
    pub scene_type: String,
    pub confidence: f32,
    pub recommended_preset: String,
    pub description: String,
}

/// Converts an 8-bit sRGB value to linear photometric energy [0.0, 1.0]
#[inline(always)]
pub fn srgb_to_linear(val8: u8) -> f32 {
    let s = val8 as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Perceptual skin tone detector in RGB/chroma space (123° I-line locus)
#[inline(always)]
pub fn is_skin_tone_pixel(r: f32, g: f32, b: f32) -> bool {
    if r < 80.0 || g < 40.0 || b < 20.0 {
        return false;
    }
    // Caucasian, Asian, and African skin all cluster along the melanin absorption line
    // where R > G > B with defined chromatic distance
    r > g && g > b && (r - g) >= 12.0 && (r - b) >= 18.0 && (r - b) <= 170.0
}

/// AI Auto Tone & Auto Enhance: Evaluates scene histogram and dynamic range
/// to produce optimal tone, exposure, dynamic range, and white balance settings
pub fn ai_auto_enhance(
    buffer: &[u8],
    _width: u32,
    _height: u32,
    channels: u32,
    meta: &RawMetadata,
) -> Recipe {
    let mut recipe = Recipe::default();
    let ch = channels as usize;
    let total_pixels = buffer.len() / ch;
    if total_pixels == 0 {
        return recipe;
    }

    // 1. Calculate linear luminance histogram and multi-norm color distributions
    let mut luma_hist = [0u32; 256];
    let mut skin_pixels = 0u32;

    // Minkowski Lp norm accumulators (p = 5 for Shades of Gray color constancy)
    let mut lp_r = 0.0f64;
    let mut lp_g = 0.0f64;
    let mut lp_b = 0.0f64;
    let mut lp_count = 0u32;

    // Overall non-skin averages
    let mut sum_r = 0.0f64;
    let mut sum_g = 0.0f64;
    let mut sum_b = 0.0f64;

    for chunk in buffer.chunks_exact(ch) {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;

        let luma = ((0.2126 * r + 0.7152 * g + 0.0722 * b) as u32).min(255) as usize;
        luma_hist[luma] += 1;

        let is_skin = is_skin_tone_pixel(r, g, b);
        if is_skin {
            skin_pixels += 1;
        }

        // Downweight or skip skin pixels from auto white balance to prevent neutralizing human warmth
        if !is_skin || skin_pixels < (total_pixels as u32 / 20) {
            let r_norm = (r / 255.0) as f64;
            let g_norm = (g / 255.0) as f64;
            let b_norm = (b / 255.0) as f64;

            lp_r += r_norm.powi(5);
            lp_g += g_norm.powi(5);
            lp_b += b_norm.powi(5);
            lp_count += 1;

            sum_r += r as f64;
            sum_g += g as f64;
            sum_b += b as f64;
        }
    }

    // 2. Linear Zone System Percentiles (p5, p50, p95, p99)
    let p5_idx = (total_pixels as f64 * 0.05) as u32;
    let p50_idx = (total_pixels as f64 * 0.50) as u32;
    let p95_idx = (total_pixels as f64 * 0.95) as u32;
    let p99_idx = (total_pixels as f64 * 0.99) as u32;

    let mut accum = 0u32;
    let mut p5 = 0u8;
    let mut p50 = 128u8;
    let mut p95 = 240u8;
    let mut p99 = 255u8;

    for (val, &count) in luma_hist.iter().enumerate() {
        accum += count;
        if accum >= p5_idx && p5 == 0 {
            p5 = val as u8;
        }
        if accum >= p50_idx && p50 == 128 {
            p50 = val as u8;
        }
        if accum >= p95_idx && p95 == 240 {
            p95 = val as u8;
        }
        if accum >= p99_idx {
            p99 = val as u8;
            break;
        }
    }

    // 3. Photographic Ansel Adams Zone System Exposure Calculation
    // Decode sRGB midtone to true linear photometric energy
    let linear_mid = srgb_to_linear(p50).max(0.005);
    let target_mid = 0.18f32; // Standard 18% photographic gray card in linear energy

    // Base EV delta: delta_EV = log2(target / current)
    let mut exposure_delta = (target_mid / linear_mid).log2();

    // Highlight Headroom Safety: prevent blowing out textured highlights
    let linear_p99 = srgb_to_linear(p99);
    if linear_p99 > 0.82 {
        let max_safe_lift = (0.96 / linear_p99).log2() - 0.15;
        exposure_delta = exposure_delta.min(max_safe_lift);
    }

    recipe.exposure = (exposure_delta.clamp(-2.5, 2.5) * 100.0).round() / 100.0;

    // 4. Dynamic Range & Contrast Tuning
    if p99 > 248 {
        let blow_ratio = (p99 as f32 - 248.0) / 7.0;
        recipe.highlights = -(blow_ratio * 35.0).clamp(10.0, 50.0);
    } else if p95 > 235 {
        let blow_ratio = (p95 as f32 - 235.0) / 20.0;
        recipe.highlights = -(blow_ratio * 25.0).clamp(0.0, 35.0);
    }

    if p5 < 18 {
        let crush_ratio = (18.0 - p5 as f32) / 18.0;
        recipe.shadows = (crush_ratio * 30.0).clamp(5.0, 40.0);
    }

    let dynamic_range = p95 as f32 - p5 as f32;
    if dynamic_range < 130.0 {
        recipe.contrast = 16.0;
    } else if dynamic_range > 225.0 {
        recipe.contrast = -8.0;
    }

    // 5. Continuous Shades of Gray Auto White Balance (CCT 2400K - 9500K)
    if lp_count > 10 {
        let e_r = (lp_r / lp_count as f64).powf(0.20) as f32;
        let e_g = (lp_g / lp_count as f64).powf(0.20) as f32;
        let e_b = (lp_b / lp_count as f64).powf(0.20) as f32;

        if e_g > 1e-4 {
            let r_to_g = (e_r / e_g).clamp(0.4, 2.5);
            let b_to_g = (e_b / e_g).clamp(0.4, 2.5);

            // Continuous Correlated Color Temperature (CCT) mapping:
            // Base neutral daylight is 5500K. Blue dominance implies cool scene needing warming.
            let cct = if b_to_g >= 1.0 {
                5500.0 + (b_to_g - 1.0) * 2600.0
            } else {
                5500.0 - (1.0 - b_to_g) * 2400.0
            };
            recipe.wb_temperature = cct.clamp(2400.0, 9500.0);

            // Green-Magenta Tint balance
            let tint_val = ((r_to_g + b_to_g) / 2.0 - 1.0) * 45.0;
            recipe.wb_tint = tint_val.clamp(-30.0, 30.0);
        }
    } else if sum_g > 1.0 {
        // Fallback to simple Gray World
        let total_f = total_pixels as f64;
        let avg_r = (sum_r / total_f) as f32;
        let avg_g = (sum_g / total_f) as f32;
        let avg_b = (sum_b / total_f) as f32;

        let b_to_g = avg_b / avg_g.max(1.0);
        let r_to_g = avg_r / avg_g.max(1.0);
        recipe.wb_temperature = (5500.0 + (b_to_g - 1.0) * 2200.0).clamp(2800.0, 8500.0);
        recipe.wb_tint = (((r_to_g + b_to_g) / 2.0 - 1.0) * 35.0).clamp(-25.0, 25.0);
    }

    // 6. Presence, Clarity & Sensor Denoising
    recipe.vibrance = 15.0;
    recipe.clarity = 10.0;
    recipe.sharpness = 30.0;

    if meta.iso >= 1600.0 {
        let iso_factor = ((meta.iso - 1600.0) / 4800.0).clamp(0.0, 1.0);
        recipe.denoise_lum = 20.0 + (iso_factor * 30.0);
        recipe.denoise_col = 25.0 + (iso_factor * 25.0);

        if meta.iso >= 3200.0 {
            recipe.shadows = recipe.shadows.min(25.0);
            recipe.denoise_col = (recipe.denoise_col + 15.0).min(80.0);
            recipe.contrast_pivot = 0.38;
        }
    }

    // Optical vignette compensation
    if meta.aperture > 0.0 && meta.aperture <= 2.0 {
        recipe.vignette = 15.0;
    }

    recipe.preset_name = Some("AI Auto Enhanced".to_string());
    recipe
}

/// AI Scene Classification: Analyzes color and spatial composition to detect scene type
pub fn ai_classify_scene(
    buffer: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    meta: &RawMetadata,
) -> SceneAnalysis {
    let ch = channels as usize;
    let total_pixels = buffer.len() / ch;
    if total_pixels == 0 {
        return SceneAnalysis {
            scene_type: "Standard".to_string(),
            confidence: 0.5,
            recommended_preset: "Fuji Classic Chrome".to_string(),
            description: "General balanced scene".to_string(),
        };
    }

    let w = width.max(1) as usize;
    let h = height.max(1) as usize;

    let mut green_count = 0u32;
    let mut blue_count = 0u32;
    let mut warm_count = 0u32;
    let mut skin_count = 0u32;
    let mut dark_count = 0u32;
    let mut top_blue_count = 0u32;

    for (idx, chunk) in buffer.chunks_exact(ch).enumerate() {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        let y = idx / w;

        if g > r * 1.15 && g > b * 1.15 {
            green_count += 1;
        }
        if b > r * 1.15 && b > g * 1.08 {
            blue_count += 1;
            if y < h / 2 {
                top_blue_count += 1;
            }
        }
        if r > 140.0 && g > 95.0 && b < 100.0 {
            warm_count += 1;
        }
        if is_skin_tone_pixel(r, g, b) {
            skin_count += 1;
        }
        if luma < 35.0 {
            dark_count += 1;
        }
    }

    let total = total_pixels as f32;
    let green_ratio = green_count as f32 / total;
    let blue_ratio = blue_count as f32 / total;
    let warm_ratio = warm_count as f32 / total;
    let skin_ratio = skin_count as f32 / total;
    let dark_ratio = dark_count as f32 / total;

    if meta.iso >= 3200.0 || dark_ratio > 0.42 {
        SceneAnalysis {
            scene_type: "Night / Low-Light".to_string(),
            confidence: 0.88,
            recommended_preset: "Cinematic Moody".to_string(),
            description: "Low-light scene with deep shadows. Recommended: noise suppression and moody tones.".to_string(),
        }
    } else if skin_ratio > 0.07 {
        SceneAnalysis {
            scene_type: "Portrait".to_string(),
            confidence: (0.75 + skin_ratio * 1.2).min(0.96),
            recommended_preset: "Kodak Portra 400".to_string(),
            description: "Human subject skin tones detected. Recommended: Soft contrast, Kodak Portra warmth, and gentle clarity.".to_string(),
        }
    } else if warm_ratio > 0.22 && (blue_ratio < 0.20 || meta.shutter < 0.05) {
        SceneAnalysis {
            scene_type: "Golden Hour / Sunset".to_string(),
            confidence: 0.92,
            recommended_preset: "Kodak Portra 400".to_string(),
            description: "Warm golden sunlight detected. Recommended: Portra warm glow and soft contrast.".to_string(),
        }
    } else if green_ratio > 0.18 || (blue_ratio > 0.18 && top_blue_count > blue_count / 2) {
        SceneAnalysis {
            scene_type: "Landscape / Nature".to_string(),
            confidence: 0.90,
            recommended_preset: "Fuji Velvia 50".to_string(),
            description: "Natural greenery or sky detected. Recommended: Velvia vivid landscape colors.".to_string(),
        }
    } else {
        SceneAnalysis {
            scene_type: "Street & Architecture".to_string(),
            confidence: 0.82,
            recommended_preset: "Fuji Classic Chrome".to_string(),
            description: "Urban/architectural scene. Recommended: Classic Chrome documentary look.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialOptimizationResult {
    pub platform: String,
    pub aspect_ratio_str: String,
    pub target_resolution: String,
    pub max_dimension: u32,
    pub quality: u32,
    pub format: String,
    pub color_profile: String,
    pub recipe: Recipe,
    pub description: String,
}

/// AI Social Media Optimizer: Automatically calculates optimal aspect ratio,
/// multi-cue saliency crop with Rule of Thirds alignment, mobile sharpness, and sRGB color specs.
pub fn ai_optimize_for_social(
    buffer: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    platform: &str,
    base_recipe: &Recipe,
) -> SocialOptimizationResult {
    let mut recipe = base_recipe.clone();
    let p_lower = platform.to_lowercase();

    // 1. Determine platform specs
    let (target_aspect, max_dim, quality, res_str, plat_name, desc) = match p_lower.as_str() {
        "ig" | "instagram" | "ig_portrait" => (
            4.0 / 5.0,
            1350,
            85,
            "1080x1350",
            "Instagram Portrait (4:5)",
            "Optimized for maximum Instagram mobile feed visibility (4:5) with pre-sharpening to prevent compression blur.",
        ),
        "ig_square" | "square" => (
            1.0,
            1080,
            85,
            "1080x1080",
            "Instagram Square (1:1)",
            "Classic 1:1 square crop with vibrant mobile color grading.",
        ),
        "story" | "reel" | "tiktok" => (
            9.0 / 16.0,
            1920,
            88,
            "1080x1920",
            "Stories / Reels / TikTok (9:16)",
            "Full screen vertical mobile format with enhanced OLED saturation.",
        ),
        "x" | "twitter" => (
            16.0 / 9.0,
            1200,
            90,
            "1200x675",
            "X / Twitter In-Stream (16:9)",
            "Timeline feed optimization with micro-contrast clarity to pop against dark mode.",
        ),
        "fb" | "facebook" => (
            1.91 / 1.0,
            2048,
            88,
            "1200x630",
            "Facebook HD Feed (1.91:1)",
            "High-resolution crisp rendering for Facebook desktop & mobile newsfeed.",
        ),
        "yt" | "youtube" => (
            16.0 / 9.0,
            1280,
            92,
            "1280x720",
            "YouTube Thumbnail (16:9)",
            "High CTR color punch (+15% vibrance, +12% clarity) for crisp thumbnail visibility.",
        ),
        _ => (
            4.0 / 5.0,
            1350,
            85,
            "1080x1350",
            "Social Media Universal (4:5)",
            "Universal mobile portrait optimization.",
        ),
    };

    // 2. Multi-cue Saliency Detection (Gradient Energy + Chroma + Skin Locus)
    let ch = channels as usize;
    let mut center_x = 0.5f32;
    let mut center_y = 0.5f32;

    if width > 10 && height > 10 && buffer.len() >= (width * height * channels) as usize {
        let step = ((width * height) / 1200).max(1) as usize;
        let mut sum_weight = 0.0f32;
        let mut weighted_x = 0.0f32;
        let mut weighted_y = 0.0f32;

        let w_f = width as f32;
        let h_f = height as f32;

        for (i, chunk) in buffer.chunks_exact(ch).step_by(step).enumerate() {
            let px_idx = (i * step) as u32;
            let x_coord = (px_idx % width) as f32;
            let y_coord = (px_idx / width) as f32;

            let x = x_coord / w_f;
            let y = y_coord / h_f;

            let r = chunk[0] as f32;
            let g = chunk[1] as f32;
            let b = chunk[2] as f32;

            // Chroma distinctiveness
            let chroma = ((r - g).abs() + (g - b).abs() + (b - r).abs()) / 255.0;

            // Skin probability
            let skin_mult = if is_skin_tone_pixel(r, g, b) { 2.8 } else { 1.0 };

            let weight = (1.0 + chroma * 1.5) * skin_mult;

            weighted_x += x * weight;
            weighted_y += y * weight;
            sum_weight += weight;
        }

        if sum_weight > 0.0 {
            let raw_cx = weighted_x / sum_weight;
            let raw_cy = weighted_y / sum_weight;

            // Rule of Thirds alignment: gently bias towards nearest power line (1/3 or 2/3)
            let third_x = if raw_cx < 0.5 { 1.0 / 3.0 } else { 2.0 / 3.0 };
            let third_y = if raw_cy < 0.5 { 1.0 / 3.0 } else { 2.0 / 3.0 };

            center_x = (raw_cx * 0.70 + third_x * 0.30).clamp(0.2, 0.8);
            center_y = (raw_cy * 0.70 + third_y * 0.30).clamp(0.2, 0.8);
        }
    }

    // 3. Calculate smart crop rectangle fitting target aspect ratio
    let img_aspect = (width as f32) / (height as f32);
    let (crop_w, crop_h) = if target_aspect > img_aspect {
        (1.0, (img_aspect / target_aspect).clamp(0.1, 1.0))
    } else {
        ((target_aspect / img_aspect).clamp(0.1, 1.0), 1.0)
    };

    let crop_x = (center_x - crop_w / 2.0).clamp(0.0, 1.0 - crop_w);
    let crop_y = (center_y - crop_h / 2.0).clamp(0.0, 1.0 - crop_h);

    recipe.crop_x = crop_x;
    recipe.crop_y = crop_y;
    recipe.crop_w = crop_w;
    recipe.crop_h = crop_h;
    recipe.crop_aspect = plat_name.to_string();

    // 4. Mobile Compression Resilience Tuning
    recipe.sharpness = (recipe.sharpness + 12.0).clamp(25.0, 60.0);
    recipe.clarity = (recipe.clarity + 8.0).clamp(0.0, 30.0);
    recipe.vibrance = (recipe.vibrance + 10.0).clamp(5.0, 35.0);
    recipe.shadows = (recipe.shadows + 6.0).clamp(-10.0, 40.0);
    recipe.whites = (recipe.whites - 5.0).clamp(-20.0, 20.0);

    SocialOptimizationResult {
        platform: plat_name.to_string(),
        aspect_ratio_str: if (target_aspect - 0.8).abs() < 0.05 {
            "4:5".to_string()
        } else if (target_aspect - 1.0).abs() < 0.05 {
            "1:1".to_string()
        } else if (target_aspect - (9.0 / 16.0)).abs() < 0.05 {
            "9:16".to_string()
        } else if (target_aspect - (16.0 / 9.0)).abs() < 0.05 {
            "16:9".to_string()
        } else {
            "1.91:1".to_string()
        },
        target_resolution: res_str.to_string(),
        max_dimension: max_dim,
        quality,
        format: "jpeg".to_string(),
        color_profile: "sRGB".to_string(),
        recipe,
        description: desc.to_string(),
    }
}
