//! Fujifilm support: the in-camera recipe (film simulation, grain, Color Chrome, tone,
//! dynamic range...) read from a RAF or JPEG, and its mapping onto an OmaStudio recipe.

pub mod baseline;
pub mod calibrate;
pub mod compat;
pub mod fp1;
pub mod lens;
pub mod library;
pub mod makernote;
pub mod pixel_shift;
pub mod ptp;
pub mod text;

use crate::recipe::Recipe;
use makernote::Tags;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Film simulation selected in the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilmSim {
    Provia,
    Velvia,
    Astia,
    ClassicChrome,
    RealaAce,
    ProNegHi,
    ProNegStd,
    ClassicNeg,
    NostalgicNeg,
    Eterna,
    BleachBypass,
    Acros,
    AcrosYellow,
    AcrosRed,
    AcrosGreen,
    Monochrome,
    MonochromeYellow,
    MonochromeRed,
    MonochromeGreen,
    Sepia,
    /// Older bodies: F1/F2/F3 studio portrait modes.
    Other,
}

impl FilmSim {
    /// OmaStudio film simulation id used by the pipeline.
    pub fn engine_id(self) -> &'static str {
        match self {
            Self::Provia | Self::Other => "fuji_provia",
            Self::Velvia => "fuji_velvia",
            Self::Astia => "fuji_astia",
            Self::ClassicChrome => "fuji_classic_chrome",
            Self::RealaAce => "fuji_reala_ace",
            Self::ProNegHi => "fuji_pro_neg_hi",
            Self::ProNegStd => "fuji_pro_neg_std",
            Self::ClassicNeg => "fuji_classic_neg",
            Self::NostalgicNeg => "fuji_nostalgic_neg",
            Self::Eterna => "fuji_eterna",
            Self::BleachBypass => "fuji_eterna_bleach_bypass",
            Self::Acros => "fuji_acros_standard",
            Self::AcrosYellow => "fuji_acros_yellow",
            Self::AcrosRed => "fuji_acros_red",
            Self::AcrosGreen => "fuji_acros_green",
            Self::Monochrome => "fuji_monochrome",
            Self::MonochromeYellow => "fuji_monochrome_yellow",
            Self::MonochromeRed => "fuji_monochrome_red",
            Self::MonochromeGreen => "fuji_monochrome_green",
            Self::Sepia => "fuji_sepia",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Provia => "Provia/Standard",
            Self::Velvia => "Velvia/Vivid",
            Self::Astia => "Astia/Soft",
            Self::ClassicChrome => "Classic Chrome",
            Self::RealaAce => "Reala Ace",
            Self::ProNegHi => "Pro Neg. Hi",
            Self::ProNegStd => "Pro Neg. Std",
            Self::ClassicNeg => "Classic Neg.",
            Self::NostalgicNeg => "Nostalgic Neg.",
            Self::Eterna => "Eterna/Cinema",
            Self::BleachBypass => "Eterna Bleach Bypass",
            Self::Acros => "Acros",
            Self::AcrosYellow => "Acros+Ye",
            Self::AcrosRed => "Acros+R",
            Self::AcrosGreen => "Acros+G",
            Self::Monochrome => "Monochrome",
            Self::MonochromeYellow => "Monochrome+Ye",
            Self::MonochromeRed => "Monochrome+R",
            Self::MonochromeGreen => "Monochrome+G",
            Self::Sepia => "Sepia",
            Self::Other => "Standard",
        }
    }

    pub fn is_monochrome(self) -> bool {
        matches!(
            self,
            Self::Acros
                | Self::AcrosYellow
                | Self::AcrosRed
                | Self::AcrosGreen
                | Self::Monochrome
                | Self::MonochromeYellow
                | Self::MonochromeRed
                | Self::MonochromeGreen
                | Self::Sepia
        )
    }
}

/// Off / Weak / Strong, as used by Grain, Color Chrome and Color Chrome FX Blue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    #[default]
    Off,
    Weak,
    Strong,
}

impl Strength {
    fn from_tag(v: Option<i64>) -> Self {
        match v {
            Some(32) => Self::Weak,
            Some(64) => Self::Strong,
            _ => Self::Off,
        }
    }

