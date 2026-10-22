//! GPU compute through wgpu: Vulkan on NVIDIA (proprietary driver and NVK), AMD
//! (RADV), Intel (ANV) and Qualcomm Adreno (Turnip), OpenGL ES 3.1 where Vulkan is
//! missing. Every GPU path has a CPU twin and any failure falls back to it, so the
//! GPU only ever makes things faster.
//!
//! `OMASTUDIO_GPU=off` disables it. Software rasterisers (llvmpipe/lavapipe) are
//! skipped because the CPU path is faster than them; `OMASTUDIO_GPU=software` allows
//! them, which the tests use on machines without a GPU.

pub mod demosaic;

use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub limits: wgpu::Limits,
    /// One job at a time: jobs size their buffers to the device's limits.
    jobs: Mutex<()>,
    demosaic: OnceLock<Result<demosaic::Pipelines, String>>,
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

static GPU: OnceLock<Result<Gpu, String>> = OnceLock::new();

/// The GPU, initialised on first use, or None (disabled, absent or unusable).
pub fn get() -> Option<&'static Gpu> {
    GPU.get_or_init(init).as_ref().ok()
}

pub fn status() -> GpuStatus {
    match GPU.get_or_init(init) {
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
            reason: Some(e.clone()),
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
        0x10005 => "Mesa (software)",
        _ => "Other",
    }
}

fn init() -> Result<Gpu, String> {
    let mode = std::env::var("OMASTUDIO_GPU").unwrap_or_default();
    if matches!(mode.as_str(), "off" | "0" | "cpu") {
        return Err("Disabled by OMASTUDIO_GPU".to_string());
    }
    let allow_software = mode == "software";
    let backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
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
    let adapter = adapters
        .into_iter()
        .filter(|a| a.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS))
        .filter(|a| allow_software || a.get_info().device_type != wgpu::DeviceType::Cpu)
        .min_by_key(rank)
        .ok_or_else(|| "No GPU with compute shaders found".to_string())?;

    let limits = adapter.limits();
    if limits.max_storage_buffers_per_shader_stage < 7 {
        return Err(format!("{}: too few storage buffers per shader", adapter.get_info().name));
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("omastudio"),
        required_features: wgpu::Features::empty(),
        required_limits: limits.clone(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| format!("{}: {}", adapter.get_info().name, e))?;
    // Errors are caught by error scopes around each job; anything else is logged
    // instead of wgpu's default panic
    device.on_uncaptured_error(Arc::new(|e| eprintln!("omastudio: GPU error: {e}")));
    Ok(Gpu { device, queue, info: adapter.get_info(), limits, jobs: Mutex::new(()), demosaic: OnceLock::new() })
}

/// Views a slice of plain numbers as bytes (Linux targets are little-endian, which
/// is what GPU buffers expect).
fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: T is a plain number type (f32/u32), any byte pattern of it is readable
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}
