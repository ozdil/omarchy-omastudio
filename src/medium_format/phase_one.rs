//! Phase One and Leaf IIQ (and Leaf MOS backs that carry the same directory).
//!
//! An IIQ is a TIFF whose header is followed, at byte 8, by "IIII" and a "Raw" magic.
//! The word after the magic points to Phase One's own directory: an entry count, a
//! reserved word, then 16-byte entries (tag, type, length, value-or-offset), offsets
//! counting from byte 8. Capture One reads the back's settings from it; the tags used
//! here were checked against IQ4 150MP, IQ3 100MP, P45+ and Leaf Credo samples.

use crate::recipe::Recipe;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const BASE: u64 = 8;
const MAX_ENTRIES: u32 = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseOneInfo {
    /// Back model and firmware, for example "IQ4 150MP" and "v7.00.7 (FWP)".
    pub back: String,
    pub firmware: Option<String>,
    pub back_serial: Option<String>,
    /// Camera body the back sat on ("XF", "645DF+", a technical camera...).
    pub body: Option<String>,
    pub lens: Option<String>,
    pub viewfinder: Option<String>,
    pub iso: Option<u32>,
    /// Sensor temperature at capture, in °C.
    pub sensor_temperature: Option<f32>,
    /// IIQ raw format code (compression and bit depth).
    pub raw_format: Option<u32>,
    pub raw_width: u32,
    pub raw_height: u32,
    pub crop: Option<super::Rect>,
}

