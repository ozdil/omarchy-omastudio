//! Local AI models through ONNX Runtime, on the best accelerator the machine has:
//! an NPU (Intel Core Ultra through OpenVINO, Snapdragon Hexagon through QNN, AMD
//! Ryzen AI through Vitis AI), then a GPU (NVIDIA TensorRT-RTX / TensorRT / CUDA,
//! AMD ROCm / MIGraphX, Intel OpenVINO GPU, WebGPU), then the CPU (XNNPACK, then
//! ONNX Runtime's own).
//!
//! ONNX Runtime is loaded at run time (`libonnxruntime.so`), so OmaStudio builds and
//! runs without it; the system package (`onnxruntime`, `onnxruntime-cuda`,
//! `onnxruntime-rocm`, or a vendor build) adds the accelerators. Models are ONNX files
//! the user installs in `~/.local/share/omastudio/models`; nothing is downloaded.
//!
//! `OMASTUDIO_NPU=off` disables local models; `OMASTUDIO_NPU=<provider>`
//! (`openvino-npu`, `qnn`, `nvrtx`, `tensorrt`, `cuda`, `rocm`, `migraphx`,
//! `openvino-gpu`, `webgpu`, `vitis`, `xnnpack`, `cpu`) forces one.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use ort::environment::Environment;
use ort::ep::{self, ExecutionProvider, ExecutionProviderDispatch};
use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;

/// Execution providers in order of preference: (id, compiled into this ONNX Runtime, builder)
type EpBuilder = fn() -> ExecutionProviderDispatch;
type EpCheck = fn() -> bool;

fn has<E: ExecutionProvider>() -> bool {
    E::is_available().unwrap_or(false)
}

fn cache_subdir(name: &str) -> Option<PathBuf> {
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(v) if Path::new(v.trim()).is_absolute() => PathBuf::from(v.trim()).join("omastudio"),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
            .join(".cache")
            .join("omastudio"),
    };
    let dir = base.join(name);
    crate::security::ensure_secure_dir(&dir).ok()?;
    Some(dir)
}

fn build_nvrtx() -> ExecutionProviderDispatch {
    let mut ep = ep::NVRTX::default();
    if let Some(dir) = cache_subdir("nvrtx-cache") {
        ep = ep.with_runtime_cache_path(dir.to_string_lossy());
    }
    ep.build()
}

fn build_tensorrt() -> ExecutionProviderDispatch {
    let mut ep = ep::TensorRT::default().with_fp16(true);
    if let Some(dir) = cache_subdir("trt-cache") {
        ep = ep.with_engine_cache(true).with_engine_cache_path(dir.to_string_lossy());
    }
    ep.build()
}

const PROVIDERS: &[(&str, EpCheck, EpBuilder)] = &[
    ("openvino-npu", has::<ep::OpenVINO>, || ep::OpenVINO::default().with_device_type("NPU").build()),
    ("qnn", has::<ep::QNN>, || ep::QNN::default().with_backend_path("libQnnHtp.so").build()),
    ("nvrtx", has::<ep::NVRTX>, build_nvrtx),
    ("tensorrt", has::<ep::TensorRT>, build_tensorrt),
    ("cuda", has::<ep::CUDA>, || ep::CUDA::default().build()),
    ("rocm", has::<ep::ROCm>, || ep::ROCm::default().build()),
    ("migraphx", has::<ep::MIGraphX>, || ep::MIGraphX::default().build()),
    ("openvino-gpu", has::<ep::OpenVINO>, || ep::OpenVINO::default().with_device_type("GPU").build()),
    ("webgpu", has::<ep::WebGPU>, || ep::WebGPU::default().build()),
    ("vitis", has::<ep::Vitis>, || ep::Vitis::default().build()),
    ("xnnpack", has::<ep::XNNPACK>, || ep::XNNPACK::default().build()),
];

const SYSTEM_LIB_DIRS: &[&str] = &[
    "/usr/lib",
    "/usr/lib64",
    "/usr/local/lib",
    "/opt/onnxruntime/lib",
    "/opt/rocm/lib",
    "/opt/intel/openvino/runtime/lib",
    "/usr/lib/aarch64-linux-gnu",
    "/usr/lib/x86_64-linux-gnu",
];

fn read_sysfs_vendor(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
}

