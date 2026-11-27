//! Demosaic on the GPU, in bands of rows so buffers stay within the device's limits
//! (a 100 MP frame on an integrated GPU works the same as a 24 MP one on a large
//! card). The shader mirrors [`crate::raw::demosaic::demosaic_map`] stage by stage.

use std::sync::mpsc;

use super::{Gpu, as_bytes};
use crate::raw::demosaic::{GPU_PHASE_STRIDE, Mosaic, gpu_tables};

/// Rows of context the CPU tiles use around their output ([`crate::raw::demosaic`]'s
/// BORDER); the band repeats them so its output matches exactly.
const BORDER: usize = 8;
/// Extra mosaic rows the green taps and the corner fallback read beyond the band.
const TAP_REACH: usize = 3;
/// Largest band buffer, whatever the device allows.
const MAX_BUFFER: u64 = 256 << 20;
const WG: u32 = 16;
/// Tallest band; more rows only cost memory, the dispatches are already full.
const MAX_BAND_ROWS: usize = 512;

pub struct Pipelines {
    layout: wgpu::BindGroupLayout,
    stages: [wgpu::ComputePipeline; 5],
    buffers: std::sync::Mutex<Option<DemosaicBuffers>>,
}

struct DemosaicBuffers {
    mosaic_size: u64,
    phases_size: u64,
    green_size: u64,
    cand_size: u64,
    homo_size: u64,
    sel_size: u64,
    out_size: u64,
    last_used: std::time::Instant,
    params: wgpu::Buffer,
    mosaic: wgpu::Buffer,
    phases: wgpu::Buffer,
    _green: wgpu::Buffer,
    _cand: wgpu::Buffer,
    _homo: wgpu::Buffer,
    _sel: wgpu::Buffer,
    out: wgpu::Buffer,
    staging: [wgpu::Buffer; 2],
    bind: wgpu::BindGroup,
}

impl DemosaicBuffers {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        mosaic_size: u64,
        phases_size: u64,
        green_size: u64,
        cand_size: u64,
        homo_size: u64,
        sel_size: u64,
        out_size: u64,
    ) -> Self {
        let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(4).next_multiple_of(4),
                usage,
                mapped_at_creation: false,
            })
        };
        let storage = wgpu::BufferUsages::STORAGE;
        let params = buffer("demosaic params", 96, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let mosaic = buffer("demosaic mosaic", mosaic_size, storage | wgpu::BufferUsages::COPY_DST);
        let phases = buffer("demosaic phases", phases_size, storage | wgpu::BufferUsages::COPY_DST);
        let green = buffer("demosaic green", green_size, storage);
        let cand = buffer("demosaic cand", cand_size, storage);
        let homo = buffer("demosaic homo", homo_size, storage);
        let sel = buffer("demosaic sel", sel_size, storage);
        let out = buffer("demosaic out", out_size, storage | wgpu::BufferUsages::COPY_SRC);
        let readback = wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST;
        let staging = [buffer("demosaic staging0", out_size, readback), buffer("demosaic staging1", out_size, readback)];
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("demosaic"),
            layout,
            entries: &[&params, &mosaic, &phases, &green, &cand, &homo, &sel, &out]
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
                .collect::<Vec<_>>(),
        });
        Self {
            mosaic_size,
            phases_size,
            green_size,
            cand_size,
            homo_size,
            sel_size,
            out_size,
            last_used: std::time::Instant::now(),
            params,
            mosaic,
            phases,
            _green: green,
            _cand: cand,
            _homo: homo,
            _sel: sel,
            out,
            staging,
            bind,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fits(&self, mosaic: u64, phases: u64, green: u64, cand: u64, homo: u64, sel: u64, out: u64) -> bool {
        self.mosaic_size >= mosaic
            && self.phases_size >= phases
            && self.green_size >= green
            && self.cand_size >= cand
            && self.homo_size >= homo
            && self.sel_size >= sel
            && self.out_size >= out
    }

    fn total_bytes(&self) -> u64 {
        96u64
            .saturating_add(self.mosaic_size)
            .saturating_add(self.phases_size)
            .saturating_add(self.green_size)
            .saturating_add(self.cand_size)
            .saturating_add(self.homo_size)
            .saturating_add(self.sel_size)
            .saturating_add(self.out_size.saturating_mul(3))
    }
}

impl Pipelines {
    fn new(device: &wgpu::Device, cache: Option<&wgpu::PipelineCache>) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("demosaic"),
            source: wgpu::ShaderSource::Wgsl(include_str!("demosaic.wgsl").into()),
        });
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        entries.extend([storage(1, true), storage(2, true)]);
        entries.extend((3..=7).map(|b| storage(b, false)));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("demosaic"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("demosaic"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let stage = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache,
            })
        };
        let stages = ["green_pass", "cand_pass", "homo_pass", "select_pass", "median_pass"].map(stage);
        Self { layout, stages, buffers: std::sync::Mutex::new(None) }
    }
}

