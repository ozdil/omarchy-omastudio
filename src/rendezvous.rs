//! Rendezvous and Secure Transfer Protocol for Omarchy Linux (OmaStudio Engine)
//!
//! Architectural foundations:
//! - USENIX Security 2021 (PrivateDrop): Salted hash topic derivation preventing static hash leaks,
//!   constant-time cryptographic comparisons mitigating timing side-channel attacks.
//! - ACM MobiCom (Cupid & Dynamic Rate Control): Adaptive chunk sizing (64 KiB - 1 MiB)
//!   and RTT-based congestion & flow control over volatile Wi-Fi / LAN channels.

use serde::{Deserialize, Serialize};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Minimum chunk size (64 KiB) for degraded / noisy network conditions
pub const MIN_CHUNK_SIZE: usize = 64 * 1024;

/// Maximum chunk size (1 MiB) for gigabit local networks & high-throughput links
pub const MAX_CHUNK_SIZE: usize = 1024 * 1024;

/// Default initial chunk size (256 KiB)
pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;

/// Epoch bucket duration: 15 minutes (900 seconds)
pub const EPOCH_BUCKET_SECONDS: u64 = 900;

/// Domain separator prefix for blind topic derivation
pub const TOPIC_DOMAIN_PREFIX: &str = "epoch-v1";

// ============================================================================
// 1. Constant-Time Comparisons (Timing Side-Channel Protection)
// ============================================================================

/// Compares two byte slices in constant time to prevent timing-attack leakage.
///
/// Implements full-buffer XOR accumulation over maximum length without early termination.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let len_a = a.len();
    let len_b = b.len();

    // Constant-time length comparison
    let mut diff = len_a ^ len_b;

    // Fixed-length walk to avoid leaking buffer lengths via execution time
    let max_len = len_a.max(len_b);

    for i in 0..max_len {
        let byte_a = if i < len_a { a[i] } else { 0 };
        let byte_b = if i < len_b { b[i] } else { 0 };
        diff |= (byte_a ^ byte_b) as usize;
    }

    // Force compiler not to optimize the loop away
    black_box(diff) == 0
}

/// Constant-time comparison for PIN strings (e.g. 6-digit pairing PINs).
pub fn verify_pin_constant_time(expected_pin: &str, candidate_pin: &str) -> bool {
    constant_time_eq(expected_pin.as_bytes(), candidate_pin.as_bytes())
}

/// Constant-time comparison for 32-byte cryptographic hashes / HMAC digests.
pub fn verify_hash_constant_time(expected: &[u8; 32], candidate: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= expected[i] ^ candidate[i];
    }
    black_box(diff) == 0
}

// ============================================================================
// 2. Cryptographic SHA-256 & HMAC-SHA256 (FIPS 180-4 & RFC 2104)
// ============================================================================

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

#[inline(always)]
fn sha256_ch(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (!x & z)
}

#[inline(always)]
fn sha256_maj(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

#[inline(always)]
fn sha256_big_sigma0(x: u32) -> u32 {
    x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
}

#[inline(always)]
fn sha256_big_sigma1(x: u32) -> u32 {
    x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
}

#[inline(always)]
fn sha256_small_sigma0(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

#[inline(always)]
fn sha256_small_sigma1(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

fn sha256_process_block(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        w[i] = sha256_small_sigma1(w[i - 2])
            .wrapping_add(w[i - 7])
            .wrapping_add(sha256_small_sigma0(w[i - 15]))
            .wrapping_add(w[i - 16]);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for i in 0..64 {
        let t1 = h
            .wrapping_add(sha256_big_sigma1(e))
            .wrapping_add(sha256_ch(e, f, g))
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let t2 = sha256_big_sigma0(a).wrapping_add(sha256_maj(a, b, c));

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// Computes the standard SHA-256 digest of input bytes.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let total_len = data.len();
    let mut offset = 0;
    let mut block = [0u8; 64];

    while offset + 64 <= total_len {
        block.copy_from_slice(&data[offset..offset + 64]);
        sha256_process_block(&mut state, &block);
        offset += 64;
    }

    let rem = total_len - offset;
    block[..rem].copy_from_slice(&data[offset..]);
    block[rem] = 0x80;
    block[(rem + 1)..64].fill(0);

    if rem >= 56 {
        sha256_process_block(&mut state, &block);
        block = [0u8; 64];
    }

    let bit_len = (total_len as u64) * 8;
    block[56..64].copy_from_slice(&bit_len.to_be_bytes());
    sha256_process_block(&mut state, &block);

    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Returns SHA-256 digest as a lower-case hexadecimal string.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256(data);
    digest_to_hex(&digest)
}

/// Computes HMAC-SHA256 (RFC 2104) given a key and message payload.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let key_hash = sha256(key);
        k_block[..32].copy_from_slice(&key_hash);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }

    let mut i_pad = [0u8; 64];
    let mut o_pad = [0u8; 64];
    for i in 0..64 {
        i_pad[i] = k_block[i] ^ 0x36;
        o_pad[i] = k_block[i] ^ 0x5c;
    }

    let mut inner_data = Vec::with_capacity(64 + message.len());
    inner_data.extend_from_slice(&i_pad);
    inner_data.extend_from_slice(message);
    let inner_hash = sha256(&inner_data);

    let mut outer_data = Vec::with_capacity(64 + 32);
    outer_data.extend_from_slice(&o_pad);
    outer_data.extend_from_slice(&inner_hash);
    sha256(&outer_data)
}

/// Returns HMAC-SHA256 digest as a lower-case hexadecimal string.
pub fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let digest = hmac_sha256(key, message);
    digest_to_hex(&digest)
}

fn digest_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

// ============================================================================
// 3. Epoch-Salted Rendezvous Topic Derivation (PrivateDrop Pattern)
// ============================================================================

/// Calculates the 15-minute epoch bucket index for a given UNIX timestamp.
pub fn epoch_bucket_for_timestamp(unix_timestamp: u64) -> u64 {
    unix_timestamp / EPOCH_BUCKET_SECONDS
}

/// Returns the current system 15-minute epoch bucket index.
pub fn current_epoch_bucket() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    epoch_bucket_for_timestamp(now)
}

