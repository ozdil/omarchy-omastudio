use omastudio_engine::ai::{ai_auto_enhance, ai_classify_scene};
use omastudio_engine::pipeline::process_buffer;
use omastudio_engine::raw::RawMetadata;
use omastudio_engine::recipe::Recipe;
use omastudio_engine::security::*;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn test_security_atomic_write_and_mode_0600() {
    let temp_dir = PathBuf::from("/tmp/omaraw_test_sec_dir");
    let _ = fs::remove_dir_all(&temp_dir);

    ensure_secure_dir(&temp_dir).expect("Directory creation must succeed");

    let dir_meta = fs::metadata(&temp_dir).expect("Dir metadata");
    let dir_mode = dir_meta.permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700, "Directory must have Mode 0700");

    let test_file = temp_dir.join("test_recipe.omaraw");
    let content = b"{\"exposure\": 1.5, \"contrast\": 20.0}";

    atomic_write_secure(&test_file, content).expect("Atomic write must succeed");

    let file_meta = fs::metadata(&test_file).expect("File metadata");
    let file_mode = file_meta.permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600, "File must have strict Mode 0600 permissions");

    let read_back = fs::read(&test_file).expect("File read");
    assert_eq!(read_back, content);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_security_reject_symlink() {
    let temp_dir = PathBuf::from("/tmp/omaraw_test_symlink_dir");
    let _ = fs::remove_dir_all(&temp_dir);
    ensure_secure_dir(&temp_dir).expect("Dir creation");

    let real_file = temp_dir.join("real.omaraw");
    fs::write(&real_file, b"test").expect("Write real file");

    let symlink_path = temp_dir.join("symlink.omaraw");
    std::os::unix::fs::symlink(&real_file, &symlink_path).expect("Create symlink");

    // Must reject symlink
    let res = verify_safe_file(&symlink_path);
    assert!(res.is_err(), "Symlinks must be strictly rejected");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_subprocess_deadline_and_reap() {
    let mut cmd = secure_command("sh");
    cmd.arg("-c").arg("trap '' TERM; while true; do sleep 1; done");

    let start = std::time::Instant::now();
    let res = run_bounded_command(cmd, Duration::from_millis(150));
    let elapsed = start.elapsed();

    assert!(res.is_err(), "Subprocess exceeding deadline must return TimedOut error");
    assert!(elapsed < Duration::from_millis(500), "Subprocess must be terminated promptly");
}

#[test]
fn test_stream_to_file_byte_limit_enforcement() {
    let temp_dir = PathBuf::from("/tmp/omaraw_test_stream_limit");
    let _ = fs::remove_dir_all(&temp_dir);
    ensure_secure_dir(&temp_dir).expect("Dir creation");

    let staging_file_path = temp_dir.join("staging.tmp");
    let mut staging_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(true)
        .mode(0o600)
        .open(&staging_file_path)
        .expect("Create staging file");

    let mut cmd = secure_command("sh");
    cmd.arg("-c").arg("yes '0123456789abcdef'");

    let start = std::time::Instant::now();
    let res = run_bounded_command_stream_to_file(
        cmd,
        &mut staging_file,
        Duration::from_secs(5),
        2048, // 2 KiB hard cap
    );
    let elapsed = start.elapsed();

    assert!(res.is_err(), "Byte limit overrun must trigger an error");
    assert!(elapsed < Duration::from_secs(2), "Process group must be reaped promptly on byte limit overrun");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_verify_secure_open_file() {
    let temp_dir = PathBuf::from("/tmp/omaraw_test_open_file_sec");
    let _ = fs::remove_dir_all(&temp_dir);
    ensure_secure_dir(&temp_dir).expect("Dir creation");

    let valid_path = temp_dir.join("valid_0600.dat");
    let mut valid_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(true)
        .mode(0o600)
        .open(&valid_path)
        .expect("Create 0600 file");

    std::io::Write::write_all(&mut valid_file, b"test_payload_secure").expect("Write");
    valid_file.sync_all().expect("Sync");

    let size = verify_secure_open_file(&valid_file, 1024).expect("Mode 0600 owned file must pass verification");
    assert_eq!(size, 19);

    // Test size cap exceeded
    let cap_err = verify_secure_open_file(&valid_file, 10);
    assert!(cap_err.is_err(), "File exceeding byte limit must fail verification");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_gdrive_corrupted_cache_purging() {
    let cache_dir = omastudio_engine::gdrive::default_gdrive_cache_dir();
    let _ = ensure_secure_dir(&cache_dir);

    let corrupt_file = cache_dir.join("corrupt_test_image.nef");
    // Write 2000 bytes of non-RAW garbage to simulate partial download
    let garbage = vec![0x41u8; 2000];
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&corrupt_file)
        .expect("Create corrupt cache file");
    std::io::Write::write_all(&mut f, &garbage).expect("Write garbage");
    f.sync_all().expect("Sync");
    drop(f);

    assert!(corrupt_file.exists());

    // Calling fetch_remote_raw should detect that LibRaw cannot open this file,
    // purge it, and proceed to download (which will fail due to rclone gdrive remote not configured or nonexistent file, but crucial point is corrupt cache file is deleted)
    let res = omastudio_engine::gdrive::fetch_remote_raw("corrupt_test_image.nef");
    assert!(res.is_err(), "Fetching nonexistent remote file should fail");
    assert!(!corrupt_file.exists(), "Corrupted/partial cache file must be purged");
}

#[test]
fn test_recipe_roundtrip() {
    let r = Recipe::fuji_classic_chrome();
    let json = serde_json::to_string(&r).expect("Serialize");
    let r2: Recipe = serde_json::from_str(&json).expect("Deserialize");
    assert_eq!(r, r2);
    assert_eq!(r2.preset_name, Some("Fuji Classic Chrome".to_string()));
}

#[test]
fn test_pipeline_and_ai_auto_enhance() {
    // Generate 64x64 synthetic gradient RAW buffer (RGB)
    let width = 64;
    let height = 64;
    let mut buffer = vec![0u8; width * height * 3];

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            buffer[idx] = (x * 4) as u8;       // Red gradient
            buffer[idx + 1] = (y * 4) as u8;   // Green gradient
            buffer[idx + 2] = 120;             // Blue base
        }
    }

    let meta = RawMetadata {
        width: 64,
        height: 64,
        raw_width: 64,
        raw_height: 64,
        make: "FUJIFILM".to_string(),
        model: "X-T5".to_string(),
        lens: "XF 33mm F1.4 R LM WR".to_string(),
        iso: 800.0,
        shutter: 0.004,
        aperture: 2.0,
        focal_length: 33.0,
        timestamp: 1700000000,
        cam_mul: [1.0, 1.0, 1.0, 1.0],
    };

    let auto_recipe = ai_auto_enhance(&buffer, width as u32, height as u32, 3, &meta);
    assert!(auto_recipe.exposure.is_finite());
    assert!(auto_recipe.wb_temperature > 2000.0);

    let scene = ai_classify_scene(&buffer, width as u32, height as u32, 3, &meta);
    assert!(!scene.scene_type.is_empty());

    let (processed, hist) = process_buffer(&buffer, width as u32, height as u32, 3, &auto_recipe);
    assert_eq!(processed.len(), buffer.len());
    assert!(hist.max_count > 0);
}

