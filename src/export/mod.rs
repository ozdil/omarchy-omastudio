pub mod watermark;

use crate::gdrive::upload_export_to_gdrive;
use crate::pipeline::process_buffer_16_to_16;
use crate::raw::RawImage;
use crate::recipe::Recipe;
use crate::security::{run_bounded_command, secure_command};
use image::{DynamicImage, ImageBuffer, Rgb};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub format: String, // "jxl", "avif", "webp", "tiff", "png", "jpeg"
    pub quality: u32,   // 1 to 100
    pub scale_percent: u32, // 25, 50, 75, 100
    pub max_dimension: Option<u32>, // e.g. 3840 for 4K, 2048 for social
    pub strip_gps: bool,
    pub preserve_exif: bool,
    pub icc_profile: Option<String>,
    pub artist_copyright: Option<String>,
    pub output_dir: String,
    pub upload_to_gdrive: bool,
    pub gdrive_folder: Option<String>,
    pub watermark: Option<watermark::WatermarkOptions>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: "jxl".to_string(),
            quality: 90,
            scale_percent: 100,
            max_dimension: None,
            strip_gps: true,
            preserve_exif: true,
            icc_profile: Some("sRGB".to_string()),
            artist_copyright: None,
            output_dir: "~/Pictures/OmaStudio_Exports".to_string(),
            upload_to_gdrive: false,
            gdrive_folder: Some("Photos/Exports".to_string()),
            watermark: None,
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

    // Demosaic at high quality with full 16-bit depth (48-bit RGB)
    let processed = raw.process_full_16(3)
        .map_err(|e| format!("Failed to demosaic full resolution 16-bit: {}", e))?;

    let (final_buf, _) = process_buffer_16_to_16(
        processed.as_slice_u16(),
        processed.width,
        processed.height,
        processed.channels,
        recipe,
    );

    let width = processed.width;
    let height = processed.height;

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

    crate::security::ensure_secure_dir(&expanded_dir)
        .map_err(|e| format!("Failed to create export directory: {}", e))?;

    let stem = raw_path
        .as_ref()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("photo");

    let fmt = options.format.to_lowercase();
    let out_filename = format!("{}_omastudio.{}", stem, fmt);
    let final_dest = expanded_dir.join(&out_filename);

    match fmt.as_str() {
        "jxl" => {
            let pid = std::process::id();
            let rand_suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let tmp_png = expanded_dir.join(format!(".tmp_{}_{}_{}.png", stem, pid, rand_suffix));
            dyn_img.save(&tmp_png)
                .map_err(|e| format!("Failed to save temporary PNG for JXL: {}", e))?;

            struct TmpPngGuard(PathBuf);
            impl Drop for TmpPngGuard {
                fn drop(&mut self) {
                    let _ = fs::remove_file(&self.0);
                }
            }
            let _guard = TmpPngGuard(tmp_png.clone());

            let mut cmd = secure_command("cjxl");
            let distance = ((100 - options.quality.clamp(1, 100)) as f32 / 10.0).to_string();
            cmd.arg("-d")
                .arg(&distance)
                .arg("-e")
                .arg("7")
                .arg("--")
                .arg(&tmp_png)
                .arg(&final_dest);

            let (code, _, stderr) = run_bounded_command(cmd, Duration::from_secs(60))
                .map_err(|e| format!("Failed to execute cjxl: {}", e))?;

            if code != 0 {
                return Err(format!("cjxl failed: {}", String::from_utf8_lossy(&stderr)));
            }
        }
        "avif" => {
            let pid = std::process::id();
            let rand_suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let tmp_png = expanded_dir.join(format!(".tmp_{}_{}_{}.png", stem, pid, rand_suffix));
            dyn_img.save(&tmp_png)
                .map_err(|e| format!("Failed to save temporary PNG for AVIF: {}", e))?;

            struct TmpPngGuard(PathBuf);
            impl Drop for TmpPngGuard {
                fn drop(&mut self) {
                    let _ = fs::remove_file(&self.0);
                }
            }
            let _guard = TmpPngGuard(tmp_png.clone());

            let mut cmd = secure_command("avifenc");
            let speed = "4";
            let cq = 63 - (options.quality * 63 / 100);
            cmd.arg("-s")
                .arg(speed)
                .arg("-a")
                .arg(format!("end-usage=q:cq-level={}", cq))
                .arg("--")
                .arg(&tmp_png)
                .arg(&final_dest);

            let (code, _, _) = run_bounded_command(cmd, Duration::from_secs(60))
                .map_err(|e| format!("Failed to execute avifenc: {}", e))?;

            if code != 0 {
                let mut f_cmd = secure_command("ffmpeg");
                let crf = (35 - (options.quality * 20 / 100)).to_string();
                f_cmd.arg("-y")
                    .arg("-i").arg(&tmp_png)
                    .arg("-c:v").arg("libsvtav1")
                    .arg("-crf").arg(&crf)
                    .arg("--")
                    .arg(&final_dest);
                let _ = run_bounded_command(f_cmd, Duration::from_secs(30));
            }
        }
        "webp" => {
            let mut buf = std::io::Cursor::new(Vec::new());
            let dyn_rgb8 = DynamicImage::ImageRgb8(dyn_img.to_rgb8());
            dyn_rgb8.write_to(&mut buf, image::ImageFormat::WebP)
                .map_err(|e| format!("Failed to encode WebP: {}", e))?;
            crate::security::atomic_write_secure(&final_dest, &buf.into_inner())
                .map_err(|e| format!("Failed to securely write WebP file: {}", e))?;
        }
        "tiff" | "tif" => {
            let mut buf = std::io::Cursor::new(Vec::new());
            dyn_img.write_to(&mut buf, image::ImageFormat::Tiff)
                .map_err(|e| format!("Failed to encode 16-bit TIFF: {}", e))?;
            crate::security::atomic_write_secure(&final_dest, &buf.into_inner())
                .map_err(|e| format!("Failed to securely write TIFF file: {}", e))?;
        }
        "png" => {
            let mut buf = std::io::Cursor::new(Vec::new());
            dyn_img.write_to(&mut buf, image::ImageFormat::Png)
                .map_err(|e| format!("Failed to encode 16-bit PNG: {}", e))?;
            crate::security::atomic_write_secure(&final_dest, &buf.into_inner())
                .map_err(|e| format!("Failed to securely write PNG file: {}", e))?;
        }
        "jpeg" | "jpg" => {
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut buf,
                    options.quality.clamp(1, 100) as u8,
                );
                let dyn_rgb8 = DynamicImage::ImageRgb8(dyn_img.to_rgb8());
                dyn_rgb8.write_with_encoder(encoder)
                    .map_err(|e| format!("Failed to encode JPEG: {}", e))?;
            }
            crate::security::atomic_write_secure(&final_dest, &buf.into_inner())
                .map_err(|e| format!("Failed to securely write JPEG file: {}", e))?;
        }
        _ => return Err(format!("Unsupported export format: {}", fmt)),
    }

    // Embed camera EXIF, GPS settings, and ICC color profile
    if options.preserve_exif || options.icc_profile.is_some() || options.artist_copyright.is_some() {
        let mut exif_cmd = secure_command("exiftool");
        exif_cmd.arg("-overwrite_original");

        if options.preserve_exif {
            exif_cmd.arg("-TagsFromFile")
                .arg(raw_path.as_ref())
                .arg("-all:all")
                .arg("-unsafe");

            if options.strip_gps {
                exif_cmd.arg("-gps:all=");
            }
        }

        if let Some(ref artist) = options.artist_copyright {
            if !artist.trim().is_empty() {
                exif_cmd.arg(format!("-Artist={}", artist))
                    .arg(format!("-Copyright={}", artist));
            }
        }

        if let Some(ref prof_name) = options.icc_profile {
            if let Some(icc_path) = crate::icc::resolve_icc_path(prof_name) {
                exif_cmd.arg(format!("-icc_profile<={}", icc_path.display()));
            }
        }

        exif_cmd.arg("--").arg(&final_dest);

        // Run exiftool with bounded timeout (safe failover if format doesn't support tags)
        let _ = run_bounded_command(exif_cmd, Duration::from_secs(20));
    }

    if options.upload_to_gdrive {
        let remote_folder = options.gdrive_folder.as_deref().unwrap_or("Photos/Exports");
        upload_export_to_gdrive(&final_dest, remote_folder)?;
    }

    Ok(final_dest)
}