/// Derives a time-shifted, salt-blinded rendezvous topic:
/// `HMAC-SHA256(OmaID, "epoch-v1" || current_15min_bucket)`
///
/// Prevents global tracking and static hash harvesting as proven in PrivateDrop (USENIX 2021).
pub fn derive_epoch_topic_for_bucket(oma_id: &str, bucket: u64) -> [u8; 32] {
    let mut message = Vec::with_capacity(TOPIC_DOMAIN_PREFIX.len() + 8);
    message.extend_from_slice(TOPIC_DOMAIN_PREFIX.as_bytes());
    message.extend_from_slice(&bucket.to_be_bytes());
    hmac_sha256(oma_id.as_bytes(), &message)
}

/// Derives the rendezvous topic for the specified timestamp.
pub fn derive_epoch_topic(oma_id: &str, unix_timestamp: u64) -> [u8; 32] {
    let bucket = epoch_bucket_for_timestamp(unix_timestamp);
    derive_epoch_topic_for_bucket(oma_id, bucket)
}

/// Derives the hexadecimal representation of the rendezvous topic for the specified timestamp.
pub fn derive_epoch_topic_hex(oma_id: &str, unix_timestamp: u64) -> String {
    let topic = derive_epoch_topic(oma_id, unix_timestamp);
    digest_to_hex(&topic)
}

/// Verifies a candidate topic against the current time with clock skew window tolerance.
///
/// Tolerates `[-window_buckets, +window_buckets]` (default 1 = +/-15 min window) using constant-time comparison.
pub fn verify_epoch_topic(
    oma_id: &str,
    candidate_topic: &[u8; 32],
    unix_timestamp: u64,
    window_buckets: u32,
) -> bool {
    // HANCORE Security: Cap window_buckets to 8 (max 2 hours) to prevent algorithmic CPU DoS
    let safe_window = (window_buckets.min(8)) as u64;
    let current_bucket = epoch_bucket_for_timestamp(unix_timestamp);
    let min_bucket = current_bucket.saturating_sub(safe_window);
    let max_bucket = current_bucket.saturating_add(safe_window);

    let mut matched = false;
    for bucket in min_bucket..=max_bucket {
        let expected = derive_epoch_topic_for_bucket(oma_id, bucket);
        if verify_hash_constant_time(&expected, candidate_topic) {
            matched = true;
        }
    }
    matched
}

// ============================================================================
// 4. Adaptive Chunking & Flow Control (ACM MobiCom Cupid Model)
// ============================================================================

/// Dynamic rate and chunk sizing controller based on RTT and link jitter.
#[derive(Debug, Clone)]
pub struct AdaptiveChunker {
    current_size: usize,
    smoothed_rtt_ms: f64,
    rtt_variance_ms: f64,
    consecutive_successes: u32,
    consecutive_losses: u32,
}

impl Default for AdaptiveChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveChunker {
    /// Creates a new AdaptiveChunker with default 256 KiB chunk size.
    pub fn new() -> Self {
        Self {
            current_size: DEFAULT_CHUNK_SIZE,
            smoothed_rtt_ms: 20.0,
            rtt_variance_ms: 5.0,
            consecutive_successes: 0,
            consecutive_losses: 0,
        }
    }

