#![allow(clippy::field_reassign_with_default)]
use crate::raw::RawMetadata;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
// time

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Recipe {
    // White Balance
    pub wb_temperature: f32, // 2000K to 12000K (Default: 5500.0)
    pub wb_tint: f32,        // -100.0 to +100.0 (Default: 0.0)

    // Photometric Tone
    pub exposure: f32,   // -5.0 to +5.0 EV (Default: 0.0)
    pub contrast: f32,   // -100.0 to +100.0 (Default: 0.0)
    pub highlights: f32, // -100.0 to +100.0 (Default: 0.0)
    pub shadows: f32,    // -100.0 to +100.0 (Default: 0.0)
    pub whites: f32,     // -100.0 to +100.0 (Default: 0.0)
    pub blacks: f32,     // -100.0 to +100.0 (Default: 0.0)
    #[serde(default)]
    pub highlight_reconstruct: bool, // Luma-guided clipped channel inpainting
    #[serde(default)]
    pub filmic_agx: bool,            // Sigmoidal AgX perceptual tone mapper

    // Presence
    pub texture: f32,    // -100.0 to +100.0 (Default: 0.0)
    pub clarity: f32,    // -100.0 to +100.0 (Default: 0.0)
    pub dehaze: f32,     // -100.0 to +100.0 (Default: 0.0)
    pub vibrance: f32,   // -100.0 to +100.0 (Default: 0.0)
    pub saturation: f32, // -100.0 to +100.0 (Default: 0.0)

    // Tone Curve (Highlights, Lights, Darks, Shadows)
    pub curve_highlights: f32,
    pub curve_lights: f32,
    pub curve_darks: f32,
    pub curve_shadows: f32,

    // HSL 8 Channels: [Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta]
    pub hsl_hue: [f32; 8],
    pub hsl_sat: [f32; 8],
    pub hsl_lum: [f32; 8],

    // DaVinci Resolve Primaries: Lift, Gamma, Gain, Offset
    #[serde(default)]
    pub lift: [f32; 3], // RGB Tint (-1.0 to +1.0)
    #[serde(default)]
    pub lift_luma: f32, // Shadows Master Luma (-1.0 to +1.0)
    #[serde(default)]
    pub gamma: [f32; 3], // RGB Tint (-1.0 to +1.0)
    #[serde(default)]
    pub gamma_luma: f32, // Midtones Master Luma (-1.0 to +1.0)
    #[serde(default)]
    pub gain: [f32; 3], // RGB Tint (-1.0 to +1.0)
    #[serde(default)]
    pub gain_luma: f32, // Highlights Master Luma (-1.0 to +1.0)
    #[serde(default)]
    pub offset: [f32; 3], // RGB Tint (-1.0 to +1.0)
    #[serde(default)]
    pub offset_luma: f32, // Overall Master Luma (-1.0 to +1.0)

    // DaVinci Resolve Signature Primaries & 3D LUT
    #[serde(default = "default_contrast_pivot")]
    pub contrast_pivot: f32, // 0.0 to 1.0 (Default: 0.435)
    #[serde(default)]
    pub color_boost: f32, // -100.0 to +100.0 (Default: 0.0)
    #[serde(default)]
    pub midtone_detail: f32, // -100.0 to +100.0 (Default: 0.0)
    #[serde(default)]
    pub lut_name: Option<String>,
    #[serde(default = "default_lut_intensity")]
    pub lut_intensity: f32, // 0.0 to 1.0 (Default: 1.0)
    #[serde(default)]
    pub lut_path: Option<String>,

    // Detail & Optics
    pub sharpness: f32,   // 0.0 to 100.0
    pub denoise_lum: f32, // 0.0 to 100.0
    pub denoise_col: f32, // 0.0 to 100.0
    pub vignette: f32,    // -100.0 to +100.0
    #[serde(default)]
    pub lens_distortion: f32, // -100.0 to +100.0 (Barrel / Pincushion)
    #[serde(default)]
    pub defringe: f32, // 0.0 to 100.0 (Chromatic Aberration reduction)

    // Geometry & Crop
    #[serde(default)]
    pub rotation: f32, // Degrees
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    #[serde(default)]
    pub crop_x: f32, // Normalized 0.0 to 1.0
    #[serde(default)]
    pub crop_y: f32,
    #[serde(default = "default_crop_dimension")]
    pub crop_w: f32,
    #[serde(default = "default_crop_dimension")]
    pub crop_h: f32,
    #[serde(default = "default_crop_aspect")]
    pub crop_aspect: String,

    // Preset identifier if applied
    pub preset_name: Option<String>,

    // Lightroom Studio Culling & Rating
    #[serde(default)]
    pub rating: u8, // 0 to 5 stars
    #[serde(default)]
    pub flag: String, // "none", "pick", "reject"

    // ACES 1.3 & Wide Gamut Color Spaces
    #[serde(default = "default_color_space")]
    pub color_space: String, // "sRGB", "DisplayP3", "Rec2020", "ACEScg"
    #[serde(default)]
    pub aces_tonemap: bool,

    // Authentic Film Simulations & Medium Format Color Science
    #[serde(default)]
    pub film_simulation: String, // "fuji_velvia", "fuji_classic_chrome", "hasselblad_hncs", etc.
    #[serde(default = "default_film_sim_intensity")]
    pub film_sim_intensity: f32, // 0.0 to 1.0 (Default: 1.0)
}

