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
/// Cap on a single storage buffer binding for the local stage (256 MiB). Desktop
/// Vulkan adapters with >= 256 MiB bindings process 4K previews (3840x2560, planes =
/// 225 MiB) in one band with zero overhead, while 50-150 MP frames automatically
/// stripe into halo-overlapped row bands that also fit on 128 MiB adapters.
const MAX_STRIP_BINDING: u64 = 256 << 20;
/// Minimum non-halo output rows per band when row-striping.
const MIN_INNER_ROWS: usize = 32;
/// Pixels each blur thread slides over; must match CHUNK in the shader.
const CHUNK: u32 = 32;
/// Maximum box-blur radius using shared-memory tiled blur kernels (`blur_h_tiled` / `blur_v_tiled`).
const TILED_BLUR_MAX_R: u32 = 64;
/// Minimum workgroup storage required to fuse `hist_main` + `hist_wf` + `hist_par` into `hist_all` (33,800 B).
const COMBINED_HIST_WG_STORAGE: u32 = 34_816;
/// Precomputed luma multiplication table size in uniform memory (768 f32 = 3072 B).
const LUMA_TERMS_BYTES: u64 = 768 * 4;
/// Parameter words per step (40 x u32 = 160 bytes, matching `struct Params` in local.wgsl).
const PARAMS_WORDS: usize = 40;
/// Size of one step's parameters in the uniform buffer.
const PARAMS_SIZE: u64 = (PARAMS_WORDS * 4) as u64;
/// Histogram workgroups; must match HIST_GROUPS in the shader.
const HIST_GROUPS: u32 = 256;

