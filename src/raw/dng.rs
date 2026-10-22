//! Linear DNG output (3 samples per pixel, uncompressed) with lens corrections as
//! standard DNG opcodes, and reading those opcodes back from any DNG.
//!
//! Lens corrections travel as OpcodeList3 entries (DNG 1.3+): WarpRectilinear for
//! distortion and lateral chromatic aberration, FixVignetteRadial for vignetting.
//! Other DNG readers apply them too; OmaStudio applies them when it opens the file.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::lens::RadialCorrection;

const OP_WARP_RECTILINEAR: u32 = 1;
const OP_FIX_VIGNETTE_RADIAL: u32 = 3;
/// Opcodes are marked optional, so readers that lack one still open the file.
const OP_FLAG_OPTIONAL: u32 = 1;
const OP_VERSION: u32 = 0x0103_0000;
const TAG_SUB_IFDS: u16 = 330;
const TAG_DNG_VERSION: u16 = 50706;
const TAG_OPCODE_LIST3: u16 = 51022;
/// Radii sampled when a correction is fitted or read back.
const SAMPLES: usize = 64;

/// What the DNG says about the camera and its colours.
pub struct DngInfo<'a> {
    pub make: &'a str,
    pub model: &'a str,
    pub unique_model: &'a str,
    /// TIFF orientation (1 normal, 3 180, 6 90 CW, 8 90 CCW).
    pub orientation: u16,
    /// XYZ (D65) to the file's RGB.
    pub color_matrix: [[f32; 3]; 3],
    pub lens: Option<&'a RadialCorrection>,
}

enum Field {
    Short(Vec<u16>),
    Long(Vec<u32>),
    Ascii(String),
    Byte(Vec<u8>),
    Undefined(Vec<u8>),
    Rational(Vec<(u32, u32)>),
    SRational(Vec<(i32, i32)>),
}

impl Field {
    fn kind_count(&self) -> (u16, u32) {
        match self {
            Self::Byte(v) => (1, v.len() as u32),
            Self::Ascii(s) => (2, s.len() as u32 + 1),
            Self::Short(v) => (3, v.len() as u32),
            Self::Long(v) => (4, v.len() as u32),
            Self::Rational(v) => (5, v.len() as u32),
            Self::Undefined(v) => (7, v.len() as u32),
            Self::SRational(v) => (10, v.len() as u32),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Byte(v) | Self::Undefined(v) => v.clone(),
            Self::Ascii(s) => s.bytes().chain([0]).collect(),
            Self::Short(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::Long(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::Rational(v) => v.iter().flat_map(|(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat()).collect(),
            Self::SRational(v) => v.iter().flat_map(|(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat()).collect(),
        }
    }
}

fn srational(v: f32) -> (i32, i32) {
    ((v as f64 * 1_000_000.0).round() as i32, 1_000_000)
}

/// Writes interleaved linear 16-bit RGB as a DNG. The file appears under `path` only
/// once it is complete.
pub fn write_linear_rgb(path: &Path, rgb: &[u16], width: usize, height: usize, info: &DngInfo) -> Result<(), String> {
    if rgb.len() != width * height * 3 || width == 0 || height == 0 {
        return Err("Image size does not match its data".to_string());
    }
    let row_bytes = width * 6;
    let rows_per_strip = (65536 / row_bytes).clamp(1, height);
    let strips = height.div_ceil(rows_per_strip);
    let total = row_bytes as u64 * height as u64;
    if total > u32::MAX as u64 - (1 << 20) {
        return Err("Image too large for a classic TIFF/DNG file (4 GB)".to_string());
    }

    let mut fields: Vec<(u16, Field)> = vec![
        (254, Field::Long(vec![0])),
        (256, Field::Long(vec![width as u32])),
        (257, Field::Long(vec![height as u32])),
        (258, Field::Short(vec![16, 16, 16])),
        (259, Field::Short(vec![1])),
        (262, Field::Short(vec![34892])),
        (271, Field::Ascii(info.make.to_string())),
        (272, Field::Ascii(info.model.to_string())),
        (273, Field::Long(vec![0; strips])),
        (274, Field::Short(vec![info.orientation])),
        (277, Field::Short(vec![3])),
        (278, Field::Long(vec![rows_per_strip as u32])),
        (279, Field::Long((0..strips).map(|s| ((rows_per_strip.min(height - s * rows_per_strip)) * row_bytes) as u32).collect())),
        (284, Field::Short(vec![1])),
        (305, Field::Ascii(format!("OmaStudio {}", env!("CARGO_PKG_VERSION")))),
        (TAG_DNG_VERSION, Field::Byte(vec![1, 4, 0, 0])),
        (50707, Field::Byte(vec![1, 3, 0, 0])),
        (50708, Field::Ascii(info.unique_model.to_string())),
        (50714, Field::Long(vec![0])),
        (50717, Field::Long(vec![65535])),
        (50721, Field::SRational(info.color_matrix.iter().flatten().map(|&v| srational(v)).collect())),
        (50728, Field::Rational(vec![(1, 1); 3])),
        (50778, Field::Short(vec![21])),
    ];
    if let Some(ops) = info.lens.map(encode_opcodes).filter(|o| !o.is_empty()) {
        fields.push((TAG_OPCODE_LIST3, Field::Undefined(ops)));
    }
    fields.sort_by_key(|(t, _)| *t);

    // Layout: header, IFD, values that don't fit in an entry, strips
    let ifd_len = 2 + fields.len() * 12 + 4;
    let mut extra: Vec<u8> = Vec::new();
    let extra_start = 8 + ifd_len;
    let mut entries: Vec<u8> = Vec::with_capacity(ifd_len);
    entries.extend((fields.len() as u16).to_le_bytes());
    let data_start = (extra_start + 4096 + fields.iter().map(|(_, f)| f.bytes().len() + 2).sum::<usize>()) as u32;
    let strip_offsets: Vec<u32> = (0..strips).map(|s| data_start + (s * rows_per_strip * row_bytes) as u32).collect();
    for (tag, field) in &mut fields {
        if *tag == 273 {
            *field = Field::Long(strip_offsets.clone());
        }
        let (kind, count) = field.kind_count();
        let mut bytes = field.bytes();
        entries.extend(tag.to_le_bytes());
        entries.extend(kind.to_le_bytes());
        entries.extend(count.to_le_bytes());
        if bytes.len() <= 4 {
            bytes.resize(4, 0);
            entries.extend(&bytes);
        } else {
            if extra.len() % 2 == 1 {
                extra.push(0);
            }
            entries.extend(((extra_start + extra.len()) as u32).to_le_bytes());
            extra.extend(bytes);
        }
    }
    entries.extend(0u32.to_le_bytes());
    if extra_start + extra.len() > data_start as usize {
        return Err("DNG header overflow".to_string());
    }

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("pixelshift.dng"),
        std::process::id()
    ));
    let write = || -> std::io::Result<()> {
        let file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        let mut out = BufWriter::with_capacity(1 << 20, file);
        out.write_all(b"II*\0")?;
        out.write_all(&8u32.to_le_bytes())?;
        out.write_all(&entries)?;
        out.write_all(&extra)?;
        out.write_all(&vec![0u8; data_start as usize - extra_start - extra.len()])?;
        let mut row = vec![0u8; row_bytes];
        for line in rgb.chunks_exact(width * 3) {
            for (b, v) in row.as_chunks_mut::<2>().0.iter_mut().zip(line) {
                *b = v.to_le_bytes();
            }
            out.write_all(&row)?;
        }
        out.into_inner().map_err(|e| e.into_error())?.sync_all()
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot write {}: {}", path.display(), e));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Cannot write {}: {}", path.display(), e)
    })
}

