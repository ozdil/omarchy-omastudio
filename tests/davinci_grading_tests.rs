use omastudio_engine::pipeline::histogram::compute_histogram_with_dimensions;
use omastudio_engine::pipeline::lut::Lut3D;
use omastudio_engine::pipeline::presence::apply_presence_ex;
use omastudio_engine::pipeline::tone::apply_tone_pixel;
use omastudio_engine::pipeline::resolve_recipe_lut;
use omastudio_engine::recipe::Recipe;

#[test]
fn test_davinci_contrast_pivot() {
    let r_low_pivot = Recipe {
        contrast: 50.0,
        contrast_pivot: 0.20,
        ..Default::default()
    };

    let r_high_pivot = Recipe {
        contrast: 50.0,
        contrast_pivot: 0.80,
        ..Default::default()
    };

    // Test a mid-highlight pixel (0.60)
    let (r_low, _, _) = apply_tone_pixel(0.60, 0.60, 0.60, &r_low_pivot, 1.0);
    let (r_high, _, _) = apply_tone_pixel(0.60, 0.60, 0.60, &r_high_pivot, 1.0);

    // With low pivot (0.20), 0.60 is far above pivot, so contrast pushes it higher.
    // With high pivot (0.80), 0.60 is below pivot, so contrast pushes it lower.
    assert!(
        r_low > r_high,
        "Low pivot should expand 0.60 upwards while high pivot compresses it downwards: got r_low={}, r_high={}",
        r_low,
        r_high
    );
}

#[test]
fn test_davinci_color_boost_protects_saturated_tones() {
    let recipe = Recipe {
        color_boost: 50.0,
        ..Default::default()
    };

    // 1. Muted pixel (low saturation: R=0.55, G=0.50, B=0.50)
    let (muted_r, muted_g, _muted_b) = apply_tone_pixel(0.55, 0.50, 0.50, &recipe, 1.0);
    let muted_diff = (muted_r - muted_g).abs();

    // 2. Already saturated pixel (R=0.90, G=0.10, B=0.10)
    let (sat_r, sat_g, _sat_b) = apply_tone_pixel(0.90, 0.10, 0.10, &recipe, 1.0);
    let sat_diff = (sat_r - sat_g).abs();

    // Color boost should enhance the chromatic separation of the muted pixel
    assert!(
        muted_diff > 0.05,
        "Muted pixel chromatic difference should be noticeably boosted by Color Boost: got {}",
        muted_diff
    );

    // Saturated pixel should remain bounded without runaway chromatic explosion
    assert!(
        sat_diff <= 0.95,
        "Saturated tone should not clip explosively: got {}",
        sat_diff
    );
}

#[test]
fn test_davinci_midtone_detail_isolation() {
    let width = 32;
    let height = 32;
    let mut buffer = vec![0.0f32; width * height * 3];

    // Top half: Deep shadows with subtle noise (0.05 vs 0.06)
    // Bottom half: Midtones with rich texture (alternating 0.48 and 0.55)
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            if y < height / 2 {
                let v = if (x + y) % 2 == 0 { 0.05 } else { 0.06 };
                buffer[idx] = v;
                buffer[idx + 1] = v;
                buffer[idx + 2] = v;
            } else {
                let v = if (x + y) % 2 == 0 { 0.48 } else { 0.55 };
                buffer[idx] = v;
                buffer[idx + 1] = v;
                buffer[idx + 2] = v;
            }
        }
    }

    let mut buf_copy = buffer.clone();
    apply_presence_ex(&mut buf_copy, width, height, 0.0, 0.0, 0.0, 60.0);

    // Compute average magnitude of change in shadow region vs midtone region
    let mut shadow_change = 0.0f32;
    let mut mid_change = 0.0f32;
    let half_pixels = (width * height / 2) as f32;

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            let diff = (buf_copy[idx] - buffer[idx]).abs();
            if y < height / 2 {
                shadow_change += diff;
            } else {
                mid_change += diff;
            }
        }
    }

    shadow_change /= half_pixels;
    mid_change /= half_pixels;

    assert!(
        mid_change > shadow_change * 1.5,
        "Midtone detail must impact midtones substantially more than shadows: mid_change={}, shadow_change={}",
        mid_change,
        shadow_change
    );
}

