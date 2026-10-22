//! Lens corrections stored by Fujifilm cameras in the FujiIFD of a RAF file: the
//! data the camera itself uses for its JPEGs.
//!
//! The RAF header points (at 0x64) to a little-endian TIFF structure whose sub-IFD
//! holds three tables of signed rationals (tag numbers as listed by ExifTool):
//!
//! - 0xF00B GeometricDistortionParams: `[scale, knots.., distortion %..]`
//! - 0xF00F ChromaticAberrationParams: `[scale, knots.., red.., blue.., scale]`
//! - 0xF010 VignettingParams: `[scale, knots.., brightness %..]`
//!
//! Knots are radii normalised to the half-diagonal. Checked against an X-T30 RAF:
//! remapping the raw development with these values brings it within half a pixel of
//! the camera's own JPEG (2.8 px without).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::raw::lens::RadialCorrection;

const TAG_SUB_IFD: u16 = 0xF000;
const TAG_DISTORTION: u16 = 0xF00B;
const TAG_CA: u16 = 0xF00F;
const TAG_VIGNETTING: u16 = 0xF010;
/// Longest table read; real files hold 19 to 64 values.
const MAX_VALUES: u32 = 128;

/// Reads the corrections of a RAF file. None for other files, or a RAF without them.
pub fn read(path: &Path) -> Option<RadialCorrection> {
    let mut f = File::open(path).ok()?;
    let mut header = [0u8; 0x6c];
    f.read_exact(&mut header).ok()?;
    if !header.starts_with(b"FUJIFILMCCD-RAW") {
        return None;
    }
    let base = u32::from_be_bytes(header[0x64..0x68].try_into().ok()?) as u64;
    let file_len = f.metadata().ok()?.len();
    if base == 0 || base + 8 > file_len {
        return None;
    }
    let mut ifd = Ifd { f, base, file_len };
    let mut tiff = [0u8; 8];
    ifd.read_at(0, &mut tiff)?;
    if &tiff[..4] != b"II*\0" {
        return None;
    }
    let first = u32::from_le_bytes(tiff[4..8].try_into().ok()?);
    let mut tables = Tables::default();
    ifd.walk(first, &mut tables, 0)?;
    tables.into_correction()
}

#[derive(Default)]
struct Tables {
    distortion: Option<Vec<f32>>,
    ca: Option<Vec<f32>>,
    vignetting: Option<Vec<f32>>,
}

impl Tables {
    fn into_correction(self) -> Option<RadialCorrection> {
        // Knot count from the distortion table: [scale, k knots, k values]
        let dist = self.distortion?;
        if dist.len() < 3 || dist.len().is_multiple_of(2) {
            return None;
        }
        let k = (dist.len() - 1) / 2;
        let knots = dist[1..=k].to_vec();
        if knots.windows(2).any(|p| p[1] <= p[0]) || knots[0] <= 0.0 {
            return None;
        }
        let distortion = Some(dist[k + 1..].to_vec()).filter(|d| d.iter().all(|v| v.abs() < 20.0));
        let same_knots = |t: &[f32]| t.len() > k && t[1..=k].iter().zip(&knots).all(|(a, b)| (a - b).abs() < 1e-3);
        let ca = self
            .ca
            .filter(|t| t.len() > 3 * k && same_knots(t))
            .map(|t| (t[k + 1..2 * k + 1].to_vec(), t[2 * k + 1..3 * k + 1].to_vec()))
            .filter(|(r, b)| r.iter().chain(b).all(|v| v.abs() < 0.05));
        let vignetting = self
            .vignetting
            .filter(|t| t.len() == 1 + 2 * k && same_knots(t))
            .map(|t| t[k + 1..].to_vec())
            .filter(|v| v.iter().all(|p| (10.0..=150.0).contains(p)));
        let corr = RadialCorrection { knots, distortion, ca, vignetting };
        corr.is_active().then_some(corr)
    }
}

struct Ifd {
    f: File,
    base: u64,
    file_len: u64,
}