const ENTRIES: &[&str] = &[
    "luma_init",
    "blur_h",
    "blur_v",
    "blur_h_tiled",
    "blur_v_tiled",
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

/// Sentinel entry index dispatched to `Pipeline::hist_all` when the adapter exposes
/// at least `COMBINED_HIST_WG_STORAGE` bytes of workgroup storage.
const HIST_ALL_ENTRY: usize = ENTRIES.len();

/// Optional single-pass scope kernel combining `hist_main`, `hist_wf`, and `hist_par`
/// (`8450 * 4 = 33,800 B` workgroup storage), reading each `px8` pixel once instead of 3x.
const HIST_ALL_WGSL: &str = r#"
struct Params {
    w: u32,
    h: u32,
    src: u32,
    dst: u32,
    r: u32,
    mode: u32,
    flags: u32,
    pad: u32,
    f0: f32,
    f1: f32,
    f2: f32,
    f3: f32,
    f4: f32,
    f5: f32,
    f6: f32,
    f7: f32,
    g: array<vec4<f32>, 4>,
    strip0: vec4<u32>,
    strip1: vec4<u32>,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(3) var<storage, read_write> px8: array<u32>;
@group(0) @binding(5) var<storage, read_write> hist: array<atomic<u32>>;
@group(0) @binding(7) var<uniform> luma_terms: array<vec4<f32>, 192>;

const HIST_GROUPS: u32 = 256u;

fn luma_term(idx: u32) -> f32 {
    return luma_terms[idx >> 2u][idx & 3u];
}

fn luma8(v: u32) -> u32 {
    let r = luma_term(v & 255u);
    let g = luma_term(256u + ((v >> 8u) & 255u));
    let b = luma_term(512u + ((v >> 16u) & 255u));
    return min(u32((r + g) + b), 255u);
}

var<workgroup> h_all: array<atomic<u32>, 8450>;

@compute @workgroup_size(256, 1, 1)
fn hist_all(@builtin(local_invocation_index) li: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    for (var k = li; k < 8450u; k += 256u) {
        atomicStore(&h_all[k], 0u);
    }
    workgroupBarrier();
    let n = p.w * p.h;
    for (var i = wg.x * 256u + li; i < n; i += 256u * HIST_GROUPS) {
        let v = px8[i];
        let r = v & 255u;
        let g = (v >> 8u) & 255u;
        let b = (v >> 16u) & 255u;
        let luma = luma8(v);
        atomicAdd(&h_all[r], 1u);
        atomicAdd(&h_all[256u + g], 1u);
        atomicAdd(&h_all[512u + b], 1u);
        atomicAdd(&h_all[768u + luma], 1u);
        if (r >= 254u || g >= 254u || b >= 254u) {
            atomicAdd(&h_all[1025u], 1u);
        } else if (r <= 1u && g <= 1u && b <= 1u) {
            atomicAdd(&h_all[1024u], 1u);
        }
        let rf = f32(r) / 255.0;
        let gf = f32(g) / 255.0;
        let bf = f32(b) / 255.0;
        let cb = -0.1146 * rf - 0.3854 * gf + 0.5000 * bf;
        let cr = 0.5000 * rf - 0.4542 * gf - 0.0458 * bf;
        let u = u32(clamp((cb + 0.5) * 47.0, 0.0, 47.0));
        let vv = u32(clamp(47.0 - (cr + 0.5) * 47.0, 0.0, 47.0));
        atomicAdd(&h_all[1026u + vv * 48u + u], 1u);

        let x = i % p.w;
        let wx = min((x * 64u) / p.w, 63u);
        let wy = 31u - (luma * 31u) / 255u;
        atomicAdd(&h_all[3330u + wy * 64u + wx], 1u);

        let col = min((x * 32u) / p.w, 31u);
        let rr = 31u - (r * 31u) / 255u;
        let gr = 31u - (g * 31u) / 255u;
        let br = 31u - (b * 31u) / 255u;
        atomicAdd(&h_all[5378u + rr * 32u + col], 1u);
        atomicAdd(&h_all[5378u + 1024u + gr * 32u + col], 1u);
        atomicAdd(&h_all[5378u + 2048u + br * 32u + col], 1u);
    }
    workgroupBarrier();
    for (var k = li; k < 8450u; k += 256u) {
        let c = atomicLoad(&h_all[k]);
        if (c > 0u) {
            atomicAdd(&hist[k], c);
        }
    }
}
"#;

/// A step's parameter words, zero after `head`.
fn words(head: &[u32]) -> [u32; PARAMS_WORDS] {
    let mut p = [0u32; PARAMS_WORDS];
    p[..head.len()].copy_from_slice(head);
    p
}

fn entry(name: &str) -> usize {
    ENTRIES.iter().position(|e| *e == name).unwrap_or_else(|| panic!("unknown entry point {name}"))
}

/// One dispatch: its kernel, parameters and workgroup counts.
struct Step {
    entry: usize,
    params: [u32; PARAMS_WORDS],
    groups: (u32, u32),
}

/// Tight per-recipe vertical halo (in rows) required around a horizontal strip so the
/// active neighbourhood passes (presence, detail, defringe) produce identical interior
/// pixels to a whole-frame run. Returns `0` when no neighbourhood pass is active.
pub fn required_halo(recipe: &Recipe, scale: f32) -> usize {
    let mut presence_r = 0usize;
    let (clarity, texture, dehaze, md) = (recipe.clarity, recipe.texture, recipe.dehaze, recipe.midtone_detail);
    let any = clarity.abs() >= 0.01 || texture.abs() >= 0.01 || dehaze.abs() >= 0.01 || md.abs() >= 0.01;
    let modules = texture.abs() >= 0.5 || clarity.abs() >= 0.5 || dehaze.abs() >= 0.5 || md.abs() >= 0.5;
    if any && modules {
        if texture.abs() >= 0.5 {
            presence_r = presence_r.max(scaled_radius(6, scale));
        }
        if clarity.abs() >= 0.5 {
            presence_r = presence_r.max(scaled_radius(16, scale));
        }
        if md.abs() >= 0.5 {
            presence_r = presence_r.max(scaled_radius(9, scale));
        }
        if dehaze.abs() >= 0.5 {
            presence_r = presence_r.max(scaled_radius(12, scale));
        }
    }

    let mut detail_r = 0usize;
    let (sharpness, dn_lum, dn_col) = (recipe.sharpness, recipe.denoise_lum, recipe.denoise_col);
    if sharpness > 0.1 || dn_lum > 0.1 || dn_col > 0.1 {
        if dn_col > 0.5 {
            let base = if dn_col > 40.0 { 2 } else { 1 };
            detail_r = detail_r.max(scaled_radius(base, scale));
        }
        if dn_lum > 0.5 || sharpness > 0.5 {
            detail_r = detail_r.max(1);
        }
    }

    let defringe_r = if recipe.defringe > 0.01 {
        1 + scaled_radius(2, scale)
    } else {
        0
    };

    let total = presence_r + detail_r + defringe_r;
    if total == 0 { 0 } else { total + 2 }
}

/// Frame-clamped `(halo, max_blur_radius)` matching the exact blur radii dispatched by [`plan`].
fn effective_halo_and_blur(recipe: &Recipe, scale: f32, frame_w: usize, frame_h: usize) -> (usize, usize) {
    let clamp_r = |base: usize| scaled_radius(base, scale).min(frame_w / 2).min(frame_h / 2).max(1);
    let mut max_blur = 0usize;
    let mut presence_r = 0usize;
    let (clarity, texture, dehaze, md) = (recipe.clarity, recipe.texture, recipe.dehaze, recipe.midtone_detail);
    let any = clarity.abs() >= 0.01 || texture.abs() >= 0.01 || dehaze.abs() >= 0.01 || md.abs() >= 0.01;
    let modules = texture.abs() >= 0.5 || clarity.abs() >= 0.5 || dehaze.abs() >= 0.5 || md.abs() >= 0.5;
    if any && modules {
        if texture.abs() >= 0.5 {
            let r = clamp_r(6);
            presence_r = presence_r.max(r);
            max_blur = max_blur.max(r);
        }
        if clarity.abs() >= 0.5 {
            let r = clamp_r(16);
            presence_r = presence_r.max(r);
            max_blur = max_blur.max(r);
        }
        if md.abs() >= 0.5 {
            let r = clamp_r(9);
            presence_r = presence_r.max(r);
            max_blur = max_blur.max(r);
        }
        if dehaze.abs() >= 0.5 {
            let r = clamp_r(12);
            presence_r = presence_r.max(r);
            max_blur = max_blur.max(r);
        }
    }

    let mut detail_r = 0usize;
    let (sharpness, dn_lum, dn_col) = (recipe.sharpness, recipe.denoise_lum, recipe.denoise_col);
    if (sharpness > 0.1 || dn_lum > 0.1 || dn_col > 0.1) && frame_w >= 3 && frame_h >= 3 {
        if dn_col > 0.5 {
            let r = clamp_r(if dn_col > 40.0 { 2 } else { 1 });
            detail_r = detail_r.max(r);
            max_blur = max_blur.max(r);
        }
        if dn_lum > 0.5 || sharpness > 0.5 {
            detail_r = detail_r.max(1);
        }
    }

    let defringe_r = if recipe.defringe > 0.01 && frame_w >= 3 && frame_h >= 3 {
        let r = clamp_r(2);
        max_blur = max_blur.max(r);
        1 + r
    } else {
        0
    };

    let total = presence_r + detail_r + defringe_r;
    let halo = if total == 0 { 0 } else { total + 2 };
    (halo, max_blur)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StripPlan {
    max_strip_rows: usize,
    halo: usize,
    inner_rows: usize,
    bands: usize,
    band_rows: usize,
}

impl StripPlan {
    fn is_single_band(&self) -> bool {
        self.bands <= 1
    }
}

fn gpu_strip_limit(gpu: &Gpu) -> u64 {
    gpu.limits
        .max_storage_buffer_binding_size
        .min(gpu.limits.max_buffer_size)
        .min(MAX_STRIP_BINDING)
}

fn compute_strip_plan(
    w: usize,
    h: usize,
    halo: usize,
    max_blur_r: usize,
    limit_bytes: u64,
    max_groups: u32,
) -> Result<StripPlan, String> {
    if w == 0 || h == 0 {
        return Err("empty frame".to_string());
    }
    let wu = u32::try_from(w).map_err(|_| format!("{w}x{h} exceeds GPU dimensions"))?;
    if wu.div_ceil(16) > max_groups || wu.div_ceil(64) > max_groups || wu.div_ceil(CHUNK).div_ceil(64) > max_groups {
        return Err(format!("{w}x{h} needs more horizontal workgroups than the GPU allows"));
    }
    let row_bytes_planes = (w as u64)
        .checked_mul(4 * PLANES)
        .ok_or_else(|| format!("{w}x{h} overflows GPU buffer size"))?;
    let limit = limit_bytes.min(u32::MAX as u64);
    let max_rows_by_mem = (limit / row_bytes_planes) as usize;
    // blur_h dispatches one workgroup row per strip row (groups.1 == strip_h),
    // so max_strip_rows must not exceed max_groups.
    let max_strip_rows = max_rows_by_mem.min(max_groups as usize);
    if max_strip_rows >= h {
        return Ok(StripPlan {
            max_strip_rows: h,
            halo: 0,
            inner_rows: h,
            bands: 1,
            band_rows: h,
        });
    }
    let min_for_halo = halo.saturating_mul(2).saturating_add(MIN_INNER_ROWS);
    let min_for_blur = max_blur_r.saturating_mul(2).saturating_add(1);
    if max_strip_rows < min_for_halo || max_strip_rows < min_for_blur {
        return Err(format!(
            "{w}x{h} exceeds the GPU's strip limits (max_strip_rows={max_strip_rows}, halo={halo}, blur_r={max_blur_r})"
        ));
    }
    let inner_rows = max_strip_rows - halo * 2;
    let bands = h.div_ceil(inner_rows).max(1);
    let band_rows = h.div_ceil(bands).max(1);
    Ok(StripPlan {
        max_strip_rows,
        halo,
        inner_rows,
        bands,
        band_rows,
    })
}

/// Recommended interior row count per band for banded 16-bit exports so each outer
/// band fits inside a single GPU strip on the active adapter (including 128 MiB
/// binding adapters on 100-150 MP frames). Returns `None` when no GPU is used.
pub fn recommended_band_rows(w: usize, h: usize, recipe: &Recipe, scale: f32) -> Option<usize> {
    let gpu = super::acquire().filter(|g| super::stages_worthwhile(g))?;
    let (halo, max_blur_r) = effective_halo_and_blur(recipe, scale, w, h);
    let limit = gpu_strip_limit(&gpu);
    let max_groups = gpu.limits.max_compute_workgroups_per_dimension;
    compute_strip_plan(w, h, halo, max_blur_r, limit, max_groups)
        .ok()
        .map(|p| p.inner_rows)
}

/// The dispatches for `recipe` on a `w` x `h` strip belonging to a `frame_w` x
/// `frame_h` frame, in the CPU's order. Empty when none of the three passes has
/// anything to do. Blur radii clamp against the full frame dimensions so multi-band
/// strips match single-band and CPU results.
fn plan(w: usize, h: usize, frame_w: usize, frame_h: usize, recipe: &Recipe, scale: f32) -> Vec<Step> {
    let (wu, hu) = (w as u32, h as u32);
    let px = (wu.div_ceil(16), hu.div_ceil(16));
    let mut steps = Vec::new();
    let step = |name: &str, src: u32, dst: u32, r: u32, mode: u32, flags: u32, f: [f32; 4], groups: (u32, u32)| Step {
        entry: entry(name),
        params: words(&[wu, hu, src, dst, r, mode, flags, 0, f[0].to_bits(), f[1].to_bits(), f[2].to_bits(), f[3].to_bits()]),
        groups,
    };
    let pixels = |name: &str, mode: u32, flags: u32, f: [f32; 4]| step(name, 0, 0, 0, mode, flags, f, px);
    // fast_separable_blur's radius, clamped against full frame dimensions, through scratch plane 4.
    // Radii <= 64 run on shared-memory tiled kernels; larger radii keep the sliding-window fallback.
    let blur = |steps: &mut Vec<Step>, src: u32, dst: u32, radius: usize| {
        let r = radius.min(frame_w / 2).min(frame_h / 2).max(1) as u32;
        if r <= TILED_BLUR_MAX_R {
            steps.push(step("blur_h_tiled", src, 4, r, 0, 0, [0.0; 4], (wu.div_ceil(256), hu)));
            steps.push(step("blur_v_tiled", 4, dst, r, 0, 0, [0.0; 4], (wu.div_ceil(16), hu.div_ceil(64))));
        } else {
            steps.push(step("blur_h", src, 4, r, 0, 0, [0.0; 4], (wu.div_ceil(CHUNK).div_ceil(64), hu)));
            steps.push(step("blur_v", 4, dst, r, 0, 0, [0.0; 4], (wu.div_ceil(64), hu.div_ceil(CHUNK))));
        }
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
    if (sharpness > 0.1 || dn_lum > 0.1 || dn_col > 0.1) && frame_w >= 3 && frame_h >= 3 {
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
    if recipe.defringe > 0.01 && frame_w >= 3 && frame_h >= 3 {
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
    Ok(Some((lay, layer_dispatch(w, h, frame))))
}

fn layer_dispatch(w: usize, h: usize, frame: (usize, usize, usize, usize)) -> Step {
    let (inv_w, inv_h) = (1.0 / (frame.2 as f32).max(1.0), 1.0 / (frame.3 as f32).max(1.0));
    let (wu, hu) = (w as u32, h as u32);
    let params = words(&[wu, hu, 0, 0, frame.0 as u32, frame.1 as u32, 0, 0, inv_w.to_bits(), inv_h.to_bits()]);
    Step { entry: entry("layers"), params, groups: (wu.div_ceil(16), hu.div_ceil(16)) }
}

struct DisplayStrip {
    out_y0: usize,
    out_y1: usize,
    sy0: usize,
    sy1: usize,
}

fn build_display_strips(
    w: usize,
    h: usize,
    strip_plan: StripPlan,
    halo: usize,
    geometry: Option<&crate::pipeline::detail::Geometry>,
) -> Result<Vec<DisplayStrip>, String> {
    if strip_plan.is_single_band() {
        return Ok(vec![DisplayStrip { out_y0: 0, out_y1: h, sy0: 0, sy1: h }]);
    }
    let mut band_rows = strip_plan.band_rows;
    loop {
        let mut strips = Vec::with_capacity(h.div_ceil(band_rows));
        let mut fits = true;
        let mut worst_src_h = 0usize;
        for out_y0 in (0..h).step_by(band_rows) {
            let out_y1 = (out_y0 + band_rows).min(h);
            let (sy0, sy1) = match geometry {
                None => (out_y0.saturating_sub(halo), (out_y1 + halo).min(h)),
                Some(geom) => {
                    let mut y_min = f32::INFINITY;
                    let mut y_max = f32::NEG_INFINITY;
                    let steps = 32usize;
                    let wf = w.saturating_sub(1) as f32;
                    let top = out_y0 as f32;
                    let bot = (out_y1 - 1) as f32;
                    for k in 0..=steps {
                        let t = k as f32 / steps as f32;
                        let ex = t * wf;
                        let ey = top + t * (bot - top);
                        for (x, y) in [(ex, top), (ex, bot), (0.0, ey), (wf, ey)] {
                            let (_, sy) = geom.source(x, y);
                            if sy.is_finite() {
                                y_min = y_min.min(sy);
                                y_max = y_max.max(sy);
                            }
                        }
                    }
                    if !y_min.is_finite() || !y_max.is_finite() || y_max < 0.0 || y_min > (h - 1) as f32 {
                        (0, 1)
                    } else {
                        let pad = (halo + 2) as f32;
                        let sy0 = (y_min - pad).floor().clamp(0.0, (h - 1) as f32) as usize;
                        let sy1 = (y_max + pad).ceil().clamp((sy0 + 1) as f32, h as f32) as usize;
                        (sy0, sy1)
                    }
                }
            };
            let src_h = sy1 - sy0;
            worst_src_h = worst_src_h.max(src_h);
            if src_h > strip_plan.max_strip_rows {
                fits = false;
                break;
            }
            strips.push(DisplayStrip { out_y0, out_y1, sy0, sy1 });
        }
        if fits {
            return Ok(strips);
        }
        if geometry.is_none() || band_rows <= 16 {
            return Err(format!(
                "{w}x{h} geometry band needs {worst_src_h} source rows (GPU strip limit {})",
                strip_plan.max_strip_rows
            ));
        }
        band_rows = (band_rows / 2).max(16);
    }
}

/// The display tail after `plan`'s steps for one strip: grain, lens distortion and
/// perspective, vignette and 8-bit quantization into compact `px8[0..valid_rows*w]`,
/// packing to 3 bytes per pixel when `ch` is 3, and accumulating the histogram and
/// scopes. Mirrors `process_buffer_16_to_8_ex`'s passes 4 to 6.
#[allow(clippy::too_many_arguments)]
fn display_steps_for_strip(
    frame_w: usize,
    frame_h: usize,
    strip: &DisplayStrip,
    ch: usize,
    recipe: &Recipe,
    scale: f32,
    geometry: Option<&crate::pipeline::detail::Geometry>,
    combine_scopes: bool,
) -> Vec<Step> {
    let wu = frame_w as u32;
    let strip_h = strip.sy1 - strip.sy0;
    let valid_rows = strip.out_y1 - strip.out_y0;
    let strip_hu = strip_h as u32;
    let valid_hu = valid_rows as u32;
    let px_strip = (wu.div_ceil(16), strip_hu.div_ceil(16));
    let px_valid = (wu.div_ceil(16), valid_hu.div_ceil(16));
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
    let finish = |h_u32: u32, mode: u32, flags: u32, strip0: [u32; 4], strip1: [u32; 4], groups: (u32, u32)| {
        let mut params = words(&[wu, h_u32, 0, 0, 0, mode, flags]);
        for (k, v) in f.iter().enumerate() {
            params[8 + k] = v.to_bits();
        }
        params[32..36].copy_from_slice(&strip0);
        params[36..40].copy_from_slice(&strip1);
        Step { entry: entry("finish"), params, groups }
    };
    match geometry {
        Some(geometry) => {
            // Grain in place across the source strip, resample output band into planes,
            // then apply vignette and 8-bit quantization into compact px8.
            if grain {
                steps.push(finish(
                    strip_hu,
                    1,
                    1,
                    [frame_w as u32, frame_h as u32, strip.sy0 as u32, 0],
                    [strip_hu, strip.out_y0 as u32, 0, 0],
                    px_strip,
                ));
            }
            let (terms, perspective, lens) = geometry.gpu_terms();
            let mut params = words(&[wu, valid_hu, 0, 0, 0, 0, perspective as u32 | (lens as u32) << 1]);
            for (k, v) in terms.iter().enumerate() {
                params[16 + k] = v.to_bits();
            }
            params[32..36].copy_from_slice(&[frame_w as u32, frame_h as u32, strip.sy0 as u32, strip_hu]);
            params[36..40].copy_from_slice(&[valid_hu, strip.out_y0 as u32, 0, 0]);
            steps.push(Step { entry: entry("geometry"), params, groups: px_valid });
            steps.push(finish(
                valid_hu,
                2,
                (vignette as u32) << 1,
                [frame_w as u32, frame_h as u32, strip.out_y0 as u32, 0],
                [valid_hu, strip.out_y0 as u32, 0, 0],
                px_valid,
            ));
        }
        None => {
            let valid_y0 = (strip.out_y0 - strip.sy0) as u32;
            let valid_y1 = (strip.out_y1 - strip.sy0) as u32;
            steps.push(finish(
                strip_hu,
                0,
                grain as u32 | (vignette as u32) << 1,
                [frame_w as u32, frame_h as u32, strip.sy0 as u32, valid_y0],
                [valid_y1, strip.out_y0 as u32, 0, 0],
                px_strip,
            ));
        }
    }
    if ch == 3 {
        let groups = ((frame_w * valid_rows * 3).div_ceil(4) as u32).div_ceil(256);
        let gx = groups.min(32768);
        let params = words(&[wu, valid_hu, gx * 256]);
        steps.push(Step { entry: entry("pack3"), params, groups: (gx, groups.div_ceil(gx)) });
    }
    if combine_scopes {
        steps.push(Step { entry: HIST_ALL_ENTRY, params: words(&[wu, valid_hu]), groups: (HIST_GROUPS, 1) });
    } else {
        for name in ["hist_main", "hist_wf", "hist_par"] {
            steps.push(Step { entry: entry(name), params: words(&[wu, valid_hu]), groups: (HIST_GROUPS, 1) });
        }
    }
    steps
}

pub struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipelines: Vec<wgpu::ComputePipeline>,
    hist_all: Option<wgpu::ComputePipeline>,
    /// Kept between calls; previews call this on every edit.
    buffers: std::sync::Mutex<Option<Buffers>>,
}

struct TimestampQueries {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    staging: wgpu::Buffer,
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
    /// Constant 768-float luma lookup table in uniform memory (uploaded once per pool allocation).
    luma_terms: wgpu::Buffer,
    /// Minimal 4-byte placeholders bound to display slots when `Out::F32` runs alone.
    _dummy_px8: wgpu::Buffer,
    _dummy_out8: wgpu::Buffer,
    _dummy_hist: wgpu::Buffer,
    /// Allocated lazily only for `Out::F32` readbacks (`rgb_size`, 12 B/px).
    staging_f32: Option<wgpu::Buffer>,
    /// Optional GPU pass timestamp queries when `Features::TIMESTAMP_QUERY` is available.
    timestamps: Option<TimestampQueries>,
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

#[allow(clippy::too_many_arguments)]
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
    luma_terms: &wgpu::Buffer,
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
            wgpu::BindGroupEntry { binding: 7, resource: luma_terms.as_entire_binding() },
        ],
    })
}

impl ComputeBuffers {
    fn new(gpu: &Gpu, layout: &wgpu::BindGroupLayout, params_size: u64, rgb_size: u64, plane_size: u64, layers_size: u64) -> Self {
        let device = &gpu.device;
        let params = create_buf(device, "local params", params_size, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let rgb = create_buf(device, "local rgb", rgb_size, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC);
        let planes = create_buf(device, "local planes", plane_size * PLANES, wgpu::BufferUsages::STORAGE);
        let layers = create_buf(device, "local layers", layers_size, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let luma_terms = create_buf(device, "local luma terms", LUMA_TERMS_BYTES, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let terms: Vec<f32> = [0.2126f32, 0.7152, 0.0722]
            .iter()
            .flat_map(|k| (0..256).map(move |v| k * v as f32))
            .collect();
        gpu.queue.write_buffer(&luma_terms, 0, as_bytes(&terms));

        let dummy_px8 = create_buf(device, "local dummy px8", 4, wgpu::BufferUsages::STORAGE);
        let dummy_out8 = create_buf(device, "local dummy out8", 4, wgpu::BufferUsages::STORAGE);
        let dummy_hist = create_buf(device, "local dummy hist", 4, wgpu::BufferUsages::STORAGE);
        let timestamps = gpu.features.contains(wgpu::Features::TIMESTAMP_QUERY).then(|| TimestampQueries {
            query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("local timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: create_buf(device, "local ts resolve", 16, wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            staging: create_buf(device, "local ts staging", 16, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
        });
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
            &luma_terms,
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
            luma_terms,
            _dummy_px8: dummy_px8,
            _dummy_out8: dummy_out8,
            _dummy_hist: dummy_hist,
            staging_f32: None,
            timestamps,
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
            .saturating_add(LUMA_TERMS_BYTES)
            .saturating_add(staging)
    }
}

impl DisplayBuffers {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, compute: &ComputeBuffers, pixel_cap: u64) -> Self {
        let px8_bytes = pixel_cap.saturating_mul(4);
        let out8_bytes = pixel_cap.saturating_mul(3).next_multiple_of(4);
        let px8 = create_buf(device, "local px8", px8_bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let out8 = create_buf(device, "local out8", out8_bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let hist_size = (crate::pipeline::histogram::SCOPE_WORDS * 4) as u64;
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
            &compute.luma_terms,
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
    fn new(gpu: &Gpu, layout: &wgpu::BindGroupLayout, params_size: u64, rgb_size: u64, plane_size: u64, layers_size: u64) -> Self {
        Self {
            compute: ComputeBuffers::new(gpu, layout, params_size, rgb_size, plane_size, layers_size),
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
    fn new(device: &wgpu::Device, cache: Option<&wgpu::PipelineCache>, combine_scopes: bool) -> Self {
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
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(LUMA_TERMS_BYTES),
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
                    cache,
                })
            })
            .collect();
        let hist_all = combine_scopes.then(|| {
            let hist_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("local hist_all"),
                source: wgpu::ShaderSource::Wgsl(HIST_ALL_WGSL.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("hist_all"),
                layout: Some(&pipeline_layout),
                module: &hist_module,
                entry_point: Some("hist_all"),
                compilation_options: Default::default(),
                cache,
            })
        });
        Self { layout, pipelines, hist_all, buffers: std::sync::Mutex::new(None) }
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
            let combine_scopes = gpu.limits.max_compute_workgroup_storage_size >= COMBINED_HIST_WG_STORAGE;
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let p = Pipeline::new(&gpu.device, gpu.pipeline_cache.as_ref(), combine_scopes);
            match pollster::block_on(scope.pop()) {
                Some(e) => Err(format!("shader: {e}")),
                None => {
                    gpu.persist_pipeline_cache();
                    Ok(p)
                }
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
    run_on_with_limit(gpu, buf, w, h, recipe, scale, gpu_strip_limit(gpu))
}

fn run_on_with_limit(
    gpu: &Gpu,
    buf: &mut [f32],
    w: usize,
    h: usize,
    recipe: &Recipe,
    scale: f32,
    limit: u64,
) -> Result<(), String> {
    if w == 0 || h == 0 || buf.len() < w * h * 3 {
        return Err("unsupported buffer".to_string());
    }
    let probe = plan(w, h, w, h, recipe, scale);
    if probe.is_empty() {
        return Ok(());
    }
    let limit = limit.min(gpu_strip_limit(gpu));
    let (halo, max_blur_r) = effective_halo_and_blur(recipe, scale, w, h);
    let strip_plan = compute_strip_plan(w, h, halo, max_blur_r, limit, gpu.limits.max_compute_workgroups_per_dimension)?;
    let _job = gpu.lock_job(crate::pipeline::interactive());
    if strip_plan.is_single_band() {
        let bands = vec![BandJob {
            strip_h: h,
            valid_y0: 0,
            out_y0: 0,
            out_y1: h,
            steps: probe,
            src: BandSrc::InPlaceF32,
        }];
        return execute_bands(gpu, w, bands, &[], Out::F32(&mut buf[..w * h * 3]), limit);
    }
    // Multi-band path keeps `buf` untouched until every band succeeds, preserving
    // the fallback contract on mid-frame GPU failure.
    let src_frame = &buf[..w * h * 3];
    let mut bands = Vec::with_capacity(strip_plan.bands);
    for out_y0 in (0..h).step_by(strip_plan.band_rows) {
        let out_y1 = (out_y0 + strip_plan.band_rows).min(h);
        let sy0 = out_y0.saturating_sub(strip_plan.halo);
        let sy1 = (out_y1 + strip_plan.halo).min(h);
        let strip_h = sy1 - sy0;
        bands.push(BandJob {
            strip_h,
            valid_y0: out_y0 - sy0,
            out_y0,
            out_y1,
            steps: plan(w, strip_h, w, h, recipe, scale),
            src: BandSrc::HostF32(&src_frame[sy0 * w * 3..sy1 * w * 3]),
        });
    }
    let mut dst = vec![0.0f32; w * h * 3];
    execute_bands(gpu, w, bands, &[], Out::F32(&mut dst), limit)?;
    buf[..w * h * 3].copy_from_slice(&dst);
    Ok(())
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
    chained_on_with_limit(gpu, input, w, ch, y0, y1, recipe, inputs, scale, frame, gpu_strip_limit(gpu))
}

#[allow(clippy::too_many_arguments)]
fn chained_on_with_limit(
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
    limit: u64,
) -> Result<Option<Vec<f32>>, String> {
    let h = y1.saturating_sub(y0);
    if w == 0 || h == 0 || !(3..=4).contains(&ch) || input.len() < y1 * w * ch {
        return Err("unsupported buffer".to_string());
    }
    let (lay, has_layers) = match layer_step(w, h, frame, recipe)? {
        Some((lay, _)) => (lay, true),
        None => (Vec::new(), false),
    };
    let probe = plan(w, h, w, h, recipe, scale);
    if !has_layers && probe.is_empty() {
        return Ok(None);
    }
    let limit = limit.min(gpu_strip_limit(gpu));
    let (halo, max_blur_r) = effective_halo_and_blur(recipe, scale, w, h);
    let strip_plan = compute_strip_plan(w, h, halo, max_blur_r, limit, gpu.limits.max_compute_workgroups_per_dimension)?;
    let _job = gpu.lock_job(crate::pipeline::interactive());
    let mut out = vec![0.0f32; w * h * 3];
    let bands = if strip_plan.is_single_band() {
        let mut steps = Vec::with_capacity(probe.len() + usize::from(has_layers));
        if has_layers {
            steps.push(layer_dispatch(w, h, frame));
        }
        steps.extend(probe);
        vec![BandJob {
            strip_h: h,
            valid_y0: 0,
            out_y0: 0,
            out_y1: h,
            steps,
            src: BandSrc::PointOps { input, ch, sy0: y0, sy1: y1, recipe, inputs },
        }]
    } else {
        let mut bands = Vec::with_capacity(strip_plan.bands);
        for out_y0 in (0..h).step_by(strip_plan.band_rows) {
            let out_y1 = (out_y0 + strip_plan.band_rows).min(h);
            let sy0 = out_y0.saturating_sub(strip_plan.halo);
            let sy1 = (out_y1 + strip_plan.halo).min(h);
            let strip_h = sy1 - sy0;
            let mut steps = Vec::new();
            if has_layers {
                steps.push(layer_dispatch(w, strip_h, (frame.0, frame.1 + sy0, frame.2, frame.3)));
            }
            steps.extend(plan(w, strip_h, w, h, recipe, scale));
            bands.push(BandJob {
                strip_h,
                valid_y0: out_y0 - sy0,
                out_y0,
                out_y1,
                steps,
                src: BandSrc::PointOps {
                    input,
                    ch,
                    sy0: y0 + sy0,
                    sy1: y0 + sy1,
                    recipe,
                    inputs,
                },
            });
        }
        bands
    };
    execute_bands(gpu, w, bands, &lay, Out::F32(&mut out), limit)?;
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

/// Runs the full display pipeline on `gpu` for the one-time integrated-GPU micro-benchmark.
pub(super) fn display_on_for_bench(
    gpu: &Gpu,
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    transfer: crate::pipeline::transfer::Transfer,
) -> Result<(), String> {
    let ops = crate::pipeline::PointOps::new(recipe);
    let (wr, wg, wb) = crate::pipeline::white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
    let e = 2.0f32.powf(recipe.exposure + recipe.baseline_exposure);
    let inputs = ops.gpu_inputs([wr * e, wg * e, wb * e], transfer);
    let scale = crate::pipeline::frame_scale(w, h);
    display_on(gpu, input, w, h, ch, recipe, &inputs, scale).map(|_| ())
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
    display_on_with_limit(gpu, input, w, h, ch, recipe, inputs, scale, gpu_strip_limit(gpu))
}

#[allow(clippy::too_many_arguments)]
fn display_on_with_limit(
    gpu: &Gpu,
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
    limit: u64,
) -> Result<(Vec<u8>, crate::pipeline::histogram::HistogramData), String> {
    display_on_with_options(gpu, input, w, h, ch, recipe, inputs, scale, limit, None)
}

#[allow(clippy::too_many_arguments)]
fn display_on_with_options(
    gpu: &Gpu,
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
    limit: u64,
    combine_scopes_override: Option<bool>,
) -> Result<(Vec<u8>, crate::pipeline::histogram::HistogramData), String> {
    if w == 0 || h == 0 || !(3..=4).contains(&ch) || input.len() < w * h * ch {
        return Err("unsupported buffer".to_string());
    }
    let (lay, has_layers) = match layer_step(w, h, (0, 0, w, h), recipe)? {
        Some((lay, _)) => (lay, true),
        None => (Vec::new(), false),
    };
    let limit = limit.min(gpu_strip_limit(gpu));
    let (halo, max_blur_r) = effective_halo_and_blur(recipe, scale, w, h);
    let strip_plan = compute_strip_plan(w, h, halo, max_blur_r, limit, gpu.limits.max_compute_workgroups_per_dimension)?;
    let geometry = crate::pipeline::detail::Geometry::new(w, h, recipe);
    let strips = build_display_strips(w, h, strip_plan, halo, geometry.as_ref())?;
    let combine_scopes = combine_scopes_override
        .unwrap_or(gpu.limits.max_compute_workgroup_storage_size >= COMBINED_HIST_WG_STORAGE);
    let mut bands = Vec::with_capacity(strips.len());
    for strip in strips {
        let strip_h = strip.sy1 - strip.sy0;
        let mut steps = Vec::new();
        if has_layers {
            steps.push(layer_dispatch(w, strip_h, (0, strip.sy0, w, h)));
        }
        steps.extend(plan(w, strip_h, w, h, recipe, scale));
        steps.extend(display_steps_for_strip(w, h, &strip, ch, recipe, scale, geometry.as_ref(), combine_scopes));
        bands.push(BandJob {
            strip_h,
            valid_y0: strip.out_y0.saturating_sub(strip.sy0),
            out_y0: strip.out_y0,
            out_y1: strip.out_y1,
            steps,
            src: BandSrc::PointOps {
                input,
                ch,
                sy0: strip.sy0,
                sy1: strip.sy1,
                recipe,
                inputs,
            },
        });
    }
    let _job = gpu.lock_job(true);
    let (mut pixels, mut words) = (Vec::new(), Vec::new());
    execute_bands(
        gpu,
        w,
        bands,
        &lay,
        Out::Display { ch, pixels: &mut pixels, words: &mut words },
        limit,
    )?;
    if ch == 4 {
        for (px, src) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(input.as_chunks::<4>().0) {
            px[3] = (src[3] >> 8) as u8;
        }
    }
    let hist = crate::pipeline::histogram::histogram_from_scope_words(&words, w * h);
    Ok((pixels, hist))
}

/// Zero-copy GPU-to-GPU Vulkan DMA-BUF display path (`WP-15`): runs the entire single-band
/// preview pipeline on the GPU, writes directly into a double-buffered exportable `Rgba16Float`
/// `VkImage`, signals an exportable `SYNC_FD` semaphore, and reads back ONLY the tiny scope/
/// histogram counts (zero pixel readback over PCIe). Returns `None` on unsupported GPUs,
/// multi-band frames, or any runtime error so the daemon falls back transparently to `memfd`.
#[allow(clippy::too_many_arguments)]
pub fn run_display_dmabuf(
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
    color_space: crate::preview_shm::ColorSpaceTag,
) -> Option<super::dmabuf::ExportedDmaBufFrame> {
    if ch != 3 || w == 0 || h == 0 || w * h < super::point_ops::MIN_PIXELS {
        return None;
    }
    let gpu = super::acquire().filter(|g| {
        super::stages_worthwhile(g)
            && g.dmabuf_caps.is_some_and(super::dmabuf::DmaBufCaps::is_supported)
    })?;
    match display_dmabuf_on(&gpu, input, w, h, ch, recipe, inputs, scale, color_space) {
        Ok(frame) => Some(frame),
        Err(e) => {
            if super::profiling_enabled() {
                eprintln!("omastudio: DMA-BUF preview unavailable, falling back to memfd: {e}");
            }
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn display_dmabuf_on(
    gpu: &Gpu,
    input: &[u16],
    w: usize,
    h: usize,
    ch: usize,
    recipe: &Recipe,
    inputs: &super::point_ops::PointInputs,
    scale: f32,
    color_space: crate::preview_shm::ColorSpaceTag,
) -> Result<super::dmabuf::ExportedDmaBufFrame, String> {
    if input.len() < w * h * ch {
        return Err("unsupported buffer".to_string());
    }
    let (lay, has_layers) = match layer_step(w, h, (0, 0, w, h), recipe)? {
        Some((lay, _)) => (lay, true),
        None => (Vec::new(), false),
    };
    let limit = gpu_strip_limit(gpu);
    let (halo, max_blur_r) = effective_halo_and_blur(recipe, scale, w, h);
    let strip_plan = compute_strip_plan(w, h, halo, max_blur_r, limit, gpu.limits.max_compute_workgroups_per_dimension)?;
    if !strip_plan.is_single_band() {
        return Err("multi-band frame uses memfd fallback".to_string());
    }
    let geometry = crate::pipeline::detail::Geometry::new(w, h, recipe);
    let strips = build_display_strips(w, h, strip_plan, halo, geometry.as_ref())?;
    if strips.len() != 1 {
        return Err("multi-strip frame uses memfd fallback".to_string());
    }
    let strip = &strips[0];
    let combine_scopes = gpu.limits.max_compute_workgroup_storage_size >= COMBINED_HIST_WG_STORAGE;
    let mut steps = Vec::new();
    if has_layers {
        steps.push(layer_dispatch(w, h, (0, 0, w, h)));
    }
    steps.extend(plan(w, h, w, h, recipe, scale));
    steps.extend(display_steps_for_strip(w, h, strip, 4, recipe, scale, geometry.as_ref(), combine_scopes));

    let max_groups = gpu.limits.max_compute_workgroups_per_dimension;
    for s in &steps {
        if s.groups.0 > max_groups || s.groups.1 > max_groups {
            return Err("frame exceeds compute workgroup limit".to_string());
        }
    }

    let pixels_u64 = (w * h) as u64;
    let (rgb_size, plane_size) = (pixels_u64 * 12, pixels_u64 * 4);
    if rgb_size > limit || plane_size * PLANES > limit || plane_size * PLANES > u32::MAX as u64 {
        return Err("frame exceeds GPU buffer limits".to_string());
    }
    let layers_size = (lay.len() * 4) as u64;
    if layers_size > limit {
        return Err("masks exceed GPU buffer limits".to_string());
    }

    let pipeline = pipeline(gpu)?;
    let pool_mutex = gpu
        .dmabuf_pool
        .get_or_init(|| super::dmabuf::DmaBufPool::new(gpu).map(std::sync::Mutex::new))
        .as_ref()
        .map_err(|e| e.clone())?;

    let _job = gpu.lock_job(true);
    let stride = PARAMS_SIZE.next_multiple_of(gpu.limits.min_uniform_buffer_offset_alignment as u64);
    let params_size = stride * steps.len() as u64;
    let device = &gpu.device;
    let timeout = gpu.poll_timeout(true);
    let mut cache = pipeline.buffers.lock().map_err(|_| "GPU buffer lock poisoned")?;
    let mut pool = pool_mutex.lock().map_err(|_| "DMA-BUF pool lock poisoned")?;

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let result = (|| -> Result<super::dmabuf::ExportedDmaBufFrame, String> {
        if !cache.as_ref().is_some_and(|b| b.compute.fits(params_size, rgb_size, plane_size, layers_size)) {
            let (p, r, l, m) = cache.as_ref().map_or((0, 0, 0, 0), |b| {
                (b.compute.params_size, b.compute.rgb_size, b.compute.plane_size, b.compute.layers_size)
            });
            *cache = None;
            *cache = Some(Buffers::new(
                gpu,
                &pipeline.layout,
                params_size.max(p).max(stride * 64),
                rgb_size.max(r),
                plane_size.max(l),
                layers_size.max(m).max(256),
            ));
        }
        let bufs = cache.as_mut().expect("buffers");
        if !bufs.display.as_ref().is_some_and(|d| d.pixel_cap >= pixels_u64) {
            let cap = bufs.display.as_ref().map_or(0, |d| d.pixel_cap).max(pixels_u64);
            bufs.display = None;
            bufs.display = Some(DisplayBuffers::new(device, &pipeline.layout, &bufs.compute, cap));
        }
        let bufs = cache.as_ref().expect("buffers");
        if !lay.is_empty() {
            gpu.queue.write_buffer(&bufs.compute.layers, 0, as_bytes(&lay));
        }

        super::point_ops::run_into(gpu, input, w, ch, 0, h, recipe, inputs, &bufs.compute.rgb)?;

        let mut params_bytes = vec![0u8; params_size as usize];
        for (i, s) in steps.iter().enumerate() {
            let at = i * stride as usize;
            params_bytes[at..at + PARAMS_SIZE as usize].copy_from_slice(as_bytes(&s.params));
        }
        if !params_bytes.is_empty() {
            gpu.queue.write_buffer(&bufs.compute.params, 0, &params_bytes);
        }

        let disp = bufs.display.as_ref().expect("display buffers");
        let hist_len = (crate::pipeline::histogram::SCOPE_WORDS * 4) as u64;
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("local dmabuf") });
        enc.clear_buffer(&disp.hist, 0, Some(hist_len));
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("local dmabuf"),
                timestamp_writes: None,
            });
            for (i, s) in steps.iter().enumerate() {
                let step_pipeline = if s.entry == HIST_ALL_ENTRY {
                    pipeline.hist_all.as_ref().expect("hist_all pipeline")
                } else {
                    &pipeline.pipelines[s.entry]
                };
                pass.set_pipeline(step_pipeline);
                pass.set_bind_group(0, &disp.bind_display, &[(i as u64 * stride) as u32]);
                pass.dispatch_workgroups(s.groups.0, s.groups.1, 1);
            }
        }
        // Copy ONLY the tiny scope/histogram buffer — zero full-frame pixel readback.
        enc.copy_buffer_to_buffer(&disp.hist, 0, &disp.staging_hist, 0, hist_len);

        let (idx, desc, dmabuf_fd, sync_fd) =
            pool.encode_submit_and_export(gpu, enc, &disp.px8, w as u32, h as u32, color_space)?;

        let (tx, rx) = mpsc::channel();
        disp.staging_hist.map_async(wgpu::MapMode::Read, 0..hist_len, move |r| {
            let _ = tx.send(r);
        });
        let mut hist_guard = super::StagingGuard::new(&disp.staging_hist);
        gpu.wait_submission(Some(idx), timeout)?;
        let hist_map = rx.recv().map_err(|e| e.to_string())?;
        if hist_map.is_err() {
            hist_guard.disarm();
        }
        hist_map.map_err(|e| e.to_string())?;
        let words: Vec<u32> = {
            let hview = disp.staging_hist.get_mapped_range(0..hist_len).map_err(|e| e.to_string())?;
            hview
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_ne_bytes(*c))
                .collect()
        };
        hist_guard.unmap_now();

        let hist = crate::pipeline::histogram::histogram_from_scope_words(&words, w * h);
        Ok(super::dmabuf::ExportedDmaBufFrame {
            desc,
            dmabuf_fd,
            sync_fd,
            histogram: hist,
        })
    })();

    let scope_res = super::drain_scopes(oom, scope);
    match (result, scope_res) {
        (Ok(frame), Ok(())) => {
            if let Some(b) = cache.as_mut() {
                b.last_used = std::time::Instant::now();
            }
            Ok(frame)
        }
        (Err(e), Ok(())) => {
            *cache = None;
            Err(e)
        }
        (Ok(_), Err(e)) | (Err(_), Err(e)) => {
            *cache = None;
            Err(e)
        }
    }
}

/// Where [`execute_bands`] puts its result.
enum Out<'a> {
    /// RGB f32, which is also uploaded when a single-band job uses [`BandSrc::InPlaceF32`].
    F32(&'a mut [f32]),
    /// The display tail's 8-bit pixels (`ch` bytes each, alpha left at 0) and the
    /// histogram and scope words accumulated across all bands.
    Display { ch: usize, pixels: &'a mut Vec<u8>, words: &'a mut Vec<u32> },
}

#[cfg(test)]
type CustomFill<'a> = Box<dyn FnOnce(&wgpu::Buffer) -> Result<(), String> + 'a>;

enum BandSrc<'a> {
    /// Single-band in-place upload from the output slice itself.
    InPlaceF32,
    /// Immutable host RGB f32 slice covering this strip's rows.
    HostF32(&'a [f32]),
    /// Fill `bufs.compute.rgb` directly on the GPU via `point_ops::run_into`.
    PointOps {
        input: &'a [u16],
        ch: usize,
        sy0: usize,
        sy1: usize,
        recipe: &'a Recipe,
        inputs: &'a super::point_ops::PointInputs<'a>,
    },
    #[cfg(test)]
    Custom(CustomFill<'a>),
}

struct BandJob<'a> {
    strip_h: usize,
    valid_y0: usize,
    out_y0: usize,
    out_y1: usize,
    steps: Vec<Step>,
    src: BandSrc<'a>,
}

/// Executes one or more row bands against the shared local buffer pool. Buffers are
/// sized once to the peak strip height across all bands; for `Out::Display`, `hist`
/// is cleared before band 0, accumulated across bands, and read back once after the
/// final band. The caller holds the GPU's job lock.
fn execute_bands(
    gpu: &Gpu,
    w: usize,
    bands: Vec<BandJob<'_>>,
    lay: &[f32],
    mut out: Out<'_>,
    limit: u64,
) -> Result<(), String> {
    if bands.is_empty() {
        return Ok(());
    }
    let limit = limit.min(gpu_strip_limit(gpu));
    let max_groups = gpu.limits.max_compute_workgroups_per_dimension;
    let mut peak_strip_rows = 0usize;
    let mut peak_valid_rows = 0usize;
    let mut total_valid_rows = 0usize;
    let mut peak_steps = 0usize;
    let mut needs_hist_all = false;
    for b in &bands {
        let valid_rows = b.out_y1.saturating_sub(b.out_y0);
        peak_strip_rows = peak_strip_rows.max(b.strip_h).max(valid_rows);
        peak_valid_rows = peak_valid_rows.max(valid_rows);
        total_valid_rows = total_valid_rows.saturating_add(valid_rows);
        peak_steps = peak_steps.max(b.steps.len());
        for s in &b.steps {
            if s.entry == HIST_ALL_ENTRY {
                needs_hist_all = true;
            }
            if s.groups.0 > max_groups || s.groups.1 > max_groups {
                return Err(format!("{w}x{} needs more workgroups than the GPU allows", b.strip_h));
            }
        }
    }
    let peak_pixels_u64 = (w * peak_strip_rows) as u64;
    let peak_valid_u64 = (w * peak_valid_rows) as u64;
    let (rgb_size, plane_size) = (peak_pixels_u64 * 12, peak_pixels_u64 * 4);
    if rgb_size > limit || plane_size * PLANES > limit || plane_size * PLANES > u32::MAX as u64 {
        return Err(format!("{w}x{peak_strip_rows} exceeds the GPU's buffer limits"));
    }
    let layers_size = (lay.len() * 4) as u64;
    if layers_size > gpu_strip_limit(gpu) {
        return Err("the masks exceed the GPU's buffer limits".to_string());
    }
    let pipeline = pipeline(gpu)?;
    if needs_hist_all && pipeline.hist_all.is_none() {
        return Err("combined histogram pipeline unavailable on this adapter".to_string());
    }

    let stride = PARAMS_SIZE.next_multiple_of(gpu.limits.min_uniform_buffer_offset_alignment as u64);
    let params_size = stride * peak_steps as u64;

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
                gpu,
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
                    && bufs.total_bytes().saturating_add(peak_valid_u64 * 11) > super::PREVIEW_VRAM_CAP
                {
                    bufs.compute.staging_f32 = None;
                }
                if !bufs.display.as_ref().is_some_and(|d| d.pixel_cap >= peak_valid_u64) {
                    let cap = bufs.display.as_ref().map_or(0, |d| d.pixel_cap).max(peak_valid_u64);
                    bufs.display = None;
                    bufs.display = Some(DisplayBuffers::new(device, &pipeline.layout, &bufs.compute, cap));
                }
            }
        }
        let bufs = cache.as_ref().expect("buffers");
        if !lay.is_empty() {
            gpu.queue.write_buffer(&bufs.compute.layers, 0, as_bytes(lay));
        }
        let hist_len = (crate::pipeline::histogram::SCOPE_WORDS * 4) as u64;
        if let Out::Display { ch, pixels, .. } = &mut out {
            pixels.clear();
            pixels.reserve(total_valid_rows * w * *ch);
        }

        let profile = super::profiling_enabled();
        let ts_active = profile.then_some(bufs.compute.timestamps.as_ref()).flatten();
        let mut params_bytes = vec![0u8; params_size as usize];
        let band_count = bands.len();
        for (band_idx, band) in bands.into_iter().enumerate() {
            let is_first = band_idx == 0;
            let is_last = band_idx + 1 == band_count;
            let valid_rows = band.out_y1 - band.out_y0;
            match band.src {
                BandSrc::InPlaceF32 => match &out {
                    Out::F32(data) => gpu.queue.write_buffer(&bufs.compute.rgb, 0, as_bytes(&data[..w * band.strip_h * 3])),
                    Out::Display { .. } => return Err("the display tail needs a fill".to_string()),
                },
                BandSrc::HostF32(slice) => gpu.queue.write_buffer(&bufs.compute.rgb, 0, as_bytes(slice)),
                BandSrc::PointOps { input, ch, sy0, sy1, recipe, inputs } => {
                    super::point_ops::run_into(gpu, input, w, ch, sy0, sy1, recipe, inputs, &bufs.compute.rgb)?;
                }
                #[cfg(test)]
                BandSrc::Custom(f) => f(&bufs.compute.rgb)?,
            }
            let step_bytes = (stride * band.steps.len() as u64) as usize;
            if step_bytes > 0 {
                params_bytes[..step_bytes].fill(0);
                for (i, s) in band.steps.iter().enumerate() {
                    let at = i * stride as usize;
                    params_bytes[at..at + PARAMS_SIZE as usize].copy_from_slice(as_bytes(&s.params));
                }
                gpu.queue.write_buffer(&bufs.compute.params, 0, &params_bytes[..step_bytes]);
            }

            let (bind, read_buf, src_offset, staging_buf, read_len, disp_hist) = match &out {
                Out::F32(_) => (
                    &bufs.compute.bind_compute,
                    &bufs.compute.rgb,
                    (band.valid_y0 * w * 12) as u64,
                    bufs.compute.staging_f32.as_ref().expect("staging_f32"),
                    (valid_rows * w * 12) as u64,
                    None,
                ),
                Out::Display { ch, .. } => {
                    let disp = bufs.display.as_ref().expect("display buffers");
                    let (src, len) = if *ch == 3 {
                        (&disp.out8, ((w * valid_rows * 3).div_ceil(4) * 4) as u64)
                    } else {
                        (&disp.px8, (w * valid_rows * 4) as u64)
                    };
                    (&disp.bind_display, src, 0u64, &disp.staging_display, len, Some((&disp.hist, &disp.staging_hist)))
                }
            };

            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("local") });
            if is_first
                && let Some((hist_buf, _)) = disp_hist
            {
                enc.clear_buffer(hist_buf, 0, Some(hist_len));
            }
            {
                let timestamp_writes = ts_active.map(|ts| wgpu::ComputePassTimestampWrites {
                    query_set: &ts.query_set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                });
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("local"),
                    timestamp_writes,
                });
                for (i, s) in band.steps.iter().enumerate() {
                    let step_pipeline = if s.entry == HIST_ALL_ENTRY {
                        pipeline.hist_all.as_ref().expect("hist_all pipeline")
                    } else {
                        &pipeline.pipelines[s.entry]
                    };
                    pass.set_pipeline(step_pipeline);
                    pass.set_bind_group(0, bind, &[(i as u64 * stride) as u32]);
                    pass.dispatch_workgroups(s.groups.0, s.groups.1, 1);
                }
            }
            if let Some(ts) = ts_active {
                enc.resolve_query_set(&ts.query_set, 0..2, &ts.resolve, 0);
                enc.copy_buffer_to_buffer(&ts.resolve, 0, &ts.staging, 0, 16);
            }
            enc.copy_buffer_to_buffer(read_buf, src_offset, staging_buf, 0, read_len);
            let read_hist = if is_last { disp_hist } else { None };
            if let Some((hist_buf, staging_hist)) = read_hist {
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
            let mut hist_state = read_hist.map(|(_, staging_hist)| {
                let rx_hist = map(staging_hist, hist_len);
                (staging_hist, rx_hist, super::StagingGuard::new(staging_hist))
            });
            let mut ts_state = ts_active.map(|ts| {
                let rx_ts = map(&ts.staging, 16);
                (&ts.staging, rx_ts, super::StagingGuard::new(&ts.staging))
            });

            gpu.wait_submission(Some(idx), timeout)?;

            let main_map = rx.recv().map_err(|e| e.to_string())?;
            let hist_map = match hist_state.as_ref() {
                Some((_, rx_hist, _)) => Some(rx_hist.recv().map_err(|e| e.to_string())?),
                None => None,
            };
            let ts_map = match ts_state.as_ref() {
                Some((_, rx_ts, _)) => Some(rx_ts.recv().map_err(|e| e.to_string())?),
                None => None,
            };
            if main_map.is_err() {
                staging_guard.disarm();
            }
            if hist_map.as_ref().is_some_and(Result::is_err)
                && let Some((_, _, ref mut hg)) = hist_state
            {
                hg.disarm();
            }
            if ts_map.as_ref().is_some_and(Result::is_err)
                && let Some((_, _, ref mut tg)) = ts_state
            {
                tg.disarm();
            }
            main_map.map_err(|e| e.to_string())?;
            if let Some(res) = hist_map {
                res.map_err(|e| e.to_string())?;
            }
            if let Some(res) = ts_map {
                res.map_err(|e| e.to_string())?;
            }

            // Scope mapped views strictly inside this block so they drop before unmap().
            {
                let view = staging_buf.get_mapped_range(0..read_len).map_err(|e| e.to_string())?;
                match &mut out {
                    Out::F32(data) => {
                        super::point_ops::as_bytes_mut(&mut data[band.out_y0 * w * 3..band.out_y1 * w * 3])
                            .copy_from_slice(&view);
                    }
                    Out::Display { ch, pixels, words } => {
                        pixels.extend_from_slice(&view[..w * valid_rows * *ch]);
                        if let Some((staging_hist, _, _)) = hist_state.as_ref() {
                            let hist = staging_hist.get_mapped_range(0..hist_len).map_err(|e| e.to_string())?;
                            words.clear();
                            words.extend(hist.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)));
                        }
                    }
                }
                if let Some((ts_staging, _, _)) = ts_state.as_ref() {
                    let ts_view = ts_staging.get_mapped_range(0..16).map_err(|e| e.to_string())?;
                    if let (Ok(b0), Ok(b1)) = (ts_view[0..8].try_into(), ts_view[8..16].try_into()) {
                        let t0 = u64::from_le_bytes(b0);
                        let t1 = u64::from_le_bytes(b1);
                        let period_ns = gpu.queue.get_timestamp_period() as f64;
                        if period_ns.is_finite() && period_ns > 0.0 && t1 >= t0 {
                            let ms = ((t1 - t0) as f64) * period_ns / 1_000_000.0;
                            super::record_gpu_pass_ms(ms);
                            if std::env::var_os("OMASTUDIO_PROFILE").is_some() {
                                eprintln!("omastudio: GPU local pass band {band_idx}: {ms:.3} ms");
                            }
                        }
                    }
                }
            }
            staging_guard.unmap_now();
            if let Some((_, _, hg)) = hist_state {
                hg.unmap_now();
            }
            if let Some((_, _, tg)) = ts_state {
                tg.unmap_now();
            }
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
            let _job = gpu.lock_job(true);
            *p.buffers.lock().unwrap() = None;
        }
        let mut buf = frame(w, h);
        run_on(&gpu, &mut buf, w, h, &recipe, 1.0).expect("F32 local stage");
        {
            let _job = gpu.lock_job(true);
            let cache = p.buffers.lock().unwrap();
            let bufs = cache.as_ref().expect("F32 buffers cached");
            assert!(bufs.display.is_none(), "F32 path should not allocate DisplayBuffers");
            assert!(bufs.compute.staging_f32.is_some(), "F32 path should allocate staging_f32");
        }

        // 2. Display path must allocate 4 B/px staging_display without staging_f32.
        {
            let _job = gpu.lock_job(true);
            *p.buffers.lock().unwrap() = None;
        }
        let input = vec![32768u16; w * h * 3];
        let ops = PointOps::new(&recipe);
        let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
        let transfer = Transfer::bt709();
        let _ = display_on(&gpu, &input, w, h, 3, &recipe, &ops.gpu_inputs([wr, wg, wb], transfer), 1.0)
            .expect("Display local stage");
        {
            let _job = gpu.lock_job(true);
            let cache = p.buffers.lock().unwrap();
            let bufs = cache.as_ref().expect("Display buffers cached");
            assert!(bufs.compute.staging_f32.is_none(), "Display path should not allocate staging_f32");
            let disp = bufs.display.as_ref().expect("DisplayBuffers allocated");
            assert_eq!(disp.staging_display.size(), (w * h * 4) as u64, "Display staging must be 4 B/px, not 12 B/px");
        }

        // 3. Any error during execute_bands() must clear the buffer cache cleanly so subsequent jobs succeed.
        {
            let _job = gpu.lock_job(true);
            let steps = plan(w, h, w, h, &recipe, 1.0);
            let mut out = vec![0.0f32; w * h * 3];
            let err = execute_bands(
                &gpu,
                w,
                vec![BandJob {
                    strip_h: h,
                    valid_y0: 0,
                    out_y0: 0,
                    out_y1: h,
                    steps,
                    src: BandSrc::Custom(Box::new(|_: &wgpu::Buffer| Err("injected fill error".to_string()))),
                }],
                &[],
                Out::F32(&mut out),
                gpu_strip_limit(&gpu),
            );
            assert!(err.is_err());
            assert!(p.buffers.lock().unwrap().is_none(), "Failed execute_bands() must invalidate buffer cache");
        }
        let mut retry = frame(w, h);
        run_on(&gpu, &mut retry, w, h, &recipe, 1.0).expect("Subsequent local job must recover after error");
    }

    /// Verifies that the right-most column and bottom-most row match CPU geometry
    /// resampling (no spurious 1px black border from strict `< w - 1` bounds).
    #[test]
    fn geometry_border_matches_cpu_on_gpu() {
        use crate::pipeline::transfer::Transfer;
        use crate::pipeline::{PointOps, white_balance};
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (32usize, 24usize);
        let input = vec![32768u16; w * h * 3];
        let recipe = Recipe {
            lens_distortion: 10.0,
            sharpness: 0.0,
            denoise_col: 0.0,
            denoise_lum: 0.0,
            ..Recipe::default()
        };
        let transfer = Transfer::bt709();
        let (cpu, _) = crate::pipeline::process_buffer_16_to_8_ex(&input, w as u32, h as u32, 3, &recipe, transfer, false, false);
        let ops = PointOps::new(&recipe);
        let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
        let (gpu_out, _) = display_on(&gpu, &input, w, h, 3, &recipe, &ops.gpu_inputs([wr, wg, wb], transfer), 1.0)
            .expect("GPU display geometry");
        for y in 0..h {
            for x in [0, w - 1] {
                let idx = (y * w + x) * 3;
                for c in 0..3 {
                    assert!(
                        cpu[idx + c].abs_diff(gpu_out[idx + c]) <= 2,
                        "border ({x},{y}) c={c}: cpu={} gpu={}",
                        cpu[idx + c],
                        gpu_out[idx + c]
                    );
                }
            }
        }
        for x in 0..w {
            let idx = ((h - 1) * w + x) * 3;
            for c in 0..3 {
                assert!(
                    cpu[idx + c].abs_diff(gpu_out[idx + c]) <= 2,
                    "bottom ({x},{}) c={c}: cpu={} gpu={}",
                    h - 1,
                    cpu[idx + c],
                    gpu_out[idx + c]
                );
            }
        }
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
        assert!(plan(400, 300, 400, 300, &r, 1.0).is_empty());
    }

    #[test]
    fn required_halo_and_150mp_128mib_strip_plan() {
        let neutral = Recipe { sharpness: 0.0, denoise_col: 0.0, denoise_lum: 0.0, defringe: 0.0, ..Recipe::default() };
        assert_eq!(required_halo(&neutral, 1.0), 0);
        assert_eq!(required_halo(&neutral, 3.7), 0);

        let sharp_only = Recipe { sharpness: 40.0, denoise_col: 0.0, denoise_lum: 0.0, defringe: 0.0, ..Recipe::default() };
        assert_eq!(required_halo(&sharp_only, 1.0), 3);
        assert_eq!(required_halo(&sharp_only, 3.7), 3);

        // 150 MP medium format (14204 x 10652) worst-case neighbourhood recipe on a 128 MiB binding adapter
        let worst_recipe = Recipe {
            clarity: 35.0,
            texture: 25.0,
            dehaze: 20.0,
            midtone_detail: 15.0,
            sharpness: 50.0,
            denoise_lum: 30.0,
            denoise_col: 60.0,
            defringe: 45.0,
            ..Recipe::default()
        };
        let (w150, h150) = (14_204usize, 10_652usize);
        let scale150 = crate::pipeline::frame_scale(w150, h150);
        let halo150 = required_halo(&worst_recipe, scale150);
        assert_eq!(halo150, 76, "150 MP worst-case halo should be 76 rows");
        let (eff_halo, max_blur) = effective_halo_and_blur(&worst_recipe, scale150, w150, h150);
        assert_eq!(eff_halo, 76);
        assert_eq!(max_blur, 59);

        let plan_128 = compute_strip_plan(w150, h150, eff_halo, max_blur, 128 << 20, 65_535)
            .expect("150 MP must fit on 128 MiB binding adapter");
        assert!(!plan_128.is_single_band());
        assert_eq!(plan_128.max_strip_rows, 393);
        assert_eq!(plan_128.inner_rows, 241);
        assert_eq!(plan_128.bands, 45);
        assert!(plan_128.band_rows + 2 * eff_halo <= plan_128.max_strip_rows);
        let peak_planes_bytes = (w150 as u64) * ((plan_128.band_rows + 2 * eff_halo) as u64) * 4 * PLANES;
        assert!(peak_planes_bytes <= (128 << 20), "peak planes {peak_planes_bytes} exceeded 128 MiB");

        // 4K (3840 x 2560): single-band fast path on >= 256 MiB adapters, automatic 2-band on 128 MiB adapters
        let (w4k, h4k) = (3840usize, 2560usize);
        let scale4k = crate::pipeline::frame_scale(w4k, h4k);
        let (halo4k, blur4k) = effective_halo_and_blur(&worst_recipe, scale4k, w4k, h4k);
        let plan_4k_256 = compute_strip_plan(w4k, h4k, halo4k, blur4k, 256 << 20, 65_535).expect("4K on 256 MiB");
        assert!(plan_4k_256.is_single_band());
        assert_eq!(plan_4k_256.band_rows, h4k);

        let plan_4k_128 = compute_strip_plan(w4k, h4k, halo4k, blur4k, 128 << 20, 65_535).expect("4K on 128 MiB");
        assert_eq!(plan_4k_128.bands, 2);
    }

    #[test]
    fn banded_local_f32_matches_cpu_and_single_band() {
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (203usize, 140usize);
        let input = frame(w, h);
        let recipe = Recipe {
            clarity: 25.0,
            texture: 20.0,
            dehaze: 15.0,
            midtone_detail: -20.0,
            sharpness: 40.0,
            denoise_lum: 30.0,
            denoise_col: 50.0,
            defringe: 40.0,
            ..Recipe::default()
        };
        let scale = 1.0;
        let (halo, max_blur) = effective_halo_and_blur(&recipe, scale, w, h);
        // Synthetic binding limit that allows max_strip_rows = 2*halo + 36 (< h), forcing >= 4 bands.
        let synthetic_rows = halo * 2 + 36;
        let synthetic_limit = (w as u64) * (synthetic_rows as u64) * 4 * PLANES;
        let sp = compute_strip_plan(w, h, halo, max_blur, synthetic_limit, 65_535).expect("multi-band plan");
        assert!(sp.bands >= 3, "expected >= 3 bands, got {}", sp.bands);

        let mut cpu = input.clone();
        presence::apply_presence_scaled(&mut cpu, w, h, recipe.clarity, recipe.texture, recipe.dehaze, recipe.midtone_detail, scale);
        detail::apply_detail_scaled(&mut cpu, w, h, recipe.sharpness, recipe.denoise_lum, recipe.denoise_col, scale);
        detail::apply_defringe_buffer(&mut cpu, w, h, recipe.defringe, scale);

        let mut single = input.clone();
        run_on(&gpu, &mut single, w, h, &recipe, scale).expect("single-band GPU");

        let mut banded = input.clone();
        run_on_with_limit(&gpu, &mut banded, w, h, &recipe, scale, synthetic_limit).expect("banded GPU");

        let mut worst_vs_single = 0.0f32;
        let mut off_vs_cpu = 0usize;
        let mut worst_vs_cpu = 0.0f32;
        for ((c, s), b) in cpu.iter().zip(&single).zip(&banded) {
            worst_vs_single = worst_vs_single.max((s - b).abs());
            let d = (c - b).abs();
            if d.is_nan() || d > 1e-3 {
                off_vs_cpu += 1;
            }
            worst_vs_cpu = if d.is_finite() { worst_vs_cpu.max(d) } else { f32::INFINITY };
        }
        eprintln!(
            "banded_local_f32 ({} bands): worst_vs_single={worst_vs_single}, off_vs_cpu={off_vs_cpu}, worst_vs_cpu={worst_vs_cpu}",
            sp.bands
        );
        assert!(worst_vs_single < 1e-3, "banded diverged from single-band GPU by {worst_vs_single}");
        assert!(off_vs_cpu * 1000 < w * h * 3, "{off_vs_cpu} values differ from CPU (worst {worst_vs_cpu})");
        assert!(worst_vs_cpu < 0.02, "worst CPU diff {worst_vs_cpu}");

        // Impossibly tiny limit must fail cleanly without modifying `buf`.
        let mut untouched = input.clone();
        let tiny_limit = (w as u64) * 8 * 4 * PLANES;
        assert!(run_on_with_limit(&gpu, &mut untouched, w, h, &recipe, scale, tiny_limit).is_err());
        assert_eq!(untouched, input, "failed banded run must leave buffer unchanged");
    }

    #[test]
    fn banded_chained_matches_cpu() {
        use crate::pipeline::transfer::Transfer;
        use crate::pipeline::{PointOps, white_balance};
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h, ch) = (157usize, 140usize, 3usize);
        let input: Vec<u16> = (0..w * h * ch)
            .map(|i| {
                let (px, c) = (i / ch, i % ch);
                let (x, y) = (px % w, px / w);
                let n = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503) ^ c as u32) % 3000;
                ((x * 30_000 / w + y * 20_000 / h) as u32 + n + c as u32 * 1500) as u16
            })
            .collect();
        let recipe = Recipe {
            exposure: 0.25,
            contrast: 18.0,
            shadows: 20.0,
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
        let (y0, y1) = (7, h);
        let rows = y1 - y0;

        let (halo, max_blur) = effective_halo_and_blur(&recipe, 1.0, w, rows);
        let synthetic_limit = (w as u64) * ((halo * 2 + 36) as u64) * 4 * PLANES;
        let sp = compute_strip_plan(w, rows, halo, max_blur, synthetic_limit, 65_535).expect("chained strip plan");
        assert!(sp.bands >= 3, "expected >= 3 bands, got {}", sp.bands);

        let single = chained_on(&gpu, &input, w, ch, y0, y1, &recipe, &ops.gpu_inputs(k, transfer), 1.0, (0, y0, w, h))
            .expect("single-band chained")
            .expect("work");
        let banded = chained_on_with_limit(
            &gpu,
            &input,
            w,
            ch,
            y0,
            y1,
            &recipe,
            &ops.gpu_inputs(k, transfer),
            1.0,
            (0, y0, w, h),
            synthetic_limit,
        )
        .expect("banded chained")
        .expect("work");

        let mut worst = 0.0f32;
        for (a, b) in single.iter().zip(&banded) {
            worst = worst.max((a - b).abs());
        }
        eprintln!("banded_chained ({} bands): worst diff vs single-band = {worst}", sp.bands);
        assert!(worst < 1e-3, "banded chained diverged from single-band by {worst}");
    }

    #[test]
    fn banded_display_matches_cpu_with_grain_vignette_geometry_and_scopes() {
        use crate::pipeline::transfer::Transfer;
        use crate::pipeline::{PointOps, white_balance};
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (163usize, 160usize);
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
        let geometry = Recipe {
            lens_distortion: 25.0,
            perspective_vertical: 18.0,
            perspective_horizontal: -8.0,
            ..grain.clone()
        };
        let barrel = Recipe { lens_distortion: -35.0, perspective_scale: 92.0, ..Recipe::default() };
        let masks = Recipe { layers: test_layers(), ..grain.clone() };
        let scale = crate::pipeline::frame_scale(w, h);
        // Synthetic binding limit giving max_strip_rows = 72 (< 160), forcing 3+ bands across all cases.
        let synthetic_limit = (w as u64) * 72 * 4 * PLANES;

        for ch in [3usize, 4] {
            let input: Vec<u16> = (0..w * h * ch)
                .map(|i| {
                    let (px, c) = (i / ch, i % ch);
                    let (x, y) = (px % w, px / w);
                    if c == 3 {
                        return (x * 400 + y * 50) as u16;
                    }
                    let n = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503) ^ c as u32) % 4000;
                    ((x * 40_000 / w + y * 20_000 / h) as u32 + n + c as u32 * 1500) as u16
                })
                .collect();
            for (name, recipe) in [
                ("default", Recipe::default()),
                ("grain_vignette", grain.clone()),
                ("geometry", geometry.clone()),
                ("barrel", barrel.clone()),
                ("masks", masks.clone()),
            ] {
                let transfer = Transfer::bt709();
                let (cpu, cpu_hist) = crate::pipeline::process_buffer_16_to_8_ex(
                    &input,
                    w as u32,
                    h as u32,
                    ch as u32,
                    &recipe,
                    transfer,
                    false,
                    false,
                );
                let ops = PointOps::new(&recipe);
                let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
                let e = 2.0f32.powf(recipe.exposure + recipe.baseline_exposure);
                let (halo, max_blur) = effective_halo_and_blur(&recipe, scale, w, h);
                let sp = compute_strip_plan(w, h, halo, max_blur, synthetic_limit, 65_535).expect("display strip plan");
                assert!(sp.bands >= 3, "{name} ch={ch}: expected >= 3 bands, got {}", sp.bands);

                let (got, hist) = display_on_with_limit(
                    &gpu,
                    &input,
                    w,
                    h,
                    ch,
                    &recipe,
                    &ops.gpu_inputs([wr * e, wg * e, wb * e], transfer),
                    scale,
                    synthetic_limit,
                )
                .expect("banded GPU preview");
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
                eprintln!(
                    "banded_display {name} ch={ch} ({} bands): {} pixels differ (worst {worst})",
                    sp.bands,
                    off_px.len()
                );
                assert!(worst <= 1, "banded {name} ch={ch}: worst difference {worst}");
                assert!(off_px.len() * 200 < w * h, "banded {name} ch={ch}: {} pixels differ", off_px.len());
                let n = off_px.len() as i64;
                for (a, b) in [(&cpu_hist.red, &hist.red), (&cpu_hist.green, &hist.green), (&cpu_hist.blue, &hist.blue), (&cpu_hist.luma, &hist.luma)] {
                    let d: i64 = a.iter().zip(b.iter()).map(|(x, y)| (*x as i64 - *y as i64).abs()).sum();
                    assert!(d <= 2 * n, "banded {name} ch={ch}: histogram differs by {d}");
                }
                assert!(
                    (cpu_hist.highlight_clipping_percent - hist.highlight_clipping_percent).abs() * (w * h) as f32
                        <= 100.0 * n as f32 + 1e-3
                );
                if n == 0 {
                    assert_eq!(cpu_hist.waveform_luma, hist.waveform_luma, "banded {name} ch={ch}: waveform");
                    assert_eq!(cpu_hist.parade_r, hist.parade_r, "banded {name} ch={ch}: parade");
                }
            }
        }
    }

    #[test]
    fn tiled_and_fallback_blur_both_match_cpu() {
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (160usize, 160usize);
        let input = frame(w, h);
        let clarity = Recipe { clarity: 35.0, sharpness: 0.0, denoise_col: 0.0, denoise_lum: 0.0, ..Recipe::default() };

        // scale = 1.0 => clarity radius 16 <= 64 => tiled shared-memory blur kernels.
        let steps_tiled = plan(w, h, w, h, &clarity, 1.0);
        assert!(steps_tiled.iter().any(|s| s.entry == entry("blur_h_tiled")));
        assert!(steps_tiled.iter().any(|s| s.entry == entry("blur_v_tiled")));
        assert!(!steps_tiled.iter().any(|s| s.entry == entry("blur_h")));

        // scale = 5.0 => clarity radius 80 (clamped to min(80, 160/2)=80 > 64) => sliding-window fallback.
        let steps_fallback = plan(w, h, w, h, &clarity, 5.0);
        assert!(steps_fallback.iter().any(|s| s.entry == entry("blur_h")));
        assert!(steps_fallback.iter().any(|s| s.entry == entry("blur_v")));

        for (label, scale) in [("tiled_r16", 1.0f32), ("fallback_r80", 5.0f32)] {
            let mut cpu = input.clone();
            presence::apply_presence_scaled(&mut cpu, w, h, clarity.clarity, 0.0, 0.0, 0.0, scale);
            let mut got = input.clone();
            run_on(&gpu, &mut got, w, h, &clarity, scale).expect(label);
            let worst = cpu.iter().zip(&got).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            eprintln!("{label}: worst diff vs CPU = {worst}");
            assert!(worst < 1e-3, "{label}: worst diff {worst}");
        }
    }

    #[test]
    fn combined_hist_all_matches_separate_scope_passes_and_timestamps() {
        use crate::pipeline::transfer::Transfer;
        use crate::pipeline::{PointOps, white_balance};
        let _test_lock = super::super::TEST_GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(gpu) = super::super::acquire() else {
            eprintln!("no GPU, skipped");
            return;
        };
        let (w, h) = (151usize, 93usize);
        let input: Vec<u16> = (0..w * h * 3)
            .map(|i| {
                let (px, c) = (i / 3, i % 3);
                let (x, y) = (px % w, px / w);
                let n = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503) ^ c as u32) % 4000;
                ((x * 40_000 / w + y * 20_000 / h) as u32 + n + c as u32 * 1500) as u16
            })
            .collect();
        let recipe = Recipe { exposure: 0.2, clarity: 20.0, sharpness: 25.0, vignette: -15.0, ..Recipe::default() };
        let transfer = Transfer::bt709();
        let ops = PointOps::new(&recipe);
        let (wr, wg, wb) = white_balance::kelvin_to_rgb_multipliers(recipe.wb_temperature, recipe.wb_tint);
        let e = 2.0f32.powf(recipe.exposure + recipe.baseline_exposure);
        let inputs = ops.gpu_inputs([wr * e, wg * e, wb * e], transfer);
        let scale = crate::pipeline::frame_scale(w, h);

        let p = pipeline(&gpu).expect("local pipeline");
        super::super::set_bench_profiling(true);
        let _ = super::super::take_last_gpu_pass_ms();
        let (px_sep, hist_sep) = display_on_with_options(&gpu, &input, w, h, 3, &recipe, &inputs, scale, gpu_strip_limit(&gpu), Some(false))
            .expect("separate scope passes");
        let ts_ms = super::super::take_last_gpu_pass_ms();
        super::super::set_bench_profiling(false);
        if gpu.features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            let ms = ts_ms.expect("TIMESTAMP_QUERY enabled should record gpu_pass_ms");
            assert!(ms.is_finite() && ms >= 0.0, "invalid GPU timestamp duration {ms}");
        }

        if p.hist_all.is_some() {
            let (px_comb, hist_comb) = display_on_with_options(&gpu, &input, w, h, 3, &recipe, &inputs, scale, gpu_strip_limit(&gpu), Some(true))
                .expect("combined hist_all pass");
            assert_eq!(px_comb, px_sep, "combined hist_all pixels must match separate passes");
            assert_eq!(hist_comb.red, hist_sep.red);
            assert_eq!(hist_comb.green, hist_sep.green);
            assert_eq!(hist_comb.blue, hist_sep.blue);
            assert_eq!(hist_comb.luma, hist_sep.luma);
            assert_eq!(hist_comb.waveform_luma, hist_sep.waveform_luma);
            assert_eq!(hist_comb.parade_r, hist_sep.parade_r);
            assert_eq!(hist_comb.parade_g, hist_sep.parade_g);
            assert_eq!(hist_comb.parade_b, hist_sep.parade_b);
            assert_eq!(hist_comb.vectorscope, hist_sep.vectorscope);
            assert_eq!(hist_comb.highlight_clipping_percent, hist_sep.highlight_clipping_percent);
            assert_eq!(hist_comb.shadow_clipping_percent, hist_sep.shadow_clipping_percent);
        } else {
            // Even when the adapter's workgroup storage limit is below 34 KiB (e.g. 32 KiB lavapipe),
            // compile HIST_ALL_WGSL through Naga so CI catches any WGSL syntax or type regressions.
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let _ = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("hist_all syntax check"),
                source: wgpu::ShaderSource::Wgsl(HIST_ALL_WGSL.into()),
            });
            assert!(
                pollster::block_on(scope.pop()).is_none(),
                "HIST_ALL_WGSL failed WGSL validation"
            );
            eprintln!(
                "adapter max_compute_workgroup_storage_size={} < {}; hist_all module validated, dispatch skipped",
                gpu.limits.max_compute_workgroup_storage_size, COMBINED_HIST_WG_STORAGE
            );
        }
    }
}
