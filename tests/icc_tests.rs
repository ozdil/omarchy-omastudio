//! Colour management: built-in ICC profiles, lcms2 conversion and ICC embedding on export.

use image::ImageDecoder;
use lcms2::{ColorSpaceSignature, InfoType, Locale, Profile};
use omastudio_engine::export::{export_photo, ExportOptions};
use omastudio_engine::icc::{
    builtin_profile, builtin_space, convert_rgb16, list_icc_profiles, RenderingIntent, BUILTIN_PROFILES,
};
use omastudio_engine::raw::OutputColorSpace;
use omastudio_engine::recipe::Recipe;

#[test]
fn builtin_profiles_round_trip_as_rgb_profiles() {
    for (id, space) in BUILTIN_PROFILES {
        let bytes = builtin_profile(space).icc().expect("serialises");
        let parsed = Profile::new_icc(&bytes).expect("parses");
        assert_eq!(parsed.color_space(), ColorSpaceSignature::RgbData, "{id}");
        let desc = parsed.info(InfoType::Description, Locale::none()).unwrap_or_default();
        assert!(desc.contains("OmaStudio"), "{id}: {desc}");
    }
}

#[test]
fn only_exact_names_map_to_builtin_spaces() {
    assert_eq!(builtin_space("sRGB"), Some(OutputColorSpace::Srgb));
    assert_eq!(builtin_space("Display P3"), Some(OutputColorSpace::DisplayP3));
    assert_eq!(builtin_space("AdobeRGB1998"), Some(OutputColorSpace::AdobeRgb));
    assert_eq!(builtin_space("ProPhotoRGB"), Some(OutputColorSpace::ProPhotoRgb));
    assert_eq!(builtin_space("rec2020.icc"), Some(OutputColorSpace::Rec2020));
    // Different spaces that merely contain "srgb" must not become sRGB
    assert_eq!(builtin_space("scRGB_Linear"), None);
    assert_eq!(builtin_space("e-sRGB"), None);
}

#[test]
fn builtin_profiles_are_always_listed() {
    let ids: Vec<String> = list_icc_profiles().into_iter().map(|p| p.id).collect();
    for (id, _) in BUILTIN_PROFILES {
        assert!(ids.iter().any(|i| i == id), "{id} missing from {ids:?}");
    }
}

#[test]
fn srgb_green_lands_on_known_adobe_rgb_values() {
    // sRGB (0, 255, 0) is (144, 255, 60) in Adobe RGB (1998)
    let mut px = vec![0u16, 65535, 0];
    let srgb = builtin_profile(OutputColorSpace::Srgb);
    let adobe = builtin_profile(OutputColorSpace::AdobeRgb);
    convert_rgb16(&mut px, &srgb, &adobe, RenderingIntent::RelativeColorimetric).unwrap();
    let px8: Vec<i32> = px.iter().map(|&v| (v as f32 / 257.0).round() as i32).collect();
    for (got, want) in px8.iter().zip([144, 255, 60]) {
        assert!((got - want).abs() <= 2, "{px8:?}");
    }
}

#[test]
fn conversion_keeps_neutrals_and_round_trips() {
    let srgb = builtin_profile(OutputColorSpace::Srgb);
    let prophoto = builtin_profile(OutputColorSpace::ProPhotoRgb);
    let original: Vec<u16> = vec![65535, 65535, 65535, 0, 0, 0, 30000, 30000, 30000, 50000, 12000, 8000];
    let mut px = original.clone();
    convert_rgb16(&mut px, &srgb, &prophoto, RenderingIntent::RelativeColorimetric).unwrap();
    // White and black stay neutral despite the D65 -> D50 white point change
    assert!(px[..3].iter().all(|&v| v >= 65400), "{:?}", &px[..3]);
    assert!(px[3..6].iter().all(|&v| v <= 100), "{:?}", &px[3..6]);
    convert_rgb16(&mut px, &prophoto, &srgb, RenderingIntent::RelativeColorimetric).unwrap();
    for (got, want) in px.iter().zip(&original) {
        assert!((*got as i32 - *want as i32).abs() <= 200, "{px:?} vs {original:?}");
    }
}

