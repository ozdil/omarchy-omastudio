//! Hasselblad 3FR and FFF: what the camera records beside the sensor data.
//!
//! Both are TIFF containers. The CFA raw sits in a SubIFD with DNG-style tags
//! (BlackLevel, WhiteLevel, DefaultCropOrigin/Size, MaskedAreas), IFD0 carries the
//! DNG ColorMatrix1 and AsShotNeutral, and the EXIF MakerNote is a plain IFD whose
//! value offsets count from the start of the file. FFF is what Phocus writes when it
//! imports a 3FR; the layout is the same. Tag meanings were worked out from sample
//! files of X2D 100C, X1D II 50C, CFV II 50C, H6D-100c, H5D, H4D, H3D and CF backs.

use super::tiff::{Directories, Tiff, Value};
use crate::recipe::Recipe;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// MakerNote: camera model name, for example "X2D 100C".
const MN_MODEL: u16 = 0x0015;
/// MakerNote: [1, x, y, width, height], the same crop as DefaultCropOrigin/Size.
const MN_CROP: u16 = 0x0059;

/// Rectangle in sensor (raw plane) coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The camera systems Hasselblad's raws come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum System {
    /// X1D, X1D II, X2D, X2D II: mirrorless, XCD lenses.
    X,
    /// 907X with a CFV back.
    V,
    /// H3D to H6D (including multi-shot bodies), HC/HCD lenses.
    H,
    /// Ixpress/CF digital backs on view and V cameras.
    Back,
    Unknown,
}