fn has_accel_vendor(dev_root: &Path, sys_root: &Path, expected_vendor: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dev_root.join("accel")) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("accel") {
            continue;
        }
        let vendor_path = sys_root.join("class/accel").join(name_str).join("device/vendor");
        if read_sysfs_vendor(&vendor_path).as_deref() == Some(expected_vendor) {
            return true;
        }
    }
    false
}

fn has_drm_render_vendor(sys_root: &Path, expected_vendor: Option<&str>) -> bool {
    let Ok(entries) = std::fs::read_dir(sys_root.join("class/drm")) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("renderD") {
            continue;
        }
        match expected_vendor {
            None => return true,
            Some(want) => {
                let vendor_path = entry.path().join("device/vendor");
                if read_sysfs_vendor(&vendor_path).as_deref() == Some(want) {
                    return true;
                }
            }
        }
    }
    false
}

fn has_dev_prefix(dev_root: &Path, prefix: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dev_root) else {
        return false;
    };
    entries
        .flatten()
        .any(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(prefix)))
}

fn hw_present_in(id: &str, dev_root: &Path, sys_root: &Path, is_aarch64: bool) -> bool {
    match id {
        "openvino-npu" => has_accel_vendor(dev_root, sys_root, "0x8086"),
        "vitis" => has_accel_vendor(dev_root, sys_root, "0x1022"),
        "qnn" => is_aarch64 && has_dev_prefix(dev_root, "fastrpc"),
        "nvrtx" | "tensorrt" | "cuda" => dev_root.join("nvidiactl").exists(),
        "rocm" | "migraphx" => dev_root.join("kfd").exists(),
        "openvino-gpu" => has_drm_render_vendor(sys_root, Some("0x8086")),
        "webgpu" => has_drm_render_vendor(sys_root, None) || dev_root.join("nvidiactl").exists(),
        "xnnpack" | "cpu" => true,
        _ => false,
    }
}

/// Zero-dependency Linux hardware presence check so auto-selection does not spend
/// seconds probing unavailable vendor EPs (e.g. CUDA on AMD-only or ROCm on Intel-only).
fn hw_present(id: &str) -> bool {
    hw_present_in(id, Path::new("/dev"), Path::new("/sys"), cfg!(target_arch = "aarch64"))
}

fn parse_so_version(file_name: &str) -> Option<Vec<u32>> {
    let rest = file_name.strip_prefix("libonnxruntime.so.1.")?;
    if rest.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for seg in rest.split('.') {
        let v = seg.parse::<u32>().ok()?;
        parts.push(v);
    }
    Some(parts)
}

fn collect_dir_candidates(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let mut push_unique = |p: PathBuf| {
        if seen.insert(p.clone()) {
            out.push(p);
        }
    };
    push_unique(dir.join("libonnxruntime.so"));
    push_unique(dir.join("libonnxruntime.so.1"));

    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut versioned: Vec<(Vec<u32>, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name();
                let s = name.to_str()?;
                let ver = parse_so_version(s)?;
                Some((ver, e.path()))
            })
            .collect();
        // Prefer newest versioned shared library first (e.g. .so.1.29.0 before .so.1.24.4).
        versioned.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (_, path) in versioned {
            push_unique(path);
        }
    }
}

fn library_candidates_from(env_path: Option<&OsStr>, home: Option<&str>, sys_dirs: &[&str]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    if let Some(p) = env_path
        && !p.is_empty()
    {
        let pb = PathBuf::from(p);
        if seen.insert(pb.clone()) {
            paths.push(pb);
        }
    }
    if let Some(home) = home
        && !home.is_empty()
    {
        collect_dir_candidates(&PathBuf::from(home).join(".local/lib"), &mut paths, &mut seen);
    }
    for dir in sys_dirs {
        collect_dir_candidates(Path::new(dir), &mut paths, &mut seen);
    }
    paths
}

fn library_candidates() -> Vec<PathBuf> {
    let env_path = std::env::var_os("ORT_DYLIB_PATH");
    let home = std::env::var("HOME").ok();
    library_candidates_from(env_path.as_deref(), home.as_deref(), SYSTEM_LIB_DIRS)
}