fn exported_icc(format: &str, profile: &str) -> (Vec<u8>, image::DynamicImage) {
    let dir = std::env::temp_dir().join(format!("omastudio_icc_{}_{}_{}", std::process::id(), format, profile));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("source.png");
    let img = image::RgbImage::from_fn(16, 8, |x, y| image::Rgb([(x * 16) as u8, (y * 32) as u8, 200]));
    img.save(&src).unwrap();
    let mut opts = ExportOptions {
        format: format.to_string(),
        output_dir: dir.to_string_lossy().to_string(),
        preserve_exif: false,
        icc_profile: Some(profile.to_string()),
        ..Default::default()
    };
    // Lossy WebP needs cwebp; the built-in lossless encoder covers the ICC check
    if format == "webp" {
        opts.quality = 100;
    }
    let out = export_photo(&src, &Recipe::default(), &opts).expect("export succeeds");
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "{format}: exports are readable by others");
    }
    let file = std::fs::read(&out).unwrap();
    let reader = image::ImageReader::open(&out).unwrap().with_guessed_format().unwrap();
    let mut decoder = reader.into_decoder().unwrap();
    // image's TIFF decoder skips ICC tags written as BYTE; find the profile in the file instead.
    // The 128-byte header holds the creation time, which differs when the export and this
    // lookup fall in different seconds, so match the tag table and data after it.
    let icc = decoder.icc_profile().unwrap().or_else(|| {
        let space = builtin_space(profile)?;
        let bytes = builtin_profile(space).icc().ok()?;
        let body = &bytes[128..];
        let at = file.windows(body.len()).position(|w| w == body)?.checked_sub(128)?;
        Some(file[at..at + bytes.len()].to_vec())
    });
    let icc = icc.unwrap_or_else(|| panic!("ICC embedded in {format}"));
    let pixels = image::DynamicImage::from_decoder(decoder).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (icc, pixels)
}

#[test]
fn exports_embed_the_requested_profile_without_exiftool() {
    for format in ["png", "jpeg", "tiff", "webp"] {
        let (icc, _) = exported_icc(format, "DisplayP3");
        let desc = Profile::new_icc(&icc)
            .unwrap()
            .info(InfoType::Description, Locale::none())
            .unwrap_or_default();
        assert!(desc.contains("Display P3"), "{format}: {desc}");
    }
}

#[test]
fn raster_sources_are_converted_into_wide_gamut_exports() {
    // A raster source decodes as sRGB, so a ProPhoto export must be converted, not relabelled:
    // saturated sRGB colours move towards the centre of the much larger ProPhoto gamut.
    let (_, srgb) = exported_icc("png", "sRGB");
    let (_, prophoto) = exported_icc("png", "ProPhotoRGB");
    let a = srgb.to_rgb16();
    let b = prophoto.to_rgb16();
    let (pa, pb) = (a.get_pixel(0, 0), b.get_pixel(0, 0));
    assert_ne!(pa, pb);
    // Blue-dominant pixel: ProPhoto blue channel is lower than sRGB's
    assert!(pb[2] < pa[2], "{pa:?} -> {pb:?}");
}

#[test]
fn custom_icc_files_are_used_and_non_rgb_profiles_rejected() {
    use omastudio_engine::icc::ExportProfile;
    let dir = std::env::temp_dir().join(format!("omastudio_icc_custom_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let rgb = dir.join("MyPrinter.icc");
    std::fs::write(&rgb, builtin_profile(OutputColorSpace::AdobeRgb).icc().unwrap()).unwrap();
    let resolved = ExportProfile::resolve(&rgb.to_string_lossy()).expect("RGB file accepted");
    assert!(matches!(resolved, ExportProfile::Custom { .. }));

    let lab = dir.join("Lab.icc");
    let lab_profile = Profile::new_lab4_context(lcms2::GlobalContext::new(), &lcms2::CIExyY { x: 0.3457, y: 0.3585, Y: 1.0 }).unwrap();
    std::fs::write(&lab, lab_profile.icc().unwrap()).unwrap();
    let err = ExportProfile::resolve(&lab.to_string_lossy()).err().expect("Lab profile rejected");
    assert!(err.contains("not an RGB profile"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}
