//! GPU compute through wgpu: Vulkan on NVIDIA (proprietary driver and NVK), AMD
//! (RADV), Intel (ANV) and Qualcomm Adreno (Turnip), OpenGL ES 3.1 where Vulkan is
//! missing. Every GPU path has a CPU twin and any failure falls back to it, so the
//! GPU only ever makes things faster.
//!
//! `OMASTUDIO_GPU=off` disables it. Software rasterisers (llvmpipe/lavapipe) are
//! skipped because the CPU path is faster than them; `OMASTUDIO_GPU=software` allows
//! them, which the tests use on machines without a GPU. `OMASTUDIO_GPU_DEVICE` selects
//! a specific adapter by 0-based ranked index or case-insensitive name/vendor substring.
//! `OMASTUDIO_GPU_TIMEOUT_MS` overrides the bounded submission poll timeout (clamped to
//! 100..=300_000 ms; defaults to 5_000 ms for interactive previews and 30_000 ms for
//! demosaic/export work).

pub mod demosaic;
pub mod layers;
pub mod local;
pub mod point_ops;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Default bounded poll timeout for interactive previews (ms).
const DEFAULT_INTERACTIVE_TIMEOUT_MS: u64 = 5_000;
/// Default bounded poll timeout for batch/export/demosaic jobs (ms).
const DEFAULT_BATCH_TIMEOUT_MS: u64 = 30_000;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 300_000;

/// Persistent cached buffer pool cap per stage; larger export allocations are evicted
/// after use or after `OVERSIZED_IDLE_EVICT` to keep VRAM bounded.
pub(super) const PREVIEW_VRAM_CAP: u64 = 160 << 20;
pub(super) const OVERSIZED_IDLE_EVICT: Duration = Duration::from_secs(10);

/// Window within which repeated device losses count as a rapid reset loop.
const RAPID_LOSS_WINDOW: Duration = Duration::from_secs(5);

pub struct Gpu {
    /// Monotonic device generation incremented on every successful initialization.
    pub generation: u64,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub limits: wgpu::Limits,
    lost: Arc<AtomicBool>,
    created_at: Instant,
    /// One job at a time: jobs size their buffers to the device's limits.
    jobs: Mutex<()>,
    demosaic: OnceLock<Result<demosaic::Pipelines, String>>,
    point_ops: OnceLock<Result<point_ops::Pipeline, String>>,
    local: OnceLock<Result<local::Pipeline, String>>,
}

impl Gpu {
    /// Whether this device has been marked lost by a driver callback, timeout, or fatal poll error.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Marks this device lost so the next [`acquire`] drops stale pipelines/buffers and reopens it.
    pub fn mark_lost(&self, reason: &str) {
        if !self.lost.swap(true, Ordering::AcqRel) {
            eprintln!("omastudio: GPU device lost (gen {}): {reason}", self.generation);
        }
    }

    /// Bounded poll timeout for this device, honouring `OMASTUDIO_GPU_TIMEOUT_MS`.
    pub fn poll_timeout(&self, interactive: bool) -> Duration {
        let raw = std::env::var("OMASTUDIO_GPU_TIMEOUT_MS").ok();
        parse_timeout_ms(raw.as_deref(), interactive)
    }

    /// Waits for `submission_index` with a bounded timeout, marking the device lost on
    /// timeout or poll error so callers fall back cleanly to the CPU instead of hanging.
    pub(super) fn wait_submission(
        &self,
        submission_index: Option<wgpu::SubmissionIndex>,
        timeout: Duration,
    ) -> Result<(), String> {
        if self.is_lost() {
            return Err("GPU device was lost".to_string());
        }
        match self.device.poll(wgpu::PollType::Wait { submission_index, timeout: Some(timeout) }) {
            Ok(_) => {
                if self.is_lost() {
                    Err("GPU device was lost during execution".to_string())
                } else {
                    Ok(())
                }
            }
            Err(wgpu::PollError::Timeout) => {
                let msg = format!("GPU submission timed out after {} ms", timeout.as_millis());
                self.mark_lost(&msg);
                Err(msg)
            }
            Err(e) => {
                let msg = format!("GPU poll failed: {e}");
                self.mark_lost(&msg);
                Err(msg)
            }
        }
    }
}