    /// Current adaptive chunk size bounded strictly between MIN_CHUNK_SIZE and MAX_CHUNK_SIZE.
    pub fn current_chunk_size(&self) -> usize {
        self.current_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE)
    }

    /// Smoothed RTT estimate in milliseconds.
    pub fn smoothed_rtt_ms(&self) -> f64 {
        self.smoothed_rtt_ms
    }

    /// Records transfer result and updates chunk sizing via Additive Increase / Multiplicative Decrease (AIMD).
    pub fn record_sample(&mut self, sample_rtt: Duration, success: bool) -> usize {
        let sample_ms = (sample_rtt.as_micros() as f64) / 1000.0;
        if !sample_ms.is_finite() {
            return self.current_chunk_size();
        }

        // Jacobson/Karels RTT estimation algorithm
        let rtt_err = sample_ms - self.smoothed_rtt_ms;
        self.smoothed_rtt_ms += 0.125 * rtt_err;
        self.rtt_variance_ms += 0.25 * (rtt_err.abs() - self.rtt_variance_ms);

        if success {
            self.consecutive_losses = 0;
            self.consecutive_successes += 1;

            // Favorable link condition: low RTT and stable jitter -> scale up chunk size
            if self.smoothed_rtt_ms < 30.0 && self.consecutive_successes >= 3 {
                let step = 64 * 1024; // 64 KiB additive step
                self.current_size = (self.current_size + step).min(MAX_CHUNK_SIZE);
                self.consecutive_successes = 0;
            } else if self.smoothed_rtt_ms < 60.0 && self.consecutive_successes >= 6 {
                let step = 32 * 1024;
                self.current_size = (self.current_size + step).min(MAX_CHUNK_SIZE);
                self.consecutive_successes = 0;
            }
        } else {
            self.consecutive_successes = 0;
            self.consecutive_losses += 1;

            // Degradation / timeout: Multiplicative decrease to avoid channel congestion
            self.current_size = (self.current_size / 2).max(MIN_CHUNK_SIZE);
        }

        self.current_chunk_size()
    }
}

// ============================================================================
// 5. Atomic Throughput Counter & ETA Calculator
// ============================================================================

/// Thread-safe real-time transfer metrics and progress tracker.
pub struct TransferMetrics {
    total_bytes: u64,
    transferred_bytes: AtomicU64,
    start_time: Instant,
    last_sample_time: Mutex<Instant>,
    last_sample_bytes: AtomicU64,
    smoothed_throughput_bytes_sec: Mutex<f64>,
}

/// Instantaneous progress snapshot suitable for IPC or UI rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgressSnapshot {
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub progress_percent: f64,
    pub current_throughput_mb_s: f64,
    pub average_throughput_mb_s: f64,
    pub eta_seconds: Option<f64>,
    pub elapsed_seconds: f64,
}

impl TransferMetrics {
    /// Initializes progress metrics for a file or batch of given byte size.
    pub fn new(total_bytes: u64) -> Self {
        let now = Instant::now();
        Self {
            total_bytes,
            transferred_bytes: AtomicU64::new(0),
            start_time: now,
            last_sample_time: Mutex::new(now),
            last_sample_bytes: AtomicU64::new(0),
            smoothed_throughput_bytes_sec: Mutex::new(0.0),
        }
    }

    /// Atomically increments transferred byte count and updates EMA throughput.
    pub fn update_progress(&self, chunk_bytes: usize) {
        self.transferred_bytes.fetch_add(chunk_bytes as u64, Ordering::Relaxed);

        let now = Instant::now();
        if let Ok(mut last_time) = self.last_sample_time.try_lock() {
            let elapsed = now.duration_since(*last_time).as_secs_f64();
            if elapsed >= 0.1 {
                let current_total = self.transferred_bytes.load(Ordering::Relaxed);
                let prev_bytes = self.last_sample_bytes.swap(current_total, Ordering::Relaxed);
                let delta_bytes = current_total.saturating_sub(prev_bytes) as f64;
                let instant_speed = delta_bytes / elapsed;

                if let Ok(mut speed_guard) = self.smoothed_throughput_bytes_sec.lock() {
                    if *speed_guard < 1.0 {
                        *speed_guard = instant_speed;
                    } else {
                        // Exponential Moving Average (alpha = 0.3)
                        *speed_guard = 0.7 * (*speed_guard) + 0.3 * instant_speed;
                    }
                }
                *last_time = now;
            }
        }
    }

    /// Total bytes in this transfer job.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Total transferred bytes so far.
    pub fn transferred_bytes(&self) -> u64 {
        self.transferred_bytes.load(Ordering::Relaxed)
    }

    /// Overall average throughput from transfer start in MB/s.
    pub fn average_throughput_mb_s(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed <= 0.0001 {
            return 0.0;
        }
        let transferred = self.transferred_bytes.load(Ordering::Relaxed) as f64;
        (transferred / elapsed) / (1024.0 * 1024.0)
    }

