//! A small TIFF/IFD reader for the metadata of medium format RAW files (3FR, FFF, IIQ,
//! MOS, MEF, PEF are all TIFF containers). It reads entries on demand from the file,
//! so a 200 MB raw is never loaded to look at a few tags.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Upper bound on entries per IFD, against corrupt counts.
const MAX_ENTRIES: usize = 1024;
/// Largest value read for one tag (MakerNote blobs are a few KiB).
const MAX_VALUE: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Ints(Vec<i64>),
    Floats(Vec<f64>),
    Text(String),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn int(&self) -> Option<i64> {
        match self {
            Self::Ints(v) => v.first().copied(),
            Self::Floats(v) => v.first().map(|f| *f as i64),
            Self::Bytes(b) => b.first().map(|&x| x as i64),
            Self::Text(_) => None,
        }
    }

    pub fn ints(&self) -> Vec<i64> {
        match self {
            Self::Ints(v) => v.clone(),
            Self::Floats(v) => v.iter().map(|f| *f as i64).collect(),
            Self::Bytes(b) => b.iter().map(|&x| x as i64).collect(),
            Self::Text(_) => Vec::new(),
        }
    }

    pub fn floats(&self) -> Vec<f64> {
        match self {
            Self::Floats(v) => v.clone(),
            Self::Ints(v) => v.iter().map(|&i| i as f64).collect(),
            _ => Vec::new(),
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    }
}

/// One IFD entry: where its value lives, not the value itself.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    /// Absolute file offset of the value.
    pub offset: u64,
}

impl Entry {
    fn size(&self) -> Option<usize> {
        let unit = match self.kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 | 16 | 17 | 18 => 8,
            _ => return None,
        };
        (self.count as usize).checked_mul(unit)
    }
}

pub struct Tiff {
    file: File,
    pub little_endian: bool,
    pub ifd0: u64,
    len: u64,
}

impl Tiff {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("Cannot open {}: {}", path.display(), e))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut head = [0u8; 8];
        file.read_exact(&mut head).map_err(|e| format!("File too short: {}", e))?;
        let little_endian = match &head[..2] {
            b"II" => true,
            b"MM" => false,
            _ => return Err("Not a TIFF-based file".to_string()),
        };
        let mut t = Self { file, little_endian, ifd0: 0, len };
        t.ifd0 = t.u32_at(&head[4..8]) as u64;
        Ok(t)
    }

    fn u16_at(&self, b: &[u8]) -> u16 {
        if self.little_endian { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) }
    }

    fn u32_at(&self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        if self.little_endian { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) }
    }

    pub fn read_at(&mut self, offset: u64, len: usize) -> Option<Vec<u8>> {
        if len > MAX_VALUE || offset.checked_add(len as u64)? > self.len {
            return None;
        }
        let mut buf = vec![0u8; len];
        self.file.seek(SeekFrom::Start(offset)).ok()?;
        self.file.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    pub fn u32(&mut self, offset: u64) -> Option<u32> {
        let b = self.read_at(offset, 4)?;
        Some(self.u32_at(&b))
    }

    /// Entries of the IFD at `offset` and the offset of the next IFD (0 at the end).
    /// `base` is added to value offsets (MakerNotes that count from their own start).
    pub fn ifd(&mut self, offset: u64, base: u64) -> (Vec<Entry>, u64) {
        let Some(n) = self.read_at(offset, 2).map(|b| self.u16_at(&b) as usize) else {
            return (Vec::new(), 0);
        };
        let n = n.min(MAX_ENTRIES);
        let Some(raw) = self.read_at(offset + 2, n * 12 + 4) else {
            return (Vec::new(), 0);
        };
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let e = &raw[i * 12..i * 12 + 12];
            let mut entry = Entry {
                tag: self.u16_at(&e[0..2]),
                kind: self.u16_at(&e[2..4]),
                count: self.u32_at(&e[4..8]),
                offset: offset + 2 + i as u64 * 12 + 8,
            };
            match entry.size() {
                Some(s) if s > 4 => entry.offset = self.u32_at(&e[8..12]) as u64 + base,
                Some(_) => {}
                None => continue,
            }
            entries.push(entry);
        }
        let next = self.u32_at(&raw[n * 12..n * 12 + 4]) as u64;
        (entries, next)
    }

    pub fn value(&mut self, e: &Entry) -> Option<Value> {
        let size = e.size()?;
        let raw = self.read_at(e.offset, size)?;
        let le = self.little_endian;
        let n = e.count as usize;
        let u16s = |b: &[u8]| -> Vec<u16> {
            b.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) }).collect()
        };
        let u32s = |b: &[u8]| -> Vec<u32> {
            b.as_chunks::<4>().0.iter().map(|c| if le { u32::from_le_bytes(*c) } else { u32::from_be_bytes(*c) }).collect()
        };
        Some(match e.kind {
            2 => Value::Text(String::from_utf8_lossy(&raw).trim_end_matches('\0').trim().to_string()),
            1 | 7 => Value::Bytes(raw),
            6 => Value::Ints(raw.iter().map(|&b| b as i8 as i64).collect()),
            3 => Value::Ints(u16s(&raw).into_iter().map(i64::from).collect()),
            8 => Value::Ints(u16s(&raw).into_iter().map(|v| v as i16 as i64).collect()),
            4 | 13 => Value::Ints(u32s(&raw).into_iter().map(i64::from).collect()),
            9 => Value::Ints(u32s(&raw).into_iter().map(|v| v as i32 as i64).collect()),
            11 => Value::Floats(u32s(&raw).into_iter().map(|v| f32::from_bits(v) as f64).collect()),
            12 => Value::Floats(
                raw.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| if le { f64::from_le_bytes(*c) } else { f64::from_be_bytes(*c) })
                    .collect(),
            ),
            5 | 10 => {
                let w = u32s(&raw);
                Value::Floats(
                    (0..n)
                        .map(|i| {
                            let (num, den) = if e.kind == 5 {
                                (w[2 * i] as f64, w[2 * i + 1] as f64)
                            } else {
                                (w[2 * i] as i32 as f64, w[2 * i + 1] as i32 as f64)
                            };
                            if den != 0.0 { num / den } else { 0.0 }
                        })
                        .collect(),
                )
            }
            _ => return None,
        })
    }

    /// All values of one IFD by tag, its entries and the offset of the next IFD.
    pub fn values(&mut self, offset: u64, base: u64) -> (HashMap<u16, Value>, Vec<Entry>, u64) {
        let (entries, next) = self.ifd(offset, base);
        let mut map = HashMap::new();
        for e in &entries {
            if let Some(v) = self.value(e) {
                map.insert(e.tag, v);
            }
        }
        (map, entries, next)
    }
}

