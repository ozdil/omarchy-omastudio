//! The neighbourhood stage on the GPU: presence (texture, clarity, dehaze, midtone
//! detail), detail (colour and luminance noise reduction, sharpening) and defringe,
//! on a whole RGB f32 buffer at once. The kernels in `local.wgsl` mirror
//! [`crate::pipeline::presence::apply_presence_scaled`],
//! [`crate::pipeline::detail::apply_detail_scaled`] and
//! [`crate::pipeline::detail::apply_defringe_buffer`] and run in the same order; any
//! failure returns false and the caller uses the CPU.

use std::sync::mpsc;

use super::{Gpu, as_bytes};
use crate::pipeline::presence::scaled_radius;
use crate::recipe::Recipe;

/// Planes of w*h floats the kernels work in (see the shader).
const PLANES: u64 = 6;
/// Largest buffer, whatever the device allows.
const MAX_BUFFER: u64 = 1 << 30;
/// Pixels each blur thread slides over; must match CHUNK in the shader.
const CHUNK: u32 = 32;
/// Size of one step's parameters in the uniform buffer.
const PARAMS_SIZE: u64 = 128;
/// Histogram workgroups; must match HIST_GROUPS in the shader.
const HIST_GROUPS: u32 = 256;

const ENTRIES: &[&str] = &[
    "luma_init",
    "blur_h",
    "blur_v",
    "accum",
    "dark",
    "presence_apply",
    "detail_extract",
    "luma_filter",
    "reconstruct",
    "gradient",
    "defringe_apply",
    "finish",
    "layers",
    "geometry",
    "pack3",
    "hist_main",
    "hist_wf",
    "hist_par",
];

/// A step's parameter words, zero after `head`.
fn words(head: &[u32]) -> [u32; 32] {
    let mut p = [0u32; 32];
    p[..head.len()].copy_from_slice(head);
    p
}

fn entry(name: &str) -> usize {
    ENTRIES.iter().position(|e| *e == name).unwrap_or_else(|| panic!("unknown entry point {name}"))
}

/// One dispatch: its kernel, parameters and workgroup counts.
struct Step {
    entry: usize,
    params: [u32; 32],
    groups: (u32, u32),
}

/// The dispatches for `recipe` on a `w` x `h` frame, in the CPU's order. Empty when
/// none of the three passes has anything to do.
fn plan(w: usize, h: usize, recipe: &Recipe, scale: f32) -> Vec<Step> {
    let (wu, hu) = (w as u32, h as u32);
    let px = (wu.div_ceil(16), hu.div_ceil(16));
    let mut steps = Vec::new();
    let step = |name: &str, src: u32, dst: u32, r: u32, mode: u32, flags: u32, f: [f32; 4], groups: (u32, u32)| Step {
        entry: entry(name),
        params: words(&[wu, hu, src, dst, r, mode, flags, 0, f[0].to_bits(), f[1].to_bits(), f[2].to_bits(), f[3].to_bits()]),
        groups,
    };
    let pixels = |name: &str, mode: u32, flags: u32, f: [f32; 4]| step(name, 0, 0, 0, mode, flags, f, px);
    // fast_separable_blur's radius, clamped the same way, through the scratch plane 4
    let blur = |steps: &mut Vec<Step>, src: u32, dst: u32, radius: usize| {
        let r = radius.min(w / 2).min(h / 2).max(1) as u32;
        steps.push(step("blur_h", src, 4, r, 0, 0, [0.0; 4], (wu.div_ceil(CHUNK).div_ceil(64), hu)));
        steps.push(step("blur_v", 4, dst, r, 0, 0, [0.0; 4], (wu.div_ceil(64), hu.div_ceil(CHUNK))));
    };

    // Presence
    let (clarity, texture, dehaze, md) = (recipe.clarity, recipe.texture, recipe.dehaze, recipe.midtone_detail);
    let any = clarity.abs() >= 0.01 || texture.abs() >= 0.01 || dehaze.abs() >= 0.01 || md.abs() >= 0.01;
    let modules = texture.abs() >= 0.5 || clarity.abs() >= 0.5 || dehaze.abs() >= 0.5 || md.abs() >= 0.5;
    if any && modules {
        steps.push(pixels("luma_init", 0, 0, [0.0; 4]));
        if texture.abs() >= 0.5 {
            blur(&mut steps, 0, 1, scaled_radius(2, scale));
            blur(&mut steps, 0, 2, scaled_radius(6, scale));
            steps.push(pixels("accum", 0, 0, [(texture / 100.0) * 1.6, 0.0, 0.0, 0.0]));
        }
        if clarity.abs() >= 0.5 {
            blur(&mut steps, 0, 1, scaled_radius(16, scale));
            steps.push(pixels("accum", 1, 0, [(clarity / 100.0) * 0.85, 0.0, 0.0, 0.0]));
        }
        if md.abs() >= 0.5 {
            blur(&mut steps, 0, 1, scaled_radius(3, scale));
            blur(&mut steps, 0, 2, scaled_radius(9, scale));
            steps.push(pixels("accum", 2, 0, [(md / 100.0) * 1.25, 0.0, 0.0, 0.0]));
        }
        let dehaze_on = dehaze.abs() >= 0.5;
        if dehaze_on {
            steps.push(pixels("dark", 0, 0, [0.0; 4]));
            blur(&mut steps, 5, 1, scaled_radius(12, scale));
        }
        steps.push(pixels("presence_apply", 0, dehaze_on as u32, [dehaze / 100.0, 0.0, 0.0, 0.0]));
    }

    // Detail
    let (sharpness, dn_lum, dn_col) = (recipe.sharpness, recipe.denoise_lum, recipe.denoise_col);
    if (sharpness > 0.1 || dn_lum > 0.1 || dn_col > 0.1) && w >= 3 && h >= 3 {
        steps.push(pixels("detail_extract", 0, 0, [0.0; 4]));
        let chroma = dn_col > 0.5;
        if chroma {
            let radius = scaled_radius(if dn_col > 40.0 { 2 } else { 1 }, scale);
            blur(&mut steps, 1, 3, radius);
            blur(&mut steps, 2, 5, radius);
        }
        let flags = (dn_lum > 0.5) as u32 | ((sharpness > 0.5) as u32) << 1;
        steps.push(pixels("luma_filter", 0, flags, [(dn_lum / 100.0) * 0.75, (sharpness / 100.0) * 0.80, 0.0, 0.0]));
        steps.push(pixels("reconstruct", 0, chroma as u32, [(dn_col / 100.0).clamp(0.0, 1.0), 0.0, 0.0, 0.0]));
    }

    // Defringe
    if recipe.defringe > 0.01 && w >= 3 && h >= 3 {
        steps.push(pixels("luma_init", 0, 0, [0.0; 4]));
        steps.push(pixels("gradient", 0, 0, [0.0; 4]));
        blur(&mut steps, 1, 2, scaled_radius(2, scale));
        steps.push(pixels("defringe_apply", 0, 0, [recipe.defringe, 0.0, 0.0, 0.0]));
    }
    steps
}

