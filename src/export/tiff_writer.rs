//! Baseline RGB TIFF writer that compresses its strips in parallel.
//!
//! The `tiff` crate compresses one strip after another on a single core, which made a
//! deflate TIFF of a 100 MP frame the slowest part of an export. Strips are
//! independent, so each one is predicted and compressed on its own thread here and
//! the file is assembled in order afterwards. LZW still goes through the `tiff` crate.

use rayon::prelude::*;
use std::io::Write;

/// Pixel data of one interleaved RGB image, 8 or 16 bits per sample.
pub enum Samples<'a> {
    U8(&'a [u8]),
    U16(&'a [u16]),
}

impl Samples<'_> {
    fn bits(&self) -> u16 {
        match self {
            Samples::U8(_) => 8,
            Samples::U16(_) => 16,
        }
    }
}

/// How the strips are stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    None,
    /// zlib (TIFF compression 8, "Adobe deflate") with the horizontal predictor.
    Deflate,
}

/// A TIFF file as the parts to write one after another: the header, the strips and
/// the directory. Uncompressed strips borrow the pixels instead of copying them.
pub struct TiffParts<'a> {
    head: Vec<u8>,
    strips: Vec<std::borrow::Cow<'a, [u8]>>,
    tail: Vec<u8>,
}

impl TiffParts<'_> {
    /// Total file size in bytes.
    pub fn len(&self) -> usize {
        self.head.len() + self.strips.iter().map(|s| s.len()).sum::<usize>() + self.tail.len()
    }

    /// Whether the file has no bytes (never true for a built file).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The parts in file order.
    pub fn slices(&self) -> Vec<&[u8]> {
        let mut out = Vec::with_capacity(self.strips.len() + 2);
        out.push(self.head.as_slice());
        out.extend(self.strips.iter().map(|s| s.as_ref()));
        out.push(self.tail.as_slice());
        out
    }

    /// The whole file in one buffer.
    pub fn concat(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len());
        for s in self.slices() {
            out.extend_from_slice(s);
        }
        out
    }
}

/// Rows per strip: about 1 MiB of samples, so a large frame splits into hundreds of
/// strips for the thread pool while each still compresses well.
fn rows_per_strip(row_bytes: usize, height: u32) -> u32 {
    ((1usize << 20) / row_bytes.max(1)).clamp(1, height.max(1) as usize) as u32
}

