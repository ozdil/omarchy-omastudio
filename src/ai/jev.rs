use crate::ai::SceneAnalysis;
use crate::raw::RawMetadata;
use crate::recipe::Recipe;
use crate::security::{run_bounded_command_with_stdin, secure_command, SecureDir};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for TypeSafe AI Jev Decision Engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevConfig {
    pub provider: String,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub timeout_secs: u64,
    pub enabled: bool,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            provider: "typesafe".to_string(),
            api_key: String::new(),
            endpoint: "https://api.typesafe.ai/v1/systemone".to_string(),
            model: "jev-latest".to_string(),
            timeout_secs: 5,
            enabled: true,
        }
    }
}

/// Structured decisions returned by Jev System 1
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevDecisions {
    pub scene_type: String,
    pub confidence: f32,
    pub recommended_preset: String,
    pub needs_highlight_recovery: bool,
    pub highlight_probability: f32,
    pub needs_shadow_lift: bool,
    pub shadow_probability: f32,
    pub recommended_contrast: String,
    pub aesthetic_score: f32,
    pub source: String,
}

/// Photographic metrics computed locally and supplied to Jev as state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevStateInput {
    pub camera_make: String,
    pub camera_model: String,
    pub lens_model: String,
    pub iso: f32,
    pub shutter_speed: f64,
    pub aperture: f32,
    pub focal_length: f32,
    pub width: u32,
    pub height: u32,
    pub p5_luma: u8,
    pub p50_luma: u8,
    pub p95_luma: u8,
    pub dynamic_range: f32,
    pub warm_ratio: f32,
    pub green_ratio: f32,
    pub blue_ratio: f32,
    pub dark_ratio: f32,
    pub avg_r: f32,
    pub avg_g: f32,
    pub avg_b: f32,
}

impl JevStateInput {
    /// Formats the state into a concise, token-efficient text representation
    /// containing strictly photographic metrics without leaking any raw pixel data.
    pub fn to_compact_state_string(&self) -> String {
        format!(
            "Camera: {} {} | Lens: {} | Exposure: ISO {}, f/{:.1}, 1/{:.0}s, {:.0}mm | \
             Dimensions: {}x{} | Luma Profile: P5={}, P50={}, P95={} (Dynamic Range: {:.0}/255) | \
             Color Distribution: Warm={:.1}%, Green={:.1}%, Blue={:.1}%, Dark={:.1}% | \
             Channel Means: R={:.1}, G={:.1}, B={:.1}",
            self.camera_make,
            self.camera_model,
            self.lens_model,
            self.iso,
            self.aperture,
            if self.shutter_speed > 0.0 { 1.0 / self.shutter_speed } else { 0.0 },
            self.focal_length,
            self.width,
            self.height,
            self.p5_luma,
            self.p50_luma,
            self.p95_luma,
            self.dynamic_range,
            self.warm_ratio * 100.0,
            self.green_ratio * 100.0,
            self.blue_ratio * 100.0,
            self.dark_ratio * 100.0,
            self.avg_r,
            self.avg_g,
            self.avg_b
        )
    }
}

/// Client for interacting with TypeSafe AI Jev System 1
pub struct JevClient {
    config: JevConfig,
}

impl JevClient {
    pub fn new(config: JevConfig) -> Self {
        Self { config }
    }

