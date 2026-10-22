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
    Ok((raw, frame))
}

/// Merges frames with known offsets into a DNG.
pub fn combine_frames(frames: &[(PathBuf, (f32, f32))], out: &Path) -> Result<Report, String> {
    if frames.len() < 2 {
        return Err("Pixel shift needs at least two frames".to_string());
    }

    // Pass 1: the same centre crop of every frame, to find the offsets' direction
    let mut probes = Vec::new();
    let mut first: Option<(usize, usize, crate::raw::demosaic::CfaPattern)> = None;
    for (path, offset) in frames {
        let (_, f) = open_frame(path)?;
        let dims = (f.width, f.height, f.pattern);
        match first {
            None => first = Some(dims),
            Some(d) if d != dims => return Err(format!("{} differs in size or sensor layout from the first frame", path.display())),
            _ => {}
        }
        let side = PROBE.min(f.width / 2).min(f.height / 2);
        let (x0, y0) = ((f.width - side) / 2, (f.height - side) / 2);
        let crop: Vec<f32> = (y0..y0 + side).flat_map(|y| f.data[y * f.width + x0..y * f.width + x0 + side].iter().copied()).collect();
        probes.push((crop, side, (x0, y0), f.pattern, *offset));
    }
    let probe_frames: Vec<ShiftFrame> = probes
        .iter()
        .map(|(d, side, origin, pattern, offset)| ShiftFrame { width: *side, height: *side, data: d, pattern: *pattern, origin: *origin, offset: *offset })
        .collect();
    let sign = best_sign(&probe_frames)?;
    drop(probe_frames);
    drop(probes);

    // Pass 2: the whole frames, one at a time
    let (width, height, _) = first.ok_or("No frames")?;
    let offsets: Vec<_> = frames.iter().map(|(_, o)| *o).collect();
    let layout = Layout::new(&offsets, width, height, sign)?;
    let mut comb = Combiner::new(layout);
    let mut reference = None;
    for (path, offset) in frames {
        let (raw, f) = open_frame(path)?;
        comb.add(&ShiftFrame { width: f.width, height: f.height, data: &f.data, pattern: f.pattern, origin: (0, 0), offset: *offset });
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
