pub mod watermark;

use crate::icc::{self, ExportProfile, RenderingIntent};
use crate::pipeline::process_buffer_16_to_16_ex;
use crate::raw::{OutputColorSpace, RawImage};
use crate::recipe::Recipe;
use crate::security::{run_bounded_command, secure_command, secure_command_with_mem_limit};
use image::{DynamicImage, ImageBuffer, ImageEncoder, Rgb};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub format: String, // "jxl", "avif", "heic", "webp", "tiff", "png", "jpeg", "exr"
    pub quality: u32,   // 1 to 100 (100 = lossless where the format has it)
    /// Bits per channel; 0 picks the format's best. See [`effective_bit_depth`].
    pub bit_depth: u8,
    /// TIFF compression: "none", "lzw" or "deflate".
    pub tiff_compression: String,
    pub scale_percent: u32, // 25, 50, 75, 100
    pub max_dimension: Option<u32>, // e.g. 3840 for 4K, 2048 for social
    pub strip_gps: bool,
    pub preserve_exif: bool,
    pub icc_profile: Option<String>,
    /// How colours outside the destination profile's gamut are mapped.
    pub rendering_intent: RenderingIntent,
    pub artist_copyright: Option<String>,
    pub output_dir: String,
    pub watermark: Option<watermark::WatermarkOptions>,
    /// Extra tag inserted into the output name (`<stem>_<tag>_omastudio.<fmt>`).
    /// Batch export sets it to the source extension when two sources share a stem.
    pub name_tag: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: "jxl".to_string(),
            quality: 90,
            bit_depth: 0,
            tiff_compression: "deflate".to_string(),
            scale_percent: 100,
            max_dimension: None,
            strip_gps: true,
            preserve_exif: true,
            icc_profile: Some("sRGB".to_string()),
            rendering_intent: RenderingIntent::default(),
            artist_copyright: None,
            output_dir: "~/Pictures/OmaStudio_Exports".to_string(),
            watermark: None,
            name_tag: None,
        }
    }
}