#[test]
fn test_color_wheels_and_optics_pipeline() {
    let width = 32;
    let height = 32;
    let buffer = vec![128u8; width * height * 3]; // Neutral gray

    let mut recipe = Recipe::cinematic_teal_orange();
    recipe.lift = [-0.1, 0.05, 0.2]; // Boost blue/cyan in shadows
    recipe.gain = [0.2, 0.1, -0.1]; // Boost orange in highlights
    recipe.defringe = 50.0;
    recipe.crop_x = 0.1;
    recipe.crop_y = 0.1;
    recipe.crop_w = 0.8;
    recipe.crop_h = 0.8;

    let (processed, hist) = process_buffer(&buffer, width as u32, height as u32, 3, &recipe);
    assert_eq!(processed.len(), buffer.len());
    assert!(hist.max_count > 0);
}

#[test]
fn test_icc_profiles_listing() {
    let profiles = omastudio_engine::icc::list_icc_profiles();
    assert!(!profiles.is_empty(), "Standard ICC profiles must be available");
    assert!(profiles.iter().any(|p| p.id == "sRGB" || p.name.contains("sRGB")));
    assert!(profiles.iter().any(|p| p.id == "AdobeRGB1998" || p.name.contains("Adobe RGB")));
}

#[test]
fn test_ai_social_media_optimization() {
    let width = 64;
    let height = 64;
    let buffer = vec![140u8; width * height * 3];
    let recipe = Recipe::default();

    // Instagram Portrait 4:5
    let ig = omastudio_engine::ai::ai_optimize_for_social(&buffer, width as u32, height as u32, 3, "ig", &recipe);
    assert_eq!(ig.aspect_ratio_str, "4:5");
    assert_eq!(ig.max_dimension, 1350);
    assert_eq!(ig.color_profile, "sRGB");
    assert!(ig.recipe.sharpness > recipe.sharpness);

    // X / Twitter 16:9
    let x = omastudio_engine::ai::ai_optimize_for_social(&buffer, width as u32, height as u32, 3, "x", &recipe);
    assert_eq!(x.aspect_ratio_str, "16:9");
    assert_eq!(x.max_dimension, 1200);

    // Story 9:16
    let story = omastudio_engine::ai::ai_optimize_for_social(&buffer, width as u32, height as u32, 3, "story", &recipe);
    assert_eq!(story.aspect_ratio_str, "9:16");
    assert_eq!(story.max_dimension, 1920);
}

