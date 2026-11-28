pub mod watermark;
pub mod exif;
pub mod hdr;
pub mod tiff_writer;

pub use hdr::HdrMode;

use crate::icc::{self, ExportProfile, RenderingIntent};
use crate::pipeline::{render_16, render_16_to_f32, render_f32_to_16, render_f32_to_f32};
use crate::raw::{OutputColorSpace, RawImage};
use crate::recipe::Recipe;
use crate::security::{run_bounded_command, secure_command, secure_command_with_mem_limit};
use image::{DynamicImage, ImageBuffer, ImageEncoder, Rgb};
use rayon::prelude::*;
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
    /// Dynamic range output mode: SDR (default), PQ10/HLG10 CICP HDR, or ISO 21496-1 Gain Map.
    pub hdr_mode: HdrMode,
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
            hdr_mode: HdrMode::Sdr,
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
    let mut prof = crate::pipeline::Profile::start();

    let fmt = options.format.to_lowercase();
    let fmt = match fmt.as_str() {
        "jpg" => "jpeg".to_string(),
        "tif" => "tiff".to_string(),
        "heif" => "heic".to_string(),
        _ => fmt,
    };
    hdr::validate_hdr_mode(&fmt, options.hdr_mode)?;
    let depth = hdr::effective_hdr_bit_depth(&fmt, effective_bit_depth(&fmt, options.bit_depth), options.hdr_mode);

    let raw = RawImage::open(&raw_path)
        .map_err(|e| format!("Could not open RAW for export: {}", e))?;
    prof.mark("open");

    // Render straight into the requested space when the decoder can (built-in spaces
    // from a RAW), otherwise into a built-in space lcms2 converts from: ProPhoto for
    // custom profiles so nothing is clipped first, sRGB for raster sources.
    let target = options.icc_profile.as_deref().map(ExportProfile::resolve).transpose()?;
    let render_space = match options.hdr_mode {
        HdrMode::Pq10 | HdrMode::Hlg10 => OutputColorSpace::Rec2020,
        HdrMode::GainMap { .. } => OutputColorSpace::Srgb,
        HdrMode::Sdr => match &target {
            Some(ExportProfile::Builtin(space)) => *space,
            Some(ExportProfile::Custom { .. } | ExportProfile::AcesCg) => OutputColorSpace::ProPhotoRgb,
            None => OutputColorSpace::Srgb,
        },
    };
    let decoded_space = if raw.set_output_space(render_space) { render_space } else { OutputColorSpace::Srgb };

    // Demosaic at high quality with full 16-bit depth (or unclipped f32 for HDR raster sources)
    let processed = raw.process_full_16(3)
        .map_err(|e| format!("Failed to demosaic full resolution 16-bit: {}", e))?;
    prof.mark("decode");
    // Measured on this development, not a second half-size one: a photo already
    // measured (by its preview in the app) keeps that value
    let recipe = &Recipe { baseline_exposure: raw.baseline_exposure_from(&processed), ..recipe.clone() };
    prof.mark("baseline");

    // Resolve output directory early so both HDR/EXR and SDR paths share destination setup
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
    let quality = options.quality.clamp(1, 100);
    let artist = options.artist_copyright.as_deref().filter(|a| !a.trim().is_empty());

    let mut exif_written = false;
    let dyn_img_size: (u32, u32);

    // Unclipped float export path for OpenEXR, CICP PQ10/HLG10 HDR, and ISO 21496-1 Gain Map.
    if fmt == "exr" || options.hdr_mode.is_hdr() {
        let transfer = processed.transfer;
        let (width, height) = (processed.width, processed.height);
        let f32_display = if let Some(f32_slice) = processed.as_slice_f32() {
            render_f32_to_f32(f32_slice, width, height, processed.channels, recipe, transfer)
        } else {
            render_16_to_f32(processed.as_slice_u16(), width, height, processed.channels, recipe, transfer)
        };
        prof.mark("render");
        drop(processed);
        prof.mark("free_decode");

        let (f32_display, out_w, out_h) = hdr::apply_geometry_f32(f32_display, width, height, recipe);
        prof.mark("geometry");
        let (f32_display, out_w, out_h) = hdr::resize_f32(
            f32_display,
            out_w,
            out_h,
            options.max_dimension,
            options.scale_percent,
        );
        prof.mark("resize");
        dyn_img_size = (out_w, out_h);

        if fmt == "exr" {
            let chroma = match target.as_ref() {
                None => icc::primaries(OutputColorSpace::Srgb),
                Some(ExportProfile::Builtin(space)) => icc::primaries(*space),
                Some(ExportProfile::AcesCg) => icc::ACES_AP1,
                Some(custom @ ExportProfile::Custom { .. }) => custom.linear()?.1,
            };
            let linear_px = hdr::linearize_and_convert_f32(&f32_display, transfer, decoded_space, chroma);
            let buf = encode_exr(out_w, out_h, &linear_px, depth, chroma)?;
            crate::security::atomic_write_user(&final_dest, &buf)
                .map_err(|e| format!("Failed to securely write OpenEXR file: {}", e))?;
        } else if matches!(options.hdr_mode, HdrMode::Pq10 | HdrMode::Hlg10) {
            let linear_rec2020 = hdr::linearize_and_convert_f32(
                &f32_display,
                transfer,
                decoded_space,
                icc::primaries(OutputColorSpace::Rec2020),
            );
            let hdr_16 = hdr::encode_hdr_16_from_linear_rec2020(&linear_rec2020, options.hdr_mode);
            let hdr_buf: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::from_raw(out_w, out_h, hdr_16)
                .ok_or_else(|| "Failed to construct 16-bit HDR ImageBuffer".to_string())?;
            let mut hdr_img = DynamicImage::ImageRgb16(hdr_buf);
            if let Some(ref wm) = options.watermark {
                let meta_opt = raw.get_metadata().ok();
                watermark::apply_watermark(&mut hdr_img, wm, meta_opt.as_ref());
            }
            // Do not embed an sRGB iCCP chunk in PQ/HLG intermediate PNGs: CICP defines the space.
            let tmp = TempPng::write(&expanded_dir, stem, &hdr_img, None)?;
            let stage = StagedOutput::new(&final_dest, ext);
            match fmt.as_str() {
                "avif" => {
                    let mut cmd = secure_command("avifenc");
                    for arg in hdr::build_avifenc_args(&tmp.0, stage.stage_path(), depth, quality, options.hdr_mode) {
                        cmd.arg(arg);
                    }
                    if run_encoder(cmd, "avifenc", 120).is_err() {
                        let _ = fs::remove_file(stage.stage_path());
                        let mut f_cmd = secure_command("ffmpeg");
                        for arg in hdr::build_ffmpeg_avif_args(&tmp.0, stage.stage_path(), depth, quality, options.hdr_mode) {
                            f_cmd.arg(arg);
                        }
                        let ff_ok = run_bounded_command(f_cmd, Duration::from_secs(60))
                            .map(|(code, _, _)| code == 0)
                            .unwrap_or(false);
                        if !ff_ok || !stage.stage_path().exists() {
                            return Err("HDR AVIF export failed: neither avifenc nor ffmpeg (libsvtav1) succeeded".to_string());
                        }
                    }
                    stage.commit()?;
                }
                "jxl" => {
                    let mut cmd = secure_command("cjxl");
                    for arg in hdr::build_cjxl_args(&tmp.0, stage.stage_path(), quality, options.hdr_mode) {
                        cmd.arg(arg);
                    }
                    run_encoder(cmd, "cjxl", 120)?;
                    stage.commit()?;
                }
                "heic" => {
                    let mut cmd = secure_command("heif-enc");
                    for arg in hdr::build_heif_enc_args(&tmp.0, stage.stage_path(), depth, quality, options.hdr_mode) {
                        cmd.arg(arg);
                    }
                    run_encoder(cmd, "heif-enc", 180)?;
                    stage.commit()?;
                }
                other => return Err(format!("Unsupported HDR export format: {}", other)),
            }
        } else if let HdrMode::GainMap { headroom_stops } = options.hdr_mode {
            // SDR base image in sRGB (same dimensions & crop/resize as HDR alternate)
            let mut sdr_u16: Vec<u16> = f32_display
                .par_iter()
                .map(|&v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
                .collect();
            if decoded_space != OutputColorSpace::Srgb {
                icc::convert_rgb16(
                    &mut sdr_u16,
                    &icc::builtin_profile(decoded_space),
                    &icc::builtin_profile(OutputColorSpace::Srgb),
                    options.rendering_intent,
                )?;
            }
            let sdr_buf: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::from_raw(out_w, out_h, sdr_u16)
                .ok_or_else(|| "Failed to construct 16-bit SDR base ImageBuffer".to_string())?;
            let mut sdr_img = DynamicImage::ImageRgb16(sdr_buf);
            if let Some(ref wm) = options.watermark {
                let meta_opt = raw.get_metadata().ok();
                watermark::apply_watermark(&mut sdr_img, wm, meta_opt.as_ref());
            }

            // HDR alternate image in linear BT.2020 -> PQ
            let linear_rec2020 = hdr::linearize_and_convert_f32(
                &f32_display,
                transfer,
                decoded_space,
                icc::primaries(OutputColorSpace::Rec2020),
            );
            let stage = StagedOutput::new(&final_dest, ext);
            match fmt.as_str() {
                "avif" => {
                    let hdr_16 = hdr::encode_hdr_16_from_linear_rec2020(&linear_rec2020, options.hdr_mode);
                    let hdr_buf: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::from_raw(out_w, out_h, hdr_16)
                        .ok_or_else(|| "Failed to construct 16-bit HDR alternate ImageBuffer".to_string())?;
                    let mut hdr_img = DynamicImage::ImageRgb16(hdr_buf);
                    if let Some(ref wm) = options.watermark {
                        let meta_opt = raw.get_metadata().ok();
                        watermark::apply_watermark(&mut hdr_img, wm, meta_opt.as_ref());
                    }
                    let tmp_sdr = TempPng::write(&expanded_dir, &format!("{stem}_sdr"), &sdr_img, None)?;
                    let tmp_hdr = TempPng::write(&expanded_dir, &format!("{stem}_hdr"), &hdr_img, None)?;
                    let mut cmd = secure_command("avifgainmaputil");
                    for arg in hdr::build_avifgainmaputil_args(
                        &tmp_sdr.0,
                        &tmp_hdr.0,
                        stage.stage_path(),
                        depth,
                        quality,
                    ) {
                        cmd.arg(arg);
                    }
                    run_encoder(cmd, "avifgainmaputil", 180).map_err(|e| {
                        format!(
                            "{}. Install libavif (with avifgainmaputil) for ISO 21496-1 AVIF Gain Map export.",
                            e
                        )
                    })?;
                    stage.commit()?;
                }
                "jpeg" => {
                    let sdr_rgb8 = DynamicImage::ImageRgb8(sdr_img.to_rgb8());
                    let srgb_icc = icc::builtin_profile(OutputColorSpace::Srgb).icc().ok();
                    let own_exif = if options.preserve_exif {
                        exif::from_raf(raw_path.as_ref())
                    } else {
                        artist.map(|_| exif::Exif::default())
                    }
                    .map(|e| {
                        e.adjusted(&exif::Adjust {
                            strip_gps: options.strip_gps,
                            srgb: true,
                            artist,
                            width: out_w,
                            height: out_h,
                        })
                    });
                    let mut jpg_cursor = std::io::Cursor::new(Vec::new());
                    {
                        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg_cursor, quality as u8);
                        set_icc(&mut encoder, srgb_icc.as_deref())?;
                        sdr_rgb8.write_with_encoder(encoder)
                            .map_err(|e| format!("Failed to encode SDR base JPEG: {}", e))?;
                    }
                    let mut sdr_jpg_bytes = jpg_cursor.into_inner();
                    if let Some(seg) = own_exif.as_ref().and_then(exif::Exif::app1) {
                        sdr_jpg_bytes = exif::insert_app1(sdr_jpg_bytes, &seg);
                        exif_written = true;
                    }
                    let hdr_raw_bytes = hdr::pack_rgba1010102_le_from_linear_rec2020(&linear_rec2020, headroom_stops);
                    let tmp_sdr = TempBytes::write(&expanded_dir, &format!("{stem}_sdr"), "jpg", &sdr_jpg_bytes)?;
                    let tmp_hdr = TempBytes::write(&expanded_dir, &format!("{stem}_hdr"), "raw", &hdr_raw_bytes)?;
                    let mut cmd = secure_command("ultrahdr_app");
                    for arg in hdr::build_ultrahdr_args(
                        &tmp_hdr.0,
                        &tmp_sdr.0,
                        stage.stage_path(),
                        out_w,
                        out_h,
                        quality,
                    ) {
                        cmd.arg(arg);
                    }
                    run_encoder(cmd, "ultrahdr_app", 180).map_err(|e| {
                        format!(
                            "{}. Install libultrahdr (ultrahdr_app) for ISO 21496-1 Ultra HDR JPEG export.",
                            e
                        )
                    })?;
                    stage.commit()?;
                }
                other => return Err(format!("Unsupported Gain Map export format: {}", other)),
            }
        }
    } else {
        // Standard SDR export path (16-bit RAW or unclipped float raster rendered to 16-bit SDR).
        let mut final_buf = if let Some(f32_slice) = processed.as_slice_f32() {
            render_f32_to_16(
                f32_slice,
                processed.width,
                processed.height,
                processed.channels,
                recipe,
                processed.transfer,
            )
        } else {
            render_16(
                processed.as_slice_u16(),
                processed.width,
                processed.height,
                processed.channels,
                recipe,
                processed.transfer,
            )
        };
        let (width, height) = (processed.width, processed.height);
        prof.mark("render");
        // A 100 MP frame is 600 MB at 16 bits; free the decode before encoding starts
        drop(processed);
        prof.mark("free_decode");

        let source_profile = icc::builtin_profile(decoded_space);
        let icc_bytes = match &target {
            None => None,
            Some(target) => {
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
        prof.mark("icc");

        // Convert to 16-bit ImageBuffer
        let img: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::from_raw(width, height, final_buf)
            .ok_or_else(|| "Failed to construct 16-bit ImageBuffer from processed data".to_string())?;
        let mut dyn_img = DynamicImage::ImageRgb16(img);

        dyn_img = apply_geometry(dyn_img, recipe);
        prof.mark("geometry");

        // Apply scaling / resizing
        let cur_w = dyn_img.width();
        let cur_h = dyn_img.height();
        if let Some(max_dim) = options.max_dimension {
            if cur_w > max_dim || cur_h > max_dim {
                dyn_img = dyn_img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3);
            }
        } else if options.scale_percent < 100 && options.scale_percent > 0 {
            let new_w = ((cur_w as u64 * options.scale_percent as u64) / 100).max(1) as u32;
            let new_h = ((cur_h as u64 * options.scale_percent as u64) / 100).max(1) as u32;
            dyn_img = dyn_img.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3);
        }

        prof.mark("resize");
        // Apply Watermark if enabled
        if let Some(ref wm) = options.watermark {
            let meta_opt = raw.get_metadata().ok();
            watermark::apply_watermark(&mut dyn_img, wm, meta_opt.as_ref());
        }

        // Pixels at the chosen depth: 8-bit for 8, 16-bit for everything above (AVIF and
        // HEIF encoders reduce 16-bit PNG input to 10/12 bits themselves).
        let dyn_img = if depth <= 8 {
            DynamicImage::ImageRgb8(dyn_img.to_rgb8())
        } else {
            dyn_img
        };
        dyn_img_size = (dyn_img.width(), dyn_img.height());
        // Camera metadata the TIFF and JPEG writers embed themselves (Fujifilm RAF sources);
        // everything else is copied by exiftool afterwards
        let srgb = options.icc_profile.as_deref().is_none_or(|p| p.eq_ignore_ascii_case("srgb"));
        let own_exif = if options.preserve_exif {
            exif::from_raf(raw_path.as_ref())
        } else {
            artist.map(|_| exif::Exif::default())
        }
        .map(|e| {
            e.adjusted(&exif::Adjust {
                strip_gps: options.strip_gps,
                srgb,
                artist,
                width: dyn_img_size.0,
                height: dyn_img_size.1,
            })
        });
        prof.mark("watermark+depth");

        match fmt.as_str() {
            "jxl" => {
                let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
                let stage = StagedOutput::new(&final_dest, ext);
                let mut cmd = secure_command("cjxl");
                for arg in hdr::build_cjxl_args(&tmp.0, stage.stage_path(), quality, HdrMode::Sdr) {
                    cmd.arg(arg);
                }
                run_encoder(cmd, "cjxl", 120)?;
                stage.commit()?;
            }
            "avif" => {
                let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
                let stage = StagedOutput::new(&final_dest, ext);
                let mut cmd = secure_command("avifenc");
                for arg in hdr::build_avifenc_args(&tmp.0, stage.stage_path(), depth, quality, HdrMode::Sdr) {
                    cmd.arg(arg);
                }
                // A missing avifenc falls through to ffmpeg, not abort the export
                if run_encoder(cmd, "avifenc", 120).is_err() {
                    let _ = fs::remove_file(stage.stage_path());
                    let mut f_cmd = secure_command("ffmpeg");
                    for arg in hdr::build_ffmpeg_avif_args(&tmp.0, stage.stage_path(), depth, quality, HdrMode::Sdr) {
                        f_cmd.arg(arg);
                    }
                    let ff_ok = run_bounded_command(f_cmd, Duration::from_secs(60))
                        .map(|(code, _, _)| code == 0)
                        .unwrap_or(false);
                    if !ff_ok || !stage.stage_path().exists() {
                        return Err("AVIF export failed: neither avifenc nor ffmpeg (libsvtav1) succeeded".to_string());
                    }
                }
                stage.commit()?;
            }
            "heic" => {
                let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
                let stage = StagedOutput::new(&final_dest, ext);
                let mut cmd = secure_command("heif-enc");
                for arg in hdr::build_heif_enc_args(&tmp.0, stage.stage_path(), depth, quality, HdrMode::Sdr) {
                    cmd.arg(arg);
                }
                run_encoder(cmd, "heif-enc", 180)?;
                stage.commit()?;
            }
            "webp" => {
                // Lossy WebP comes from cwebp (libwebp-utils), which keeps the ICC profile.
                // Quality 100 uses the built-in lossless encoder. A missing cwebp is an error:
                // falling back to lossless would silently write files many times larger.
                if quality < 100 {
                    let tmp = TempPng::write(&expanded_dir, stem, &dyn_img, icc_bytes)?;
                    let stage = StagedOutput::new(&final_dest, ext);
                    let mut cmd = secure_command("cwebp");
                    cmd.arg("-q").arg(quality.to_string()).arg("-m").arg("6").arg("-metadata").arg("icc")
                        .arg("-o").arg(stage.stage_path()).arg("--").arg(&tmp.0);
                    run_encoder(cmd, "cwebp", 120).map_err(|e| {
                        format!("{}. Install libwebp-utils, or export at quality 100 for lossless WebP.", e)
                    })?;
                    stage.commit()?;
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
                let write_err = |e: std::io::Error| format!("Failed to securely write TIFF file: {}", e);
                match tiff_codec(&options.tiff_compression) {
                    Some(codec) => {
                        let rgb16;
                        let rgb8;
                        let samples = match (&dyn_img, depth <= 8) {
                            (DynamicImage::ImageRgb8(img), true) => tiff_writer::Samples::U8(img.as_raw()),
                            (DynamicImage::ImageRgb16(img), false) => tiff_writer::Samples::U16(img.as_raw()),
                            (img, true) => {
                                rgb8 = img.to_rgb8();
                                tiff_writer::Samples::U8(rgb8.as_raw())
                            }
                            (img, false) => {
                                rgb16 = img.to_rgb16();
                                tiff_writer::Samples::U16(rgb16.as_raw())
                            }
                        };
                        let parts = tiff_writer::build(dyn_img.width(), dyn_img.height(), samples, codec, icc_bytes, own_exif.as_ref())?;
                        exif_written = own_exif.is_some();
                        prof.mark("encode");
                        crate::security::atomic_write_user_parts(&final_dest, &parts.slices()).map_err(write_err)?;
                    }
                    None => {
                        let buf = encode_tiff(&dyn_img, depth, &options.tiff_compression, icc_bytes)?;
                        prof.mark("encode");
                        crate::security::atomic_write_user(&final_dest, &buf).map_err(write_err)?;
                    }
                }
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
                let mut buf = buf.into_inner();
                if let Some(seg) = own_exif.as_ref().and_then(exif::Exif::app1) {
                    buf = exif::insert_app1(buf, &seg);
                    exif_written = true;
                }
                prof.mark("encode");
                crate::security::atomic_write_user(&final_dest, &buf)
                    .map_err(|e| format!("Failed to securely write JPEG file: {}", e))?;
            }
            _ => return Err(format!("Unsupported export format: {}", fmt)),
        }
    }

    prof.mark("write");
    // Copy camera EXIF and GPS settings; the ICC profile / CICP is already in the file
    if !exif_written && (options.preserve_exif || options.artist_copyright.is_some()) {
        // Enforce 512 MiB RLIMIT_AS ceiling on ExifTool helper to defend against
        // unbounded pointer array allocations (e.g. unchecked APP11/JUMBF sequence numbers)
        let mut exif_cmd = secure_command_with_mem_limit("exiftool", Some(512 * 1024 * 1024));
        exif_cmd.arg("-overwrite_original");

        if options.preserve_exif {
            exif_cmd.arg("-TagsFromFile")
                .arg(safe_exiftool_path(raw_path.as_ref()))
                .arg("-all:all")
                .arg("-unsafe")
                // Pixels are already rotated by the decoder; copying the source Orientation
                // tag would make viewers rotate the export a second time.
                .arg("-Orientation#=1")
                // The export carries its own ICC profile or CICP; a profile copied from the
                // camera file would replace it
                .arg("--ICC_Profile:all");
            // EXIF can only name sRGB; PQ/HLG or wide-gamut profiles are "uncalibrated" there
            let srgb = match options.hdr_mode {
                HdrMode::Pq10 | HdrMode::Hlg10 => false,
                HdrMode::GainMap { .. } => true,
                HdrMode::Sdr => options.icc_profile.as_deref().is_none_or(|p| p.eq_ignore_ascii_case("srgb")),
            };
            exif_cmd.arg(if srgb { "-ExifIFD:ColorSpace#=1" } else { "-ExifIFD:ColorSpace#=65535" });

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

        exif_cmd.arg("--").arg(safe_exiftool_path(&final_dest));

        // Run exiftool with bounded timeout; surface failures as stderr warnings without
        // aborting an otherwise valid export.
        match run_bounded_command(exif_cmd, Duration::from_secs(20)) {
            Ok((0, _, _)) => {}
            Ok((code, _, stderr)) => {
                eprintln!(
                    "Warning: exiftool exited with status {} for {}: {}",
                    code,
                    final_dest.display(),
                    String::from_utf8_lossy(&stderr).trim()
                );
            }
            Err(e) => {
                eprintln!("Warning: exiftool failed for {}: {}", final_dest.display(), e);
            }
        }
        prof.mark("exiftool");
    }

    // Exports are ordinary files to share: rw-r--r-- whichever encoder wrote them
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&final_dest, fs::Permissions::from_mode(0o644));
    }
    prof.finish_as(&format!("export {fmt} {}x{}", dyn_img_size.0, dyn_img_size.1));

    Ok(final_dest)
}