fn init_environment_from_candidates<T, F>(candidates: &[PathBuf], mut loader: F) -> Result<T, String>
where
    F: FnMut(&Path) -> Result<T, String>,
{
    let existing: Vec<&PathBuf> = candidates.iter().filter(|p| p.is_file()).collect();
    if existing.is_empty() {
        return Err(
            "ONNX Runtime is not installed (libonnxruntime.so not found; install the onnxruntime package)"
                .to_string(),
        );
    }
    let mut errors = Vec::new();
    for lib in existing {
        match loader(lib) {
            Ok(env) => return Ok(env),
            Err(e) => errors.push(format!("{}: {e}", lib.display())),
        }
    }
    Err(format!("Cannot initialize ONNX Runtime ({})", errors.join("; ")))
}

/// Loads ONNX Runtime once, trying every existing candidate library sequentially.
fn environment() -> Result<&'static Environment, String> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| {
        if std::env::var("OMASTUDIO_NPU").is_ok_and(|v| v == "off") {
            return Err("Local models are disabled (OMASTUDIO_NPU=off)".to_string());
        }
        let candidates = library_candidates();
        init_environment_from_candidates(&candidates, |lib| {
            ort::init_from(lib)
                .map_err(|e| format!("Cannot load: {e}"))?
                .with_name("omastudio")
                .build()
                .map_err(|e| format!("Cannot start: {e}"))
        })
    })
    .as_ref()
    .map_err(Clone::clone)
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub available: bool,
    pub reason: Option<String>,
    /// Hardware-verified execution providers ready on this machine, best first.
    pub providers: Vec<&'static str>,
    /// Execution providers compiled into the loaded ONNX Runtime library, best first.
    pub compiled_providers: Vec<&'static str>,
    pub models_dir: String,
    pub models: Vec<String>,
}

pub fn models_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/omastudio/models")
}

pub fn status() -> Status {
    let models = std::fs::read_dir(models_dir())
        .map(|d| {
            let mut m: Vec<String> = d
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".onnx"))
                .collect();
            m.sort();
            m
        })
        .unwrap_or_default();
    let models_dir = models_dir().to_string_lossy().into_owned();
    match environment() {
        Ok(_) => {
            let mut compiled_providers: Vec<&'static str> = PROVIDERS
                .iter()
                .filter(|(_, available, _)| available())
                .map(|(id, ..)| *id)
                .collect();
            compiled_providers.dedup();
            let mut providers: Vec<&'static str> = compiled_providers
                .iter()
                .copied()
                .filter(|id| hw_present(id))
                .collect();
            compiled_providers.push("cpu");
            providers.push("cpu");
            Status {
                available: true,
                reason: None,
                providers,
                compiled_providers,
                models_dir,
                models,
            }
        }
        Err(e) => Status {
            available: false,
            reason: Some(e),
            providers: Vec::new(),
            compiled_providers: Vec::new(),
            models_dir,
            models,
        },
    }
}

/// Opens a model on the first execution provider that accepts it.
pub fn open(model: &Path) -> Result<(Session, &'static str), String> {
    let env = environment()?;
    let forced = std::env::var("OMASTUDIO_NPU").ok().filter(|v| !v.is_empty());
    let mut last_error = String::new();
    for (id, available, build) in PROVIDERS {
        let is_forced = forced.as_deref() == Some(*id);
        if forced.as_deref().is_some_and(|f| f != *id) || !available() {
            continue;
        }
        // Skip absent vendor hardware during automatic selection; allow explicit user overrides.
        if !is_forced && !hw_present(id) {
            continue;
        }
        let attempt = Session::builder(env)
            .and_then(|b| b.with_execution_providers([build().error_on_failure()]).map_err(Into::into))
            .and_then(|mut b| b.commit_from_file(model));
        match attempt {
            Ok(session) => return Ok((session, id)),
            Err(e) => last_error = format!("{id}: {e}"),
        }
    }
    if forced.as_deref().is_some_and(|f| f != "cpu") {
        return Err(format!(
            "Execution provider {} is not usable: {last_error}",
            forced.unwrap_or_default()
        ));
    }
    Session::builder(env)
        .and_then(|mut b| b.commit_from_file(model))
        .map(|s| (s, "cpu"))
        .map_err(|e| format!("Cannot open {}: {e}", model.display()))
}

const MAX_CACHED_SESSIONS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionKey {
    canonical_path: PathBuf,
    mtime_ns: i128,
    file_len: u64,
    forced_ep: Option<String>,
}

