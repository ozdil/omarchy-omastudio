//! Camera metadata written into exports by the engine itself.
//!
//! exiftool copied the camera's EXIF into every export by reading the RAW and then
//! rewriting the whole output file, about half a second for a medium format TIFF.
//! For Fujifilm RAF files the camera's complete EXIF (IFD0, Exif IFD, GPS IFD and the
//! Fujifilm MakerNote) sits in the embedded JPEG; it is read here, adjusted for the
//! export and written by the TIFF and JPEG writers as they write the file. Other
//! sources and formats still go through exiftool.

use std::path::Path;

/// One TIFF directory entry with its value in little-endian byte order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub tag: u16,
    pub ty: u16,
    pub count: u32,
    pub data: Vec<u8>,
}

impl Entry {
    pub fn short(tag: u16, v: u16) -> Self {
        Self { tag, ty: 3, count: 1, data: v.to_le_bytes().to_vec() }
    }

    pub fn long(tag: u16, v: u32) -> Self {
        Self { tag, ty: 4, count: 1, data: v.to_le_bytes().to_vec() }
    }

    pub fn ascii(tag: u16, s: &str) -> Self {
        let mut data: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
        data.push(0);
        Self { tag, ty: 2, count: data.len() as u32, data }
    }
}

/// Camera metadata for an export: descriptive IFD0 tags, the Exif IFD and the GPS IFD.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exif {
    pub ifd0: Vec<Entry>,
    pub exif: Vec<Entry>,
    pub gps: Vec<Entry>,
}

/// What the export asks for, as in [`super::ExportOptions`].
pub struct Adjust<'a> {
    pub strip_gps: bool,
    /// EXIF ColorSpace: true for sRGB (1), false for "uncalibrated" (65535).
    pub srgb: bool,
    pub artist: Option<&'a str>,
    pub width: u32,
    pub height: u32,
}

/// IFD0 tags that describe the photo rather than the camera's image data.
const IFD0_KEEP: [u16; 7] = [270, 271, 272, 305, 306, 315, 33432];
const EXIF_IFD: u16 = 0x8769;
const GPS_IFD: u16 = 0x8825;
const INTEROP_IFD: u16 = 0xA005;
const MAKER_NOTE: u16 = 0x927C;
const MAX_ENTRIES: usize = 1000;
const MAX_VALUE: usize = 1 << 20;

/// Reads the camera's EXIF from a Fujifilm RAF, or None for other files or when the
/// embedded JPEG carries none.
pub fn from_raf(path: &Path) -> Option<Exif> {
    let jpeg = crate::fuji::makernote::embedded_jpeg(path).ok()?;
    parse(app1_tiff(&jpeg)?)
}

/// The TIFF structure inside a JPEG's EXIF APP1 segment.
fn app1_tiff(jpeg: &[u8]) -> Option<&[u8]> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut pos = 2;
    while pos + 4 <= jpeg.len() && jpeg[pos] == 0xFF {
        let marker = jpeg[pos + 1];
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if marker == 0xDA || len < 2 {
            return None;
        }
        let seg = jpeg.get(pos + 4..pos + 2 + len)?;
        if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
            return Some(&seg[6..]);
        }
        pos += 2 + len;
    }
    None
}

struct Reader<'a> {
    data: &'a [u8],
    le: bool,
}

impl Reader<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(at..at + 2)?.try_into().ok()?;
        Some(if self.le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(at..at + 4)?.try_into().ok()?;
        Some(if self.le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }

    /// The entries of the IFD at `at`, values converted to little-endian. A MakerNote
    /// is kept only when it is self-contained (Fujifilm's, offsets from its own start).
    fn ifd(&self, at: usize) -> Vec<Entry> {
        let Some(n) = self.u16(at) else { return Vec::new() };
        let mut out = Vec::new();
        for i in 0..(n as usize).min(MAX_ENTRIES) {
            let e = at + 2 + i * 12;
            let (Some(tag), Some(ty), Some(count)) = (self.u16(e), self.u16(e + 2), self.u32(e + 4)) else { break };
            let Some(size) = type_size(ty) else { continue };
            let Some(total) = (count as usize).checked_mul(size).filter(|&t| t <= MAX_VALUE) else { continue };
            let start = if total <= 4 {
                e + 8
            } else {
                match self.u32(e + 8) {
                    Some(o) => o as usize,
                    None => continue,
                }
            };
            let Some(raw) = self.data.get(start..start + total) else { continue };
            if tag == MAKER_NOTE && !raw.starts_with(b"FUJIFILM") {
                continue;
            }
            out.push(Entry { tag, ty, count, data: to_le(raw, ty, self.le) });
        }
        out
    }

    fn pointer(&self, entries: &[Entry], tag: u16) -> Option<usize> {
        let e = entries.iter().find(|e| e.tag == tag && e.count == 1 && matches!(e.ty, 4 | 13))?;
        Some(u32::from_le_bytes(e.data[..4].try_into().ok()?) as usize)
    }
}

