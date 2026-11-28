//! HDR transfer curves (SMPTE ST 2084 PQ, ITU-R BT.2100 HLG), unclipped linear
//! primary conversions, float geometry/resizing, and CICP / ISO 21496-1 Gain Map
//! encoder command builders (`MOD-TECH-01`, `BUG-BE-18`).

use crate::icc::{self, Primaries};
use crate::pipeline::transfer::Transfer;
use crate::raw::OutputColorSpace;
use crate::recipe::Recipe;
use rayon::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::Path;

/// ITU-R BT.2408-7 / ISO 21496-1 SDR diffuse white luminance in cd/m² (nits).
pub const HDR_DIFFUSE_WHITE_NITS: f32 = 203.0;
/// SMPTE ST 2084 (PQ) peak luminance in cd/m² (nits).
pub const PQ_PEAK_NITS: f32 = 10_000.0;
/// Scene-referred HLG scale so linear `1.0` (203-nit BT.2408 diffuse white) encodes
/// to HLG signal `0.750` (75% HLG reference white on a 1000-nit display at gamma 1.2:
/// `0.2649625^1.2 ≈ 0.203`).
pub const HLG_DIFFUSE_WHITE_E: f64 = 0.264_962_5;

pub const DEFAULT_GAINMAP_HEADROOM_STOPS: f32 = 2.0;
pub const MIN_GAINMAP_HEADROOM_STOPS: f32 = 1.0;
pub const MAX_GAINMAP_HEADROOM_STOPS: f32 = 4.0;

/// Output dynamic range mode for exports (`MOD-TECH-01`).
///
/// - `Sdr`: Standard dynamic range with embedded ICC profile (default).
/// - `Pq10`: SMPTE ST 2084 (PQ) HDR in ITU-R BT.2020 primaries (CICP `9/16/*`) for AVIF, JPEG XL, HEIF.
/// - `Hlg10`: ITU-R BT.2100 HLG HDR in BT.2020 primaries (CICP `9/18/*`) for AVIF, JPEG XL, HEIF.
/// - `GainMap`: ISO/IEC 21496-1 SDR base + HDR gain map for AVIF (`avifgainmaputil`) and JPEG (`ultrahdr_app`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum HdrMode {
    #[default]
    Sdr,
    Pq10,
    Hlg10,
    GainMap {
        /// Peak HDR headroom above SDR diffuse white in stops (EV), clamped to `1.0..=4.0`.
        headroom_stops: f32,
    },
}

impl HdrMode {
    pub fn is_hdr(self) -> bool {
        !matches!(self, Self::Sdr)
    }

    /// Clamps gain-map headroom to the supported `1.0..=4.0` EV range.
    pub fn clamped_headroom_stops(headroom_stops: f32) -> f32 {
        if headroom_stops.is_finite() {
            headroom_stops.clamp(MIN_GAINMAP_HEADROOM_STOPS, MAX_GAINMAP_HEADROOM_STOPS)
        } else {
            DEFAULT_GAINMAP_HEADROOM_STOPS
        }
    }
}

impl Serialize for HdrMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match *self {
            Self::Sdr => serializer.serialize_str("sdr"),
            Self::Pq10 => serializer.serialize_str("pq10"),
            Self::Hlg10 => serializer.serialize_str("hlg10"),
            Self::GainMap { headroom_stops } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("mode", "gain_map")?;
                map.serialize_entry("headroom_stops", &headroom_stops)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for HdrMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct GainMapParams {
            #[serde(default = "default_headroom")]
            headroom_stops: f32,
        }
        fn default_headroom() -> f32 {
            DEFAULT_GAINMAP_HEADROOM_STOPS
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawHdrMode {
            Str(String),
            Tagged {
                mode: String,
                #[serde(default = "default_headroom")]
                headroom_stops: f32,
            },
            ExternalGainMap {
                gain_map: GainMapParams,
            },
        }

        let parse_mode = |name: &str, headroom: f32| -> Result<HdrMode, String> {
            match name.trim().to_ascii_lowercase().as_str() {
                "sdr" | "none" | "" => Ok(HdrMode::Sdr),
                "pq10" | "pq" | "hdr10" => Ok(HdrMode::Pq10),
                "hlg10" | "hlg" => Ok(HdrMode::Hlg10),
                "gain_map" | "gainmap" | "ultrahdr" => Ok(HdrMode::GainMap { headroom_stops: headroom }),
                other => Err(format!("Unsupported hdr_mode '{}'", other)),
            }
        };

        match RawHdrMode::deserialize(deserializer)? {
            RawHdrMode::Str(s) => parse_mode(&s, DEFAULT_GAINMAP_HEADROOM_STOPS).map_err(serde::de::Error::custom),
            RawHdrMode::Tagged { mode, headroom_stops } => {
                parse_mode(&mode, headroom_stops).map_err(serde::de::Error::custom)
            }
            RawHdrMode::ExternalGainMap { gain_map } => Ok(HdrMode::GainMap {
                headroom_stops: gain_map.headroom_stops,
            }),
        }
    }
}