impl SessionKey {
    fn for_model(model: &Path, forced_ep: Option<String>) -> Result<Self, String> {
        let canonical_path = std::fs::canonicalize(model).unwrap_or_else(|_| model.to_path_buf());
        let meta = std::fs::metadata(&canonical_path)
            .map_err(|e| format!("Cannot read model {}: {e}", model.display()))?;
        let mtime_ns = meta
            .modified()
            .ok()
            .map(|t| match t.duration_since(UNIX_EPOCH) {
                Ok(d) => d.as_nanos() as i128,
                Err(e) => -(e.duration().as_nanos() as i128),
            })
            .unwrap_or(0);
        Ok(Self {
            canonical_path,
            mtime_ns,
            file_len: meta.len(),
            forced_ep,
        })
    }
}

struct LruCache<K, V> {
    cap: usize,
    tick: u64,
    entries: HashMap<K, (u64, V)>,
}

impl<K: Eq + std::hash::Hash + Clone, V> LruCache<K, V> {
    fn new(cap: usize) -> Self {
        Self { cap: cap.max(1), tick: 0, entries: HashMap::new() }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        let next = self.next_tick();
        if let Some((used, _)) = self.entries.get_mut(key) {
            *used = next;
        }
        self.entries.get(key).map(|(_, v)| v)
    }

    fn insert(&mut self, key: K, value: V) {
        if !self.entries.contains_key(&key) && self.entries.len() >= self.cap
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
        let next = self.next_tick();
        self.entries.insert(key, (next, value));
    }
}

struct CachedSession {
    session: Session,
    provider: &'static str,
}

fn session_cache() -> &'static Mutex<LruCache<SessionKey, Arc<Mutex<CachedSession>>>> {
    static CACHE: OnceLock<Mutex<LruCache<SessionKey, Arc<Mutex<CachedSession>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(MAX_CACHED_SESSIONS)))
}

fn get_or_open_cached(model: &Path) -> Result<Arc<Mutex<CachedSession>>, String> {
    let forced = std::env::var("OMASTUDIO_NPU").ok().filter(|v| !v.is_empty());
    let key = SessionKey::for_model(model, forced)?;
    {
        let mut cache = session_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = cache.get(&key) {
            return Ok(Arc::clone(hit));
        }
    }
    let (session, provider) = open(model)?;
    let entry = Arc::new(Mutex::new(CachedSession { session, provider }));
    let mut cache = session_cache().lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(key, Arc::clone(&entry));
    Ok(entry)
}

/// Input layout and normalisation of a segmentation model, from `<model>.json` when present.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub mean: [f32; 3],
    pub std: [f32; 3],
    /// Input size when the model's own is dynamic
    pub size: u32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        // ImageNet statistics, used by U2-Net, IS-Net, BiRefNet and most segmenters
        Self { mean: [0.485, 0.456, 0.406], std: [0.229, 0.224, 0.225], size: 1024 }
    }
}

