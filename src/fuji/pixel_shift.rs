//! Fujifilm Pixel Shift Multi-Shot (GFX100, GFX100S, GFX100 II): merges the RAF
//! files of one sequence into a linear DNG, as Fujifilm's Pixel Shift Combiner does.
//!
//! Each RAF records its sensor offset in the MakerNote (PixelShiftOffset, 0x1106, in
//! photosites; PixelShiftShots, 0x1105, is the length of the sequence). Four frames
//! with whole-photosite steps give measured red, green and blue at every pixel; 16
//! frames add half-photosite steps and double the resolution (400 MP from a 100 MP
//! sensor). The merge itself is in [`crate::raw::pixel_shift`].
//!
//! The DNG holds linear Rec. 2020 RGB with the as-shot white balance applied, and
//! the camera's lens corrections as DNG opcodes.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::makernote;
use crate::raw::develop::{self, CfaFrame};
use crate::raw::dng::{self, DngInfo};
use crate::raw::pixel_shift::{best_sign, Combiner, Layout, ShiftFrame};
use crate::raw::{OutputColorSpace, RawImage};

const TAG_SHOTS: u16 = 0x1105;
const TAG_OFFSET: u16 = 0x1106;
/// Side of the centre crop used to find the offsets' direction.
const PROBE: usize = 192;

#[derive(Debug, Serialize)]
pub struct Report {
    pub output: String,
    pub frames: usize,
    /// Sequence length the camera recorded, when present.
    pub shots_recorded: Option<i64>,
    pub width: usize,
    pub height: usize,
    /// 2 when half-photosite steps doubled the resolution.
    pub scale: usize,
    /// Share of pixels where every colour was measured (1.0 for a complete sequence).
    pub coverage: f32,
    pub lens_corrections: bool,
}

/// Reads the offset each RAF recorded and merges them into `out`.
pub fn combine(paths: &[PathBuf], out: &Path) -> Result<Report, String> {
    let mut frames = Vec::new();
    let mut shots = None;
    for p in paths {
        let tags = makernote::read_file(p)?;
        let off = tags.get(&TAG_OFFSET).map(|v| v.floats()).unwrap_or_default();
        if off.len() < 2 {
            return Err(format!("{} has no pixel shift offset; is it from a Pixel Shift sequence?", p.display()));
        }
        shots = shots.or(tags.get(&TAG_SHOTS).and_then(|v| v.int()));
        frames.push((p.clone(), (off[0] as f32, off[1] as f32)));
    }
    let mut report = combine_frames(&frames, out)?;
    report.shots_recorded = shots;
    Ok(report)
}

fn open_frame(path: &Path) -> Result<(RawImage, CfaFrame), String> {
    let raw = RawImage::open(path)?;
    let frame = raw.cfa_frame().ok_or_else(|| format!("{} is not a Bayer or X-Trans RAW", path.display()))?;
    if frame.width < 8 || frame.height < 8 {
        return Err(format!("{} is too small for pixel shift ({}x{})", path.display(), frame.width, frame.height));
    }
    Ok((raw, frame))
}

/// Picks up to 4 frame indices whose offsets form a whole-photosite quad around frame 0
/// (e.g. `(x0, y0)`, `(x0±1, y0)`, `(x0, y0±1)`, `(x0±1, y0±1)`). Caching just those 4
/// frames lets both 4-shot and standard 16-shot sequences determine `sign` and merge every
/// RAF in a single LibRaw decode pass with RAM bounded to 4 frames.
fn select_sign_probe_indices(offsets: &[(f32, f32)]) -> Option<Vec<usize>> {
    if offsets.len() <= 4 {
        return Some((0..offsets.len()).collect());
    }
    let (x0, y0) = offsets[0];
    let near = |a: f32, b: f32| (a - b).abs() < 0.05;
    for dx in [1.0f32, -1.0f32] {
        for dy in [1.0f32, -1.0f32] {
            let ix = offsets.iter().position(|&(x, y)| near(x, x0 + dx) && near(y, y0));
            let iy = offsets.iter().position(|&(x, y)| near(x, x0) && near(y, y0 + dy));
            let ixy = offsets.iter().position(|&(x, y)| near(x, x0 + dx) && near(y, y0 + dy));
            if let (Some(ix), Some(iy), Some(ixy)) = (ix, iy, ixy) {
                return Some(vec![0, ix, iy, ixy]);
            }
        }
    }
    None
}

fn extract_probe(f: &CfaFrame) -> (Vec<f32>, usize, (usize, usize)) {
    let side = PROBE.min(f.width / 2).min(f.height / 2);
    let (x0, y0) = ((f.width - side) / 2, (f.height - side) / 2);
    let crop: Vec<f32> = (y0..y0 + side)
        .flat_map(|y| f.data[y * f.width + x0..y * f.width + x0 + side].iter().copied())
        .collect();
    (crop, side, (x0, y0))
}

