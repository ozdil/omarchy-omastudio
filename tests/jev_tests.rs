use omastudio_engine::ai::jev::{
    apply_jev_decisions_to_recipe, extract_photographic_state, JevClient, JevConfig,
    JevDecisions, JevStateInput,
};
use omastudio_engine::ai::ai_classify_scene_with_jev;
use omastudio_engine::raw::RawMetadata;
use omastudio_engine::recipe::Recipe;

#[test]
fn test_jev_state_input_formatting() {
    let state = JevStateInput {
        camera_make: "Sony".to_string(),
        camera_model: "ILCE-7RM4".to_string(),
        lens_model: "FE 24-70mm F2.8 GM".to_string(),
        iso: 200.0,
        shutter_speed: 0.002, // 1/500s
        aperture: 2.8,
        focal_length: 35.0,
        width: 9504,
        height: 6336,
        p5_luma: 15,
        p50_luma: 118,
        p95_luma: 242,
        dynamic_range: 227.0,
        warm_ratio: 0.18,
        green_ratio: 0.42,
        blue_ratio: 0.15,
        dark_ratio: 0.08,
        avg_r: 110.0,
        avg_g: 135.0,
        avg_b: 98.0,
    };

    let formatted = state.to_compact_state_string();
    assert!(formatted.contains("Sony ILCE-7RM4"));
    assert!(formatted.contains("FE 24-70mm F2.8 GM"));
    assert!(formatted.contains("ISO 200"));
    assert!(formatted.contains("f/2.8"));
    assert!(formatted.contains("1/500s"));
    assert!(formatted.contains("Dynamic Range: 227/255"));
    assert!(formatted.contains("Green=42.0%"));
}

#[test]
fn test_jev_api_key_validation() {
    // Valid alphanumeric keys
    assert!(JevClient::is_valid_api_key("sk_live_1234567890abcdef"));
    assert!(JevClient::is_valid_api_key("typesafe_jev_token_xyz-123"));

    // Invalid keys (empty, spaces, newlines / CRLF injection attempts)
    assert!(!JevClient::is_valid_api_key(""));
    assert!(!JevClient::is_valid_api_key("   "));
    assert!(!JevClient::is_valid_api_key("token\r\nInjected-Header: evil"));
    assert!(!JevClient::is_valid_api_key("token with space"));
}

#[test]
fn test_parse_jev_detailed_response() {
    let response_json = r#"{
        "model": "jev-latest",
        "decisions": {
            "scene_type": {
                "value": "Landscape / Nature",
                "confidence": 0.96
            },
            "recommended_preset": {
                "value": "Fuji Velvia 50",
                "confidence": 0.94
            },
            "needs_highlight_recovery": {
                "value": true,
                "probability": 0.88
            },
            "needs_shadow_lift": {
                "value": false,
                "probability": 0.12
            },
            "recommended_contrast": {
                "value": "Punchy / High Contrast"
            },
            "aesthetic_score": {
                "value": 9.2
            }
        }
    }"#;

    let decisions = JevClient::parse_jev_response(response_json).expect("Failed to parse Jev response");
    assert_eq!(decisions.scene_type, "Landscape / Nature");
    assert!((decisions.confidence - 0.96).abs() < 0.01);
    assert_eq!(decisions.recommended_preset, "Fuji Velvia 50");
    assert!(decisions.needs_highlight_recovery);
    assert!((decisions.highlight_probability - 0.88).abs() < 0.01);
    assert!(!decisions.needs_shadow_lift);
    assert_eq!(decisions.recommended_contrast, "Punchy / High Contrast");
    assert!((decisions.aesthetic_score - 9.2).abs() < 0.01);
    assert_eq!(decisions.source, "jev-system1");
}