/// Outcome of a GPU stage attempt, allowing callers to trip a per-frame circuit breaker
/// on runtime GPU failures without tripping it on intentional skips.
#[derive(Debug)]
pub enum StageOutcome<T> {
    Done(T),
    Skipped,
    Failed,
}

/// RAII guard for a `map_async` staging buffer so early returns, timeouts, or errors
/// never leave a cached staging buffer mapped.
///
/// Any `BufferView` returned by `get_mapped_range` must be dropped before this guard.
pub(super) struct StagingGuard<'a> {
    buf: &'a wgpu::Buffer,
    active: bool,
}

impl<'a> StagingGuard<'a> {
    pub(super) fn new(buf: &'a wgpu::Buffer) -> Self {
        Self { buf, active: true }
    }

    /// Disarms the guard when `map_async` itself failed and left the buffer unmapped.
    pub(super) fn disarm(&mut self) {
        self.active = false;
    }

    /// Unmaps the buffer immediately and consumes the guard.
    pub(super) fn unmap_now(mut self) {
        if self.active {
            self.active = false;
            self.buf.unmap();
        }
    }
}

impl Drop for StagingGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            self.buf.unmap();
        }
    }
}

/// Pops both the inner `OutOfMemory` scope and outer `Validation` scope in LIFO order
/// so neither scope is silently dropped and real Vulkan/WebGPU diagnostics always surface.
pub(super) fn drain_scopes(oom: wgpu::ErrorScopeGuard, validation: wgpu::ErrorScopeGuard) -> Result<(), String> {
    let oom_err = pollster::block_on(oom.pop());
    let val_err = pollster::block_on(validation.pop());
    if let Some(e) = oom_err {
        return Err(format!("out of GPU memory: {e}"));
    }
    if let Some(e) = val_err {
        return Err(e.to_string());
    }
    Ok(())
}

/// What `omastudio-engine gpu` reports.
#[derive(Debug, Serialize)]
pub struct GpuStatus {
    pub enabled: bool,
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub backend: Option<String>,
    pub device_type: Option<String>,
    pub driver: Option<String>,
    /// Why no GPU is used, when none is.
    pub reason: Option<String>,
}

enum GpuSlot {
    Uninitialized,
    Ready {
        gpu: Arc<Gpu>,
        rapid_losses: u32,
    },
    Unavailable {
        reason: String,
        retry_after: Option<Instant>,
        consecutive_failures: u32,
    },
}

enum InitError {
    Permanent(String),
    Transient(String),
}

static GPU: RwLock<GpuSlot> = RwLock::new(GpuSlot::Uninitialized);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

fn parse_timeout_ms(raw: Option<&str>, interactive: bool) -> Duration {
    let default_ms = if interactive { DEFAULT_INTERACTIVE_TIMEOUT_MS } else { DEFAULT_BATCH_TIMEOUT_MS };
    let ms = raw
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|v| v.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS))
        .unwrap_or(default_ms);
    Duration::from_millis(ms)
}

fn backoff_for(consecutive_failures: u32) -> Duration {
    match consecutive_failures {
        0 | 1 => Duration::from_millis(250),
        2 => Duration::from_secs(2),
        3 => Duration::from_secs(10),
        _ => Duration::from_secs(30),
    }
}

