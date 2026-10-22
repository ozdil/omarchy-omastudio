//! Medium format support: Hasselblad (3FR/FFF, X/V/H systems), Phase One and Leaf
//! (IIQ, MOS), Mamiya (MEF) and Pentax 645 (PEF/DNG).
//!
//! LibRaw decodes the sensor data of all of them. This module reads what the makers'
//! own software (Phocus, Capture One) uses beside it: the image crop, lens and body
//! data, the camera's colour data, and it adds what those tools do on top: lens cast
//! (flat field) calibration, lens profiles, multi-shot merging and tethering.

pub mod hasselblad;
pub mod hblm;
pub mod lens;
pub mod multishot;
pub mod phase_one;
pub mod tiff;

use crate::recipe::Recipe;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use hasselblad::{HasselbladInfo, Rect};
pub use phase_one::PhaseOneInfo;

/// Raw file extensions of medium format cameras and backs.
pub const RAW_EXTENSIONS: &[&str] = &["3fr", "fff", "iiq", "mos", "mef", "pef"];

/// Whether `path` has one of the medium format raw extensions.
pub fn is_medium_format(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| RAW_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// What a medium format raw says about itself, by maker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "maker", rename_all = "snake_case")]
pub enum MediumFormatInfo {
    Hasselblad(HasselbladInfo),
    PhaseOne(PhaseOneInfo),
}

impl MediumFormatInfo {
    /// Reads a 3FR/FFF/IIQ (Phase One and Leaf); None for other files.
    pub fn read(path: &Path) -> Option<Self> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "3fr" | "fff" => HasselbladInfo::read(path).map(Self::Hasselblad),
            "iiq" => PhaseOneInfo::read(path).map(Self::PhaseOne),
            // Leaf's MOS and some TIFF-wrapped backs carry the same IIQ directory
            "mos" | "tif" | "tiff" => PhaseOneInfo::read(path).map(Self::PhaseOne),
            _ => None,
        }
    }

    /// Image area inside the raw plane that the maker's software keeps.
    pub fn crop(&self) -> Option<Rect> {
        match self {
            Self::Hasselblad(h) => h.crop,
            Self::PhaseOne(_) => None,
        }
    }

    pub fn to_recipe(&self) -> Recipe {
        match self {
            Self::Hasselblad(h) => h.to_recipe(),
            Self::PhaseOne(p) => p.to_recipe(),
        }
    }
}

/// The crop to develop `path` with, if it is a medium format raw that has one.
pub fn develop_crop(path: &Path) -> Option<Rect> {
    MediumFormatInfo::read(path)?.crop()
}

/// Starting recipe for medium format raws without a sidecar.
pub fn camera_recipe(path: &Path) -> Option<Recipe> {
    Some(MediumFormatInfo::read(path)?.to_recipe())
}

/// LibRaw's Bayer `filters` word re-based so that (0, 0) is photosite (dy, dx) of the
/// old origin. Each 2-bit field holds the colour at row 0..8, column 0..2.
pub fn shift_bayer_filters(filters: u32, dy: usize, dx: usize) -> u32 {
    let fc = |row: usize, col: usize| (filters >> ((((row << 1) & 14) | (col & 1)) << 1)) & 3;
    let mut out = 0u32;
    for row in 0..8 {
        for col in 0..2 {
            out |= fc(row + dy, col + dx) << ((((row << 1) & 14) | (col & 1)) << 1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifting_bayer_filters_moves_the_pattern() {
        // RGGB: 0x94949494; one column right gives GRBG (0x61616161), one row down GBRG
        assert_eq!(shift_bayer_filters(0x94949494, 0, 0), 0x94949494);
        assert_eq!(shift_bayer_filters(0x94949494, 0, 1), 0x61616161);
        assert_eq!(shift_bayer_filters(0x94949494, 1, 0), 0x49494949);
        assert_eq!(shift_bayer_filters(0x94949494, 2, 2), 0x94949494);
    }

    #[test]
    fn non_medium_format_files_are_ignored() {
        assert!(MediumFormatInfo::read(Path::new("/nonexistent/a.nef")).is_none());
        assert!(develop_crop(Path::new("/nonexistent/a.3fr")).is_none());
    }
}