fn type_size(ty: u16) -> Option<usize> {
    Some(match ty {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// Converts a value from the source byte order to little-endian, element by element.
fn to_le(raw: &[u8], ty: u16, le: bool) -> Vec<u8> {
    let unit = match ty {
        3 | 8 => 2,
        4 | 9 | 11 | 13 | 5 | 10 => 4,
        12 => 8,
        _ => 1,
    };
    if le || unit == 1 {
        return raw.to_vec();
    }
    raw.chunks(unit).flat_map(|c| c.iter().rev().copied()).collect()
}

/// Parses a TIFF structure (as in an EXIF APP1 segment).
pub fn parse(tiff: &[u8]) -> Option<Exif> {
    let le = match tiff.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let r = Reader { data: tiff, le };
    let ifd0 = r.ifd(r.u32(4)? as usize);
    let exif = r.pointer(&ifd0, EXIF_IFD).map(|at| r.ifd(at)).unwrap_or_default();
    let gps = r.pointer(&ifd0, GPS_IFD).map(|at| r.ifd(at)).unwrap_or_default();
    if exif.is_empty() && gps.is_empty() && ifd0.is_empty() {
        return None;
    }
    Some(Exif {
        ifd0: ifd0.into_iter().filter(|e| IFD0_KEEP.contains(&e.tag)).collect(),
        exif: exif.into_iter().filter(|e| e.tag != INTEROP_IFD && e.tag != EXIF_IFD && e.tag != GPS_IFD).collect(),
        gps,
    })
}

impl Exif {
    /// Applies the export's choices: no GPS when asked, the colour space, the export's
    /// pixel size, upright orientation (the pixels are already rotated) and the artist.
    pub fn adjusted(mut self, a: &Adjust) -> Self {
        if a.strip_gps {
            self.gps.clear();
        }
        let set = |list: &mut Vec<Entry>, e: Entry| match list.iter_mut().find(|x| x.tag == e.tag) {
            Some(x) => *x = e,
            None => list.push(e),
        };
        set(&mut self.ifd0, Entry::short(274, 1));
        if let Some(artist) = a.artist.map(str::trim).filter(|s| !s.is_empty()) {
            set(&mut self.ifd0, Entry::ascii(315, artist));
            set(&mut self.ifd0, Entry::ascii(33432, artist));
        }
        if !self.exif.is_empty() {
            set(&mut self.exif, Entry::short(0xA001, if a.srgb { 1 } else { 65535 }));
            set(&mut self.exif, Entry::long(0xA002, a.width));
            set(&mut self.exif, Entry::long(0xA003, a.height));
        }
        self
    }

    /// Lays out `ifd0` (the caller's entries plus these tags and pointers to the Exif
    /// and GPS IFDs) starting at absolute file offset `start`, which must be even.
    /// Returns the bytes to write at `start`.
    pub fn layout(&self, mut ifd0: Vec<Entry>, start: u32) -> Result<Vec<u8>, String> {
        for e in &self.ifd0 {
            if !ifd0.iter().any(|x| x.tag == e.tag) {
                ifd0.push(e.clone());
            }
        }
        // Pointer values are patched once the sizes are known
        if !self.exif.is_empty() {
            ifd0.push(Entry::long(EXIF_IFD, 0));
        }
        if !self.gps.is_empty() {
            ifd0.push(Entry::long(GPS_IFD, 0));
        }
        ifd0.sort_by_key(|e| e.tag);
        let exif_at = start as u64 + block_len(&ifd0) as u64;
        let gps_at = exif_at + if self.exif.is_empty() { 0 } else { block_len(&sorted(&self.exif)) as u64 };
        if gps_at + block_len(&sorted(&self.gps)) as u64 > u32::MAX as u64 {
            return Err("Metadata does not fit a classic TIFF".into());
        }
        for e in ifd0.iter_mut() {
            match e.tag {
                EXIF_IFD => e.data = (exif_at as u32).to_le_bytes().to_vec(),
                GPS_IFD => e.data = (gps_at as u32).to_le_bytes().to_vec(),
                _ => {}
            }
        }
        let mut out = Vec::new();
        write_block(&ifd0, start, &mut out);
        if !self.exif.is_empty() {
            write_block(&sorted(&self.exif), exif_at as u32, &mut out);
        }
        if !self.gps.is_empty() {
            write_block(&sorted(&self.gps), gps_at as u32, &mut out);
        }
        Ok(out)
    }

    /// A JPEG APP1 segment (marker, length, "Exif\0\0", TIFF), or None when it would
    /// exceed the 64 KB a segment can hold.
    pub fn app1(&self) -> Option<Vec<u8>> {
        // EXIF requires these in a JPEG's IFD0; 72 dpi as exiftool's copies had, and
        // centred chroma samples as the encoder writes them
        let ppi = Entry { tag: 282, ty: 5, count: 1, data: [72u32.to_le_bytes(), 1u32.to_le_bytes()].concat() };
        let required = vec![Entry { tag: 283, ..ppi.clone() }, ppi, Entry::short(296, 2), Entry::short(0x0213, 1)];
        let mut tiff = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        tiff.extend(self.layout(required, 8).ok()?);
        let len = 2 + 6 + tiff.len();
        if len > u16::MAX as usize {
            return None;
        }
        let mut seg = vec![0xFF, 0xE1];
        seg.extend_from_slice(&(len as u16).to_be_bytes());
        seg.extend_from_slice(b"Exif\0\0");
        seg.extend(tiff);
        Some(seg)
    }
}

fn sorted(list: &[Entry]) -> Vec<Entry> {
    let mut v = list.to_vec();
    v.sort_by_key(|e| e.tag);
    v
}

fn padded(n: usize) -> usize {
    n + (n & 1)
}

/// Bytes of an IFD with its out-of-line values.
fn block_len(entries: &[Entry]) -> usize {
    2 + entries.len() * 12 + 4 + entries.iter().filter(|e| e.data.len() > 4).map(|e| padded(e.data.len())).sum::<usize>()
}

/// Writes an IFD (no next IFD) located at absolute offset `at`, values after it.
fn write_block(entries: &[Entry], at: u32, out: &mut Vec<u8>) {
    let mut data_at = at as usize + 2 + entries.len() * 12 + 4;
    let mut data = Vec::new();
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.tag.to_le_bytes());
        out.extend_from_slice(&e.ty.to_le_bytes());
        out.extend_from_slice(&e.count.to_le_bytes());
        if e.data.len() <= 4 {
            let mut field = [0u8; 4];
            field[..e.data.len()].copy_from_slice(&e.data);
            out.extend_from_slice(&field);
        } else {
            out.extend_from_slice(&(data_at as u32).to_le_bytes());
            data.extend_from_slice(&e.data);
            if e.data.len() % 2 == 1 {
                data.push(0);
            }
            data_at += padded(e.data.len());
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend(data);
}

/// Inserts an APP1 segment after a JPEG's SOI marker and its JFIF (APP0) segment if
/// it has one, where exiftool puts it.
pub fn insert_app1(jpeg: Vec<u8>, app1: &[u8]) -> Vec<u8> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return jpeg;
    }
    let mut at = 2;
    if jpeg.get(2..4) == Some(&[0xFF, 0xE0]) && jpeg.len() >= 6 {
        at = (4 + u16::from_be_bytes([jpeg[4], jpeg[5]]) as usize).min(jpeg.len());
    }
    let mut out = Vec::with_capacity(jpeg.len() + app1.len());
    out.extend_from_slice(&jpeg[..at]);
    out.extend_from_slice(app1);
    out.extend_from_slice(&jpeg[at..]);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A big-endian EXIF block like a camera's: IFD0 (Make, Orientation, XResolution,
    /// Exif pointer, GPS pointer), Exif IFD (ExposureTime, ISO, MakerNote, Interop
    /// pointer) and a GPS IFD (version).
    pub(crate) fn camera_tiff() -> Vec<u8> {
        let mut t = Vec::new();
        let be16 = |v: u16| v.to_be_bytes();
        let be32 = |v: u32| v.to_be_bytes();
        t.extend_from_slice(b"MM\x00\x2a");
        t.extend_from_slice(&be32(8));
        // IFD0 at 8: 5 entries -> 2 + 60 + 4 = 66 -> data from 74
        let entry = |t: &mut Vec<u8>, tag: u16, ty: u16, count: u32, val: [u8; 4]| {
            t.extend_from_slice(&be16(tag));
            t.extend_from_slice(&be16(ty));
            t.extend_from_slice(&be32(count));
            t.extend_from_slice(&val);
        };
        let make_at = 74u32; // "FUJIFILM\0" (9) -> 84 after pad
        let exif_at = 84u32;
        // Exif IFD: 4 entries -> 2 + 48 + 4 = 54 -> data from 138
        let expo_at = 138u32; // rational 8 bytes -> 146
        let note_at = 146u32; // makernote 16 bytes -> 162
        let gps_at = 162u32;
        t.extend_from_slice(&be16(5));
        entry(&mut t, 271, 2, 9, be32(make_at));
        entry(&mut t, 274, 3, 1, [0, 6, 0, 0]);
        entry(&mut t, 282, 5, 1, be32(0)); // value outside: skipped (offset 0 points at header)
        entry(&mut t, EXIF_IFD, 4, 1, be32(exif_at));
        entry(&mut t, GPS_IFD, 4, 1, be32(gps_at));
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(b"FUJIFILM\0\0");
        assert_eq!(t.len(), 84);
        t.extend_from_slice(&be16(4));
        entry(&mut t, 0x829A, 5, 1, be32(expo_at));
        entry(&mut t, 0x8827, 3, 1, [0x01, 0x90, 0, 0]); // ISO 400
        entry(&mut t, MAKER_NOTE, 7, 16, be32(note_at));
        entry(&mut t, INTEROP_IFD, 4, 1, be32(0));
        t.extend_from_slice(&be32(0));
        assert_eq!(t.len(), 138);
        t.extend_from_slice(&be32(1));
        t.extend_from_slice(&be32(250));
        t.extend_from_slice(b"FUJIFILM\x0c\x00\x00\x00\x00\x00\x00\x00");
        assert_eq!(t.len(), 162);
        t.extend_from_slice(&be16(1));
        entry(&mut t, 0, 1, 4, [2, 3, 0, 0]);
        t.extend_from_slice(&be32(0));
        t
    }

    #[test]
    fn parses_big_endian_camera_exif_and_keeps_what_matters() {
        let exif = parse(&camera_tiff()).unwrap();
        // Make kept; Orientation and XResolution are the camera's image layout, dropped
        assert_eq!(exif.ifd0.iter().map(|e| e.tag).collect::<Vec<_>>(), vec![271]);
        assert_eq!(exif.ifd0[0].data, b"FUJIFILM\0");
        let expo = exif.exif.iter().find(|e| e.tag == 0x829A).unwrap();
        assert_eq!(expo.data, [1u32.to_le_bytes(), 250u32.to_le_bytes()].concat());
        let iso = exif.exif.iter().find(|e| e.tag == 0x8827).unwrap();
        assert_eq!(iso.data, 400u16.to_le_bytes());
        assert!(exif.exif.iter().any(|e| e.tag == MAKER_NOTE));
        assert!(!exif.exif.iter().any(|e| e.tag == INTEROP_IFD));
        assert_eq!(exif.gps.len(), 1);
    }

    #[test]
    fn adjusts_for_the_export() {
        let a = Adjust { strip_gps: true, srgb: false, artist: Some("Ozan"), width: 100, height: 50 };
        let exif = parse(&camera_tiff()).unwrap().adjusted(&a);
        assert!(exif.gps.is_empty());
        assert!(exif.ifd0.contains(&Entry::short(274, 1)));
        assert!(exif.ifd0.contains(&Entry::ascii(315, "Ozan")));
        assert!(exif.ifd0.contains(&Entry::ascii(33432, "Ozan")));
        assert!(exif.exif.contains(&Entry::short(0xA001, 65535)));
        assert!(exif.exif.contains(&Entry::long(0xA002, 100)));
    }

    #[test]
    fn app1_round_trips_through_the_parser() {
        let a = Adjust { strip_gps: false, srgb: true, artist: None, width: 6, height: 4 };
        let exif = parse(&camera_tiff()).unwrap().adjusted(&a);
        let seg = exif.app1().unwrap();
        assert_eq!(&seg[..2], &[0xFF, 0xE1]);
        assert_eq!(u16::from_be_bytes([seg[2], seg[3]]) as usize, seg.len() - 2);
        let back = parse(&seg[10..]).unwrap();
        let mut want = exif.clone();
        // The parser keeps only descriptive IFD0 tags; Orientation is written but not read back
        want.ifd0.retain(|e| e.tag != 274);
        want.ifd0.sort_by_key(|e| e.tag);
        want.exif.sort_by_key(|e| e.tag);
        assert_eq!(back, want);
    }

    #[test]
    fn jpeg_with_inserted_exif_decodes_and_parses() {
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([200, 100, 50]));
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
        let a = Adjust { strip_gps: false, srgb: true, artist: None, width: 8, height: 8 };
        let exif = parse(&camera_tiff()).unwrap().adjusted(&a);
        let out = insert_app1(jpeg.into_inner(), &exif.app1().unwrap());
        assert!(image::load_from_memory(&out).is_ok());
        let seg = exif.app1().unwrap();
        let r = Reader { data: &seg[10..], le: true };
        let tags: Vec<u16> = r.ifd(8).iter().map(|e| e.tag).collect();
        for t in [271, 274, 282, 283, 296, 0x0213, EXIF_IFD] {
            assert!(tags.contains(&t), "IFD0 lacks {t:#x}: {tags:?}");
        }
        let back = parse(app1_tiff(&out).unwrap()).unwrap();
        assert!(back.exif.iter().any(|e| e.tag == 0x8827));
    }
}