/// Least-squares fit of `y(r)` with the given powers of r, each sample weighted by
/// `r^weight_power` (1 for a warp, whose error in pixels grows with the radius).
fn fit(samples: &[(f64, f64)], powers: &[i32], weight_power: i32) -> Vec<f64> {
    let n = powers.len();
    let mut a = vec![vec![0f64; n + 1]; n];
    for &(r, y) in samples {
        let w = r.powi(weight_power);
        let basis: Vec<f64> = powers.iter().map(|&p| r.powi(p) * w).collect();
        for i in 0..n {
            for j in 0..n {
                a[i][j] += basis[i] * basis[j];
            }
            a[i][n] += basis[i] * y * w;
        }
    }
    // Gaussian elimination with partial pivoting
    for col in 0..n {
        let pivot = (col..n).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs())).unwrap_or(col);
        a.swap(col, pivot);
        let d = a[col][col];
        if d.abs() < 1e-12 {
            return vec![0.0; n];
        }
        for v in &mut a[col][col..] {
            *v /= d;
        }
        let pivot_row = a[col].clone();
        for (row, line) in a.iter_mut().enumerate() {
            if row != col {
                let f = line[col];
                for (v, p) in line[col..].iter_mut().zip(&pivot_row[col..]) {
                    *v -= f * p;
                }
            }
        }
    }
    a.iter().map(|r| r[n]).collect()
}

