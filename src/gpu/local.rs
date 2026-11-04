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
const PARAMS_SIZE: u64 = 48;

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
];

fn entry(name: &str) -> usize {
    ENTRIES.iter().position(|e| *e == name).unwrap_or_else(|| panic!("unknown entry point {name}"))
}

/// One dispatch: its kernel, parameters and workgroup counts.
struct Step {
    entry: usize,
    params: [u32; 12],
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
        params: [wu, hu, src, dst, r, mode, flags, 0, f[0].to_bits(), f[1].to_bits(), f[2].to_bits(), f[3].to_bits()],
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

pub struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipelines: Vec<wgpu::ComputePipeline>,
    /// Kept between calls; previews call this on every edit.
    buffers: std::sync::Mutex<Option<Buffers>>,
}

struct Buffers {
    rgb_size: u64,
    plane_size: u64,
    params_size: u64,
    params: wgpu::Buffer,
    rgb: wgpu::Buffer,
    staging: wgpu::Buffer,
    bind: wgpu::BindGroup,
    _planes: wgpu::Buffer,
}

impl Buffers {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, params_size: u64, rgb_size: u64, plane_size: u64) -> Self {
        let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(4).next_multiple_of(4), usage, mapped_at_creation: false })
        };
        let params = buffer("local params", params_size, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let rgb = buffer("local rgb", rgb_size, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC);
        let planes = buffer("local planes", plane_size * PLANES, wgpu::BufferUsages::STORAGE);
        let staging = buffer("local staging", rgb_size, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("local"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &params, offset: 0, size: wgpu::BufferSize::new(PARAMS_SIZE) }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: rgb.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: planes.as_entire_binding() },
            ],
        });
        Self { rgb_size, plane_size, params_size, params, rgb, staging, bind, _planes: planes }
    }

    fn fits(&self, params: u64, rgb: u64, plane: u64) -> bool {
        self.params_size >= params && self.rgb_size >= rgb && self.plane_size >= plane
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
    if let Some(gpu) = super::get().filter(|g| super::stages_worthwhile(g)) {
        let _ = pipeline(gpu);
    }
}

fn pipeline(gpu: &Gpu) -> Result<&Pipeline, String> {
    gpu.local
        .get_or_init(|| {
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

/// Runs presence, detail and defringe on `buf` (`w` x `h` interleaved RGB f32) in
/// place, with neighbourhoods sized for a frame `scale` times the reference. Returns
/// false when there is no usable GPU or the job failed; `buf` is then unchanged.
pub fn run(buf: &mut [f32], w: usize, h: usize, recipe: &Recipe, scale: f32) -> bool {
    if w * h < super::point_ops::MIN_PIXELS {
        return false;
    }
    let Some(gpu) = super::get().filter(|g| super::stages_worthwhile(g)) else {
        return false;
    };
    match run_on(gpu, buf, w, h, recipe, scale) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("omastudio: GPU presence/detail stage failed, using the CPU: {e}");
            false
        }
    }
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
    execute(gpu, w, h, &steps, &mut buf[..w * h * 3], None::<fn(&wgpu::Buffer) -> Result<(), String>>)
}

/// Point stage and this stage in one go for rows `y0..y1` of `input` (`w` pixels of
/// `ch` 16-bit channels per row): the point stage's result stays on the GPU and only
/// the final RGB f32 comes back. None when there is no usable GPU, nothing for this
/// stage to do (the caller then runs the point stage alone) or the job failed.
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
) -> Option<Vec<f32>> {
    if (y1 - y0) * w < super::point_ops::MIN_PIXELS {
        return None;
    }
    let gpu = super::get().filter(|g| super::stages_worthwhile(g))?;
    match chained_on(gpu, input, w, ch, y0, y1, recipe, inputs, scale) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("omastudio: GPU point + presence/detail stages failed, using the CPU: {e}");
            None
        }
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
) -> Result<Option<Vec<f32>>, String> {
    let h = y1.saturating_sub(y0);
    if w == 0 || h == 0 || !(3..=4).contains(&ch) || input.len() < y1 * w * ch {
        return Err("unsupported buffer".to_string());
    }
    let steps = plan(w, h, recipe, scale);
    if steps.is_empty() {
        return Ok(None);
    }
    let _job = gpu.jobs.lock().map_err(|_| "GPU lock poisoned")?;
    let mut out = vec![0.0f32; w * h * 3];
    execute(gpu, w, h, &steps, &mut out, Some(|rgb: &wgpu::Buffer| super::point_ops::run_into(gpu, input, w, ch, y0, y1, recipe, inputs, rgb)))?;
    Ok(Some(out))
}

