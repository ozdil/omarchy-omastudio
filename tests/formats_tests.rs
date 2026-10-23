//! The file lists in the interface must match what the engine opens.

use omastudio_engine::raw::{raster::RASTER_EXTENSIONS, RAW_EXTENSIONS};

fn qml_list(source: &str, property: &str) -> Vec<String> {
    let start = source.find(&format!("readonly property var {}: [", property)).expect("property present");
    let rest = &source[start..];
    let list = &rest[rest.find('[').unwrap() + 1..rest.find(']').unwrap()];
    list.split(',').map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect()
}

#[test]
fn interface_lists_match_engine_lists() {
    let qml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/app/qml/MainWindow.qml")).unwrap();
    assert_eq!(qml_list(&qml, "rawExtensions"), RAW_EXTENSIONS);
    assert_eq!(qml_list(&qml, "imageExtensions"), RASTER_EXTENSIONS);
}

#[test]
fn exr_export_opens_again() {
    use omastudio_engine::export::{export_photo, ExportOptions};
    use omastudio_engine::raw::RawImage;
    use omastudio_engine::recipe::Recipe;
    let dir = std::env::temp_dir().join(format!("omastudio_fmt_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("grey.png");
    image::RgbImage::from_pixel(8, 8, image::Rgb([128, 128, 128])).save(&src).unwrap();
    for (format, depth) in [("exr", 16u8), ("exr", 32), ("tiff", 16), ("png", 16)] {
        let opts = ExportOptions {
            format: format.to_string(),
            bit_depth: depth,
            output_dir: dir.to_string_lossy().to_string(),
            preserve_exif: false,
            ..Default::default()
        };
        let out = export_photo(&src, &Recipe::default(), &opts).expect("export");
        let img = RawImage::open(&out).expect("opens again");
        let buf = img.process_custom(false, 0, 8).expect("decodes");
        let px = buf.as_slice()[0];
        // Mid grey survives the linear (EXR) or display-encoded round trip
        assert!((px as i32 - 128).abs() <= 6, "{format}/{depth}: {px}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