impl System {
    fn from_model(model: &str) -> Self {
        let m = model.to_ascii_uppercase();
        if m.starts_with('X') {
            Self::X
        } else if m.starts_with("CFV") || m.contains("907X") {
            Self::V
        } else if m.starts_with('H') {
            Self::H
        } else if m.starts_with("CF") || m.contains("IXPRESS") {
            Self::Back
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HasselbladInfo {
    pub model: String,
    pub system: System,
    pub firmware: Option<String>,
    pub lens: Option<String>,
    pub body_serial: Option<String>,
    pub lens_serial: Option<String>,
    /// Full raw plane size.
    pub raw_width: u32,
    pub raw_height: u32,
    /// Image area to keep; the rest is masked or dark border.
    pub crop: Option<Rect>,
    /// Optically black areas, for black level estimation.
    pub masked_areas: Vec<Rect>,
    pub black_level: Option<f64>,
    pub white_level: Option<f64>,
    /// DNG ColorMatrix1 (XYZ -> camera), row-major 3x3.
    pub color_matrix: Option<[f64; 9]>,
    pub as_shot_neutral: Option<[f64; 3]>,
    pub baseline_sharpness: Option<f64>,
    /// Number of CFA frames in the file (multi-shot bodies store more than one).
    pub frames: u32,
    /// Whether the file came from Phocus (FFF) rather than straight from the camera.
    pub phocus_file: bool,
}

impl HasselbladInfo {
    /// Reads a 3FR/FFF; None for files from other makers.
    pub fn read(path: &Path) -> Option<Self> {
        let mut t = Tiff::open(path).ok()?;
        let dirs = Directories::read(&mut t);
        let make = dirs.find(0x010f).and_then(Value::text).unwrap_or("");
        let unique = dirs.find(0xc614).and_then(Value::text).unwrap_or("");
        if !make.eq_ignore_ascii_case("hasselblad") && !unique.starts_with("Hasselblad") {
            return None;
        }

        let mn = dirs.makernote.map(|(offset, _)| t.values(offset, 0).0).unwrap_or_default();
        let text = |v: Option<&Value>| v.and_then(Value::text).map(|s| s.trim().to_string());
        let model = text(mn.get(&MN_MODEL))
            .or_else(|| text(dirs.find(0x0110)))
            .map(|m| m.trim_start_matches("Hasselblad ").to_string())
            .unwrap_or_default();

        let raw = dirs.raw_ifd();
        let rget = |tag: u16| raw.and_then(|r| r.get(&tag));
        let raw_width = rget(0x0100).and_then(Value::int).unwrap_or(0).max(0) as u32;
        let raw_height = rget(0x0101).and_then(Value::int).unwrap_or(0).max(0) as u32;

        let crop = match (rget(0xc61f).map(Value::floats), rget(0xc620).map(Value::floats)) {
            (Some(o), Some(s)) if o.len() == 2 && s.len() == 2 => Some(Rect {
                x: o[0] as u32,
                y: o[1] as u32,
                width: s[0] as u32,
                height: s[1] as u32,
            }),
            _ => mn.get(&MN_CROP).map(Value::ints).filter(|v| v.len() == 5).map(|v| Rect {
                x: v[1] as u32,
                y: v[2] as u32,
                width: v[3] as u32,
                height: v[4] as u32,
            }),
        }
        .filter(|r| r.width > 0 && r.height > 0 && r.x + r.width <= raw_width && r.y + r.height <= raw_height);

        // MaskedAreas: top, left, bottom, right per rectangle
        let masked_areas = rget(0xc68e)
            .map(Value::ints)
            .unwrap_or_default()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|a| a[2] > a[0] && a[3] > a[1])
            .map(|a| Rect { x: a[1] as u32, y: a[0] as u32, width: (a[3] - a[1]) as u32, height: (a[2] - a[0]) as u32 })
            .collect();

        let floats = |v: Option<&Value>| v.map(Value::floats).unwrap_or_default();
        let cm = floats(dirs.find(0xc621));
        let neutral = floats(dirs.find(0xc628));
        let frames = dirs
            .sub
            .iter()
            .chain(dirs.main.iter())
            .filter(|m| m.get(&0x0106).and_then(Value::int) == Some(32803))
            .count() as u32;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();

        Some(Self {
            system: System::from_model(&model),
            model,
            firmware: text(dirs.find(0x0131)),
            lens: text(dirs.exif.get(&0xa434)).filter(|l| !l.is_empty()),
            body_serial: text(dirs.exif.get(&0xa431)),
            lens_serial: text(dirs.exif.get(&0xa435)),
            raw_width,
            raw_height,
            crop,
            masked_areas,
            black_level: rget(0xc61a).map(Value::floats).and_then(|v| v.first().copied()),
            white_level: rget(0xc61d).map(Value::floats).and_then(|v| v.first().copied()),
            color_matrix: (cm.len() >= 9).then(|| std::array::from_fn(|i| cm[i])),
            as_shot_neutral: (neutral.len() == 3).then(|| [neutral[0], neutral[1], neutral[2]]),
            baseline_sharpness: dirs.find(0xc62c).map(Value::floats).and_then(|v| v.first().copied()),
            frames: frames.max(1),
            phocus_file: ext == "fff",
        })
    }

    /// Starting recipe: Hasselblad's look is one natural colour rendering for every
    /// camera, so the HNCS simulation is applied and nothing else is pushed.
    #[allow(clippy::field_reassign_with_default)]
    pub fn to_recipe(&self) -> Recipe {
        let mut r = Recipe::default();
        r.film_simulation = "hasselblad_hncs".to_string();
        r.film_sim_intensity = 1.0;
        r.preset_name = Some("Camera: Hasselblad Natural Colour".to_string());
        // X-system files ask for more capture sharpening than the H backs
        if let Some(s) = self.baseline_sharpness {
            r.sharpness = (s as f32 * 20.0).clamp(10.0, 60.0);
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_from_model_names() {
        assert_eq!(System::from_model("X2D 100C"), System::X);
        assert_eq!(System::from_model("X1D II 50C"), System::X);
        assert_eq!(System::from_model("CFV II 50C"), System::V);
        assert_eq!(System::from_model("H6D-100cMS"), System::H);
        assert_eq!(System::from_model("H3DII-39"), System::H);
        assert_eq!(System::from_model("CF-22"), System::Back);
    }

    #[test]
    fn other_makers_are_not_hasselblad() {
        let bytes = crate::medium_format::tiff::tests::tiff(&[(0x010f, 2, b"Phase One\0".iter().map(|&b| b as u32).collect())]);
        let p = std::env::temp_dir().join(format!("omastudio-h-{}.3fr", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        assert!(HasselbladInfo::read(&p).is_none());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn reads_crop_and_dng_tags_from_ifd0_raw() {
        let text = |s: &str| s.bytes().chain(std::iter::once(0)).map(u32::from).collect::<Vec<_>>();
        let bytes = crate::medium_format::tiff::tests::tiff(&[
            (0x0100, 3, vec![11904]),
            (0x0101, 3, vec![8842]),
            (0x0106, 3, vec![32803]),
            (0x010f, 2, text("Hasselblad")),
            (0x0110, 2, text("Hasselblad X2D 100C")),
            (0xc61a, 5, vec![4096, 1]),
            (0xc61f, 3, vec![128, 96]),
            (0xc620, 3, vec![11656, 8742]),
            (0xc628, 5, vec![38, 100, 1, 1, 63, 100]),
            (0xc68e, 3, vec![12, 76, 50, 11820]),
        ]);
        let p = std::env::temp_dir().join(format!("omastudio-x2d-{}.3fr", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        let info = HasselbladInfo::read(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(info.model, "X2D 100C");
        assert_eq!(info.system, System::X);
        assert_eq!(info.crop, Some(Rect { x: 128, y: 96, width: 11656, height: 8742 }));
        assert_eq!(info.masked_areas, vec![Rect { x: 76, y: 12, width: 11744, height: 38 }]);
        assert_eq!(info.black_level, Some(4096.0));
        assert_eq!(info.as_shot_neutral, Some([0.38, 1.0, 0.63]));
        assert_eq!(info.to_recipe().film_simulation, "hasselblad_hncs");
    }
}