/// The adjustment layers of `recipe` packed for the shader and the step that applies
/// them to a `w` x `h` band whose first pixel is (`frame.0`, `frame.1`) of a
/// `frame.2` x `frame.3` frame; None when there are no layers.
fn layer_step(w: usize, h: usize, frame: (usize, usize, usize, usize), recipe: &Recipe) -> Result<Option<(Vec<f32>, Step)>, String> {
    if recipe.layers.is_empty() {
        return Ok(None);
    }
    let lay = super::layers::pack(&recipe.layers).ok_or("masks too large for the GPU")?;
    let (inv_w, inv_h) = (1.0 / (frame.2 as f32).max(1.0), 1.0 / (frame.3 as f32).max(1.0));
    let (wu, hu) = (w as u32, h as u32);
    let params = words(&[wu, hu, 0, 0, frame.0 as u32, frame.1 as u32, 0, 0, inv_w.to_bits(), inv_h.to_bits()]);
    Ok(Some((lay, Step { entry: entry("layers"), params, groups: (wu.div_ceil(16), hu.div_ceil(16)) })))
}

/// The display tail after `plan`'s steps: grain, lens distortion and perspective,
/// vignette and 8-bit quantization into px8, packing to 3 bytes per pixel when `ch`
/// is 3, and the histogram and scopes. It mirrors process_buffer_16_to_8_ex's passes
/// 4 to 6.
fn display_steps(w: usize, h: usize, ch: usize, recipe: &Recipe, scale: f32) -> Vec<Step> {
    let (wu, hu) = (w as u32, h as u32);
    let px = (wu.div_ceil(16), hu.div_ceil(16));
    let mut steps = Vec::new();
    let grain = recipe.grain_amount > 0.001;
    let vignette = recipe.vignette.abs() >= 0.01;
    let f = [
        (recipe.grain_amount / 100.0) * 0.12,
        (recipe.grain_size.max(1.0) * scale).max(1.0),
        scale.max(1.0),
        recipe.grain_roughness / 100.0,
        recipe.vignette,
    ];
    let finish = |mode: u32, flags: u32| {
        let mut params = words(&[wu, hu, 0, 0, 0, mode, flags]);
        for (k, v) in f.iter().enumerate() {
            params[8 + k] = v.to_bits();
        }
        Step { entry: entry("finish"), params, groups: px }
    };
    match crate::pipeline::detail::Geometry::new(w, h, recipe) {
        Some(geometry) => {
            // Grain in place, the resampled frame into the planes, then the vignette
            if grain {
                steps.push(finish(1, 1));
            }
            let (terms, perspective, lens) = geometry.gpu_terms();
            let mut params = words(&[wu, hu, 0, 0, 0, 0, perspective as u32 | (lens as u32) << 1]);
            for (k, v) in terms.iter().enumerate() {
                params[16 + k] = v.to_bits();
            }
            steps.push(Step { entry: entry("geometry"), params, groups: px });
            steps.push(finish(2, (vignette as u32) << 1));
        }
        None => steps.push(finish(0, grain as u32 | (vignette as u32) << 1)),
    }
    if ch == 3 {
        let groups = ((w * h * 3).div_ceil(4) as u32).div_ceil(256);
        let gx = groups.min(32768);
        let params = words(&[wu, hu, gx * 256]);
        steps.push(Step { entry: entry("pack3"), params, groups: (gx, groups.div_ceil(gx)) });
    }
    for name in ["hist_main", "hist_wf", "hist_par"] {
        steps.push(Step { entry: entry(name), params: words(&[wu, hu]), groups: (HIST_GROUPS, 1) });
    }
    steps
}

pub struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipelines: Vec<wgpu::ComputePipeline>,
    /// Kept between calls; previews call this on every edit.
    buffers: std::sync::Mutex<Option<Buffers>>,
}

/// Core compute buffers shared by both `Out::F32` (48 B/px including lazy `staging_f32`)
/// and `Out::Display` (47 B/px together with `DisplayBuffers`, without `staging_f32`).
struct ComputeBuffers {
    rgb_size: u64,
    plane_size: u64,
    params_size: u64,
    layers_size: u64,
    params: wgpu::Buffer,
    rgb: wgpu::Buffer,
    planes: wgpu::Buffer,
    layers: wgpu::Buffer,
    /// Minimal 4-byte placeholders bound to display slots when `Out::F32` runs alone.
    _dummy_px8: wgpu::Buffer,
    _dummy_out8: wgpu::Buffer,
    _dummy_hist: wgpu::Buffer,
    /// Allocated lazily only for `Out::F32` readbacks (`rgb_size`, 12 B/px).
    staging_f32: Option<wgpu::Buffer>,
    bind_compute: wgpu::BindGroup,
}

/// Display-only buffers (`Out::Display`), keeping `staging_display` at 4 B/px (`w*h*4`)
/// instead of 12 B/px and avoiding display allocations during `Out::F32` exports.
struct DisplayBuffers {
    pixel_cap: u64,
    hist_size: u64,
    px8: wgpu::Buffer,
    out8: wgpu::Buffer,
    hist: wgpu::Buffer,
    staging_hist: wgpu::Buffer,
    staging_display: wgpu::Buffer,
    bind_display: wgpu::BindGroup,
}

struct Buffers {
    compute: ComputeBuffers,
    display: Option<DisplayBuffers>,
    last_used: std::time::Instant,
}

fn create_buf(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(4).next_multiple_of(4),
        usage,
        mapped_at_creation: false,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    params: &wgpu::Buffer,
    rgb: &wgpu::Buffer,
    planes: &wgpu::Buffer,
    px8: &wgpu::Buffer,
    out8: &wgpu::Buffer,
    hist: &wgpu::Buffer,
    layers: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: params,
                    offset: 0,
                    size: wgpu::BufferSize::new(PARAMS_SIZE),
                }),
            },
            wgpu::BindGroupEntry { binding: 1, resource: rgb.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: planes.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: px8.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: out8.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: hist.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 6, resource: layers.as_entire_binding() },
        ],
    })
}

