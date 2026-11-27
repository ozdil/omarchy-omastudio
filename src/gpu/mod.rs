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

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Default bounded poll timeout for interactive previews (ms).
const DEFAULT_INTERACTIVE_TIMEOUT_MS: u64 = 5_000;
/// Default bounded poll timeout for batch/export/demosaic jobs (ms).
const DEFAULT_BATCH_TIMEOUT_MS: u64 = 30_000;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 300_000;
/// Maximum persisted Vulkan pipeline cache blob size (64 MiB).
const MAX_PIPELINE_CACHE_BYTES: usize = 64 << 20;

/// Persistent cached buffer pool cap per stage; larger export allocations are evicted
/// after use or after `OVERSIZED_IDLE_EVICT` to keep VRAM bounded.
pub(super) const PREVIEW_VRAM_CAP: u64 = 160 << 20;
pub(super) const OVERSIZED_IDLE_EVICT: Duration = Duration::from_secs(10);

/// Window within which repeated device losses count as a rapid reset loop.
const RAPID_LOSS_WINDOW: Duration = Duration::from_secs(5);

struct JobState {
    busy: bool,
    interactive_waiters: usize,
}

struct JobQueue {
    state: Mutex<JobState>,
    cv: Condvar,
}

impl JobQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(JobState { busy: false, interactive_waiters: 0 }),
            cv: Condvar::new(),
        }
    }

    fn lock(&self, interactive: bool) -> JobGuard<'_> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if interactive {
            st.interactive_waiters += 1;
            while st.busy {
                st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            st.interactive_waiters -= 1;
            st.busy = true;
        } else {
            while st.busy || st.interactive_waiters > 0 {
                st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            st.busy = true;
        }
        JobGuard { queue: self }
    }

    fn has_interactive_waiter(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).interactive_waiters > 0
    }
}

/// Priority-fair lock guard for GPU jobs. Interactive preview requests wait ahead of
/// background demosaic/export bands so background work yields within one band (~5-15 ms).
pub(super) struct JobGuard<'a> {
    queue: &'a JobQueue,
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        let mut st = self.queue.state.lock().unwrap_or_else(|e| e.into_inner());
        st.busy = false;
        self.queue.cv.notify_all();
    }
}

static NEXT_PREVIEW_ID: AtomicU64 = AtomicU64::new(1);

/// GPU-resident packed 16-bit preview source buffer so interactive slider drags upload
/// only ~512 B of parameters instead of re-uploading ~59 MB of pixels over PCIe per tick.
pub(super) struct PreviewSource {
    pub id: u64,
    pub key: u64,
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub sample_hash: u64,
    pub buffer: wgpu::Buffer,
}

pub struct Gpu {
    /// Monotonic device generation incremented on every successful initialization.
    pub generation: u64,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub limits: wgpu::Limits,
    /// Opportunistically enabled optional features (`PIPELINE_CACHE`, `SHADER_F16`,
    /// `SUBGROUP`, `TIMESTAMP_QUERY`).
    pub features: wgpu::Features,
    pub(super) pipeline_cache: Option<wgpu::PipelineCache>,
    pipeline_cache_path: Option<PathBuf>,
    pipeline_cache_io: Mutex<()>,
    lost: Arc<AtomicBool>,
    created_at: Instant,
    /// Priority-fair job serialization: interactive previews preempt queued background bands.
    jobs: JobQueue,
    demosaic: OnceLock<Result<demosaic::Pipelines, String>>,
    point_ops: OnceLock<Result<point_ops::Pipeline, String>>,
    local: OnceLock<Result<local::Pipeline, String>>,
    preview_src: Mutex<Option<Arc<PreviewSource>>>,
    stages_auto: OnceLock<bool>,
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