#[test]
fn test_secure_dir_hierarchy_rejects_symlinks() {
    let base_dir = PathBuf::from("/tmp/omaraw_test_hierarchy");
    let _ = fs::remove_dir_all(&base_dir);
    fs::create_dir_all(&base_dir).expect("Create base dir");

    let planted_target = base_dir.join("planted_target");
    fs::create_dir_all(&planted_target).expect("Create planted target");

    // Plant a symlink as intermediate component
    let intermediate_symlink = base_dir.join("gdrive_symlink");
    std::os::unix::fs::symlink(&planted_target, &intermediate_symlink).expect("Create intermediate symlink");

    // Target path goes through intermediate symlink
    let attacked_path = intermediate_symlink.join("subfolder");

    // open_or_create_hierarchy must refuse to traverse symlink (O_NOFOLLOW / PermissionDenied)
    let res = SecureDir::open_or_create_hierarchy(&attacked_path);
    assert!(res.is_err(), "SecureDir::open_or_create_hierarchy must strictly reject following symlink hierarchy");

    let _ = fs::remove_dir_all(&base_dir);
}

#[test]
fn test_preview_dir_ownership_and_atomic_write() {
    let preview_dir = get_secure_preview_dir();
    assert!(preview_dir.exists(), "Preview directory must exist");
    let meta = fs::symlink_metadata(&preview_dir).expect("Preview dir metadata");
    assert!(!meta.file_type().is_symlink(), "Preview dir must not be a symlink");
    let current_uid = unsafe { libc::getuid() };
    assert_eq!(std::os::unix::fs::MetadataExt::uid(&meta), current_uid, "Preview dir must be owned by current user");

    let test_ppm = preview_dir.join("test_secure_preview.ppm");
    let dummy_ppm = b"P6\n2 2\n255\n\xff\x00\x00\x00\xff\x00\x00\x00\xff\xff\xff\xff";
    atomic_write_secure(&test_ppm, dummy_ppm).expect("Write secure preview PPM");

    let file_meta = fs::symlink_metadata(&test_ppm).expect("PPM metadata");
    assert!(!file_meta.file_type().is_symlink(), "Preview file must not be a symlink");
    let file_mode = file_meta.permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600, "Preview file must have mode 0600");

    let _ = fs::remove_file(test_ppm);
}

#[test]
fn test_watermark_rejects_symlink_logo() {
    use omastudio_engine::export::watermark::{apply_watermark, WatermarkOptions};

    let temp_dir = PathBuf::from("/tmp/omaraw_test_watermark_sec");
    let _ = fs::remove_dir_all(&temp_dir);
    ensure_secure_dir(&temp_dir).expect("Ensure temp dir");

    let real_logo = temp_dir.join("real_logo.png");
    let img_logo = image::RgbaImage::new(64, 64);
    img_logo.save(&real_logo).expect("Save real logo");

    let symlink_logo = temp_dir.join("symlink_logo.png");
    std::os::unix::fs::symlink(&real_logo, &symlink_logo).expect("Symlink logo");

    let mut target_img = image::DynamicImage::new_rgb8(200, 200);

    let opts = WatermarkOptions {
        enabled: true,
        watermark_type: "logo".to_string(),
        logo_path: Some(symlink_logo.to_string_lossy().to_string()),
        text: "SECURE TEST".to_string(),
        ..Default::default()
    };

    // apply_watermark must safely reject symlink and fallback to text watermark without crashing
    apply_watermark(&mut target_img, &opts, None);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_export_atomic_file_write_mode_and_symlink_refusal() {
    let temp_dir = PathBuf::from("/tmp/omaraw_test_export_atomic");
    let _ = fs::remove_dir_all(&temp_dir);
    ensure_secure_dir(&temp_dir).expect("Create export dir");

    let test_webp = temp_dir.join("test_out.webp");
    let dummy_data = b"RIFF....WEBP";
    atomic_write_secure(&test_webp, dummy_data).expect("Atomic write webp");

    let meta = fs::symlink_metadata(&test_webp).expect("Webp metadata");
    assert!(!meta.file_type().is_symlink(), "Export file must not be a symlink");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "Export file must have strict Mode 0600 permissions");

    let _ = fs::remove_dir_all(&temp_dir);
}