impl ComputeBuffers {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, params_size: u64, rgb_size: u64, plane_size: u64, layers_size: u64) -> Self {
        let params = create_buf(device, "local params", params_size, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let rgb = create_buf(device, "local rgb", rgb_size, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC);
        let planes = create_buf(device, "local planes", plane_size * PLANES, wgpu::BufferUsages::STORAGE);
        let layers = create_buf(device, "local layers", layers_size, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let dummy_px8 = create_buf(device, "local dummy px8", 4, wgpu::BufferUsages::STORAGE);
        let dummy_out8 = create_buf(device, "local dummy out8", 4, wgpu::BufferUsages::STORAGE);
        let dummy_hist = create_buf(device, "local dummy hist", 4, wgpu::BufferUsages::STORAGE);
        let bind_compute = create_bind_group(
            device,
            layout,
            "local compute",
            &params,
            &rgb,
            &planes,
            &dummy_px8,
            &dummy_out8,
            &dummy_hist,
            &layers,
        );
        Self {
            rgb_size,
            plane_size,
            params_size,
            layers_size,
            params,
            rgb,
            planes,
            layers,
            _dummy_px8: dummy_px8,
            _dummy_out8: dummy_out8,
            _dummy_hist: dummy_hist,
            staging_f32: None,
            bind_compute,
        }
    }

    fn fits(&self, params: u64, rgb: u64, plane: u64, layers: u64) -> bool {
        self.params_size >= params && self.rgb_size >= rgb && self.plane_size >= plane && self.layers_size >= layers
    }

    fn ensure_staging_f32(&mut self, device: &wgpu::Device) -> &wgpu::Buffer {
        let size = self.rgb_size;
        self.staging_f32.get_or_insert_with(|| {
            create_buf(device, "local staging f32", size, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST)
        })
    }

    fn total_bytes(&self) -> u64 {
        let staging = self.staging_f32.as_ref().map_or(0, |_| self.rgb_size);
        self.params_size
            .saturating_add(self.rgb_size)
            .saturating_add(self.plane_size.saturating_mul(PLANES))
            .saturating_add(self.layers_size)
            .saturating_add(staging)
    }
}

impl DisplayBuffers {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, compute: &ComputeBuffers, pixel_cap: u64) -> Self {
        let px8_bytes = pixel_cap.saturating_mul(4);
        let out8_bytes = pixel_cap.saturating_mul(3).next_multiple_of(4);
        let px8 = create_buf(device, "local px8", px8_bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let out8 = create_buf(device, "local out8", out8_bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        // The counts, then the luma terms (see LUMA_TERMS in the shader)
        let hist_size = ((crate::pipeline::histogram::SCOPE_WORDS + 768) * 4) as u64;
        let hist = create_buf(device, "local hist", hist_size, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST);
        let staging_hist = create_buf(device, "local staging hist", hist_size, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        // Sized for at most 4 bytes/pixel (RGBA8), never 12 bytes/pixel (RGB f32)
        let staging_display = create_buf(device, "local staging display", px8_bytes, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        let bind_display = create_bind_group(
            device,
            layout,
            "local display",
            &compute.params,
            &compute.rgb,
            &compute.planes,
            &px8,
            &out8,
            &hist,
            &compute.layers,
        );
        Self { pixel_cap, hist_size, px8, out8, hist, staging_hist, staging_display, bind_display }
    }

    fn total_bytes(&self) -> u64 {
        let px8_bytes = self.pixel_cap.saturating_mul(4);
        let out8_bytes = self.pixel_cap.saturating_mul(3).next_multiple_of(4);
        px8_bytes
            .saturating_add(out8_bytes)
            .saturating_add(px8_bytes)
            .saturating_add(self.hist_size.saturating_mul(2))
    }
}

impl Buffers {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, params_size: u64, rgb_size: u64, plane_size: u64, layers_size: u64) -> Self {
        Self {
            compute: ComputeBuffers::new(device, layout, params_size, rgb_size, plane_size, layers_size),
            display: None,
            last_used: std::time::Instant::now(),
        }
    }

    fn total_bytes(&self) -> u64 {
        self.compute
            .total_bytes()
            .saturating_add(self.display.as_ref().map_or(0, DisplayBuffers::total_bytes))
    }
}

impl Pipeline {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("local"),
            source: wgpu::ShaderSource::Wgsl(include_str!("local.wgsl").into()),
        });
        let storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("local"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(PARAMS_SIZE),
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                storage(3),
                storage(4),
                storage(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("local"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = ENTRIES
            .iter()
            .map(|name| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(name),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
            .collect();
        Self { layout, pipelines, buffers: std::sync::Mutex::new(None) }
    }
}

/// Opens the GPU and compiles the stage's shader ahead of the first use.
pub fn warm_up() {
    if let Some(gpu) = super::acquire().filter(|g| super::stages_worthwhile(g)) {
        let _ = pipeline(&gpu);
    }
}

fn pipeline(gpu: &Gpu) -> Result<&Pipeline, String> {
    gpu.local
        .get_or_init(|| {
            if gpu.limits.max_storage_buffers_per_shader_stage < 6 {
                return Err(format!(
                    "{}: local stage needs at least 6 storage buffers per shader (has {})",
                    gpu.info.name, gpu.limits.max_storage_buffers_per_shader_stage
                ));
            }
            if gpu.limits.max_compute_workgroup_storage_size < 16_384 {
                return Err(format!(
                    "{}: local stage needs at least 16384 bytes of workgroup storage (has {})",
                    gpu.info.name, gpu.limits.max_compute_workgroup_storage_size
                ));
            }
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let p = Pipeline::new(&gpu.device);
            match pollster::block_on(scope.pop()) {
                Some(e) => Err(format!("shader: {e}")),
                None => Ok(p),
            }
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// Runs presence, detail and defringe on `buf` in place, distinguishing intentional skips
/// from runtime GPU failures for per-frame circuit breaking.
pub fn run_checked(buf: &mut [f32], w: usize, h: usize, recipe: &Recipe, scale: f32) -> super::StageOutcome<()> {
    if w * h < super::point_ops::MIN_PIXELS {
        return super::StageOutcome::Skipped;
    }
    let Some(gpu) = super::acquire().filter(|g| super::stages_worthwhile(g)) else {
        return super::StageOutcome::Skipped;
    };
    match run_on(&gpu, buf, w, h, recipe, scale) {
        Ok(()) => super::StageOutcome::Done(()),
        Err(e) => {
            eprintln!("omastudio: GPU presence/detail stage failed, using the CPU: {e}");
            super::StageOutcome::Failed
        }
    }
}

/// Runs presence, detail and defringe on `buf` (`w` x `h` interleaved RGB f32) in
/// place, with neighbourhoods sized for a frame `scale` times the reference. Returns
/// false when there is no usable GPU or the job failed; `buf` is then unchanged.
pub fn run(buf: &mut [f32], w: usize, h: usize, recipe: &Recipe, scale: f32) -> bool {
    matches!(run_checked(buf, w, h, recipe, scale), super::StageOutcome::Done(()))
}

fn run_on(gpu: &Gpu, buf: &mut [f32], w: usize, h: usize, recipe: &Recipe, scale: f32) -> Result<(), String> {
    if w == 0 || h == 0 || buf.len() < w * h * 3 {
        return Err("unsupported buffer".to_string());
    }
    let steps = plan(w, h, recipe, scale);
    if steps.is_empty() {
        return Ok(());
    }
    let _job = gpu.jobs.lock().map_err(|_| "GPU lock poisoned")?;
    execute(gpu, w, h, &steps, &[], Out::F32(&mut buf[..w * h * 3]), None::<fn(&wgpu::Buffer) -> Result<(), String>>)
}

/// Chained point + layers + presence/detail stage returning [`super::StageOutcome`] so
/// callers can trip a per-frame circuit breaker only on actual GPU failures (not when
/// the recipe simply has no local steps to chain).
#[allow(clippy::too_many_arguments)]
pub fn run_chained_checked(
    input: &[u16],
    w: usize,
    ch: usize,
    y0: usize,
    y1: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
    frame: (usize, usize, usize, usize),
) -> super::StageOutcome<Vec<f32>> {
    if (y1 - y0) * w < super::point_ops::MIN_PIXELS {
        return super::StageOutcome::Skipped;
    }
    let Some(gpu) = super::acquire().filter(|g| super::stages_worthwhile(g)) else {
        return super::StageOutcome::Skipped;
    };
    match chained_on(&gpu, input, w, ch, y0, y1, recipe, inputs, scale, frame) {
        Ok(Some(v)) => super::StageOutcome::Done(v),
        Ok(None) => super::StageOutcome::Skipped,
        Err(e) => {
            eprintln!("omastudio: GPU point + presence/detail stages failed, using the CPU: {e}");
            super::StageOutcome::Failed
        }
    }
}

/// Point stage, adjustment layers and this stage in one go for rows `y0..y1` of
/// `input` (`w` pixels of `ch` 16-bit channels per row): the point stage's result
/// stays on the GPU and only the final RGB f32 comes back. `frame` places the band for
/// the masks: the frame position of its first pixel and the frame's size. None when
/// there is no usable GPU, nothing for these stages to do (the caller then runs the
/// point stage alone) or the job failed.
#[allow(clippy::too_many_arguments)]
pub fn run_chained(
    input: &[u16],
    w: usize,
    ch: usize,
    y0: usize,
    y1: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
    frame: (usize, usize, usize, usize),
) -> Option<Vec<f32>> {
    match run_chained_checked(input, w, ch, y0, y1, recipe, inputs, scale, frame) {
        super::StageOutcome::Done(v) => Some(v),
        super::StageOutcome::Skipped | super::StageOutcome::Failed => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn chained_on(
    gpu: &Gpu,
    input: &[u16],
    w: usize,
    ch: usize,
    y0: usize,
    y1: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
    frame: (usize, usize, usize, usize),
) -> Result<Option<Vec<f32>>, String> {
    let h = y1.saturating_sub(y0);
    if w == 0 || h == 0 || !(3..=4).contains(&ch) || input.len() < y1 * w * ch {
        return Err("unsupported buffer".to_string());
    }
    let (lay, mut steps) = match layer_step(w, h, frame, recipe)? {
        Some((lay, step)) => (lay, vec![step]),
        None => (Vec::new(), Vec::new()),
    };
    steps.extend(plan(w, h, recipe, scale));
    if steps.is_empty() {
        return Ok(None);
    }
    let _job = gpu.jobs.lock().map_err(|_| "GPU lock poisoned")?;
    let mut out = vec![0.0f32; w * h * 3];
    execute(
        gpu,
        w,
        h,
        &steps,
        &lay,
        Out::F32(&mut out),
        Some(|rgb: &wgpu::Buffer| super::point_ops::run_into(gpu, input, w, ch, y0, y1, recipe, inputs, rgb)),
    )?;
    Ok(Some(out))
}

/// Whole 8-bit preview on the GPU, distinguishing intentional skips from runtime GPU
/// failures so callers do not retry downstream GPU stages on the same failing frame.
#[allow(clippy::too_many_arguments)]
pub fn run_display_checked(
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
) -> super::StageOutcome<(Vec<u8>, crate::pipeline::histogram::HistogramData)> {
    if w * h < super::point_ops::MIN_PIXELS {
        return super::StageOutcome::Skipped;
    }
    let Some(gpu) = super::acquire().filter(|g| super::stages_worthwhile(g)) else {
        return super::StageOutcome::Skipped;
    };
    match display_on(&gpu, input, w, h, ch, recipe, inputs, scale) {
        Ok(v) => super::StageOutcome::Done(v),
        Err(e) => {
            eprintln!("omastudio: GPU preview stages failed, using the CPU: {e}");
            super::StageOutcome::Failed
        }
    }
}

/// The whole 8-bit preview on the GPU for a `w` x `h` frame: the point stage,
/// adjustment layers, presence, detail, defringe, grain, geometry, vignette and 8-bit
/// quantization, with the histogram and scopes counted there too. Only the 8-bit
/// pixels (`ch` per pixel; alpha comes from the input) and the counts come back. None
/// when there is no usable GPU or the job failed.
#[allow(clippy::too_many_arguments)]
pub fn run_display(
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
) -> Option<(Vec<u8>, crate::pipeline::histogram::HistogramData)> {
    match run_display_checked(input, w, h, ch, recipe, inputs, scale) {
        super::StageOutcome::Done(v) => Some(v),
        super::StageOutcome::Skipped | super::StageOutcome::Failed => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn display_on(
    gpu: &Gpu,
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
) -> Result<(Vec<u8>, crate::pipeline::histogram::HistogramData), String> {
    if w == 0 || h == 0 || !(3..=4).contains(&ch) || input.len() < w * h * ch {
        return Err("unsupported buffer".to_string());
    }
    let (lay, mut steps) = match layer_step(w, h, (0, 0, w, h), recipe)? {
        Some((lay, step)) => (lay, vec![step]),
        None => (Vec::new(), Vec::new()),
    };
    steps.extend(plan(w, h, recipe, scale));
    steps.extend(display_steps(w, h, ch, recipe, scale));
    let _job = gpu.jobs.lock().map_err(|_| "GPU lock poisoned")?;
    let (mut pixels, mut words) = (Vec::new(), Vec::new());
    execute(
        gpu,
        w,
        h,
        &steps,
        &lay,
        Out::Display { ch, pixels: &mut pixels, words: &mut words },
        Some(|rgb: &wgpu::Buffer| super::point_ops::run_into(gpu, input, w, ch, 0, h, recipe, inputs, rgb)),
    )?;
    if ch == 4 {
        for (px, src) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(input.as_chunks::<4>().0) {
            px[3] = (src[3] >> 8) as u8;
        }
    }
    let hist = crate::pipeline::histogram::histogram_from_scope_words(&words, w * h);
    Ok((pixels, hist))
}

/// Where [`execute`] puts its result.
enum Out<'a> {
    /// RGB f32, which is also uploaded when there is no `fill`.
    F32(&'a mut [f32]),
    /// The display tail's 8-bit pixels (`ch` bytes each, alpha left at 0) and the
    /// histogram and scope words.
    Display { ch: usize, pixels: &'a mut Vec<u8>, words: &'a mut Vec<u32> },
}

/// Fills the device RGB buffer with `fill` (or uploads the f32 output when there is
/// none), runs `steps` on it and reads the result back into `out`. The caller holds
/// the GPU's job lock.
fn execute(
    gpu: &Gpu,
    w: usize,
    h: usize,
    steps: &[Step],
    lay: &[f32],
    mut out: Out<'_>,
    fill: Option<impl FnOnce(&wgpu::Buffer) -> Result<(), String>>,
) -> Result<(), String> {
    let limit = gpu.limits.max_storage_buffer_binding_size.min(gpu.limits.max_buffer_size).min(MAX_BUFFER);
    let pixels_u64 = (w * h) as u64;
    let (rgb_size, plane_size) = (pixels_u64 * 12, pixels_u64 * 4);
    if rgb_size > limit || plane_size * PLANES > limit || plane_size * PLANES > u32::MAX as u64 {
        return Err(format!("{w}x{h} exceeds the GPU's buffer limits"));
    }
    let max_groups = gpu.limits.max_compute_workgroups_per_dimension;
    if steps.iter().any(|s| s.groups.0 > max_groups || s.groups.1 > max_groups) {
        return Err(format!("{w}x{h} needs more workgroups than the GPU allows"));
    }
    let layers_size = (lay.len() * 4) as u64;
    if layers_size > limit {
        return Err("the masks exceed the GPU's buffer limits".to_string());
    }
    let pipeline = pipeline(gpu)?;

    let stride = PARAMS_SIZE.next_multiple_of(gpu.limits.min_uniform_buffer_offset_alignment as u64);
    let mut params = vec![0u8; (stride * steps.len() as u64) as usize];
    for (i, s) in steps.iter().enumerate() {
        let at = i * stride as usize;
        params[at..at + PARAMS_SIZE as usize].copy_from_slice(as_bytes(&s.params));
    }
    let params_size = params.len() as u64;

    let device = &gpu.device;
    let interactive = matches!(out, Out::Display { .. }) || crate::pipeline::interactive();
    let timeout = gpu.poll_timeout(interactive);
    let mut cache = pipeline.buffers.lock().map_err(|_| "GPU buffer lock poisoned")?;
    if cache
        .as_ref()
        .is_some_and(|b| b.total_bytes() > super::PREVIEW_VRAM_CAP && b.last_used.elapsed() >= super::OVERSIZED_IDLE_EVICT)
    {
        *cache = None;
    }

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let body = (|| -> Result<(), String> {
        if !cache.as_ref().is_some_and(|b| b.compute.fits(params_size, rgb_size, plane_size, layers_size)) {
            // Grow to the largest request seen, so alternating sizes don't reallocate
            let (p, r, l, m) = cache.as_ref().map_or((0, 0, 0, 0), |b| {
                (b.compute.params_size, b.compute.rgb_size, b.compute.plane_size, b.compute.layers_size)
            });
            *cache = None;
            *cache = Some(Buffers::new(
                device,
                &pipeline.layout,
                params_size.max(p).max(stride * 64),
                rgb_size.max(r),
                plane_size.max(l),
                layers_size.max(m).max(256),
            ));
        }
        let bufs = cache.as_mut().expect("buffers");
        match &out {
            Out::F32(_) => {
                // Drop unused display buffers when switching modes would cross the VRAM cap.
                if bufs.display.is_some()
                    && bufs.total_bytes().saturating_add(bufs.compute.rgb_size) > super::PREVIEW_VRAM_CAP
                {
                    bufs.display = None;
                }
                let _ = bufs.compute.ensure_staging_f32(device);
            }
            Out::Display { .. } => {
                if bufs.compute.staging_f32.is_some()
                    && bufs.total_bytes().saturating_add(pixels_u64 * 11) > super::PREVIEW_VRAM_CAP
                {
                    bufs.compute.staging_f32 = None;
                }
                if !bufs.display.as_ref().is_some_and(|d| d.pixel_cap >= pixels_u64) {
                    let cap = bufs.display.as_ref().map_or(0, |d| d.pixel_cap).max(pixels_u64);
                    bufs.display = None;
                    bufs.display = Some(DisplayBuffers::new(device, &pipeline.layout, &bufs.compute, cap));
                }
            }
        }
        let bufs = cache.as_ref().expect("buffers");
        match fill {
            Some(f) => f(&bufs.compute.rgb)?,
            None => match &out {
                Out::F32(data) => gpu.queue.write_buffer(&bufs.compute.rgb, 0, as_bytes(&data[..w * h * 3])),
                Out::Display { .. } => return Err("the display tail needs a fill".to_string()),
            },
        }
        gpu.queue.write_buffer(&bufs.compute.params, 0, &params);
        if !lay.is_empty() {
            gpu.queue.write_buffer(&bufs.compute.layers, 0, as_bytes(lay));
        }

        // Bytes read back: RGB f32, or 8-bit pixels (whole words) plus the scope counts
        let (bind, read_buf, staging_buf, read_len, hist_pair) = match &out {
            Out::F32(_) => (
                &bufs.compute.bind_compute,
                &bufs.compute.rgb,
                bufs.compute.staging_f32.as_ref().expect("staging_f32"),
                rgb_size,
                None,
            ),
            Out::Display { ch, .. } => {
                let disp = bufs.display.as_ref().expect("display buffers");
                let (src, len) = if *ch == 3 {
                    (&disp.out8, ((w * h * 3).div_ceil(4) * 4) as u64)
                } else {
                    (&disp.px8, (w * h * 4) as u64)
                };
                let hist_len = (crate::pipeline::histogram::SCOPE_WORDS * 4) as u64;
                let terms: Vec<f32> = [0.2126f32, 0.7152, 0.0722].iter().flat_map(|k| (0..256).map(move |v| k * v as f32)).collect();
                gpu.queue.write_buffer(&disp.hist, hist_len, as_bytes(&terms));
                (&disp.bind_display, src, &disp.staging_display, len, Some((&disp.hist, &disp.staging_hist, hist_len)))
            }
        };

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("local") });
        if let Some((hist_buf, _, hist_len)) = hist_pair {
            enc.clear_buffer(hist_buf, 0, Some(hist_len));
        }
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("local"), timestamp_writes: None });
            for (i, s) in steps.iter().enumerate() {
                pass.set_pipeline(&pipeline.pipelines[s.entry]);
                pass.set_bind_group(0, bind, &[(i as u64 * stride) as u32]);
                pass.dispatch_workgroups(s.groups.0, s.groups.1, 1);
            }
        }
        enc.copy_buffer_to_buffer(read_buf, 0, staging_buf, 0, read_len);
        if let Some((hist_buf, staging_hist, hist_len)) = hist_pair {
            enc.copy_buffer_to_buffer(hist_buf, 0, staging_hist, 0, hist_len);
        }
        let idx = gpu.queue.submit([enc.finish()]);

        let map = |buf: &wgpu::Buffer, len: u64| {
            let (tx, rx) = mpsc::channel();
            buf.map_async(wgpu::MapMode::Read, 0..len, move |r| {
                let _ = tx.send(r);
            });
            rx
        };
        let rx = map(staging_buf, read_len);
        let mut staging_guard = super::StagingGuard::new(staging_buf);
        let mut hist_state = hist_pair.map(|(_, staging_hist, hist_len)| {
            let rx_hist = map(staging_hist, hist_len);
            (staging_hist, hist_len, rx_hist, super::StagingGuard::new(staging_hist))
        });

        gpu.wait_submission(Some(idx), timeout)?;

        let main_map = rx.recv().map_err(|e| e.to_string())?;
        let hist_map = match hist_state.as_ref() {
            Some((_, _, rx_hist, _)) => Some(rx_hist.recv().map_err(|e| e.to_string())?),
            None => None,
        };
        if main_map.is_err() {
            staging_guard.disarm();
        }
        if hist_map.as_ref().is_some_and(Result::is_err)
            && let Some((_, _, _, ref mut hg)) = hist_state
        {
            hg.disarm();
        }
        main_map.map_err(|e| e.to_string())?;
        if let Some(res) = hist_map {
            res.map_err(|e| e.to_string())?;
        }

        // Scope mapped views strictly inside this block so they drop before unmap().
        {
            let view = staging_buf.get_mapped_range(0..read_len).map_err(|e| e.to_string())?;
            match &mut out {
                Out::F32(data) => super::point_ops::as_bytes_mut(&mut data[..w * h * 3]).copy_from_slice(&view),
                Out::Display { ch, pixels, words } => {
                    pixels.clear();
                    pixels.extend_from_slice(&view[..w * h * *ch]);
                    let (staging_hist, hist_len, _, _) = hist_state.as_ref().expect("hist state");
                    let hist = staging_hist.get_mapped_range(0..*hist_len).map_err(|e| e.to_string())?;
                    words.clear();
                    words.extend(hist.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)));
                }
            }
        }
        staging_guard.unmap_now();
        if let Some((_, _, _, hg)) = hist_state {
            hg.unmap_now();
        }
        Ok(())
    })();

    let scope_res = super::drain_scopes(oom, scope);
    if scope_res.is_err() || body.is_err() {
        *cache = None;
        scope_res?;
        return body;
    }
    if cache.as_ref().is_some_and(|b| b.total_bytes() > super::PREVIEW_VRAM_CAP) {
        *cache = None;
    } else if let Some(b) = cache.as_mut() {
        b.last_used = std::time::Instant::now();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{detail, presence};

    fn frame(w: usize, h: usize) -> Vec<f32> {
        // Gradients, hashed noise and a few hard edges, mostly in 0..1 with some
        // values above white like a highlight push
        (0..w * h * 3)
            .map(|i| {
                let (px, c) = (i / 3, i % 3);
                let (x, y) = (px % w, px / w);
                let n = ((x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663) ^ (c as u32 * 83_492_791)) % 1000;
                let base = (x as f32 / w as f32) * 0.6 + (y as f32 / h as f32) * 0.3 + c as f32 * 0.05;
                let edge = if (x / 23 + y / 17) % 2 == 0 { 0.25 } else { 0.0 };
                base + edge + n as f32 / 1000.0 * 0.08
            })
            .collect()
    }

    fn recipes() -> Vec<(&'static str, Recipe, f32)> {
        let base = Recipe { sharpness: 0.0, denoise_col: 0.0, denoise_lum: 0.0, ..Recipe::default() };
        let mut out = Vec::new();
        let mut r = base.clone();
        r.texture = 40.0;
        out.push(("texture", r.clone(), 1.0));
        r = base.clone();
        r.clarity = -35.0;
        out.push(("clarity", r.clone(), 1.0));
        r = base.clone();
        r.dehaze = 30.0;
        out.push(("dehaze", r.clone(), 1.0));
        r.dehaze = -30.0;
        out.push(("haze", r.clone(), 1.0));
        r = base.clone();
        r.midtone_detail = 50.0;
        out.push(("midtone_detail", r.clone(), 1.0));
        r = base.clone();
        r.sharpness = 60.0;
        out.push(("sharpness", r.clone(), 1.0));
        r = base.clone();
        r.denoise_lum = 45.0;
        r.denoise_col = 55.0;
        out.push(("denoise", r.clone(), 1.0));
        r = base.clone();
        r.sharpness = 0.3;
        out.push(("detail_round_trip", r.clone(), 1.0));
        r = base.clone();
        r.defringe = 70.0;
        out.push(("defringe", r.clone(), 1.0));
        let all = Recipe {
            clarity: 25.0,
            texture: 20.0,
            dehaze: 10.0,
            midtone_detail: -20.0,
            sharpness: 40.0,
            denoise_lum: 30.0,
            denoise_col: 30.0,
            defringe: 40.0,
            ..Recipe::default()
        };
        out.push(("everything", all.clone(), 1.0));
        // Neighbourhoods scaled for a larger frame, radii clamped by the height
        out.push(("everything_scaled", all, 2.5));
        out
    }

    /// Runs only where a GPU (or, with OMASTUDIO_GPU=software, lavapipe) is available.
    #[test]
    fn gpu_matches_cpu() {
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (203, 91);
        let input = frame(w, h);
        for (name, recipe, scale) in recipes() {
            let mut cpu = input.clone();
            presence::apply_presence_scaled(&mut cpu, w, h, recipe.clarity, recipe.texture, recipe.dehaze, recipe.midtone_detail, scale);
            detail::apply_detail_scaled(&mut cpu, w, h, recipe.sharpness, recipe.denoise_lum, recipe.denoise_col, scale);
            detail::apply_defringe_buffer(&mut cpu, w, h, recipe.defringe, scale);
            let mut got = input.clone();
            run_on(&gpu, &mut got, w, h, &recipe, scale).expect("GPU local stage");
            let (mut off, mut worst) = (0usize, 0.0f32);
            for (a, b) in cpu.iter().zip(&got) {
                let d = (a - b).abs();
                if d.is_nan() || d > 1e-3 {
                    off += 1;
                }
                worst = if d.is_finite() { worst.max(d) } else { f32::INFINITY };
            }
            eprintln!("{name}: {off} values differ by more than 1e-3 (worst {worst})");
            // A value on the noise gate's or defringe's threshold may land either side
            assert!(off * 1000 < w * h * 3, "{name}: {off} values differ (worst {worst})");
            assert!(worst < 0.02, "{name}: worst difference {worst}");
        }
    }

    /// The chained point + presence/detail run against the CPU doing both.
    #[test]
    fn chained_matches_cpu() {
        use crate::pipeline::transfer::Transfer;
        use crate::pipeline::{PointOps, white_balance};
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h, ch) = (157usize, 83usize, 3usize);
        let input: Vec<u16> = (0..w * h * ch)
            .map(|i| {
                let (px, c) = (i / ch, i % ch);
                let (x, y) = (px % w, px / w);
                let n = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503) ^ c as u32) % 3000;
                ((x * 30_000 / w + y * 20_000 / h) as u32 + n + c as u32 * 1500) as u16
            })
            .collect();
        let recipe = Recipe {
            exposure: 0.3,
            contrast: 20.0,
            shadows: 25.0,
            clarity: 25.0,
            texture: 20.0,
            sharpness: 40.0,
            denoise_col: 30.0,
            defringe: 30.0,
            film_simulation: "fuji_classic_chrome".into(),
            layers: test_layers(),
            ..Recipe::default()
        };
        let transfer = Transfer::bt709();
        let ops = PointOps::new(&recipe);
        let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
        let e = 2.0f32.powf(recipe.exposure + recipe.baseline_exposure);
        let k = [wr * e, wg * e, wb * e];
        // Rows 5..h of the input, as an export band would ask for
        let (y0, y1) = (5, h);
        let rows = y1 - y0;
        let mut cpu = vec![0.0f32; rows * w * 3];
        for i in 0..rows * w {
            let px = &input[(y0 * w + i) * ch..(y0 * w + i) * ch + 3];
            let enc = |c: usize| transfer.encode(px[c] as f32 / 65535.0 * k[c]);
            let (r, g, b) = ops.apply(enc(0), enc(1), enc(2));
            // The band's rows sit at y0.. of the frame
            let (u, v) = (((i % w) as f32 + 0.5) / w as f32, ((y0 + i / w) as f32 + 0.5) / h as f32);
            let (r, g, b) = crate::pipeline::layers::apply_layers_pixel(r, g, b, &recipe.layers, u, v);
            cpu[i * 3..i * 3 + 3].copy_from_slice(&[r, g, b]);
        }
        presence::apply_presence_scaled(&mut cpu, w, rows, recipe.clarity, recipe.texture, recipe.dehaze, recipe.midtone_detail, 1.0);
        detail::apply_detail_scaled(&mut cpu, w, rows, recipe.sharpness, recipe.denoise_lum, recipe.denoise_col, 1.0);
        detail::apply_defringe_buffer(&mut cpu, w, rows, recipe.defringe, 1.0);

        let got = chained_on(&gpu, &input, w, ch, y0, y1, &recipe, &ops.gpu_inputs(k, transfer), 1.0, (0, y0, w, h))
            .expect("GPU chained stages")
            .expect("work to do");
        let (mut off, mut worst) = (0usize, 0.0f32);
        for (a, b) in cpu.iter().zip(&got) {
            let d = (a - b).abs();
            if d.is_nan() || d > 2e-3 {
                off += 1;
            }
            worst = if d.is_finite() { worst.max(d) } else { f32::INFINITY };
        }
        eprintln!("chained: {off} values differ by more than 2e-3 (worst {worst})");
        assert!(off * 1000 < rows * w * 3, "{off} values differ (worst {worst})");
        assert!(worst < 0.05, "worst difference {worst}");
    }

    /// The GPU preview against process_buffer_16_to_8_ex on the CPU (the frame is below
    /// MIN_PIXELS, so the pipeline itself stays on the CPU).
    #[test]
    fn display_matches_cpu() {
        use crate::pipeline::transfer::Transfer;
        use crate::pipeline::{PointOps, white_balance};
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (163usize, 91usize);
        let grain = Recipe {
            exposure: 0.2,
            clarity: 20.0,
            sharpness: 30.0,
            grain_amount: 35.0,
            grain_size: 1.6,
            grain_roughness: 60.0,
            vignette: -30.0,
            ..Recipe::default()
        };
        for ch in [3usize, 4] {
            let input: Vec<u16> = (0..w * h * ch)
                .map(|i| {
                    let (px, c) = (i / ch, i % ch);
                    let (x, y) = (px % w, px / w);
                    if c == 3 {
                        return (x * 400) as u16;
                    }
                    let n = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503) ^ c as u32) % 4000;
                    ((x * 40_000 / w + y * 20_000 / h) as u32 + n + c as u32 * 1500) as u16
                })
                .collect();
            let geometry = Recipe { lens_distortion: 30.0, perspective_vertical: 25.0, perspective_horizontal: -10.0, ..grain.clone() };
            let barrel = Recipe { lens_distortion: -40.0, perspective_scale: 90.0, ..Recipe::default() };
            let masks = Recipe { layers: test_layers(), ..grain.clone() };
            for (name, recipe) in [("default", Recipe::default()), ("grain_vignette", grain.clone()), ("geometry", geometry), ("barrel", barrel), ("masks", masks)] {
                let transfer = Transfer::bt709();
                let (cpu, cpu_hist) = crate::pipeline::process_buffer_16_to_8_ex(&input, w as u32, h as u32, ch as u32, &recipe, transfer, false, false);
                let ops = PointOps::new(&recipe);
                let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
                let e = 2.0f32.powf(recipe.exposure + recipe.baseline_exposure);
                let scale = crate::pipeline::frame_scale(w, h);
                let (got, hist) = display_on(&gpu, &input, w, h, ch, &recipe, &ops.gpu_inputs([wr * e, wg * e, wb * e], transfer), scale).expect("GPU preview");
                assert_eq!(got.len(), cpu.len());
                let mut off_px = std::collections::HashSet::new();
                let mut worst = 0u8;
                for (i, (a, b)) in cpu.iter().zip(&got).enumerate() {
                    let d = a.abs_diff(*b);
                    worst = worst.max(d);
                    if d > 0 {
                        off_px.insert(i / ch);
                    }
                }
                // A value on a quantization step may round the other way
                eprintln!("{name} ch={ch}: {} pixels differ (worst {worst})", off_px.len());
                assert!(worst <= 1, "{name} ch={ch}: worst difference {worst}");
                assert!(off_px.len() * 200 < w * h, "{name} ch={ch}: {} pixels differ", off_px.len());
                // Each differing pixel moves at most one count per histogram
                let n = off_px.len() as i64;
                for (a, b) in [(&cpu_hist.red, &hist.red), (&cpu_hist.green, &hist.green), (&cpu_hist.blue, &hist.blue), (&cpu_hist.luma, &hist.luma)] {
                    let d: i64 = a.iter().zip(b.iter()).map(|(x, y)| (*x as i64 - *y as i64).abs()).sum();
                    assert!(d <= 2 * n, "{name} ch={ch}: histogram differs by {d}");
                }
                assert!((cpu_hist.highlight_clipping_percent - hist.highlight_clipping_percent).abs() * (w * h) as f32 <= 100.0 * n as f32 + 1e-3);
                if n == 0 {
                    assert_eq!(cpu_hist.waveform_luma, hist.waveform_luma, "{name} ch={ch}: waveform");
                    assert_eq!(cpu_hist.parade_r, hist.parade_r, "{name} ch={ch}: parade");
                }
                // The vectorscope's chroma sums may be fused into multiply-adds on the
                // GPU, so a pixel on a cell boundary can land in the next cell
                let moved = cpu_hist.vectorscope.iter().zip(&hist.vectorscope).filter(|(a, b)| a.abs_diff(**b) > 8).count();
                eprintln!("{name} ch={ch}: {moved} vectorscope cells differ");
                assert!(moved * 20 <= cpu_hist.vectorscope.iter().filter(|&&c| c > 0).count().max(20), "{name} ch={ch}: {moved} vectorscope cells differ");
            }
        }
    }

