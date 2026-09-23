use omastudio_engine::export::{export_photo, ExportOptions};
use omastudio_engine::pipeline::{
    process_buffer_16_to_8, process_buffer_16_to_16, process_split_comparison_16_to_8,
};
use omastudio_engine::raw::RawImage;
use omastudio_engine::recipe::Recipe;
use std::path::PathBuf;

#[test]
fn test_16bit_pipeline_precision_and_dynamic_range() {
    let width = 64;
    let height = 64;
    let mut buffer16 = vec![0u16; width * height * 3];

    // Create 16-bit smooth gradient (0 to 65535)
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            buffer16[idx] = ((x * 65535) / width) as u16;
            buffer16[idx + 1] = ((y * 65535) / height) as u16;
            buffer16[idx + 2] = 32768; // Mid gray in blue
        }
    }

    let recipe = Recipe {
        exposure: 1.0,
        shadows: 40.0,
        highlights: -30.0,
        ..Default::default()
    };

    // Test 16-bit to 8-bit viewport rendering
    let (buf8, hist8) = process_buffer_16_to_8(
        &buffer16,
        width as u32,
        height as u32,
        3,
        &recipe,
    );
    assert_eq!(buf8.len(), width * height * 3);
    assert!(hist8.max_count > 0);

    // Test 16-bit to 16-bit export rendering
    let (buf16_out, hist16) = process_buffer_16_to_16(
        &buffer16,
        width as u32,
        height as u32,
        3,
        &recipe,
    );
    assert_eq!(buf16_out.len(), buffer16.len());
    assert!(hist16.max_count > 0);

    // Test Before/After split comparison
    let (split_buf, _) = process_split_comparison_16_to_8(
        &buffer16,
        width as u32,
        height as u32,
        3,
        &recipe,
        0.5,
    );
    assert_eq!(split_buf.len(), width * height * 3);
}

#[test]
fn test_real_raw_16bit_loading_and_tiff_export() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let sample_raw = PathBuf::from(home).join("Downloads/yurt/_DSF2246.RAF");
    if !sample_raw.exists() {
        eprintln!("Sample RAW not present on system, skipping real RAW integration test");
        return;
    }

    let raw = RawImage::open(&sample_raw).expect("Should open sample Fujifilm RAW");
    let preview16 = raw.process_preview_16(true).expect("Should process 16-bit preview");
    assert_eq!(preview16.bits_per_sample, 16);
    assert!(preview16.width > 0);
    assert!(preview16.height > 0);
    assert_eq!(preview16.data_size, (preview16.width * preview16.height * preview16.channels * 2) as usize);

    let u16_slice = preview16.as_slice_u16();
    assert_eq!(u16_slice.len(), (preview16.width * preview16.height * preview16.channels) as usize);

    // Test 16-bit viewport pipeline processing
    let recipe = Recipe::default();
    let (preview8, hist) = process_buffer_16_to_8(
        u16_slice,
        preview16.width,
        preview16.height,
        preview16.channels,
        &recipe,
    );
    assert_eq!(preview8.len(), (preview16.width * preview16.height * preview16.channels) as usize);
    assert!(hist.max_count > 0);

    // Test 16-bit TIFF Export
    let tmp_dir = std::env::temp_dir().join("omastudio_16bit_test");
    let export_opts = ExportOptions {
        format: "tiff".to_string(),
        scale_percent: 25, // Scale down for fast test
        output_dir: tmp_dir.to_string_lossy().to_string(),
        preserve_exif: false,
        icc_profile: None,
        ..Default::default()
    };

    let exported = export_photo(sample_raw, &recipe, &export_opts).expect("Export to 16-bit TIFF should succeed");
    assert!(exported.exists());

    // Verify it is encoded as 16-bit RGB
    let loaded = image::open(&exported).expect("Should open exported TIFF");
    assert_eq!(loaded.color(), image::ColorType::Rgb16);

    let _ = std::fs::remove_file(exported);
    let _ = std::fs::remove_dir(tmp_dir);
}

#[test]
fn test_shadow_lift_preserves_color_without_chroma_explosion() {
    use omastudio_engine::pipeline::tone::apply_tone_pixel;

    let recipe = Recipe {
        shadows: 53.0,
        highlights: -46.0,
        ..Default::default()
    };

    // 1. Pure black must remain 0.0 (anchoring black point)
    let (r_blk, g_blk, b_blk) = apply_tone_pixel(0.0, 0.0, 0.0, &recipe, 1.0);
    assert_eq!((r_blk, g_blk, b_blk), (0.0, 0.0, 0.0), "Black point must remain anchored");

    // 2. Muted shadow color (e.g. pink chair in room shadow)
    let (r, g, b) = (0.122f32, 0.0305f32, 0.0610f32);
    let luma_in = 0.2126 * r + 0.7152 * g + 0.0722 * b;

    let (r_out, g_out, b_out) = apply_tone_pixel(r, g, b, &recipe, 1.0);
    let luma_out = 0.2126 * r_out + 0.7152 * g_out + 0.0722 * b_out;

    // Luminance must be lifted
    assert!(luma_out > luma_in * 1.5, "Shadows should be noticeably illuminated");

    // Chroma gain must remain natural (under 1.4x), never exploding into neon lasers
    let chroma_in = (r - luma_in).abs() + (g - luma_in).abs() + (b - luma_in).abs();
    let chroma_out = (r_out - luma_out).abs() + (g_out - luma_out).abs() + (b_out - luma_out).abs();
    let chroma_gain = chroma_out / chroma_in;

    assert!(
        chroma_gain <= 1.40,
        "Chroma gain ({:.2}x) must not exceed perceptual threshold (prevents color explosion)",
        chroma_gain
    );

    // Red channel must not blow out to clipping
    assert!(r_out < 0.50, "Red channel ({:.3}) must not clip into saturated neon pink", r_out);
}