pub fn export_photo<P: AsRef<Path>>(
    raw_path: P,
    recipe: &Recipe,
    options: &ExportOptions,
) -> Result<PathBuf, String> {
    let raw = RawImage::open(&raw_path)
        .map_err(|e| format!("Could not open RAW for export: {}", e))?;
    let recipe = &Recipe { baseline_exposure: raw.baseline_exposure(), ..recipe.clone() };

    // Render straight into the requested space when the decoder can (built-in spaces
    // from a RAW), otherwise into a built-in space lcms2 converts from: ProPhoto for
    // custom profiles so nothing is clipped first, sRGB for raster sources.
    let target = options.icc_profile.as_deref().map(ExportProfile::resolve).transpose()?;
    let render_space = match &target {
        Some(ExportProfile::Builtin(space)) => *space,
        Some(ExportProfile::Custom { .. } | ExportProfile::AcesCg) => OutputColorSpace::ProPhotoRgb,
        None => OutputColorSpace::Srgb,
    };
    let decoded_space = if raw.set_output_space(render_space) { render_space } else { OutputColorSpace::Srgb };

    // Demosaic at high quality with full 16-bit depth (48-bit RGB)
    let processed = raw.process_full_16(3)
        .map_err(|e| format!("Failed to demosaic full resolution 16-bit: {}", e))?;

    let (mut final_buf, _) = process_buffer_16_to_16_ex(
        processed.as_slice_u16(),
        processed.width,
        processed.height,
        processed.channels,
        recipe,
        processed.transfer,
    );
    let (width, height) = (processed.width, processed.height);
    // A 100 MP frame is 600 MB at 16 bits; free the decode before encoding starts
    drop(processed);

    let fmt = options.format.to_lowercase();
    let fmt = match fmt.as_str() {
        "jpg" => "jpeg".to_string(),
        "tif" => "tiff".to_string(),
        "heif" => "heic".to_string(),
        _ => fmt,
    };
    let depth = effective_bit_depth(&fmt, options.bit_depth);
    let source_profile = icc::builtin_profile(decoded_space);

    // OpenEXR is scene-linear float: its conversion runs at encode time, straight from
    // the decoded space, so nothing is quantised to a display curve in between.
    let exr_target = if fmt == "exr" {
        let target = target.as_ref().map_or(Ok(ExportProfile::Builtin(OutputColorSpace::Srgb)), |t| match t {
            ExportProfile::Builtin(space) => Ok(ExportProfile::Builtin(*space)),
            ExportProfile::AcesCg => Ok(ExportProfile::AcesCg),
            ExportProfile::Custom { .. } => t.linear().map(|_| ExportProfile::AcesCg),
        })?;
        Some(target.linear()?)
    } else {
        None
    };

    let icc_bytes = match (&target, &exr_target) {
        (_, Some(_)) | (None, _) => None,
        (Some(target), None) => {
            match target {
                ExportProfile::Builtin(space) if *space == decoded_space => {}
                ExportProfile::Builtin(space) => {
                    icc::convert_rgb16(&mut final_buf, &source_profile, &icc::builtin_profile(*space), options.rendering_intent)?
                }
                ExportProfile::AcesCg => {
                    icc::convert_rgb16(&mut final_buf, &source_profile, &icc::acescg_profile(), options.rendering_intent)?
                }
                ExportProfile::Custom { profile, .. } => {
                    icc::convert_rgb16(&mut final_buf, &source_profile, profile, options.rendering_intent)?
                }
            }
            Some(target.icc_bytes()?)
        }
    };
    let icc_bytes = icc_bytes.as_deref();

    // Convert to 16-bit ImageBuffer
    let img: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::from_raw(width, height, final_buf)
        .ok_or_else(|| "Failed to construct 16-bit ImageBuffer from processed data".to_string())?;
    let mut dyn_img = DynamicImage::ImageRgb16(img);

    // Apply geometric transformations (Rotation / Flip)
    let rot_norm = ((recipe.rotation.round() as i32) % 360 + 360) % 360;
    if (45..135).contains(&rot_norm) {
        dyn_img = dyn_img.rotate90();
    } else if (135..225).contains(&rot_norm) {
        dyn_img = dyn_img.rotate180();
    } else if (225..315).contains(&rot_norm) {
        dyn_img = dyn_img.rotate270();
    }

    if recipe.flip_h {
        dyn_img = dyn_img.fliph();
    }
    if recipe.flip_v {
        dyn_img = dyn_img.flipv();
    }

    // Apply Crop if active
    let (cw, ch) = (dyn_img.width(), dyn_img.height());
    if recipe.crop_w < 0.999 || recipe.crop_h < 0.999 || recipe.crop_x > 0.001 || recipe.crop_y > 0.001 {
        let cx = ((recipe.crop_x * cw as f32).round() as u32).min(cw.saturating_sub(1));
        let cy = ((recipe.crop_y * ch as f32).round() as u32).min(ch.saturating_sub(1));
        let crop_width = ((recipe.crop_w * cw as f32).round() as u32).min(cw - cx).max(1);
        let crop_height = ((recipe.crop_h * ch as f32).round() as u32).min(ch - cy).max(1);
        dyn_img = dyn_img.crop_imm(cx, cy, crop_width, crop_height);
    }

    // Apply scaling / resizing
    let cur_w = dyn_img.width();
    let cur_h = dyn_img.height();
    if let Some(max_dim) = options.max_dimension {
        if cur_w > max_dim || cur_h > max_dim {
            dyn_img = dyn_img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3);
        }
    } else if options.scale_percent < 100 && options.scale_percent > 0 {
        let new_w = (cur_w * options.scale_percent) / 100;
        let new_h = (cur_h * options.scale_percent) / 100;
        dyn_img = dyn_img.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3);
    }

    // Apply Watermark if enabled
    if let Some(ref wm) = options.watermark {
        let meta_opt = raw.get_metadata().ok();
        watermark::apply_watermark(&mut dyn_img, wm, meta_opt.as_ref());
    }

    // Resolve output directory
    let expanded_dir = if options.output_dir.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(&options.output_dir[2..])
    } else {
        PathBuf::from(&options.output_dir)
    };

    // User-chosen folder: may be a symlink or an external mount; permissions untouched
    crate::security::SecureDir::open_user_dir(&expanded_dir)
        .map_err(|e| format!("Failed to create export directory: {}", e))?;

    let stem = raw_path
        .as_ref()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("photo");

    let ext = match fmt.as_str() {
        "jpeg" => "jpg",
        "tiff" => "tif",
        other => other,
    };
    let out_filename = match options.name_tag.as_deref().map(sanitize_name_tag) {
        Some(tag) if !tag.is_empty() => format!("{}_{}_omastudio.{}", stem, tag, ext),
        _ => format!("{}_omastudio.{}", stem, ext),
    };
    let final_dest = expanded_dir.join(&out_filename);

    // Pixels at the chosen depth: 8-bit for 8, 16-bit for everything above (AVIF and
    // HEIF encoders reduce 16-bit PNG input to 10/12 bits themselves).
    let dyn_img = if depth <= 8 && fmt != "exr" {
        DynamicImage::ImageRgb8(dyn_img.to_rgb8())
    } else {
        dyn_img
    };
    let quality = options.quality.clamp(1, 100);

    match fmt.as_str() {
        "jxl" => {
            let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
            // cjxl carries the PNG's iCCP profile and bit depth into the JXL file;
            // distance 0 is mathematically lossless
            let distance = if quality >= 100 { "0".to_string() } else { ((100 - quality) as f32 / 10.0).to_string() };
            let mut cmd = secure_command("cjxl");
            // The ISOBMFF container lets exiftool add the camera's Exif box below; a bare
            // codestream cannot carry metadata
            cmd.arg("-d").arg(&distance).arg("-e").arg("7").arg("--container=1")
                .arg("--").arg(&tmp.0).arg(&final_dest);
            run_encoder(cmd, "cjxl", 120)?;
        }
        "avif" => {
            let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
            let mut cmd = secure_command("avifenc");
            cmd.arg("-s").arg("4").arg("-d").arg(depth.to_string());
            if quality >= 100 {
                cmd.arg("--lossless");
            } else {
                cmd.arg("-q").arg(quality.to_string());
            }
            cmd.arg("--").arg(&tmp.0).arg(&final_dest);
            // A missing avifenc falls through to ffmpeg, not abort the export
            if run_encoder(cmd, "avifenc", 120).is_err() {
                let mut f_cmd = secure_command("ffmpeg");
                let crf = (35 - (quality * 20 / 100)).to_string();
                f_cmd.arg("-y")
                    .arg("-i").arg(&tmp.0)
                    .arg("-c:v").arg("libsvtav1")
                    .arg("-crf").arg(&crf)
                    .arg("--")
                    .arg(&final_dest);
                let ff_ok = run_bounded_command(f_cmd, Duration::from_secs(60))
                    .map(|(code, _, _)| code == 0)
                    .unwrap_or(false);
                if !ff_ok || !final_dest.exists() {
                    return Err("AVIF export failed: neither avifenc nor ffmpeg (libsvtav1) succeeded".to_string());
                }
            }
        }
        "heic" => {
            let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
            // heif-enc (libheif) with x265; the PNG's ICC profile is carried over
            let mut cmd = secure_command("heif-enc");
            if quality >= 100 {
                cmd.arg("--lossless");
            } else {
                cmd.arg("-q").arg(quality.to_string());
            }
            cmd.arg("-b").arg(depth.to_string()).arg("-o").arg(&final_dest).arg("--").arg(&tmp.0);
            run_encoder(cmd, "heif-enc", 180)?;
        }
        "webp" => {
            // Lossy WebP comes from cwebp (libwebp-utils), which keeps the ICC profile.
            // Quality 100 uses the built-in lossless encoder. A missing cwebp is an error:
            // falling back to lossless would silently write files many times larger.
            if quality < 100 {
                let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
                let mut cmd = secure_command("cwebp");
                cmd.arg("-q").arg(quality.to_string()).arg("-m").arg("6").arg("-metadata").arg("icc")
                    .arg("-o").arg(&final_dest).arg("--").arg(&tmp.0);
                run_encoder(cmd, "cwebp", 120).map_err(|e| {
                    format!("{}. Install libwebp-utils, or export at quality 100 for lossless WebP.", e)
                })?;
            } else {
                let mut buf = Vec::new();
                let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
                set_icc(&mut encoder, icc_bytes)?;
                dyn_img.write_with_encoder(encoder)
                    .map_err(|e| format!("Failed to encode WebP: {}", e))?;
                crate::security::atomic_write_user(&final_dest, &buf)
                    .map_err(|e| format!("Failed to securely write WebP file: {}", e))?;
            }
        }
        "tiff" => {
            let buf = encode_tiff(&dyn_img, depth, &options.tiff_compression, icc_bytes)?;
            crate::security::atomic_write_user(&final_dest, &buf)
                .map_err(|e| format!("Failed to securely write TIFF file: {}", e))?;
        }
        "png" => {
            let buf = encode_png(&dyn_img, icc_bytes)?;
            crate::security::atomic_write_user(&final_dest, &buf)
                .map_err(|e| format!("Failed to securely write PNG file: {}", e))?;
        }
        "jpeg" => {
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality as u8);
                set_icc(&mut encoder, icc_bytes)?;
                dyn_img.write_with_encoder(encoder)
                    .map_err(|e| format!("Failed to encode JPEG: {}", e))?;
            }
            crate::security::atomic_write_user(&final_dest, &buf.into_inner())
                .map_err(|e| format!("Failed to securely write JPEG file: {}", e))?;
        }
        "exr" => {
            let (linear, chroma) = exr_target.as_ref().ok_or("OpenEXR target missing")?;
            let rgb16 = dyn_img.to_rgb16();
            let pixels = icc::convert_rgb16_to_f32(rgb16.as_raw(), &source_profile, linear, options.rendering_intent)?;
            let buf = encode_exr(rgb16.width(), rgb16.height(), &pixels, depth, *chroma)?;
            crate::security::atomic_write_user(&final_dest, &buf)
                .map_err(|e| format!("Failed to securely write OpenEXR file: {}", e))?;
        }
        _ => return Err(format!("Unsupported export format: {}", fmt)),
    }

    // Copy camera EXIF and GPS settings; the ICC profile is already in the file
    if options.preserve_exif || options.artist_copyright.is_some() {
        // Enforce 512 MiB RLIMIT_AS ceiling on ExifTool helper to defend against
        // unbounded pointer array allocations (e.g. unchecked APP11/JUMBF sequence numbers)
        let mut exif_cmd = secure_command_with_mem_limit("exiftool", Some(512 * 1024 * 1024));
        exif_cmd.arg("-overwrite_original");

        if options.preserve_exif {
            exif_cmd.arg("-TagsFromFile")
                .arg(raw_path.as_ref())
                .arg("-all:all")
                .arg("-unsafe")
                // Pixels are already rotated by the decoder; copying the source Orientation
                // tag would make viewers rotate the export a second time.
                .arg("-Orientation#=1");

            if options.strip_gps {
                exif_cmd.arg("-gps:all=");
            }
        }

        if let Some(ref artist) = options.artist_copyright
            && !artist.trim().is_empty()
        {
            exif_cmd.arg(format!("-Artist={}", artist))
                .arg(format!("-Copyright={}", artist));
        }

        exif_cmd.arg("--").arg(&final_dest);

        // Run exiftool with bounded timeout (safe failover if format doesn't support tags)
        let _ = run_bounded_command(exif_cmd, Duration::from_secs(20));
    }

    // Exports are ordinary files to share: rw-r--r-- whichever encoder wrote them
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&final_dest, fs::Permissions::from_mode(0o644));
    }

    Ok(final_dest)
}