    /// 0, 50 or 100.
    pub fn amount(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Weak => 50.0,
            Self::Strong => 100.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrainSize {
    #[default]
    Small,
    Large,
}

/// The recipe a Fujifilm camera recorded with a photo. Steps use the camera's own
/// scales (for example highlight -2..+4, colour -4..+4), so it reads like the menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraRecipe {
    pub camera: String,
    pub film_simulation: FilmSim,
    pub film_simulation_name: String,
    pub grain: Strength,
    pub grain_size: GrainSize,
    pub color_chrome: Strength,
    pub color_chrome_fx_blue: Strength,
    pub white_balance: String,
    /// Kelvin, when the white balance was set by colour temperature.
    pub color_temperature: Option<u32>,
    /// White balance shift in camera steps (red, blue), -9..+9.
    pub wb_shift: (i32, i32),
    /// 100, 200 or 400; None when unknown.
    pub dynamic_range: Option<u32>,
    pub dynamic_range_auto: bool,
    pub d_range_priority: Option<String>,
    pub highlight: f32,
    pub shadow: f32,
    pub color: i32,
    pub sharpness: i32,
    pub noise_reduction: i32,
    pub clarity: i32,
    /// Monochromatic colour: warm(+)/cool(-) and magenta(-)/green(+), -9..+9.
    pub mono_warm_cool: i32,
    pub mono_magenta_green: i32,
    /// Exposure compensation in EV (FP1 and text recipes; 0 for photos).
    #[serde(default)]
    pub exposure_bias: f32,
}

impl Default for CameraRecipe {
    fn default() -> Self {
        Self {
            camera: String::new(),
            film_simulation: FilmSim::Provia,
            film_simulation_name: FilmSim::Provia.display_name().to_string(),
            grain: Strength::Off,
            grain_size: GrainSize::Small,
            color_chrome: Strength::Off,
            color_chrome_fx_blue: Strength::Off,
            white_balance: "Auto".to_string(),
            color_temperature: None,
            wb_shift: (0, 0),
            dynamic_range: Some(100),
            dynamic_range_auto: false,
            d_range_priority: None,
            highlight: 0.0,
            shadow: 0.0,
            color: 0,
            sharpness: 0,
            noise_reduction: 0,
            clarity: 0,
            mono_warm_cool: 0,
            mono_magenta_green: 0,
            exposure_bias: 0.0,
        }
    }
}

impl CameraRecipe {
    /// Reads the camera recipe of a RAF or Fujifilm JPEG; None for other files.
    pub fn read(path: &Path) -> Option<Self> {
        let tags = makernote::read_file(path).ok()?;
        Self::from_tags(&tags, &camera_model(&tags))
    }

