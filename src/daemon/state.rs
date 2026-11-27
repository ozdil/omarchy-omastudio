//! Daemon photo cache, background full-resolution decoder, and detail resampling helpers.

use std::sync::{Arc, Mutex};

use omastudio_engine::{
    pipeline::{self, transfer::Transfer},
    raw::{self, RawImage},
};
use rayon::prelude::*;

use crate::cli::{ResponseWrapper, print_json};
use super::protocol::FullReady;

pub struct DaemonCache {
    pub current_path: Option<String>,
    pub raw_buffer_16: Option<Arc<Vec<u16>>>,
    pub raw_buffer_8: Option<Arc<Vec<u8>>>,
    /// Transfer curve of `raw_buffer_16` (linear light for LibRaw decodes).
    pub transfer: Transfer,
    /// The photo's baseline exposure (see `Recipe::baseline_exposure`).
    pub baseline_exposure: f32,
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub metadata: Option<raw::RawMetadata>,
    pub ping_pong: usize,
    /// The photo at full resolution for the viewport's 1:1 detail view, decoded in the
    /// background after `load` (see `full_ready`).
    pub full: Option<Arc<FullFrame>>,
    /// `full` with the recipe's heal and clone spots applied, for the spots it was
    /// made with, so panning the detail view does not redo them.
    pub full_retouched: Option<(Arc<FullFrame>, Vec<pipeline::retouch::RetouchSpot>, Arc<FullFrame>)>,
    /// Monotonic generation incremented on every photo load for GPU-resident preview caching.
    pub preview_gen: u64,
    pub detail_ping_pong: usize,
}

impl Default for DaemonCache {
    fn default() -> Self {
        Self {
            current_path: None,
            raw_buffer_16: None,
            raw_buffer_8: None,
            transfer: Transfer::default(),
            baseline_exposure: 0.0,
            width: 0,
            height: 0,
            channels: 0,
            metadata: None,
            ping_pong: 0,
            full: None,
            full_retouched: None,
            preview_gen: 0,
            detail_ping_pong: 0,
        }
    }
}

pub struct FullFrame {
    pub data: Vec<u16>,
    pub width: usize,
    pub height: usize,
    pub transfer: Transfer,
    pub baseline_exposure: f32,
}

/// Longest edge of the interactive preview buffer. A half-size decode of a 100 MP
/// GFX file is 25 MP, far more than any display shows at once; pixel-exact views
/// render from the full decode instead.
pub const PREVIEW_MAX_EDGE: u32 = 3840;

/// Scales a half-size decode down to [`PREVIEW_MAX_EDGE`] when it is larger.
pub fn cap_preview(data: &[u16], width: u32, height: u32, channels: u32) -> (Vec<u16>, u32, u32) {
    let long = width.max(height);
    if long <= PREVIEW_MAX_EDGE || !(channels == 3 || channels == 4) {
        return (data.to_vec(), width, height);
    }
    let scale = PREVIEW_MAX_EDGE as f64 / long as f64;
    let (nw, nh) = (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    );
    let filter = image::imageops::FilterType::Triangle;
    let out = if channels == 3 {
        image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(width, height, data.to_vec())
            .map(|i| image::imageops::resize(&i, nw, nh, filter).into_raw())
    } else {
        image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(width, height, data.to_vec())
            .map(|i| image::imageops::resize(&i, nw, nh, filter).into_raw())
    };
    match out {
        Some(v) => (v, nw, nh),
        None => (data.to_vec(), width, height),
    }
}

/// Decodes `path` at full resolution on a background thread and stores it in the cache
/// if that photo is still the current one.
pub fn spawn_full_decode(cache: Arc<Mutex<DaemonCache>>, path: String) {
    // One full-resolution decode at a time: a 100 MP frame needs over a gigabyte, and
    // stepping through a folder must not start one per photo passed on the way.
    static FULL_DECODE: Mutex<()> = Mutex::new(());
    std::thread::spawn(move || {
        let still_current = || {
            let c = cache.lock().unwrap_or_else(|e| e.into_inner());
            c.current_path.as_deref() == Some(path.as_str())
        };
        let _turn = FULL_DECODE.lock().unwrap_or_else(|e| e.into_inner());
        if !still_current() {
            return;
        }
        let Ok(raw) = RawImage::open(&path) else { return };
        let Ok(full) = raw.process_full_16(3) else { return };
        let baseline_exposure = raw.baseline_exposure_from(&full);
        drop(raw);
        if full.channels != 3 {
            return;
        }
        let frame = FullFrame {
            width: full.width as usize,
            height: full.height as usize,
            transfer: full.transfer,
            baseline_exposure,
            data: full.to_vec_u16(),
        };
        let ready = FullReady { path: path.clone(), width: frame.width, height: frame.height };
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.current_path.as_deref() != Some(path.as_str()) {
            return;
        }
        c.full = Some(Arc::new(frame));
        c.full_retouched = None;
        drop(c);
        print_json(&ResponseWrapper::ok_action("full_ready", ready).with_context(None, Some(path)));
    });
}

