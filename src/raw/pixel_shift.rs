//! Pixel shift: frames taken with the sensor moved by whole and half photosites,
//! merged so that every output pixel holds measured red, green and blue (no
//! demosaicing) and, with half-photosite steps, twice the linear resolution.
//!
//! A frame's photosite (row i, column j) saw the scene at (j + sx * ox, i + sy * oy),
//! where (ox, oy) is the sensor offset the camera recorded for that frame and (sx, sy)
//! the direction convention, found by [`best_sign`]. With half-photosite offsets the
//! output grid has twice the density; each of its points then lines up with exactly
//! one photosite of every frame in the matching group of four whole-step frames, which
//! between them cover red, both greens and blue.

use rayon::prelude::*;

use super::demosaic::CfaPattern;

/// One frame: normalised photosites (1.0 = clipping) and the camera's offset for it.
pub struct ShiftFrame<'a> {
    pub width: usize,
    pub height: usize,
    pub data: &'a [f32],
    pub pattern: CfaPattern,
    /// Position of `data`'s first photosite on the sensor, for the CFA phase.
    pub origin: (usize, usize),
    /// Sensor offset of this frame in photosites (x, y).
    pub offset: (f32, f32),
}

impl ShiftFrame<'_> {
    #[inline(always)]
    fn color(&self, i: usize, j: usize) -> usize {
        self.pattern.color(i + self.origin.1, j + self.origin.0)
    }
}

/// Output grid shared by all frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// 1 for whole-photosite offsets, 2 when half steps are present.
    pub scale: usize,
    /// Direction convention of the recorded offsets.
    pub sign: (f32, f32),
    pub width: usize,
    pub height: usize,
    x_min: f32,
    y_min: f32,
}

impl Layout {
    /// Grid covering the part of the scene that every frame saw.
    pub fn new(offsets: &[(f32, f32)], width: usize, height: usize, sign: (f32, f32)) -> Result<Self, String> {
        if offsets.is_empty() || width < 4 || height < 4 {
            return Err("No frames to combine".to_string());
        }
        for &(x, y) in offsets {
            for v in [x, y] {
                if !v.is_finite() || v.abs() > 8.0 || (v * 2.0 - (v * 2.0).round()).abs() > 0.05 {
                    return Err(format!("Unsupported pixel shift offset {v}: only whole and half photosite steps"));
                }
            }
        }
        let half = offsets.iter().any(|&(x, y)| (x - x.round()).abs() > 0.25 || (y - y.round()).abs() > 0.25);
        let scale = if half { 2 } else { 1 };
        let xs: Vec<f32> = offsets.iter().map(|o| sign.0 * o.0).collect();
        let ys: Vec<f32> = offsets.iter().map(|o| sign.1 * o.1).collect();
        let (x_min, x_max) = (xs.iter().copied().fold(f32::MIN, f32::max), (width - 1) as f32 + xs.iter().copied().fold(f32::MAX, f32::min));
        let (y_min, y_max) = (ys.iter().copied().fold(f32::MIN, f32::max), (height - 1) as f32 + ys.iter().copied().fold(f32::MAX, f32::min));
        if x_max <= x_min || y_max <= y_min {
            return Err("Pixel shift offsets leave no common area".to_string());
        }
        let span = |lo: f32, hi: f32| ((hi - lo) * scale as f32 + 1e-3).floor() as usize + 1;
        Ok(Self { scale, sign, width: span(x_min, x_max), height: span(y_min, y_max), x_min, y_min })
    }

    /// Photosite index of output coordinate `p` in a frame offset by `o`, if it lines up.
    #[inline(always)]
    fn source(&self, p: usize, min: f32, sign: f32, o: f32, len: usize) -> Option<usize> {
        let s = min + p as f32 / self.scale as f32 - sign * o;
        let r = s.round();
        ((s - r).abs() < 0.01 && r >= 0.0 && (r as usize) < len).then_some(r as usize)
    }
}

/// Running per-channel mean of the photosites that land on each output pixel.
pub struct Combiner {
    pub layout: Layout,
    /// Interleaved RGB, 0..=65535 for 0..=1.
    rgb: Vec<u16>,
    /// Samples per channel: red bits 0-1, green 2-5, blue 6-7 (saturating).
    count: Vec<u8>,
}

const COUNT_SHIFT: [u32; 3] = [0, 2, 6];
const COUNT_MAX: [u8; 3] = [3, 15, 3];

impl Combiner {
    pub fn new(layout: Layout) -> Self {
        let n = layout.width * layout.height;
        Self { layout, rgb: vec![0; n * 3], count: vec![0; n] }
    }

