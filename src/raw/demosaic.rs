//! Demosaicing of Bayer and X-Trans sensor data.
//!
//! The full-resolution path is an edge-directed colour-difference interpolator that
//! works on any repeating CFA layout:
//!
//! 1. Green at red/blue sites is estimated along four directions (horizontal,
//!    vertical and both diagonals). Each estimate interpolates the nearest greens on
//!    either side and adds a curvature correction from same-colour samples (the
//!    Hamilton-Adams term for Bayer). Estimates are blended with weights that fall
//!    with the gradient along their direction, so interpolation follows edges.
//! 2. Red and blue are rebuilt from colour differences (R-G, B-G), which vary much
//!    more slowly than the colours themselves, averaged over nearby samples of that
//!    colour and weighted against green edges.
//! 3. A 3x3 median on the colour differences removes isolated false colour.
//!
//! The preview path bins each 2x2 (Bayer) or 3x3 (X-Trans) cell into one RGB pixel,
//! which needs no interpolation at all.

use rayon::prelude::*;

/// Colour filter layout of a sensor, in visible-area coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CfaPattern {
    /// LibRaw/dcraw `filters` bits for a 2x2 (or 2x8) Bayer layout.
    Bayer(u32),
    /// 6x6 X-Trans layout.
    XTrans([[u8; 6]; 6]),
}

impl CfaPattern {
    /// Colour (0 red, 1 green, 2 blue) of the photosite at `row`, `col`.
    #[inline(always)]
    pub fn color(&self, row: usize, col: usize) -> usize {
        match self {
            Self::Bayer(filters) => {
                let c = (filters >> ((((row << 1) & 14) | (col & 1)) << 1)) & 3;
                // Second green of 4-colour notation is still green
                if c == 3 { 1 } else { c as usize }
            }
            Self::XTrans(xt) => xt[row % 6][col % 6] as usize,
        }
    }

    /// Side of the square cell [`bin`] averages into one pixel.
    pub fn bin_size(&self) -> usize {
        match self {
            Self::Bayer(_) => 2,
            Self::XTrans(_) => 3,
        }
    }
}

/// White-balanced, normalised (0..1) photosite values of the visible area.
pub struct Mosaic<'a> {
    pub width: usize,
    pub height: usize,
    pub data: &'a [f32],
    pub pattern: CfaPattern,
}

/// Averages each Bayer 2x2 / X-Trans 3x3 cell into one RGB pixel (interleaved).
pub fn bin(m: &Mosaic) -> (Vec<f32>, usize, usize) {
    let s = m.pattern.bin_size();
    let (w, h) = (m.width / s, m.height / s);
    let mut out = vec![0f32; w * h * 3];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(by, row)| {
        for bx in 0..w {
            let mut sum = [0f32; 3];
            let mut n = [0u32; 3];
            for dy in 0..s {
                for dx in 0..s {
                    let (y, x) = (by * s + dy, bx * s + dx);
                    let c = m.pattern.color(y, x);
                    sum[c] += m.data[y * m.width + x];
                    n[c] += 1;
                }
            }
            for c in 0..3 {
                row[bx * 3 + c] = if n[c] > 0 { sum[c] / n[c] as f32 } else { 0.0 };
            }
        }
    });
    (out, w, h)
}

const DIRECTIONS: [(isize, isize); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];
/// How far (in photosites) to look for a green or same-colour neighbour.
const REACH: isize = 3;
const EPS: f32 = 1e-4;
/// Rows/columns of output computed per work item.
const TILE: usize = 96;
/// Extra photosites read around a tile: 3 for the green taps reach, 2 for the
/// colour-difference window, 1 for homogeneity and 2 for its sum.
const BORDER: usize = 8;

/// Along one direction from a non-green site: nearest greens at +k and -k, and the
/// nearest same-colour sites at an equal distance on both sides (for curvature).
#[derive(Clone, Copy)]
struct Taps {
    plus: isize,
    minus: isize,
    same: Option<isize>,
}

