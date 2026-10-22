//! Film simulation calibration from the user's own photos.
//!
//! Every RAF carries the JPEG the camera rendered with its film simulation. Developing
//! the RAF neutrally and comparing it with that JPEG pixel by pixel gives samples of
//! the camera's look; they are fitted into a 33³ 3D LUT per film simulation, stored in
//! `~/.local/share/omastudio/film-luts/<simulation>.cube`. The pipeline uses such a LUT
//! instead of its built-in approximation. No Fujifilm data is shipped: each LUT is
//! measured from the photographer's own files.

use super::{CameraRecipe, makernote};
use crate::pipeline::{self, lut::Lut3D};
use crate::raw::RawImage;
use crate::recipe::Recipe;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const LUT_SIZE: usize = 33;
/// The comparison grid is the camera JPEG reduced by this factor, which averages out
/// noise, sharpening and small misalignment.
const REDUCE: u32 = 8;

pub fn lut_dir() -> PathBuf {
    pipeline::film_sim::calibrated_lut_dir()
}

pub fn lut_path(sim_id: &str) -> PathBuf {
    lut_dir().join(format!("{}.cube", sim_id))
}

/// Sample accumulator for one film simulation.
pub struct Fit {
    sum: Vec<[f64; 3]>,
    weight: Vec<f64>,
    pub photos: usize,
    pub samples: usize,
}

impl Default for Fit {
    fn default() -> Self {
        let n = LUT_SIZE * LUT_SIZE * LUT_SIZE;
        Self { sum: vec![[0.0; 3]; n], weight: vec![0.0; n], photos: 0, samples: 0 }
    }
}

fn index(r: usize, g: usize, b: usize) -> usize {
    (b * LUT_SIZE + g) * LUT_SIZE + r
}

impl Fit {
    /// Adds one source → target colour pair (both 0..1, display-encoded), spread over the
    /// 8 surrounding LUT nodes with trilinear weights. The offset target − source is
    /// accumulated, so sparse nodes fall back towards identity smoothly.
    pub fn add(&mut self, src: [f32; 3], dst: [f32; 3]) {
        let m = (LUT_SIZE - 1) as f32;
        let pos = src.map(|v| v.clamp(0.0, 1.0) * m);
        let base = pos.map(|p| (p.floor() as usize).min(LUT_SIZE - 2));
        let frac = [pos[0] - base[0] as f32, pos[1] - base[1] as f32, pos[2] - base[2] as f32];
        let off = [dst[0] - src[0], dst[1] - src[1], dst[2] - src[2]];
        for corner in 0..8 {
            let (dr, dg, db) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let w = (if dr == 1 { frac[0] } else { 1.0 - frac[0] })
                * (if dg == 1 { frac[1] } else { 1.0 - frac[1] })
                * (if db == 1 { frac[2] } else { 1.0 - frac[2] });
            if w <= 0.0 {
                continue;
            }
            let i = index(base[0] + dr, base[1] + dg, base[2] + db);
            for (sum, o) in self.sum[i].iter_mut().zip(off) {
                *sum += (w * o) as f64;
            }
            self.weight[i] += w as f64;
        }
        self.samples += 1;
    }

    /// Solves for a smooth LUT: each node balances its own samples against the mean of
    /// its neighbours (a membrane fit), so empty regions are filled from nearby data.
    pub fn solve(&self) -> Lut3D {
        let n = LUT_SIZE;
        let smooth = 2.0;
        let data: Vec<[f64; 3]> = self
            .sum
            .iter()
            .zip(&self.weight)
            .map(|(s, &w)| if w > 0.0 { [s[0] / w, s[1] / w, s[2] / w] } else { [0.0; 3] })
            .collect();
        let mut off = data.clone();
        for _ in 0..300 {
            for b in 0..n {
                for g in 0..n {
                    for r in 0..n {
                        let i = index(r, g, b);
                        let mut acc = [0.0f64; 3];
                        let mut count = 0.0;
                        let mut visit = |j: usize| {
                            for c in 0..3 {
                                acc[c] += off[j][c];
                            }
                            count += 1.0;
                        };
                        if r > 0 {
                            visit(index(r - 1, g, b));
                        }
                        if r + 1 < n {
                            visit(index(r + 1, g, b));
                        }
                        if g > 0 {
                            visit(index(r, g - 1, b));
                        }
                        if g + 1 < n {
                            visit(index(r, g + 1, b));
                        }
                        if b > 0 {
                            visit(index(r, g, b - 1));
                        }
                        if b + 1 < n {
                            visit(index(r, g, b + 1));
                        }
                        let w = self.weight[i];
                        for c in 0..3 {
                            off[i][c] = (w * data[i][c] + smooth * acc[c]) / (w + smooth * count);
                        }
                    }
                }
            }
        }
        let mut lut = Lut3D::identity(n);
        for (entry, o) in lut.table.iter_mut().zip(&off) {
            for c in 0..3 {
                entry[c] = (entry[c] + o[c] as f32).clamp(0.0, 1.0);
            }
        }
        lut
    }
}