    /// Loads configuration from environment variables or ~/.config/omastudio/ai.json
    pub fn load_default_config() -> JevConfig {
        let mut config = JevConfig::default();

        // 1. Try reading ~/.config/omastudio/ai.json with strict HANCORE bounds (max 64 KiB, 0600 mode, UID check)
        const MAX_CONFIG_BYTES: usize = 64 * 1024;
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let config_dir = PathBuf::from(&home).join(".config/omastudio");

        if let Ok(sec_dir) = SecureDir::open_or_create_hierarchy(&config_dir) {
            if let Ok(c_name) = std::ffi::CString::new("ai.json") {
                if let Ok(mut file) = sec_dir.open_existing_file_ro(&c_name) {
                    // HANCORE: 0600 mode, S_IFREG, UID ownership, and size limit verification
                    if crate::security::verify_secure_open_file(&file, MAX_CONFIG_BYTES).is_ok() {
                        let mut content = String::new();
                        // HANCORE: take(MAX + 1) to prevent silent truncation
                        if file.by_ref().take((MAX_CONFIG_BYTES as u64) + 1).read_to_string(&mut content).is_ok()
                            && content.len() <= MAX_CONFIG_BYTES {
                                if let Ok(parsed) = serde_json::from_str::<JevConfig>(&content) {
                                    config = parsed;
                                }
                        }
                    }
                }
            }
        }

        // 2. Environment variables take precedence if present
        if let Ok(val) = std::env::var("TYPESAFE_API_KEY") {
            let trimmed = val.trim().to_string();
            if Self::is_valid_api_key(&trimmed) {
                config.api_key = trimmed;
                config.provider = "typesafe".to_string();
            }
        } else if let Ok(val) = std::env::var("OPENROUTER_API_KEY") {
            let trimmed = val.trim().to_string();
            if Self::is_valid_api_key(&trimmed) {
                config.api_key = trimmed;
                config.provider = "openrouter".to_string();
                if config.endpoint == "https://api.typesafe.ai/v1/systemone" {
                    config.endpoint = "https://openrouter.ai/api/v1/decisions".to_string();
                }
            }
        }

        config
    }