fn encode_opcodes(corr: &RadialCorrection) -> Vec<u8> {
    let radii: Vec<f64> = (0..=SAMPLES).map(|i| i as f64 / SAMPLES as f64).collect();
    let mut ops: Vec<(u32, Vec<u8>)> = Vec::new();
    if corr.distortion.is_some() || corr.ca.is_some() {
        let planes: Vec<Vec<f64>> = (0..3)
            .map(|c| {
                let pts: Vec<(f64, f64)> = radii.iter().map(|&r| (r, corr.sample_scale(r as f32)[c] as f64)).collect();
                fit(&pts, &[0, 2, 4, 6], 1)
            })
            .collect();
        let mut p = Vec::new();
        p.extend(3u32.to_be_bytes());
        for k in &planes {
            for v in k.iter().chain(&[0.0, 0.0]) {
                p.extend(v.to_be_bytes());
            }
        }
        p.extend(0.5f64.to_be_bytes());
        p.extend(0.5f64.to_be_bytes());
        ops.push((OP_WARP_RECTILINEAR, p));
    }
    if corr.vignetting.is_some() {
        let pts: Vec<(f64, f64)> = radii.iter().map(|&r| (r, corr.sample_gain(r as f32) as f64 - 1.0)).collect();
        let k = fit(&pts, &[2, 4, 6, 8, 10], 0);
        let mut p = Vec::new();
        for v in k.iter().chain(&[0.5, 0.5]) {
            p.extend(v.to_be_bytes());
        }
        ops.push((OP_FIX_VIGNETTE_RADIAL, p));
    }
    if ops.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.extend((ops.len() as u32).to_be_bytes());
    for (id, params) in ops {
        out.extend(id.to_be_bytes());
        out.extend(OP_VERSION.to_be_bytes());
        out.extend(OP_FLAG_OPTIONAL.to_be_bytes());
        out.extend((params.len() as u32).to_be_bytes());
        out.extend(params);
    }
    out
}

/// Turns an OpcodeList3 into a radial correction. Opcodes centred away from the image
/// centre, or with tangential terms, are not handled and give None.
fn decode_opcodes(data: &[u8]) -> Option<RadialCorrection> {
    let be32 = |b: &[u8], o: usize| b.get(o..o + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap()));
    let f64_at = |b: &[u8], o: usize| b.get(o..o + 8).map(|s| f64::from_be_bytes(s.try_into().unwrap()));
    let count = be32(data, 0)?.min(64);
    let mut pos = 4usize;
    let mut warp: Option<Vec<[f64; 4]>> = None;
    let mut vignette: Option<[f64; 5]> = None;
    for _ in 0..count {
        let id = be32(data, pos)?;
        let size = be32(data, pos + 12)? as usize;
        let p = data.get(pos + 16..pos + 16 + size)?;
        pos += 16 + size;
        let centred = |o: usize| Some((f64_at(p, o)? - 0.5).abs() < 0.02 && (f64_at(p, o + 8)? - 0.5).abs() < 0.02);
        match id {
            OP_WARP_RECTILINEAR => {
                let planes = be32(p, 0)? as usize;
                if !(planes == 1 || planes == 3) || !centred(4 + planes * 48)? {
                    return None;
                }
                let mut k = Vec::new();
                for plane in 0..planes {
                    let o = 4 + plane * 48;
                    let c: Vec<f64> = (0..6).map(|i| f64_at(p, o + i * 8)).collect::<Option<_>>()?;
                    if c[4].abs() > 1e-4 || c[5].abs() > 1e-4 {
                        return None;
                    }
                    k.push([c[0], c[1], c[2], c[3]]);
                }
                warp = Some(k);
            }
            OP_FIX_VIGNETTE_RADIAL => {
                if !centred(40)? {
                    return None;
                }
                let c: Vec<f64> = (0..5).map(|i| f64_at(p, i * 8)).collect::<Option<_>>()?;
                vignette = Some([c[0], c[1], c[2], c[3], c[4]]);
            }
            _ => {}
        }
    }
    let knots: Vec<f32> = (1..=SAMPLES).map(|i| i as f32 / SAMPLES as f32).collect();
    let poly = |k: &[f64; 4], r: f64| k[0] + k[1] * r.powi(2) + k[2] * r.powi(4) + k[3] * r.powi(6);
    let (distortion, ca) = match &warp {
        Some(k) => {
            let g = &k[k.len() / 2];
            let d: Vec<f32> = knots.iter().map(|&r| ((poly(g, r as f64) - 1.0) * 100.0) as f32).collect();
            let ca = (k.len() == 3).then(|| {
                let diff = |c: &[f64; 4]| knots.iter().map(|&r| (poly(c, r as f64) - poly(g, r as f64)) as f32).collect();
                (diff(&k[0]), diff(&k[2]))
            });
            (Some(d), ca)
        }
        None => (None, None),
    };
    let vignetting = vignette.map(|k| {
        knots
            .iter()
            .map(|&r| {
                let r2 = r as f64 * r as f64;
                let gain = 1.0 + k[0] * r2 + k[1] * r2.powi(2) + k[2] * r2.powi(3) + k[3] * r2.powi(4) + k[4] * r2.powi(5);
                (100.0 / gain.max(0.1)) as f32
            })
            .collect()
    });
    let corr = RadialCorrection { knots, distortion, ca, vignetting };
    corr.is_active().then_some(corr)
}