/// Merges frames with known offsets into a DNG.
pub fn combine_frames(frames: &[(PathBuf, (f32, f32))], out: &Path) -> Result<Report, String> {
    if frames.len() < 2 {
        return Err("Pixel shift needs at least two frames".to_string());
    }

    let offsets: Vec<(f32, f32)> = frames.iter().map(|(_, o)| *o).collect();
    let mut first: Option<(usize, usize, crate::raw::demosaic::CfaPattern)> = None;
    let mut reference = None;
    let mut cached: Vec<(usize, CfaFrame, (f32, f32))> = Vec::new();
    let mut probes = Vec::new();

    if let Some(probe_indices) = select_sign_probe_indices(&offsets) {
        // Single-pass path: decode and cache only the <=4 probe frames, find `sign`,
        // consume them into the Combiner, then stream any remaining frames once each.
        for idx in probe_indices {
            let (path, offset) = &frames[idx];
            let (raw, f) = open_frame(path)?;
            let dims = (f.width, f.height, f.pattern);
            match first {
                None => first = Some(dims),
                Some(d) if d != dims => {
                    return Err(format!("{} differs in size or sensor layout from the first frame", path.display()));
                }
                _ => {}
            }
            if idx == 0 && reference.is_none() {
                let meta = raw.get_metadata().ok();
                let lens = raw.lens_correction().cloned();
                reference = Some((f.camera_to(OutputColorSpace::Rec2020), f.flip, meta, lens));
            }
            let (crop, side, origin) = extract_probe(&f);
            probes.push((crop, side, origin, f.pattern, *offset));
            cached.push((idx, f, *offset));
        }
    } else {
        // Fallback for non-standard offset layouts: probe all frames, then stream Pass 2.
        for (path, offset) in frames {
            let (_, f) = open_frame(path)?;
            let dims = (f.width, f.height, f.pattern);
            match first {
                None => first = Some(dims),
                Some(d) if d != dims => {
                    return Err(format!("{} differs in size or sensor layout from the first frame", path.display()));
                }
                _ => {}
            }
            let (crop, side, origin) = extract_probe(&f);
            probes.push((crop, side, origin, f.pattern, *offset));
        }
    }

    let probe_frames: Vec<ShiftFrame> = probes
        .iter()
        .map(|(d, side, origin, pattern, offset)| ShiftFrame {
            width: *side,
            height: *side,
            data: d,
            pattern: *pattern,
            origin: *origin,
            offset: *offset,
        })
        .collect();
    let sign = best_sign(&probe_frames)?;
    drop(probe_frames);
    drop(probes);

    let (width, height, expected_pattern) = first.ok_or("No frames")?;
    let layout = Layout::new(&offsets, width, height, sign)?;
    let mut comb = Combiner::new(layout);

    let mut consumed = vec![false; frames.len()];
    for (idx, f, offset) in cached.drain(..) {
        comb.add(&ShiftFrame {
            width: f.width,
            height: f.height,
            data: &f.data,
            pattern: f.pattern,
            origin: (0, 0),
            offset,
        });
        consumed[idx] = true;
    }

    for (idx, (path, offset)) in frames.iter().enumerate() {
        if consumed[idx] {
            continue;
        }
        let (raw, f) = open_frame(path)?;
        if (f.width, f.height, f.pattern) != (width, height, expected_pattern) {
            return Err(format!("{} differs in size or sensor layout from the first frame", path.display()));
        }
        comb.add(&ShiftFrame {
            width: f.width,
            height: f.height,
            data: &f.data,
            pattern: f.pattern,
            origin: (0, 0),
            offset: *offset,
        });
        if reference.is_none() {
            let meta = raw.get_metadata().ok();
            let lens = raw.lens_correction().cloned();
            reference = Some((f.camera_to(OutputColorSpace::Rec2020), f.flip, meta, lens));
        }
    }
    let (matrix, flip, meta, lens) = reference.ok_or("No frames")?;
    let coverage = comb.coverage();
    let rgb = comb.finish(matrix);

    let (make, model) = meta.map(|m| (m.make, m.model)).unwrap_or_else(|| ("FUJIFILM".to_string(), String::new()));
    let unique = format!("{} {} Pixel Shift", make, model).trim().to_string();
    let orientation = match flip {
        3 => 3,
        5 => 8,
        6 => 6,
        _ => 1,
    };
    let info = DngInfo {
        make: &make,
        model: &model,
        unique_model: &unique,
        orientation,
        color_matrix: develop::xyz_d65_to_rec2020(),
        lens: lens.as_ref(),
    };
    dng::write_linear_rgb(out, &rgb, layout.width, layout.height, &info)?;
    Ok(Report {
        output: out.display().to_string(),
        frames: frames.len(),
        shots_recorded: None,
        width: layout.width,
        height: layout.height,
        scale: layout.scale,
        coverage,
        lens_corrections: lens.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_sign_probe_finds_quad_for_4_and_16_shot_sequences() {
        let four = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        assert_eq!(select_sign_probe_indices(&four), Some(vec![0, 1, 2, 3]));

        let mut sixteen = Vec::new();
        for &(hx, hy) in &[(0.0, 0.0), (0.5, 0.0), (0.5, 0.5), (0.0, 0.5)] {
            for &(wx, wy) in &four {
                sixteen.push((wx + hx, wy + hy));
            }
        }
        let picked = select_sign_probe_indices(&sixteen).expect("16-shot has whole-step quad");
        assert_eq!(picked.len(), 4);
        assert_eq!(picked[0], 0);
    }

    /// Runs the whole path on a real RAF (set OMASTUDIO_TEST_RAF): four copies of one
    /// frame posing as a whole-step sequence, then the DNG is opened and developed.
    #[test]
    #[ignore]
    fn real_raf_round_trip() {
        let Ok(raf) = std::env::var("OMASTUDIO_TEST_RAF") else { return };
        let raf = PathBuf::from(raf);
        let dir = std::env::temp_dir().join(format!("omastudio-ps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("ps.dng");
        let frames: Vec<_> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().map(|&o| (raf.clone(), o)).collect();
        let report = combine_frames(&frames, &out).unwrap();
        // The sample may be X-Trans, where whole steps cannot cover every colour
        assert!(report.coverage > 0.5, "coverage {}", report.coverage);
        let img = RawImage::open(&out).unwrap();
        let buf = img.process_full_16(3).unwrap();
        assert_eq!((buf.width as usize, buf.height as usize), (report.width, report.height));
        assert!(img.lens_correction().is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