impl Ifd {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Option<()> {
        let at = self.base.checked_add(offset)?;
        if at.checked_add(buf.len() as u64)? > self.file_len {
            return None;
        }
        self.f.seek(SeekFrom::Start(at)).ok()?;
        self.f.read_exact(buf).ok()
    }

    fn walk(&mut self, offset: u32, tables: &mut Tables, depth: u32) -> Option<()> {
        if depth > 2 {
            return None;
        }
        let mut count = [0u8; 2];
        self.read_at(offset as u64, &mut count)?;
        let n = u16::from_le_bytes(count).min(256) as usize;
        let mut entries = vec![0u8; n * 12];
        self.read_at(offset as u64 + 2, &mut entries)?;
        for e in entries.as_chunks::<12>().0 {
            let tag = u16::from_le_bytes([e[0], e[1]]);
            let kind = u16::from_le_bytes([e[2], e[3]]);
            let cnt = u32::from_le_bytes([e[4], e[5], e[6], e[7]]);
            let value = u32::from_le_bytes([e[8], e[9], e[10], e[11]]);
            match tag {
                TAG_SUB_IFD => {
                    let _ = self.walk(value, tables, depth + 1);
                }
                TAG_DISTORTION | TAG_CA | TAG_VIGNETTING if kind == 10 && cnt > 0 && cnt <= MAX_VALUES => {
                    let mut raw = vec![0u8; cnt as usize * 8];
                    if self.read_at(value as u64, &mut raw).is_none() {
                        continue;
                    }
                    let vals: Vec<f32> = raw
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|r| {
                            let num = i32::from_le_bytes([r[0], r[1], r[2], r[3]]);
                            let den = i32::from_le_bytes([r[4], r[5], r[6], r[7]]);
                            if den == 0 { 0.0 } else { num as f32 / den as f32 }
                        })
                        .collect();
                    let slot = match tag {
                        TAG_DISTORTION => &mut tables.distortion,
                        TAG_CA => &mut tables.ca,
                        _ => &mut tables.vignetting,
                    };
                    *slot = Some(vals);
                }
                _ => {}
            }
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knots() -> Vec<f32> {
        vec![0.3535, 0.5, 0.6127, 0.707, 0.7908, 0.8662, 0.9352, 1.0, 1.0606]
    }

    #[test]
    fn x_t30_layout_is_understood() {
        let k = knots();
        let mut dist = vec![416.5556];
        dist.extend(&k);
        dist.extend([0.0385, -0.2249, -0.4995, -0.7726, -1.036, -1.309, -1.5724, -1.8471, -2.1218]);
        let mut ca = vec![416.5556];
        ca.extend(&k);
        ca.extend([0.0; 9]);
        ca.extend([0.0001; 9]);
        ca.push(416.5556);
        let mut vig = vec![416.5556];
        vig.extend(&k);
        vig.extend([98.84, 98.08, 97.29, 96.51, 95.7, 94.55, 92.96, 90.98, 89.08]);
        let corr = Tables { distortion: Some(dist), ca: Some(ca), vignetting: Some(vig) }.into_correction().unwrap();
        assert_eq!(corr.knots.len(), 9);
        assert!((corr.distortion.as_ref().unwrap()[8] + 2.1218).abs() < 1e-4);
        assert!((corr.ca.as_ref().unwrap().1[0] - 0.0001).abs() < 1e-6);
        assert!((corr.vignetting.as_ref().unwrap()[8] - 89.08).abs() < 1e-3);
    }

    #[test]
    fn inconsistent_tables_are_ignored() {
        let mut dist = vec![416.5556];
        dist.extend(knots());
        dist.extend([0.0; 9]);
        // Vignetting table with a different length is dropped, distortion kept
        let corr = Tables { distortion: Some(dist), ca: None, vignetting: Some(vec![1.0; 64]) }.into_correction().unwrap();
        assert!(corr.vignetting.is_none() && corr.distortion.is_some());
        assert!(Tables { distortion: Some(vec![1.0, 2.0]), ..Default::default() }.into_correction().is_none());
    }
}
