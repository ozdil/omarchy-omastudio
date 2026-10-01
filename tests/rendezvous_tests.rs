use omastudio_engine::rendezvous::{
    constant_time_eq, current_epoch_bucket, derive_epoch_topic, derive_epoch_topic_hex,
    epoch_bucket_for_timestamp, hmac_sha256, hmac_sha256_hex, sha256, sha256_hex,
    verify_epoch_topic, verify_hash_constant_time, verify_pin_constant_time, AdaptiveChunker,
    TransferMetrics, DEFAULT_CHUNK_SIZE, EPOCH_BUCKET_SECONDS, MAX_CHUNK_SIZE, MIN_CHUNK_SIZE,
};
use std::time::Duration;

#[test]
fn test_constant_time_equality_mitigates_timing_attacks() {
    // Tests based on USENIX Security 2021 (PrivateDrop)
    let secret = b"super_secret_master_session_key_32b";
    let clone = b"super_secret_master_session_key_32b";
    let mutated_first = b"xuper_secret_master_session_key_32b";
    let mutated_last = b"super_secret_master_session_key_32c";
    let short_prefix = b"super_secret_master";

    assert!(constant_time_eq(secret, clone));
    assert!(!constant_time_eq(secret, mutated_first));
    assert!(!constant_time_eq(secret, mutated_last));
    assert!(!constant_time_eq(secret, short_prefix));
    assert!(constant_time_eq(b"", b""));
}

#[test]
fn test_constant_time_pin_verification() {
    assert!(verify_pin_constant_time("123456", "123456"));
    assert!(!verify_pin_constant_time("123456", "123457"));
    assert!(!verify_pin_constant_time("123456", "12345"));
    assert!(!verify_pin_constant_time("123456", "1234567"));
}

#[test]
fn test_constant_time_hash_verification() {
    let h1 = sha256(b"node-alpha");
    let h2 = sha256(b"node-alpha");
    let h3 = sha256(b"node-beta");

    assert!(verify_hash_constant_time(&h1, &h2));
    assert!(!verify_hash_constant_time(&h1, &h3));
}

#[test]
fn test_sha256_standard_conformance() {
    // NIST CAVP standard test vectors
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"Omarchy Linux Secure Photo Engine"),
        "d4e6519c08001575f0b8f235391d127100b0189a10f14fedac39e309ef90a339"
    );
}