/// Validates that `mode` is supported by normalized export format `fmt`.
pub fn validate_hdr_mode(fmt: &str, mode: HdrMode) -> Result<(), String> {
    match mode {
        HdrMode::Sdr => Ok(()),
        HdrMode::Pq10 | HdrMode::Hlg10 => match fmt {
            "avif" | "jxl" | "heic" => Ok(()),
            "exr" => Err(
                "OpenEXR is always exported as scene-linear float; use SDR mode for EXR or choose AVIF/JXL/HEIF for PQ/HLG"
                    .to_string(),
            ),
            other => Err(format!(
                "PQ/HLG HDR export is supported for AVIF, JPEG XL, and HEIF, not '{}'",
                other
            )),
        },
        HdrMode::GainMap { headroom_stops } => {
            if !headroom_stops.is_finite() {
                return Err("Gain map headroom_stops must be a finite number".to_string());
            }
            match fmt {
                "avif" | "jpeg" => Ok(()),
                other => Err(format!(
                    "ISO 21496-1 Gain Map HDR export is supported for AVIF and JPEG, not '{}'",
                    other
                )),
            }
        }
    }
}

/// Ensures HDR exports use at least 10 bits per channel where supported by the container.
pub fn effective_hdr_bit_depth(fmt: &str, depth: u8, mode: HdrMode) -> u8 {
    match mode {
        HdrMode::Sdr => depth,
        HdrMode::Pq10 | HdrMode::Hlg10 => match fmt {
            "jxl" => 16,
            "avif" | "heic" => depth.max(10),
            _ => depth,
        },
        HdrMode::GainMap { .. } => match fmt {
            "avif" => depth.max(10),
            "jpeg" => 8,
            _ => depth,
        },
    }
}

/// SMPTE ST 2084 (PQ) OETF: maps scene-linear RGB where `1.0` = 203 nits (BT.2408 diffuse
/// white) into `[0.0, 1.0]` PQ signal (with `1.0` = 10,000 nits).
#[inline]
pub fn pq_encode(linear: f32) -> f32 {
    if !linear.is_finite() || linear <= 0.0 {
        return 0.0;
    }
    const M1: f64 = 2610.0 / 16384.0;
    const M2: f64 = 2523.0 / 4096.0 * 128.0;
    const C1: f64 = 3424.0 / 4096.0;
    const C2: f64 = 2413.0 / 4096.0 * 32.0;
    const C3: f64 = 2392.0 / 4096.0 * 32.0;
    let y = (linear as f64 * (HDR_DIFFUSE_WHITE_NITS as f64 / PQ_PEAK_NITS as f64)).clamp(0.0, 1.0);
    let ym1 = y.powf(M1);
    let num = C1 + C2 * ym1;
    let den = 1.0 + C3 * ym1;
    (num / den).powf(M2).clamp(0.0, 1.0) as f32
}

/// ITU-R BT.2100 HLG OETF: maps scene-linear RGB where `1.0` = 203 nits (BT.2408 diffuse
/// white) into `[0.0, 1.0]` HLG signal (`1.0` linear → `0.750` HLG reference white).
#[inline]
pub fn hlg_encode(linear: f32) -> f32 {
    if !linear.is_finite() || linear <= 0.0 {
        return 0.0;
    }
    const A: f64 = 0.17883277;
    const B: f64 = 0.28466892;
    const C: f64 = 0.55991073;
    let e = (linear as f64 * HLG_DIFFUSE_WHITE_E).clamp(0.0, 1.0);
    let out = if e <= 1.0 / 12.0 {
        (3.0 * e).sqrt()
    } else {
        A * (12.0 * e - B).ln() + C
    };
    out.clamp(0.0, 1.0) as f32
}

type Mat3 = [[f64; 3]; 3];