    /// Runs only where a GPU (or, with OMASTUDIO_GPU=software, lavapipe) is available.
    #[test]
    fn buffer_pool_splits_compute_and_display_and_recovers_after_error() {
        use crate::pipeline::transfer::Transfer;
        use crate::pipeline::{PointOps, white_balance};
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let p = pipeline(&gpu).expect("local pipeline");
        let (w, h) = (64usize, 48usize);
        let recipe = Recipe { clarity: 20.0, sharpness: 25.0, ..Recipe::default() };

        // 1. F32 compute-only path must not allocate DisplayBuffers.
        {
            let _job = gpu.jobs.lock().unwrap();
            *p.buffers.lock().unwrap() = None;
        }
        let mut buf = frame(w, h);
        run_on(&gpu, &mut buf, w, h, &recipe, 1.0).expect("F32 local stage");
        {
            let _job = gpu.jobs.lock().unwrap();
            let cache = p.buffers.lock().unwrap();
            let bufs = cache.as_ref().expect("F32 buffers cached");
            assert!(bufs.display.is_none(), "F32 path should not allocate DisplayBuffers");
            assert!(bufs.compute.staging_f32.is_some(), "F32 path should allocate staging_f32");
        }

        // 2. Display path must allocate 4 B/px staging_display without staging_f32.
        {
            let _job = gpu.jobs.lock().unwrap();
            *p.buffers.lock().unwrap() = None;
        }
        let input = vec![32768u16; w * h * 3];
        let ops = PointOps::new(&recipe);
        let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
        let transfer = Transfer::bt709();
        let _ = display_on(&gpu, &input, w, h, 3, &recipe, &ops.gpu_inputs([wr, wg, wb], transfer), 1.0)
            .expect("Display local stage");
        {
            let _job = gpu.jobs.lock().unwrap();
            let cache = p.buffers.lock().unwrap();
            let bufs = cache.as_ref().expect("Display buffers cached");
            assert!(bufs.compute.staging_f32.is_none(), "Display path should not allocate staging_f32");
            let disp = bufs.display.as_ref().expect("DisplayBuffers allocated");
            assert_eq!(disp.staging_display.size(), (w * h * 4) as u64, "Display staging must be 4 B/px, not 12 B/px");
        }

        // 3. Any error during execute() must clear the buffer cache cleanly so subsequent jobs succeed.
        {
            let _job = gpu.jobs.lock().unwrap();
            let steps = plan(w, h, &recipe, 1.0);
            let mut out = vec![0.0f32; w * h * 3];
            let err = execute(
                &gpu,
                w,
                h,
                &steps,
                &[],
                Out::F32(&mut out),
                Some(|_: &wgpu::Buffer| Err("injected fill error".to_string())),
            );
            assert!(err.is_err());
            assert!(p.buffers.lock().unwrap().is_none(), "Failed execute() must invalidate buffer cache");
        }
        let mut retry = frame(w, h);
        run_on(&gpu, &mut retry, w, h, &recipe, 1.0).expect("Subsequent local job must recover after error");
    }