    /// Validates that an API key is non-empty and contains only safe printable ASCII characters
    pub fn is_valid_api_key(key: &str) -> bool {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_graphic())
    }

    /// Queries the Jev System 1 model with structured state and typed questions
    pub fn query_decisions(&self, state: &JevStateInput) -> Result<JevDecisions, String> {
        if !self.config.enabled {
            return Err("Jev decision engine is disabled in configuration".to_string());
        }

        if !Self::is_valid_api_key(&self.config.api_key) {
            return Err("Missing or invalid API key for Jev decision engine".to_string());
        }

        // Validate endpoint URL (HTTPS mandatory, no control characters, no whitespace or CRLF)
        if !self.config.endpoint.starts_with("https://") {
            return Err("Security policy violation: Endpoint must use HTTPS".to_string());
        }
        if !self.config.endpoint.chars().all(|c| c.is_ascii_graphic()) {
            return Err("Security policy violation: Endpoint contains control characters or whitespace".to_string());
        }

        let state_str = state.to_compact_state_string();

        // Construct standard TypeSafe AI System 1 request payload
        let request_payload = serde_json::json!({
            "model": self.config.model,
            "state": state_str,
            "questions": {
                "scene_type": {
                    "type": "choice",
                    "options": [
                        "Portrait",
                        "Landscape / Nature",
                        "Night / Low-Light",
                        "Street & Architecture",
                        "Golden Hour / Sunset",
                        "Macro / Close-up",
                        "Wildlife / Action",
                        "Studio / High-Key"
                    ],
                    "instructions": "Determine the primary photographic scene classification."
                },
                "recommended_preset": {
                    "type": "choice",
                    "options": [
                        "Kodak Portra 400",
                        "Fuji Velvia 50",
                        "Fuji Classic Chrome",
                        "Cinematic Moody",
                        "Kodak Tri-X 400",
                        "Leica Monochrome",
                        "Modern Clean"
                    ],
                    "instructions": "Recommend the most aesthetically fitting film simulation or grade."
                },
                "needs_highlight_recovery": {
                    "type": "noul",
                    "instructions": "Are highlights clipped or near clipping requiring highlight recovery?"
                },
                "needs_shadow_lift": {
                    "type": "noul",
                    "instructions": "Are shadows underexposed or crushed requiring shadow lift?"
                },
                "recommended_contrast": {
                    "type": "choice",
                    "options": [
                        "Soft / Low Contrast",
                        "Standard / Natural",
                        "Punchy / High Contrast",
                        "Cinematic Dynamic"
                    ],
                    "instructions": "Choose the optimal contrast treatment based on dynamic range."
                },
                "aesthetic_score": {
                    "type": "score",
                    "instructions": "Score the photographic potential and tonal balance from 1 to 10."
                }
            }
        });

        let payload_str = serde_json::to_string(&request_payload)
            .map_err(|e| format!("Failed to serialize Jev request: {}", e))?;

        // Prepare bounded curl invocation according to HANCORE security rules.
        // HANCORE Security: Never pass API keys via cmdline (-H "Authorization: Bearer <key>")
        // because arguments are visible to all system users in /proc/<pid>/cmdline.
        // Pass sensitive authorization headers securely via stdin config (-K -).
        let effective_timeout = self.config.timeout_secs.clamp(1, 30);
        let mut cmd = secure_command("curl");
        cmd.arg("-s")
            .arg("-S")
            .arg("--max-time")
            .arg(effective_timeout.to_string())
            .arg("-X")
            .arg("POST")
            .arg("-K")
            .arg("-")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("--data-raw")
            .arg(&payload_str)
            .arg("--")
            .arg(&self.config.endpoint);

        let curl_config = format!("header = \"Authorization: Bearer {}\"\n", self.config.api_key);
        let timeout = Duration::from_secs(effective_timeout + 2);
        let (exit_code, stdout, stderr) = run_bounded_command_with_stdin(cmd, timeout, Some(curl_config.as_bytes()))
            .map_err(|e| format!("Jev command execution failed: {}", e))?;

        if exit_code != 0 {
            let err_msg = String::from_utf8_lossy(&stderr);
            return Err(format!("Jev HTTP request failed (code {}): {}", exit_code, err_msg.trim()));
        }

        let resp_str = String::from_utf8(stdout)
            .map_err(|e| format!("Invalid UTF-8 in Jev response: {}", e))?;

        Self::parse_jev_response(&resp_str)
    }

    /// Tolerant parser for TypeSafe AI / OpenRouter response formats
    pub fn parse_jev_response(json_str: &str) -> Result<JevDecisions, String> {
        let root: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse Jev JSON response: {}", e))?;

        // Extract decisions map from root or "decisions" or "answers"
        let decisions = if let Some(d) = root.get("decisions").and_then(|v| v.as_object()) {
            d
        } else if let Some(a) = root.get("answers").and_then(|v| v.as_object()) {
            a
        } else if let Some(obj) = root.as_object() {
            obj
        } else {
            return Err("Unexpected Jev JSON structure: root is not an object".to_string());
        };

        // Helper to extract value and confidence from a decision node
        let extract_choice = |key: &str, default_val: &str| -> (String, f32) {
            if let Some(node) = decisions.get(key) {
                if let Some(val_str) = node.get("value").and_then(|v| v.as_str()) {
                    let conf = node.get("confidence")
                        .and_then(|c| c.as_f64())
                        .map(|c| c as f32)
                        .unwrap_or(0.90);
                    return (val_str.to_string(), conf);
                } else if let Some(val_str) = node.as_str() {
                    return (val_str.to_string(), 0.90);
                }
            }
            (default_val.to_string(), 0.50)
        };

        let extract_noul = |key: &str| -> (bool, f32) {
            if let Some(node) = decisions.get(key) {
                if let Some(b) = node.get("value").and_then(|v| v.as_bool()) {
                    let prob = node.get("probability")
                        .and_then(|p| p.as_f64())
                        .map(|p| p as f32)
                        .unwrap_or(if b { 0.85 } else { 0.15 });
                    return (b, prob);
                } else if let Some(b) = node.as_bool() {
                    return (b, if b { 0.85 } else { 0.15 });
                }
            }
            (false, 0.0)
        };

        let extract_score = |key: &str, default_val: f32| -> f32 {
            if let Some(node) = decisions.get(key) {
                if let Some(num) = node.get("value").and_then(|v| v.as_f64()) {
                    return num as f32;
                } else if let Some(num) = node.as_f64() {
                    return num as f32;
                }
            }
            default_val
        };

        let (scene_type, confidence) = extract_choice("scene_type", "Standard");
        let (recommended_preset, _) = extract_choice("recommended_preset", "Fuji Classic Chrome");
        let (recommended_contrast, _) = extract_choice("recommended_contrast", "Standard / Natural");
        let (needs_highlight_recovery, highlight_probability) = extract_noul("needs_highlight_recovery");
        let (needs_shadow_lift, shadow_probability) = extract_noul("needs_shadow_lift");
        let aesthetic_score = extract_score("aesthetic_score", 7.0);

        Ok(JevDecisions {
            scene_type,
            confidence,
            recommended_preset,
            needs_highlight_recovery,
            highlight_probability,
            needs_shadow_lift,
            shadow_probability,
            recommended_contrast,
            aesthetic_score,
            source: "jev-system1".to_string(),
        })
    }
}