/// Bits per channel a format is written with. 0 (or an unsupported request) picks
/// the format's best: 16 for PNG, TIFF and JPEG XL, 12 for AVIF, 10 for HEIF and
/// half float (16) for OpenEXR. JPEG and WebP are always 8-bit.
pub fn effective_bit_depth(format: &str, requested: u8) -> u8 {
    let (allowed, best): (&[u8], u8) = match format {
        "jpeg" | "jpg" | "webp" => (&[8], 8),
        "png" | "tiff" | "tif" | "jxl" => (&[8, 16], 16),
        "avif" => (&[8, 10, 12], 12),
        "heic" | "heif" => (&[8, 10, 12], 10),
        "exr" => (&[16, 32], 16),
        _ => (&[8], 8),
    };
    if allowed.contains(&requested) { requested } else { best }
}

/// A PNG handed to an external encoder; removed when dropped.
struct TempPng(PathBuf);

impl TempPng {
    fn write(dir: &Path, stem: &str, img: &DynamicImage, icc: Option<&[u8]>) -> Result<Self, String> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let path = dir.join(format!(".tmp_{}_{}_{}.png", stem, std::process::id(), nanos));
        let png = encode_png(img, icc)?;
        crate::security::atomic_write_user(&path, &png)
            .map_err(|e| format!("Failed to save temporary PNG: {}", e))?;
        Ok(Self(path))
    }
}