fn default_color_space() -> String {
    "sRGB".to_string()
}

fn default_film_sim_intensity() -> f32 {
    1.0
}

fn default_contrast_pivot() -> f32 {
    0.435
}

fn default_lut_intensity() -> f32 {
    1.0
}

fn default_crop_dimension() -> f32 {
    1.0
}

fn default_crop_aspect() -> String {
    "Original".to_string()
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            wb_temperature: 5500.0,
            wb_tint: 0.0,
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            highlight_reconstruct: true,
            filmic_agx: false,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            curve_highlights: 0.0,
            curve_lights: 0.0,
            curve_darks: 0.0,
            curve_shadows: 0.0,
            hsl_hue: [0.0; 8],
            hsl_sat: [0.0; 8],
            hsl_lum: [0.0; 8],
            lift: [0.0; 3],
            lift_luma: 0.0,
            gamma: [0.0; 3],
            gamma_luma: 0.0,
            gain: [0.0; 3],
            gain_luma: 0.0,
            offset: [0.0; 3],
            offset_luma: 0.0,
            contrast_pivot: 0.435,
            color_boost: 0.0,
            midtone_detail: 0.0,
            lut_name: None,
            lut_intensity: 1.0,
            lut_path: None,
            sharpness: 25.0, // Default subtle lens sharpening
            denoise_lum: 0.0,
            denoise_col: 10.0, // Default chromatic cleanup
            vignette: 0.0,
            lens_distortion: 0.0,
            defringe: 0.0,
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
            crop_x: 0.0,
            crop_y: 0.0,
            crop_w: 1.0,
            crop_h: 1.0,
            crop_aspect: "Original".to_string(),
            preset_name: None,
            rating: 0,
            flag: "none".to_string(),
            color_space: "sRGB".to_string(),
            aces_tonemap: false,
            film_simulation: String::new(),
            film_sim_intensity: 1.0,
        }
    }
}

impl Recipe {
    pub fn fuji_classic_chrome() -> Self {
        let mut r = Self::default();
        r.preset_name = Some("Fuji Classic Chrome".to_string());
        r.contrast = 15.0;
        r.highlights = -10.0;
        r.shadows = 5.0;
        r.clarity = 12.0;
        r.vibrance = -15.0;
        r.saturation = -8.0;
        // Subtle teal/cyan sky and warm earth tones
        r.hsl_sat[1] = 10.0;  // Orange
        r.hsl_sat[5] = -20.0; // Blue
        r.hsl_hue[5] = -8.0;  // Shift blue toward cyan
        r.vignette = -12.0;
        r
    }

    pub fn fuji_velvia() -> Self {
        let mut r = Self::default();
        r.preset_name = Some("Fuji Velvia 50".to_string());
        r.contrast = 28.0;
        r.highlights = -15.0;
        r.shadows = 10.0;
        r.vibrance = 30.0;
        r.saturation = 18.0;
        r.clarity = 16.0;
        r.hsl_sat[3] = 25.0; // Green boost
        r.hsl_sat[5] = 25.0; // Deep blue sky
        r
    }

    pub fn leica_monochrom() -> Self {
        let mut r = Self::default();
        r.preset_name = Some("Leica Monochrom HC".to_string());
        r.saturation = -100.0; // Full B&W
        r.vibrance = -100.0;
        r.contrast = 35.0;
        r.highlights = -20.0;
        r.shadows = -10.0;
        r.whites = 15.0;
        r.blacks = -25.0;
        r.texture = 15.0;
        r.clarity = 25.0;
        r.sharpness = 40.0;
        r.vignette = -18.0;
        r
    }