/// Interpolation recipe for one position of the repeating CFA cell.
struct Phase {
    color: usize,
    taps: [Option<Taps>; 4],
    /// Photosites of red (index 0) and blue (index 1) near this site: (dy, dx, 1/dist^2).
    window: [Vec<(isize, isize, f32)>; 2],
}

struct Tables {
    period: usize,
    phases: Vec<Phase>,
}

impl Tables {
    fn new(pattern: CfaPattern) -> Self {
        let period = match pattern {
            CfaPattern::Bayer(_) => 2,
            CfaPattern::XTrans(_) => 6,
        };
        // Work in the middle of a large virtual pattern so offsets never go negative
        let base = (period * 4) as isize;
        let color = |y: isize, x: isize| pattern.color((base + y) as usize, (base + x) as usize);
        let radius: isize = if period == 2 { 1 } else { 2 };
        let mut phases = Vec::with_capacity(period * period);
        for py in 0..period as isize {
            for px in 0..period as isize {
                let c = color(py, px);
                let find = |dir: (isize, isize), sign: isize, want: usize| {
                    (1..=REACH).find(|&k| color(py + sign * k * dir.0, px + sign * k * dir.1) == want)
                };
                let taps = DIRECTIONS.map(|dir| {
                    if c == 1 {
                        return None;
                    }
                    let (plus, minus) = (find(dir, 1, 1)?, find(dir, -1, 1)?);
                    let same = match (find(dir, 1, c), find(dir, -1, c)) {
                        (Some(a), Some(b)) if a == b => Some(a),
                        _ => None,
                    };
                    Some(Taps { plus, minus, same })
                });
                let window = [0usize, 2].map(|t| {
                    let mut v = Vec::new();
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            if (dy, dx) != (0, 0) && color(py + dy, px + dx) == t {
                                v.push((dy, dx, 1.0 / (dy * dy + dx * dx) as f32));
                            }
                        }
                    }
                    v
                });
                phases.push(Phase { color: c, taps, window });
            }
        }
        Self { period, phases }
    }

    #[inline(always)]
    fn phase(&self, y: usize, x: usize) -> &Phase {
        &self.phases[(y % self.period) * self.period + x % self.period]
    }
}

/// Full-resolution demosaic; returns interleaved linear RGB.
///
/// Every candidate direction is developed into a full RGB image, and per pixel the
/// directions whose result is most homogeneous (fewest abrupt changes in luma and
/// chroma around it) are kept, so interpolation follows edges instead of crossing them.
pub fn demosaic(m: &Mosaic) -> Vec<f32> {
    let (w, h) = (m.width, m.height);
    let tables = Tables::new(m.pattern);
    let mut rgb = vec![0f32; w * h * 3];
    rgb.par_chunks_mut(TILE * w * 3).enumerate().for_each(|(band, out)| {
        let y0 = band * TILE;
        let y1 = (y0 + TILE).min(h);
        for x0 in (0..w).step_by(TILE) {
            let x1 = (x0 + TILE).min(w);
            develop_tile(m, &tables, (y0, y1, x0, x1), out, w);
        }
    });
    suppress_false_color(&mut rgb, w, h);
    rgb
}

