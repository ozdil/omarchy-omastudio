//! Radial lens corrections (distortion, lateral chromatic aberration, vignetting)
//! described at a few radii, as cameras store them in their RAW files.
//!
//! Radii are normalised to the half-diagonal of the image, so the same correction
//! fits a full-size and a half-size decode. Between knots values are interpolated
//! linearly; beyond the last knot the last value holds.

use rayon::prelude::*;

use crate::pipeline::transfer::Transfer;

#[derive(Debug, Clone, PartialEq)]
pub struct RadialCorrection {
    /// Normalised radii, increasing.
    pub knots: Vec<f32>,
    /// Distortion in percent: the output pixel at radius r samples the sensor image
    /// at r * (1 + d / 100). Negative values correct barrel distortion.
    pub distortion: Option<Vec<f32>>,
    /// Extra radial scale of red and blue relative to green (lateral CA), as fractions.
    pub ca: Option<(Vec<f32>, Vec<f32>)>,
    /// Brightness at each knot in percent of the centre (100 = no fall-off).
    pub vignetting: Option<Vec<f32>>,
}

/// Entries of the per-radius table the remap reads.
const TABLE: usize = 2048;

/// Per-channel sampling scale and brightness gain at a radius.
#[derive(Clone, Copy)]
struct Step {
    scale: [f32; 3],
    gain: f32,
}

impl RadialCorrection {
    /// True when there is anything to correct.
    pub fn is_active(&self) -> bool {
        self.distortion.is_some() || self.ca.is_some() || self.vignetting.is_some()
    }

    fn interp(&self, values: &[f32], r: f32, at_centre: f32) -> f32 {
        let (k, v) = (&self.knots, values);
        if k.is_empty() || r <= 0.0 {
            return at_centre;
        }
        if r < k[0] {
            return at_centre + (v[0] - at_centre) * r / k[0];
        }
        for i in 1..k.len() {
            if r <= k[i] {
                let t = (r - k[i - 1]) / (k[i] - k[i - 1]).max(1e-6);
                return v[i - 1] + (v[i] - v[i - 1]) * t;
            }
        }
        v[v.len() - 1]
    }

    fn step(&self, r: f32) -> Step {
        let g = 1.0 + self.distortion.as_ref().map_or(0.0, |d| self.interp(d, r, 0.0) / 100.0);
        let (cr, cb) = self.ca.as_ref().map_or((0.0, 0.0), |(red, blue)| (self.interp(red, r, 0.0), self.interp(blue, r, 0.0)));
        let v = self.vignetting.as_ref().map_or(100.0, |v| self.interp(v, r, 100.0));
        Step { scale: [g + cr, g, g + cb], gain: 100.0 / v.clamp(10.0, 100.0) }
    }

    /// Sampling scale of red, green and blue at normalised radius `r`.
    pub fn sample_scale(&self, r: f32) -> [f32; 3] {
        self.step(r).scale
    }

    /// Brightness gain at normalised radius `r` (1 = none).
    pub fn sample_gain(&self, r: f32) -> f32 {
        self.step(r).gain
    }

    /// Table over r in 0..=1 plus the zoom that keeps every sample inside the frame.
    fn table(&self) -> (Vec<Step>, f32) {
        let steps: Vec<Step> = (0..=TABLE).map(|i| self.step(i as f32 / TABLE as f32)).collect();
        // Pincushion corrections would sample beyond the frame near the edges; zoom in
        // just enough that they don't (barrel corrections never need it)
        let reach = steps
            .iter()
            .enumerate()
            .map(|(i, s)| s.scale.iter().copied().fold(0.0f32, f32::max) * i as f32 / TABLE as f32)
            .fold(0.0f32, f32::max);
        (steps, if reach > 1.0 { 1.0 / reach } else { 1.0 })
    }

    /// Corrects interleaved RGB (3 channels) of `width` x `height`. `to_linear` and
    /// `from_linear` convert a sample to linear light and back for the vignetting gain.
    fn apply<T>(&self, src: &[T], width: usize, height: usize, to_linear: impl Fn(T) -> f32 + Sync, from_linear: impl Fn(f32) -> T + Sync) -> Vec<T>
    where
        T: Copy + Default + Send + Sync,
    {
        let (steps, zoom) = self.table();
        let (cx, cy) = ((width as f32 - 1.0) / 2.0, (height as f32 - 1.0) / 2.0);
        let half_diag = ((width * width + height * height) as f32).sqrt() / 2.0;
        let geometric = self.distortion.is_some() || self.ca.is_some() || zoom != 1.0;
        let mut out = vec![T::default(); width * height * 3];
        out.par_chunks_mut(width * 3).enumerate().for_each(|(y, row)| {
            let dy = y as f32 - cy;
            for x in 0..width {
                let dx = x as f32 - cx;
                let r = (dx * dx + dy * dy).sqrt() / half_diag;
                let pos = (r * TABLE as f32).min(TABLE as f32);
                let i = (pos as usize).min(TABLE - 1);
                let t = pos - i as f32;
                let (a, b) = (steps[i], steps[i + 1]);
                let gain = a.gain + (b.gain - a.gain) * t;
                for c in 0..3 {
                    let v = if geometric {
                        let s = (a.scale[c] + (b.scale[c] - a.scale[c]) * t) * zoom;
                        bilinear(src, width, height, cx + dx * s, cy + dy * s, c, &to_linear)
                    } else {
                        to_linear(src[(y * width + x) * 3 + c])
                    };
                    row[x * 3 + c] = from_linear(v * gain);
                }
            }
        });
        out
    }