    /// Adds one frame (its offset must be one of those the layout was built from).
    pub fn add(&mut self, frame: &ShiftFrame) {
        let l = self.layout;
        let (ox, oy) = frame.offset;
        self.rgb.par_chunks_mut(l.width * 3).zip(self.count.par_chunks_mut(l.width)).enumerate().for_each(|(y, (row, cnt))| {
            let Some(i) = l.source(y, l.y_min, l.sign.1, oy, frame.height) else { return };
            for x in 0..l.width {
                let Some(j) = l.source(x, l.x_min, l.sign.0, ox, frame.width) else { continue };
                let c = frame.color(i, j);
                let v = frame.data[i * frame.width + j].clamp(0.0, 1.0) * 65535.0;
                let k = (cnt[x] >> COUNT_SHIFT[c]) & COUNT_MAX[c];
                let old = row[x * 3 + c] as f32;
                row[x * 3 + c] = (old + (v - old) / (k as f32 + 1.0)).round() as u16;
                if k < COUNT_MAX[c] {
                    cnt[x] += 1 << COUNT_SHIFT[c];
                }
            }
        });
    }

    /// Share of output pixels that received every colour.
    pub fn coverage(&self) -> f32 {
        let full = self.count.par_iter().filter(|&&k| (0..3).all(|c| (k >> COUNT_SHIFT[c]) & COUNT_MAX[c] > 0)).count();
        full as f32 / self.count.len().max(1) as f32
    }

    /// Fills colours no frame measured from their neighbours, then converts every pixel
    /// with `matrix` (white-balanced camera RGB to the output space). Returns
    /// interleaved 16-bit RGB of `layout.width` x `layout.height`.
    pub fn finish(mut self, matrix: [[f32; 3]; 3]) -> Vec<u16> {
        let (w, h) = (self.layout.width, self.layout.height);
        let has = |count: &[u8], i: usize, c: usize| (count[i] >> COUNT_SHIFT[c]) & COUNT_MAX[c] > 0;
        let (rgb, count) = (&self.rgb, &self.count);
        let fills: Vec<(usize, u16)> = (0..w * h)
            .into_par_iter()
            .flat_map_iter(|i| {
                (0..3).filter(move |&c| !has(count, i, c)).filter_map(move |c| {
                    let (y, x) = (i / w, i % w);
                    for r in 1..=3usize {
                        let (mut sum, mut n) = (0u32, 0u32);
                        for yy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                            for xx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                                let j = yy * w + xx;
                                if has(count, j, c) {
                                    sum += rgb[j * 3 + c] as u32;
                                    n += 1;
                                }
                            }
                        }
                        if let Some(mean) = sum.checked_div(n) {
                            return Some((i * 3 + c, mean as u16));
                        }
                    }
                    None
                })
            })
            .collect();
        for (k, v) in fills {
            self.rgb[k] = v;
        }
        self.rgb.par_chunks_mut(3).for_each(|p| {
            let v = [p[0] as f32, p[1] as f32, p[2] as f32];
            for (r, m) in matrix.iter().enumerate() {
                p[r] = (m[0] * v[0] + m[1] * v[1] + m[2] * v[2]).round().clamp(0.0, 65535.0) as u16;
            }
        });
        self.rgb
    }
}