impl Drop for TempPng {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Runs an external encoder; a missing tool or a non-zero exit is an error.
fn run_encoder(cmd: std::process::Command, name: &str, timeout_secs: u64) -> Result<(), String> {
    let (code, _, stderr) = run_bounded_command(cmd, Duration::from_secs(timeout_secs))
        .map_err(|e| format!("Could not run {} (is it installed?): {}", name, e))?;
    if code != 0 {
        return Err(format!("{} failed: {}", name, String::from_utf8_lossy(&stderr).trim()));
    }
    Ok(())
}

/// Encodes an 8- or 16-bit TIFF with the given compression and the ICC profile.
fn encode_tiff(img: &DynamicImage, depth: u8, compression: &str, icc: Option<&[u8]>) -> Result<Vec<u8>, String> {
    use tiff::encoder::{colortype, Compression, DeflateLevel, Predictor, TiffEncoder};
    use tiff::tags::Tag;
    let err = |e: tiff::TiffError| format!("Failed to encode TIFF: {}", e);
    let (compression, predictor) = match compression {
        "none" => (Compression::Uncompressed, Predictor::None),
        "lzw" => (Compression::Lzw, Predictor::Horizontal),
        _ => (Compression::Deflate(DeflateLevel::Balanced), Predictor::Horizontal),
    };
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut buf).map_err(err)?
            .with_compression(compression)
            .with_predictor(predictor);
        let (w, h) = (img.width(), img.height());
        if depth <= 8 {
            let rgb = img.to_rgb8();
            let mut image = encoder.new_image::<colortype::RGB8>(w, h).map_err(err)?;
            if let Some(icc) = icc {
                image.encoder().write_tag(Tag::IccProfile, icc).map_err(err)?;
            }
            image.write_data(rgb.as_raw()).map_err(err)?;
        } else {
            let rgb = img.to_rgb16();
            let mut image = encoder.new_image::<colortype::RGB16>(w, h).map_err(err)?;
            if let Some(icc) = icc {
                image.encoder().write_tag(Tag::IccProfile, icc).map_err(err)?;
            }
            image.write_data(rgb.as_raw()).map_err(err)?;
        }
    }
    Ok(buf.into_inner())
}

