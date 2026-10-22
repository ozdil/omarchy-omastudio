//! Reads the Fujifilm MakerNote from a RAF (or a Fujifilm JPEG) without external tools.
//!
//! A RAF starts with a 16-byte "FUJIFILMCCD-RAW " magic and stores an embedded JPEG
//! (offset and length at bytes 84 and 88, big-endian). That JPEG's EXIF block holds the
//! MakerNote: "FUJIFILM", then a little-endian IFD whose offsets are relative to the
//! start of the MakerNote. Tag numbers and value encodings follow ExifTool's FujiFilm
//! table.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Bytes read from the start of the embedded JPEG; EXIF (APP1) is at most 64 KiB.
const JPEG_HEAD: u64 = 128 * 1024;
/// Upper bound on MakerNote IFD entries, against corrupt counts.
const MAX_ENTRIES: usize = 1024;

/// One MakerNote value, decoded to the integers the tags use.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Ints(Vec<i64>),
    Text(String),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn int(&self) -> Option<i64> {
        match self {
            Self::Ints(v) => v.first().copied(),
            Self::Bytes(b) => b.first().map(|&x| x as i8 as i64),
            Self::Text(_) => None,
        }
    }

    pub fn ints(&self) -> &[i64] {
        match self {
            Self::Ints(v) => v,
            _ => &[],
        }
    }
}

/// MakerNote tags by number.
pub type Tags = HashMap<u16, Value>;

/// Reads the Fujifilm MakerNote of a RAF or JPEG file.
pub fn read_file(path: &Path) -> Result<Tags, String> {
    let mut file = File::open(path).map_err(|e| format!("Cannot open {}: {}", path.display(), e))?;
    let mut head = [0u8; 92];
    file.read_exact(&mut head).map_err(|e| format!("File too short: {}", e))?;

    let jpeg = if head.starts_with(b"FUJIFILMCCD-RAW") {
        let offset = u32::from_be_bytes([head[84], head[85], head[86], head[87]]) as u64;
        let length = u32::from_be_bytes([head[88], head[89], head[90], head[91]]) as u64;
        let mut buf = vec![0u8; length.min(JPEG_HEAD) as usize];
        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        file.read_exact(&mut buf).map_err(|e| format!("Embedded JPEG truncated: {}", e))?;
        buf
    } else if head.starts_with(&[0xFF, 0xD8]) {
        let mut buf = head.to_vec();
        file.take(JPEG_HEAD - buf.len() as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
        buf
    } else {
        return Err("Not a Fujifilm RAF or JPEG file".to_string());
    };
    parse_jpeg(&jpeg)
}

/// Finds the EXIF block of a JPEG and returns its Fujifilm MakerNote tags.
pub fn parse_jpeg(jpeg: &[u8]) -> Result<Tags, String> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err("Embedded image is not a JPEG".to_string());
    }
    let mut pos = 2;
    while pos + 4 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            break;
        }
        let marker = jpeg[pos + 1];
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if marker == 0xDA || len < 2 {
            break;
        }
        let seg = jpeg.get(pos + 4..pos + 2 + len).ok_or("Truncated JPEG segment")?;
        if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
            return parse_exif(&seg[6..]);
        }
        pos += 2 + len;
    }
    Err("No EXIF block found".to_string())
}

#[derive(Clone, Copy)]
struct Tiff<'a> {
    data: &'a [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let b = self.data.get(at..at + 2)?;
        Some(if self.le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b = self.data.get(at..at + 4)?;
        let a = [b[0], b[1], b[2], b[3]];
        Some(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }

    /// (tag, type, count, value-or-offset field position) of each entry of the IFD at `at`.
    fn entries(&self, at: usize) -> Vec<(u16, u16, u32, usize)> {
        let Some(n) = self.u16(at) else { return Vec::new() };
        (0..(n as usize).min(MAX_ENTRIES))
            .filter_map(|i| {
                let e = at + 2 + i * 12;
                Some((self.u16(e)?, self.u16(e + 2)?, self.u32(e + 4)?, e + 8))
            })
            .collect()
    }
}

