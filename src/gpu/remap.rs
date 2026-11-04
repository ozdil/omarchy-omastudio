//! The export's last pass on the GPU: lens distortion and perspective remap of the
//! whole developed frame, vignette and 16-bit quantization (`remap.wgsl`). It mirrors
//! the lens branch of `pipeline::process_16_to_16_banded`. The frame goes up and comes
//! back in bands of output rows, each with the span of source rows it reads, so a
//! 100 MP frame fits the GPU's buffer limits.

use std::sync::mpsc;

use super::{Gpu, as_bytes};
use crate::pipeline::detail::Geometry;

/// Bytes of output per band.
const BAND_BYTES: usize = 128 << 20;
/// Rows added around the sampled source span.
const SPAN_MARGIN: usize = 4;

pub struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl Pipeline {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("remap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("remap.wgsl").into()),
        });
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("remap"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                storage(1, true),
                storage(2, false),
            ],
        });
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("remap"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("remap"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("remap"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { layout, pipeline }
    }
}

fn pipeline(gpu: &Gpu) -> Result<&Pipeline, String> {
    gpu.remap
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

/// Remaps `pre` (the `w` x `h` frame as u16 RGB scaled by 32767.5) through `geometry`
/// and applies the vignette, returning the 16-bit RGB result. None when there is no
/// usable GPU or the job failed; the caller then uses the CPU.
pub fn run(pre: &[u16], w: usize, h: usize, geometry: &Geometry, vignette: f32) -> Option<Vec<u16>> {
    if w * h < super::point_ops::MIN_PIXELS {
        return None;
    }
    let gpu = super::get().filter(|g| super::stages_worthwhile(g))?;
    let band_rows = (BAND_BYTES / (w * 6)).max(2);
    match run_on(gpu, pre, w, h, geometry, vignette, band_rows) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("omastudio: GPU export remap failed, using the CPU: {e}");
            None
        }
    }
}

/// The source rows the output rows `oy0..oy1` read, from a grid of samples.
fn source_span(w: usize, h: usize, oy0: usize, oy1: usize, geometry: &Geometry) -> Option<(usize, usize)> {
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    let (nx, ny) = (64usize, 16usize);
    for j in 0..=ny {
        let y = oy0 as f32 + (oy1 - 1 - oy0) as f32 * j as f32 / ny as f32;
        for i in 0..=nx {
            let x = (w - 1) as f32 * i as f32 / nx as f32;
            let (sx, sy) = geometry.source(x, y);
            if sx >= 0.0 && sx < (w - 1) as f32 && sy >= 0.0 && sy < (h - 1) as f32 {
                lo = lo.min(sy);
                hi = hi.max(sy);
            }
        }
    }
    if !lo.is_finite() {
        return None;
    }
    let s0 = (lo.floor() as usize).saturating_sub(SPAN_MARGIN);
    let s1 = (hi.floor() as usize + 2 + SPAN_MARGIN).min(h);
    Some((s0, s1))
}