    /// Acquires exclusive access to submit GPU work. Interactive callers register as
    /// waiters so background multi-band work yields between bands.
    pub(super) fn lock_job(&self, interactive: bool) -> JobGuard<'_> {
        self.jobs.lock(interactive)
    }

    /// Returns true while an interactive preview is waiting for the GPU job lock.
    pub(super) fn has_interactive_waiter(&self) -> bool {
        self.jobs.has_interactive_waiter()
    }

    /// Persists the compiled Vulkan pipeline cache atomically to disk when supported.
    pub(super) fn persist_pipeline_cache(&self) {
        let (Some(cache), Some(path)) = (&self.pipeline_cache, &self.pipeline_cache_path) else {
            return;
        };
        let _io = self.pipeline_cache_io.lock().unwrap_or_else(|e| e.into_inner());
        let Some(data) = cache.get_data() else {
            return;
        };
        if data.is_empty() || data.len() > MAX_PIPELINE_CACHE_BYTES {
            return;
        }
        if let Some(parent) = path.parent()
            && crate::security::ensure_secure_dir(parent).is_err()
        {
            return;
        }
        let _ = crate::security::atomic_write_secure(path, &data);
    }

    /// Returns a GPU-resident packed 16-bit source buffer matching `(key, w, h, ch, sample_hash)`,
    /// uploading the full frame once on cache miss. Returns `None` if the frame exceeds `max_bytes`.
    pub(super) fn get_or_upload_preview_source(
        &self,
        key: u64,
        input: &[u16],
        width: usize,
        height: usize,
        channels: usize,
        max_bytes: u64,
    ) -> Option<Arc<PreviewSource>> {
        let needed_samples = width.checked_mul(height)?.checked_mul(channels)?;
        if input.len() < needed_samples || needed_samples == 0 || needed_samples > u32::MAX as usize {
            return None;
        }
        let raw_bytes = (needed_samples as u64).checked_mul(2)?;
        let alloc_bytes = raw_bytes.next_multiple_of(4).max(4);
        if alloc_bytes > max_bytes || alloc_bytes > PREVIEW_VRAM_CAP {
            return None;
        }
        let slice = &input[..needed_samples];
        let sample_hash = quick_sample_hash(slice);

        let mut guard = self.preview_src.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.as_ref()
            && existing.key == key
            && existing.width == width
            && existing.height == height
            && existing.channels == channels
            && existing.sample_hash == sample_hash
        {
            return Some(Arc::clone(existing));
        }
        // Drop previous resident buffer before allocating the replacement to keep VRAM bounded.
        *guard = None;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview resident src"),
            size: alloc_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let even = slice.len() & !1;
        if even > 0 {
            self.queue.write_buffer(&buffer, 0, as_bytes(&slice[..even]));
        }
        if even < slice.len() {
            let tail = slice[even] as u32;
            self.queue.write_buffer(&buffer, (even * 2) as u64, as_bytes(&[tail]));
        }
        let src = Arc::new(PreviewSource {
            id: NEXT_PREVIEW_ID.fetch_add(1, Ordering::Relaxed),
            key,
            width,
            height,
            channels,
            sample_hash,
            buffer,
        });
        *guard = Some(Arc::clone(&src));
        Some(src)
    }

    /// Releases any GPU-resident preview source buffer.
    pub fn clear_preview_source(&self) {
        *self.preview_src.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
    /// Opportunistically enabled optional features on this adapter.
    pub features: Vec<&'static str>,
    /// Whether processing stages (point ops / local / display) are enabled on this GPU.
    pub stages_enabled: bool,
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
/// with a fresh device generation after a driver reset / device loss. Returns `None`
/// while [`with_gpu_disabled`] is active on the current thread.
pub fn acquire() -> Option<Arc<Gpu>> {
    if gpu_disabled_on_thread() {
        return None;
    }
    acquire_result().ok()
}

/// Alias for [`acquire`]: returns the active GPU or `None` when disabled, absent, or cooling down.
pub fn get() -> Option<Arc<Gpu>> {
    acquire()
}

fn feature_names(features: wgpu::Features) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (flag, name) in [
        (wgpu::Features::PIPELINE_CACHE, "PIPELINE_CACHE"),
        (wgpu::Features::SHADER_F16, "SHADER_F16"),
        (wgpu::Features::SUBGROUP, "SUBGROUP"),
        (wgpu::Features::TIMESTAMP_QUERY, "TIMESTAMP_QUERY"),
    ] {
        if features.contains(flag) {
            out.push(name);
        }
    }
    out
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
            features: feature_names(g.features),
            stages_enabled: stages_worthwhile(&g),
            reason: None,
        },
        Err(e) => GpuStatus {
            enabled: false,
            name: None,
            vendor: None,
            backend: None,
            device_type: None,
            driver: None,
            features: Vec::new(),
            stages_enabled: false,
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

fn omastudio_cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        let p = PathBuf::from(xdg.trim());
        if p.is_absolute() {
            return p.join("omastudio");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache").join("omastudio")
}

fn fnv1a_update(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

fn fnv1a_start() -> u64 {
    0xcbf2_9ce4_8422_2325
}

fn quick_sample_hash(samples: &[u16]) -> u64 {
    let mut h = fnv1a_update(fnv1a_start(), &(samples.len() as u64).to_le_bytes());
    if samples.is_empty() {
        return h;
    }
    let stride = (samples.len() / 64).max(1);
    let mut i = 0usize;
    while i < samples.len() {
        h = fnv1a_update(h, &samples[i].to_le_bytes());
        i += stride;
    }
    fnv1a_update(h, &samples[samples.len() - 1].to_le_bytes())
}

fn shader_bundle_hash(info: &wgpu::AdapterInfo) -> u64 {
    let mut h = fnv1a_start();
    for s in [
        include_str!("demosaic.wgsl"),
        include_str!("point_ops.wgsl"),
        include_str!("local.wgsl"),
        env!("CARGO_PKG_VERSION"),
        info.driver.as_str(),
        info.driver_info.as_str(),
    ] {
        h = fnv1a_update(h, s.as_bytes());
        h = fnv1a_update(h, &[0xff]);
    }
    h
}

fn pipeline_cache_file(info: &wgpu::AdapterInfo) -> Option<PathBuf> {
    let key = wgpu::util::pipeline_cache_key(info)?;
    let hash = shader_bundle_hash(info);
    Some(omastudio_cache_dir().join("pipeline-cache").join(format!("{key}-{hash:016x}.bin")))
}

fn load_pipeline_cache(device: &wgpu::Device, info: &wgpu::AdapterInfo, features: wgpu::Features) -> (Option<wgpu::PipelineCache>, Option<PathBuf>) {
    if !features.contains(wgpu::Features::PIPELINE_CACHE) {
        return (None, None);
    }
    let path = pipeline_cache_file(info);
    let initial = path
        .as_ref()
        .and_then(|p| crate::security::read_secure_file(p, MAX_PIPELINE_CACHE_BYTES).ok())
        .filter(|b| !b.is_empty());
    // SAFETY: `initial` is either None or read from our own owner-verified, O_NOFOLLOW
    // cache file written exclusively via `PipelineCache::get_data()` and keyed by
    // adapter/driver/shader hash; `fallback: true` safely discards any stale/corrupt blob.
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("omastudio pipeline cache"),
            data: initial.as_deref(),
            fallback: true,
        })
    };
    (Some(cache), path)
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
    let optional = wgpu::Features::PIPELINE_CACHE
        | wgpu::Features::SHADER_F16
        | wgpu::Features::SUBGROUP
        | wgpu::Features::TIMESTAMP_QUERY;
    let features = adapter.features() & optional;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("omastudio"),
        required_features: features,
        required_limits: limits.clone(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| InitError::Transient(format!("{}: {}", info.name, e)))?;

    let (pipeline_cache, pipeline_cache_path) = load_pipeline_cache(&device, &info, features);

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
        features,
        pipeline_cache,
        pipeline_cache_path,
        pipeline_cache_io: Mutex::new(()),
        lost,
        created_at: Instant::now(),
        jobs: JobQueue::new(),
        demosaic: OnceLock::new(),
        point_ops: OnceLock::new(),
        local: OnceLock::new(),
        preview_src: Mutex::new(None),
        stages_auto: OnceLock::new(),
    })
}