fn develop_tile(m: &Mosaic, t: &Tables, (y0, y1, x0, x1): (usize, usize, usize, usize), out: &mut [f32], w: usize) {
    let ey0 = y0.saturating_sub(BORDER);
    let ex0 = x0.saturating_sub(BORDER);
    let ey1 = (y1 + BORDER).min(m.height);
    let ex1 = (x1 + BORDER).min(m.width);
    let (tw, th) = (ex1 - ex0, ey1 - ey0);
    let n = tw * th;
    let inside = |y: isize, x: isize| y >= 0 && x >= 0 && (y as usize) < m.height && (x as usize) < m.width;
    let raw = |y: isize, x: isize| m.data[y as usize * m.width + x as usize];

    // Green per direction; an undefined direction falls back to the blend of the others
    let mut green = [vec![0f32; n], vec![0f32; n], vec![0f32; n], vec![0f32; n]];
    for ly in 0..th {
        for lx in 0..tw {
            let (y, x) = ((ey0 + ly) as isize, (ex0 + lx) as isize);
            let i = ly * tw + lx;
            let p = t.phase(y as usize, x as usize);
            let centre = raw(y, x);
            if p.color == 1 {
                for g in green.iter_mut() {
                    g[i] = centre;
                }
                continue;
            }
            let mut est = [f32::NAN; 4];
            let (mut sum, mut weight) = (0f32, 0f32);
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for (d, dir) in DIRECTIONS.iter().enumerate() {
                let Some(tap) = p.taps[d] else { continue };
                let (yp, xp) = (y + dir.0 * tap.plus, x + dir.1 * tap.plus);
                let (ym, xm) = (y - dir.0 * tap.minus, x - dir.1 * tap.minus);
                if !inside(yp, xp) || !inside(ym, xm) {
                    continue;
                }
                let (gp, gm) = (raw(yp, xp), raw(ym, xm));
                let (kp, km) = (tap.plus as f32, tap.minus as f32);
                let mut e = (gp * km + gm * kp) / (kp + km);
                let mut gradient = (gp - gm).abs() / (kp + km);
                if let Some(s) = tap.same {
                    let (ya, xa, yb, xb) = (y + dir.0 * s, x + dir.1 * s, y - dir.0 * s, x - dir.1 * s);
                    if inside(ya, xa) && inside(yb, xb) {
                        let lap = 2.0 * centre - raw(ya, xa) - raw(yb, xb);
                        let s2 = (s * s) as f32;
                        e += lap * kp * km / s2;
                        gradient += lap.abs() / s2;
                    }
                }
                // Never leave the range of the greens it came from
                e = e.clamp(gp.min(gm), gp.max(gm));
                lo = lo.min(gp.min(gm));
                hi = hi.max(gp.max(gm));
                est[d] = e;
                let wgt = 1.0 / ((EPS + gradient) * (EPS + gradient) * (kp + km));
                sum += wgt * e;
                weight += wgt;
            }
            let blend = if weight > 0.0 {
                (sum / weight).clamp(lo, hi)
            } else {
                // Image corners: no complete direction, average the greens nearby
                let (mut s, mut k) = (0f32, 0f32);
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let (yy, xx) = (y + dy, x + dx);
                        if inside(yy, xx) && m.pattern.color(yy as usize, xx as usize) == 1 {
                            s += raw(yy, xx);
                            k += 1.0;
                        }
                    }
                }
                if k > 0.0 { s / k } else { centre }
            };
            for d in 0..4 {
                green[d][i] = if est[d].is_nan() { blend } else { est[d] };
            }
        }
    }

    let local: Vec<f32> = (ey0..ey1).flat_map(|y| m.data[y * m.width + ex0..y * m.width + ex1].iter().copied()).collect();

    // Red and blue per direction from colour differences against that direction's green.
    // Window offsets are flattened per phase; only sites near the tile edge need the
    // bounds-checked path.
    let flat: Vec<[Vec<(isize, f32)>; 2]> = t
        .phases
        .iter()
        .map(|p| p.window.each_ref().map(|win| win.iter().map(|&(dy, dx, wd)| (dy * tw as isize + dx, wd)).collect()))
        .collect();
    let phase_idx: Vec<usize> = (0..n)
        .map(|i| ((ey0 + i / tw) % t.period) * t.period + (ex0 + i % tw) % t.period)
        .collect();
    let mut rgb = [vec![[0f32; 3]; n], vec![[0f32; 3]; n], vec![[0f32; 3]; n], vec![[0f32; 3]; n]];
    for (d, plane) in rgb.iter_mut().enumerate() {
        let g = &green[d];
        for ly in 0..th {
            let interior_row = ly >= 2 && ly + 2 < th;
            for lx in 0..tw {
                let i = ly * tw + lx;
                let pi = phase_idx[i];
                let p = &t.phases[pi];
                let g0 = g[i];
                let mut px = [0f32, g0, 0f32];
                for (k, c) in [0usize, 2].into_iter().enumerate() {
                    if p.color == c {
                        px[c] = local[i];
                        continue;
                    }
                    let (mut sum, mut weight) = (0f32, 0f32);
                    if interior_row && lx >= 2 && lx + 2 < tw {
                        for &(off, wd) in &flat[pi][k] {
                            let j = (i as isize + off) as usize;
                            let edge = EPS + (g[j] - g0).abs();
                            let wgt = wd / (edge * edge);
                            sum += wgt * (local[j] - g[j]);
                            weight += wgt;
                        }
                    } else {
                        for &(dy, dx, wd) in &p.window[k] {
                            let (ly2, lx2) = (ly as isize + dy, lx as isize + dx);
                            if ly2 < 0 || lx2 < 0 || ly2 as usize >= th || lx2 as usize >= tw {
                                continue;
                            }
                            let j = ly2 as usize * tw + lx2 as usize;
                            let edge = EPS + (g[j] - g0).abs();
                            let wgt = wd / (edge * edge);
                            sum += wgt * (local[j] - g[j]);
                            weight += wgt;
                        }
                    }
                    px[c] = (g0 + if weight > 0.0 { sum / weight } else { 0.0 }).max(0.0);
                }
                plane[i] = px;
            }
        }
    }

    // Perceptual luma / chroma of each candidate
    let ycc = |p: [f32; 3]| {
        let [r, g, b] = p.map(|v| v.max(0.0).sqrt());
        let y = 0.25 * r + 0.5 * g + 0.25 * b;
        [y, r - g, b - g]
    };
    let lab: Vec<Vec<[f32; 3]>> = rgb.iter().map(|plane| plane.iter().map(|&p| ycc(p)).collect()).collect();

    // Homogeneity: neighbours within the tolerance of the smoothest direction
    let mut homo = [vec![0u8; n], vec![0u8; n], vec![0u8; n], vec![0u8; n]];
    let tw_i = tw as isize;
    let dir_off: [isize; 4] = DIRECTIONS.map(|(dy, dx)| dy * tw_i + dx);
    let ring: [isize; 9] = [-tw_i - 1, -tw_i, -tw_i + 1, -1, 0, 1, tw_i - 1, tw_i, tw_i + 1];
    let mut eps_l = vec![0f32; tw];
    let mut eps_c = vec![0f32; tw];
    for ly in 1..th.saturating_sub(1) {
        let row = ly * tw;
        eps_l.fill(f32::MAX);
        eps_c.fill(f32::MAX);
        for d in 0..4 {
            let plane = &lab[d];
            for lx in 1..tw - 1 {
                let i = row + lx;
                let (a, b) = ((i as isize + dir_off[d]) as usize, (i as isize - dir_off[d]) as usize);
                let (c, pa, pb) = (plane[i], plane[a], plane[b]);
                let dl = (c[0] - pa[0]).abs().max((c[0] - pb[0]).abs());
                let dc = chroma_dist(c, pa).max(chroma_dist(c, pb));
                eps_l[lx] = eps_l[lx].min(dl);
                eps_c[lx] = eps_c[lx].min(dc);
            }
        }
        for d in 0..4 {
            let plane = &lab[d];
            let out = &mut homo[d];
            for lx in 1..tw - 1 {
                let i = row + lx;
                let c = plane[i];
                let (el, ec) = (eps_l[lx], eps_c[lx]);
                let mut count = 0u8;
                for off in ring {
                    let q = plane[(i as isize + off) as usize];
                    count += u8::from((c[0] - q[0]).abs() <= el && chroma_dist(c, q) <= ec);
                }
                out[i] = count;
            }
        }
    }

    // 5x5 box sums of homogeneity, separably: rows first, then columns
    let mut score_map = [vec![0u16; n], vec![0u16; n], vec![0u16; n], vec![0u16; n]];
    for d in 0..4 {
        let mut rows = vec![0u16; n];
        for ly in 0..th {
            for lx in 0..tw {
                let (a, b) = (lx.saturating_sub(2), (lx + 2).min(tw - 1));
                rows[ly * tw + lx] = homo[d][ly * tw + a..=ly * tw + b].iter().map(|&v| v as u16).sum();
            }
        }
        for ly in 0..th {
            let (a, b) = (ly.saturating_sub(2), (ly + 2).min(th - 1));
            for lx in 0..tw {
                score_map[d][ly * tw + lx] = (a..=b).map(|yy| rows[yy * tw + lx]).sum();
            }
        }
    }

    // Keep the most homogeneous directions and average them
    for y in y0..y1 {
        let ly = y - ey0;
        let row = &mut out[(y - y0) * w * 3..(y - y0 + 1) * w * 3];
        for x in x0..x1 {
            let lx = x - ex0;
            let i = ly * tw + lx;
            let score: [u16; 4] = std::array::from_fn(|d| score_map[d][i]);
            let best = *score.iter().max().unwrap_or(&0);
            let keep = best - best / 8;
            let mut acc = [0f32; 3];
            let mut k = 0f32;
            for d in 0..4 {
                if score[d] >= keep {
                    for c in 0..3 {
                        acc[c] += rgb[d][i][c];
                    }
                    k += 1.0;
                }
            }
            for c in 0..3 {
                row[x * 3 + c] = acc[c] / k;
            }
        }
    }
}

