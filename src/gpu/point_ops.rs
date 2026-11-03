//! The creative per-pixel stage (white balance and exposure in linear light, display
//! curve, tone, curves, mixer, skin, wheels, HSL, LUT, film simulation, Color Chrome,
//! gamut compression) on the GPU, in bands of rows so buffers stay within the device's
//! limits. The shader mirrors [`crate::pipeline::PointOps::apply`]; any failure returns
//! None and the caller uses the CPU.

use std::sync::mpsc;

use super::{Gpu, as_bytes};
use crate::pipeline::curves::{Curves, TABLE_SIZE};
use crate::pipeline::film_sim::FilmSimulation;
use crate::pipeline::lut::Lut3D;
use crate::pipeline::skin_tone::{DEFAULT_SKIN_HUE_OKLCH, DEFAULT_SKIN_HUE_TOLERANCE};
use crate::pipeline::transfer::Transfer;
use crate::recipe::Recipe;

/// Largest band buffer, whatever the device allows.
const MAX_BUFFER: u64 = 256 << 20;
const WG: u32 = 16;
/// Images below this size stay on the CPU: the transfer would cost more than it saves.
pub const MIN_PIXELS: usize = 1 << 18;

/// Everything the stage needs besides the recipe's own numbers.
pub struct PointInputs<'a> {
    /// White balance multipliers times the exposure factor.
    pub gain: [f32; 3],
    pub transfer: Transfer,
    pub curves: &'a Curves,
    pub look_lut: Option<&'a Lut3D>,
    pub film_sim: FilmSimulation,
    pub film_lut: Option<&'a Lut3D>,
    pub color_chrome: bool,
    pub gamut_compress: bool,
}

/// Parameter slots, in the order they are packed. `P_<NAME>` constants are generated
/// for the shader from this list, so the two always agree.
const SLOTS: &[(&str, u32)] = &[
    ("WIDTH", 1),
    ("ROWS", 1),
    ("CHANNELS", 1),
    ("SRC_SHIFT", 1),
    ("K", 3),
    ("POWER", 1),
    ("SLOPE", 1),
    ("TOE_LINEAR", 1),
    ("OFFSET_ENC", 1),
    ("BT_POWER", 1),
    ("BT_SLOPE", 1),
    ("BT_TOE_ENCODED", 1),
    ("BT_TOE_LINEAR", 1),
    ("BT_OFFSET", 1),
    ("HIGHLIGHT_RECONSTRUCT", 1),
    ("HIGHLIGHTS", 1),
    ("SHADOWS", 1),
    ("WHITES", 1),
    ("BLACKS", 1),
    ("CONTRAST", 1),
    ("CONTRAST_PIVOT", 1),
    ("CURVE_HIGHLIGHTS", 1),
    ("CURVE_LIGHTS", 1),
    ("CURVE_DARKS", 1),
    ("CURVE_SHADOWS", 1),
    ("FILMIC_AGX", 1),
    ("ACES_TONEMAP", 1),
    ("SATURATION", 1),
    ("VIBRANCE", 1),
    ("COLOR_BOOST", 1),
    ("CURVE_MASTER", 1),
    ("CURVE_RED", 1),
    ("CURVE_GREEN", 1),
    ("CURVE_BLUE", 1),
    ("MIXER_ENABLED", 1),
    ("MIXER_MONO", 1),
    ("MIXER", 9),
    ("SKIN_ENABLED", 1),
    ("SKIN_TARGET", 1),
    ("SKIN_TOL", 1),
    ("SKIN_U_HUE", 1),
    ("SKIN_U_SAT", 1),
    ("SKIN_A_HUE", 1),
    ("SKIN_A_SAT", 1),
    ("WHEELS_ACTIVE", 1),
    ("LIFT", 4),
    ("GAMMA", 4),
    ("GAIN", 4),
    ("OFFSET", 4),
    ("HSL_ACTIVE", 1),
    ("HSL_HUE", 8),
    ("HSL_SAT", 8),
    ("HSL_LUM", 8),
    ("LOOK_LUT_ON", 1),
    ("LOOK_INTENSITY", 1),
    ("LOOK_LUT", 8),
    ("FILM_SIM", 1),
    ("FILM_INTENSITY", 1),
    ("FILM_LUT_ON", 1),
    ("FILM_LUT", 8),
    ("CHROME_ON", 1),
    ("CHROME_AMOUNT", 1),
    ("CHROME_BLUE", 1),
    ("ACES_ON", 1),
];