/// Persistent micro-benchmark record for integrated / unified-memory GPUs (AMD Strix
/// Halo, Apple Silicon Honeykrisp, Intel Xe3, Qualcomm Adreno Turnip).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct GpuBenchEntry {
    vendor: u32,
    device: u32,
    driver: String,
    driver_info: String,
    engine_version: String,
    gpu_ms: f64,
    cpu_ms: f64,
    worthwhile: bool,
}

impl GpuBenchEntry {
    fn matches(&self, info: &wgpu::AdapterInfo) -> bool {
        self.vendor == info.vendor
            && self.device == info.device
            && self.driver == info.driver
            && self.driver_info == info.driver_info
            && self.engine_version == env!("CARGO_PKG_VERSION")
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct GpuBenchCache {
    version: u32,
    entries: Vec<GpuBenchEntry>,
}

const GPU_BENCH_CACHE_VERSION: u32 = 1;

fn gpu_bench_cache_path() -> PathBuf {
    omastudio_cache_dir().join("gpu-bench-v1.json")
}

fn load_gpu_bench_cache(path: &Path) -> GpuBenchCache {
    let Ok(bytes) = crate::security::read_secure_file(path, 64 * 1024) else {
        return GpuBenchCache { version: GPU_BENCH_CACHE_VERSION, entries: Vec::new() };
    };
    match serde_json::from_slice::<GpuBenchCache>(&bytes) {
        Ok(c) if c.version == GPU_BENCH_CACHE_VERSION => c,
        _ => GpuBenchCache { version: GPU_BENCH_CACHE_VERSION, entries: Vec::new() },
    }
}

fn save_gpu_bench_cache(path: &Path, cache: &GpuBenchCache) {
    if let Some(parent) = path.parent()
        && crate::security::ensure_secure_dir(parent).is_err()
    {
        return;
    }
    if let Ok(json) = serde_json::to_vec_pretty(cache) {
        let _ = crate::security::atomic_write_secure(path, &json);
    }
}

fn evaluate_integrated_gpu(gpu: &Gpu) -> bool {
    let path = gpu_bench_cache_path();
    let mut cache = load_gpu_bench_cache(&path);
    if let Some(hit) = cache.entries.iter().find(|e| e.matches(&gpu.info)) {
        return hit.worthwhile;
    }
    // One-time synthetic 2560x1707 resident-source GPU vs CPU comparison (~30-50 ms).
    let (w, h) = (2560usize, 1707usize);
    let mut input = vec![0u16; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let n = ((x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663)) % 2048;
            let i = (y * w + x) * 3;
            input[i] = ((x * 24_000 / w) as u32 + n) as u16;
            input[i + 1] = ((y * 24_000 / h) as u32 + n) as u16;
            input[i + 2] = (((x + y) * 20_000 / (w + h)) as u32 + n) as u16;
        }
    }
    let recipe = crate::recipe::Recipe {
        exposure: 0.25,
        contrast: 20.0,
        highlights: -25.0,
        shadows: 20.0,
        clarity: 25.0,
        sharpness: 35.0,
        grain_amount: 15.0,
        vignette: -15.0,
        ..Default::default()
    };
    let transfer = crate::pipeline::transfer::Transfer::bt709();
    let bench_key = preview_source_key(u64::MAX - 1, &recipe.retouch, w, h, 3);

    // Warm-up run (compiles shaders & uploads resident preview source once).
    let warmup_ok = with_preview_source_key(bench_key, || {
        local::display_on_for_bench(gpu, &input, w, h, 3, &recipe, transfer).is_ok()
    });
    if !warmup_ok {
        gpu.clear_preview_source();
        return false;
    }
    let t_gpu = Instant::now();
    let gpu_ok = with_preview_source_key(bench_key, || {
        local::display_on_for_bench(gpu, &input, w, h, 3, &recipe, transfer).is_ok()
    });
    let gpu_ms = t_gpu.elapsed().as_secs_f64() * 1000.0;
    gpu.clear_preview_source();
    if !gpu_ok {
        return false;
    }

    let t_cpu = Instant::now();
    with_gpu_disabled(|| {
        let _ = crate::pipeline::process_buffer_16_to_8_ex(&input, w as u32, h as u32, 3, &recipe, transfer, false, false);
    });
    let cpu_ms = t_cpu.elapsed().as_secs_f64() * 1000.0;
    let worthwhile = gpu_ms < cpu_ms * 0.85;

    cache.entries.retain(|e| !e.matches(&gpu.info));
    cache.entries.push(GpuBenchEntry {
        vendor: gpu.info.vendor,
        device: gpu.info.device,
        driver: gpu.info.driver.clone(),
        driver_info: gpu.info.driver_info.clone(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        gpu_ms: (gpu_ms * 100.0).round() / 100.0,
        cpu_ms: (cpu_ms * 100.0).round() / 100.0,
        worthwhile,
    });
    if cache.entries.len() > 16 {
        let drain = cache.entries.len() - 16;
        cache.entries.drain(0..drain);
    }
    save_gpu_bench_cache(&path, &cache);
    worthwhile
}

/// Whether the processing stages (point operations, presence and detail) pay off on
/// this device. Uses a 3-layer policy:
/// 1. `OMASTUDIO_GPU_STAGES=on|off` explicit override.
/// 2. Discrete GPUs always enable GPU stages.
/// 3. Integrated / unified-memory APUs (AMD Strix Halo, Apple Honeykrisp, Intel Xe3,
///    Qualcomm Adreno) run a cached one-time resident-source micro-benchmark
///    (`$XDG_CACHE_HOME/omastudio/gpu-bench-v1.json`) and enable GPU stages when
///    `gpu_ms < cpu_ms * 0.85`.
pub fn stages_worthwhile(gpu: &Gpu) -> bool {
    match std::env::var("OMASTUDIO_GPU_STAGES").as_deref() {
        Ok("on") => true,
        Ok("off") => false,
        _ => match gpu.info.device_type {
            wgpu::DeviceType::DiscreteGpu => true,
            wgpu::DeviceType::IntegratedGpu => *gpu.stages_auto.get_or_init(|| evaluate_integrated_gpu(gpu)),
            _ => false,
        },
    }
}

thread_local! {
    static PREVIEW_SOURCE_KEY: Cell<Option<u64>> = const { Cell::new(None) };
    static GPU_DISABLED_TL: Cell<bool> = const { Cell::new(false) };
    static LAST_GPU_PASS_MS: Cell<Option<f64>> = const { Cell::new(None) };
}

static BENCH_PROFILING: AtomicBool = AtomicBool::new(false);

/// Enables or disables GPU timestamp query capture for benchmark runs (`bench.rs`).
pub fn set_bench_profiling(on: bool) {
    BENCH_PROFILING.store(on, Ordering::Relaxed);
}

pub(super) fn profiling_enabled() -> bool {
    BENCH_PROFILING.load(Ordering::Relaxed) || std::env::var_os("OMASTUDIO_PROFILE").is_some()
}

pub(super) fn record_gpu_pass_ms(ms: f64) {
    LAST_GPU_PASS_MS.with(|cell| {
        let prev = cell.get().unwrap_or(0.0);
        cell.set(Some(prev + ms));
    });
}

/// Consumes the accumulated GPU compute pass time (in milliseconds) recorded on the
/// current thread via `TIMESTAMP_QUERY`, if available.
pub fn take_last_gpu_pass_ms() -> Option<f64> {
    LAST_GPU_PASS_MS.with(|cell| cell.take())
}

/// Computes a stable resident-source cache key from a monotonic photo-load generation,
/// active retouch spots, and preview dimensions.
pub fn preview_source_key(
    photo_gen: u64,
    retouch: &[crate::pipeline::retouch::RetouchSpot],
    width: usize,
    height: usize,
    channels: usize,
) -> u64 {
    let mut h = fnv1a_start();
    for v in [photo_gen, width as u64, height as u64, channels as u64, retouch.len() as u64] {
        h = fnv1a_update(h, &v.to_le_bytes());
    }
    for s in retouch {
        let mode = match s.mode {
            crate::pipeline::retouch::RetouchMode::Heal => 0u8,
            crate::pipeline::retouch::RetouchMode::Clone => 1u8,
        };
        h = fnv1a_update(h, &[mode]);
        for f in [s.x, s.y, s.source_x, s.source_y, s.radius, s.feather, s.opacity] {
            h = fnv1a_update(h, &f.to_bits().to_le_bytes());
        }
    }
    h
}

/// Runs `f` with a GPU-resident preview source key active on the current thread so
/// repeated interactive renders of the same photo reuse the uploaded 16-bit buffer in VRAM.
pub fn with_preview_source_key<R>(key: u64, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<u64>);
    impl Drop for Reset {
        fn drop(&mut self) {
            PREVIEW_SOURCE_KEY.with(|c| c.set(self.0));
        }
    }
    let prev = PREVIEW_SOURCE_KEY.with(|c| c.replace(Some(key)));
    let _reset = Reset(prev);
    f()
}

pub(super) fn current_preview_source_key() -> Option<u64> {
    PREVIEW_SOURCE_KEY.with(|c| c.get())
}

fn with_gpu_disabled<R>(f: impl FnOnce() -> R) -> R {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            GPU_DISABLED_TL.with(|c| c.set(self.0));
        }
    }
    let prev = GPU_DISABLED_TL.with(|c| c.replace(true));
    let _reset = Reset(prev);
    f()
}