/// Demosaics `m` and converts it with `matrix` (camera RGB to the output space) to
/// interleaved 16-bit RGB, or None when there is no usable GPU or the job failed
/// (the caller then uses the CPU).
pub fn demosaic(m: &Mosaic, matrix: [[f32; 3]; 3]) -> Option<Vec<u16>> {
    let gpu = super::acquire()?;
    match run(&gpu, m, matrix, MAX_BAND_ROWS) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("omastudio: GPU demosaic failed, using the CPU: {e}");
            None
        }
    }
}

struct Band {
    out_y0: usize,
    out_y1: usize,
    ext_y0: usize,
    ext_y1: usize,
    sel_y0: usize,
    sel_y1: usize,
    mosaic_y0: usize,
    mosaic_y1: usize,
}

impl Band {
    fn new(y0: usize, rows: usize, h: usize) -> Self {
        let out_y1 = (y0 + rows).min(h);
        let ext_y0 = y0.saturating_sub(BORDER);
        let ext_y1 = (out_y1 + BORDER).min(h);
        Self {
            out_y0: y0,
            out_y1,
            ext_y0,
            ext_y1,
            sel_y0: y0.saturating_sub(1),
            sel_y1: (out_y1 + 1).min(h),
            mosaic_y0: ext_y0.saturating_sub(TAP_REACH),
            mosaic_y1: (ext_y1 + TAP_REACH).min(h),
        }
    }

    fn params(&self, w: usize, h: usize, period: u32, matrix: &[[f32; 3]; 3]) -> [u32; 24] {
        let mut p = [0u32; 24];
        let ints = [w, h, self.mosaic_y0, self.ext_y0, self.ext_y1, self.out_y0, self.out_y1, self.sel_y0, self.sel_y1];
        for (slot, v) in p.iter_mut().zip(ints) {
            *slot = v as u32;
        }
        p[9] = period;
        for (r, row) in matrix.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                p[12 + r * 4 + c] = v.to_bits();
            }
        }
        p
    }
}

struct InFlight<'a> {
    buf: &'a wgpu::Buffer,
    guard: super::StagingGuard<'a>,
    out_y0: usize,
    out_y1: usize,
    len: u64,
    idx: wgpu::SubmissionIndex,
    rx: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