/// Builds a little-endian baseline TIFF of `width` x `height` RGB pixels.
pub fn build<'a>(
    width: u32,
    height: u32,
    samples: Samples<'a>,
    codec: Codec,
    icc: Option<&[u8]>,
) -> Result<TiffParts<'a>, String> {
    let bits = samples.bits();
    let bytes_per_sample = (bits / 8) as usize;
    let row_samples = width as usize * 3;
    let row_bytes = row_samples * bytes_per_sample;
    let expected = row_samples * height as usize;
    let len = match &samples {
        Samples::U8(s) => s.len(),
        Samples::U16(s) => s.len(),
    };
    if width == 0 || height == 0 || len != expected {
        return Err(format!("TIFF: {len} samples for a {width}x{height} RGB image"));
    }
    let rps = rows_per_strip(row_bytes, height);
    let strip_count = height.div_ceil(rps) as usize;
    let strip_samples = row_samples * rps as usize;

    let strips: Vec<std::borrow::Cow<'a, [u8]>> = match (codec, samples) {
        (Codec::None, Samples::U8(s)) => s.chunks(strip_samples).map(std::borrow::Cow::Borrowed).collect(),
        (Codec::None, Samples::U16(s)) => {
            if cfg!(target_endian = "little") {
                // SAFETY: u16 has no padding and any byte pattern is a valid u8; the
                // slice covers exactly the same memory
                let bytes = unsafe { std::slice::from_raw_parts(s.as_ptr().cast::<u8>(), s.len() * 2) };
                bytes.chunks(strip_samples * 2).map(std::borrow::Cow::Borrowed).collect()
            } else {
                s.par_chunks(strip_samples)
                    .map(|c| std::borrow::Cow::Owned(c.iter().flat_map(|v| v.to_le_bytes()).collect()))
                    .collect()
            }
        }
        (Codec::Deflate, Samples::U8(s)) => s
            .par_chunks(strip_samples)
            .map(|c| {
                let mut buf = c.to_vec();
                for row in buf.chunks_mut(row_samples) {
                    for i in (3..row.len()).rev() {
                        row[i] = row[i].wrapping_sub(row[i - 3]);
                    }
                }
                zlib(&buf).map(std::borrow::Cow::Owned)
            })
            .collect::<Result<_, _>>()?,
        (Codec::Deflate, Samples::U16(s)) => s
            .par_chunks(strip_samples)
            .map(|c| {
                let mut buf = Vec::with_capacity(c.len() * 2);
                for row in c.chunks(row_samples) {
                    let mut prev = [0u16; 3];
                    for px in row.as_chunks::<3>().0 {
                        for k in 0..3 {
                            buf.extend_from_slice(&px[k].wrapping_sub(prev[k]).to_le_bytes());
                            prev[k] = px[k];
                        }
                    }
                }
                zlib(&buf).map(std::borrow::Cow::Owned)
            })
            .collect::<Result<_, _>>()?,
    };
    debug_assert_eq!(strips.len(), strip_count);

    // Layout: header (8) | bits per sample (6) | resolution (16) | ICC | strips | IFD | arrays
    let mut head = Vec::with_capacity(64 + icc.map_or(0, |p| p.len()));
    head.extend_from_slice(b"II");
    head.extend_from_slice(&42u16.to_le_bytes());
    head.extend_from_slice(&0u32.to_le_bytes()); // IFD offset, patched below
    let bps_off = head.len() as u32;
    for _ in 0..3 {
        head.extend_from_slice(&bits.to_le_bytes());
    }
    let xres_off = head.len() as u32;
    // 300 / 1 pixels per inch, as the `tiff` crate wrote
    for _ in 0..2 {
        head.extend_from_slice(&300u32.to_le_bytes());
        head.extend_from_slice(&1u32.to_le_bytes());
    }
    let yres_off = xres_off + 8;
    let icc_off = head.len() as u32;
    if let Some(p) = icc {
        head.extend_from_slice(p);
    }
    if head.len() % 2 == 1 {
        head.push(0);
    }

    let mut offsets = Vec::with_capacity(strip_count);
    let mut pos = head.len() as u64;
    for s in &strips {
        offsets.push(pos);
        pos += s.len() as u64;
    }
    if pos % 2 == 1 {
        // Word alignment for the directory: one pad byte after the last strip
        pos += 1;
    }
    let ifd_off = pos;
    let compression: u16 = if codec == Codec::Deflate { 8 } else { 1 };
    let predictor: u16 = if codec == Codec::Deflate { 2 } else { 1 };

    // Entries: (tag, type, count, value-or-offset); types 3 SHORT, 4 LONG, 5 RATIONAL, 7 UNDEFINED
    let mut entries: Vec<(u16, u16, u32, u32)> = vec![
        (256, 4, 1, width),
        (257, 4, 1, height),
        (258, 3, 3, bps_off),
        (259, 3, 1, compression as u32),
        (262, 3, 1, 2),
        (273, 4, strip_count as u32, 0), // patched: strip offsets
        (277, 3, 1, 3),
        (278, 4, 1, rps),
        (279, 4, strip_count as u32, 0), // patched: strip byte counts
        (282, 5, 1, xres_off),
        (283, 5, 1, yres_off),
        (284, 3, 1, 1),
        (296, 3, 1, 2),
        (317, 3, 1, predictor as u32),
    ];
    if let Some(p) = icc {
        entries.push((34675, 7, p.len() as u32, icc_off));
    }
    let ifd_len = 2 + entries.len() as u64 * 12 + 4;
    let arrays_off = ifd_off + ifd_len;
    let counts_off = arrays_off + strip_count as u64 * 4;
    let end = counts_off + strip_count as u64 * 4;
    if end > u32::MAX as u64 {
        return Err("TIFF: the image is larger than 4 GB; choose deflate compression or a smaller size".into());
    }
    let single = strip_count == 1;
    for e in entries.iter_mut() {
        match e.0 {
            273 => e.3 = if single { offsets[0] as u32 } else { arrays_off as u32 },
            279 => e.3 = if single { strips[0].len() as u32 } else { counts_off as u32 },
            _ => {}
        }
    }
    head[4..8].copy_from_slice(&(ifd_off as u32).to_le_bytes());

    let mut tail = Vec::with_capacity((end - ifd_off + 1) as usize);
    if (head.len() as u64 + strips.iter().map(|s| s.len() as u64).sum::<u64>()) % 2 == 1 {
        tail.push(0);
    }
    tail.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, value) in &entries {
        tail.extend_from_slice(&tag.to_le_bytes());
        tail.extend_from_slice(&ty.to_le_bytes());
        tail.extend_from_slice(&count.to_le_bytes());
        // A SHORT value sits in the first two bytes of the field
        if *ty == 3 && *count == 1 {
            tail.extend_from_slice(&(*value as u16).to_le_bytes());
            tail.extend_from_slice(&0u16.to_le_bytes());
        } else {
            tail.extend_from_slice(&value.to_le_bytes());
        }
    }
    tail.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    if !single {
        for o in &offsets {
            tail.extend_from_slice(&(*o as u32).to_le_bytes());
        }
        for s in &strips {
            tail.extend_from_slice(&(s.len() as u32).to_le_bytes());
        }
    }
    Ok(TiffParts { head, strips, tail })
}