fn acquire_result() -> Result<Arc<Gpu>, String> {
    {
        let slot = GPU.read().unwrap_or_else(|e| e.into_inner());
        match &*slot {
            GpuSlot::Ready { gpu, .. } if !gpu.is_lost() => return Ok(Arc::clone(gpu)),
            GpuSlot::Unavailable { reason, retry_after, .. } => match retry_after {
                None => return Err(reason.clone()),
                Some(deadline) if Instant::now() < *deadline => return Err(reason.clone()),
                Some(_) => {}
            },
            GpuSlot::Uninitialized | GpuSlot::Ready { .. } => {}
        }
    }

    let mut slot = GPU.write().unwrap_or_else(|e| e.into_inner());
    let mut prev_failures = 0u32;
    let mut next_rapid_losses = 0u32;

    match &*slot {
        GpuSlot::Ready { gpu, .. } if !gpu.is_lost() => return Ok(Arc::clone(gpu)),
        GpuSlot::Ready { gpu, rapid_losses } => {
            let rapid = gpu.created_at.elapsed() < RAPID_LOSS_WINDOW;
            next_rapid_losses = if rapid { rapid_losses.saturating_add(1) } else { 1 };
            // Drop stale Arc<Gpu> (and its pipelines/buffers) before reopening the device.
            *slot = GpuSlot::Uninitialized;
            if next_rapid_losses >= 3 {
                let fails = next_rapid_losses - 2;
                let reason = "GPU device repeatedly lost; cooling down before retry".to_string();
                *slot = GpuSlot::Unavailable {
                    reason: reason.clone(),
                    retry_after: Some(Instant::now() + backoff_for(fails)),
                    consecutive_failures: fails,
                };
                return Err(reason);
            }
        }
        GpuSlot::Unavailable { reason, retry_after, consecutive_failures } => match retry_after {
            None => return Err(reason.clone()),
            Some(deadline) if Instant::now() < *deadline => return Err(reason.clone()),
            Some(_) => prev_failures = *consecutive_failures,
        },
        GpuSlot::Uninitialized => {}
    }

    match init() {
        Ok(gpu) => {
            let gpu = Arc::new(gpu);
            *slot = GpuSlot::Ready { gpu: Arc::clone(&gpu), rapid_losses: next_rapid_losses };
            Ok(gpu)
        }
        Err(InitError::Permanent(reason)) => {
            *slot = GpuSlot::Unavailable { reason: reason.clone(), retry_after: None, consecutive_failures: 0 };
            Err(reason)
        }
        Err(InitError::Transient(reason)) => {
            let fails = prev_failures.saturating_add(1);
            *slot = GpuSlot::Unavailable {
                reason: reason.clone(),
                retry_after: Some(Instant::now() + backoff_for(fails)),
                consecutive_failures: fails,
            };
            Err(reason)
        }
    }
}

/// Acquires the active GPU, initializing it on first use or transparently recovering
/// with a fresh device generation after a driver reset / device loss.
pub fn acquire() -> Option<Arc<Gpu>> {
    acquire_result().ok()
}

/// Alias for [`acquire`]: returns the active GPU or `None` when disabled, absent, or cooling down.
pub fn get() -> Option<Arc<Gpu>> {
    acquire()
}

pub fn status() -> GpuStatus {
    match acquire_result() {
        Ok(g) => GpuStatus {
            enabled: true,
            name: Some(g.info.name.clone()),
            vendor: Some(vendor_name(g.info.vendor).to_string()),
            backend: Some(format!("{:?}", g.info.backend)),
            device_type: Some(format!("{:?}", g.info.device_type)),
            driver: Some(format!("{} {}", g.info.driver, g.info.driver_info).trim().to_string()),
            reason: None,
        },
        Err(e) => GpuStatus {
            enabled: false,
            name: None,
            vendor: None,
            backend: None,
            device_type: None,
            driver: None,
            reason: Some(e),
        },
    }
}

fn vendor_name(id: u32) -> &'static str {
    match id {
        0x10de => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x5143 => "Qualcomm",
        0x13b5 => "Arm",
        0x106b => "Apple",
        0x1af4 => "VirtIO",
        0x1414 => "Microsoft",
        0x1ae0 => "Google",
        0x1010 => "Imagination",
        0x1ed5 => "Moore Threads",
        0x10005 => "Mesa (software)",
        _ => "Other",
    }
}

/// Picks an adapter from an already-ranked candidate slice according to `OMASTUDIO_GPU_DEVICE`
/// (0-based index or case-insensitive substring on adapter name or vendor).
fn select_adapter_index<T, F>(candidates: &[T], selector: Option<&str>, describe: F) -> Result<usize, String>
where
    F: Fn(&T) -> (&str, &str),
{
    if candidates.is_empty() {
        return Err("No GPU with compute shaders found".to_string());
    }
    let Some(raw) = selector.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(0);
    };
    if let Ok(idx) = raw.parse::<usize>() {
        if idx < candidates.len() {
            return Ok(idx);
        }
        return Err(format!("OMASTUDIO_GPU_DEVICE={raw} out of range (found {} adapter(s))", candidates.len()));
    }
    let needle = raw.to_ascii_lowercase();
    candidates
        .iter()
        .position(|c| {
            let (name, vendor) = describe(c);
            name.to_ascii_lowercase().contains(&needle) || vendor.to_ascii_lowercase().contains(&needle)
        })
        .ok_or_else(|| format!("OMASTUDIO_GPU_DEVICE={raw:?} matched no compute adapter"))
}