fn mul_mat3(a: Mat3, b: Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

fn mul_mat3_vec(m: Mat3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn invert_mat3(m: Mat3) -> Mat3 {
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = m[1][2] * m[2][0] - m[1][0] * m[2][2];
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    if det.abs() < 1e-18 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let inv = 1.0 / det;
    [
        [
            c00 * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            c01 * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            c02 * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ]
}

fn xy_to_xyz(xy: (f64, f64)) -> [f64; 3] {
    let (x, y) = xy;
    [x / y, 1.0, (1.0 - x - y) / y]
}

fn rgb_to_xyz_matrix(prim: Primaries) -> Mat3 {
    let ([r, g, b], w) = prim;
    let xr = xy_to_xyz(r);
    let xg = xy_to_xyz(g);
    let xb = xy_to_xyz(b);
    let xw = xy_to_xyz(w);
    let cols = [
        [xr[0], xg[0], xb[0]],
        [xr[1], xg[1], xb[1]],
        [xr[2], xg[2], xb[2]],
    ];
    let s = mul_mat3_vec(invert_mat3(cols), xw);
    [
        [cols[0][0] * s[0], cols[0][1] * s[1], cols[0][2] * s[2]],
        [cols[1][0] * s[0], cols[1][1] * s[1], cols[1][2] * s[2]],
        [cols[2][0] * s[0], cols[2][1] * s[1], cols[2][2] * s[2]],
    ]
}

fn bradford_adaptation(src_white: (f64, f64), dst_white: (f64, f64)) -> Mat3 {
    if (src_white.0 - dst_white.0).abs() < 1e-7 && (src_white.1 - dst_white.1).abs() < 1e-7 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    const BFD: Mat3 = [
        [0.8951000, 0.2664000, -0.1614000],
        [-0.7502000, 1.7135000, 0.0367000],
        [0.0389000, -0.0685000, 1.0296000],
    ];
    let bfd_inv = invert_mat3(BFD);
    let cone_s = mul_mat3_vec(BFD, xy_to_xyz(src_white));
    let cone_d = mul_mat3_vec(BFD, xy_to_xyz(dst_white));
    let scale = [
        [cone_d[0] / cone_s[0], 0.0, 0.0],
        [0.0, cone_d[1] / cone_s[1], 0.0],
        [0.0, 0.0, cone_d[2] / cone_s[2]],
    ];
    mul_mat3(bfd_inv, mul_mat3(scale, BFD))
}

/// Computes the unclipped linear 3×3 RGB conversion matrix from `src` primaries/white point
/// to `dst` primaries/white point using Bradford chromatic adaptation when white points differ.
pub fn linear_primaries_matrix(src: Primaries, dst: Primaries) -> [[f32; 3]; 3] {
    let m_src = rgb_to_xyz_matrix(src);
    let m_adapt = bradford_adaptation(src.1, dst.1);
    let m_dst_inv = invert_mat3(rgb_to_xyz_matrix(dst));
    let m = mul_mat3(m_dst_inv, mul_mat3(m_adapt, m_src));
    [
        [m[0][0] as f32, m[0][1] as f32, m[0][2] as f32],
        [m[1][0] as f32, m[1][1] as f32, m[1][2] as f32],
        [m[2][0] as f32, m[2][1] as f32, m[2][2] as f32],
    ]
}

/// Decodes unclipped display-encoded RGB (`transfer`) back to scene-linear light and converts
/// primaries from `src_space` to `dst_primaries` without clamping `> 1.0` highlights.
pub fn linearize_and_convert_f32(
    display_rgb: &[f32],
    transfer: Transfer,
    src_space: OutputColorSpace,
    dst_primaries: Primaries,
) -> Vec<f32> {
    let m = linear_primaries_matrix(icc::primaries(src_space), dst_primaries);
    let mut out = vec![0.0f32; display_rgb.len()];
    out.par_chunks_mut(3)
        .zip(display_rgb.par_chunks(3))
        .for_each(|(dst, src)| {
            let lr = transfer.decode(src[0]);
            let lg = transfer.decode(src[1]);
            let lb = transfer.decode(src[2]);
            dst[0] = (m[0][0] * lr + m[0][1] * lg + m[0][2] * lb).max(0.0);
            dst[1] = (m[1][0] * lr + m[1][1] * lg + m[1][2] * lb).max(0.0);
            dst[2] = (m[2][0] * lr + m[2][1] * lg + m[2][2] * lb).max(0.0);
        });
    out
}

/// Encodes unclipped linear BT.2020 float RGB into 16-bit PQ or HLG samples (`0..=65535`).
pub fn encode_hdr_16_from_linear_rec2020(linear_rec2020: &[f32], mode: HdrMode) -> Vec<u16> {
    match mode {
        HdrMode::Hlg10 => linear_rec2020
            .par_iter()
            .map(|&v| (hlg_encode(v).clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
            .collect(),
        HdrMode::GainMap { headroom_stops } => {
            let max_lin = 2.0f32.powf(HdrMode::clamped_headroom_stops(headroom_stops));
            linear_rec2020
                .par_iter()
                .map(|&v| (pq_encode(v.min(max_lin)).clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
                .collect()
        }
        HdrMode::Pq10 | HdrMode::Sdr => linear_rec2020
            .par_iter()
            .map(|&v| (pq_encode(v).clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
            .collect(),
    }
}

/// Packs unclipped linear BT.2020 float RGB into little-endian `UHDR_IMG_FMT_32bppRGBA1010102`
/// (PQ-encoded 10-bit R/G/B in bits `0..30`, opaque 2-bit alpha `3` in bits `30..32`) for `ultrahdr_app`.
pub fn pack_rgba1010102_le_from_linear_rec2020(linear_rec2020: &[f32], headroom_stops: f32) -> Vec<u8> {
    let max_lin = 2.0f32.powf(HdrMode::clamped_headroom_stops(headroom_stops));
    let num_pixels = linear_rec2020.len() / 3;
    let mut out = vec![0u8; num_pixels * 4];
    out.par_chunks_mut(4)
        .zip(linear_rec2020.par_chunks_exact(3))
        .for_each(|(dst, src)| {
            let r10 = (pq_encode(src[0].min(max_lin)).clamp(0.0, 1.0) * 1023.0 + 0.5) as u32;
            let g10 = (pq_encode(src[1].min(max_lin)).clamp(0.0, 1.0) * 1023.0 + 0.5) as u32;
            let b10 = (pq_encode(src[2].min(max_lin)).clamp(0.0, 1.0) * 1023.0 + 0.5) as u32;
            let word = (r10 & 0x3ff) | ((g10 & 0x3ff) << 10) | ((b10 & 0x3ff) << 20) | (3u32 << 30);
            dst.copy_from_slice(&word.to_le_bytes());
        });
    out
}

/// Applies crop, rotation, straightening, and flips directly to an unclipped `f32` RGB buffer
/// using the exact same coordinate rules as [`super::apply_geometry`].
pub fn apply_geometry_f32(
    mut data: Vec<f32>,
    mut w: u32,
    mut h: u32,
    recipe: &Recipe,
) -> (Vec<f32>, u32, u32) {
    if w == 0 || h == 0 || data.len() < (w as usize) * (h as usize) * 3 {
        return (data, w, h);
    }

    if recipe.crop_w < 0.999 || recipe.crop_h < 0.999 || recipe.crop_x > 0.001 || recipe.crop_y > 0.001 {
        let cx = ((recipe.crop_x.clamp(0.0, 1.0) * w as f32).round() as u32).min(w.saturating_sub(1));
        let cy = ((recipe.crop_y.clamp(0.0, 1.0) * h as f32).round() as u32).min(h.saturating_sub(1));
        let cw = ((recipe.crop_w.clamp(0.0, 1.0) * w as f32).round() as u32).min(w - cx).max(1);
        let ch = ((recipe.crop_h.clamp(0.0, 1.0) * h as f32).round() as u32).min(h - cy).max(1);
        let mut cropped = vec![0.0f32; (cw as usize) * (ch as usize) * 3];
        for y in 0..ch as usize {
            let src_start = ((cy as usize + y) * w as usize + cx as usize) * 3;
            let dst_start = y * cw as usize * 3;
            cropped[dst_start..dst_start + cw as usize * 3]
                .copy_from_slice(&data[src_start..src_start + cw as usize * 3]);
        }
        data = cropped;
        w = cw;
        h = ch;
    }

    let angle = if recipe.rotation.is_finite() { recipe.rotation } else { 0.0 };
    let quarters = (angle / 90.0).round();
    let residual = angle - quarters * 90.0;
    match (quarters as i64).rem_euclid(4) {
        1 => {
            // 90° clockwise: (x, y) -> (h - 1 - y, x)
            let (sw, sh) = (w as usize, h as usize);
            let mut rot = vec![0.0f32; sw * sh * 3];
            for y in 0..sh {
                for x in 0..sw {
                    let nx = sh - 1 - y;
                    let ny = x;
                    let src = (y * sw + x) * 3;
                    let dst = (ny * sh + nx) * 3;
                    rot[dst..dst + 3].copy_from_slice(&data[src..src + 3]);
                }
            }
            data = rot;
            std::mem::swap(&mut w, &mut h);
        }
        2 => {
            let n = (w as usize) * (h as usize);
            let mut rot = vec![0.0f32; n * 3];
            for i in 0..n {
                let dst = (n - 1 - i) * 3;
                let src = i * 3;
                rot[dst..dst + 3].copy_from_slice(&data[src..src + 3]);
            }
            data = rot;
        }
        3 => {
            // 270° clockwise (90° CCW): (x, y) -> (y, w - 1 - x)
            let (sw, sh) = (w as usize, h as usize);
            let mut rot = vec![0.0f32; sw * sh * 3];
            for y in 0..sh {
                for x in 0..sw {
                    let nx = y;
                    let ny = sw - 1 - x;
                    let src = (y * sw + x) * 3;
                    let dst = (ny * sh + nx) * 3;
                    rot[dst..dst + 3].copy_from_slice(&data[src..src + 3]);
                }
            }
            data = rot;
            std::mem::swap(&mut w, &mut h);
        }
        _ => {}
    }

    if residual.abs() >= 0.01 {
        (data, w, h) = straighten_f32(&data, w, h, residual);
    }

    if recipe.flip_h {
        let (sw, sh) = (w as usize, h as usize);
        for y in 0..sh {
            let row = &mut data[y * sw * 3..(y + 1) * sw * 3];
            for x in 0..sw / 2 {
                let a = x * 3;
                let b = (sw - 1 - x) * 3;
                for c in 0..3 {
                    row.swap(a + c, b + c);
                }
            }
        }
    }
    if recipe.flip_v {
        let (sw, sh) = (w as usize, h as usize);
        let row_len = sw * 3;
        for y in 0..sh / 2 {
            let top = y * row_len;
            let bot = (sh - 1 - y) * row_len;
            let (left, right) = data.split_at_mut(bot);
            left[top..top + row_len].swap_with_slice(&mut right[..row_len]);
        }
    }

    (data, w, h)
}

fn straighten_f32(data: &[f32], w_u32: u32, h_u32: u32, degrees: f32) -> (Vec<f32>, u32, u32) {
    let (w, h) = (w_u32 as f32, h_u32 as f32);
    let a = degrees.to_radians();
    let (sin, cos) = (a.sin(), a.cos());
    let (sa, ca) = (sin.abs(), cos.abs());
    let scale = (w / (w * ca + h * sa)).min(h / (w * sa + h * ca));
    let (ow, oh) = (((w * scale).floor() as u32).max(1), ((h * scale).floor() as u32).max(1));
    let (sw, sh) = (w_u32 as usize, h_u32 as usize);
    let mut out = vec![0.0f32; ow as usize * oh as usize * 3];
    out.par_chunks_mut(ow as usize * 3).enumerate().for_each(|(y, row)| {
        let dy = y as f32 + 0.5 - oh as f32 / 2.0;
        for x in 0..ow as usize {
            let dx = x as f32 + 0.5 - ow as f32 / 2.0;
            let sx = (dx * cos + dy * sin + w / 2.0 - 0.5).clamp(0.0, w - 1.0);
            let sy = (-dx * sin + dy * cos + h / 2.0 - 0.5).clamp(0.0, h - 1.0);
            let (x0, y0) = (sx as usize, sy as usize);
            let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            for c in 0..3 {
                let at = |yy: usize, xx: usize| data[(yy * sw + xx) * 3 + c];
                let top = at(y0, x0) * (1.0 - fx) + at(y0, x1) * fx;
                let bottom = at(y1, x0) * (1.0 - fx) + at(y1, x1) * fx;
                row[x * 3 + c] = (top * (1.0 - fy) + bottom * fy).max(0.0);
            }
        }
    });
    (out, ow, oh)
}

/// Resizes an unclipped `f32` RGB buffer matching `ExportOptions` scaling rules without clamping `> 1.0`.
pub fn resize_f32(
    data: Vec<f32>,
    w: u32,
    h: u32,
    max_dimension: Option<u32>,
    scale_percent: u32,
) -> (Vec<f32>, u32, u32) {
    if w == 0 || h == 0 {
        return (data, w, h);
    }
    let (nw, nh) = if let Some(max_dim) = max_dimension {
        if w > max_dim || h > max_dim {
            let ratio = (max_dim as f64 / w as f64).min(max_dim as f64 / h as f64);
            (
                (w as f64 * ratio).round().max(1.0) as u32,
                (h as f64 * ratio).round().max(1.0) as u32,
            )
        } else {
            return (data, w, h);
        }
    } else if scale_percent < 100 && scale_percent > 0 {
        (
            ((w as u64 * scale_percent as u64) / 100).max(1) as u32,
            ((h as u64 * scale_percent as u64) / 100).max(1) as u32,
        )
    } else {
        return (data, w, h);
    };

    if nw == w && nh == h {
        return (data, w, h);
    }

    let (sw, sh, dw, dh) = (w as usize, h as usize, nw as usize, nh as usize);
    let scale_x = sw as f64 / dw as f64;
    let scale_y = sh as f64 / dh as f64;
    let mut out = vec![0.0f32; dw * dh * 3];
    out.par_chunks_mut(dw * 3).enumerate().for_each(|(y, row)| {
        let sy0 = (y as f64 * scale_y).floor() as usize;
        let sy1 = (((y + 1) as f64 * scale_y).ceil() as usize).min(sh).max(sy0 + 1);
        for x in 0..dw {
            if scale_x >= 1.0 && scale_y >= 1.0 {
                let sx0 = (x as f64 * scale_x).floor() as usize;
                let sx1 = (((x + 1) as f64 * scale_x).ceil() as usize).min(sw).max(sx0 + 1);
                let mut acc = [0.0f64; 3];
                for yy in sy0..sy1 {
                    for xx in sx0..sx1 {
                        let k = (yy * sw + xx) * 3;
                        acc[0] += data[k] as f64;
                        acc[1] += data[k + 1] as f64;
                        acc[2] += data[k + 2] as f64;
                    }
                }
                let n = ((sy1 - sy0) * (sx1 - sx0)).max(1) as f64;
                row[x * 3] = (acc[0] / n) as f32;
                row[x * 3 + 1] = (acc[1] / n) as f32;
                row[x * 3 + 2] = (acc[2] / n) as f32;
            } else {
                let sx = ((x as f64 + 0.5) * scale_x - 0.5).clamp(0.0, (sw - 1) as f64);
                let sy = ((y as f64 + 0.5) * scale_y - 0.5).clamp(0.0, (sh - 1) as f64);
                let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
                let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
                let (fx, fy) = ((sx - x0 as f64) as f32, (sy - y0 as f64) as f32);
                for c in 0..3 {
                    let at = |yy: usize, xx: usize| data[(yy * sw + xx) * 3 + c];
                    let top = at(y0, x0) * (1.0 - fx) + at(y0, x1) * fx;
                    let bot = at(y1, x0) * (1.0 - fx) + at(y1, x1) * fx;
                    row[x * 3 + c] = (top * (1.0 - fy) + bot * fy).max(0.0);
                }
            }
        }
    });
    (out, nw, nh)
}

/// Builds `avifenc` arguments for SDR, PQ10, or HLG10 AVIF output.
///
/// Lossless (`quality >= 100`) uses CICP matrix `0` (Identity GBR); lossy YUV uses matrix `9`
/// (ITU-R BT.2020 NCL) per `libavif` CICP rules.
pub fn build_avifenc_args(input_png: &Path, output: &Path, depth: u8, quality: u32, mode: HdrMode) -> Vec<String> {
    let mut args = vec![
        "-s".to_string(),
        "4".to_string(),
        "-d".to_string(),
        depth.to_string(),
    ];
    let matrix = if quality >= 100 { "0" } else { "9" };
    match mode {
        HdrMode::Pq10 => {
            args.push("--cicp".to_string());
            args.push(format!("9/16/{matrix}"));
            args.push("--ignore-icc".to_string());
        }
        HdrMode::Hlg10 => {
            args.push("--cicp".to_string());
            args.push(format!("9/18/{matrix}"));
            args.push("--ignore-icc".to_string());
        }
        HdrMode::Sdr | HdrMode::GainMap { .. } => {}
    }
    if quality >= 100 {
        args.push("--lossless".to_string());
    } else {
        args.push("-q".to_string());
        args.push(quality.to_string());
    }
    args.push("--".to_string());
    args.push(input_png.to_string_lossy().to_string());
    args.push(output.to_string_lossy().to_string());
    args
}

/// Builds `avifgainmaputil combine` arguments for ISO/IEC 21496-1 AVIF Gain Map export.
pub fn build_avifgainmaputil_args(
    sdr_png: &Path,
    hdr_pq_png: &Path,
    output: &Path,
    depth: u8,
    quality: u32,
) -> Vec<String> {
    vec![
        "combine".to_string(),
        "-d".to_string(),
        depth.to_string(),
        "-q".to_string(),
        quality.to_string(),
        "--qgain-map".to_string(),
        quality.to_string(),
        "-s".to_string(),
        "4".to_string(),
        "--cicp-base".to_string(),
        "1/13/1".to_string(),
        "--cicp-alternate".to_string(),
        "9/16/9".to_string(),
        "--ignore-profile".to_string(),
        "--".to_string(),
        sdr_png.to_string_lossy().to_string(),
        hdr_pq_png.to_string_lossy().to_string(),
        output.to_string_lossy().to_string(),
    ]
}

/// Builds FFmpeg AVIF encoding arguments without `--` (unsupported by FFmpeg), using the
/// `file:` protocol to guard leading `-` or protocol-like filenames, selecting 10-bit pixel
/// format for >8-bit depths or HDR modes, and tagging BT.2020 PQ/HLG signaling when requested.
pub fn build_ffmpeg_avif_args(
    input: &Path,
    output: &Path,
    depth: u8,
    quality: u32,
    mode: HdrMode,
) -> Vec<String> {
    let pix_fmt = if depth > 8 || matches!(mode, HdrMode::Pq10 | HdrMode::Hlg10) {
        "yuv420p10le"
    } else {
        "yuv420p"
    };
    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        format!("file:{}", input.display()),
        "-c:v".to_string(),
        "libsvtav1".to_string(),
        "-pix_fmt".to_string(),
        pix_fmt.to_string(),
    ];
    match mode {
        HdrMode::Pq10 => {
            args.extend([
                "-color_primaries".to_string(),
                "bt2020".to_string(),
                "-color_trc".to_string(),
                "smpte2084".to_string(),
                "-colorspace".to_string(),
                "bt2020nc".to_string(),
            ]);
        }
        HdrMode::Hlg10 => {
            args.extend([
                "-color_primaries".to_string(),
                "bt2020".to_string(),
                "-color_trc".to_string(),
                "arib-std-b67".to_string(),
                "-colorspace".to_string(),
                "bt2020nc".to_string(),
            ]);
        }
        HdrMode::Sdr | HdrMode::GainMap { .. } => {}
    }
    if quality >= 100 {
        args.push("-crf".to_string());
        args.push("0".to_string());
        args.push("-svtav1-params".to_string());
        args.push("lossless=1".to_string());
    } else {
        let crf = (35u32.saturating_sub(quality * 20 / 100)).to_string();
        args.push("-crf".to_string());
        args.push(crf);
    }
    args.push("-f".to_string());
    args.push("avif".to_string());
    args.push(format!("file:{}", output.display()));
    args
}

/// Builds `cjxl` arguments for SDR, PQ10, or HLG10 JPEG XL output.
pub fn build_cjxl_args(input_png: &Path, output: &Path, quality: u32, mode: HdrMode) -> Vec<String> {
    let distance = if quality >= 100 {
        "0".to_string()
    } else {
        ((100 - quality) as f32 / 10.0).to_string()
    };
    let mut args = vec![
        "-d".to_string(),
        distance,
        "-e".to_string(),
        "7".to_string(),
        "--container=1".to_string(),
    ];
    match mode {
        HdrMode::Pq10 => {
            args.push("-x".to_string());
            args.push("color_space=RGB_D65_202_Rel_PeQ".to_string());
        }
        HdrMode::Hlg10 => {
            args.push("-x".to_string());
            args.push("color_space=RGB_D65_202_Rel_HLG".to_string());
        }
        HdrMode::Sdr | HdrMode::GainMap { .. } => {}
    }
    args.push("--".to_string());
    args.push(input_png.to_string_lossy().to_string());
    args.push(output.to_string_lossy().to_string());
    args
}

/// Builds `heif-enc` arguments for SDR, PQ10, or HLG10 HEIF/HEIC output.
///
/// Note `libheif`'s singular `--transfer_characteristic` option and that
/// `--matrix_coefficients 0` (Identity) is paired with `--lossless` (`chroma=444`)
/// while lossy YUV uses `--matrix_coefficients 9` (BT.2020 NCL).
pub fn build_heif_enc_args(input_png: &Path, output: &Path, depth: u8, quality: u32, mode: HdrMode) -> Vec<String> {
    let mut args = Vec::new();
    if quality >= 100 {
        args.push("--lossless".to_string());
    } else {
        args.push("-q".to_string());
        args.push(quality.to_string());
    }
    args.push("-b".to_string());
    args.push(depth.to_string());
    let matrix = if quality >= 100 { "0" } else { "9" };
    match mode {
        HdrMode::Pq10 => {
            if quality >= 100 {
                args.push("-p".to_string());
                args.push("chroma=444".to_string());
            }
            args.extend([
                "--colour_primaries".to_string(),
                "9".to_string(),
                "--transfer_characteristic".to_string(),
                "16".to_string(),
                "--matrix_coefficients".to_string(),
                matrix.to_string(),
            ]);
        }
        HdrMode::Hlg10 => {
            if quality >= 100 {
                args.push("-p".to_string());
                args.push("chroma=444".to_string());
            }
            args.extend([
                "--colour_primaries".to_string(),
                "9".to_string(),
                "--transfer_characteristic".to_string(),
                "18".to_string(),
                "--matrix_coefficients".to_string(),
                matrix.to_string(),
            ]);
        }
        HdrMode::Sdr | HdrMode::GainMap { .. } => {}
    }
    args.push("-o".to_string());
    args.push(output.to_string_lossy().to_string());
    args.push("--".to_string());
    args.push(input_png.to_string_lossy().to_string());
    args
}

/// Builds `ultrahdr_app` arguments (encode scenario 2: raw RGBA1010102 PQ BT.2100 HDR intent +
/// compressed sRGB SDR JPEG -> ISO 21496-1 / Ultra HDR JPEG).
pub fn build_ultrahdr_args(
    hdr_raw: &Path,
    sdr_jpeg: &Path,
    output: &Path,
    width: u32,
    height: u32,
    quality: u32,
) -> Vec<String> {
    vec![
        "-m".to_string(),
        "0".to_string(),
        "-p".to_string(),
        hdr_raw.to_string_lossy().to_string(),
        "-i".to_string(),
        sdr_jpeg.to_string_lossy().to_string(),
        "-w".to_string(),
        width.to_string(),
        "-h".to_string(),
        height.to_string(),
        "-a".to_string(),
        "5".to_string(),
        "-C".to_string(),
        "2".to_string(),
        "-c".to_string(),
        "0".to_string(),
        "-t".to_string(),
        "2".to_string(),
        "-Q".to_string(),
        quality.to_string(),
        "-z".to_string(),
        output.to_string_lossy().to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pq_and_hlg_match_canonical_reference_points() {
        assert_eq!(pq_encode(0.0), 0.0);
        assert_eq!(pq_encode(-1.0), 0.0);
        // ITU-R BT.2408: 203-nit SDR diffuse white (linear 1.0) -> ~0.5807 PQ
        assert!((pq_encode(1.0) - 0.58069).abs() < 1e-3, "pq(1.0)={}", pq_encode(1.0));
        // 1000 nits -> ~0.7518 PQ
        assert!(
            (pq_encode(1000.0 / HDR_DIFFUSE_WHITE_NITS) - 0.75183).abs() < 1e-3,
            "pq(1000 nit)={}",
            pq_encode(1000.0 / HDR_DIFFUSE_WHITE_NITS)
        );
        // 10,000 nits -> 1.0 PQ
        assert!((pq_encode(PQ_PEAK_NITS / HDR_DIFFUSE_WHITE_NITS) - 1.0).abs() < 1e-5);

        assert_eq!(hlg_encode(0.0), 0.0);
        assert_eq!(hlg_encode(-0.5), 0.0);
        // BT.2408: 203-nit SDR diffuse white (linear 1.0) -> 75% HLG
        assert!((hlg_encode(1.0) - 0.750).abs() < 1e-3, "hlg(1.0)={}", hlg_encode(1.0));
        // Peak HLG (1000 nits at gamma 1.2) -> 1.0 HLG
        assert!((hlg_encode((1.0 / HLG_DIFFUSE_WHITE_E) as f32) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn srgb_to_rec2020_matrix_matches_bt2087_and_preserves_white() {
        let m = linear_primaries_matrix(
            icc::primaries(OutputColorSpace::Srgb),
            icc::primaries(OutputColorSpace::Rec2020),
        );
        assert!((m[0][0] - 0.627404).abs() < 1e-4);
        assert!((m[0][1] - 0.329283).abs() < 1e-4);
        assert!((m[0][2] - 0.043313).abs() < 1e-4);
        assert!((m[1][0] - 0.069097).abs() < 1e-4);
        assert!((m[1][1] - 0.919540).abs() < 1e-4);
        assert!((m[1][2] - 0.011362).abs() < 1e-4);
        assert!((m[2][0] - 0.016391).abs() < 1e-4);
        assert!((m[2][1] - 0.088013).abs() < 1e-4);
        assert!((m[2][2] - 0.895595).abs() < 1e-4);

        // Bradford white preservation across ProPhoto D50 -> Rec.2020 D65 and sRGB D65 -> ACEScg D60
        for (src, dst) in [
            (icc::primaries(OutputColorSpace::ProPhotoRgb), icc::primaries(OutputColorSpace::Rec2020)),
            (icc::primaries(OutputColorSpace::Srgb), icc::ACES_AP1),
        ] {
            let mat = linear_primaries_matrix(src, dst);
            for row in mat {
                let sum = row[0] + row[1] + row[2];
                assert!((sum - 1.0).abs() < 1e-3, "neutral white row sum {sum}");
            }
        }
    }

    #[test]
    fn hdr_mode_serde_accepts_strings_tagged_and_external_forms() {
        assert_eq!(serde_json::from_str::<HdrMode>("\"sdr\"").unwrap(), HdrMode::Sdr);
        assert_eq!(serde_json::from_str::<HdrMode>("\"pq10\"").unwrap(), HdrMode::Pq10);
        assert_eq!(serde_json::from_str::<HdrMode>("\"hlg10\"").unwrap(), HdrMode::Hlg10);
        assert_eq!(
            serde_json::from_str::<HdrMode>("\"gain_map\"").unwrap(),
            HdrMode::GainMap { headroom_stops: 2.0 }
        );
        assert_eq!(
            serde_json::from_str::<HdrMode>(r#"{"mode":"gain_map","headroom_stops":3.25}"#).unwrap(),
            HdrMode::GainMap { headroom_stops: 3.25 }
        );
        assert_eq!(
            serde_json::from_str::<HdrMode>(r#"{"gain_map":{"headroom_stops":1.5}}"#).unwrap(),
            HdrMode::GainMap { headroom_stops: 1.5 }
        );
        // Round-trip serialization
        let gm = HdrMode::GainMap { headroom_stops: 2.5 };
        let json = serde_json::to_string(&gm).unwrap();
        assert_eq!(serde_json::from_str::<HdrMode>(&json).unwrap(), gm);
    }

    #[test]
    fn hdr_mode_validation_rejects_incompatible_formats() {
        assert!(validate_hdr_mode("avif", HdrMode::Pq10).is_ok());
        assert!(validate_hdr_mode("jxl", HdrMode::Hlg10).is_ok());
        assert!(validate_hdr_mode("heic", HdrMode::Pq10).is_ok());
        assert!(validate_hdr_mode("avif", HdrMode::GainMap { headroom_stops: 2.0 }).is_ok());
        assert!(validate_hdr_mode("jpeg", HdrMode::GainMap { headroom_stops: 2.0 }).is_ok());

        assert!(validate_hdr_mode("jpeg", HdrMode::Pq10).is_err());
        assert!(validate_hdr_mode("png", HdrMode::Hlg10).is_err());
        assert!(validate_hdr_mode("tiff", HdrMode::GainMap { headroom_stops: 2.0 }).is_err());
        assert!(validate_hdr_mode("jxl", HdrMode::GainMap { headroom_stops: 2.0 }).is_err());
        assert!(validate_hdr_mode("exr", HdrMode::Pq10).is_err());
        assert!(validate_hdr_mode("jpeg", HdrMode::GainMap { headroom_stops: f32::NAN }).is_err());
    }

    #[test]
    fn rgba1010102_packing_matches_little_endian_bit_layout() {
        // White at 2.0 stops (4x diffuse white = 812 nits) -> PQ ~0.7289 -> 10-bit ~746
        let packed = pack_rgba1010102_le_from_linear_rec2020(&[0.0, 1.0, 4.0], 2.0);
        assert_eq!(packed.len(), 4);
        let word = u32::from_le_bytes([packed[0], packed[1], packed[2], packed[3]]);
        let r = word & 0x3ff;
        let g = (word >> 10) & 0x3ff;
        let b = (word >> 20) & 0x3ff;
        let a = (word >> 30) & 0x3;
        assert_eq!(r, 0);
        assert!((g as i32 - 594).abs() <= 2, "diffuse white PQ 10-bit g={g}");
        assert!((b as i32 - 746).abs() <= 2, "4x white PQ 10-bit b={b}");
        assert_eq!(a, 3);
    }

    #[test]
    fn encoder_cli_arg_builders_emit_expected_hdr_cicp_and_gainmap_flags() {
        let inp = Path::new("/tmp/in.png");
        let hdr = Path::new("/tmp/hdr.png");
        let raw = Path::new("/tmp/hdr.raw");
        let jpg = Path::new("/tmp/sdr.jpg");
        let out = Path::new("/tmp/out.bin");

        // avifenc: lossy uses matrix 9 (BT.2020 NCL), lossless uses matrix 0 (Identity)
        let avif_pq_lossy = build_avifenc_args(inp, out, 10, 90, HdrMode::Pq10);
        assert!(avif_pq_lossy.windows(2).any(|w| w == ["--cicp", "9/16/9"]));
        assert!(avif_pq_lossy.contains(&"--ignore-icc".to_string()));
        let avif_hlg_lossless = build_avifenc_args(inp, out, 12, 100, HdrMode::Hlg10);
        assert!(avif_hlg_lossless.windows(2).any(|w| w == ["--cicp", "9/18/0"]));
        assert!(avif_hlg_lossless.contains(&"--lossless".to_string()));

        // avifgainmaputil combine
        let avif_gm = build_avifgainmaputil_args(inp, hdr, out, 10, 88);
        assert_eq!(avif_gm[0], "combine");
        assert!(avif_gm.windows(2).any(|w| w == ["--cicp-base", "1/13/1"]));
        assert!(avif_gm.windows(2).any(|w| w == ["--cicp-alternate", "9/16/9"]));
        assert!(avif_gm.contains(&"--ignore-profile".to_string()));

        // cjxl
        let cjxl_pq = build_cjxl_args(inp, out, 90, HdrMode::Pq10);
        assert!(cjxl_pq.windows(2).any(|w| w == ["-x", "color_space=RGB_D65_202_Rel_PeQ"]));
        let cjxl_hlg = build_cjxl_args(inp, out, 100, HdrMode::Hlg10);
        assert!(cjxl_hlg.windows(2).any(|w| w == ["-x", "color_space=RGB_D65_202_Rel_HLG"]));

        // heif-enc (singular --transfer_characteristic!)
        let heif_pq = build_heif_enc_args(inp, out, 10, 90, HdrMode::Pq10);
        assert!(heif_pq.windows(2).any(|w| w == ["--colour_primaries", "9"]));
        assert!(heif_pq.windows(2).any(|w| w == ["--transfer_characteristic", "16"]));
        assert!(heif_pq.windows(2).any(|w| w == ["--matrix_coefficients", "9"]));
        let heif_hlg_ll = build_heif_enc_args(inp, out, 10, 100, HdrMode::Hlg10);
        assert!(heif_hlg_ll.windows(2).any(|w| w == ["--transfer_characteristic", "18"]));
        assert!(heif_hlg_ll.windows(2).any(|w| w == ["--matrix_coefficients", "0"]));
        assert!(heif_hlg_ll.windows(2).any(|w| w == ["-p", "chroma=444"]));

        // ultrahdr_app
        let uhdr = build_ultrahdr_args(raw, jpg, out, 1920, 1080, 92);
        assert!(uhdr.windows(2).any(|w| w == ["-m", "0"]));
        assert!(uhdr.windows(2).any(|w| w == ["-a", "5"]));
        assert!(uhdr.windows(2).any(|w| w == ["-C", "2"]));
        assert!(uhdr.windows(2).any(|w| w == ["-c", "0"]));
        assert!(uhdr.windows(2).any(|w| w == ["-t", "2"]));
        assert!(uhdr.windows(2).any(|w| w == ["-Q", "92"]));

        // ffmpeg HDR AVIF fallback
        let ff_pq = build_ffmpeg_avif_args(inp, out, 10, 90, HdrMode::Pq10);
        assert!(ff_pq.windows(2).any(|w| w == ["-color_primaries", "bt2020"]));
        assert!(ff_pq.windows(2).any(|w| w == ["-color_trc", "smpte2084"]));
        assert!(ff_pq.windows(2).any(|w| w == ["-colorspace", "bt2020nc"]));
        assert!(ff_pq.contains(&"yuv420p10le".to_string()));
        let ff_hlg = build_ffmpeg_avif_args(inp, out, 10, 90, HdrMode::Hlg10);
        assert!(ff_hlg.windows(2).any(|w| w == ["-color_trc", "arib-std-b67"]));
    }
}