/// Fills the device RGB buffer with `fill` (or uploads `data` when there is none),
/// runs `steps` on it and reads the result back into `data`. The caller holds the
/// GPU's job lock.
fn execute(
    gpu: &Gpu,
    w: usize,
    h: usize,
    steps: &[Step],
    data: &mut [f32],
    fill: Option<impl FnOnce(&wgpu::Buffer) -> Result<(), String>>,
) -> Result<(), String> {
    let limit = gpu.limits.max_storage_buffer_binding_size.min(gpu.limits.max_buffer_size).min(MAX_BUFFER);
    let (rgb_size, plane_size) = ((w * h * 12) as u64, (w * h * 4) as u64);
    if rgb_size > limit || plane_size * PLANES > limit || plane_size * PLANES > u32::MAX as u64 {
        return Err(format!("{w}x{h} exceeds the GPU's buffer limits"));
    }
    let max_groups = gpu.limits.max_compute_workgroups_per_dimension;
    if steps.iter().any(|s| s.groups.0 > max_groups || s.groups.1 > max_groups) {
        return Err(format!("{w}x{h} needs more workgroups than the GPU allows"));
    }
    let pipeline = pipeline(gpu)?;

    let stride = PARAMS_SIZE.next_multiple_of(gpu.limits.min_uniform_buffer_offset_alignment as u64);
    let mut params = vec![0u8; (stride * steps.len() as u64) as usize];
    for (i, s) in steps.iter().enumerate() {
        let at = i * stride as usize;
        params[at..at + PARAMS_SIZE as usize].copy_from_slice(as_bytes(&s.params));
    }

    let device = &gpu.device;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let mut cache = pipeline.buffers.lock().map_err(|_| "GPU buffer lock poisoned")?;
    let params_size = params.len() as u64;
    if !cache.as_ref().is_some_and(|b| b.fits(params_size, rgb_size, plane_size)) {
        // Grow to the largest request seen, so alternating sizes don't reallocate
        let (p, r, l) = cache.as_ref().map_or((0, 0, 0), |b| (b.params_size, b.rgb_size, b.plane_size));
        *cache = None;
        *cache = Some(Buffers::new(device, &pipeline.layout, params_size.max(p).max(stride * 64), rgb_size.max(r), plane_size.max(l)));
    }
    let bufs = cache.as_ref().expect("buffers");
    let filled = match fill {
        Some(f) => f(&bufs.rgb),
        None => {
            gpu.queue.write_buffer(&bufs.rgb, 0, as_bytes(&data[..w * h * 3]));
            Ok(())
        }
    };
    gpu.queue.write_buffer(&bufs.params, 0, &params);

    let mut idx = None;
    if filled.is_ok() {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("local") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("local"), timestamp_writes: None });
            for (i, s) in steps.iter().enumerate() {
                pass.set_pipeline(&pipeline.pipelines[s.entry]);
                pass.set_bind_group(0, &bufs.bind, &[(i as u64 * stride) as u32]);
                pass.dispatch_workgroups(s.groups.0, s.groups.1, 1);
            }
        }
        enc.copy_buffer_to_buffer(&bufs.rgb, 0, &bufs.staging, 0, rgb_size);
        idx = Some(gpu.queue.submit([enc.finish()]));
    }
    let read = idx.map(|idx| {
        let (tx, rx) = mpsc::channel();
        bufs.staging.map_async(wgpu::MapMode::Read, 0..rgb_size, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None }).map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())
    });
    let oom_err = pollster::block_on(oom.pop());
    let val_err = pollster::block_on(scope.pop());
    if let Some(e) = oom_err {
        *cache = None;
        return Err(format!("out of GPU memory: {e}"));
    }
    if let Some(e) = val_err {
        *cache = None;
        return Err(e.to_string());
    }
    filled?;
    read.expect("submitted")?;
    {
        let view = bufs.staging.get_mapped_range(0..rgb_size).map_err(|e| e.to_string())?;
        super::point_ops::as_bytes_mut(&mut data[..w * h * 3]).copy_from_slice(&view);
    }
    bufs.staging.unmap();
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
        let Some(gpu) = super::super::get() else {
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
            run_on(gpu, &mut got, w, h, &recipe, scale).expect("GPU local stage");
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
        let Some(gpu) = super::super::get() else {
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
            cpu[i * 3..i * 3 + 3].copy_from_slice(&[r, g, b]);
        }
        presence::apply_presence_scaled(&mut cpu, w, rows, recipe.clarity, recipe.texture, recipe.dehaze, recipe.midtone_detail, 1.0);
        detail::apply_detail_scaled(&mut cpu, w, rows, recipe.sharpness, recipe.denoise_lum, recipe.denoise_col, 1.0);
        detail::apply_defringe_buffer(&mut cpu, w, rows, recipe.defringe, 1.0);

        let got = chained_on(gpu, &input, w, ch, y0, y1, &recipe, &ops.gpu_inputs(k, transfer), 1.0)
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

    #[test]
    fn neutral_recipe_has_no_steps() {
        let r = Recipe { sharpness: 0.0, denoise_col: 0.0, denoise_lum: 0.0, defringe: 0.0, ..Recipe::default() };
        assert!(plan(400, 300, &r, 1.0).is_empty());
    }
}