fn model_config(model: &Path) -> ModelConfig {
    crate::security::read_secure_file(&model.with_extension("json"), 64 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// A single-channel mask, 0-255.
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub provider: &'static str,
}

const MIN_INPUT_SIDE: u32 = 16;
const MAX_INPUT_SIDE: u32 = 4096;
const MAX_OUTPUT_SIDE: u32 = 8192;

fn validate_model_config(config: &ModelConfig) -> Result<(), String> {
    if !config.mean.iter().all(|v| v.is_finite()) {
        return Err("Model config mean contains non-finite values".to_string());
    }
    if !config.std.iter().all(|v| v.is_finite() && v.abs() >= 1e-6) {
        return Err("Model config std must be finite and have magnitude >= 1e-6".to_string());
    }
    if !(MIN_INPUT_SIDE..=MAX_INPUT_SIDE).contains(&config.size) {
        return Err(format!(
            "Model config size {} is outside supported bounds ({MIN_INPUT_SIDE}..={MAX_INPUT_SIDE})",
            config.size
        ));
    }
    Ok(())
}

fn resolve_input_dims(dims: &[i64], fallback_size: u32) -> Result<(u32, u32, usize), String> {
    if dims.len() != 4 || (dims[1] != 3 && dims[1] > 0) {
        return Err(format!("Expected an input of shape [1, 3, H, W], the model has {dims:?}"));
    }
    let resolve_side = |d: i64| -> Result<u32, String> {
        let side = if d > 0 {
            u32::try_from(d).map_err(|_| format!("Input dimension {d} out of range"))?
        } else {
            fallback_size
        };
        if !(MIN_INPUT_SIDE..=MAX_INPUT_SIDE).contains(&side) {
            return Err(format!(
                "Model input dimension {side} is outside supported bounds ({MIN_INPUT_SIDE}..={MAX_INPUT_SIDE})"
            ));
        }
        Ok(side)
    };
    let h = resolve_side(dims[2])?;
    let w = resolve_side(dims[3])?;
    let plane = (w as usize)
        .checked_mul(h as usize)
        .ok_or_else(|| "Model input plane size overflow".to_string())?;
    plane
        .checked_mul(3)
        .ok_or_else(|| "Model input tensor size overflow".to_string())?;
    Ok((w, h, plane))
}

fn resolve_output_dims(out_dims: &[i64], values_len: usize) -> Result<(u32, u32, usize), String> {
    let [.., oh, ow] = out_dims else {
        return Err(format!("Unexpected output shape {out_dims:?}"));
    };
    if *oh <= 0 || *ow <= 0 || *oh > MAX_OUTPUT_SIDE as i64 || *ow > MAX_OUTPUT_SIDE as i64 {
        return Err(format!("Unexpected output shape {out_dims:?}"));
    }
    let count = (*oh as usize)
        .checked_mul(*ow as usize)
        .ok_or_else(|| format!("Output shape {out_dims:?} overflows buffer size"))?;
    if count > values_len {
        return Err(format!("Output shape {out_dims:?} exceeds tensor length {values_len}"));
    }
    Ok((*oh as u32, *ow as u32, count))
}

/// Runs a salient-object or sky segmentation model (`[1,3,H,W]` in, `[1,1,H,W]` out)
/// on an RGB image and returns the mask at the image's size, reusing cached sessions.
pub fn segment(model: &Path, image: &image::RgbImage) -> Result<Mask, String> {
    let config = model_config(model);
    validate_model_config(&config)?;
    let cached = get_or_open_cached(model)?;
    let mut guard = cached.lock().unwrap_or_else(|e| e.into_inner());
    let provider = guard.provider;
    let session = &mut guard.session;

    let input = session.inputs().first().ok_or("The model has no input")?;
    let input_name = input.name().to_string();
    let shape = input.dtype().tensor_shape().ok_or("The model's input is not a tensor")?;
    let dims: Vec<i64> = shape.iter().copied().collect();
    let (w, h, plane) = resolve_input_dims(&dims, config.size)?;

    let resized = image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle);
    let mut data = vec![0f32; 3 * plane];
    for (i, px) in resized.pixels().enumerate() {
        for c in 0..3 {
            data[c * plane + i] = (px[c] as f32 / 255.0 - config.mean[c]) / config.std[c];
        }
    }
    let tensor = Tensor::from_array(([1usize, 3, h as usize, w as usize], data)).map_err(|e| e.to_string())?;
    let outputs = session
        .run(ort::inputs![input_name.as_str() => tensor])
        .map_err(|e| format!("Model failed: {e}"))?;
    let (out_shape, values) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("Unexpected model output: {e}"))?;
    let out_dims: Vec<i64> = out_shape.iter().copied().collect();
    let (oh, ow, out_count) = resolve_output_dims(&out_dims, values.len())?;
    // The first channel of the output; logits are squashed, probabilities kept
    let first = &values[..out_count];
    let (lo, hi) = first.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let logits = lo < -0.01 || hi > 1.01;
    let small: Vec<u8> = first
        .iter()
        .map(|&v| {
            let p = if logits { 1.0 / (1.0 + (-v).exp()) } else { v };
            (p.clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect();
    let small = image::GrayImage::from_raw(ow, oh, small).ok_or("Mask size mismatch")?;
    let full = image::imageops::resize(&small, image.width(), image.height(), image::imageops::FilterType::Triangle);
    Ok(Mask { width: full.width(), height: full.height(), data: full.into_raw(), provider })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "omastudio_ml_{label}_{}_{}",
            std::process::id(),
            TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn provider_order_matches_linux_priority() {
        let ids: Vec<&str> = PROVIDERS.iter().map(|(id, ..)| *id).collect();
        assert_eq!(
            ids,
            vec![
                "openvino-npu",
                "qnn",
                "nvrtx",
                "tensorrt",
                "cuda",
                "rocm",
                "migraphx",
                "openvino-gpu",
                "webgpu",
                "vitis",
                "xnnpack",
            ]
        );
    }

    #[test]
    fn hw_probe_detects_vendor_devices_accurately() {
        let root = temp_dir("hw_probe");
        let dev = root.join("dev");
        let sys = root.join("sys");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::create_dir_all(&sys).unwrap();

        // Empty fake /dev and /sys: only CPU / XNNPACK should report present.
        for id in ["openvino-npu", "qnn", "nvrtx", "tensorrt", "cuda", "rocm", "migraphx", "openvino-gpu", "webgpu", "vitis"] {
            assert!(!hw_present_in(id, &dev, &sys, false), "{id} should be absent");
        }
        assert!(hw_present_in("xnnpack", &dev, &sys, false));
        assert!(hw_present_in("cpu", &dev, &sys, false));

        // Intel NPU (/dev/accel/accel0 + vendor 0x8086) and AMD XDNA (/dev/accel/accel1 + vendor 0x1022).
        std::fs::create_dir_all(dev.join("accel")).unwrap();
        std::fs::write(dev.join("accel/accel0"), b"").unwrap();
        std::fs::create_dir_all(sys.join("class/accel/accel0/device")).unwrap();
        std::fs::write(sys.join("class/accel/accel0/device/vendor"), b"0x8086\n").unwrap();
        assert!(hw_present_in("openvino-npu", &dev, &sys, false));
        assert!(!hw_present_in("vitis", &dev, &sys, false));

        std::fs::write(dev.join("accel/accel1"), b"").unwrap();
        std::fs::create_dir_all(sys.join("class/accel/accel1/device")).unwrap();
        std::fs::write(sys.join("class/accel/accel1/device/vendor"), b"0x1022\n").unwrap();
        assert!(hw_present_in("vitis", &dev, &sys, false));

        // NVIDIA (/dev/nvidiactl), AMD KFD (/dev/kfd), Intel DRM GPU, Qualcomm fastrpc on aarch64.
        std::fs::write(dev.join("nvidiactl"), b"").unwrap();
        assert!(hw_present_in("nvrtx", &dev, &sys, false));
        assert!(hw_present_in("tensorrt", &dev, &sys, false));
        assert!(hw_present_in("cuda", &dev, &sys, false));
        assert!(hw_present_in("webgpu", &dev, &sys, false));

        std::fs::write(dev.join("kfd"), b"").unwrap();
        assert!(hw_present_in("rocm", &dev, &sys, false));
        assert!(hw_present_in("migraphx", &dev, &sys, false));

        std::fs::create_dir_all(sys.join("class/drm/renderD128/device")).unwrap();
        std::fs::write(sys.join("class/drm/renderD128/device/vendor"), b"0x8086\n").unwrap();
        assert!(hw_present_in("openvino-gpu", &dev, &sys, false));

        std::fs::write(dev.join("fastrpc-cdsp"), b"").unwrap();
        assert!(!hw_present_in("qnn", &dev, &sys, false), "qnn requires aarch64");
        assert!(hw_present_in("qnn", &dev, &sys, true));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn library_candidates_sorts_versioned_so_and_falls_back_sequentially() {
        let root = temp_dir("lib_candidates");
        let dir_a = root.join("a");
        let dir_b = root.join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        let bad_so = dir_a.join("libonnxruntime.so");
        let old_ver = dir_b.join("libonnxruntime.so.1.21.0");
        let new_ver = dir_b.join("libonnxruntime.so.1.29.0");
        let minor_9 = dir_b.join("libonnxruntime.so.1.9.0");
        std::fs::write(&bad_so, b"broken").unwrap();
        std::fs::write(&old_ver, b"old").unwrap();
        std::fs::write(&new_ver, b"new").unwrap();
        std::fs::write(&minor_9, b"oldest").unwrap();

        let dirs = [dir_a.to_str().unwrap(), dir_b.to_str().unwrap()];
        let cands = library_candidates_from(None, None, &dirs);

        // Versioned candidates inside dir_b must sort numerically descending (1.29.0 > 1.21.0 > 1.9.0).
        let pos_new = cands.iter().position(|p| p == &new_ver).unwrap();
        let pos_old = cands.iter().position(|p| p == &old_ver).unwrap();
        let pos_9 = cands.iter().position(|p| p == &minor_9).unwrap();
        assert!(pos_new < pos_old && pos_old < pos_9, "bad version sort: {cands:?}");

        // Sequential loader skips failing candidates and succeeds on the next valid library.
        let mut tried = Vec::new();
        let loaded = init_environment_from_candidates(&cands, |p| {
            tried.push(p.to_path_buf());
            if p == new_ver {
                Ok("ok-1.29")
            } else {
                Err("ABI mismatch".to_string())
            }
        })
        .expect("should fall back to working candidate");
        assert_eq!(loaded, "ok-1.29");
        assert_eq!(tried, vec![bad_so, new_ver]);

        // System library search list includes ROCm and OpenVINO runtime paths.
        assert!(SYSTEM_LIB_DIRS.contains(&"/opt/rocm/lib"));
        assert!(SYSTEM_LIB_DIRS.contains(&"/opt/intel/openvino/runtime/lib"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lru_session_cache_evicts_least_recently_used_and_tracks_file_changes() {
        let mut cache = LruCache::new(MAX_CACHED_SESSIONS);
        cache.insert("m1", 1);
        cache.insert("m2", 2);
        cache.insert("m3", 3);
        // Touch m1 so m2 becomes least recently used.
        assert_eq!(cache.get(&"m1"), Some(&1));
        cache.insert("m4", 4);
        assert_eq!(cache.get(&"m2"), None, "m2 should have been evicted");
        assert_eq!(cache.get(&"m1"), Some(&1));
        assert_eq!(cache.get(&"m3"), Some(&3));
        assert_eq!(cache.get(&"m4"), Some(&4));

        // SessionKey detects file content length changes and forced EP changes.
        let root = temp_dir("session_key");
        let model = root.join("u2net.onnx");
        std::fs::write(&model, b"v1-model-bytes").unwrap();
        let k1 = SessionKey::for_model(&model, None).unwrap();
        let k1_forced = SessionKey::for_model(&model, Some("cuda".into())).unwrap();
        assert_ne!(k1, k1_forced);

        std::fs::write(&model, b"v2-updated-model-bytes-longer").unwrap();
        let k2 = SessionKey::for_model(&model, None).unwrap();
        assert_ne!(k1, k2, "model file change must invalidate SessionKey");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn model_config_rejects_zero_std_nonfinite_and_extreme_sizes() {
        assert!(validate_model_config(&ModelConfig::default()).is_ok());

        let mut zero_std = ModelConfig::default();
        zero_std.std[1] = 0.0;
        assert!(validate_model_config(&zero_std).is_err());

        let mut nan_mean = ModelConfig::default();
        nan_mean.mean[0] = f32::NAN;
        assert!(validate_model_config(&nan_mean).is_err());

        let tiny = ModelConfig { size: 0, ..ModelConfig::default() };
        assert!(validate_model_config(&tiny).is_err());

        let huge = ModelConfig { size: 65535, ..ModelConfig::default() };
        assert!(validate_model_config(&huge).is_err());
    }

    #[test]
    fn tensor_dimension_validation_rejects_negative_and_oversized_dims() {
        assert_eq!(resolve_input_dims(&[1, 3, -1, -1], 512).unwrap(), (512, 512, 512 * 512));
        assert!(resolve_input_dims(&[1, 3, 8, 512], 512).is_err());
        assert!(resolve_input_dims(&[1, 3, 16384, 16384], 512).is_err());
        assert!(resolve_input_dims(&[1, 4, 512, 512], 512).is_err());

        // (-1) * (-1) == 1 <= values_len must still be rejected
        assert!(resolve_output_dims(&[1, 1, -1, -1], 1024).is_err());
        assert!(resolve_output_dims(&[1, 1, 0, 100], 1024).is_err());
        assert!(resolve_output_dims(&[1, 1, 9000, 9000], usize::MAX).is_err());
        assert!(resolve_output_dims(&[1, 1, 64, 64], 100).is_err());
        assert_eq!(resolve_output_dims(&[1, 1, 32, 16], 512).unwrap(), (32, 16, 512));
    }
}