fn zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    // Level 6 matches the `tiff` crate's "balanced" deflate
    let mut enc = flate2::write::ZlibEncoder::new(Vec::with_capacity(data.len() / 2), flate2::Compression::new(6));
    enc.write_all(data).and_then(|_| enc.finish()).map_err(|e| format!("Failed to compress TIFF strip: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Tiff).expect("valid TIFF")
    }

    fn pattern16(w: u32, h: u32) -> Vec<u16> {
        (0..w * h * 3).map(|i| (i.wrapping_mul(2654435761) >> 7) as u16).collect()
    }

    #[test]
    fn roundtrip_16_bit_both_codecs() {
        let (w, h) = (301, 97);
        let px = pattern16(w, h);
        for codec in [Codec::None, Codec::Deflate] {
            let file = build(w, h, Samples::U16(&px), codec, None).unwrap().concat();
            let img = decode(&file).to_rgb16();
            assert_eq!((img.width(), img.height()), (w, h));
            assert_eq!(img.as_raw(), &px, "{codec:?}");
        }
    }

    #[test]
    fn roundtrip_8_bit_with_icc_and_many_strips() {
        // 2000 px wide rows of 6000 bytes: 174 rows per strip, so 1 MiB-ish strips
        let (w, h) = (2000, 523);
        let px: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
        let icc = vec![7u8; 301];
        for codec in [Codec::None, Codec::Deflate] {
            let file = build(w, h, Samples::U8(&px), codec, Some(&icc)).unwrap().concat();
            let mut dec = tiff::decoder::Decoder::new(std::io::Cursor::new(&file)).unwrap();
            assert_eq!(dec.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap(), icc);
            assert!(dec.strip_count().unwrap() > 1);
            let img = decode(&file).to_rgb8();
            assert_eq!(img.as_raw(), &px, "{codec:?}");
        }
    }

    #[test]
    fn single_strip_and_odd_sizes() {
        let (w, h) = (3, 1);
        let px = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9];
        let parts = build(w, h, Samples::U8(&px), Codec::None, Some(&[1, 2, 3])).unwrap();
        assert_eq!(parts.len(), parts.concat().len());
        assert_eq!(decode(&parts.concat()).to_rgb8().as_raw(), &px);
    }

    #[test]
    fn rejects_wrong_sample_count() {
        assert!(build(4, 4, Samples::U8(&[0; 10]), Codec::None, None).is_err());
    }
}