fn run(gpu: &Gpu, m: &Mosaic, matrix: [[f32; 3]; 3], max_rows: usize) -> Result<Vec<u16>, String> {
    let (w, h) = (m.width, m.height);
    if w < 6 || h < 6 || m.data.len() < w * h {
        return Err("image too small".to_string());
    }
    let (period, table) = gpu_tables(m.pattern).ok_or("CFA layout not supported on the GPU")?;
    debug_assert_eq!(table.len() % GPU_PHASE_STRIDE, 0);
    let pipelines = gpu.demosaic.get_or_init(|| {
        if gpu.limits.max_storage_buffers_per_shader_stage < 7 {
            return Err(format!(
                "{}: demosaic needs at least 7 storage buffers per shader (has {})",
                gpu.info.name, gpu.limits.max_storage_buffers_per_shader_stage
            ));
        }
        if gpu.limits.max_compute_workgroup_storage_size < 16_384 {
            return Err(format!(
                "{}: demosaic needs at least 16384 bytes of workgroup storage (has {})",
                gpu.info.name, gpu.limits.max_compute_workgroup_storage_size
            ));
        }
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let p = Pipelines::new(&gpu.device, gpu.pipeline_cache.as_ref());
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(format!("shader: {e}")),
            None => {
                gpu.persist_pipeline_cache();
                Ok(p)
            }
        }
    });
    let pipelines = pipelines.as_ref().map_err(|e| e.clone())?;

    // Band height from the largest buffer: 4 candidates x RGB x f32 per pixel
    let limit = gpu.limits.max_storage_buffer_binding_size.min(gpu.limits.max_buffer_size).min(MAX_BUFFER);
    let row_cost = (w * 48) as u64;
    let rows = (limit / row_cost).saturating_sub(2 * BORDER as u64) as usize;
    if rows < 8 {
        return Err(format!("image {w} px wide exceeds the GPU's buffer limits"));
    }
    let rows = rows.min(max_rows).min(h);
    let ext_rows = rows + 2 * BORDER;
    let mosaic_rows = ext_rows + 2 * TAP_REACH;
    let row_words = (w * 3).div_ceil(2);
    let row_bytes = row_words * 4;

    let mosaic_size = (mosaic_rows * w * 4) as u64;
    let phases_size = (table.len() * 4) as u64;
    let green_size = (ext_rows * w * 16) as u64;
    let cand_size = (ext_rows * w * 48) as u64;
    let homo_size = (ext_rows * w * 4) as u64;
    let sel_size = ((rows + 2) * w * 12) as u64;
    let out_size = (rows * row_bytes) as u64;

    let device = &gpu.device;
    let timeout = gpu.poll_timeout(false);
    let mut result = vec![0u16; w * h * 3];
    let groups = |n: usize| (n as u32).div_ceil(WG);
    let mut next_y0 = 0usize;

    while next_y0 < h {
        let _job = gpu.lock_job(false);
        let mut cache = pipelines.buffers.lock().map_err(|_| "GPU demosaic buffer lock poisoned")?;
        if cache
            .as_ref()
            .is_some_and(|b| b.total_bytes() > super::PREVIEW_VRAM_CAP && b.last_used.elapsed() >= super::OVERSIZED_IDLE_EVICT)
        {
            *cache = None;
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let seg = (|| -> Result<usize, String> {
            if !cache.as_ref().is_some_and(|b| b.fits(mosaic_size, phases_size, green_size, cand_size, homo_size, sel_size, out_size)) {
                let (mo, ph, gr, ca, ho, se, ou) = cache.as_ref().map_or((0, 0, 0, 0, 0, 0, 0), |b| {
                    (b.mosaic_size, b.phases_size, b.green_size, b.cand_size, b.homo_size, b.sel_size, b.out_size)
                });
                *cache = None;
                *cache = Some(DemosaicBuffers::new(
                    device,
                    &pipelines.layout,
                    mosaic_size.max(mo),
                    phases_size.max(ph),
                    green_size.max(gr),
                    cand_size.max(ca),
                    homo_size.max(ho),
                    sel_size.max(se),
                    out_size.max(ou),
                ));
            }
            let bufs = cache.as_ref().expect("demosaic buffers");
            gpu.queue.write_buffer(&bufs.phases, 0, as_bytes(&table));

            let collect = |mut p: InFlight<'_>, result: &mut [u16]| -> Result<(), String> {
                gpu.wait_submission(Some(p.idx), timeout)?;
                match p.rx.recv().map_err(|e| e.to_string())? {
                    Ok(()) => {}
                    Err(e) => {
                        p.guard.disarm();
                        return Err(e.to_string());
                    }
                }
                {
                    let view = p.buf.get_mapped_range(0..p.len).map_err(|e| e.to_string())?;
                    for ry in 0..(p.out_y1 - p.out_y0) {
                        let src_row = &view[ry * row_bytes..ry * row_bytes + w * 6];
                        let dst_row = &mut result[(p.out_y0 + ry) * w * 3..(p.out_y0 + ry + 1) * w * 3];
                        for (d, s) in dst_row.iter_mut().zip(src_row.as_chunks::<2>().0) {
                            *d = u16::from_le_bytes(*s);
                        }
                    }
                }
                p.guard.unmap_now();
                Ok(())
            };

            let mut pending: Option<InFlight<'_>> = None;
            let mut y0 = next_y0;
            let mut band_i = 0usize;
            while y0 < h {
                // Yield between bands when an interactive preview is waiting for the GPU.
                if band_i > 0 && gpu.has_interactive_waiter() {
                    break;
                }
                let band = Band::new(y0, rows, h);
                gpu.queue.write_buffer(&bufs.params, 0, as_bytes(&band.params(w, h, period, &matrix)));
                gpu.queue.write_buffer(&bufs.mosaic, 0, as_bytes(&m.data[band.mosaic_y0 * w..band.mosaic_y1 * w]));
                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("demosaic") });
                let ext = band.ext_y1 - band.ext_y0;
                let dispatch = [
                    (groups(w), groups(ext)),
                    (groups(w), groups(ext)),
                    (groups(w), groups(ext)),
                    (groups(w), groups(band.sel_y1 - band.sel_y0)),
                    (groups(w.div_ceil(2)), groups(band.out_y1 - band.out_y0)),
                ];
                for (stage, (gx, gy)) in pipelines.stages.iter().zip(dispatch) {
                    // A pass per stage: each reads what the previous one wrote
                    let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                    pass.set_pipeline(stage);
                    pass.set_bind_group(0, &bufs.bind, &[]);
                    pass.dispatch_workgroups(gx, gy, 1);
                }
                let k = band_i % 2;
                let out_len = ((band.out_y1 - band.out_y0) * row_bytes) as u64;
                enc.copy_buffer_to_buffer(&bufs.out, 0, &bufs.staging[k], 0, out_len);
                let idx = gpu.queue.submit([enc.finish()]);

                let (tx, rx) = mpsc::channel();
                bufs.staging[k].map_async(wgpu::MapMode::Read, 0..out_len, move |r| {
                    let _ = tx.send(r);
                });
                let next_guard = super::StagingGuard::new(&bufs.staging[k]);
                if let Some(p) = pending.take() {
                    collect(p, &mut result)?;
                }
                pending = Some(InFlight {
                    buf: &bufs.staging[k],
                    guard: next_guard,
                    out_y0: band.out_y0,
                    out_y1: band.out_y1,
                    len: out_len,
                    idx,
                    rx,
                });
                y0 = band.out_y1;
                band_i += 1;
            }
            if let Some(p) = pending.take() {
                collect(p, &mut result)?;
            }
            Ok(y0)
        })();

        let scope_res = super::drain_scopes(oom, scope);
        if scope_res.is_err() || seg.is_err() {
            *cache = None;
        }
        scope_res?;
        next_y0 = seg?;
        if let Some(b) = cache.as_mut() {
            b.last_used = std::time::Instant::now();
        }
        if next_y0 >= h && cache.as_ref().is_some_and(|b| b.total_bytes() > super::PREVIEW_VRAM_CAP) {
            *cache = None;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::demosaic::{CfaPattern, demosaic_map};

    const XTRANS: [[u8; 6]; 6] = [
        [1, 1, 0, 1, 1, 2],
        [1, 1, 2, 1, 1, 0],
        [2, 0, 1, 0, 2, 1],
        [1, 1, 2, 1, 1, 0],
        [1, 1, 0, 1, 1, 2],
        [0, 2, 1, 2, 0, 1],
    ];

    fn scene(y: usize, x: usize) -> [f32; 3] {
        let (fy, fx) = (y as f32, x as f32);
        let stripe = ((fx * 0.31).sin() * (fy * 0.17).cos() > 0.2) as u8 as f32;
        let disc = (((fx - 70.0).powi(2) + (fy - 50.0).powi(2)) < 900.0) as u8 as f32;
        let ramp = fx / 200.0;
        [0.05 + 0.7 * stripe * (1.0 - disc) + 0.2 * ramp, 0.1 + 0.5 * disc + 0.1 * stripe, 0.08 + 0.6 * (1.0 - stripe) * ramp + 0.3 * disc]
    }

    #[test]
    fn shader_uses_the_cpu_homogeneity_margin() {
        use crate::raw::demosaic::{HOMO_SLACK_C, HOMO_SLACK_L, HOMO_SLACK_REL};
        let src = include_str!("demosaic.wgsl");
        for (name, v) in [("HOMO_SLACK_REL", HOMO_SLACK_REL), ("HOMO_SLACK_L", HOMO_SLACK_L), ("HOMO_SLACK_C", HOMO_SLACK_C)] {
            assert!(src.contains(&format!("const {name}: f32 = {v:e};")), "{name} differs between the CPU and the shader");
        }
    }

    /// Runs only where a GPU (or, with OMASTUDIO_GPU=software, lavapipe) is available.
    #[test]
    fn gpu_matches_cpu() {
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu_dev) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let matrix = [[1.6, -0.4, -0.2], [-0.2, 1.4, -0.2], [0.0, -0.3, 1.3]];
        let to_u16 = |p: [f32; 3]| -> [u16; 3] {
            std::array::from_fn(|r| {
                let v = matrix[r][0] * p[0] + matrix[r][1] * p[1] + matrix[r][2] * p[2];
                (v.clamp(0.0, 1.0) * 65535.0).round() as u16
            })
        };
        for pattern in [CfaPattern::Bayer(0x94949494), CfaPattern::XTrans(XTRANS)] {
            let (w, h) = (203, 157);
            let data: Vec<f32> = (0..w * h).map(|i| scene(i / w, i % w)[pattern.color(i / w, i % w)]).collect();
            let mosaic = Mosaic { width: w, height: h, data: &data, pattern };
            let cpu = demosaic_map(&mosaic, to_u16);
            let gpu = demosaic(&mosaic, matrix).expect("GPU demosaic");
            // Bands much shorter than the image must stitch without seams
            let banded = run(&gpu_dev, &mosaic, matrix, 37).expect("banded GPU demosaic");
            assert_eq!(gpu, banded, "{pattern:?}: band seams");
            let diffs: Vec<u16> = cpu.iter().zip(&gpu).map(|(a, b)| a.abs_diff(*b)).collect();
            let off = diffs.iter().filter(|&&d| d > 2).count();
            let worst = diffs.iter().copied().max().unwrap_or(0);
            // Float rounding may flip a near-tie in direction selection on a few pixels
            eprintln!("{pattern:?}: {off} of {} values differ by more than 2 (worst {worst})", diffs.len());
            assert!(off * 2000 < diffs.len(), "{pattern:?}: {off} values differ by more than 2 (worst {worst})");
        }
    }
}