/// Crop, rotation and flips of an export, as the viewport shows them.
///
/// The viewport draws the crop rectangle on the photo before it is rotated, so the
/// crop is cut first. The rotation is any angle: whole quarter turns are exact, and
/// the remaining angle (straightening, at most 45°) is resampled and cropped to the
/// largest rectangle of the same shape, so no empty corners are exported.
pub fn apply_geometry(mut img: DynamicImage, recipe: &Recipe) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    if recipe.crop_w < 0.999 || recipe.crop_h < 0.999 || recipe.crop_x > 0.001 || recipe.crop_y > 0.001 {
        let cx = ((recipe.crop_x.clamp(0.0, 1.0) * w as f32).round() as u32).min(w.saturating_sub(1));
        let cy = ((recipe.crop_y.clamp(0.0, 1.0) * h as f32).round() as u32).min(h.saturating_sub(1));
        let cw = ((recipe.crop_w.clamp(0.0, 1.0) * w as f32).round() as u32).min(w - cx).max(1);
        let ch = ((recipe.crop_h.clamp(0.0, 1.0) * h as f32).round() as u32).min(h - cy).max(1);
        img = img.crop_imm(cx, cy, cw, ch);
    }

    let angle = if recipe.rotation.is_finite() { recipe.rotation } else { 0.0 };
    let quarters = (angle / 90.0).round();
    let residual = angle - quarters * 90.0;
    img = match (quarters as i64).rem_euclid(4) {
        1 => img.rotate90(),
        2 => img.rotate180(),
        3 => img.rotate270(),
        _ => img,
    };
    if residual.abs() >= 0.01 {
        img = straighten(img, residual);
    }

    if recipe.flip_h {
        img = img.fliph();
    }
    if recipe.flip_v {
        img = img.flipv();
    }
    img
}