fn gpu_disabled_on_thread() -> bool {
    GPU_DISABLED_TL.with(|c| c.get())
}

/// Views a slice of plain numbers as bytes (Linux targets are little-endian, which
/// is what GPU buffers expect).
fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: T is a plain number type (f32/u32/u16), any byte pattern of it is readable
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

    #[test]
    fn gpu_bench_cache_roundtrip_and_matching() {
        let dir = std::env::temp_dir().join(format!(
            "omastudio_gpu_bench_test_{}_{}",
            std::process::id(),
            NEXT_PREVIEW_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let path = dir.join("gpu-bench-v1.json");
        let mut info = wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Vulkan);
        info.name = "AMD Radeon 8060S (RADV GFX1151)".into();
        info.vendor = 0x1002;
        info.device = 0x1586;
        info.driver = "radv".into();
        info.driver_info = "Mesa 26.0.0".into();
        let entry = GpuBenchEntry {
            vendor: info.vendor,
            device: info.device,
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            gpu_ms: 8.2,
            cpu_ms: 29.4,
            worthwhile: true,
        };
        let cache = GpuBenchCache { version: GPU_BENCH_CACHE_VERSION, entries: vec![entry.clone()] };
        save_gpu_bench_cache(&path, &cache);

        let loaded = load_gpu_bench_cache(&path);
        assert_eq!(loaded.version, GPU_BENCH_CACHE_VERSION);
        assert_eq!(loaded.entries, vec![entry]);
        assert!(loaded.entries[0].matches(&info));

        let mut bumped_driver = info.clone();
        bumped_driver.driver_info = "Mesa 26.1.0".into();
        assert!(!loaded.entries[0].matches(&bumped_driver));

        let mut other_device = info;
        other_device.device = 0x9999;
        assert!(!loaded.entries[0].matches(&other_device));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn priority_fair_job_lock_prefers_interactive_waiter() {
        let queue = Arc::new(JobQueue::new());
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));

        // Hold lock on main thread so both a background and an interactive waiter queue up.
        let initial = queue.lock(false);
        assert!(!queue.has_interactive_waiter());

        let q_bg = Arc::clone(&queue);
        let o_bg = Arc::clone(&order);
        let bg_thread = std::thread::spawn(move || {
            let _g = q_bg.lock(false);
            o_bg.lock().unwrap().push("background");
        });

        // Let background waiter enter condvar wait before registering interactive waiter.
        std::thread::sleep(Duration::from_millis(15));

        let q_int = Arc::clone(&queue);
        let o_int = Arc::clone(&order);
        let int_thread = std::thread::spawn(move || {
            let _g = q_int.lock(true);
            o_int.lock().unwrap().push("interactive");
        });

        // Wait until interactive waiter is registered in JobQueue.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !queue.has_interactive_waiter() {
            assert!(Instant::now() < deadline, "interactive waiter never registered");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(queue.has_interactive_waiter());

        drop(initial);
        int_thread.join().unwrap();
        bg_thread.join().unwrap();

        assert_eq!(*order.lock().unwrap(), vec!["interactive", "background"]);
    }

    #[test]
    fn preview_source_key_tracks_gen_dims_and_retouch() {
        use crate::pipeline::retouch::{RetouchMode, RetouchSpot};
        let k1 = preview_source_key(1, &[], 3840, 2560, 3);
        let k1_again = preview_source_key(1, &[], 3840, 2560, 3);
        assert_eq!(k1, k1_again);
        assert_ne!(k1, preview_source_key(2, &[], 3840, 2560, 3));
        assert_ne!(k1, preview_source_key(1, &[], 2560, 1707, 3));
        assert_ne!(k1, preview_source_key(1, &[], 3840, 2560, 4));
        let spot = RetouchSpot {
            x: 0.4,
            y: 0.5,
            source_x: 0.42,
            source_y: 0.51,
            radius: 0.02,
            feather: 0.5,
            opacity: 1.0,
            mode: RetouchMode::Heal,
        };
        assert_ne!(k1, preview_source_key(1, &[spot], 3840, 2560, 3));
    }

    #[test]
    fn pipeline_cache_and_features_probe_on_gpu() {
        let _test_lock = TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let st = status();
        assert!(st.enabled);
        eprintln!(
            "GPU status: name={:?} features={:?} stages_enabled={} max_wg_storage={}",
            st.name, st.features, st.stages_enabled, gpu.limits.max_compute_workgroup_storage_size
        );
        if gpu.features.contains(wgpu::Features::PIPELINE_CACHE) {
            assert!(gpu.pipeline_cache.is_some());
            assert!(st.features.contains(&"PIPELINE_CACHE"));
            gpu.persist_pipeline_cache();
        }
    }
}