    /// One layer of each kind of mask, with components and every adjustment.
    fn test_layers() -> Vec<crate::pipeline::layers::AdjustmentLayer> {
        use crate::pipeline::layers::{AdjustmentLayer, BrushStroke, MaskComponent, MaskMode, MaskShape};
        vec![
            AdjustmentLayer {
                exposure: -0.8,
                highlights: -40.0,
                saturation: 20.0,
                shape: MaskShape::LinearGradient { start_x: 0.5, start_y: 0.0, end_x: 0.5, end_y: 0.6 },
                components: vec![MaskComponent {
                    mode: MaskMode::Subtract,
                    invert: false,
                    shape: MaskShape::LumaRange { min_luma: 0.0, max_luma: 0.3, falloff: 0.1 },
                }],
                ..AdjustmentLayer::default()
            },
            AdjustmentLayer {
                invert: true,
                opacity: 0.7,
                exposure: 0.5,
                shadows: 30.0,
                blacks: -20.0,
                contrast: 25.0,
                shape: MaskShape::RadialGradient { center_x: 0.4, center_y: 0.5, radius_x: 0.3, radius_y: 0.25, feather: 0.6 },
                ..AdjustmentLayer::default()
            },
            AdjustmentLayer {
                whites: 35.0,
                tint_r: 0.2,
                tint_b: -0.1,
                shape: MaskShape::Brush {
                    strokes: vec![
                        BrushStroke { points: vec![[0.1, 0.2], [0.6, 0.7]], size: 0.08, ..BrushStroke::default() },
                        BrushStroke { points: vec![[0.3, 0.3]], size: 0.05, erase: true, ..BrushStroke::default() },
                    ],
                    aspect: 0.56,
                    cache: Default::default(),
                },
                components: vec![MaskComponent {
                    mode: MaskMode::Intersect,
                    invert: true,
                    shape: MaskShape::ColorRange { r: 0.5, g: 0.4, b: 0.3, range: 0.2, feather: 0.5 },
                }],
                ..AdjustmentLayer::default()
            },
            AdjustmentLayer { enabled: false, exposure: 3.0, ..AdjustmentLayer::default() },
        ]
    }

    #[test]
    fn neutral_recipe_has_no_steps() {
        let r = Recipe { sharpness: 0.0, denoise_col: 0.0, denoise_lum: 0.0, defringe: 0.0, ..Recipe::default() };
        assert!(plan(400, 300, &r, 1.0).is_empty());
    }
}
