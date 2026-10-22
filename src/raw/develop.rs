//! OmaStudio's own RAW development: LibRaw only unpacks the sensor data, and black
//! level, white balance, demosaicing and the camera colour matrix happen here.
//!
//! The result is linear light in the requested output space, the same contract as a
//! LibRaw decode with a linear curve, so the pipeline treats both alike.

use super::OutputColorSpace;
use super::demosaic::{self, CfaPattern, Mosaic};
use super::ffi::COmaRawCfaInfo;
use rayon::prelude::*;

/// Sensor data of the visible area plus what is needed to develop it.
pub struct CfaFrame {
    pub width: usize,
    pub height: usize,
    /// Black-subtracted, white-balanced values, 1.0 = clipping point.
    pub data: Vec<f32>,
    pub pattern: CfaPattern,
    /// Camera RGB (white balanced) -> linear sRGB.
    pub rgb_cam: [[f32; 3]; 3],
    /// LibRaw `flip`: 0, 3 (180), 5 (90 CCW), 6 (90 CW).
    pub flip: i32,
}

impl CfaFrame {
    /// Builds the frame from LibRaw's unpacked CFA plane.
    ///
    /// # Safety
    /// `raw` must point to at least `raw_pitch * raw_height` values that stay valid
    /// for the duration of the call.
    pub unsafe fn from_libraw(raw: *const u16, info: &COmaRawCfaInfo) -> Option<Self> {
        let (width, height) = (info.width.max(0) as usize, info.height.max(0) as usize);
        let pitch = info.raw_pitch.max(0) as usize;
        let (top, left) = (info.top_margin.max(0) as usize, info.left_margin.max(0) as usize);
        if raw.is_null() || width < 6 || height < 6 || top + height > info.raw_height as usize || left + width > pitch {
            return None;
        }
        // SAFETY: the caller guarantees raw_pitch * raw_height readable values
        let plane = unsafe { std::slice::from_raw_parts(raw, pitch * info.raw_height as usize) };

        let pattern = if info.filters == 9 {
            let mut xt = [[0u8; 6]; 6];
            for (r, row) in info.xtrans.iter().enumerate() {
                for (c, v) in row.iter().enumerate() {
                    if !(0..=2).contains(v) {
                        return None;
                    }
                    xt[r][c] = *v as u8;
                }
            }
            CfaPattern::XTrans(xt)
        } else if info.filters > 1000 {
            CfaPattern::Bayer(info.filters)
        } else {
            return None;
        };

        // As-shot white balance, normalised so the weakest channel is 1: every channel
        // then clips at 1.0 together and blown highlights stay neutral.
        let mut mul = if info.cam_mul[0] > 0.0 && info.cam_mul[1] > 0.0 { info.cam_mul } else { info.pre_mul };
        if mul[1] <= 0.0 {
            return None;
        }
        if mul[3] <= 0.0 {
            mul[3] = mul[1];
        }
        let min = mul[..3].iter().copied().fold(f32::MAX, f32::min);
        if min <= 0.0 {
            return None;
        }
        let mul = [mul[0] / min, mul[1] / min, mul[2] / min];

        // LibRaw's adjust_maximum_thr (0.75): when the brightest photosite is close to the
        // nominal white level, it is the real clipping point. Same rule, same brightness.
        let data_max = (top..top + height)
            .into_par_iter()
            .map(|y| plane[y * pitch + left..y * pitch + left + width].iter().copied().max().unwrap_or(0))
            .max()
            .unwrap_or(0) as f32;
        let white = if data_max > 0.75 * info.white && data_max < info.white { data_max } else { info.white };
        let (ph, pw) = (info.black_pattern_h.max(0) as usize, info.black_pattern_w.max(0) as usize);
        let mut data = vec![0f32; width * height];
        data.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
            let src = &plane[(y + top) * pitch + left..(y + top) * pitch + left + width];
            for (x, out) in row.iter_mut().enumerate() {
                let c = pattern.color(y, x);
                let mut black = info.black[c];
                if ph > 0 && pw > 0 {
                    black += info.black_pattern[(y % ph) * pw + x % pw];
                }
                // Black can live in the per-colour levels or the pattern (Fujifilm uses the
                // 6x6 pattern), so the usable range is taken per photosite
                let range = (white - black).max(1.0);
                *out = ((src[x] as f32 - black) / range * mul[c]).clamp(0.0, 1.0);
            }
        });

        let rgb_cam = std::array::from_fn(|r| std::array::from_fn(|c| info.rgb_cam[r][c]));
        Some(Self { width, height, data, pattern, rgb_cam, flip: info.flip })
    }

    /// Develops the frame into interleaved 16-bit linear RGB in `space`.
    /// `half_size` bins instead of demosaicing (1/2 size for Bayer, 1/3 for X-Trans).
    /// The frame is consumed so its sensor data is freed before the image is rotated.
    pub fn develop(self, space: OutputColorSpace, half_size: bool) -> (Vec<u16>, u32, u32) {
        let m = mul3(&srgb_to(space), &self.rgb_cam);
        let to_u16 = |p: [f32; 3]| -> [u16; 3] {
            std::array::from_fn(|r| {
                let v = m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2];
                (v.clamp(0.0, 1.0) * 65535.0).round() as u16
            })
        };
        let mosaic = Mosaic { width: self.width, height: self.height, data: &self.data, pattern: self.pattern };
        let (out, w, h) = if half_size {
            let (rgb, w, h) = demosaic::bin(&mosaic);
            let mut out = vec![0u16; rgb.len()];
            out.par_chunks_mut(3).zip(rgb.par_chunks(3)).for_each(|(o, p)| o.copy_from_slice(&to_u16([p[0], p[1], p[2]])));
            (out, w, h)
        } else {
            (demosaic::demosaic_map(&mosaic, to_u16), self.width, self.height)
        };
        let flip = self.flip;
        drop(self);
        let (out, w, h) = rotate(out, w, h, flip);
        (out, w as u32, h as u32)
    }
}