/// Rotates by `degrees` (clockwise, |degrees| <= 45) and keeps the largest centred
/// rectangle of the original shape that lies inside the rotated photo.
fn straighten(img: DynamicImage, degrees: f32) -> DynamicImage {
    let src = img.to_rgb16();
    let (w, h) = (src.width() as f32, src.height() as f32);
    let a = degrees.to_radians();
    let (sin, cos) = (a.sin(), a.cos());
    let (sa, ca) = (sin.abs(), cos.abs());
    let scale = (w / (w * ca + h * sa)).min(h / (w * sa + h * ca));
    let (ow, oh) = (((w * scale).floor() as u32).max(1), ((h * scale).floor() as u32).max(1));
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let data = src.as_raw();
    let mut out = vec![0u16; ow as usize * oh as usize * 3];
    out.par_chunks_mut(ow as usize * 3).enumerate().for_each(|(y, row)| {
        let dy = y as f32 + 0.5 - oh as f32 / 2.0;
        for x in 0..ow as usize {
            let dx = x as f32 + 0.5 - ow as f32 / 2.0;
            // Inverse of a clockwise rotation in y-down coordinates
            let sx = (dx * cos + dy * sin + w / 2.0 - 0.5).clamp(0.0, w - 1.0);
            let sy = (-dx * sin + dy * cos + h / 2.0 - 0.5).clamp(0.0, h - 1.0);
            let (x0, y0) = (sx as usize, sy as usize);
            let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            for c in 0..3 {
                let at = |yy: usize, xx: usize| data[(yy * sw + xx) * 3 + c] as f32;
                let top = at(y0, x0) * (1.0 - fx) + at(y0, x1) * fx;
                let bottom = at(y1, x0) * (1.0 - fx) + at(y1, x1) * fx;
                row[x * 3 + c] = (top * (1.0 - fy) + bottom * fy + 0.5) as u16;
            }
        }
    });
    ImageBuffer::<Rgb<u16>, Vec<u16>>::from_raw(ow, oh, out).map(DynamicImage::ImageRgb16).unwrap_or(DynamicImage::ImageRgb16(src))
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

static EXPORT_TEMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A PNG handed to an external encoder; removed when dropped.
struct TempPng(PathBuf);

impl TempPng {
    fn write(dir: &Path, stem: &str, img: &DynamicImage, icc: Option<&[u8]>) -> Result<Self, String> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let seq = EXPORT_TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!(".tmp_{}_{}_{}_{}.png", stem, std::process::id(), seq, nanos));
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

/// A temporary binary file handed to an external encoder (e.g. raw RGBA1010102 or SDR JPEG
/// for `ultrahdr_app`); removed automatically on drop.
struct TempBytes(PathBuf);

impl TempBytes {
    fn write(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> Result<Self, String> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let seq = EXPORT_TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!(".tmp_{}_{}_{}_{}.{}", stem, std::process::id(), seq, nanos, ext));
        crate::security::atomic_write_user(&path, bytes)
            .map_err(|e| format!("Failed to save temporary {}: {}", ext, e))?;
        Ok(Self(path))
    }
}