/// The IFDs medium format raws keep their metadata in.
#[derive(Debug, Default)]
pub struct Directories {
    /// IFD0 and the IFDs chained after it.
    pub main: Vec<HashMap<u16, Value>>,
    /// SubIFDs (0x014a), where 3FR/FFF keep the raw image and its DNG-style tags.
    pub sub: Vec<HashMap<u16, Value>>,
    pub exif: HashMap<u16, Value>,
    /// Offset of the MakerNote value, when there is one.
    pub makernote: Option<(u64, u32)>,
}

impl Directories {
    pub fn read(t: &mut Tiff) -> Self {
        let mut d = Self::default();
        let mut next = t.ifd0;
        let mut seen = 0;
        while next != 0 && seen < 8 {
            seen += 1;
            let (values, _, n) = t.values(next, 0);
            if let Some(subs) = values.get(&0x014a) {
                for o in subs.ints().into_iter().take(8) {
                    d.sub.push(t.values(o as u64, 0).0);
                }
            }
            if let Some(exif) = values.get(&0x8769).and_then(Value::int) {
                let (ev, entries, _) = t.values(exif as u64, 0);
                d.makernote = entries.iter().find(|e| e.tag == 0x927c).map(|e| (e.offset, e.count));
                if d.exif.is_empty() {
                    d.exif = ev;
                }
            }
            d.main.push(values);
            next = n;
        }
        d
    }

    /// First value of `tag` in IFD0, the SubIFDs or EXIF.
    pub fn find(&self, tag: u16) -> Option<&Value> {
        self.main
            .iter()
            .chain(self.sub.iter())
            .chain(std::iter::once(&self.exif))
            .find_map(|m| m.get(&tag))
    }

    /// The SubIFD/IFD that holds the CFA raw (PhotometricInterpretation 32803).
    pub fn raw_ifd(&self) -> Option<&HashMap<u16, Value>> {
        self.sub
            .iter()
            .chain(self.main.iter())
            .find(|m| m.get(&0x0106).and_then(Value::int) == Some(32803))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A little-endian TIFF with IFD0 holding `entries` (tag, type, values as u32).
    pub(crate) fn tiff(entries: &[(u16, u16, Vec<u32>)]) -> Vec<u8> {
        let mut out = b"II*\0".to_vec();
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let data_start = 8 + 2 + entries.len() * 12 + 4;
        let mut data = Vec::new();
        for (tag, kind, vals) in entries {
            let mut bytes = Vec::new();
            for v in vals {
                match kind {
                    3 => bytes.extend_from_slice(&(*v as u16).to_le_bytes()),
                    2 | 1 | 7 => bytes.push(*v as u8),
                    _ => bytes.extend_from_slice(&v.to_le_bytes()),
                }
            }
            let count = if *kind == 5 { vals.len() / 2 } else { vals.len() };
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&(count as u32).to_le_bytes());
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
    fn reads_inline_offset_and_rational_values() {
        let bytes = tiff(&[
            (0x0100, 3, vec![11904]),
            (0x0110, 2, b"X2D 100C\0".iter().map(|&b| b as u32).collect()),
            (0xc61f, 3, vec![128, 96]),
            (0xc628, 5, vec![38, 100, 1, 1, 63, 100]),
        ]);
        let dir = std::env::temp_dir().join(format!("omastudio-tiff-{}", std::process::id()));
        std::fs::write(&dir, &bytes).unwrap();
        let mut t = Tiff::open(&dir).unwrap();
        let (v, _, _) = t.values(t.ifd0, 0);
        std::fs::remove_file(&dir).ok();
        assert_eq!(v[&0x0100].int(), Some(11904));
        assert_eq!(v[&0x0110].text(), Some("X2D 100C"));
        assert_eq!(v[&0xc61f].ints(), vec![128, 96]);
        assert_eq!(v[&0xc628].floats(), vec![0.38, 1.0, 0.63]);
    }

    #[test]
    fn survives_truncated_and_garbage_ifds() {
        let dir = std::env::temp_dir().join(format!("omastudio-tiff-bad-{}", std::process::id()));
        let mut bytes = tiff(&[(0x0100, 3, vec![1])]);
        bytes[8] = 0xff; // absurd entry count
        bytes[9] = 0xff;
        std::fs::write(&dir, &bytes).unwrap();
        let mut t = Tiff::open(&dir).unwrap();
        let _ = Directories::read(&mut t);
        std::fs::write(&dir, b"II*\0\xff\xff\xff\x7f").unwrap();
        let mut t = Tiff::open(&dir).unwrap();
        assert!(Directories::read(&mut t).main.iter().all(|m| m.is_empty()));
        std::fs::remove_file(&dir).ok();
        assert!(Tiff::open(Path::new("/nonexistent/x.3fr")).is_err());
    }
}
