use omastudio_engine::pipeline::{process_buffer_16_to_8, process_buffer_16_to_16};
use omastudio_engine::raw::RawImage;
use omastudio_engine::recipe::Recipe;
use std::path::PathBuf;

#[test]
fn test_presence_clarity_midtone_isolation() {
    let width = 64;
    let height = 64;
    let mut buffer16 = vec![0u16; width * height * 3];

    // Create gradient with shadow, midtone, and highlight regions
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            // Linear gradient from 0 to 65535
            let v = ((x * 65535) / (width - 1)) as u16;
            buffer16[idx] = v;
            buffer16[idx + 1] = v;
            buffer16[idx + 2] = v;
        }
    }

    let recipe_neutral = Recipe {
        sharpness: 0.0,
        denoise_col: 0.0,
        ..Default::default()
    };
    let recipe_clarity = Recipe {
        clarity: 60.0,
        sharpness: 0.0,
        denoise_col: 0.0,
        ..Default::default()
    };

    let (out_neutral, _) = process_buffer_16_to_8(&buffer16, width as u32, height as u32, 3, &recipe_neutral);
    let (out_clarity, _) = process_buffer_16_to_8(&buffer16, width as u32, height as u32, 3, &recipe_clarity);

    // Midtone pixel (x = 32) vs extreme shadow (x = 2) vs extreme highlight (x = 62)
    let idx_shadow = (32 * width + 2) * 3;
    let idx_midtone = (32 * width + 32) * 3;
    let idx_highlight = (32 * width + 62) * 3;

    let shadow_diff = (out_clarity[idx_shadow] as i32 - out_neutral[idx_shadow] as i32).abs();
    let midtone_diff = (out_clarity[idx_midtone] as i32 - out_neutral[idx_midtone] as i32).abs();
    let highlight_diff = (out_clarity[idx_highlight] as i32 - out_neutral[idx_highlight] as i32).abs();

    println!(
        "Clarity response -> Shadow diff: {}, Midtone diff: {}, Highlight diff: {}",
        shadow_diff, midtone_diff, highlight_diff
    );

    // Extreme shadows and highlights must have minimal alteration compared to midtones
    assert!(
        shadow_diff <= 2,
        "Clarity must not alter deep shadows"
    );
    assert!(
        highlight_diff <= 3,
        "Clarity must not clip or blow out specular highlights"
    );
}

#[test]
fn test_presence_texture_frequency_micro_contrast() {
    let width = 64;
    let height = 64;
    let mut buffer16 = vec![32768u16; width * height * 3]; // Neutral mid-gray

    // Add fine checkerboard texture (simulating fabric or skin pores)
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            if (x + y) % 2 == 0 {
                buffer16[idx] = 34000;
                buffer16[idx + 1] = 34000;
                buffer16[idx + 2] = 34000;
            } else {
                buffer16[idx] = 31500;
                buffer16[idx + 1] = 31500;
                buffer16[idx + 2] = 31500;
            }
        }
    }

    let recipe_neutral = Recipe {
        sharpness: 0.0,
        denoise_col: 0.0,
        ..Default::default()
    };
    let recipe_pos_texture = Recipe {
        texture: 75.0,
        sharpness: 0.0,
        denoise_col: 0.0,
        ..Default::default()
    };
    let recipe_neg_texture = Recipe {
        texture: -75.0,
        sharpness: 0.0,
        denoise_col: 0.0,
        ..Default::default()
    };

    let (out_neutral, _) = process_buffer_16_to_8(&buffer16, width as u32, height as u32, 3, &recipe_neutral);
    let (out_pos, _) = process_buffer_16_to_8(&buffer16, width as u32, height as u32, 3, &recipe_pos_texture);
    let (out_neg, _) = process_buffer_16_to_8(&buffer16, width as u32, height as u32, 3, &recipe_neg_texture);

    let idx1 = (16 * width + 16) * 3;
    let idx2 = (16 * width + 17) * 3;

    let contrast_neutral = (out_neutral[idx1] as i32 - out_neutral[idx2] as i32).abs();
    let contrast_pos = (out_pos[idx1] as i32 - out_pos[idx2] as i32).abs();
    let contrast_neg = (out_neg[idx1] as i32 - out_neg[idx2] as i32).abs();

    println!(
        "Texture micro-contrast -> Neutral: {}, Positive (+75): {}, Negative (-75): {}",
        contrast_neutral, contrast_pos, contrast_neg
    );

    assert!(
        contrast_pos >= contrast_neutral,
        "Positive Texture must enhance fine surface detail"
    );
    assert!(
        contrast_neg <= contrast_neutral,
        "Negative Texture must smoothly soften fine skin texture"
    );
}