/// Computes photographic input state from buffer and metadata
pub fn extract_photographic_state(
    buffer: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    meta: &RawMetadata,
) -> JevStateInput {
    let ch = channels as usize;
    let total_pixels = buffer.len() / ch;
    if total_pixels == 0 {
        return JevStateInput {
            camera_make: meta.make.clone(),
            camera_model: meta.model.clone(),
            lens_model: meta.lens.clone(),
            iso: meta.iso,
            shutter_speed: meta.shutter as f64,
            aperture: meta.aperture,
            focal_length: meta.focal_length,
            width,
            height,
            p5_luma: 10,
            p50_luma: 128,
            p95_luma: 240,
            dynamic_range: 230.0,
            warm_ratio: 0.0,
            green_ratio: 0.0,
            blue_ratio: 0.0,
            dark_ratio: 0.0,
            avg_r: 128.0,
            avg_g: 128.0,
            avg_b: 128.0,
        };
    }

    let mut sum_r: f64 = 0.0;
    let mut sum_g: f64 = 0.0;
    let mut sum_b: f64 = 0.0;
    let mut luma_hist = [0u32; 256];

    let mut green_count = 0u32;
    let mut blue_count = 0u32;
    let mut warm_count = 0u32;
    let mut dark_count = 0u32;

    for chunk in buffer.chunks_exact(ch) {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];
        let luma = ((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) as u32).min(255) as usize;

        sum_r += r as f64;
        sum_g += g as f64;
        sum_b += b as f64;
        luma_hist[luma] += 1;

        let rf = r as f32;
        let gf = g as f32;
        let bf = b as f32;
        let lf = luma as f32;

        if gf > rf * 1.15 && gf > bf * 1.15 {
            green_count += 1;
        }
        if bf > rf * 1.15 && bf > gf * 1.10 {
            blue_count += 1;
        }
        if rf > 150.0 && gf > 100.0 && bf < 100.0 {
            warm_count += 1;
        }
        if lf < 35.0 {
            dark_count += 1;
        }
    }

    let tot_f = total_pixels as f64;
    let avg_r = (sum_r / tot_f) as f32;
    let avg_g = (sum_g / tot_f) as f32;
    let avg_b = (sum_b / tot_f) as f32;

    let p5_idx = (tot_f * 0.05) as u32;
    let p50_idx = (tot_f * 0.50) as u32;
    let p95_idx = (tot_f * 0.95) as u32;

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

    let total_f32 = total_pixels as f32;
    JevStateInput {
        camera_make: meta.make.clone(),
        camera_model: meta.model.clone(),
        lens_model: meta.lens.clone(),
        iso: meta.iso,
        shutter_speed: meta.shutter as f64,
        aperture: meta.aperture,
        focal_length: meta.focal_length,
        width,
        height,
        p5_luma: p5,
        p50_luma: p50,
        p95_luma: p95,
        dynamic_range: (p95 as f32 - p5 as f32).max(0.0),
        warm_ratio: warm_count as f32 / total_f32,
        green_ratio: green_count as f32 / total_f32,
        blue_ratio: blue_count as f32 / total_f32,
        dark_ratio: dark_count as f32 / total_f32,
        avg_r,
        avg_g,
        avg_b,
    }
}

/// Applies Jev decisions to refine a Recipe's tone and preset
pub fn apply_jev_decisions_to_recipe(recipe: &mut Recipe, jev: &JevDecisions) {
    recipe.preset_name = Some(jev.recommended_preset.clone());

    if jev.needs_highlight_recovery {
        let pull = (jev.highlight_probability * 35.0).clamp(15.0, 45.0);
        recipe.highlights = -pull;
    }

    if jev.needs_shadow_lift {
        let lift = (jev.shadow_probability * 35.0).clamp(15.0, 45.0);
        recipe.shadows = lift;
    }

    match jev.recommended_contrast.as_str() {
        "Punchy / High Contrast" => recipe.contrast = 20.0,
        "Soft / Low Contrast" => recipe.contrast = -10.0,
        "Cinematic Dynamic" => recipe.contrast = 12.0,
        _ => recipe.contrast = 5.0,
    }
}

