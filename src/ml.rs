//! Local AI models through ONNX Runtime, on the best accelerator the machine has:
//! an NPU (Intel Core Ultra through OpenVINO, Snapdragon Hexagon through QNN, AMD
//! Ryzen AI through Vitis AI), then a GPU (CUDA, ROCm/MIGraphX, OpenVINO GPU,
//! WebGPU), then the CPU (XNNPACK, then ONNX Runtime's own).
//!
//! ONNX Runtime is loaded at run time (`libonnxruntime.so`), so OmaStudio builds and
//! runs without it; the system package (`onnxruntime`, or a build with the vendor's
//! execution provider) adds the accelerators. Models are ONNX files the user installs
//! in `~/.local/share/omastudio/models`; nothing is downloaded.
//!
//! `OMASTUDIO_NPU=off` disables local models; `OMASTUDIO_NPU=<provider>` (openvino-npu,
//! qnn, vitis, cuda, migraphx, openvino-gpu, webgpu, xnnpack, cpu) forces one.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ort::ep::{self, ExecutionProvider, ExecutionProviderDispatch};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;

/// Execution providers in order of preference: (id, compiled into this ONNX Runtime, builder)
type EpBuilder = fn() -> ExecutionProviderDispatch;
type EpCheck = fn() -> bool;
fn has<E: ExecutionProvider>() -> bool {
    E::is_available().unwrap_or(false)
}
const PROVIDERS: &[(&str, EpCheck, EpBuilder)] = &[
    ("openvino-npu", has::<ep::OpenVINO>, || ep::OpenVINO::default().with_device_type("NPU").build()),
    ("qnn", has::<ep::QNN>, || ep::QNN::default().with_backend_path("libQnnHtp.so").build()),
    ("vitis", has::<ep::Vitis>, || ep::Vitis::default().build()),
    ("cuda", has::<ep::CUDA>, || ep::CUDA::default().build()),
    ("migraphx", has::<ep::MIGraphX>, || ep::MIGraphX::default().build()),
    ("openvino-gpu", has::<ep::OpenVINO>, || ep::OpenVINO::default().with_device_type("GPU").build()),
    ("webgpu", has::<ep::WebGPU>, || ep::WebGPU::default().build()),
    ("xnnpack", has::<ep::XNNPACK>, || ep::XNNPACK::default().build()),
];

fn library_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("ORT_DYLIB_PATH") {
        paths.push(PathBuf::from(p));
    }
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(&home).join(".local/lib/libonnxruntime.so"));
    }
    for dir in ["/usr/lib", "/usr/lib64", "/usr/local/lib", "/opt/onnxruntime/lib", "/usr/lib/aarch64-linux-gnu", "/usr/lib/x86_64-linux-gnu"] {
        paths.push(Path::new(dir).join("libonnxruntime.so"));
        paths.push(Path::new(dir).join("libonnxruntime.so.1"));
    }
    paths
}

/// Loads ONNX Runtime once; the error says why local models are unavailable.
fn environment() -> Result<&'static Environment, String> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| {
        if std::env::var("OMASTUDIO_NPU").is_ok_and(|v| v == "off") {
            return Err("Local models are disabled (OMASTUDIO_NPU=off)".to_string());
        }
        let lib = library_candidates()
            .into_iter()
            .find(|p| p.is_file())
            .ok_or("ONNX Runtime is not installed (libonnxruntime.so not found; install the onnxruntime package)")?;
        ort::init_from(&lib)
            .map_err(|e| format!("Cannot load {}: {e}", lib.display()))?
            .with_name("omastudio")
            .build()
            .map_err(|e| format!("Cannot start ONNX Runtime: {e}"))
    })
    .as_ref()
    .map_err(Clone::clone)
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub available: bool,
    pub reason: Option<String>,
    /// Execution providers this ONNX Runtime build includes, best first
    pub providers: Vec<&'static str>,
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
            let mut providers: Vec<&'static str> = PROVIDERS
                .iter()
                .filter(|(_, available, _)| available())
                .map(|(id, ..)| *id)
                .collect();
            providers.dedup();
            providers.push("cpu");
            Status { available: true, reason: None, providers, models_dir, models }
        }
        Err(e) => Status { available: false, reason: Some(e), providers: Vec::new(), models_dir, models },
    }
}

/// Opens a model on the first execution provider that accepts it.
pub fn open(model: &Path) -> Result<(Session, &'static str), String> {
    let env = environment()?;
    let forced = std::env::var("OMASTUDIO_NPU").ok().filter(|v| !v.is_empty());
    let mut last_error = String::new();
    for (id, available, build) in PROVIDERS {
        if forced.as_deref().is_some_and(|f| f != *id) || !available() {
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
        return Err(format!("Execution provider {} is not usable: {last_error}", forced.unwrap_or_default()));
    }
    Session::builder(env)
        .and_then(|mut b| b.commit_from_file(model))
        .map(|s| (s, "cpu"))
        .map_err(|e| format!("Cannot open {}: {e}", model.display()))
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

/// Runs a salient-object or sky segmentation model (`[1,3,H,W]` in, `[1,1,H,W]` out)
/// on an RGB image and returns the mask at the image's size.
pub fn segment(model: &Path, image: &image::RgbImage) -> Result<Mask, String> {
    let config = model_config(model);
    let (mut session, provider) = open(model)?;
    let input = session.inputs().first().ok_or("The model has no input")?;
    let input_name = input.name().to_string();
    let shape = input.dtype().tensor_shape().ok_or("The model's input is not a tensor")?;
    let dims: Vec<i64> = shape.iter().copied().collect();
    if dims.len() != 4 || (dims[1] != 3 && dims[1] > 0) {
        return Err(format!("Expected an input of shape [1, 3, H, W], the model has {dims:?}"));
    }
    let side = |d: i64| if d > 0 { d as u32 } else { config.size };
    let (w, h) = (side(dims[3]), side(dims[2]));

    let resized = image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle);
    let plane = (w * h) as usize;
    let mut data = vec![0f32; 3 * plane];
    for (i, px) in resized.pixels().enumerate() {
        for c in 0..3 {
            data[c * plane + i] = (px[c] as f32 / 255.0 - config.mean[c]) / config.std[c];
        }
    }
    let tensor = Tensor::from_array(([1usize, 3, h as usize, w as usize], data)).map_err(|e| e.to_string())?;
    let outputs = session.run(ort::inputs![input_name.as_str() => tensor]).map_err(|e| format!("Model failed: {e}"))?;
    let (out_shape, values) = outputs[0].try_extract_tensor::<f32>().map_err(|e| format!("Unexpected model output: {e}"))?;
    let out_dims: Vec<i64> = out_shape.iter().copied().collect();
    let (oh, ow) = match out_dims.as_slice() {
        [.., oh, ow] if (*oh * *ow) as usize <= values.len() => (*oh as u32, *ow as u32),
        _ => return Err(format!("Unexpected output shape {out_dims:?}")),
    };
    // The first channel of the output; logits are squashed, probabilities kept
    let first = &values[..(oh * ow) as usize];
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
