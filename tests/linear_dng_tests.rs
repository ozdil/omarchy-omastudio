use omastudio_engine::raw::dng::{write_linear_rgb, DngInfo};
use omastudio_engine::raw::RawImage;

/// A linear (already demosaiced) DNG goes through OmaStudio's own develop path.
#[test]
fn linear_dng_is_developed_by_own_pipeline() {
    let dir = std::env::temp_dir().join(format!("omastudio-lindng-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("grey.dng");
    // LibRaw refuses images under 22 pixels a side
    let (w, h) = (64usize, 48usize);
    let rgb = vec![20000u16; w * h * 3];
    let info = DngInfo {
        make: "FUJIFILM",
        model: "GFX100S",
        unique_model: "FUJIFILM GFX100S",
        orientation: 1,
        color_matrix: [[1.7167, -0.3557, -0.2534], [-0.6667, 1.6165, 0.0158], [0.0176, -0.0428, 0.9421]],
        lens: None,
    };
    write_linear_rgb(&path, &rgb, w, h, &info).unwrap();

    let img = RawImage::open(&path).unwrap();
    let frame = img.linear_frame().expect("linear DNG yields a linear frame");
    assert_eq!((frame.width, frame.height, frame.channels), (w, h, 3));
    let buf = img.process_custom(false, 0, 16).unwrap();
    assert_eq!((buf.width as usize, buf.height as usize, buf.channels), (w, h, 3u32));
    std::fs::remove_dir_all(&dir).unwrap();
}