fn slot(name: &str) -> usize {
    let mut at = 0;
    for (n, len) in SLOTS {
        if *n == name {
            return at;
        }
        at += *len as usize;
    }
    panic!("unknown parameter slot {name}");
}

fn slot_count() -> usize {
    SLOTS.iter().map(|s| s.1 as usize).sum()
}

fn shader_source() -> String {
    let mut src = String::new();
    let mut at = 0u32;
    for (name, len) in SLOTS {
        src.push_str(&format!("const P_{name}: u32 = {at}u;\n"));
        at += len;
    }
    src.push_str(include_str!("point_ops.wgsl"));
    src
}

struct Params(Vec<u32>);

impl Params {
    fn f(&mut self, name: &str, v: &[f32]) {
        let at = slot(name);
        for (i, x) in v.iter().enumerate() {
            self.0[at + i] = x.to_bits();
        }
    }
    fn u(&mut self, name: &str, v: u32) {
        self.0[slot(name)] = v;
    }
    fn b(&mut self, name: &str, v: bool) {
        self.u(name, u32::from(v));
    }
}

/// Packs the recipe and the tables. Returns the parameters and the table buffer.
fn pack(recipe: &Recipe, inputs: &PointInputs) -> (Params, Vec<f32>) {
    let mut p = Params(vec![0; slot_count()]);
    let [power, slope, _, toe_linear, offset] = inputs.transfer.parts();
    p.f("K", &inputs.gain.map(|g| g / 65535.0));
    p.f("POWER", &[power]);
    p.f("SLOPE", &[slope]);
    p.f("TOE_LINEAR", &[toe_linear]);
    p.f("OFFSET_ENC", &[offset]);
    let [bp, bs, bte, btl, bo] = Transfer::bt709().parts();
    p.f("BT_POWER", &[bp]);
    p.f("BT_SLOPE", &[bs]);
    p.f("BT_TOE_ENCODED", &[bte]);
    p.f("BT_TOE_LINEAR", &[btl]);
    p.f("BT_OFFSET", &[bo]);

    let r = recipe;
    p.b("HIGHLIGHT_RECONSTRUCT", r.highlight_reconstruct);
    for (name, v) in [
        ("HIGHLIGHTS", r.highlights),
        ("SHADOWS", r.shadows),
        ("WHITES", r.whites),
        ("BLACKS", r.blacks),
        ("CONTRAST", r.contrast),
        ("CONTRAST_PIVOT", r.contrast_pivot),
        ("CURVE_HIGHLIGHTS", r.curve_highlights),
        ("CURVE_LIGHTS", r.curve_lights),
        ("CURVE_DARKS", r.curve_darks),
        ("CURVE_SHADOWS", r.curve_shadows),
        ("SATURATION", r.saturation),
        ("VIBRANCE", r.vibrance),
        ("COLOR_BOOST", r.color_boost),
    ] {
        p.f(name, &[v]);
    }
    p.b("FILMIC_AGX", r.filmic_agx);
    p.b("ACES_TONEMAP", r.aces_tonemap);

    // Tables: four curves, then the look LUT, then the film LUT
    let mut tables = vec![0.0f32; 4 * TABLE_SIZE];
    for (i, (name, t)) in ["CURVE_MASTER", "CURVE_RED", "CURVE_GREEN", "CURVE_BLUE"].iter().zip(inputs.curves.tables()).enumerate() {
        if let Some(t) = t {
            tables[i * TABLE_SIZE..(i + 1) * TABLE_SIZE].copy_from_slice(t);
        }
        p.b(name, t.is_some());
    }

    p.b("MIXER_ENABLED", r.rgb_mixer_enabled);
    p.b("MIXER_MONO", r.rgb_mixer_monochrome);
    p.f(
        "MIXER",
        &[
            r.mixer_red_in_r,
            r.mixer_green_in_r,
            r.mixer_blue_in_r,
            r.mixer_red_in_g,
            r.mixer_green_in_g,
            r.mixer_blue_in_g,
            r.mixer_red_in_b,
            r.mixer_green_in_b,
            r.mixer_blue_in_b,
        ],
    );

    p.b("SKIN_ENABLED", r.skin_tone_enabled);
    p.f("SKIN_TARGET", &[if r.skin_target_hue > 0.0 { r.skin_target_hue } else { DEFAULT_SKIN_HUE_OKLCH }]);
    p.f("SKIN_TOL", &[if r.skin_hue_range > 0.0 { r.skin_hue_range } else { DEFAULT_SKIN_HUE_TOLERANCE }]);
    p.f("SKIN_U_HUE", &[r.skin_uniformity_hue]);
    p.f("SKIN_U_SAT", &[r.skin_uniformity_sat]);
    p.f("SKIN_A_HUE", &[r.skin_amount_hue]);
    p.f("SKIN_A_SAT", &[r.skin_amount_sat]);

    let wheel = |c: [f32; 3], l: f32| [c[0], c[1], c[2], l];
    let wheels = [wheel(r.lift, r.lift_luma), wheel(r.gamma, r.gamma_luma), wheel(r.gain, r.gain_luma), wheel(r.offset, r.offset_luma)];
    p.b("WHEELS_ACTIVE", wheels.iter().any(|w| w.iter().any(|&v| v != 0.0)));
    for (name, w) in ["LIFT", "GAMMA", "GAIN", "OFFSET"].iter().zip(wheels) {
        p.f(name, &w);
    }

    p.b("HSL_ACTIVE", (0..8).any(|i| r.hsl_hue[i] != 0.0 || r.hsl_sat[i] != 0.0 || r.hsl_lum[i] != 0.0));
    p.f("HSL_HUE", &r.hsl_hue);
    p.f("HSL_SAT", &r.hsl_sat);
    p.f("HSL_LUM", &r.hsl_lum);

    let mut put_lut = |p: &mut Params, name: &str, lut: &Lut3D| {
        let at = slot(name);
        p.0[at] = lut.size as u32;
        p.0[at + 1] = tables.len() as u32;
        for i in 0..3 {
            p.0[at + 2 + i] = lut.domain_min[i].to_bits();
            p.0[at + 5 + i] = lut.domain_max[i].to_bits();
        }
        tables.extend(lut.table.iter().flatten());
    };
    p.b("LOOK_LUT_ON", inputs.look_lut.is_some());
    p.f("LOOK_INTENSITY", &[r.lut_intensity]);
    if let Some(l) = inputs.look_lut {
        put_lut(&mut p, "LOOK_LUT", l);
    }
    p.u("FILM_SIM", inputs.film_sim as u32);
    p.f("FILM_INTENSITY", &[r.film_sim_intensity]);
    let film_lut = inputs.film_lut.filter(|_| inputs.film_sim != FilmSimulation::None);
    p.b("FILM_LUT_ON", film_lut.is_some());
    if let Some(l) = film_lut {
        put_lut(&mut p, "FILM_LUT", l);
    }
    p.b("CHROME_ON", inputs.color_chrome);
    p.f("CHROME_AMOUNT", &[r.color_chrome]);
    p.f("CHROME_BLUE", &[r.color_chrome_blue]);
    p.b("ACES_ON", inputs.gamut_compress);
    (p, tables)
}

