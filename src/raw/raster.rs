//! Non-RAW images: JPEG, PNG, WebP, TIFF (8/16-bit and float), HEIF/HEIC, AVIF,
//! JPEG XL, OpenEXR, Radiance HDR and PNM.
//!
//! Everything is returned the way the raster pipeline expects it: sRGB-encoded RGB.
//! An embedded ICC profile is converted to sRGB with Little CMS, and float images
//! (scene-linear EXR, HDR, float TIFF) are encoded with the sRGB curve. HEIF, AVIF
//! and JPEG XL are decoded by their reference tools (libheif, libavif, libjxl) into
//! a 16-bit PNG in a private temporary directory.

use crate::icc::{self, RenderingIntent};
use crate::raw::OutputColorSpace;
use crate::security::{run_bounded_command, secure_command_with_mem_limit};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Extensions opened as raster images.
pub const RASTER_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "webp", "tif", "tiff", "heic", "heif", "hif", "avif", "jxl", "exr", "hdr", "pnm",
    "ppm", "pgm", "pam",
];

/// Largest memory an external decoder may use.
const DECODER_MEMORY: u64 = 4 * 1024 * 1024 * 1024;

pub fn is_raster(path: &Path) -> bool {
    extension(path).is_some_and(|e| RASTER_EXTENSIONS.contains(&e.as_str()))
}

fn extension(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase())
}

/// Decodes a raster image to sRGB-encoded RGB (8-bit stays 8-bit when no conversion
/// is needed, everything else becomes 16-bit).
pub fn decode(path: &Path) -> Result<DynamicImage, String> {
    match extension(path).as_deref() {
        Some("heic" | "heif" | "hif") => via_png(path, &["heif-dec", "heif-convert"], |tool, src, dst| {
            let mut cmd = secure_command_with_mem_limit(tool, Some(DECODER_MEMORY));
            cmd.arg("--").arg(src).arg(dst);
            cmd
        }),
        Some("avif") => via_png(path, &["avifdec"], |tool, src, dst| {
            let mut cmd = secure_command_with_mem_limit(tool, Some(DECODER_MEMORY));
            cmd.arg("-d").arg("16").arg("--").arg(src).arg(dst);
            cmd
        }),
        Some("jxl") => via_png(path, &["djxl"], |tool, src, dst| {
            let mut cmd = secure_command_with_mem_limit(tool, Some(DECODER_MEMORY));
            cmd.arg("--").arg(src).arg(dst);
            cmd
        }),
        _ => decode_with_image(path),
    }
}

/// Runs the first available external decoder into a temporary PNG and reads it.
fn via_png(
    path: &Path,
    tools: &[&str],
    build: impl Fn(&str, &Path, &Path) -> std::process::Command,
) -> Result<DynamicImage, String> {
    let dir = TempDir::new()?;
    let out = dir.0.join("decoded.png");
    let mut last_err = String::new();
    for tool in tools {
        match run_bounded_command(build(tool, path, &out), Duration::from_secs(120)) {
            Ok((0, _, _)) if out.exists() => return decode_with_image(&out),
            Ok((code, _, stderr)) => {
                last_err = format!("{} failed ({}): {}", tool, code, String::from_utf8_lossy(&stderr).trim())
            }
            Err(e) => last_err = format!("could not run {} (is it installed?): {}", tool, e),
        }
    }
    Err(format!("Could not decode {}: {}", path.display(), last_err))
}

fn decode_with_image(path: &Path) -> Result<DynamicImage, String> {
    let reader = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("Failed to open image {}: {}", path.display(), e))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("Failed to read image {}: {}", path.display(), e))?;
    let icc = decoder.icc_profile().ok().flatten();
    let img = DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("Failed to decode image {}: {}", path.display(), e))?;
    Ok(to_srgb(img, icc.as_deref()))
}

/// sRGB-encodes float images and converts an embedded non-sRGB profile to sRGB.
pub fn to_srgb(img: DynamicImage, icc: Option<&[u8]>) -> DynamicImage {
    let is_float = matches!(img, DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_));
    if is_float {
        // Scene-linear (EXR, HDR, float TIFF) with Rec.709 primaries
        let rgb = img.to_rgb32f();
        let (w, h) = rgb.dimensions();
        let srgb = crate::pipeline::transfer::Transfer::srgb();
        let data: Vec<u16> = rgb
            .into_raw()
            .into_iter()
            .map(|v| (srgb.encode(v.clamp(0.0, 1.0)) * 65535.0).round() as u16)
            .collect();
        return image::ImageBuffer::from_raw(w, h, data).map(DynamicImage::ImageRgb16).unwrap_or(img);
    }
    let Some(bytes) = icc else { return img };
    let Ok(profile) = lcms2::Profile::new_icc(bytes) else { return img };
    let description = profile.info(lcms2::InfoType::Description, lcms2::Locale::none()).unwrap_or_default();
    if profile.color_space() != lcms2::ColorSpaceSignature::RgbData || description.to_lowercase().contains("srgb") {
        return img;
    }
    let mut rgb = img.to_rgb16();
    let srgb = icc::builtin_profile(OutputColorSpace::Srgb);
    if icc::convert_rgb16(&mut rgb, &profile, &srgb, RenderingIntent::RelativeColorimetric).is_err() {
        return img;
    }
    DynamicImage::ImageRgb16(rgb)
}

/// A private (0700) temporary directory, removed with its contents when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let dir = std::env::temp_dir().join(format!("omastudio-decode-{}-{}", std::process::id(), nanos));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| format!("Could not create a temporary directory: {}", e))?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_images_are_srgb_encoded() {
        let img = image::Rgb32FImage::from_pixel(2, 2, image::Rgb([0.214, 1.0, 4.0]));
        let out = to_srgb(DynamicImage::ImageRgb32F(img), None).to_rgb16();
        let p = out.get_pixel(0, 0);
        assert!((p[0] as i32 - 32768).abs() < 400, "{:?}", p);
        assert_eq!(p[1], 65535);
        assert_eq!(p[2], 65535);
    }

    #[test]
    fn wide_gamut_profiles_are_converted_to_srgb() {
        let icc = icc::builtin_profile(OutputColorSpace::ProPhotoRgb).icc().unwrap();
        let img = image::RgbImage::from_pixel(1, 1, image::Rgb([40, 160, 40]));
        let out = to_srgb(DynamicImage::ImageRgb8(img.clone()), Some(&icc)).to_rgb16();
        // ProPhoto green is far more saturated than sRGB can show: red drops to 0
        assert!(out.get_pixel(0, 0)[0] < 2000, "{:?}", out.get_pixel(0, 0));
        // An sRGB profile leaves the pixels alone
        let srgb = icc::builtin_profile(OutputColorSpace::Srgb).icc().unwrap();
        assert!(matches!(to_srgb(DynamicImage::ImageRgb8(img), Some(&srgb)), DynamicImage::ImageRgb8(_)));
    }

    #[test]
    fn extensions() {
        assert!(is_raster(Path::new("a/IMG_0001.HEIC")));
        assert!(is_raster(Path::new("x.jxl")));
        assert!(!is_raster(Path::new("x.raf")));
    }
}
