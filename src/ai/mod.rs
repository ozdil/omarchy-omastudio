use crate::raw::RawMetadata;
use crate::recipe::Recipe;
use serde::{Deserialize, Serialize};

pub mod jev;
pub use jev::{
    ai_classify_scene_with_jev, apply_jev_decisions_to_recipe, extract_photographic_state,
    JevClient, JevConfig, JevDecisions, JevStateInput,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAnalysis {
    pub scene_type: String,
    pub confidence: f32,
    pub recommended_preset: String,
    pub description: String,
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

    // 1. Calculate channel averages and luminance distribution
    let mut sum_r: f64 = 0.0;
    let mut sum_g: f64 = 0.0;
    let mut sum_b: f64 = 0.0;
    let mut luma_hist = [0u32; 256];

    for chunk in buffer.chunks_exact(ch) {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];
        let luma = ((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) as u32).min(255) as usize;

        sum_r += r as f64;
        sum_g += g as f64;
        sum_b += b as f64;
        luma_hist[luma] += 1;
    }

    let avg_r = (sum_r / total_pixels as f64) as f32;
    let avg_g = (sum_g / total_pixels as f64) as f32;
    let avg_b = (sum_b / total_pixels as f64) as f32;

    // 2. Percentile analysis (p5, p50, p95)
    let p5_idx = (total_pixels as f64 * 0.05) as u32;
    let p50_idx = (total_pixels as f64 * 0.50) as u32;
    let p95_idx = (total_pixels as f64 * 0.95) as u32;

    let mut accum = 0u32;
    let mut p5 = 0u8;
    let mut p50 = 128u8;
    let mut p95 = 255u8;

    for (val, &count) in luma_hist.iter().enumerate() {
        accum += count;
        if accum >= p5_idx && p5 == 0 {
            p5 = val as u8;
        }
        if accum >= p50_idx && p50 == 128 {
            p50 = val as u8;
        }
        if accum >= p95_idx {
            p95 = val as u8;
            break;
        }
    }

    // 3. AI Exposure calculation (Target midtone p50 around 118-125 for 18% photographic gray)
    let current_mid = (p50 as f32 / 255.0).max(0.02);
    let target_mid = 0.46; // ~118 sRGB
    let exposure_delta = (target_mid / current_mid).log2().clamp(-2.0, 2.0);
    recipe.exposure = (exposure_delta * 100.0).round() / 100.0;

    // 4. Dynamic range compensation
    if p95 > 240 {
        let blow_ratio = (p95 as f32 - 240.0) / 15.0;
        recipe.highlights = -(blow_ratio * 35.0).clamp(0.0, 50.0);
    }

    if p5 < 15 {
        let crush_ratio = (15.0 - p5 as f32) / 15.0;
        recipe.shadows = (crush_ratio * 30.0).clamp(0.0, 45.0);
    }

    let dynamic_range = p95 as f32 - p5 as f32;
    if dynamic_range < 140.0 {
        recipe.contrast = 15.0;
    } else if dynamic_range > 230.0 {
        recipe.contrast = -8.0;
    }

    // 5. AI White Balance Color Cast correction
    if avg_g > 1.0 {
        let r_to_g = avg_r / avg_g;
        let b_to_g = avg_b / avg_g;

        if b_to_g < 0.85 {
            recipe.wb_temperature = 4800.0;
        } else if b_to_g > 1.15 {
            recipe.wb_temperature = 6200.0;
        } else {
            recipe.wb_temperature = 5500.0;
        }

        let tint_shift = ((r_to_g + b_to_g) / 2.0 - 1.0) * 40.0;
        recipe.wb_tint = tint_shift.clamp(-25.0, 25.0);
    }

    // 6. Presence & Clarity
    recipe.vibrance = 15.0;
    recipe.clarity = 10.0;
    recipe.sharpness = 30.0;

    if meta.iso >= 1600.0 {
        let iso_factor = ((meta.iso - 1600.0) / 4800.0).clamp(0.0, 1.0);
        recipe.denoise_lum = 20.0 + (iso_factor * 30.0);
        recipe.denoise_col = 25.0 + (iso_factor * 25.0);

        // JEV-PHOTO-04: High-ISO shadow lift ceiling prevents amplified sensor chroma noise
        if meta.iso >= 3200.0 {
            recipe.shadows = recipe.shadows.min(25.0);
            recipe.denoise_col = (recipe.denoise_col + 15.0).min(80.0);
            recipe.contrast_pivot = 0.38; // Deeper analog contrast anchor
        }
    }

    // Optical aperture awareness: wide open apertures get subtle vignette correction
    if meta.aperture > 0.0 && meta.aperture <= 2.0 {
        recipe.vignette = 15.0; // Counteract natural optical vignetting
    }

    recipe.preset_name = Some("AI Auto Enhanced".to_string());
    recipe
}

