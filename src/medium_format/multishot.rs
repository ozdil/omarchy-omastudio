//! Multi-shot merging for pixel-shift backs (Hasselblad H-MS, Phase One and Sinar
//! style 4-shot).
//!
//! A 4-shot sequence moves the sensor by one photosite between frames so that each
//! scene point is seen through a red, a blue and two green filters. Merging takes
//! the CFA planes as they came off the sensor (black subtracted, same size) and
//! returns full RGB without demosaicing. The 6- and 16-shot modes add half-pixel
//! steps on top; they are merged as 4-shot groups and then interleaved.

/// Sensor offsets (row, column) of the frames of a 4-shot sequence, in capture order.
pub const FOUR_SHOT: [(usize, usize); 4] = [(0, 0), (0, 1), (1, 1), (1, 0)];

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    FrameCount(usize),
    SizeMismatch,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FrameCount(n) => write!(f, "a 4-shot merge needs 4 frames, got {}", n),
            Self::SizeMismatch => write!(f, "the frames differ in size"),
        }
    }
}

impl std::error::Error for Error {}

/// One raw frame: a CFA plane and the colour (0 R, 1 G, 2 B) of each photosite.
pub struct Frame<'a> {
    pub data: &'a [f32],
    pub width: usize,
    pub height: usize,
    /// Colour of photosite (row, col) of this frame's own plane.
    pub color: &'a dyn Fn(usize, usize) -> usize,
}

/// Merges a 4-shot sequence into interleaved RGB (3 values per pixel). Frame `i` was
/// taken with the sensor shifted by `offsets[i]`, so scene point (y, x) fell on its
/// photosite (y + dy, x + dx). The one-pixel border the shift loses is filled from
/// the nearest full pixel.
pub fn merge_four(frames: &[Frame], offsets: &[(usize, usize); 4]) -> Result<(Vec<f32>, usize, usize), Error> {
    if frames.len() != 4 {
        return Err(Error::FrameCount(frames.len()));
    }
    let (w, h) = (frames[0].width, frames[0].height);
    if frames.iter().any(|f| f.width != w || f.height != h || f.data.len() < w * h) || w < 2 || h < 2 {
        return Err(Error::SizeMismatch);
    }
    let (ow, oh) = (w - 1, h - 1);
    let mut out = vec![0f32; w * h * 3];
    for y in 0..oh {
        for x in 0..ow {
            let mut sum = [0f32; 3];
            let mut n = [0u32; 3];
            for (f, &(dy, dx)) in frames.iter().zip(offsets) {
                let (sy, sx) = (y + dy, x + dx);
                let c = (f.color)(sy, sx).min(2);
                sum[c] += f.data[sy * w + sx];
                n[c] += 1;
            }
            let o = (y * w + x) * 3;
            for c in 0..3 {
                out[o + c] = if n[c] > 0 { sum[c] / n[c] as f32 } else { 0.0 };
            }
        }
    }
    // Last column and row: copy their neighbours
    for y in 0..oh {
        let o = (y * w + ow) * 3;
        out.copy_within(o - 3..o, o);
    }
    let last = oh * w * 3;
    out.copy_within(last - w * 3..last, last);
    Ok((out, w, h))
}

/// Merges four developed-ready frames of one 4-shot sequence into 16-bit linear RGB
/// in `space`, rotated like the camera recorded it.
pub fn merge_raw_frames(frames: &[crate::raw::develop::CfaFrame], space: crate::raw::OutputColorSpace) -> Result<(Vec<u16>, usize, usize), Error> {
    let colors: Vec<_> = frames.iter().map(|f| move |r: usize, c: usize| f.pattern.color(r, c)).collect();
    let views: Vec<Frame> = frames
        .iter()
        .zip(&colors)
        .map(|(f, color)| Frame { data: &f.data, width: f.width, height: f.height, color })
        .collect();
    let (rgb, w, h) = merge_four(&views, &FOUR_SHOT)?;
    let m = frames[0].camera_to(space);
    let to_u16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let out: Vec<u16> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| std::array::from_fn::<u16, 3, _>(|r| to_u16(m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2])))
        .collect();
    Ok(crate::raw::develop::rotate_rgb(out, w, h, frames[0].flip))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat scene of colour `rgb` photographed through an RGGB mosaic.
    fn shoot(rgb: [f32; 3], w: usize, h: usize) -> Vec<f32> {
        (0..h).flat_map(|y| (0..w).map(move |x| rgb[rggb(y, x)])).collect()
    }

    fn rggb(y: usize, x: usize) -> usize {
        match (y & 1, x & 1) {
            (0, 0) => 0,
            (1, 1) => 2,
            _ => 1,
        }
    }

    #[test]
    fn flat_scene_comes_back_in_full_colour() {
        let (w, h) = (6, 4);
        let plane = shoot([0.1, 0.2, 0.3], w, h);
        let frames: Vec<Frame> = (0..4).map(|_| Frame { data: &plane, width: w, height: h, color: &rggb }).collect();
        let (rgb, ow, oh) = merge_four(&frames, &FOUR_SHOT).unwrap();
        assert_eq!((ow, oh), (w, h));
        assert!(rgb.chunks(3).all(|p| p == [0.1, 0.2, 0.3]));
    }

    #[test]
    fn each_point_sees_every_colour() {
        // Scene with a different value per point; each frame samples it shifted
        let (w, h) = (5, 5);
        let scene = |y: usize, x: usize| (y * 10 + x) as f32;
        let planes: Vec<Vec<f32>> = FOUR_SHOT
            .iter()
            .map(|&(dy, dx)| (0..h).flat_map(|sy: usize| (0..w).map(move |sx: usize| scene(sy.saturating_sub(dy), sx.saturating_sub(dx)))).collect())
            .collect();
        let frames: Vec<Frame> = planes.iter().map(|p| Frame { data: p, width: w, height: h, color: &rggb }).collect();
        let (rgb, _, _) = merge_four(&frames, &FOUR_SHOT).unwrap();
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let p = &rgb[(y * w + x) * 3..][..3];
                assert_eq!(p, [scene(y, x); 3], "at {},{}", y, x);
            }
        }
    }

    #[test]
    fn wrong_input_is_refused() {
        let plane = vec![0f32; 16];
        let f = |w| Frame { data: &plane, width: w, height: 4, color: &rggb };
        assert_eq!(merge_four(&[f(4), f(4)], &FOUR_SHOT).unwrap_err(), Error::FrameCount(2));
        assert_eq!(merge_four(&[f(4), f(4), f(4), f(2)], &FOUR_SHOT).unwrap_err(), Error::SizeMismatch);
    }
}