    /// Corrects linear 16-bit RGB.
    pub fn apply_linear_u16(&self, src: &[u16], width: usize, height: usize) -> Vec<u16> {
        self.apply(src, width, height, |v| v as f32, |v| v.round().clamp(0.0, 65535.0) as u16)
    }

    /// Corrects 16-bit RGB encoded with `transfer`.
    pub fn apply_encoded_u16(&self, src: &[u16], width: usize, height: usize, transfer: Transfer) -> Vec<u16> {
        self.apply(
            src,
            width,
            height,
            |v| transfer.decode(v as f32 / 65535.0),
            |v| (transfer.encode(v) * 65535.0).round().clamp(0.0, 65535.0) as u16,
        )
    }

    /// Corrects 8-bit RGB encoded with `transfer`.
    pub fn apply_encoded_u8(&self, src: &[u8], width: usize, height: usize, transfer: Transfer) -> Vec<u8> {
        let lut: Vec<f32> = (0..=255).map(|v| transfer.decode(v as f32 / 255.0)).collect();
        self.apply(src, width, height, |v| lut[v as usize], |v| (transfer.encode(v) * 255.0).round().clamp(0.0, 255.0) as u8)
    }
}

#[inline(always)]
fn bilinear<T: Copy>(src: &[T], w: usize, h: usize, x: f32, y: f32, c: usize, to_linear: &impl Fn(T) -> f32) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |yy: usize, xx: usize| to_linear(src[(yy * w + xx) * 3 + c]);
    let top = at(y0, x0) * (1.0 - fx) + at(y0, x1) * fx;
    let bottom = at(y1, x0) * (1.0 - fx) + at(y1, x1) * fx;
    top * (1.0 - fy) + bottom * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knots() -> Vec<f32> {
        vec![0.25, 0.5, 0.75, 1.0]
    }

    #[test]
    fn vignetting_brightens_corners_only() {
        let corr = RadialCorrection { knots: knots(), distortion: None, ca: None, vignetting: Some(vec![100.0, 98.0, 90.0, 80.0]) };
        let (w, h) = (41, 31);
        let src = vec![1000u16; w * h * 3];
        let out = corr.apply_linear_u16(&src, w, h);
        assert_eq!(out[(15 * w + 20) * 3], 1000);
        let corner = out[0];
        // Corner pixel sits at r = 0.97: about 81 % brightness, so a gain near 1.24
        assert!((1220..=1250).contains(&corner), "corner {corner}");
    }

    #[test]
    fn barrel_correction_pulls_corner_detail_outward() {
        // A bright dot near the corner of the sensor image moves further out
        let corr = RadialCorrection { knots: knots(), distortion: Some(vec![0.0, -1.0, -3.0, -6.0]), ca: None, vignetting: None };
        let (w, h) = (201, 151);
        let mut src = vec![0u16; w * h * 3];
        let (dot_x, dot_y) = (170usize, 130usize);
        for c in 0..3 {
            src[(dot_y * w + dot_x) * 3 + c] = 60000;
        }
        let out = corr.apply_linear_u16(&src, w, h);
        let (mut best, mut at) = (0u16, (0, 0));
        for y in 0..h {
            for x in 0..w {
                if out[(y * w + x) * 3 + 1] > best {
                    best = out[(y * w + x) * 3 + 1];
                    at = (x, y);
                }
            }
        }
        assert!(at.0 > dot_x && at.1 > dot_y, "dot moved to {at:?}");
    }

    #[test]
    fn pincushion_never_samples_outside() {
        let corr = RadialCorrection { knots: knots(), distortion: Some(vec![0.5, 1.0, 2.0, 4.0]), ca: None, vignetting: None };
        let (w, h) = (60, 40);
        let src = vec![500u16; w * h * 3];
        let out = corr.apply_linear_u16(&src, w, h);
        assert!(out.iter().all(|&v| v == 500));
    }

    #[test]
    fn lateral_ca_shifts_red_and_blue_apart() {
        let corr = RadialCorrection { knots: knots(), distortion: None, ca: Some((vec![0.01; 4], vec![-0.01; 4])), vignetting: None };
        let (w, h) = (101, 101);
        // Horizontal ramp: a larger sampling radius on the right half reads brighter values
        let src: Vec<u16> = (0..w * h * 3).map(|i| ((i / 3) % w * 500) as u16).collect();
        let out = corr.apply_linear_u16(&src, w, h);
        let i = (50 * w + 98) * 3;
        assert!(out[i] > out[i + 1] && out[i + 1] > out[i + 2], "{:?}", &out[i..i + 3]);
    }
}