/// AI Scene Classification: Analyzes color and tonal composition to detect scene type
pub fn ai_classify_scene(
    buffer: &[u8],
    _width: u32,
    _height: u32,
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

    let mut green_count = 0u32;
    let mut blue_count = 0u32;
    let mut warm_count = 0u32;
    let mut dark_count = 0u32;

    for chunk in buffer.chunks_exact(ch) {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        if g > r * 1.15 && g > b * 1.15 {
            green_count += 1;
        }
        if b > r * 1.15 && b > g * 1.1 {
            blue_count += 1;
        }
        if r > 150.0 && g > 100.0 && b < 100.0 {
            warm_count += 1;
        }
        if luma < 35.0 {
            dark_count += 1;
        }
    }

    let total = total_pixels as f32;
    let green_ratio = green_count as f32 / total;
    let blue_ratio = blue_count as f32 / total;
    let warm_ratio = warm_count as f32 / total;
    let dark_ratio = dark_count as f32 / total;

    if meta.iso >= 3200.0 || dark_ratio > 0.45 {
        SceneAnalysis {
            scene_type: "Night / Low-Light".to_string(),
            confidence: 0.88,
            recommended_preset: "Cinematic Moody".to_string(),
            description: "Low-light scene with deep shadows. Recommended: noise suppression and moody tones.".to_string(),
        }
    } else if warm_ratio > 0.25 {
        SceneAnalysis {
            scene_type: "Golden Hour / Sunset".to_string(),
            confidence: 0.92,
            recommended_preset: "Kodak Portra 400".to_string(),
            description: "Warm golden sunlight detected. Recommended: Portra warm glow and soft contrast.".to_string(),
        }
    } else if green_ratio > 0.20 || blue_ratio > 0.22 {
        SceneAnalysis {
            scene_type: "Landscape / Nature".to_string(),
            confidence: 0.90,
            recommended_preset: "Fuji Velvia 50".to_string(),
            description: "Natural greenery or sky detected. Recommended: Velvia vivid landscape colors.".to_string(),
        }
    } else if warm_ratio > 0.12 && green_ratio < 0.10 {
        SceneAnalysis {
            scene_type: "Portrait".to_string(),
            confidence: 0.84,
            recommended_preset: "Kodak Portra 400".to_string(),
            description: "Human subject skin tones detected. Recommended: Soft contrast and gentle clarity.".to_string(),
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
/// smart subject-centered crop, mobile sharpness, and sRGB color specs for Instagram, X, Facebook, YouTube, etc.
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

    // 2. Intelligent Saliency / Subject Center of Mass Detection
    let ch = channels as usize;
    let mut center_x = 0.5f32;
    let mut center_y = 0.5f32;

    if width > 10 && height > 10 && buffer.len() >= (width * height * channels) as usize {
        let step = ((width * height) / 1000).max(1) as usize;
        let mut sum_weight = 0.0f32;
        let mut weighted_x = 0.0f32;
        let mut weighted_y = 0.0f32;

        for (i, chunk) in buffer.chunks_exact(ch).step_by(step).enumerate() {
            let px_idx = (i * step) as u32;
            let x = (px_idx % width) as f32 / width as f32;
            let y = (px_idx / width) as f32 / height as f32;

            let r = chunk[0] as f32;
            let g = chunk[1] as f32;
            let b = chunk[2] as f32;
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

            // Warm skin tones and high-contrast midtones get higher saliency
            let skin_weight = if r > 120.0 && g > 70.0 && b < 160.0 && r > g && g > b { 2.5 } else { 1.0 };
            let contrast_weight = 1.0 + ((luma - 128.0).abs() / 128.0) * 0.5;
            let weight = skin_weight * contrast_weight;

            weighted_x += x * weight;
            weighted_y += y * weight;
            sum_weight += weight;
        }

        if sum_weight > 0.0 {
            center_x = (weighted_x / sum_weight).clamp(0.2, 0.8);
            center_y = (weighted_y / sum_weight).clamp(0.2, 0.8);
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
