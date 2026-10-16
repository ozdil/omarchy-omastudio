//! Regression tests for bugs fixed in the full-stack review.

use omastudio_engine::pipeline::film_sim::FilmSimulation;
use omastudio_engine::recipe::Recipe;
use omastudio_engine::rendezvous::generate_fallback_qr_svg;
use omastudio_engine::security::{atomic_write_secure, ensure_secure_dir};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn partial_recipe_json_keeps_engine_defaults() {
    let r: Recipe = serde_json::from_str("{}").expect("empty recipe parses");
    assert_eq!(r, Recipe::default());
    assert!(r.highlight_reconstruct, "highlight reconstruction must default to on");
}

#[test]
fn partial_adjustment_layer_does_not_reject_recipe() {
    let json = r#"{"layers":[{"id":"l1","exposure":0.5}]}"#;
    let r: Recipe = serde_json::from_str(json).expect("layer with missing sliders parses");
    assert_eq!(r.layers.len(), 1);
}

#[test]
fn hncs_film_sim_stays_finite_above_white() {
    let sim = FilmSimulation::from_id("hasselblad_hncs");
    for v in [1.2f32, 1.5, -0.1] {
        let (r, g, b) = sim.apply_pixel(v, v, v, 1.0);
        assert!(r.is_finite() && g.is_finite() && b.is_finite(), "NaN for input {}", v);
    }
}

#[test]
fn fallback_qr_svg_renders_modules() {
    let svg = generate_fallback_qr_svg("omasend://identity/1234567812345678");
    // A real QR symbol for this payload has hundreds of dark modules
    assert!(svg.matches("<rect x=").count() > 200);
}

#[test]
fn atomic_write_leaves_existing_parent_permissions_alone() {
    let base = std::env::temp_dir().join(format!("oma_perm_regress_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let shared = base.join("shared");
    fs::create_dir_all(&shared).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();

    atomic_write_secure(&shared.join("out.jpg"), b"data").expect("write");

    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&base), 0o755, "ancestor permissions must be preserved");
    assert_eq!(mode(&shared), 0o755, "user folder permissions must be preserved");

    // Engine-owned directories are still created/tightened to 0700
    let private = shared.join("cache");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();
    ensure_secure_dir(&private).unwrap();
    assert_eq!(mode(&private), 0o700);

    let _ = fs::remove_dir_all(&base);
}