/// Merges frames that are all in memory.
pub fn combine(frames: &[ShiftFrame], sign: (f32, f32), matrix: [[f32; 3]; 3]) -> Result<(Vec<u16>, Layout), String> {
    let first = frames.first().ok_or("No frames to combine")?;
    let offsets: Vec<_> = frames.iter().map(|f| f.offset).collect();
    let layout = Layout::new(&offsets, first.width, first.height, sign)?;
    let mut comb = Combiner::new(layout);
    for f in frames {
        if (f.width, f.height) != (first.width, first.height) || f.pattern != first.pattern {
            return Err("Pixel shift frames differ in size or sensor layout".to_string());
        }
        comb.add(f);
    }
    Ok((comb.finish(matrix), layout))
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Picks the direction convention of the recorded offsets: with the wrong one, colours
/// are gathered from photosites a step or two apart and edges turn into colour
/// fringes. `frames` are crops of the same sensor area of every frame.
pub fn best_sign(frames: &[ShiftFrame]) -> Result<(f32, f32), String> {
    let mut best: Option<(f64, (f32, f32))> = None;
    for sign in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        let (rgb, l) = combine(frames, sign, IDENTITY)?;
        let chroma = |i: usize| {
            let (r, g, b) = (rgb[i * 3] as f64, rgb[i * 3 + 1] as f64, rgb[i * 3 + 2] as f64);
            (r - g, b - g)
        };
        let mut energy = 0f64;
        for y in 0..l.height - 1 {
            for x in 0..l.width - 1 {
                let i = y * l.width + x;
                let (a, b) = chroma(i);
                for j in [i + 1, i + l.width] {
                    let (c, d) = chroma(j);
                    energy += (a - c).abs() + (b - d).abs();
                }
            }
        }
        if best.is_none_or(|(e, _)| energy < e) {
            best = Some((energy, sign));
        }
    }
    Ok(best.map(|(_, s)| s).unwrap_or((1.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RGGB
    const BAYER: CfaPattern = CfaPattern::Bayer(0x94949494);

    /// A scene with colour edges, sampled on a grid `res` times finer than the sensor.
    fn scene(x: f32, y: f32) -> [f32; 3] {
        let stripe = ((x * 0.7).sin() * (y * 0.45).cos() > 0.0) as u8 as f32;
        let disc = (((x - 30.0).powi(2) + (y - 22.0).powi(2)) < 140.0) as u8 as f32;
        [0.2 + 0.6 * stripe, 0.3 + 0.4 * disc, 0.15 + 0.5 * (1.0 - stripe) * disc + 0.1 * (x / 64.0)]
    }

    /// Frames as the camera would record them: photosite (i, j) sees scene point
    /// (j + sx * ox, i + sy * oy).
    fn shoot(offsets: &[(f32, f32)], sign: (f32, f32), w: usize, h: usize) -> Vec<Vec<f32>> {
        offsets
            .iter()
            .map(|&(ox, oy)| {
                let mut d = vec![0f32; w * h];
                for i in 0..h {
                    for j in 0..w {
                        let p = scene(j as f32 + sign.0 * ox, i as f32 + sign.1 * oy);
                        d[i * w + j] = p[BAYER.color(i, j)];
                    }
                }
                d
            })
            .collect()
    }

    fn frames<'a>(data: &'a [Vec<f32>], offsets: &[(f32, f32)], w: usize, h: usize) -> Vec<ShiftFrame<'a>> {
        data.iter()
            .zip(offsets)
            .map(|(d, &offset)| ShiftFrame { width: w, height: h, data: d, pattern: BAYER, origin: (0, 0), offset })
            .collect()
    }

    fn four() -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
    }

    fn sixteen() -> Vec<(f32, f32)> {
        let mut v = Vec::new();
        for half in [(0.0, 0.0), (0.5, 0.0), (0.5, 0.5), (0.0, 0.5)] {
            for whole in four() {
                v.push((whole.0 + half.0, whole.1 + half.1));
            }
        }
        v
    }

    #[test]
    fn four_shots_give_true_colour_everywhere() {
        let (w, h) = (64, 48);
        let offs = four();
        let data = shoot(&offs, (1.0, 1.0), w, h);
        let (rgb, l) = combine(&frames(&data, &offs, w, h), (1.0, 1.0), IDENTITY).unwrap();
        assert_eq!((l.scale, l.width, l.height), (1, w - 1, h - 1));
        for y in 0..l.height {
            for x in 0..l.width {
                let truth = scene(x as f32 + 1.0, y as f32 + 1.0);
                for c in 0..3 {
                    let v = rgb[(y * l.width + x) * 3 + c] as f32 / 65535.0;
                    assert!((v - truth[c]).abs() < 1e-3, "({x},{y}) c{c}: {v} vs {}", truth[c]);
                }
            }
        }
    }

    #[test]
    fn sixteen_shots_double_the_resolution() {
        let (w, h) = (40, 30);
        let offs = sixteen();
        let data = shoot(&offs, (1.0, 1.0), w, h);
        let fs = frames(&data, &offs, w, h);
        let layout = Layout::new(&offs, w, h, (1.0, 1.0)).unwrap();
        let mut comb = Combiner::new(layout);
        for f in &fs {
            comb.add(f);
        }
        assert_eq!(comb.coverage(), 1.0);
        let rgb = comb.finish(IDENTITY);
        assert_eq!(layout.scale, 2);
        for y in 0..layout.height {
            for x in 0..layout.width {
                let truth = scene(1.5 + x as f32 / 2.0, 1.5 + y as f32 / 2.0);
                for c in 0..3 {
                    let v = rgb[(y * layout.width + x) * 3 + c] as f32 / 65535.0;
                    assert!((v - truth[c]).abs() < 1e-3, "({x},{y}) c{c}: {v} vs {}", truth[c]);
                }
            }
        }
    }

    #[test]
    fn offset_direction_is_detected() {
        let (w, h) = (64, 48);
        for truth in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
            let offs = sixteen();
            let data = shoot(&offs, truth, w, h);
            assert_eq!(best_sign(&frames(&data, &offs, w, h)).unwrap(), truth);
        }
    }

    #[test]
    fn unsupported_offsets_are_refused() {
        assert!(Layout::new(&[(0.0, 0.0), (0.3, 0.0)], 10, 10, (1.0, 1.0)).is_err());
        assert!(Layout::new(&[], 10, 10, (1.0, 1.0)).is_err());
    }
}