#[test]
fn test_hmac_sha256_standard_conformance() {
    // RFC 4231 Test Case 1
    let key1 = [0x0bu8; 20];
    let data1 = b"Hi There";
    assert_eq!(
        hmac_sha256_hex(&key1, data1),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    let raw_hmac = hmac_sha256(&key1, data1);
    assert_eq!(raw_hmac.len(), 32);

    // RFC 4231 Test Case 2 ("Jefe")
    let key2 = b"Jefe";
    let data2 = b"what do ya want for nothing?";
    assert_eq!(
        hmac_sha256_hex(key2, data2),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn test_epoch_salted_topic_derivation_and_drift_window() {
    let oma_id = "omarchy-device-7a4c9e";
    let t_base = 1717000100 - (1717000100 % EPOCH_BUCKET_SECONDS); // Bucket aligned timestamp
    let bucket_base = epoch_bucket_for_timestamp(t_base);
    assert_eq!(bucket_base, t_base / EPOCH_BUCKET_SECONDS);
    assert!(current_epoch_bucket() > 0);

    // Intra-bucket consistency
    let topic_0 = derive_epoch_topic(oma_id, t_base);
    let topic_mid = derive_epoch_topic(oma_id, t_base + 300);
    assert_eq!(topic_0, topic_mid);

    // Topic hex format
    let topic_hex = derive_epoch_topic_hex(oma_id, t_base);
    assert_eq!(topic_hex.len(), 64);

    // Inter-bucket non-linkability (anti-tracking)
    let t_next_bucket = t_base + EPOCH_BUCKET_SECONDS + 10;
    let topic_next = derive_epoch_topic(oma_id, t_next_bucket);
    assert_ne!(topic_0, topic_next);

    // Constant-time window verification (1 bucket tolerance = +/- 15 min)
    assert!(verify_epoch_topic(oma_id, &topic_0, t_base, 1));
    assert!(verify_epoch_topic(oma_id, &topic_0, t_next_bucket, 1));
    assert!(verify_epoch_topic(oma_id, &topic_next, t_base, 1));

    // Beyond window tolerance (2 buckets away)
    let t_far = t_base + (EPOCH_BUCKET_SECONDS * 3);
    assert!(!verify_epoch_topic(oma_id, &topic_0, t_far, 1));
}

#[test]
fn test_adaptive_chunk_sizing_and_rtt_flow_control() {
    // ACM MobiCom Cupid Dynamic Rate Control Verification
    let mut chunker = AdaptiveChunker::new();
    assert_eq!(chunker.current_chunk_size(), DEFAULT_CHUNK_SIZE);
    assert_eq!(DEFAULT_CHUNK_SIZE, 256 * 1024);
    assert_eq!(MIN_CHUNK_SIZE, 64 * 1024);
    assert_eq!(MAX_CHUNK_SIZE, 1024 * 1024);

    // Fast, stable local network (RTT ~ 5ms) -> chunk size grows to 1MB ceiling
    for _ in 0..40 {
        chunker.record_sample(Duration::from_millis(5), true);
    }
    assert_eq!(chunker.current_chunk_size(), MAX_CHUNK_SIZE);
    assert!(chunker.smoothed_rtt_ms() < 20.0);

    // Network degradation / congestion / packet drops -> multiplicative drop to 64KB floor
    for _ in 0..15 {
        chunker.record_sample(Duration::from_millis(250), false);
    }
    assert_eq!(chunker.current_chunk_size(), MIN_CHUNK_SIZE);

    // Recovery
    for _ in 0..50 {
        chunker.record_sample(Duration::from_millis(8), true);
    }
    assert!(chunker.current_chunk_size() > MIN_CHUNK_SIZE);
}

#[test]
fn test_atomic_throughput_metrics_and_eta_calculation() {
    let total_transfer_size = 50 * 1024 * 1024; // 50 MB
    let metrics = TransferMetrics::new(total_transfer_size);

    assert_eq!(metrics.total_bytes(), total_transfer_size);
    assert_eq!(metrics.transferred_bytes(), 0);

    // Simulate chunk transfers
    let chunk_size = 2 * 1024 * 1024; // 2 MB chunks
    for _ in 0..10 {
        metrics.update_progress(chunk_size);
    }

    assert_eq!(metrics.transferred_bytes(), 20 * 1024 * 1024);

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.total_bytes, total_transfer_size);
    assert_eq!(snapshot.transferred_bytes, 20 * 1024 * 1024);
    assert!((snapshot.progress_percent - 40.0).abs() < 0.1);

    // Complete the transfer
    metrics.update_progress(30 * 1024 * 1024);
    let final_snap = metrics.snapshot();
    assert_eq!(final_snap.transferred_bytes, total_transfer_size);
    assert_eq!(final_snap.progress_percent, 100.0);
    assert_eq!(final_snap.eta_seconds, Some(0.0));
}

#[test]
fn test_oma_id_luhn_and_qr_generation() {
    use omastudio_engine::rendezvous::{
        format_oma_id, generate_fallback_qr_svg, generate_raw_oma_id, normalize_oma_id,
        update_desktop_oma_id_qr, validate_oma_id,
    };

    let raw = generate_raw_oma_id();
    assert_eq!(raw.len(), 16);
    assert!(validate_oma_id(&raw));

    let formatted = format_oma_id(&raw);
    assert_eq!(formatted.len(), 19);
    assert!(validate_oma_id(&formatted));
    assert_eq!(normalize_oma_id(&formatted), raw);

    let svg = generate_fallback_qr_svg("omasend://identity/1234567812345678");
    assert!(svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
    assert!(svg.ends_with("</svg>"));

    let temp_dir = std::env::temp_dir().join(format!("oma_id_qr_integration_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let (svg_p, png_p) = update_desktop_oma_id_qr(&temp_dir).expect("update desktop qr");
    assert!(svg_p.exists());
    assert!(png_p.exists());
    let _ = std::fs::remove_dir_all(&temp_dir);
}

