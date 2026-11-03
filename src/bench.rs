//! `omastudio-engine bench [photo] [--runs N]`: times the preview pipeline stage by
//! stage, so GPU work goes where the time is. Each configuration turns on one more
//! group of modules than the one before, and the difference between two rows is
//! what that group costs. Without a photo a 3840×2560 synthetic frame is used.

use std::time::{Duration, Instant};

use serde::Serialize;

use omastudio_engine::pipeline::{self, transfer::Transfer};
use omastudio_engine::raw::RawImage;
use omastudio_engine::recipe::Recipe;

#[derive(Serialize)]
pub struct BenchRow {
    pub stage: &'static str,
    pub interactive: bool,
    pub median_ms: f64,
    pub min_ms: f64,
}

#[derive(Serialize)]
pub struct BenchReport {
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub threads: usize,
    pub gpu: Option<String>,
    pub decode_ms: Option<f64>,
    pub runs: usize,
    pub rows: Vec<BenchRow>,
}

/// The cumulative recipes, from neutral to everything on.
fn stages() -> Vec<(&'static str, Recipe)> {
    let mut r = Recipe::default();
    let mut out = vec![("neutral", r.clone())];

    r.exposure = 0.3;
    r.contrast = 20.0;
    r.highlights = -30.0;
    r.shadows = 25.0;
    r.vibrance = 15.0;
    r.saturation = 5.0;
    r.curve_points = vec![[0.0, 0.0], [0.25, 0.2], [0.75, 0.8], [1.0, 1.0]];
    r.hsl_sat[3] = 10.0;
    r.hsl_hue[4] = -8.0;
    r.film_simulation = "fuji_classic_chrome".into();
    out.push(("tone_colour", r.clone()));

    r.clarity = 25.0;
    r.texture = 20.0;
    r.dehaze = 10.0;
    out.push(("presence", r.clone()));

    r.sharpness = 40.0;
    r.denoise_lum = 30.0;
    r.denoise_col = 30.0;
    out.push(("detail", r.clone()));

    r.grain_amount = 25.0;
    r.vignette = -20.0;
    out.push(("grain_vignette", r.clone()));
    out
}

fn synthetic(w: usize, h: usize) -> Vec<u16> {
    // Smooth gradients plus hashed noise, so blurs and LUTs see realistic variety.
    // Values stay below ~40% linear like most of a real exposure; the brightest
    // highlights take the exact (unbaked) path in interactive previews.
    let mut buf = vec![0u16; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let n = ((x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663)) % 2048;
            let i = (y * w + x) * 3;
            buf[i] = ((x * 24_000 / w) as u32 + n) as u16;
            buf[i + 1] = ((y * 24_000 / h) as u32 + n) as u16;
            buf[i + 2] = (((x + y) * 20_000 / (w + h)) as u32 + n) as u16;
        }
    }
    buf
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e5).round() / 100.0
}

pub fn run(photo: Option<&str>, runs: usize) -> Result<BenchReport, String> {
    let runs = runs.clamp(1, 50);
    let (source, buf, w, h, ch, transfer, decode_ms) = match photo {
        Some(p) => {
            let t = Instant::now();
            let raw = RawImage::open(p)?;
            let preview = raw.process_preview_16(true)?;
            let d = ms(t.elapsed());
            let (w, h, ch, tr) = (preview.width, preview.height, preview.channels, preview.transfer);
            (p.to_string(), preview.as_slice_u16().to_vec(), w, h, ch, tr, Some(d))
        }
        None => {
            let (w, h) = (3840usize, 2560usize);
            ("synthetic".to_string(), synthetic(w, h), w as u32, h as u32, 3, Transfer::bt709(), None)
        }
    };

    let mut rows = Vec::new();
    for interactive in [true, false] {
        pipeline::set_interactive(interactive);
        for (stage, recipe) in stages() {
            // One warm-up run fills the film simulation and LUT caches
            let _ = pipeline::process_buffer_16_to_8_ex(&buf, w, h, ch, &recipe, transfer, false, false);
            let mut times: Vec<Duration> = (0..runs)
                .map(|_| {
                    let t = Instant::now();
                    let _ = pipeline::process_buffer_16_to_8_ex(&buf, w, h, ch, &recipe, transfer, false, false);
                    t.elapsed()
                })
                .collect();
            times.sort();
            rows.push(BenchRow { stage, interactive, median_ms: ms(times[times.len() / 2]), min_ms: ms(times[0]) });
        }
    }
    pipeline::set_interactive(false);

    Ok(BenchReport {
        source,
        width: w,
        height: h,
        threads: rayon::current_num_threads(),
        gpu: omastudio_engine::gpu::get().map(|g| g.info.name.clone()),
        decode_ms,
        runs,
        rows,
    })
}