#[inline(always)]
fn chroma_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// 3x3 median on the colour differences R-G and B-G, which removes isolated false
/// colour without softening luminance detail.
fn suppress_false_color(rgb: &mut [f32], w: usize, h: usize) {
    if w < 3 || h < 3 {
        return;
    }
    let diff: Vec<[f32; 2]> = rgb.par_chunks(3).map(|p| [p[0] - p[1], p[2] - p[1]]).collect();
    rgb.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        if y == 0 || y == h - 1 {
            return;
        }
        for x in 1..w - 1 {
            let g = row[x * 3 + 1];
            for (k, c) in [0usize, 2].into_iter().enumerate() {
                let at = |dy: usize, dx: usize| diff[(y + dy - 1) * w + (x + dx - 1)][k];
                let v = [at(0, 0), at(0, 1), at(0, 2), at(1, 0), at(1, 1), at(1, 2), at(2, 0), at(2, 1), at(2, 2)];
                row[x * 3 + c] = (g + median9(v)).max(0.0);
            }
        }
    });
}

/// Median of nine values with a fixed exchange network (Paeth / Devillard).
#[inline(always)]
fn median9(mut p: [f32; 9]) -> f32 {
    let mut s = |a: usize, b: usize| {
        if p[a] > p[b] {
            p.swap(a, b);
        }
    };
    s(1, 2); s(4, 5); s(7, 8); s(0, 1); s(3, 4); s(6, 7);
    s(1, 2); s(4, 5); s(7, 8); s(0, 3); s(5, 8); s(4, 7);
    s(3, 6); s(1, 4); s(2, 5); s(4, 7); s(4, 2); s(6, 4);
    s(4, 2);
    p[4]
}