    /// Instantaneous smoothed throughput in MB/s.
    pub fn current_throughput_mb_s(&self) -> f64 {
        if let Ok(speed) = self.smoothed_throughput_bytes_sec.lock() {
            if *speed > 0.0 {
                return *speed / (1024.0 * 1024.0);
            }
        }
        self.average_throughput_mb_s()
    }

    /// Estimated remaining time (ETA) in seconds.
    pub fn eta_seconds(&self) -> Option<f64> {
        let transferred = self.transferred_bytes.load(Ordering::Relaxed);
        if transferred >= self.total_bytes {
            return Some(0.0);
        }

        let remaining_bytes = self.total_bytes.saturating_sub(transferred) as f64;
        let speed_mb_s = self.current_throughput_mb_s();
        let speed_bytes_s = speed_mb_s * 1024.0 * 1024.0;

        if speed_bytes_s > 1024.0 {
            Some(remaining_bytes / speed_bytes_s)
        } else {
            None
        }
    }

    /// Returns a complete progress snapshot for reporting.
    pub fn snapshot(&self) -> TransferProgressSnapshot {
        let total = self.total_bytes;
        let transferred = self.transferred_bytes.load(Ordering::Relaxed);
        let progress_percent = if total > 0 {
            ((transferred as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
        } else {
            100.0
        };

        TransferProgressSnapshot {
            total_bytes: total,
            transferred_bytes: transferred,
            progress_percent,
            current_throughput_mb_s: self.current_throughput_mb_s(),
            average_throughput_mb_s: self.average_throughput_mb_s(),
            eta_seconds: self.eta_seconds(),
            elapsed_seconds: self.start_time.elapsed().as_secs_f64(),
        }
    }
}

// ============================================================================
// 6. Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq_basic() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_verify_pin_constant_time() {
        assert!(verify_pin_constant_time("849201", "849201"));
        assert!(!verify_pin_constant_time("849201", "849202"));
        assert!(!verify_pin_constant_time("849201", "84920"));
    }

    #[test]
    fn test_sha256_nist_test_vectors() {
        // Empty string
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // "abc"
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq" (56 bytes)
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn test_hmac_sha256_rfc4231_test_case_1() {
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        assert_eq!(
            hmac_sha256_hex(&key, data),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn test_epoch_salted_rendezvous_topic_derivation() {
        let oma_id = "omarchy-node-f73a";
        let t1 = 1700000100 - (1700000100 % EPOCH_BUCKET_SECONDS); // Bucket boundary
        let t2 = t1 + 300; // Same 15-min bucket
        let t3 = t1 + 1000; // Next 15-min bucket

        let topic1 = derive_epoch_topic(oma_id, t1);
        let topic2 = derive_epoch_topic(oma_id, t2);
        let topic3 = derive_epoch_topic(oma_id, t3);

        // Same epoch bucket produces identical blind topic
        assert_eq!(topic1, topic2);
        // Different epoch bucket produces distinct non-correlatable topic
        assert_ne!(topic1, topic3);

        // Verification with window tolerance = 1
        assert!(verify_epoch_topic(oma_id, &topic1, t1, 1));
        assert!(verify_epoch_topic(oma_id, &topic1, t3, 1));
        assert!(!verify_epoch_topic(oma_id, &topic1, t1 + 3600, 1)); // 1 hour later fails
    }

    #[test]
    fn test_adaptive_chunker_bounds_and_adaptation() {
        let mut chunker = AdaptiveChunker::new();
        assert_eq!(chunker.current_chunk_size(), DEFAULT_CHUNK_SIZE);

        // Excellent channel conditions -> size grows towards MAX_CHUNK_SIZE
        for _ in 0..40 {
            chunker.record_sample(Duration::from_millis(5), true);
        }
        assert_eq!(chunker.current_chunk_size(), MAX_CHUNK_SIZE);

        // Packet loss -> size shrinks towards MIN_CHUNK_SIZE
        for _ in 0..10 {
            chunker.record_sample(Duration::from_millis(300), false);
        }
        assert_eq!(chunker.current_chunk_size(), MIN_CHUNK_SIZE);
    }

    #[test]
    fn test_transfer_metrics_and_eta() {
        let total = 100 * 1024 * 1024; // 100 MB
        let metrics = TransferMetrics::new(total);

        metrics.update_progress(10 * 1024 * 1024);
        assert_eq!(metrics.transferred_bytes(), 10 * 1024 * 1024);

        let snap = metrics.snapshot();
        assert_eq!(snap.total_bytes, total);
        assert!((snap.progress_percent - 10.0).abs() < 0.1);
    }
}