/// Lens corrections of a DNG file (OpcodeList3), if any.
pub fn read_corrections(path: &Path) -> Option<RadialCorrection> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = [0u8; 8];
    f.read_exact(&mut head).ok()?;
    let le = match &head[..4] {
        b"II*\0" => true,
        b"MM\0*" => false,
        _ => return None,
    };
    let u16_of = |b: [u8; 2]| if le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) };
    let u32_of = |b: [u8; 4]| if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) };
    let mut read = |off: u64, n: usize| -> Option<Vec<u8>> {
        if off.checked_add(n as u64)? > len || n > 1 << 20 {
            return None;
        }
        f.seek(SeekFrom::Start(off)).ok()?;
        let mut v = vec![0u8; n];
        f.read_exact(&mut v).ok()?;
        Some(v)
    };
    let mut queue = vec![u32_of(head[4..8].try_into().ok()?) as u64];
    let (mut is_dng, mut opcodes) = (false, None);
    let mut visited = 0;
    while let Some(ifd) = queue.pop() {
        visited += 1;
        if visited > 16 {
            break;
        }
        let n = u16_of(read(ifd, 2)?.try_into().ok()?).min(512) as usize;
        let entries = read(ifd + 2, n * 12)?;
        for e in entries.as_chunks::<12>().0 {
            let tag = u16_of([e[0], e[1]]);
            let kind = u16_of([e[2], e[3]]);
            let count = u32_of([e[4], e[5], e[6], e[7]]);
            let value = u32_of([e[8], e[9], e[10], e[11]]);
            match tag {
                TAG_DNG_VERSION => is_dng = true,
                TAG_SUB_IFDS if kind == 4 || kind == 13 => {
                    if count == 1 {
                        queue.push(value as u64);
                    } else if let Some(v) = read(value as u64, count.min(8) as usize * 4) {
                        queue.extend(v.as_chunks::<4>().0.iter().map(|b| u32_of(*b) as u64));
                    }
                }
                TAG_OPCODE_LIST3 if count > 4 => opcodes = read(value as u64, count as usize),
                _ => {}
            }
        }
    }
    if !is_dng {
        return None;
    }
    decode_opcodes(&opcodes?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RadialCorrection {
        RadialCorrection {
            knots: vec![0.3535, 0.5, 0.6127, 0.707, 0.7908, 0.8662, 0.9352, 1.0, 1.0606],
            distortion: Some(vec![0.0385, -0.2249, -0.4995, -0.7726, -1.036, -1.309, -1.5724, -1.8471, -2.1218]),
            ca: Some((vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0001, 0.0002, 0.0002, 0.0003], vec![0.0; 9])),
            vignetting: Some(vec![98.84, 98.08, 97.29, 96.51, 95.7, 94.55, 92.96, 90.98, 89.08]),
        }
    }

    #[test]
    fn opcodes_round_trip_closely() {
        let corr = sample();
        let back = decode_opcodes(&encode_opcodes(&corr)).unwrap();
        for i in 0..=20 {
            let r = i as f32 / 20.0;
            let (a, b) = (corr.sample_scale(r), back.sample_scale(r));
            for c in 0..3 {
                // Displacement error as a share of the half-diagonal. The polynomial is
                // smoother than the camera's piecewise-linear knots; 2.5e-4 is about a
                // photosite on a 100 MP sensor, worst near the first knot
                let err = r * (a[c] - b[c]).abs();
                assert!(err < 2.5e-4, "scale r={r} c{c}: {} vs {} ({err})", a[c], b[c]);
            }
            assert!((corr.sample_gain(r) - back.sample_gain(r)).abs() < 3e-3, "gain r={r}");
        }
    }

    #[test]
    fn written_dng_is_read_back() {
        let dir = std::env::temp_dir().join(format!("omastudio-dng-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.dng");
        let (w, h) = (37, 23);
        let rgb: Vec<u16> = (0..w * h * 3).map(|i| (i * 7 % 65536) as u16).collect();
        let corr = sample();
        let info = DngInfo {
            make: "FUJIFILM",
            model: "GFX100S",
            unique_model: "FUJIFILM GFX100S Pixel Shift",
            orientation: 1,
            color_matrix: [[1.7167, -0.3557, -0.2534], [-0.6667, 1.6165, 0.0158], [0.0176, -0.0428, 0.9421]],
            lens: Some(&corr),
        };
        write_linear_rgb(&path, &rgb, w, h, &info).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // Strip data is the image, row by row, little-endian
        let tail = &bytes[bytes.len() - w * h * 6..];
        assert_eq!(u16::from_le_bytes([tail[6], tail[7]]), rgb[3]);
        assert!(read_corrections(&path).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