type Mat3 = [[f32; 3]; 3];

fn mul3(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn invert3(m: &Mat3) -> Mat3 {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let inv = 1.0 / det;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ]
}

/// RGB -> XYZ for the given xy primaries and white point.
fn rgb_to_xyz(primaries: [(f32, f32); 3], white: (f32, f32)) -> Mat3 {
    let xyz = |(x, y): (f32, f32)| [x / y, 1.0, (1.0 - x - y) / y];
    let p: Mat3 = std::array::from_fn(|r| std::array::from_fn(|c| xyz(primaries[c])[r]));
    let wv = xyz(white);
    let pi = invert3(&p);
    let s: [f32; 3] = std::array::from_fn(|r| (0..3).map(|k| pi[r][k] * wv[k]).sum());
    std::array::from_fn(|r| std::array::from_fn(|c| p[r][c] * s[c]))
}

const D65: (f32, f32) = (0.3127, 0.3290);
const D50: (f32, f32) = (0.3457, 0.3585);

/// Bradford chromatic adaptation from D65 to D50.
fn bradford_d65_to_d50() -> Mat3 {
    let b: Mat3 = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];
    let xyz = |(x, y): (f32, f32)| [x / y, 1.0, (1.0 - x - y) / y];
    let cone = |w: (f32, f32)| -> [f32; 3] { std::array::from_fn(|r| (0..3).map(|k| b[r][k] * xyz(w)[k]).sum()) };
    let (src, dst) = (cone(D65), cone(D50));
    let scale: Mat3 = std::array::from_fn(|r| std::array::from_fn(|c| if r == c { dst[r] / src[r] } else { 0.0 }));
    mul3(&invert3(&b), &mul3(&scale, &b))
}

/// Linear sRGB -> linear `space` (same primaries and white points as the built-in ICC profiles).
pub fn srgb_to(space: OutputColorSpace) -> Mat3 {
    let srgb = rgb_to_xyz([(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)], D65);
    let (target, adapt) = match space {
        OutputColorSpace::Srgb => return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        OutputColorSpace::DisplayP3 => (rgb_to_xyz([(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)], D65), false),
        OutputColorSpace::AdobeRgb => (rgb_to_xyz([(0.640, 0.330), (0.210, 0.710), (0.150, 0.060)], D65), false),
        OutputColorSpace::Rec2020 => (rgb_to_xyz([(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)], D65), false),
        OutputColorSpace::ProPhotoRgb => {
            (rgb_to_xyz([(0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)], D50), true)
        }
    };
    let xyz = if adapt { mul3(&bradford_d65_to_d50(), &srgb) } else { srgb };
    mul3(&invert3(&target), &xyz)
}

/// Applies LibRaw's `flip` to interleaved RGB.
fn rotate(src: Vec<u16>, w: usize, h: usize, flip: i32) -> (Vec<u16>, usize, usize) {
    let map: fn(usize, usize, usize, usize) -> (usize, usize) = match flip {
        3 => |x, y, w, h| (w - 1 - x, h - 1 - y),
        // 90 CCW: source (x, y) lands at (y, w - 1 - x) in an h x w image
        5 => |x, y, w, _| (y, w - 1 - x),
        // 90 CW
        6 => |x, y, _, h| (h - 1 - y, x),
        _ => return (src, w, h),
    };
    let (ow, oh) = if flip == 3 { (w, h) } else { (h, w) };
    let mut out = vec![0u16; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (ox, oy) = map(x, y, w, h);
            let (si, di) = ((y * w + x) * 3, (oy * ow + ox) * 3);
            out[di..di + 3].copy_from_slice(&src[si..si + 3]);
        }
    }
    let _ = oh;
    (out, ow, oh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_to_adobe_matches_the_published_matrix() {
        // Linear sRGB -> Adobe RGB (1998), both D65
        let m = srgb_to(OutputColorSpace::AdobeRgb);
        let want = [[0.7152, 0.2848, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0412, 0.9588]];
        for r in 0..3 {
            for c in 0..3 {
                assert!((m[r][c] - want[r][c]).abs() < 2e-3, "{m:?}");
            }
        }
    }

    #[test]
    fn white_stays_white_in_every_space() {
        for space in [
            OutputColorSpace::Srgb,
            OutputColorSpace::DisplayP3,
            OutputColorSpace::AdobeRgb,
            OutputColorSpace::ProPhotoRgb,
            OutputColorSpace::Rec2020,
        ] {
            let m = srgb_to(space);
            for row in m {
                assert!((row.iter().sum::<f32>() - 1.0).abs() < 1e-3, "{space:?} {m:?}");
            }
        }
    }

    #[test]
    fn rotations_move_pixels_where_viewers_expect() {
        // 2x1 image: red, green
        let src = vec![1, 0, 0, 0, 2, 0];
        let (cw, w, h) = rotate(src.clone(), 2, 1, 6);
        assert_eq!((w, h), (1, 2));
        assert_eq!(cw, vec![1, 0, 0, 0, 2, 0]);
        let (ccw, _, _) = rotate(src.clone(), 2, 1, 5);
        assert_eq!(ccw, vec![0, 2, 0, 1, 0, 0]);
        let (r180, _, _) = rotate(src, 2, 1, 3);
        assert_eq!(r180, vec![0, 2, 0, 1, 0, 0]);
    }
}