#[test]
fn test_parse_jev_simple_flat_response() {
    let response_json = r#"{
        "scene_type": "Portrait",
        "recommended_preset": "Kodak Portra 400",
        "needs_highlight_recovery": false,
        "needs_shadow_lift": true,
        "recommended_contrast": "Soft / Low Contrast",
        "aesthetic_score": 8.5
    }"#;

    let decisions = JevClient::parse_jev_response(response_json).expect("Failed to parse flat response");
    assert_eq!(decisions.scene_type, "Portrait");
    assert_eq!(decisions.recommended_preset, "Kodak Portra 400");
    assert!(!decisions.needs_highlight_recovery);
    assert!(decisions.needs_shadow_lift);
    assert_eq!(decisions.recommended_contrast, "Soft / Low Contrast");
    assert!((decisions.aesthetic_score - 8.5).abs() < 0.01);
}

#[test]
fn test_apply_jev_decisions_to_recipe() {
    let mut recipe = Recipe::default();
    let decisions = JevDecisions {
        scene_type: "Landscape / Nature".to_string(),
        confidence: 0.95,
        recommended_preset: "Fuji Velvia 50".to_string(),
        needs_highlight_recovery: true,
        highlight_probability: 0.80,
        needs_shadow_lift: true,
        shadow_probability: 0.70,
        recommended_contrast: "Punchy / High Contrast".to_string(),
        aesthetic_score: 9.0,
        source: "jev-system1".to_string(),
    };

    apply_jev_decisions_to_recipe(&mut recipe, &decisions);

    assert_eq!(recipe.preset_name.as_deref(), Some("Fuji Velvia 50"));
    assert!(recipe.highlights < -20.0, "Highlights should be pulled down for recovery");
    assert!(recipe.shadows > 20.0, "Shadows should be lifted");
    assert_eq!(recipe.contrast, 20.0, "Contrast should be set to 20.0 for Punchy");
}

#[test]
fn test_ai_classify_scene_with_jev_offline_fallback() {
    // Generate synthetic image buffer (e.g. green-dominant landscape)
    let width = 64;
    let height = 64;
    let mut buffer = Vec::with_capacity((width * height * 3) as usize);
    for _ in 0..(width * height) {
        buffer.push(50);  // R
        buffer.push(180); // G (green dominant)
        buffer.push(40);  // B
    }

    let meta = RawMetadata {
        width,
        height,
        raw_width: width,
        raw_height: height,
        make: "Fujifilm".to_string(),
        model: "X-T5".to_string(),
        lens: "XF 16-55mm F2.8 R LM WR".to_string(),
        iso: 160.0,
        shutter: 0.004,
        aperture: 4.0,
        focal_length: 24.0,
        timestamp: 1700000000,
        cam_mul: [1.0, 1.0, 1.0, 1.0],
    };

    // When no API key is configured, function must seamlessly fall back to local heuristic engine
    let (analysis, jev_opt) = ai_classify_scene_with_jev(&buffer, width, height, 3, &meta);

    assert!(jev_opt.is_none(), "Should fall back without Jev decisions when no API key configured");
    assert_eq!(analysis.scene_type, "Landscape / Nature");
    assert!(analysis.description.contains("Local Heuristic Engine"));
    assert_eq!(analysis.recommended_preset, "Fuji Velvia 50");
}

#[test]
fn test_extract_photographic_state_and_config() {
    let config = JevConfig::default();
    assert_eq!(config.provider, "typesafe");
    assert_eq!(config.model, "jev-latest");
    assert_eq!(config.timeout_secs, 5);

    let width = 32;
    let height = 32;
    let mut buffer = Vec::new();
    for _ in 0..(width * height) {
        buffer.push(200); // warm R
        buffer.push(120); // G
        buffer.push(60);  // B
    }

    let meta = RawMetadata {
        width,
        height,
        raw_width: width,
        raw_height: height,
        make: "Nikon".to_string(),
        model: "Z8".to_string(),
        lens: "NIKKOR Z 50mm f/1.2 S".to_string(),
        iso: 64.0,
        shutter: 0.001,
        aperture: 1.2,
        focal_length: 50.0,
        timestamp: 1700000000,
        cam_mul: [1.0, 1.0, 1.0, 1.0],
    };

    let state = extract_photographic_state(&buffer, width, height, 3, &meta);
    assert_eq!(state.camera_make, "Nikon");
    assert_eq!(state.camera_model, "Z8");
    assert!(state.warm_ratio > 0.5);
    assert!(state.dynamic_range >= 0.0);
}