impl PhaseOneInfo {
    pub fn read(path: &Path) -> Option<Self> {
        let mut f = File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        let mut head = [0u8; 20];
        f.read_exact(&mut head).ok()?;
        if &head[8..12] != b"IIII" || u32::from_le_bytes([head[12], head[13], head[14], head[15]]) >> 8 != 0x526177 {
            return None;
        }
        let dir = u32::from_le_bytes([head[16], head[17], head[18], head[19]]) as u64 + BASE;
        let mut word = [0u8; 8];
        f.seek(SeekFrom::Start(dir)).ok()?;
        f.read_exact(&mut word).ok()?;
        let n = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        if n == 0 || n > MAX_ENTRIES {
            return None;
        }
        let mut raw = vec![0u8; n as usize * 16];
        f.read_exact(&mut raw).ok()?;

        let mut info = Self {
            back: String::new(),
            firmware: None,
            back_serial: None,
            body: None,
            lens: None,
            viewfinder: None,
            iso: None,
            sensor_temperature: None,
            raw_format: None,
            raw_width: 0,
            raw_height: 0,
            crop: None,
        };
        let (mut left, mut top, mut width, mut height) = (0, 0, 0, 0);
        for e in raw.as_chunks::<16>().0 {
            let w = |i: usize| u32::from_le_bytes([e[i], e[i + 1], e[i + 2], e[i + 3]]);
            let (tag, length, data) = (w(0), w(8), w(12));
            let mut text = || -> Option<String> {
                if length == 0 || length > 256 || BASE + data as u64 + length as u64 > len {
                    return None;
                }
                let mut b = vec![0u8; length as usize];
                f.seek(SeekFrom::Start(BASE + data as u64)).ok()?;
                f.read_exact(&mut b).ok()?;
                let s = String::from_utf8_lossy(b.split(|&c| c == 0).next().unwrap_or(&[])).trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            match tag {
                0x0102 => info.back_serial = text(),
                0x0105 => info.iso = (data > 0 && data < 1_000_000).then_some(data),
                0x0108 => info.raw_width = data,
                0x0109 => info.raw_height = data,
                0x010a => left = data,
                0x010b => top = data,
                0x010c => width = data,
                0x010d => height = data,
                0x010e => info.raw_format = Some(data),
                0x0210 => info.sensor_temperature = Some(f32::from_bits(data)).filter(|t| t.is_finite() && t.abs() < 150.0),
                0x0301 => {
                    if let Some(s) = text() {
                        let (back, fw) = s.split_once(',').unwrap_or((&s, ""));
                        info.back = back.trim().to_string();
                        info.firmware = Some(fw.trim().to_string()).filter(|f| !f.is_empty());
                    }
                }
                0x0410 => info.body = text(),
                0x0412 => info.lens = text(),
                0x0455 => info.viewfinder = text(),
                _ => {}
            }
        }
        if width > 0 && height > 0 && left + width <= info.raw_width && top + height <= info.raw_height {
            info.crop = Some(super::Rect { x: left, y: top, width, height });
        }
        Some(info)
    }

    /// Starting recipe. Capture One renders backs fairly flat and lets the user build
    /// the look, so only capture sharpening suited to the back's pixel pitch is set.
    #[allow(clippy::field_reassign_with_default)]
    pub fn to_recipe(&self) -> Recipe {
        let mut r = Recipe::default();
        r.preset_name = Some(format!("Camera: {}", if self.back.is_empty() { "Phase One" } else { &self.back }));
        // 150 MP and 100 MP backs have small photosites and need less sharpening
        r.sharpness = if self.raw_width >= 11_000 { 20.0 } else { 30.0 };
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iiq(entries: &[(u32, u32, u32)], strings: &[(u32, &str)]) -> Vec<u8> {
        let mut out = b"II*\0\0\0\0\0IIII".to_vec();
        out.extend_from_slice(&0x5261_7743u32.to_le_bytes());
        // Directory right after the header, strings after it
        out.extend_from_slice(&12u32.to_le_bytes());
        let n = entries.len() + strings.len();
        out.extend_from_slice(&(n as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let mut data = Vec::new();
        let data_rel = 12 + 8 + n as u32 * 16;
        for &(tag, kind, value) in entries {
            for w in [tag, kind, 4, value] {
                out.extend_from_slice(&w.to_le_bytes());
            }
        }
        for &(tag, s) in strings {
            let off = data_rel + data.len() as u32;
            for w in [tag, 1, s.len() as u32 + 1, off] {
                out.extend_from_slice(&w.to_le_bytes());
            }
            data.extend_from_slice(s.as_bytes());
            data.push(0);
        }
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn reads_back_lens_and_crop() {
        let bytes = iiq(
            &[(0x0108, 4, 14308), (0x0109, 4, 10760), (0x010a, 4, 102), (0x010b, 4, 106), (0x010c, 4, 14204), (0x010d, 4, 10652), (0x0105, 4, 100), (0x0210, 4, 27.3f32.to_bits())],
            &[(0x0301, "IQ4 150MP, v7.00.7 (FWP)"), (0x0102, "JD010188"), (0x0410, "XF"), (0x0412, "45 mm f/22.0-2.8")],
        );
        let p = std::env::temp_dir().join(format!("omastudio-iiq-{}.iiq", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        let info = PhaseOneInfo::read(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(info.back, "IQ4 150MP");
        assert_eq!(info.firmware.as_deref(), Some("v7.00.7 (FWP)"));
        assert_eq!(info.back_serial.as_deref(), Some("JD010188"));
        assert_eq!(info.body.as_deref(), Some("XF"));
        assert_eq!(info.lens.as_deref(), Some("45 mm f/22.0-2.8"));
        assert_eq!(info.iso, Some(100));
        assert!((info.sensor_temperature.unwrap() - 27.3).abs() < 1e-4);
        assert_eq!(info.crop, Some(super::super::Rect { x: 102, y: 106, width: 14204, height: 10652 }));
        assert!(info.to_recipe().preset_name.unwrap().contains("IQ4"));
    }

    #[test]
    fn plain_tiffs_are_not_iiq() {
        let p = std::env::temp_dir().join(format!("omastudio-notiiq-{}.tif", std::process::id()));
        std::fs::write(&p, b"II*\0\x08\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0").unwrap();
        assert!(PhaseOneInfo::read(&p).is_none());
        std::fs::remove_file(&p).ok();
    }
}