#[cfg(test)]
mod tests {
    use super::*;

    const XTRANS: [[u8; 6]; 6] = [
        [1, 1, 0, 1, 1, 2],
        [1, 1, 2, 1, 1, 0],
        [2, 0, 1, 0, 2, 1],
        [1, 1, 2, 1, 1, 0],
        [1, 1, 0, 1, 1, 2],
        [0, 2, 1, 2, 0, 1],
    ];
    /// RGGB
    const BAYER: u32 = 0x94949494;

    fn mosaic_of(pattern: CfaPattern, w: usize, h: usize, f: impl Fn(usize, usize) -> [f32; 3]) -> Vec<f32> {
        let mut data = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                data[y * w + x] = f(y, x)[pattern.color(y, x)];
            }
        }
        data
    }

    #[test]
    fn bayer_rggb_layout() {
        let p = CfaPattern::Bayer(BAYER);
        assert_eq!([p.color(0, 0), p.color(0, 1), p.color(1, 0), p.color(1, 1)], [0, 1, 1, 2]);
    }

    #[test]
    fn flat_colours_are_reproduced() {
        for pattern in [CfaPattern::Bayer(BAYER), CfaPattern::XTrans(XTRANS)] {
            let (w, h) = (36, 24);
            let data = mosaic_of(pattern, w, h, |_, _| [0.6, 0.4, 0.2]);
            let m = Mosaic { width: w, height: h, data: &data, pattern };
            let rgb = demosaic(&m);
            for px in rgb.chunks(3) {
                assert!((px[0] - 0.6).abs() < 1e-4 && (px[1] - 0.4).abs() < 1e-4 && (px[2] - 0.2).abs() < 1e-4, "{pattern:?} {px:?}");
            }
            let (binned, bw, bh) = bin(&m);
            assert_eq!((bw, bh), (w / pattern.bin_size(), h / pattern.bin_size()));
            assert!((binned[0] - 0.6).abs() < 1e-5 && (binned[2] - 0.2).abs() < 1e-5);
        }
    }

    #[test]
    fn smooth_gradients_have_small_error() {
        for pattern in [CfaPattern::Bayer(BAYER), CfaPattern::XTrans(XTRANS)] {
            let (w, h) = (60, 48);
            let truth = |y: usize, x: usize| {
                let (fy, fx) = (y as f32 / h as f32, x as f32 / w as f32);
                [0.2 + 0.5 * fx, 0.3 + 0.3 * fy, 0.1 + 0.4 * fx * fy]
            };
            let data = mosaic_of(pattern, w, h, truth);
            let rgb = demosaic(&Mosaic { width: w, height: h, data: &data, pattern });
            for y in 4..h - 4 {
                for x in 4..w - 4 {
                    let t = truth(y, x);
                    for c in 0..3 {
                        let e = (rgb[(y * w + x) * 3 + c] - t[c]).abs();
                        assert!(e < 0.01, "{pattern:?} ({y},{x}) c{c} err {e}");
                    }
                }
            }
        }
    }

    #[test]
    fn diagonal_edges_do_not_bleed_colour() {
        for pattern in [CfaPattern::Bayer(BAYER), CfaPattern::XTrans(XTRANS)] {
            let (w, h) = (48, 48);
            let truth = |y: usize, x: usize| if x + y < 48 { [0.05; 3] } else { [0.8; 3] };
            let data = mosaic_of(pattern, w, h, truth);
            let rgb = demosaic(&Mosaic { width: w, height: h, data: &data, pattern });
            for y in 4..h - 4 {
                for x in 4..w - 4 {
                    let px = &rgb[(y * w + x) * 3..(y * w + x) * 3 + 3];
                    let chroma = (px[0] - px[1]).abs().max((px[2] - px[1]).abs());
                    assert!(chroma < 0.08, "{pattern:?} ({y},{x}) {px:?}");
                }
            }
        }
    }

    #[test]
    fn sharp_edges_do_not_bleed_colour() {
        // A vertical black/white edge: interpolation across it would create coloured fringes
        for pattern in [CfaPattern::Bayer(BAYER), CfaPattern::XTrans(XTRANS)] {
            let (w, h) = (48, 36);
            let truth = |_: usize, x: usize| if x < w / 2 { [0.05; 3] } else { [0.8; 3] };
            let data = mosaic_of(pattern, w, h, truth);
            let rgb = demosaic(&Mosaic { width: w, height: h, data: &data, pattern });
            for y in 4..h - 4 {
                for x in 4..w - 4 {
                    let px = &rgb[(y * w + x) * 3..(y * w + x) * 3 + 3];
                    let chroma = (px[0] - px[1]).abs().max((px[2] - px[1]).abs());
                    assert!(chroma < 0.05, "{pattern:?} ({y},{x}) {px:?}");
                }
            }
        }
    }
}