impl Drop for TempBytes {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// RAII sibling staging path for external encoders (cjxl, avifenc, ffmpeg, heif-enc, cwebp,
/// avifgainmaputil, ultrahdr_app). Preserves the target extension so format-inferring encoders
/// work, and atomically renames into `dest` only on `commit()`. Drops unlink the incomplete
/// stage file on any error.
struct StagedOutput {
    stage: PathBuf,
    dest: PathBuf,
    committed: bool,
}

impl StagedOutput {
    fn new(dest: &Path, ext: &str) -> Self {
        let dir = dest.parent().unwrap_or_else(|| Path::new("."));
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let seq = EXPORT_TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stage = dir.join(format!(".tmp_export_{}_{}_{}.{}", std::process::id(), seq, nanos, ext));
        Self { stage, dest: dest.to_path_buf(), committed: false }
    }

    fn stage_path(&self) -> &Path {
        &self.stage
    }

    fn commit(mut self) -> Result<(), String> {
        let meta = fs::symlink_metadata(&self.stage)
            .map_err(|e| format!("Encoder did not produce staged output: {}", e))?;
        if meta.file_type().is_symlink() || meta.len() == 0 {
            return Err("Encoder produced invalid or empty staged output".to_string());
        }
        fs::rename(&self.stage, &self.dest)
            .map_err(|e| format!("Failed to finalize export {}: {}", self.dest.display(), e))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedOutput {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.stage);
        }
    }
}