#[test]
fn test_davinci_3d_lut_trilinear_interpolation() {
    let lut = Lut3D::kodak_2383();
    assert_eq!(lut.size, 17);
    assert_eq!(lut.table.len(), 17 * 17 * 17);

    // Sample neutral gray
    let (r, g, b) = lut.sample(0.5, 0.5, 0.5);
    assert!(r > 0.0 && r < 1.0);
    assert!(g > 0.0 && g < 1.0);
    assert!(b > 0.0 && b < 1.0);

    // Check resolve_recipe_lut
    let mut recipe = Recipe {
        lut_name: Some("Kodak 2383".to_string()),
        ..Default::default()
    };
    assert!(resolve_recipe_lut(&recipe).is_some());

    recipe.lut_name = Some("Teal & Orange".to_string());
    assert!(resolve_recipe_lut(&recipe).is_some());

    recipe.lut_name = Some("Silver Nitrate".to_string());
    let silver_lut = resolve_recipe_lut(&recipe).unwrap();
    let (sr, sg, sb) = silver_lut.sample(0.8, 0.2, 0.1);
    // Silver nitrate is monochrome: R == G == B
    assert!(
        (sr - sg).abs() < 1e-4 && (sg - sb).abs() < 1e-4,
        "Silver nitrate LUT must output pure monochrome grayscale"
    );
}

#[test]
fn test_davinci_video_scopes_generation() {
    let width = 64;
    let height = 32;
    let mut test_img = vec![0u8; width * height * 3];

    // Left half: Cyan (R=0, G=200, B=200)
    // Right half: Skin tone (R=210, G=150, B=120)
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            if x < width / 2 {
                test_img[idx] = 0;
                test_img[idx + 1] = 200;
                test_img[idx + 2] = 200;
            } else {
                test_img[idx] = 210;
                test_img[idx + 1] = 150;
                test_img[idx + 2] = 120;
            }
        }
    }

    let scopes = compute_histogram_with_dimensions(&test_img, width, height, 3);

    // Waveform Luma must be populated (64 cols x 32 rows = 2048 entries)
    assert_eq!(scopes.waveform_luma.len(), 64 * 32);
    let wf_active = scopes.waveform_luma.iter().filter(|&&v| v > 0).count();
    assert!(wf_active > 0, "Waveform luma must contain active energy");

    // RGB Parade must be populated (32 cols x 32 rows = 1024 entries each)
    assert_eq!(scopes.parade_r.len(), 32 * 32);
    assert_eq!(scopes.parade_g.len(), 32 * 32);
    assert_eq!(scopes.parade_b.len(), 32 * 32);

    // Vectorscope must be populated (48 x 48 = 2304 entries)
    assert_eq!(scopes.vectorscope.len(), 48 * 48);
    let vec_active = scopes.vectorscope.iter().filter(|&&v| v > 0).count();
    assert!(vec_active > 0, "Vectorscope must contain active chromaticity coordinates");
}

#[test]
fn test_sota_highlight_reconstruction_repairs_clipping() {
    let r_default = Recipe::default(); // highlight_reconstruct = true
    let r_off = Recipe {
        highlight_reconstruct: false,
        ..Default::default()
    };

    // Simulate harsh clipped green channel: R=0.70, G=1.15 (blown), B=0.60
    let (r_fixed, g_fixed, b_fixed) = apply_tone_pixel(0.70, 1.15, 0.60, &r_default, 1.0);
    let (r_raw, g_raw, b_raw) = apply_tone_pixel(0.70, 1.15, 0.60, &r_off, 1.0);

    // Without reconstruction, G remains disproportionately massive causing harsh green cast
    let cast_raw = g_raw - r_raw.max(b_raw);
    let cast_fixed = g_fixed - r_fixed.max(b_fixed);

    assert!(
        cast_fixed < cast_raw,
        "Highlight reconstruction should harmonize clipped channel energy: raw_cast={}, fixed_cast={}",
        cast_raw,
        cast_fixed
    );
}

#[test]
fn test_agx_filmic_tone_curve_monotonicity() {
    use omastudio_engine::pipeline::tone::apply_agx_filmic_curve;

    let v0 = apply_agx_filmic_curve(0.0);
    let v_mid = apply_agx_filmic_curve(0.5);
    let v_high = apply_agx_filmic_curve(1.2);

    assert_eq!(v0, 0.0, "AgX curve should anchor 0.0 to 0.0");
    assert!(v_mid > 0.0 && v_mid < 1.0, "AgX mid value should be normalized");
    assert!(v_high >= v_mid && v_high <= 1.0, "AgX curve must be monotonic and clamped to 1.0");
}