    pub fn from_tags(tags: &Tags, camera: &str) -> Option<Self> {
        let int = |t: u16| tags.get(&t).and_then(|v| v.int());
        // A MakerNote without film mode or saturation is not a recipe-bearing Fujifilm one
        if int(0x1401).is_none() && int(0x1003).is_none() {
            return None;
        }

        let saturation = int(0x1003).unwrap_or(0);
        let film_simulation = match saturation {
            0x300 => FilmSim::Monochrome,
            0x301 => FilmSim::MonochromeRed,
            0x302 => FilmSim::MonochromeYellow,
            0x303 => FilmSim::MonochromeGreen,
            0x310 => FilmSim::Sepia,
            0x500 => FilmSim::Acros,
            0x501 => FilmSim::AcrosRed,
            0x502 => FilmSim::AcrosYellow,
            0x503 => FilmSim::AcrosGreen,
            _ => match int(0x1401).unwrap_or(0) {
                0x000 => FilmSim::Provia,
                0x120 => FilmSim::Astia,
                0x200 | 0x400 => FilmSim::Velvia,
                0x500 => FilmSim::ProNegStd,
                0x501 => FilmSim::ProNegHi,
                0x600 => FilmSim::ClassicChrome,
                0x700 => FilmSim::Eterna,
                0x800 => FilmSim::ClassicNeg,
                0x900 => FilmSim::BleachBypass,
                0xa00 => FilmSim::NostalgicNeg,
                0xb00 => FilmSim::RealaAce,
                _ => FilmSim::Other,
            },
        };
        let color = match saturation {
            0x0 => 0,
            0x080 => 1,
            0x100 => 2,
            0x0c0 => 3,
            0x0e0 => 4,
            0x180 => -1,
            0x400 | 0x200 => -2,
            0x4c0 => -3,
            0x4e0 => -4,
            _ => 0,
        };
        let sharpness = match int(0x1001) {
            Some(0x00) => -4,
            Some(0x01) => -3,
            Some(0x02) => -2,
            Some(0x82) => -1,
            Some(0x84) => 1,
            Some(0x04) => 2,
            Some(0x05) => 3,
            Some(0x06) => 4,
            _ => 0,
        };
        let noise_reduction = match int(0x100e) {
            Some(0x100) => 2,
            Some(0x180) => 1,
            Some(0x1c0) => 3,
            Some(0x1e0) => 4,
            Some(0x200) => -2,
            Some(0x280) => -1,
            Some(0x2c0) => -3,
            Some(0x2e0) => -4,
            _ => 0,
        };
        let white_balance = match int(0x1002) {
            Some(0x0) => "Auto",
            Some(0x1) => "Auto (white priority)",
            Some(0x2) => "Auto (ambience priority)",
            Some(0x100) => "Daylight",
            Some(0x200) => "Shade",
            Some(0x300..=0x304) => "Fluorescent",
            Some(0x400) => "Incandescent",
            Some(0x500) => "Flash",
            Some(0x600) => "Underwater",
            Some(0xf00..=0xf04) => "Custom",
            Some(0xff0) => "Kelvin",
            _ => "Unknown",
        }
        .to_string();
        let color_temperature = (int(0x1002) == Some(0xff0)).then(|| int(0x1005)).flatten().map(|k| k as u32);
        // Newer bodies store the shift multiplied by 20
        let wb = tags.get(&0x100a).map(|v| v.ints().to_vec()).unwrap_or_default();
        let shift = |v: Option<&i64>| v.map(|&x| if x.abs() > 9 { x / 20 } else { x } as i32).unwrap_or(0);
        let wb_shift = (shift(wb.first()), shift(wb.get(1)));

        let dr_setting = int(0x1402);
        let dynamic_range_auto = dr_setting == Some(0);
        let dynamic_range = match dr_setting {
            Some(0x100) => Some(100),
            Some(0x200) => Some(200),
            Some(0x201) => Some(400),
            // Auto / manual: the value actually used
            _ => int(0x140b).or(int(0x1403)).filter(|v| [100, 200, 400].contains(v)).map(|v| v as u32),
        };
        // D-Range Priority replaces the DR setting (no 0x1402 / 0x140b then). In Auto,
        // some bodies (GFX 100) record the level picked in the "fixed" tag instead
        let d_range_priority = match (int(0x1443), int(0x1444), int(0x1445)) {
            (Some(0), Some(1), _) | (Some(0), None, Some(1)) => Some("Auto (weak)"),
            (Some(0), Some(2), _) | (Some(0), None, Some(2)) => Some("Auto (strong)"),
            (Some(0), Some(3), _) => Some("Auto (plus)"),
            (Some(0), _, _) => Some("Auto"),
            (Some(1), _, Some(1)) => Some("Weak"),
            (Some(1), _, Some(2)) => Some("Strong"),
            _ => None,
        }
        .map(str::to_string);

        // `+ 0.0` turns -0.0 into 0.0 for tags that hold zero
        let tone = |t: u16| int(t).map(|v| -(v as f32) / 16.0 + 0.0).unwrap_or(0.0);
        Some(Self {
            camera: camera.to_string(),
            film_simulation,
            film_simulation_name: film_simulation.display_name().to_string(),
            grain: Strength::from_tag(int(0x1047)),
            grain_size: if int(0x104c) == Some(32) { GrainSize::Large } else { GrainSize::Small },
            color_chrome: Strength::from_tag(int(0x1048)),
            color_chrome_fx_blue: Strength::from_tag(int(0x104e)),
            white_balance,
            color_temperature,
            wb_shift,
            dynamic_range,
            dynamic_range_auto,
            d_range_priority,
            highlight: tone(0x1041),
            shadow: tone(0x1040),
            color,
            sharpness,
            noise_reduction,
            clarity: int(0x100f).map(|v| (v / 1000) as i32).unwrap_or(0),
            mono_warm_cool: int(0x1049).unwrap_or(0) as i32,
            mono_magenta_green: int(0x104b).unwrap_or(0) as i32,
            exposure_bias: 0.0,
        })
    }

