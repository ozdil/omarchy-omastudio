use image::{DynamicImage, ImageBuffer, Rgb};
use omastudio_engine::export::watermark::{apply_watermark, resolve_watermark_text, WatermarkOptions};
use omastudio_engine::pipeline::aces::{
    acesccg_to_linear, apply_aces_gamut_compression, apply_aces_tonemap, linear_to_acescg,
    WorkingColorSpace,
};
use omastudio_engine::pipeline::film_sim::FilmSimulation;
use omastudio_engine::raw::RawMetadata;

#[test]
fn test_aces_color_space_transforms_and_stability() {
    let spaces = [
        WorkingColorSpace::Srgb,
        WorkingColorSpace::DisplayP3,
        WorkingColorSpace::Rec2020,
        WorkingColorSpace::AcesCg,
    ];

    let test_rgbs = [
        (0.18f32, 0.18f32, 0.18f32),
        (0.95f32, 0.05f32, 0.05f32),
        (0.05f32, 0.90f32, 0.10f32),
        (0.02f32, 0.08f32, 0.85f32),
        (0.65f32, 0.55f32, 0.40f32), // Skin tone region
    ];

    for &space in &spaces {
        for &(r, g, b) in &test_rgbs {
            let (ar, ag, ab) = linear_to_acescg(r, g, b, space);
            assert!(ar.is_finite() && ag.is_finite() && ab.is_finite());

            let (r2, g2, b2) = acesccg_to_linear(ar, ag, ab, space);
            assert!(r2.is_finite() && g2.is_finite() && b2.is_finite());

            assert!(
                (r - r2).abs() < 1e-3,
                "Space {:?} failed r roundtrip: {} vs {}",
                space,
                r,
                r2
            );
            assert!(
                (g - g2).abs() < 1e-3,
                "Space {:?} failed g roundtrip: {} vs {}",
                space,
                g,
                g2
            );
            assert!(
                (b - b2).abs() < 1e-3,
                "Space {:?} failed b roundtrip: {} vs {}",
                space,
                b,
                b2
            );
        }
    }
}

#[test]
fn test_aces_gamut_compression_handles_extreme_saturation() {
    let extreme_blue = (0.01f32, 0.05f32, 5.0f32);
    let (cr, cg, cb) = apply_aces_gamut_compression(extreme_blue.0, extreme_blue.1, extreme_blue.2);
    assert!(cr.is_finite() && cg.is_finite() && cb.is_finite());
    assert!(cb <= extreme_blue.2, "Gamut compression should roll off peak saturation");
    assert!(cr >= 0.0 && cg >= 0.0);

    let extreme_red = (12.0f32, 0.02f32, 0.01f32);
    let (cr2, cg2, cb2) = apply_aces_gamut_compression(extreme_red.0, extreme_red.1, extreme_red.2);
    assert!(cr2.is_finite() && cg2.is_finite() && cb2.is_finite());
    assert!(cr2 <= extreme_red.0);
}

#[test]
fn test_aces_rrt_odt_tonemapping_monotonicity() {
    let mut prev = -1.0f32;
    let mut val = 0.0f32;
    while val <= 20.0 {
        let mapped = apply_aces_tonemap(val);
        assert!(mapped.is_finite());
        assert!(mapped >= prev, "Tonemap curve must be monotonic: {} vs {}", mapped, prev);
        assert!(mapped <= 1.05, "Tonemap must asymptotically limit highlights near 1.0");
        prev = mapped;
        val += 0.05;
    }
}

#[test]
fn test_film_simulations_all_variants_produce_valid_linear_output() {
    let sims = [
        FilmSimulation::None,
        FilmSimulation::FujiProvia,
        FilmSimulation::FujiVelvia,
        FilmSimulation::FujiAstia,
        FilmSimulation::FujiClassicChrome,
        FilmSimulation::FujiClassicNeg,
        FilmSimulation::FujiEterna,
        FilmSimulation::FujiAcrosStandard,
        FilmSimulation::FujiAcrosYellow,
        FilmSimulation::FujiAcrosRed,
        FilmSimulation::FujiAcrosGreen,
        FilmSimulation::HasselbladHncs,
        FilmSimulation::HasselbladXPan,
    ];

    let skin_tone = (0.78f32, 0.58f32, 0.44f32);

    for sim in &sims {
        let (sr, sg, sb) = sim.apply_pixel(skin_tone.0, skin_tone.1, skin_tone.2, 0.75);
        assert!(sr.is_finite() && sg.is_finite() && sb.is_finite());
        assert!((0.0..=1.0).contains(&sr));
        assert!((0.0..=1.0).contains(&sg));
        assert!((0.0..=1.0).contains(&sb));

        if sim.is_monochrome() {
            let (mr, mg, mb) = sim.apply_pixel(skin_tone.0, skin_tone.1, skin_tone.2, 1.0);
            assert!((mr - mg).abs() < 1e-4, "Monochrome simulation must have identical channels at 100% intensity");
            assert!((mg - mb).abs() < 1e-4);
        }
    }
}

#[test]
fn test_watermark_integration_and_9_point_placement() {
    let meta = RawMetadata {
        width: 1920,
        height: 1080,
        raw_width: 1920,
        raw_height: 1080,
        make: "Fujifilm".to_string(),
        model: "GFX 100 II".to_string(),
        lens: "GF 45mm F2.8".to_string(),
        iso: 100.0,
        shutter: 0.004, // 1/250s
        aperture: 2.8,
        focal_length: 45.0,
        timestamp: 1774900000,
        cam_mul: [1.0, 1.0, 1.0, 1.0],
    };

    let template = "{camera} | {lens} | {aperture} | {shutter} | {iso}";
    let resolved = resolve_watermark_text(template, Some(&meta));
    assert!(resolved.contains("Fujifilm GFX 100 II"));
    assert!(resolved.contains("GF 45mm F2.8"));
    assert!(resolved.contains("f/2.8"));
    assert!(resolved.contains("1/250s"));
    assert!(resolved.contains("ISO 100"));

    // Test 9-point grid placement on 8-bit image
    for pos in 0..9 {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(400, 300);
        let mut dyn_img = DynamicImage::ImageRgb8(img);
        let opts = WatermarkOptions {
            enabled: true,
            watermark_type: "text".to_string(),
            text: resolved.clone(),
            logo_path: None,
            position_index: pos,
            opacity: 0.9,
            size: 10,
            margin: 16,
            color: "#ffffff".to_string(),
            drop_shadow: true,
        };

        apply_watermark(&mut dyn_img, &opts, Some(&meta));
        // Verify image has non-zero pixels
        let rgb = dyn_img.to_rgb8();
        let white_pixels = rgb.pixels().filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200).count();
        assert!(
            white_pixels > 20,
            "Watermark at position {} must render visible white text pixels: got {}",
            pos,
            white_pixels
        );
    }
}