/// Box-averages an RGB8 image onto a `dw`×`dh` grid, as 0..1 floats.
pub(crate) fn reduce(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0f32; 3]; (dw * dh) as usize];
    for (y, row) in out.chunks_mut(dw as usize).enumerate() {
        let y0 = (y as u64 * sh as u64 / dh as u64) as u32;
        let y1 = (((y as u64 + 1) * sh as u64 / dh as u64) as u32).max(y0 + 1);
        for (x, px) in row.iter_mut().enumerate() {
            let x0 = (x as u64 * sw as u64 / dw as u64) as u32;
            let x1 = (((x as u64 + 1) * sw as u64 / dw as u64) as u32).max(x0 + 1);
            let mut acc = [0u64; 3];
            for yy in y0..y1.min(sh) {
                for xx in x0..x1.min(sw) {
                    let i = ((yy * sw + xx) * 3) as usize;
                    for c in 0..3 {
                        acc[c] += src[i + c] as u64;
                    }
                }
            }
            let n = ((y1.min(sh) - y0) * (x1.min(sw) - x0)).max(1) as f32 * 255.0;
            *px = [acc[0] as f32 / n, acc[1] as f32 / n, acc[2] as f32 / n];
        }
    }
    out
}

/// Camera settings that change the JPEG beyond the film simulation itself.
pub fn non_neutral_settings(r: &CameraRecipe) -> Vec<&'static str> {
    let mut v = Vec::new();
    if r.highlight != 0.0 || r.shadow != 0.0 {
        v.push("highlight/shadow tone");
    }
    if r.color != 0 {
        v.push("colour");
    }
    if r.dynamic_range.unwrap_or(100) > 100 {
        v.push("dynamic range above DR100");
    }
    if r.color_chrome != super::Strength::Off || r.color_chrome_fx_blue != super::Strength::Off {
        v.push("Color Chrome");
    }
    v
}

/// A RAF developed neutrally next to the camera's own rendering of it, on one grid.
pub struct Pair {
    pub sim_id: String,
    pub source: Vec<[f32; 3]>,
    pub target: Vec<[f32; 3]>,
    pub width: u32,
    pub height: u32,
}

/// Reads one RAF: its film simulation, its neutral development and the camera JPEG.
pub fn load_pair(path: &Path, allow_non_neutral: bool) -> Result<Pair, String> {
    let camera = CameraRecipe::read(path).ok_or("Not a Fujifilm RAF with a camera recipe")?;
    let extra = non_neutral_settings(&camera);
    if !extra.is_empty() && !allow_non_neutral {
        return Err(format!("Shot with {}; use photos with neutral settings or pass --any", extra.join(", ")));
    }
    let jpeg_bytes = makernote::embedded_jpeg(path)?;
    let jpeg = image::load_from_memory_with_format(&jpeg_bytes, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Cannot decode the camera JPEG: {}", e))?
        .to_rgb8();
    let (jw, jh) = jpeg.dimensions();

    let raw = RawImage::open(path)?;
    let preview = raw.process_preview_16(true)?;
    if preview.channels != 3 {
        return Err("Unexpected channel count".into());
    }
    let neutral = Recipe { sharpness: 0.0, denoise_col: 0.0, ..Recipe::default() };
    let (rendered, _) = pipeline::process_buffer_16_to_8_ex(
        preview.as_slice_u16(),
        preview.width,
        preview.height,
        3,
        &neutral,
        preview.transfer,
        false,
        false,
    );
    if (preview.width as f32 / preview.height as f32 - jw as f32 / jh as f32).abs() > 0.02 {
        return Err(format!("Camera JPEG ({}x{}) and RAW ({}x{}) are framed differently", jw, jh, preview.width, preview.height));
    }
    let (dw, dh) = ((jw / REDUCE).max(16), (jh / REDUCE).max(16));
    Ok(Pair {
        sim_id: camera.film_simulation.engine_id().to_string(),
        source: reduce(&rendered, preview.width, preview.height, dw, dh),
        target: reduce(jpeg.as_raw(), jw, jh, dw, dh),
        width: dw,
        height: dh,
    })
}

/// Quality figures for a fit: mean absolute error (0..255 scale) on held-out rows.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Report {
    pub film_simulation: String,
    pub photos: usize,
    pub samples: usize,
    pub error_builtin: f32,
    pub error_calibrated: f32,
    pub lut: String,
}