fn init() -> Result<Gpu, InitError> {
    let mode = std::env::var("OMASTUDIO_GPU").unwrap_or_default();
    if matches!(mode.as_str(), "off" | "0" | "cpu") {
        return Err(InitError::Permanent("Disabled by OMASTUDIO_GPU".to_string()));
    }
    let allow_software = mode == "software";
    let backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));

    // Discrete before integrated, Vulkan before GL; compute shaders are required
    let rank = |a: &wgpu::Adapter| {
        let info = a.get_info();
        let kind = match info.device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu => 2,
            wgpu::DeviceType::Other => 3,
            wgpu::DeviceType::Cpu => 4,
        };
        (kind, u8::from(info.backend != wgpu::Backend::Vulkan))
    };
    let mut candidates: Vec<(wgpu::Adapter, wgpu::AdapterInfo)> = adapters
        .into_iter()
        .filter(|a| a.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS))
        .filter(|a| allow_software || a.get_info().device_type != wgpu::DeviceType::Cpu)
        .map(|a| {
            let info = a.get_info();
            (a, info)
        })
        .collect();
    candidates.sort_by_key(|(a, _)| rank(a));

    let selector = std::env::var("OMASTUDIO_GPU_DEVICE").ok();
    let idx = select_adapter_index(&candidates, selector.as_deref(), |(_, info)| {
        (info.name.as_str(), vendor_name(info.vendor))
    })
    .map_err(InitError::Transient)?;
    let (adapter, info) = candidates.swap_remove(idx);

    let limits = adapter.limits();
    // Point ops needs 4 storage buffers; local (6) and demosaic (7) gate individually.
    if limits.max_storage_buffers_per_shader_stage < 4 {
        return Err(InitError::Transient(format!(
            "{}: too few storage buffers per shader ({} < 4)",
            info.name, limits.max_storage_buffers_per_shader_stage
        )));
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("omastudio"),
        required_features: wgpu::Features::empty(),
        required_limits: limits.clone(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| InitError::Transient(format!("{}: {}", info.name, e)))?;

    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let lost = Arc::new(AtomicBool::new(false));
    let lost_cb = Arc::clone(&lost);
    device.set_device_lost_callback(move |reason, message| {
        if !lost_cb.swap(true, Ordering::AcqRel) {
            eprintln!("omastudio: GPU device lost (gen {generation}, {reason:?}): {message}");
        }
    });
    // Errors are caught by error scopes around each job; anything else is logged
    // instead of wgpu's default panic
    device.on_uncaptured_error(Arc::new(|e| eprintln!("omastudio: GPU error: {e}")));
    Ok(Gpu {
        generation,
        device,
        queue,
        info,
        limits,
        lost,
        created_at: Instant::now(),
        jobs: Mutex::new(()),
        demosaic: OnceLock::new(),
        point_ops: OnceLock::new(),
        local: OnceLock::new(),
    })
}

/// Whether the processing stages (point operations, presence and detail) pay off on
/// this device. Copying a frame to the GPU and back costs more than the CPU needs on
/// integrated GPUs, which share the CPU's memory bandwidth (measured on Intel Arc:
/// ~8x slower than the CPU for the point stage), so only discrete GPUs are used unless
/// OMASTUDIO_GPU_STAGES=on asks for any GPU (off keeps them on the CPU).
pub fn stages_worthwhile(gpu: &Gpu) -> bool {
    match std::env::var("OMASTUDIO_GPU_STAGES").as_deref() {
        Ok("on") => true,
        Ok("off") => false,
        _ => gpu.info.device_type == wgpu::DeviceType::DiscreteGpu,
    }
}

/// Views a slice of plain numbers as bytes (Linux targets are little-endian, which
/// is what GPU buffers expect).
fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: T is a plain number type (f32/u32), any byte pattern of it is readable
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