/// The full-resolution frame with `spots` applied, reusing the last result while the
/// frame and the spots stay the same.
pub fn retouched_full(
    cache: &Mutex<DaemonCache>,
    full: Arc<FullFrame>,
    spots: &[pipeline::retouch::RetouchSpot],
) -> Arc<FullFrame> {
    if spots.is_empty() {
        return full;
    }
    if let Some((base, cached_spots, done)) = &cache.lock().unwrap_or_else(|e| e.into_inner()).full_retouched
        && Arc::ptr_eq(base, &full)
        && cached_spots.as_slice() == spots
    {
        return done.clone();
    }
    let done = match pipeline::retouch::apply(&full.data, full.width, full.height, 3, spots) {
        std::borrow::Cow::Borrowed(_) => return full,
        std::borrow::Cow::Owned(data) => Arc::new(FullFrame { data, ..*full }),
    };
    cache.lock().unwrap_or_else(|e| e.into_inner()).full_retouched = Some((full, spots.to_vec(), done.clone()));
    done
}

/// Pixels of `frame` scaled by `scale`, for the rectangle (x, y, w, h) of the scaled
/// frame: box-filtered when reducing, bilinear when enlarging.
pub fn resample_rect(frame: &FullFrame, scale: f64, rect: (usize, usize, usize, usize)) -> Vec<u16> {
    let (rx, ry, rw, rh) = rect;
    let (fw, fh) = (frame.width, frame.height);
    let mut out = vec![0u16; rw * rh * 3];
    out.par_chunks_mut(rw * 3).enumerate().for_each(|(j, row)| {
        let vy = ry + j;
        for i in 0..rw {
            let vx = rx + i;
            let px = &mut row[i * 3..i * 3 + 3];
            if scale < 1.0 {
                let (x0, x1) = (
                    (vx as f64 / scale) as usize,
                    (((vx + 1) as f64 / scale) as usize).max((vx as f64 / scale) as usize + 1),
                );
                let (y0, y1) = (
                    (vy as f64 / scale) as usize,
                    (((vy + 1) as f64 / scale) as usize).max((vy as f64 / scale) as usize + 1),
                );
                let (x1, y1) = (x1.min(fw), y1.min(fh));
                let (x0, y0) = (x0.min(x1.saturating_sub(1)), y0.min(y1.saturating_sub(1)));
                let mut sum = [0u64; 3];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let k = (y * fw + x) * 3;
                        for (acc, &v) in sum.iter_mut().zip(&frame.data[k..k + 3]) {
                            *acc += v as u64;
                        }
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)).max(1) as u64;
                for (p, acc) in px.iter_mut().zip(sum) {
                    *p = (acc / n) as u16;
                }
            } else {
                let sx = ((vx as f64 + 0.5) / scale - 0.5).clamp(0.0, (fw - 1) as f64);
                let sy = ((vy as f64 + 0.5) / scale - 0.5).clamp(0.0, (fh - 1) as f64);
                let (x0, y0) = (sx as usize, sy as usize);
                let (x1, y1) = ((x0 + 1).min(fw - 1), (y0 + 1).min(fh - 1));
                let (fx, fy) = (sx - x0 as f64, sy - y0 as f64);
                for (c, p) in px.iter_mut().enumerate() {
                    let at = |y: usize, x: usize| frame.data[(y * fw + x) * 3 + c] as f64;
                    let v = (at(y0, x0) * (1.0 - fx) + at(y0, x1) * fx) * (1.0 - fy)
                        + (at(y1, x0) * (1.0 - fx) + at(y1, x1) * fx) * fy;
                    *p = v.round() as u16;
                }
            }
        }
    });
    out
}