fn parse_exif(tiff: &[u8]) -> Result<Tags, String> {
    let le = match tiff.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err("Bad TIFF header in EXIF".to_string()),
    };
    let t = Tiff { data: tiff, le };
    let ifd0 = t.u32(4).ok_or("Bad TIFF header")? as usize;
    let exif_ifd = t
        .entries(ifd0)
        .into_iter()
        .find(|e| e.0 == 0x8769)
        .and_then(|e| t.u32(e.3))
        .ok_or("No EXIF IFD")? as usize;
    let (_, _, count, field) = t
        .entries(exif_ifd)
        .into_iter()
        .find(|e| e.0 == 0x927C)
        .ok_or("No MakerNote")?;
    let offset = t.u32(field).ok_or("Bad MakerNote offset")? as usize;
    let note = tiff.get(offset..offset + count as usize).ok_or("MakerNote out of range")?;
    parse_makernote(note)
}

/// Parses a Fujifilm MakerNote ("FUJIFILM" + little-endian IFD, offsets from its start).
pub fn parse_makernote(note: &[u8]) -> Result<Tags, String> {
    if !note.starts_with(b"FUJIFILM") {
        return Err("Not a Fujifilm MakerNote".to_string());
    }
    let t = Tiff { data: note, le: true };
    let ifd = t.u32(8).ok_or("Bad MakerNote header")? as usize;
    let mut tags = Tags::new();
    for (tag, kind, count, field) in t.entries(ifd) {
        let size = match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 => 4,
            5 | 10 => 8,
            _ => continue,
        };
        let Some(total) = (count as usize).checked_mul(size) else { continue };
        if total > 64 * 1024 {
            continue;
        }
        let start = if total <= 4 {
            field
        } else {
            match t.u32(field) {
                Some(o) => o as usize,
                None => continue,
            }
        };
        let Some(raw) = note.get(start..start + total) else { continue };
        let value = match kind {
            2 => Value::Text(String::from_utf8_lossy(raw).trim_end_matches('\0').trim().to_string()),
            1 | 7 => Value::Bytes(raw.to_vec()),
            6 => Value::Ints(raw.iter().map(|&b| b as i8 as i64).collect()),
            3 => Value::Ints(raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c) as i64).collect()),
            8 => Value::Ints(raw.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c) as i64).collect()),
            4 => Value::Ints(raw.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c) as i64).collect()),
            9 => Value::Ints(raw.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c) as i64).collect()),
            // Rationals: numerator only is enough for the tags used here
            5 | 10 => Value::Ints(
                raw.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| {
                        let n = i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as i64;
                        let d = i32::from_le_bytes([c[4], c[5], c[6], c[7]]) as i64;
                        if d != 0 { n / d } else { 0 }
                    })
                    .collect(),
            ),
            _ => continue,
        };
        tags.insert(tag, value);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a MakerNote with the given (tag, type, values) entries.
    pub(crate) fn makernote(entries: &[(u16, u16, Vec<i64>)]) -> Vec<u8> {
        let mut out = b"FUJIFILM".to_vec();
        out.extend_from_slice(&12u32.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let data_start = 12 + 2 + entries.len() * 12 + 4;
        let mut data = Vec::new();
        for (tag, kind, vals) in entries {
            let size = match kind {
                3 | 8 => 2,
                _ => 4,
            };
            let mut bytes = Vec::new();
            for v in vals {
                if size == 2 {
                    bytes.extend_from_slice(&(*v as i16).to_le_bytes());
                } else {
                    bytes.extend_from_slice(&(*v as i32).to_le_bytes());
                }
            }
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&(vals.len() as u32).to_le_bytes());
            if bytes.len() <= 4 {
                bytes.resize(4, 0);
                out.extend_from_slice(&bytes);
            } else {
                out.extend_from_slice(&((data_start + data.len()) as u32).to_le_bytes());
                data.extend_from_slice(&bytes);
            }
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn reads_inline_and_offset_values() {
        let note = makernote(&[(0x1401, 3, vec![0x600]), (0x100a, 9, vec![40, -40]), (0x1041, 9, vec![-16])]);
        let tags = parse_makernote(&note).unwrap();
        assert_eq!(tags[&0x1401].int(), Some(0x600));
        assert_eq!(tags[&0x100a].ints(), &[40, -40]);
        assert_eq!(tags[&0x1041].int(), Some(-16));
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(parse_makernote(b"FUJIFILM\xff\xff\xff\xff").unwrap().is_empty());
        assert!(parse_makernote(b"NIKON").is_err());
        assert!(parse_jpeg(&[0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF]).is_err());
        let mut note = makernote(&[(0x1401, 3, vec![0x600])]);
        note[14] = 0xff; // absurd entry count
        let _ = parse_makernote(&note);
    }
}