/// Encodes scene-linear float RGB as OpenEXR (half or full float, ZIP compressed)
/// with the colour space's chromaticities.
fn encode_exr(
    width: u32,
    height: u32,
    pixels: &[f32],
    depth: u8,
    chroma: icc::Primaries,
) -> Result<Vec<u8>, String> {
    use exr::meta::attribute::Chromaticities;
    use exr::prelude::*;
    let (w, h) = (width as usize, height as usize);
    let v2 = |p: (f64, f64)| Vec2(p.0 as f32, p.1 as f32);
    let at = move |pos: Vec2<usize>| {
        let i = (pos.y() * w + pos.x()) * 3;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    };
    let mut attributes = LayerAttributes::named("rgb");
    attributes.software_name = Some(Text::from("OmaStudio"));
    let encoding = Encoding::SMALL_LOSSLESS;
    let mut buf = std::io::Cursor::new(Vec::new());
    let chromaticities = Chromaticities {
        red: v2(chroma.0[0]),
        green: v2(chroma.0[1]),
        blue: v2(chroma.0[2]),
        white: v2(chroma.1),
    };
    let result = if depth >= 32 {
        let layer = Layer::new((w, h), attributes, encoding, SpecificChannels::rgb(at));
        let mut image = Image::from_layer(layer);
        image.attributes.chromaticities = Some(chromaticities);
        image.write().to_buffered(&mut buf)
    } else {
        let half = move |pos: Vec2<usize>| {
            let (r, g, b) = at(pos);
            (f16::from_f32(r), f16::from_f32(g), f16::from_f32(b))
        };
        let layer = Layer::new((w, h), attributes, encoding, SpecificChannels::rgb(half));
        let mut image = Image::from_layer(layer);
        image.attributes.chromaticities = Some(chromaticities);
        image.write().to_buffered(&mut buf)
    };
    result.map_err(|e| format!("Failed to encode OpenEXR: {}", e))?;
    Ok(buf.into_inner())
}

/// Hands the export's ICC profile to an image encoder.
fn set_icc(encoder: &mut impl ImageEncoder, icc: Option<&[u8]>) -> Result<(), String> {
    match icc {
        Some(bytes) => encoder
            .set_icc_profile(bytes.to_vec())
            .map_err(|e| format!("Could not embed ICC profile: {}", e)),
        None => Ok(()),
    }
}

/// Encodes a PNG (16-bit when the image is) with the ICC profile in its iCCP chunk.
fn encode_png(img: &DynamicImage, icc: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut buf);
    set_icc(&mut encoder, icc)?;
    img.write_with_encoder(encoder)
        .map_err(|e| format!("Failed to encode 16-bit PNG: {}", e))?;
    Ok(buf)
}