/// Guards relative paths starting with `-` or `@` before handing them to `exiftool`.
fn safe_exiftool_path(p: &Path) -> std::ffi::OsString {
    if p.is_relative()
        && let Some(text) = p.to_str()
        && (text.starts_with('-') || text.starts_with('@'))
    {
        return PathBuf::from(".").join(p).into_os_string();
    }
    p.as_os_str().to_os_string()
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
/// The parallel writer's codec for a TIFF compression name, or None for LZW, which
/// the `tiff` crate writes.
fn tiff_codec(compression: &str) -> Option<tiff_writer::Codec> {
    match compression {
        "none" => Some(tiff_writer::Codec::None),
        "lzw" => None,
        _ => Some(tiff_writer::Codec::Deflate),
    }
}

/// TIFF through the `tiff` crate, used for LZW.
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

    fn marked(w: u32, h: u32) -> DynamicImage {
        // Black frame with a white pixel in the top-left quarter
        let mut img = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(w, h);
        img.put_pixel(w / 8, h / 8, Rgb([65535, 65535, 65535]));
        DynamicImage::ImageRgb16(img)
    }

    fn white_at(img: &DynamicImage) -> (u32, u32) {
        let rgb = img.to_rgb16();
        let p = rgb.enumerate_pixels().find(|(_, _, p)| p[0] > 30000).expect("marker kept");
        (p.0, p.1)
    }

    /// The crop is drawn on the unrotated photo, so it is cut before the quarter turn.
    #[test]
    fn crop_is_cut_before_rotation() {
        let r = Recipe { crop_x: 0.0, crop_y: 0.0, crop_w: 0.5, crop_h: 0.5, rotation: 90.0, ..Recipe::default() };
        let out = apply_geometry(marked(80, 40), &r);
        assert_eq!((out.width(), out.height()), (20, 40));
        // (10, 5) in the 40x20 crop lands at (20 - 1 - 5, 10) after a clockwise turn
        assert_eq!(white_at(&out), (14, 10));
    }

    /// A small angle is applied, not rounded away, and leaves no empty corners.
    #[test]
    fn straightening_angle_is_applied() {
        let mut img = ImageBuffer::<Rgb<u16>, Vec<u16>>::from_pixel(300, 200, Rgb([40000, 40000, 40000]));
        img.put_pixel(0, 0, Rgb([40000, 40000, 40000]));
        let r = Recipe { rotation: 3.0, ..Recipe::default() };
        let out = apply_geometry(DynamicImage::ImageRgb16(img), &r).to_rgb16();
        assert!(out.width() < 300 && out.width() > 270, "{}", out.width());
        assert!((out.width() as f32 / out.height() as f32 - 1.5).abs() < 0.02);
        for (x, y) in [(0, 0), (out.width() - 1, 0), (0, out.height() - 1), (out.width() - 1, out.height() - 1)] {
            assert!(out.get_pixel(x, y)[0] > 39000, "corner ({x}, {y}) is empty");
        }
        // 50 degrees is a quarter turn and 40 degrees back, not a plain quarter turn
        let r = Recipe { rotation: 50.0, ..Recipe::default() };
        let out = apply_geometry(marked(300, 200), &r);
        assert!(out.width() < 200);
    }

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

    #[test]
    fn ffmpeg_avif_args_omit_double_dash_and_honor_depth_and_lossless() {
        let inp = Path::new("/tmp/-in.png");
        let out = Path::new("/tmp/-out.avif");
        let lossy10 = hdr::build_ffmpeg_avif_args(inp, out, 12, 85, HdrMode::Sdr);
        assert!(!lossy10.iter().any(|a| a == "--"), "ffmpeg rejects -- separator");
        assert!(lossy10.contains(&"file:/tmp/-in.png".to_string()));
        assert!(lossy10.contains(&"file:/tmp/-out.avif".to_string()));
        assert!(lossy10.contains(&"yuv420p10le".to_string()));
        assert!(lossy10.contains(&"avif".to_string()));

        let lossless8 = hdr::build_ffmpeg_avif_args(inp, out, 8, 100, HdrMode::Sdr);
        assert!(lossless8.contains(&"yuv420p".to_string()));
        assert!(lossless8.contains(&"lossless=1".to_string()));
    }

    #[test]
    fn exiftool_path_guards_leading_dash_and_at() {
        assert_eq!(safe_exiftool_path(Path::new("-photo.raf")), "./-photo.raf");
        assert_eq!(safe_exiftool_path(Path::new("@list.raf")), "./@list.raf");
        assert_eq!(safe_exiftool_path(Path::new("/abs/-photo.raf")), "/abs/-photo.raf");
        assert_eq!(safe_exiftool_path(Path::new("normal.raf")), "normal.raf");
    }

    #[test]
    fn staged_output_commits_atomically_and_cleans_up_on_drop() {
        let dir = std::env::temp_dir().join(format!("omastudio_stage_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out.jxl");

        // Dropped without commit leaves no stage file and does not create dest
        let abandoned_stage = {
            let s = StagedOutput::new(&dest, "jxl");
            fs::write(s.stage_path(), b"partial").unwrap();
            s.stage_path().to_path_buf()
        };
        assert!(!abandoned_stage.exists());
        assert!(!dest.exists());

        // Committed stage atomically appears at dest
        let s = StagedOutput::new(&dest, "jxl");
        fs::write(s.stage_path(), b"complete-jxl").unwrap();
        s.commit().unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"complete-jxl");

        let _ = fs::remove_dir_all(&dir);
    }

    /// BUG-BE-18: Opening a scene-linear HDR OpenEXR with > 1.0 highlights must preserve
    /// headroom in preview and allow highlight recovery in both preview and SDR/EXR export.
    #[test]
    fn exr_highlight_recovery_and_unclipped_exr_roundtrip() {
        let dir = std::env::temp_dir().join(format!("omastudio_hdr_exr_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let (w, h) = (16u32, 16u32);
        let mut src_pixels = Vec::with_capacity((w * h * 3) as usize);
        for _y in 0..h {
            for x in 0..w {
                if x < w / 2 {
                    // Neutral 2-stop HDR highlight (+2 EV above diffuse white)
                    src_pixels.extend_from_slice(&[4.0f32, 4.0f32, 4.0f32]);
                } else {
                    // Chromatic HDR highlight (R=+2 EV, G=+1 EV, B=-1 EV)
                    src_pixels.extend_from_slice(&[4.0f32, 2.0f32, 0.5f32]);
                }
            }
        }
        let exr_bytes = encode_exr(w, h, &src_pixels, 32, icc::primaries(OutputColorSpace::Srgb)).unwrap();
        let exr_path = dir.join("hdr_plate.exr");
        fs::write(&exr_path, &exr_bytes).unwrap();

        // 1. Half-size preview stays unclipped f32 with > 1.0 values intact
        let raw = RawImage::open(&exr_path).unwrap();
        let preview = raw.process_preview_16(true).unwrap();
        assert!(preview.is_float(), "EXR preview must stay OwnedF32");
        let f32_prev = preview.as_slice_f32().unwrap();
        let pw = preview.width as usize;
        let right_idx = (preview.height as usize / 2 * pw + pw - 1) * 3;
        assert!((f32_prev[0] - 4.0).abs() < 1e-3, "neutral headroom clipped: {}", f32_prev[0]);
        assert!((f32_prev[right_idx] - 4.0).abs() < 1e-3, "red headroom clipped: {}", f32_prev[right_idx]);
        assert!((f32_prev[right_idx + 1] - 2.0).abs() < 1e-3, "green headroom clipped: {}", f32_prev[right_idx + 1]);

        // 2. Preview at -3.0 EV recovers [4.0, 2.0, 0.5] -> [0.5, 0.25, 0.0625] linear
        // (sRGB ≈ [187, 137, 72]; a clipped u16 loader would collapse both R and G to 99!)
        let dark_recipe = Recipe {
            exposure: -3.0,
            highlight_reconstruct: false,
            sharpness: 0.0,
            denoise_col: 0.0,
            denoise_lum: 0.0,
            ..Recipe::default()
        };
        let (rgb8, _) = crate::pipeline::process_buffer_f32_to_8(
            f32_prev,
            preview.width,
            preview.height,
            preview.channels,
            &dark_recipe,
        );
        let pr = &rgb8[right_idx..right_idx + 3];
        assert!((pr[0] as i32 - 187).abs() <= 4, "R should recover to ~187, got {:?}", pr);
        assert!((pr[1] as i32 - 137).abs() <= 4, "G should recover to ~137, got {:?}", pr);
        assert!((pr[2] as i32 - 72).abs() <= 4, "B should recover to ~72, got {:?}", pr);

        // 3. SDR 16-bit PNG export at -3.0 EV recovers exact unclipped channels
        let png_opts = ExportOptions {
            format: "png".to_string(),
            bit_depth: 16,
            preserve_exif: false,
            output_dir: dir.to_string_lossy().to_string(),
            ..ExportOptions::default()
        };
        let png_out = export_photo(&exr_path, &dark_recipe, &png_opts).unwrap();
        let decoded_png = image::open(&png_out).unwrap().to_rgb16();
        let px = decoded_png.get_pixel(w - 2, h / 2);
        assert!((px[0] as i32 - 48192).abs() <= 1000, "PNG R should recover to ~48192, got {:?}", px);
        assert!((px[1] as i32 - 35217).abs() <= 1000, "PNG G should recover to ~35217, got {:?}", px);
        assert!((px[2] as i32 - 18423).abs() <= 1000, "PNG B should recover to ~18423, got {:?}", px);

        // 4. OpenEXR export at neutral 0.0 EV preserves > 1.0 scene-linear headroom unclipped!
        let neutral_recipe = Recipe {
            highlight_reconstruct: false,
            sharpness: 0.0,
            denoise_col: 0.0,
            denoise_lum: 0.0,
            ..Recipe::default()
        };
        let exr_opts = ExportOptions {
            format: "exr".to_string(),
            bit_depth: 32,
            preserve_exif: false,
            output_dir: dir.to_string_lossy().to_string(),
            ..ExportOptions::default()
        };
        let exr_out = export_photo(&exr_path, &neutral_recipe, &exr_opts).unwrap();
        let re_opened = RawImage::open(&exr_out).unwrap().process_full_16(3).unwrap();
        let roundtrip = re_opened.as_slice_f32().unwrap();
        let left_mid = (8 * w as usize + 2) * 3;
        let right_mid = (8 * w as usize + w as usize - 2) * 3;
        assert!((roundtrip[left_mid] - 4.0).abs() < 0.05, "neutral EXR clipped: {}", roundtrip[left_mid]);
        assert!((roundtrip[left_mid + 1] - 4.0).abs() < 0.05, "neutral EXR clipped: {}", roundtrip[left_mid + 1]);
        assert!(roundtrip[right_mid] > 2.5, "chromatic EXR R clipped: {}", roundtrip[right_mid]);
        assert!(roundtrip[right_mid + 1] > 1.8, "chromatic EXR G clipped: {}", roundtrip[right_mid + 1]);

        let _ = fs::remove_dir_all(&dir);
    }
}