#[cfg(test)]
pub(super) static TEST_GPU_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_name_covers_modern_and_virtual_vendors() {
        assert_eq!(vendor_name(0x10de), "NVIDIA");
        assert_eq!(vendor_name(0x1002), "AMD");
        assert_eq!(vendor_name(0x1022), "AMD");
        assert_eq!(vendor_name(0x8086), "Intel");
        assert_eq!(vendor_name(0x5143), "Qualcomm");
        assert_eq!(vendor_name(0x13b5), "Arm");
        assert_eq!(vendor_name(0x106b), "Apple");
        assert_eq!(vendor_name(0x1af4), "VirtIO");
        assert_eq!(vendor_name(0x1414), "Microsoft");
        assert_eq!(vendor_name(0x1ae0), "Google");
        assert_eq!(vendor_name(0x1010), "Imagination");
        assert_eq!(vendor_name(0x1ed5), "Moore Threads");
        assert_eq!(vendor_name(0x10005), "Mesa (software)");
        assert_eq!(vendor_name(0x9999), "Other");
    }

    #[test]
    fn timeout_parsing_clamps_and_respects_defaults() {
        assert_eq!(parse_timeout_ms(None, true), Duration::from_millis(5_000));
        assert_eq!(parse_timeout_ms(None, false), Duration::from_millis(30_000));
        assert_eq!(parse_timeout_ms(Some("invalid"), true), Duration::from_millis(5_000));
        assert_eq!(parse_timeout_ms(Some("10"), true), Duration::from_millis(100));
        assert_eq!(parse_timeout_ms(Some("999999"), false), Duration::from_millis(300_000));
        assert_eq!(parse_timeout_ms(Some(" 12500 "), true), Duration::from_millis(12_500));
    }

    #[test]
    fn backoff_schedule_escalates_and_caps_at_30s() {
        assert_eq!(backoff_for(1), Duration::from_millis(250));
        assert_eq!(backoff_for(2), Duration::from_secs(2));
        assert_eq!(backoff_for(3), Duration::from_secs(10));
        assert_eq!(backoff_for(4), Duration::from_secs(30));
        assert_eq!(backoff_for(10), Duration::from_secs(30));
    }

    #[test]
    fn adapter_selector_supports_index_and_case_insensitive_substring() {
        let adapters = [
            ("NVIDIA GeForce RTX 5090", "NVIDIA"),
            ("AMD Radeon 8060S (RADV GFX1151)", "AMD"),
            ("llvmpipe (LLVM 21.1, 256 bits)", "Mesa (software)"),
        ];
        assert_eq!(select_adapter_index(&adapters, None, |a| *a).unwrap(), 0);
        assert_eq!(select_adapter_index(&adapters, Some("  "), |a| *a).unwrap(), 0);
        assert_eq!(select_adapter_index(&adapters, Some("1"), |a| *a).unwrap(), 1);
        assert!(select_adapter_index(&adapters, Some("3"), |a| *a).is_err());
        assert_eq!(select_adapter_index(&adapters, Some("gfx1151"), |a| *a).unwrap(), 1);
        assert_eq!(select_adapter_index(&adapters, Some("amd"), |a| *a).unwrap(), 1);
        assert_eq!(select_adapter_index(&adapters, Some("LLVMPIPE"), |a| *a).unwrap(), 2);
        assert!(select_adapter_index(&adapters, Some("intel"), |a| *a).is_err());
        assert!(select_adapter_index::<(&str, &str), _>(&[], None, |a| *a).is_err());
    }

    /// Runs only where a GPU (or, with OMASTUDIO_GPU=software, lavapipe) is available.
    #[test]
    fn device_lost_triggers_fresh_generation_recovery() {
        let _test_lock = TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(first) = acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let gen1 = first.generation;
        assert!(!first.is_lost());
        first.mark_lost("simulated driver reset in test");
        assert!(first.is_lost());

        let second = acquire().expect("GPU should recover with a new generation after DeviceLost");
        assert!(second.generation > gen1, "expected generation > {gen1}, got {}", second.generation);
        assert!(!second.is_lost());
    }
}
