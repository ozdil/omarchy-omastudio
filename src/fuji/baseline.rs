//! Baseline exposure of Fujifilm RAW files, measured from the file itself.
//!
//! Fujifilm cameras expose the sensor to protect highlights and brighten the image
//! while rendering it; how much depends on the model, the dynamic range setting and
//! the scene. Rather than relying on a per-model table, the engine develops a small
//! copy of the photo with the camera's own recipe and finds the exposure at which its
//! median brightness equals that of the JPEG the camera stored in the same RAF.

use super::{CameraRecipe, makernote};
use crate::pipeline;
use crate::raw::{ProcessedBuffer, RawImage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Long edge of the development used for the measurement.
const PROBE_EDGE: u32 = 320;
/// Search range, in EV.
const RANGE: (f32, f32) = (-3.0, 4.0);

type Key = (PathBuf, u64, Option<SystemTime>);
/// Baseline exposure in EV, and why it could not be measured if it could not.
type Measured = (f32, Option<String>);

/// Results per file, so the preview, the full-resolution frame and the export of one
/// photo share exactly the same value.
fn memo() -> &'static Mutex<HashMap<Key, Measured>> {
    static MEMO: OnceLock<Mutex<HashMap<Key, Measured>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(path: &Path) -> Key {
    let meta = std::fs::metadata(path).ok();
    (
        path.to_path_buf(),
        meta.as_ref().map_or(0, |m| m.len()),
        meta.and_then(|m| m.modified().ok()),
    )
}

/// Baseline exposure in EV for `raw`; 0 for files from other makers, without a camera
/// JPEG, or with `OMASTUDIO_BASELINE_EXPOSURE=off`. `decoded` is a linear 16-bit
/// development of the photo at any size when the caller already has one.
pub fn exposure(raw: &RawImage, decoded: Option<&ProcessedBuffer>) -> f32 {
    explain(raw, decoded).0
}

/// [`exposure`], with the reason when it could not be measured.
pub fn explain(raw: &RawImage, decoded: Option<&ProcessedBuffer>) -> Measured {
    if std::env::var("OMASTUDIO_BASELINE_EXPOSURE").is_ok_and(|v| v == "off") {
        return (0.0, None);
    }
    let path = raw.path();
    if !raw.get_metadata().is_ok_and(|m| m.make.to_ascii_lowercase().starts_with("fujifilm")) {
        return (0.0, None);
    }
    let k = key(path);
    if let Some(found) = memo().lock().unwrap().get(&k) {
        return found.clone();
    }
    // Without a development at hand, a separate handle decodes a half-size one so the
    // caller's handle stays untouched for its own decode
    let owned;
    let buf = match decoded {
        Some(b) => b,
        None => match RawImage::open(path).and_then(|r| r.process_preview_16(true)) {
            Ok(b) => {
                owned = b;
                &owned
            }
            Err(e) => return (0.0, Some(e)),
        },
    };
    let result = match measure(path, buf) {
        Ok(ev) => (ev, None),
        Err(e) => (0.0, Some(e)),
    };
    memo().lock().unwrap().insert(k, result.clone());
    result
}

fn measure(path: &Path, buf: &ProcessedBuffer) -> Result<f32, String> {
    let target = camera_median(path).ok_or("no readable camera JPEG")?;
    if !(0.02..=0.95).contains(&target) {
        return Err(format!("camera JPEG median {target:.3} is out of range"));
    }
    let mut recipe = CameraRecipe::read(path).ok_or("no camera recipe")?.to_recipe();
    recipe.sharpness = 0.0;
    recipe.grain_amount = 0.0;
    recipe.denoise_lum = 0.0;
    recipe.denoise_col = 0.0;
    let (small, w, h) = shrink(buf.as_slice_u16(), buf.width, buf.height, buf.channels).ok_or("unusable development")?;
    let median_at = |ev: f32| {
        let r = crate::recipe::Recipe { baseline_exposure: ev, ..recipe.clone() };
        let (out, _) = pipeline::process_buffer_16_to_8_ex(&small, w, h, 3, &r, buf.transfer, false, false);
        median_luma(&out)
    };
    // The median brightness rises with exposure, so a bisection finds the match
    let (mut lo, mut hi) = RANGE;
    let (at_lo, at_hi) = (median_at(lo), median_at(hi));
    if at_hi < target || at_lo > target {
        return Err(format!("camera JPEG median {target:.3} outside {at_lo:.3}..{at_hi:.3} over {lo}..{hi} EV"));
    }
    for _ in 0..10 {
        let mid = 0.5 * (lo + hi);
        if median_at(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(0.5 * (lo + hi))
}

/// Median brightness of the camera JPEG, from its EXIF thumbnail when it has one.
fn camera_median(path: &Path) -> Option<f32> {
    let jpeg = makernote::embedded_jpeg(path).ok()?;
    let img = exif_thumbnail(&jpeg)
        .and_then(|t| image::load_from_memory_with_format(t, image::ImageFormat::Jpeg).ok())
        .or_else(|| image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).ok())?
        .to_rgb8();
    if std::env::var_os("OMASTUDIO_BASELINE_DEBUG").is_some() {
        eprintln!("baseline: {} reference {}x{}, median {:.3}", path.display(), img.width(), img.height(), median_luma(img.as_raw()));
    }
    Some(median_luma(img.as_raw()))
}

/// The thumbnail JPEG inside the APP1 (EXIF) segment of a JPEG, if any.
fn exif_thumbnail(jpeg: &[u8]) -> Option<&[u8]> {
    let mut i = 2;
    while i + 4 <= jpeg.len() && jpeg[i] == 0xFF {
        let marker = jpeg[i + 1];
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        let seg = jpeg.get(i + 4..i + 2 + len)?;
        if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
            let start = seg.windows(3).position(|w| w == [0xFF, 0xD8, 0xFF])?;
            let end = seg.windows(2).rposition(|w| w == [0xFF, 0xD9])? + 2;
            return (end > start).then(|| &seg[start..end]);
        }
        if marker == 0xDA {
            break;
        }
        i += 2 + len;
    }
    None
}

/// Median of Rec. 709 luma of an 8-bit RGB image, 0..1.
fn median_luma(rgb: &[u8]) -> f32 {
    let mut hist = [0usize; 256];
    for p in rgb.as_chunks::<3>().0 {
        let y = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
        hist[(y.round() as usize).min(255)] += 1;
    }
    let half = rgb.len() / 6;
    let mut acc = 0;
    for (v, n) in hist.iter().enumerate() {
        acc += n;
        if acc > half {
            return v as f32 / 255.0;
        }
    }
    1.0
}

/// Box-averages a 16-bit RGB(A) buffer to at most [`PROBE_EDGE`] pixels on the long side.
fn shrink(data: &[u16], w: u32, h: u32, ch: u32) -> Option<(Vec<u16>, u32, u32)> {
    let (w, h, ch) = (w as usize, h as usize, ch as usize);
    if ch < 3 || w == 0 || h == 0 || data.len() < w * h * ch {
        return None;
    }
    let f = w.max(h).div_ceil(PROBE_EDGE as usize).max(1);
    let (sw, sh) = (w / f, h / f);
    if sw == 0 || sh == 0 {
        return None;
    }
    let mut out = vec![0u16; sw * sh * 3];
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0u64; 3];
            for yy in y * f..(y + 1) * f {
                let row = &data[(yy * w + x * f) * ch..(yy * w + (x + 1) * f) * ch];
                for p in row.chunks_exact(ch) {
                    for c in 0..3 {
                        acc[c] += p[c] as u64;
                    }
                }
            }
            let n = (f * f) as u64;
            for c in 0..3 {
                out[(y * sw + x) * 3 + c] = (acc[c] / n) as u16;
            }
        }
    }
    Some((out, sw as u32, sh as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_a_split_image() {
        // 6 dark pixels, 4 bright ones
        let mut rgb = vec![0u8; 30];
        rgb[18..].fill(255);
        assert_eq!(median_luma(&rgb), 0.0);
        rgb[12..].fill(255);
        assert_eq!(median_luma(&rgb), 1.0);
    }

    #[test]
    fn shrink_averages_blocks() {
        let data = vec![1000u16; 640 * 2 * 3 * 4];
        let (out, w, h) = shrink(&data, 640 * 2, 4, 3).unwrap();
        assert_eq!((w, h), (320, 1));
        assert!(out.iter().all(|&v| v == 1000));
    }

    #[test]
    fn finds_the_exif_thumbnail() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let mut seg = b"Exif\0\0IIxx".to_vec();
        seg.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xDB, 1, 2, 0xFF, 0xD9]);
        jpeg.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&seg);
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
        assert_eq!(exif_thumbnail(&jpeg), Some(&[0xFF, 0xD8, 0xFF, 0xDB, 1, 2, 0xFF, 0xD9][..]));
    }
}
