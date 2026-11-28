//! Reads the Fujifilm MakerNote from a RAF (or a Fujifilm JPEG) without external tools.
//!
//! A RAF starts with a 16-byte "FUJIFILMCCD-RAW " magic and stores an embedded JPEG
//! (offset and length at bytes 84 and 88, big-endian). That JPEG's EXIF block holds the
//! MakerNote: "FUJIFILM", then a little-endian IFD whose offsets are relative to the
//! start of the MakerNote. Tag numbers and value encodings follow ExifTool's FujiFilm
//! table.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Bytes read from the start of the embedded JPEG; EXIF (APP1) is at most 64 KiB.
const JPEG_HEAD: u64 = 128 * 1024;
/// Upper bound on MakerNote IFD entries, against corrupt counts.
const MAX_ENTRIES: usize = 1024;

/// Opens a regular file without following symlinks and returns `(file, byte_len)`.
fn open_regular_nofollow(path: &Path) -> Result<(File, u64), String> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    opts.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = opts.open(path).map_err(|e| format!("Cannot open {}: {}", path.display(), e))?;
    let meta = file.metadata().map_err(|e| format!("Cannot stat {}: {}", path.display(), e))?;
    if !meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    Ok((file, meta.len()))
}

/// One MakerNote value, decoded to the integers the tags use.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Ints(Vec<i64>),
    /// Rationals, kept as fractions (pixel shift offsets are half pixels).
    Ratios(Vec<f64>),
    Text(String),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn int(&self) -> Option<i64> {
        match self {
            Self::Ints(v) => v.first().copied(),
            Self::Ratios(v) => v.first().map(|x| x.trunc() as i64),
            Self::Bytes(b) => b.first().map(|&x| x as i8 as i64),
            Self::Text(_) => None,
        }
    }

    /// Numeric values as floats (rationals keep their fraction).
    pub fn floats(&self) -> Vec<f64> {
        match self {
            Self::Ints(v) => v.iter().map(|&x| x as f64).collect(),
            Self::Ratios(v) => v.clone(),
            _ => Vec::new(),
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
    let (mut file, file_len) = open_regular_nofollow(path)?;
    let mut head = [0u8; 92];
    file.read_exact(&mut head).map_err(|e| format!("File too short: {}", e))?;

    let jpeg = if head.starts_with(b"FUJIFILMCCD-RAW") {
        let offset = u32::from_be_bytes([head[84], head[85], head[86], head[87]]) as u64;
        let length = u32::from_be_bytes([head[88], head[89], head[90], head[91]]) as u64;
        let read_len = length.min(JPEG_HEAD);
        if read_len == 0 || offset.checked_add(read_len).is_none_or(|end| end > file_len) {
            return Err("Embedded JPEG truncated".to_string());
        }
        let mut buf = vec![0u8; read_len as usize];
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

/// The full-size JPEG the camera stored inside a RAF.
pub fn embedded_jpeg(path: &Path) -> Result<Vec<u8>, String> {
    const MAX: u64 = 64 * 1024 * 1024;
    let (mut file, file_len) = open_regular_nofollow(path)?;
    let mut head = [0u8; 92];
    file.read_exact(&mut head).map_err(|e| format!("File too short: {}", e))?;
    if !head.starts_with(b"FUJIFILMCCD-RAW") {
        return Err("Not a Fujifilm RAF file".to_string());
    }
    let offset = u32::from_be_bytes([head[84], head[85], head[86], head[87]]) as u64;
    let length = u32::from_be_bytes([head[88], head[89], head[90], head[91]]) as u64;
    if length == 0 || length > MAX || offset.checked_add(length).is_none_or(|end| end > file_len) {
        return Err("RAF has no usable embedded JPEG".to_string());
    }
    let mut buf = vec![0u8; length as usize];
    file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    file.read_exact(&mut buf).map_err(|e| format!("Embedded JPEG truncated: {}", e))?;
    Ok(buf)
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
            5 | 10 => Value::Ratios(
                raw.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| {
                        let (n, d) = if kind == 5 {
                            (u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64, u32::from_le_bytes([c[4], c[5], c[6], c[7]]) as f64)
                        } else {
                            (i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64, i32::from_le_bytes([c[4], c[5], c[6], c[7]]) as f64)
                        };
                        if d != 0.0 { n / d } else { 0.0 }
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

    #[test]
    fn embedded_jpeg_rejects_oversized_claimed_length_and_symlinks() {
        let base = std::env::temp_dir().join(format!("omastudio-mn-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let raf = base.join("truncated.raf");
        let mut header = [0u8; 92];
        header[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
        // Claim a 32 MiB embedded JPEG starting at offset 92 inside a 92-byte file
        header[84..88].copy_from_slice(&92u32.to_be_bytes());
        header[88..92].copy_from_slice(&(32u32 * 1024 * 1024).to_be_bytes());
        std::fs::write(&raf, header).unwrap();

        assert!(embedded_jpeg(&raf).is_err());
        assert!(read_file(&raf).is_err());

        #[cfg(unix)]
        {
            let link = base.join("link.raf");
            std::os::unix::fs::symlink(&raf, &link).unwrap();
            assert!(embedded_jpeg(&link).is_err());
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