pub struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl Pipeline {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point_ops"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("point_ops"),
            entries: &[storage(0, true), storage(1, true), storage(2, true), storage(3, false)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point_ops"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("point_pass"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("point_pass"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { layout, pipeline }
    }
}

/// Runs the stage on rows `y0..y1` of `input` (`w` pixels of `ch` 16-bit channels per
/// row) and returns interleaved RGB f32 for those rows, or None when there is no
/// usable GPU or the job failed.
#[allow(clippy::too_many_arguments)]
pub fn run(input: &[u16], w: usize, ch: usize, y0: usize, y1: usize, recipe: &Recipe, inputs: &PointInputs) -> Option<Vec<f32>> {
    if (y1 - y0) * w < MIN_PIXELS {
        return None;
    }
    let gpu = super::get()?;
    match run_on(gpu, input, w, ch, y0, y1, recipe, inputs, usize::MAX) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("omastudio: GPU point stage failed, using the CPU: {e}");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_on(
    gpu: &Gpu,
    input: &[u16],
    w: usize,
    ch: usize,
    y0: usize,
    y1: usize,
    recipe: &Recipe,
    inputs: &PointInputs,
    max_rows: usize,
) -> Result<Vec<f32>, String> {
    if !(3..=4).contains(&ch) || y1 <= y0 || input.len() < y1 * w * ch {
        return Err("unsupported buffer".to_string());
    }
    let pipeline = gpu.point_ops.get_or_init(|| {
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let p = Pipeline::new(&gpu.device);
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(format!("shader: {e}")),
            None => Ok(p),
        }
    });
    let pipeline = pipeline.as_ref().map_err(|e| e.clone())?;
    let (mut params, tables) = pack(recipe, inputs);
    let _job = gpu.jobs.lock().map_err(|_| "GPU lock poisoned")?;

    // Band height from the largest buffer: RGB f32 out per pixel
    let limit = gpu.limits.max_storage_buffer_binding_size.min(gpu.limits.max_buffer_size).min(MAX_BUFFER);
    let rows = (limit / (w as u64 * 12)) as usize;
    if rows < 1 || (tables.len() * 4) as u64 > limit {
        return Err(format!("image {w} px wide exceeds the GPU's buffer limits"));
    }
    let rows = rows.min(max_rows).min(y1 - y0).min(65535 * WG as usize);

    let device = &gpu.device;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let buffer = |label: &str, size: usize, usage: wgpu::BufferUsages| {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (size.max(4) as u64).next_multiple_of(4), usage, mapped_at_creation: false })
    };
    let storage = wgpu::BufferUsages::STORAGE;
    let param_buf = buffer("params", params.0.len() * 4, storage | wgpu::BufferUsages::COPY_DST);
    // One spare sample in front for bands that start on an odd sample, one behind for
    // an odd count
    let src = buffer("src", (rows * w * ch + 2) * 2, storage | wgpu::BufferUsages::COPY_DST);
    let table_buf = buffer("tables", tables.len() * 4, storage | wgpu::BufferUsages::COPY_DST);
    let out = buffer("out", rows * w * 12, storage | wgpu::BufferUsages::COPY_SRC);
    let staging = buffer("staging", rows * w * 12, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
    gpu.queue.write_buffer(&table_buf, 0, as_bytes(&tables));
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point_ops"),
        layout: &pipeline.layout,
        entries: &[&param_buf, &src, &table_buf, &out]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
            .collect::<Vec<_>>(),
    });

    let mut result = vec![0.0f32; (y1 - y0) * w * 3];
    let mut words: Vec<u32> = Vec::new();
    for by0 in (y0..y1).step_by(rows) {
        let by1 = (by0 + rows).min(y1);
        let (s0, s1) = (by0 * w * ch, by1 * w * ch);
        // Whole words: start on an even sample, the shader skips the spare one
        let shift = s0 & 1;
        let samples = &input[s0 - shift..s1];
        words.clear();
        words.extend(samples.chunks(2).map(|p| p[0] as u32 | (p.get(1).copied().unwrap_or(0) as u32) << 16));

        params.u("WIDTH", w as u32);
        params.u("ROWS", (by1 - by0) as u32);
        params.u("CHANNELS", ch as u32);
        params.u("SRC_SHIFT", shift as u32);
        gpu.queue.write_buffer(&param_buf, 0, as_bytes(&params.0));
        gpu.queue.write_buffer(&src, 0, as_bytes(&words));

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("point_ops") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups((w as u32).div_ceil(WG), ((by1 - by0) as u32).div_ceil(WG), 1);
        }
        let out_len = ((by1 - by0) * w * 12) as u64;
        enc.copy_buffer_to_buffer(&out, 0, &staging, 0, out_len);
        gpu.queue.submit([enc.finish()]);

        let (tx, rx) = mpsc::channel();
        staging.map_async(wgpu::MapMode::Read, 0..out_len, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        {
            let view = staging.get_mapped_range(0..out_len).map_err(|e| e.to_string())?;
            let dst = &mut result[(by0 - y0) * w * 3..(by1 - y0) * w * 3];
            for (d, s) in dst.iter_mut().zip(view.as_chunks::<4>().0) {
                *d = f32::from_le_bytes(*s);
            }
        }
        staging.unmap();
    }
    if let Some(e) = pollster::block_on(oom.pop()) {
        return Err(format!("out of GPU memory: {e}"));
    }
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(e.to_string());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{PointOps, white_balance};

    #[test]
    fn film_ids_match_the_shader() {
        // The shader switches on the enum's position
        let ids = [
            (FilmSimulation::None, 0),
            (FilmSimulation::FujiProvia, 1),
            (FilmSimulation::FujiClassicChrome, 4),
            (FilmSimulation::FujiEternaBleachBypass, 11),
            (FilmSimulation::FujiAcrosStandard, 12),
            (FilmSimulation::FujiMonochrome, 16),
            (FilmSimulation::FujiSepia, 20),
            (FilmSimulation::HasselbladHncs, 21),
            (FilmSimulation::HasselbladXPan, 22),
        ];
        for (sim, id) in ids {
            assert_eq!(sim as u32, id, "{sim:?}");
        }
    }

    #[test]
    fn every_slot_is_packed_once() {
        let mut names: Vec<&str> = SLOTS.iter().map(|s| s.0).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), SLOTS.len());
        assert!(shader_source().contains("const P_ACES_ON: u32"));
    }

    fn frame(w: usize, h: usize, ch: usize) -> Vec<u16> {
        // Colour ramps over the whole range, including clipped highlights
        let mut v = vec![0u16; w * h * ch];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * ch;
                let hash = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503)) >> 20;
                v[i] = ((x * 65535 / w) as u32 ^ hash) as u16;
                v[i + 1] = ((y * 65535 / h) as u32).min(65535) as u16;
                v[i + 2] = (((x + 2 * y) * 40_000 / (w + 2 * h)) as u32 + hash) as u16;
                if ch == 4 {
                    v[i + 3] = 65535;
                }
            }
        }
        v
    }

    fn recipes() -> Vec<(&'static str, Recipe)> {
        let base = Recipe::default();
        let mut out = vec![("default", base.clone())];
        let mut r = base.clone();
        r.exposure = 0.7;
        r.highlight_reconstruct = true;
        r.highlights = -40.0;
        r.shadows = 35.0;
        r.whites = 20.0;
        r.blacks = -15.0;
        r.contrast = 30.0;
        r.curve_highlights = 10.0;
        r.curve_shadows = -10.0;
        r.vibrance = 25.0;
        r.saturation = 10.0;
        r.color_boost = 15.0;
        r.curve_points = vec![[0.0, 0.0], [0.3, 0.25], [0.7, 0.8], [1.0, 1.0]];
        r.curve_points_blue = vec![[0.0, 0.05], [1.0, 0.95]];
        out.push(("tone_curves", r.clone()));
        r.filmic_agx = true;
        out.push(("agx", r.clone()));
        r.filmic_agx = false;
        r.aces_tonemap = true;
        r.gamut_compression = true;
        out.push(("aces", r.clone()));

        let mut c = base.clone();
        c.rgb_mixer_enabled = true;
        c.mixer_red_in_r = 0.9;
        c.mixer_green_in_r = 0.15;
        c.mixer_blue_in_g = 0.1;
        c.skin_tone_enabled = true;
        c.skin_uniformity_hue = 60.0;
        c.skin_uniformity_sat = 40.0;
        c.skin_amount_hue = 20.0;
        c.lift = [0.05, 0.0, -0.05];
        c.gain_luma = 0.1;
        c.offset = [0.0, 0.02, 0.0];
        c.hsl_hue = [10.0, 0.0, -20.0, 0.0, 15.0, 0.0, 0.0, 0.0];
        c.hsl_sat = [0.0, 30.0, 0.0, -40.0, 0.0, 20.0, 0.0, 0.0];
        c.hsl_lum = [0.0, 0.0, 10.0, 0.0, 0.0, -10.0, 0.0, 5.0];
        c.color_chrome = 100.0;
        c.color_chrome_blue = 50.0;
        out.push(("colour", c.clone()));
        let mut m = c.clone();
        m.rgb_mixer_monochrome = true;
        out.push(("mono_mixer", m));
        for sim in [
            "fuji_provia", "fuji_velvia", "fuji_astia", "fuji_classic_chrome", "fuji_classic_neg", "fuji_nostalgic_neg",
            "fuji_pro_neg_hi", "fuji_pro_neg_std", "fuji_reala_ace", "fuji_eterna", "fuji_eterna_bleach_bypass",
            "fuji_acros", "fuji_acros_red", "fuji_monochrome_green", "fuji_sepia", "hasselblad_hncs", "hasselblad_xpan",
        ] {
            let mut f = base.clone();
            f.film_simulation = sim.into();
            f.film_sim_intensity = if sim == "fuji_velvia" { 0.6 } else { 1.0 };
            out.push((sim, f));
        }
        let mut l = base.clone();
        l.lut_name = Some("Kodak 2383".into());
        l.lut_intensity = 0.8;
        out.push(("look_lut", l));
        out
    }

    /// Runs only where a GPU (or, with OMASTUDIO_GPU=software, lavapipe) is available.
    #[test]
    fn gpu_matches_cpu() {
        let Some(gpu) = super::super::get() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let transfer = Transfer::bt709();
        for ch in [3usize, 4] {
            let (w, h) = (161, 97);
            let input = frame(w, h, ch);
            for (name, recipe) in recipes() {
                let ops = PointOps::new(&recipe);
                let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
                let e = 2.0f32.powf(recipe.exposure + recipe.baseline_exposure);
                let inputs = ops.gpu_inputs([wr * e, wg * e, wb * e], transfer);
                // Bands of 13 rows, starting on odd samples with 3 channels
                let got = run_on(gpu, &input, w, ch, 0, h, &recipe, &inputs, 13).expect("GPU point stage");
                let mut worst = 0.0f32;
                let mut off = 0;
                for i in 0..w * h {
                    let px = &input[i * ch..i * ch + 3];
                    let k = [wr * e, wg * e, wb * e];
                    let enc = |c: usize| transfer.encode(px[c] as f32 / 65535.0 * k[c]);
                    let (r, g, b) = ops.apply(enc(0), enc(1), enc(2));
                    for (c, v) in [r, g, b].into_iter().enumerate() {
                        let d = (v - got[i * 3 + c]).abs();
                        if !(d <= 2e-3) {
                            off += 1;
                        }
                        if d.is_finite() {
                            worst = worst.max(d);
                        } else {
                            worst = f32::INFINITY;
                        }
                    }
                }
                eprintln!("{name} ch={ch}: {off} values differ by more than 2e-3 (worst {worst})");
                // Transcendental functions round differently on the GPU; a value that
                // sits on a branch threshold may take the other branch
                assert!(off * 1000 < w * h * 3, "{name} ch={ch}: {off} values differ (worst {worst})");
                assert!(worst < 0.05, "{name} ch={ch}: worst difference {worst}");
            }
        }
    }
}