fn usable(src: [f32; 3], dst: [f32; 3]) -> bool {
    // Clipped channels carry no colour information
    src.iter().chain(dst.iter()).all(|&v| v < 0.985)
}

/// Held-out (neutral, camera) sample pairs, keyed by film simulation.
type HeldOut = HashMap<String, Vec<([f32; 3], [f32; 3])>>;

/// Fits LUTs from RAF files and writes them. Odd grid rows are held out to measure
/// how well the calibrated LUT and the built-in approximation match the camera.
pub fn calibrate(paths: &[PathBuf], allow_non_neutral: bool, write: bool) -> (Vec<Report>, Vec<String>) {
    let mut fits: HashMap<String, Fit> = HashMap::new();
    let mut held: HeldOut = HashMap::new();
    let mut skipped = Vec::new();
    for path in paths {
        let pair = match load_pair(path, allow_non_neutral) {
            Ok(p) => p,
            Err(e) => {
                skipped.push(format!("{}: {}", path.display(), e));
                continue;
            }
        };
        let fit = fits.entry(pair.sim_id.clone()).or_default();
        fit.photos += 1;
        let hold = held.entry(pair.sim_id.clone()).or_default();
        for y in 0..pair.height as usize {
            for x in 0..pair.width as usize {
                let i = y * pair.width as usize + x;
                let (s, d) = (pair.source[i], pair.target[i]);
                if !usable(s, d) {
                    continue;
                }
                if y % 2 == 0 {
                    fit.add(s, d);
                } else {
                    hold.push((s, d));
                }
            }
        }
    }
    let mut reports = Vec::new();
    for (sim_id, fit) in fits {
        let lut = fit.solve();
        let sim = pipeline::film_sim::FilmSimulation::from_id(&sim_id);
        let hold = &held[&sim_id];
        let err = |f: &dyn Fn([f32; 3]) -> (f32, f32, f32)| {
            let total: f32 = hold
                .iter()
                .map(|(s, d)| {
                    let (r, g, b) = f(*s);
                    (r - d[0]).abs() + (g - d[1]).abs() + (b - d[2]).abs()
                })
                .sum();
            total / (hold.len().max(1) as f32 * 3.0) * 255.0
        };
        let error_builtin = err(&|s| sim.apply_pixel(s[0], s[1], s[2], 1.0));
        let error_calibrated = err(&|s| lut.sample(s[0], s[1], s[2]));
        let path = lut_path(&sim_id);
        if write && let Err(e) = write_cube(&lut, &path, &sim_id) {
            skipped.push(format!("{}: {}", path.display(), e));
        }
        reports.push(Report {
            film_simulation: sim_id,
            photos: fit.photos,
            samples: fit.samples,
            error_builtin,
            error_calibrated,
            lut: path.to_string_lossy().into_owned(),
        });
    }
    reports.sort_by(|a, b| a.film_simulation.cmp(&b.film_simulation));
    (reports, skipped)
}

pub fn write_cube(lut: &Lut3D, path: &Path, title: &str) -> Result<(), String> {
    let mut out = format!(
        "# OmaStudio film simulation calibration, measured from the photographer's own RAF files\nTITLE \"{}\"\nLUT_3D_SIZE {}\n",
        title, lut.size
    );
    for e in &lut.table {
        out += &format!("{:.6} {:.6} {:.6}\n", e[0], e[1], e[2]);
    }
    if let Some(dir) = path.parent() {
        crate::security::ensure_secure_dir(dir).map_err(|e| e.to_string())?;
    }
    crate::security::atomic_write_secure(path, out.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_recovers_a_smooth_look_and_fills_gaps() {
        // A look that lifts red and lowers blue; samples cover only the lower half of the cube
        let look = |s: [f32; 3]| [(s[0] * 1.1 + 0.02).min(1.0), s[1], s[2] * 0.9];
        let mut fit = Fit::default();
        for i in 0..20 {
            for j in 0..20 {
                for k in 0..20 {
                    let s = [i as f32 / 40.0, j as f32 / 40.0, k as f32 / 40.0];
                    fit.add(s, look(s));
                }
            }
        }
        let lut = fit.solve();
        let (r, g, b) = lut.sample(0.3, 0.2, 0.4);
        let want = look([0.3, 0.2, 0.4]);
        assert!((r - want[0]).abs() < 0.01 && (g - want[1]).abs() < 0.01 && (b - want[2]).abs() < 0.01);
        // Far outside the samples the LUT stays finite and in range
        let (r, g, b) = lut.sample(0.95, 0.95, 0.95);
        assert!([r, g, b].iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn reduce_averages_blocks() {
        let src = [0u8, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255];
        let out = reduce(&src, 2, 2, 1, 1);
        assert!((out[0][0] - 0.5).abs() < 1e-6);
    }
}
