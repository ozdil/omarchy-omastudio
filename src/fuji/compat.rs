//! Compatibility check of Fujifilm RAW files: how the engine reads the sensor and
//! the recorded settings, and how close its development with the camera's recipe
//! comes to the JPEG the camera embedded in the same file.

use super::{CameraRecipe, makernote};
use crate::pipeline;
use crate::raw::RawImage;
use crate::raw::demosaic::CfaPattern;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Default, Serialize)]
pub struct Sensor {
    /// "x-trans", "bayer" or "other" (developed by LibRaw)
    pub layout: String,
    pub width: usize,
    pub height: usize,
    pub own_development: bool,
    pub colour_matrix: bool,
    pub lens_correction: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct Match {
    /// Mean absolute difference to the camera JPEG, 0..255 per channel
    pub mean_error: f32,
    /// Mean brightness of the development relative to the camera JPEG (1 = equal)
    pub brightness_ratio: f32,
    /// Mean difference per channel (development minus camera), 0..255
    pub channel_bias: [f32; 3],
    pub compared_at: String,
}

#[derive(Debug, Default, Serialize)]
pub struct FileReport {
    pub file: String,
    pub make: String,
    pub model: String,
    pub iso: f32,
    pub opens: bool,
    pub sensor: Option<Sensor>,
    pub recipe: Option<CameraRecipe>,
    /// Settings that the camera recorded and the engine only approximates
    pub approximated: Vec<&'static str>,
    pub camera_match: Option<Match>,
    pub problems: Vec<String>,
}

fn sensor(raw: &RawImage) -> Sensor {
    match raw.cfa_frame() {
        Some(frame) => Sensor {
            layout: match frame.pattern {
                CfaPattern::XTrans(_) => "x-trans".into(),
                CfaPattern::Bayer(_) => "bayer".into(),
            },
            width: frame.width,
            height: frame.height,
            own_development: true,
            colour_matrix: frame.rgb_cam.iter().flatten().any(|v| v.abs() > 1e-6)
                && frame.rgb_cam != [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            lens_correction: raw.lens_correction().is_some(),
        },
        None => Sensor {
            layout: "other".into(),
            lens_correction: raw.lens_correction().is_some(),
            ..Sensor::default()
        },
    }
}

/// Develops the photo with the camera's recipe and compares it with the camera JPEG.
fn camera_match(raw: &RawImage, path: &Path, recipe: &CameraRecipe, full: bool) -> Result<Match, String> {
    let jpeg = makernote::embedded_jpeg(path)?;
    let jpeg = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Cannot decode the camera JPEG: {e}"))?
        .to_rgb8();
    let (jw, jh) = jpeg.dimensions();
    let buf = if full { raw.process_full_16(3)? } else { raw.process_preview_16(true)? };
    if buf.channels != 3 {
        return Err("Unexpected channel count".into());
    }
    let mut r = recipe.to_recipe();
    r.baseline_exposure = raw.baseline_exposure();
    // Sharpening and grain are texture; they would only add noise to the comparison
    r.sharpness = 0.0;
    r.grain_amount = 0.0;
    let (dev, _) = pipeline::process_buffer_16_to_8_ex(buf.as_slice_u16(), buf.width, buf.height, 3, &r, buf.transfer, false, false);
    let aspect = |w: u32, h: u32| w as f32 / h as f32;
    // A vertical shot: the development is rotated, the camera JPEG may not be (its EXIF
    // orientation says how); compare with both rotations and keep the closer one
    let candidates = if (aspect(buf.width, buf.height) - aspect(jw, jh)).abs() <= 0.02 {
        vec![jpeg]
    } else if (aspect(buf.width, buf.height) - aspect(jh, jw)).abs() <= 0.02 {
        vec![image::imageops::rotate90(&jpeg), image::imageops::rotate270(&jpeg)]
    } else {
        return Err(format!("Camera JPEG ({jw}x{jh}) and RAW ({}x{}) are framed differently", buf.width, buf.height));
    };
    candidates
        .iter()
        .filter_map(|j| measure(&dev, buf.width, buf.height, j))
        .min_by(|a, b| a.mean_error.total_cmp(&b.mean_error))
        .ok_or_else(|| "The photo is clipped everywhere".to_string())
}

/// Compares a development with the camera JPEG on a coarse grid, ignoring clipped areas.
fn measure(dev: &[u8], w: u32, h: u32, jpeg: &image::RgbImage) -> Option<Match> {
    let (jw, jh) = jpeg.dimensions();
    let (dw, dh) = ((jw / 8).max(16), (jh / 8).max(16));
    let a = super::calibrate::reduce(dev, w, h, dw, dh);
    let b = super::calibrate::reduce(jpeg.as_raw(), jw, jh, dw, dh);
    let (mut err, mut la, mut lb, mut bias, mut n) = (0f64, 0f64, 0f64, [0f64; 3], 0usize);
    for (p, q) in a.iter().zip(&b) {
        // Clipped areas carry no information about the rendering
        if p.iter().chain(q.iter()).any(|&v| v >= 0.985) {
            continue;
        }
        for c in 0..3 {
            let d = (p[c] - q[c]) as f64;
            err += d.abs();
            bias[c] += d;
        }
        la += (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) as f64;
        lb += (0.2126 * q[0] + 0.7152 * q[1] + 0.0722 * q[2]) as f64;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let s = 255.0 / n as f64;
    Some(Match {
        mean_error: (err * s / 3.0) as f32,
        brightness_ratio: (la / lb.max(1e-9)) as f32,
        channel_bias: [(bias[0] * s) as f32, (bias[1] * s) as f32, (bias[2] * s) as f32],
        compared_at: format!("{dw}x{dh} (camera JPEG {jw}x{jh})"),
    })
}

pub fn check(path: &Path, full: bool) -> FileReport {
    let mut rep = FileReport { file: path.display().to_string(), ..FileReport::default() };
    let raw = match RawImage::open(path) {
        Ok(r) => r,
        Err(e) => {
            rep.problems.push(format!("Does not open: {e}"));
            return rep;
        }
    };
    rep.opens = true;
    if let Ok(m) = raw.get_metadata() {
        rep.make = m.make;
        rep.model = m.model;
        rep.iso = m.iso;
    }
    let s = sensor(&raw);
    if !s.own_development {
        rep.problems.push("Developed by LibRaw, not the engine's own path".into());
    } else if !s.colour_matrix {
        rep.problems.push("No colour matrix for this camera".into());
    }
    rep.sensor = Some(s);
    match CameraRecipe::read(path) {
        Some(r) => {
            rep.approximated = super::calibrate::non_neutral_settings(&r);
            match camera_match(&raw, path, &r, full) {
                Ok(m) => rep.camera_match = Some(m),
                Err(e) => rep.problems.push(e),
            }
            rep.recipe = Some(r);
        }
        None => rep.problems.push("No Fujifilm settings found in the file".into()),
    }
    rep
}

/// Checks each file; a failure in one file is reported and does not stop the others.
pub fn check_all(paths: &[std::path::PathBuf], full: bool) -> Vec<FileReport> {
    paths
        .iter()
        .map(|p| {
            std::panic::catch_unwind(|| check(p, full)).unwrap_or_else(|_| FileReport {
                file: p.display().to_string(),
                problems: vec!["The engine crashed on this file".into()],
                ..FileReport::default()
            })
        })
        .collect()
}
