//! Compares OmaStudio's own development with LibRaw's on a real RAW file.
//! Run with `OMASTUDIO_SAMPLE_RAW=/path/to/file.RAF cargo test --release --test demosaic_compare -- --ignored --nocapture`.

use omastudio_engine::raw::{DemosaicEngine, RawImage};
use std::time::Instant;

fn psnr(a: &[u16], b: &[u16], w: usize, h: usize, border: usize) -> [f64; 3] {
    let mut se = [0f64; 3];
    let mut n = 0f64;
    for y in border..h - border {
        for x in border..w - border {
            let i = (y * w + x) * 3;
            // Skip clipped areas, where highlight handling (not demosaicing) differs
            if a[i..i + 3].iter().chain(&b[i..i + 3]).any(|&v| v > 60000) {
                continue;
            }
            for c in 0..3 {
                let d = a[i + c] as f64 - b[i + c] as f64;
                se[c] += d * d;
            }
            n += 1.0;
        }
    }
    se.map(|e| 10.0 * (65535f64 * 65535.0 / (e / n)).log10())
}

#[test]
#[ignore]
fn own_development_tracks_libraw() {
    let Ok(path) = std::env::var("OMASTUDIO_SAMPLE_RAW") else {
        eprintln!("OMASTUDIO_SAMPLE_RAW not set, skipping");
        return;
    };
    for half in [true, false] {
        let raw = RawImage::open(&path).unwrap();
        // Warm the page cache and LibRaw's unpack so timings compare processing only
        let _ = raw.get_metadata();
        let t = Instant::now();
        let ours = raw.process_with(DemosaicEngine::Own, half, 3, 16).unwrap();
        let t_ours = t.elapsed();
        let raw = RawImage::open(&path).unwrap();
        let t = Instant::now();
        let lib = raw.process_with(DemosaicEngine::LibRaw, half, 3, 16).unwrap();
        let t_lib = t.elapsed();
        println!(
            "half={half}: ours {}x{} in {:?}, LibRaw {}x{} in {:?}",
            ours.width, ours.height, t_ours, lib.width, lib.height, t_lib
        );
        if (ours.width, ours.height) == (lib.width, lib.height) {
            let p = psnr(ours.as_slice_u16(), lib.as_slice_u16(), ours.width as usize, ours.height as usize, 8);
            println!("  PSNR vs LibRaw (R, G, B): {:.1} {:.1} {:.1} dB", p[0], p[1], p[2]);
            if let (false, Ok(dir)) = (half, std::env::var("OMASTUDIO_DUMP_DIR")) {
                dump_crops(&dir, ours.as_slice_u16(), lib.as_slice_u16(), ours.width as usize, ours.height as usize);
            }
        }
        let mean = |b: &[u16]| -> [f64; 3] {
            let mut s = [0f64; 3];
            for px in b.chunks(3) {
                for c in 0..3 {
                    s[c] += px[c] as f64;
                }
            }
            s.map(|v| v / (b.len() / 3) as f64)
        };
        println!("  mean ours {:?} LibRaw {:?}", mean(ours.as_slice_u16()), mean(lib.as_slice_u16()));
    }
}

/// Writes side-by-side (ours | LibRaw) sRGB crops of the most detailed areas, 2x enlarged.
fn dump_crops(dir: &str, ours: &[u16], lib: &[u16], w: usize, h: usize) {
    let t = omastudio_engine::pipeline::transfer::Transfer::srgb();
    let (cw, ch) = (160usize, 120usize);
    // Rank tiles by luminance detail
    let mut tiles = Vec::new();
    for ty in (0..h - ch).step_by(ch) {
        for tx in (0..w - cw).step_by(cw) {
            let mut acc = 0f64;
            for y in (ty + 1..ty + ch).step_by(2) {
                for x in (tx + 1..tx + cw).step_by(2) {
                    let i = (y * w + x) * 3 + 1;
                    acc += (lib[i] as f64 - lib[i - 3] as f64).abs() + (lib[i] as f64 - lib[i - w * 3] as f64).abs();
                }
            }
            tiles.push((acc, tx, ty));
        }
    }
    tiles.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (n, &(_, tx, ty)) in tiles.iter().take(4).enumerate() {
        let mut img = image::RgbImage::new((cw * 2 + 4) as u32 * 2, ch as u32 * 2);
        for (k, src) in [ours, lib].iter().enumerate() {
            for y in 0..ch {
                for x in 0..cw {
                    let i = ((ty + y) * w + tx + x) * 3;
                    let px = image::Rgb(std::array::from_fn(|c| (t.encode(src[i + c] as f32 / 65535.0 * 2.0).clamp(0.0, 1.0) * 255.0) as u8));
                    for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        img.put_pixel(((k * (cw + 4) + x) * 2 + dx) as u32, (y * 2 + dy) as u32, px);
                    }
                }
            }
        }
        img.save(format!("{dir}/crop{n}.png")).unwrap();
    }
}