/// Keeps only ASCII alphanumerics so a tag can never inject separators into the name.
fn sanitize_name_tag(tag: &str) -> String {
    tag.chars().filter(|c| c.is_ascii_alphanumeric()).take(24).collect::<String>().to_lowercase()
}

/// Name tags for a batch: sources sharing a file stem (DSC_0001.NEF + DSC_0001.JPG,
/// or same-named files from two folders) would otherwise overwrite each other's
/// export, so each of them is tagged with its extension, plus an index when even
/// the extension repeats.
pub fn batch_name_tags(paths: &[&str]) -> Vec<Option<String>> {
    use std::collections::HashMap;
    let key = |p: &str| -> (String, String) {
        let path = Path::new(p);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("photo").to_string();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        (stem, ext)
    };
    let mut stem_count: HashMap<String, usize> = HashMap::new();
    let mut stem_ext_count: HashMap<(String, String), usize> = HashMap::new();
    for p in paths {
        let (stem, ext) = key(p);
        *stem_count.entry(stem.clone()).or_default() += 1;
        *stem_ext_count.entry((stem, ext)).or_default() += 1;
    }
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    paths
        .iter()
        .map(|p| {
            let (stem, ext) = key(p);
            if stem_count[&stem] < 2 {
                return None;
            }
            let k = (stem, ext.clone());
            if stem_ext_count[&k] < 2 {
                return Some(ext);
            }
            let n = seen.entry(k).or_default();
            *n += 1;
            Some(format!("{}{}", ext, n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_depth_follows_format_limits() {
        assert_eq!(effective_bit_depth("jpeg", 16), 8);
        assert_eq!(effective_bit_depth("png", 0), 16);
        assert_eq!(effective_bit_depth("tiff", 8), 8);
        assert_eq!(effective_bit_depth("avif", 16), 12);
        assert_eq!(effective_bit_depth("heic", 0), 10);
        assert_eq!(effective_bit_depth("exr", 32), 32);
        assert_eq!(effective_bit_depth("exr", 8), 16);
    }

    fn gradient() -> DynamicImage {
        let img = ImageBuffer::from_fn(64, 32, |x, y| Rgb([(x * 1000) as u16, (y * 2000) as u16, 30000u16]));
        DynamicImage::ImageRgb16(img)
    }

    #[test]
    fn tiff_round_trips_16_bit_with_icc() {
        let icc = icc::builtin_profile(OutputColorSpace::AdobeRgb).icc().unwrap();
        for compression in ["none", "lzw", "deflate"] {
            let buf = encode_tiff(&gradient(), 16, compression, Some(&icc)).unwrap();
            let mut dec = tiff::decoder::Decoder::new(std::io::Cursor::new(&buf)).unwrap();
            assert_eq!(dec.dimensions().unwrap(), (64, 32));
            let icc_tag = dec.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap();
            assert_eq!(icc_tag, icc);
            match dec.read_image().unwrap() {
                tiff::decoder::DecodingResult::U16(px) => assert_eq!(px[3 * 5], 5000),
                _ => panic!("expected 16-bit samples"),
            }
        }
        let buf8 = encode_tiff(&gradient(), 8, "lzw", None).unwrap();
        let mut dec = tiff::decoder::Decoder::new(std::io::Cursor::new(&buf8)).unwrap();
        assert!(matches!(dec.read_image().unwrap(), tiff::decoder::DecodingResult::U8(_)));
    }

    #[test]
    fn exr_is_linear_with_chromaticities() {
        let src = icc::builtin_profile(OutputColorSpace::Srgb);
        let (linear, chroma) = ExportProfile::Builtin(OutputColorSpace::Srgb).linear().unwrap();
        // sRGB 50 % grey is about 21.4 % linear
        let px = icc::convert_rgb16_to_f32(&[32768, 32768, 32768], &src, &linear, RenderingIntent::RelativeColorimetric).unwrap();
        assert!((px[0] - 0.214).abs() < 0.01, "{}", px[0]);
        let buf = encode_exr(1, 1, &px, 32, chroma).unwrap();
        let meta = exr::meta::MetaData::read_from_buffered(std::io::Cursor::new(&buf), false).unwrap();
        let c = meta.headers[0].shared_attributes.chromaticities.unwrap();
        assert!((c.red.0 - 0.64).abs() < 1e-4);
    }

    #[test]
    fn acescg_resolves_without_a_file() {
        assert!(matches!(ExportProfile::resolve("ACEScg").unwrap(), ExportProfile::AcesCg));
        assert!(!ExportProfile::AcesCg.icc_bytes().unwrap().is_empty());
    }
}