    /// Starting OmaStudio recipe that renders like this camera recipe.
    ///
    /// Camera steps are mapped onto OmaStudio's -100..100 sliders. The raw file
    /// already carries the camera's white balance, so only the shift is applied.
    #[allow(clippy::field_reassign_with_default)]
    pub fn to_recipe(&self) -> Recipe {
        let mut r = Recipe::default();
        r.film_simulation = self.film_simulation.engine_id().to_string();
        r.preset_name = Some(format!("Camera: {}", self.film_simulation_name));

        // Highlight/Shadow -2..+4: positive is harder (brighter highlights, deeper shadows)
        r.highlights = (self.highlight * 12.5).clamp(-100.0, 100.0);
        r.shadows = (-self.shadow * 12.5).clamp(-100.0, 100.0);
        r.saturation = if self.film_simulation.is_monochrome() { 0.0 } else { self.color as f32 * 10.0 };
        r.sharpness = (25.0 + self.sharpness as f32 * 10.0).clamp(0.0, 100.0);
        r.denoise_lum = (10.0 + self.noise_reduction as f32 * 5.0).clamp(0.0, 100.0);
        r.clarity = self.clarity as f32 * 10.0;

        // DR200/DR400 expose the raw 1/2 stops darker and lift it back in development.
        // D-Range Priority does the same: Weak as DR200, Strong and Plus as DR400
        let drp = self.d_range_priority.as_deref().map(str::to_ascii_lowercase);
        let effective_dr = self.dynamic_range.or(match drp.as_deref() {
            Some(p) if p.contains("weak") => Some(200),
            Some(p) if p.contains("strong") || p.contains("plus") => Some(400),
            _ => None,
        });
        if let Some(dr) = effective_dr
            && dr > 100
        {
            let stops = (dr as f32 / 100.0).log2();
            r.exposure = stops;
            r.highlights -= 25.0 * stops;
        }

        r.exposure += self.exposure_bias;

        // WB shift: blue steps move along the amber/blue axis, red steps along magenta/green
        let (red, blue) = self.wb_shift;
        r.wb_temperature = 5500.0 - blue as f32 * 150.0 + red as f32 * 50.0;
        r.wb_tint = red as f32 * 4.0;

        if self.grain != Strength::Off {
            r.grain_amount = if self.grain == Strength::Weak { 20.0 } else { 40.0 };
            r.grain_roughness = if self.grain == Strength::Weak { 35.0 } else { 65.0 };
            r.grain_size = if self.grain_size == GrainSize::Large { 2.0 } else { 1.0 };
        }
        r.color_chrome = self.color_chrome.amount();
        r.color_chrome_blue = self.color_chrome_fx_blue.amount();
        r
    }
}

fn camera_model(tags: &Tags) -> String {
    // FujiModel / FujiModel2 (0x1447 / 0x1448) carry "X-T30_0100"
    for t in [0x1447u16, 0x1448] {
        if let Some(makernote::Value::Text(s)) = tags.get(&t) {
            return s.split('_').next().unwrap_or(s).to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use makernote::Value;

    fn tags(entries: &[(u16, i64)]) -> Tags {
        entries.iter().map(|&(t, v)| (t, Value::Ints(vec![v]))).collect()
    }

    #[test]
    fn decodes_classic_chrome_recipe() {
        let mut t = tags(&[
            (0x1401, 0x600),
            (0x1003, 0x180),
            (0x1001, 0x84),
            (0x100e, 0x2c0),
            (0x100f, -2000),
            (0x1041, -16),
            (0x1040, 24),
            (0x1047, 64),
            (0x104c, 32),
            (0x1048, 32),
            (0x104e, 64),
            (0x1402, 0x200),
        ]);
        t.insert(0x100a, Value::Ints(vec![40, -80]));
        let r = CameraRecipe::from_tags(&t, "X-T30").unwrap();
        assert_eq!(r.film_simulation, FilmSim::ClassicChrome);
        assert_eq!((r.color, r.sharpness, r.noise_reduction, r.clarity), (-1, 1, -3, -2));
        assert_eq!((r.highlight, r.shadow), (1.0, -1.5));
        assert_eq!((r.grain, r.grain_size), (Strength::Strong, GrainSize::Large));
        assert_eq!((r.color_chrome, r.color_chrome_fx_blue), (Strength::Weak, Strength::Strong));
        assert_eq!(r.wb_shift, (2, -4));
        assert_eq!(r.dynamic_range, Some(200));

        let recipe = r.to_recipe();
        assert_eq!(recipe.film_simulation, "fuji_classic_chrome");
        assert!(recipe.exposure > 0.99 && recipe.highlights < 0.0);
        assert_eq!(recipe.color_chrome, 50.0);
    }

    #[test]
    fn monochrome_lives_in_the_saturation_tag() {
        let r = CameraRecipe::from_tags(&tags(&[(0x1401, 0x0), (0x1003, 0x502)]), "").unwrap();
        assert_eq!(r.film_simulation, FilmSim::AcrosYellow);
        assert_eq!(r.to_recipe().saturation, 0.0);
    }

    #[test]
    fn d_range_priority_lifts_like_the_matching_dr() {
        // GFX 100: D-Range Priority Auto, the level picked only in the "fixed" tag
        let r = CameraRecipe::from_tags(&tags(&[(0x1401, 0x0), (0x1400, 1), (0x1443, 0), (0x1445, 1)]), "GFX 100").unwrap();
        assert_eq!(r.dynamic_range, None);
        assert_eq!(r.d_range_priority.as_deref(), Some("Auto (weak)"));
        assert!((r.to_recipe().exposure - 1.0).abs() < 1e-6);

        let strong = CameraRecipe::from_tags(&tags(&[(0x1401, 0x0), (0x1443, 1), (0x1445, 2)]), "").unwrap();
        assert!((strong.to_recipe().exposure - 2.0).abs() < 1e-6);
    }

    #[test]
    fn non_recipe_makernotes_are_ignored() {
        assert!(CameraRecipe::from_tags(&tags(&[(0x1000, 1)]), "").is_none());
    }
}