/// Deterministic offline photographic intelligence engine (Jev Offline Expert Mode)
pub fn generate_offline_jev_decisions(state: &JevStateInput, meta: &RawMetadata) -> JevDecisions {
    // 1. Photographic scene analysis via Bayesian evaluation of EXIF + Luma + Chroma
    let is_wide_aperture = meta.aperture > 0.0 && meta.aperture <= 2.8;
    let is_telephoto = meta.focal_length >= 50.0;
    let is_high_iso = meta.iso >= 1600.0;
    let is_wide_angle = meta.focal_length > 0.0 && meta.focal_length <= 35.0;

    let (scene_type, preset, confidence) = if is_high_iso || state.dark_ratio > 0.40 {
        ("Night / Low-Light".to_string(), "Cinematic Moody".to_string(), 0.90)
    } else if (is_wide_aperture && is_telephoto && state.warm_ratio > 0.10) || state.warm_ratio > 0.28 {
        ("Portrait".to_string(), "Kodak Portra 400".to_string(), 0.92)
    } else if state.warm_ratio > 0.22 && state.blue_ratio < 0.20 {
        ("Golden Hour / Sunset".to_string(), "Kodak Portra 400".to_string(), 0.94)
    } else if state.green_ratio > 0.18 || (is_wide_angle && state.blue_ratio > 0.15) {
        ("Landscape / Nature".to_string(), "Fuji Velvia 50".to_string(), 0.95)
    } else {
        ("Street & Architecture".to_string(), "Fuji Classic Chrome".to_string(), 0.86)
    };

    // 2. Highlight and Shadow evaluation
    let needs_highlight_recovery = state.p95_luma >= 238;
    let highlight_probability = if state.p95_luma >= 238 {
        ((state.p95_luma as f32 - 235.0) / 20.0).clamp(0.65, 0.98)
    } else {
        0.10
    };

    let needs_shadow_lift = state.p5_luma <= 18 && !is_high_iso;
    let shadow_probability = if state.p5_luma <= 18 {
        ((20.0 - state.p5_luma as f32) / 20.0).clamp(0.55, 0.95)
    } else {
        0.12
    };

    // 3. Recommended contrast
    let recommended_contrast = if state.dynamic_range < 140.0 {
        "Punchy / High Contrast".to_string()
    } else if state.dynamic_range > 220.0 {
        "Soft / Low Contrast".to_string()
    } else {
        "Standard / Natural".to_string()
    };

    // 4. Mathematical aesthetic score [1.0 to 10.0]
    let clip_penalty = if state.p95_luma >= 250 { 1.5 } else { 0.0 };
    let crush_penalty = if state.p5_luma <= 5 { 1.0 } else { 0.0 };
    let dr_bonus = (state.dynamic_range / 255.0) * 1.5;
    let aesthetic_score = (8.0 + dr_bonus - clip_penalty - crush_penalty).clamp(5.0, 9.8);

    JevDecisions {
        scene_type,
        confidence,
        recommended_preset: preset,
        needs_highlight_recovery,
        highlight_probability,
        needs_shadow_lift,
        shadow_probability,
        recommended_contrast,
        aesthetic_score: (aesthetic_score * 10.0).round() / 10.0,
        source: "jev-offline-expert".to_string(),
    }
}

/// Hybrid scene classification: attempts Jev System 1 first; falls back transparently to local heuristics
pub fn ai_classify_scene_with_jev(
    buffer: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    meta: &RawMetadata,
) -> (SceneAnalysis, Option<JevDecisions>) {
    let state = extract_photographic_state(buffer, width, height, channels, meta);
    let client = JevClient::new(JevClient::load_default_config());

    match client.query_decisions(&state) {
        Ok(jev) => {
            let analysis = SceneAnalysis {
                scene_type: jev.scene_type.clone(),
                confidence: jev.confidence,
                recommended_preset: jev.recommended_preset.clone(),
                description: format!(
                    "TypeSafe AI Jev decision [Score: {:.0}/10, Contrast: {}]",
                    jev.aesthetic_score, jev.recommended_contrast
                ),
            };
            (analysis, Some(jev))
        }
        Err(_err) => {
            // Graceful fallback to local heuristic classifier
            let mut analysis = crate::ai::ai_classify_scene(buffer, width, height, channels, meta);
            analysis.description = format!("{} (Local Heuristic Engine)", analysis.description);
            (analysis, None)
        }
    }
}