#[test]
fn test_detail_sharpness_and_chroma_denoise() {
    let width = 64;
    let height = 64;
    let mut buffer16 = vec![30000u16; width * height * 3];

    // Add sharp edge down the middle (x = 32)
    for y in 0..height {
        for x in 32..width {
            let idx = (y * width + x) * 3;
            buffer16[idx] = 45000;
            buffer16[idx + 1] = 45000;
            buffer16[idx + 2] = 45000;
        }
    }

    // Add color noise speckle
    buffer16[(10 * width + 10) * 3] = 40000; // Red spike
    buffer16[(10 * width + 10) * 3 + 1] = 20000; // Green dip

    let recipe_sharp = Recipe {
        sharpness: 80.0,
        denoise_col: 80.0,
        ..Default::default()
    };

    let (out_sharp, _) = process_buffer_16_to_8(&buffer16, width as u32, height as u32, 3, &recipe_sharp);

    // Edge contrast between x=31 and x=32
    let idx_left = (32 * width + 31) * 3;
    let idx_right = (32 * width + 32) * 3;

    let edge_step = out_sharp[idx_right] as i32 - out_sharp[idx_left] as i32;
    assert!(edge_step > 50, "Edge must remain crisp and well-defined");

    // Color speckle at (10, 10) should be smoothed by chroma denoise
    let speckle_idx = (10 * width + 10) * 3;
    let r_diff = (out_sharp[speckle_idx] as i32 - out_sharp[speckle_idx + 1] as i32).abs();
    assert!(r_diff < 40, "Chroma noise speckle should be smoothed out by chroma denoise");
}

#[test]
fn test_real_medium_format_raw_full_pipeline_benchmark() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let sample_path = PathBuf::from(home).join("Downloads/yurt/_DSF2254.RAF");
    if !sample_path.exists() {
        eprintln!("Sample RAW not present on system, skipping benchmark");
        return;
    }

    let raw = RawImage::open(&sample_path).expect("Should open RAF");
    let preview16 = raw.process_preview_16(true).expect("Should process 16-bit preview");
    let u16_slice = preview16.as_slice_u16();

    let recipe = Recipe {
        exposure: 0.35,
        contrast: 15.0,
        highlights: -35.0,
        shadows: 45.0,
        whites: 10.0,
        blacks: -5.0,
        texture: 20.0,
        clarity: 15.0,
        dehaze: 10.0,
        vibrance: 12.0,
        sharpness: 35.0,
        denoise_col: 20.0,
        lift: [0.0, 0.02, 0.05], // Subtle filmic cool shadows
        gain: [0.03, 0.01, 0.0],  // Subtle warm highlights
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let (buf8, hist) = process_buffer_16_to_8(
        u16_slice,
        preview16.width,
        preview16.height,
        preview16.channels,
        &recipe,
    );
    let elapsed = start.elapsed();

    println!(
        "Rendered 16-bit RAF ({}x{}) with full premier pipeline in {:.2}ms",
        preview16.width,
        preview16.height,
        elapsed.as_secs_f64() * 1000.0
    );

    assert_eq!(buf8.len(), (preview16.width * preview16.height * 3) as usize);
    assert!(hist.max_count > 0);
    let threshold_ms = if cfg!(debug_assertions) { 1500 } else { 650 };
    assert!(
        elapsed.as_millis() < threshold_ms,
        "Viewport render took {}ms, must execute under {}ms on modern multi-core Linux PC",
        elapsed.as_millis(),
        threshold_ms
    );
}

#[test]
fn test_sixteen_bit_to_sixteen_bit_export_fidelity() {
    let width = 32;
    let height = 32;
    let mut buffer16 = vec![0u16; width * height * 3];

    // Subtle 16-bit gradients that would suffer from 8-bit quantization banding
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            // High-precision 16-bit stepping: step by 16 values (sub-8-bit level)
            let val = (x as u16) * 16 + 32000;
            buffer16[idx] = val;
            buffer16[idx + 1] = val;
            buffer16[idx + 2] = val;
        }
    }

    let recipe = Recipe {
        exposure: 0.1,
        highlights: -10.0,
        shadows: 15.0,
        clarity: 10.0,
        texture: 10.0,
        sharpness: 25.0,
        ..Default::default()
    };

    let (buf16, hist) = process_buffer_16_to_16(&buffer16, width as u32, height as u32, 3, &recipe);

    assert_eq!(buf16.len(), width * height * 3);
    assert!(hist.max_count > 0);

    // Verify fine gradations are preserved without 8-bit truncation (values shouldn't be stepped by 256)
    let val_x0 = buf16[0];
    let val_x1 = buf16[3];
    let diff = (val_x1 as i32 - val_x0 as i32).abs();
    println!("16-bit gradation step diff: {}", diff);
    assert!(diff > 0 && diff < 256, "16-bit gradation must preserve subtle sub-8-bit steps");
}

