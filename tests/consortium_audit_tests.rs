use omastudio_engine::pipeline::lut::Lut3D;
use omastudio_engine::pipeline::tone::{apply_agx_filmic_curve, apply_tone_pixel};
use omastudio_engine::recipe::Recipe;
use omastudio_engine::rendezvous::{constant_time_eq, verify_epoch_topic};
use omastudio_engine::security::read_secure_file;
use std::fs::File;
use std::io::Write;

#[test]
fn test_constant_time_eq_different_lengths() {
    let a = b"123456";
    let b = b"12345";
    let c = b"12345678";
    let d = b"123456";

    assert!(!constant_time_eq(a, b));
    assert!(!constant_time_eq(a, c));
    assert!(constant_time_eq(a, d));
    assert!(constant_time_eq(b"", b""));
}

#[test]
fn test_verify_epoch_topic_unbounded_tolerance_capped() {
    let oma_id = "test-peer-oma-001";
    let candidate = [0x5au8; 32];
    let now = 1774000000u64;

    // Tolerance u32::MAX would take 8.5 billion iterations without the security cap.
    // With safe_window = tolerance.min(8), this finishes in < 1 millisecond.
    let start = std::time::Instant::now();
    let verified = verify_epoch_topic(oma_id, &candidate, now, u32::MAX);
    let elapsed = start.elapsed();

    assert!(!verified);
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "verify_epoch_topic with u32::MAX tolerance must be capped and complete within 50ms, took {:?}",
        elapsed
    );
}

#[test]
fn test_color_boost_negative_channel_no_nan() {
    let recipe = Recipe {
        color_boost: 100.0,
        saturation: 50.0,
        ..Default::default()
    };

    // Extreme scenario where chroma subtraction produces negative channels
    let (r, g, b) = apply_tone_pixel(0.95, -0.20, -0.10, &recipe, 1.0);
    assert!(!r.is_nan(), "Red channel must not be NaN");
    assert!(!g.is_nan(), "Green channel must not be NaN");
    assert!(!b.is_nan(), "Blue channel must not be NaN");
    assert!(!r.is_infinite(), "Red channel must not be Inf");
    assert!(!g.is_infinite(), "Green channel must not be Inf");
    assert!(!b.is_infinite(), "Blue channel must not be Inf");
}

#[test]
fn test_agx_filmic_highlight_retains_high_luminance() {
    // With max_ev = 4.0 (linear 16.0), a high input should smoothly approach 1.0
    let v_low = apply_agx_filmic_curve(0.18); // 18% middle gray
    let v_high = apply_agx_filmic_curve(4.0); // +2 EV highlight
    let v_extreme = apply_agx_filmic_curve(16.0); // +4 EV highlight

    assert!(v_low > 0.40 && v_low < 0.70, "Middle gray should map to midtone");
    assert!(v_high > v_low, "Highlights must be strictly brighter than midtones");
    assert!(v_extreme > v_high, "Extreme highlights must continue monotonic ascent");
    assert!(v_extreme <= 1.0, "AgX must smoothly clamp to 1.0 without blowout");
}

#[test]
fn test_lut_sample_extreme_bounds_no_panic() {
    let lut = Lut3D::identity(17);

    // Boundaries exactly at 1.0, above 1.0, and negative values
    let (r1, g1, b1) = lut.sample(1.0, 1.0, 1.0);
    let (r2, g2, b2) = lut.sample(1.5, 2.0, 10.0);
    let (r3, g3, b3) = lut.sample(-0.5, -1.0, 0.0);

    assert_eq!((r1, g1, b1), (1.0, 1.0, 1.0));
    assert_eq!((r2, g2, b2), (1.0, 1.0, 1.0));
    assert_eq!((r3, g3, b3), (0.0, 0.0, 0.0));
}

#[test]
fn test_read_secure_file_bounds_and_symlinks() {
    let temp_dir = std::env::temp_dir().join(format!("omastudio_test_sec_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    // 1. Regular file within bounds
    let valid_file = temp_dir.join("valid.dat");
    {
        let mut f = File::create(&valid_file).unwrap();
        f.write_all(b"Hello Omarchy Secure Storage").unwrap();
    }
    let data = read_secure_file(&valid_file, 1024).unwrap();
    assert_eq!(data, b"Hello Omarchy Secure Storage");

    // 2. Oversized file exceeds limit
    let err_size = read_secure_file(&valid_file, 5);
    assert!(err_size.is_err(), "Must reject oversized file");

    // 3. Symlink rejection
    let symlink_file = temp_dir.join("symlink.dat");
    let _ = std::os::unix::fs::symlink(&valid_file, &symlink_file);
    let err_sym = read_secure_file(&symlink_file, 1024);
    assert!(err_sym.is_err(), "Must strictly reject symlinks");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