    pub fn kodak_portra() -> Self {
        let mut r = Self::default();
        r.preset_name = Some("Kodak Portra 400".to_string());
        r.wb_temperature = 5700.0; // Warm golden glow
        r.wb_tint = 4.0;
        r.contrast = -5.0;
        r.highlights = -12.0;
        r.shadows = 18.0;
        r.whites = -8.0;
        r.texture = -5.0; // Soft skin
        r.clarity = 6.0;
        r.vibrance = 10.0;
        r.saturation = -4.0;
        r.hsl_lum[1] = 12.0; // Brighten orange skin tones
        r.hsl_sat[1] = -5.0;
        r
    }

    pub fn cinematic_teal_orange() -> Self {
        let mut r = Self::default();
        r.preset_name = Some("Cinematic Teal & Orange".to_string());
        r.contrast = 20.0;
        r.shadows = -10.0;
        r.highlights = -15.0;
        r.clarity = 15.0;
        r.vibrance = 20.0;
        // DaVinci Resolve 3-way color wheel grading
        r.lift = [-0.05, 0.04, 0.12]; // Teal in shadows
        r.lift_luma = -0.05;
        r.gain = [0.14, 0.05, -0.08]; // Warm orange in highlights
        r.gain_luma = 0.02;
        r.hsl_hue[5] = -25.0; // Blue to Teal
        r.hsl_sat[5] = 30.0;
        r.hsl_hue[1] = -5.0;  // Orange warmth
        r.hsl_sat[1] = 25.0;
        r.vignette = -20.0;
        r
    }

    pub fn save_sidecar<P: AsRef<Path>>(&self, raw_path: P) -> io::Result<PathBuf> {
        let path = raw_path.as_ref();
        let sidecar_path = path.with_extension(format!(
            "{}.omastudio",
            path.extension().and_then(|s| s.to_str()).unwrap_or("raw")
        ));

        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        crate::security::atomic_write_secure(&sidecar_path, &json)?;
        Ok(sidecar_path)
    }

    pub fn load_sidecar<P: AsRef<Path>>(raw_path: P) -> Option<Self> {
        let path = raw_path.as_ref();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("raw");
        
        // Check primary .omastudio sidecar first
        let studio_sidecar = path.with_extension(format!("{}.omastudio", ext));
        if let Ok(data) = crate::security::read_secure_file(&studio_sidecar, 1024 * 1024) {
            if let Ok(recipe) = serde_json::from_slice(&data) {
                return Some(recipe);
            }
        }

        // Fallback to legacy .omaraw sidecar
        let legacy_sidecar = path.with_extension(format!("{}.omaraw", ext));
        if let Ok(data) = crate::security::read_secure_file(&legacy_sidecar, 1024 * 1024) {
            if let Ok(recipe) = serde_json::from_slice(&data) {
                return Some(recipe);
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: String,
    pub path: String,
    pub remote_path: Option<String>,
    pub thumbnail_path: String,
    pub metadata: RawMetadata,
    pub rating: u8,
    pub color_label: String,
    pub flag: String,
    pub recipe: Recipe,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Catalog {
    pub version: u32,
    pub items: HashMap<String, CatalogItem>,
}

impl Catalog {
    pub fn default_catalog_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let studio_path = PathBuf::from(&home)
            .join(".local/share/omastudio")
            .join("catalog.json");
        if studio_path.exists() {
            return studio_path;
        }
        let legacy_path = PathBuf::from(&home)
            .join(".local/share/omaraw")
            .join("catalog.json");
        if legacy_path.exists() {
            return legacy_path;
        }
        studio_path
    }

    pub fn load() -> Self {
        let path = Self::default_catalog_path();
        if let Ok(bytes) = crate::security::read_secure_file(&path, 16 * 1024 * 1024) {
            if let Ok(cat) = serde_json::from_slice::<Catalog>(&bytes) {
                return cat;
            }
        }
        Self {
            version: 1,
            items: HashMap::new(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::default_catalog_path();
        let data = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        crate::security::atomic_write_secure(&path, &data)
    }

    pub fn upsert(&mut self, item: CatalogItem) {
        self.items.insert(item.path.clone(), item);
    }

    pub fn get(&self, path: &str) -> Option<&CatalogItem> {
        self.items.get(path)
    }
}