pub(crate) fn run_on(gpu: &Gpu, pre: &[u16], w: usize, h: usize, geometry: &Geometry, vignette: f32, band_rows: usize) -> Result<Vec<u16>, String> {
    if w < 2 || h < 2 || pre.len() < w * h * 3 {
        return Err("unsupported buffer".to_string());
    }
    let band_rows = band_rows.max(2) & !1;
    let limit = gpu.limits.max_storage_buffer_binding_size.min(gpu.limits.max_buffer_size).min(1 << 30);
    let pipeline = pipeline(gpu)?;
    let device = &gpu.device;
    let _job = gpu.jobs.lock().map_err(|_| "GPU lock poisoned")?;
    let (terms, perspective, lens) = geometry.gpu_terms();
    let flags = perspective as u32 | (lens as u32) << 1 | ((vignette.abs() >= 0.01) as u32) << 2;

    // Each band's source span, so the buffers are sized once for the largest
    let mut bands = Vec::new();
    for oy0 in (0..h).step_by(band_rows) {
        let oy1 = (oy0 + band_rows).min(h);
        // A band that reads nothing still needs a source row to bind
        let span = source_span(w, h, oy0, oy1, geometry).unwrap_or((0, 1));
        bands.push((oy0, oy1, span));
    }
    let max_src = bands.iter().map(|(_, _, (s0, s1))| (s1 - s0) * w * 6).max().unwrap_or(0).next_multiple_of(4) as u64;
    let max_dst = ((band_rows * w * 3).div_ceil(2) * 4) as u64;
    if max_src > limit || max_dst > limit {
        return Err(format!("{w}x{h} exceeds the GPU's buffer limits"));
    }

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(4), usage, mapped_at_creation: false })
    };
    let params = buffer("remap params", 128, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
    let src = buffer("remap src", max_src, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
    let dst = buffer("remap dst", max_dst, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
    let staging = buffer("remap staging", max_dst, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("remap"),
        layout: &pipeline.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: dst.as_entire_binding() },
        ],
    });

    let mut out = vec![0u16; w * h * 3];
    let max_groups = gpu.limits.max_compute_workgroups_per_dimension;
    let mut result = Ok(());
    for &(oy0, oy1, (s0, s1)) in &bands {
        let rows = oy1 - oy0;
        let words = (rows * w * 3).div_ceil(2);
        let groups = (words as u32).div_ceil(256);
        let gx = groups.min(max_groups);
        let gy = groups.div_ceil(gx);
        let mut p = [0u32; 32];
        p[..8].copy_from_slice(&[w as u32, h as u32, oy0 as u32, rows as u32, s0 as u32, (s1 - s0) as u32, flags, gx]);
        p[8] = vignette.to_bits();
        for (k, v) in terms.iter().enumerate() {
            p[16 + k] = v.to_bits();
        }
        gpu.queue.write_buffer(&params, 0, as_bytes(&p));
        // Rows s0..s1 as whole words (an odd count of u16 values gets a zero after it)
        let slice = &pre[s0 * w * 3..s1 * w * 3];
        if slice.len() % 2 == 1 {
            let mut padded = slice.to_vec();
            padded.push(0);
            gpu.queue.write_buffer(&src, 0, as_bytes(&padded));
        } else {
            gpu.queue.write_buffer(&src, 0, as_bytes(slice));
        }
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("remap") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("remap"), timestamp_writes: None });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        let len = (words * 4) as u64;
        enc.copy_buffer_to_buffer(&dst, 0, &staging, 0, len);
        let idx = gpu.queue.submit([enc.finish()]);
        let (tx, rx) = mpsc::channel();
        staging.slice(0..len).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let read = device
            .poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
            .map_err(|e| e.to_string())
            .and_then(|_| rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string()));
        if let Err(e) = read {
            result = Err(e);
            break;
        }
        let copied = staging.get_mapped_range(0..len).map(|view| {
            let target = &mut out[oy0 * w * 3..oy1 * w * 3];
            for (o, b) in target.iter_mut().zip(view.as_chunks::<2>().0) {
                *o = u16::from_le_bytes(*b);
            }
        });
        staging.unmap();
        if let Err(e) = copied {
            result = Err(e.to_string());
            break;
        }
    }
    if let Some(e) = pollster::block_on(oom.pop()) {
        return Err(format!("out of GPU memory: {e}"));
    }
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(e.to_string());
    }
    result.map(|()| out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::Recipe;

    /// The GPU remap against the CPU's, over several bands.
    #[test]
    fn remap_matches_cpu() {
        let Some(gpu) = super::super::get() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (211usize, 157usize);
        let pre: Vec<u16> = (0..w * h * 3)
            .map(|i| {
                let (px, c) = (i / 3, i % 3);
                let (x, y) = (px % w, px / w);
                let n = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503) ^ c as u32) % 9000;
                ((x * 30_000 / w + y * 20_000 / h) as u32 + n + c as u32 * 2000) as u16
            })
            .collect();
        let cases = [
            Recipe { lens_distortion: 30.0, perspective_vertical: 25.0, perspective_horizontal: -10.0, vignette: -30.0, ..Recipe::default() },
            Recipe { lens_distortion: -40.0, perspective_scale: 90.0, ..Recipe::default() },
            Recipe { perspective_horizontal: 60.0, vignette: 20.0, ..Recipe::default() },
        ];
        for recipe in cases {
            let g = Geometry::new(w, h, &recipe).expect("geometry");
            let cpu = crate::pipeline::remap_16_cpu(&pre, w, h, &g, &recipe);
            let got = run_on(gpu, &pre, w, h, &g, recipe.vignette, 24).expect("GPU remap");
            let (mut off, mut worst) = (0usize, 0u16);
            for (a, b) in cpu.iter().zip(&got) {
                let d = a.abs_diff(*b);
                worst = worst.max(d);
                off += (d > 1) as usize;
            }
            eprintln!("remap: {off} values differ by more than 1 (worst {worst})");
            // A sample on the frame's edge may land inside on one side and outside on
            // the other; everything else is within one step
            assert!(off * 1000 < w * h * 3, "{off} values differ (worst {worst})");
        }
    }
}
