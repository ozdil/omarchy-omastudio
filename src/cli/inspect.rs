//! Photo inspection, single-shot rendering, folder scanning, AI auto/Jev/social, and engine status commands.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use omastudio_engine::{
    ai::{
        self, JevDecisions, ai_auto_enhance, ai_classify_scene, ai_classify_scene_with_jev,
        ai_optimize_for_social, apply_jev_decisions_to_recipe,
    },
    gpu, hash, icc, paths,
    pipeline::{process_buffer_16_to_8_ex, process_split_comparison_16_to_8_ex},
    raw::{self, RawImage},
    recipe::{Catalog, CatalogItem, Recipe},
    security,
};
use serde::Serialize;

use crate::daemon::protocol::RenderResult;
use crate::theme;
use super::{ResponseWrapper, print_json};

/// Bumped when cached thumbnails must be regenerated. v2: earlier scans copied raster
/// files that failed to decode into the cache unchanged, so those entries are not
/// guaranteed to be engine-encoded and are no longer used.
const THUMBNAIL_CACHE_VERSION: &str = "v2";

/// Cache path of a photo's thumbnail. Keyed by a hash of the full path (not just the
/// file stem) so DSC_0001.NEF / DSC_0001.JPG or same-named files from two cards don't
/// share or overwrite each other's thumbnail.
pub fn thumbnail_path_for(photo: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(photo).unwrap_or_else(|_| photo.to_path_buf());
    let key = format!("{}:{}", THUMBNAIL_CACHE_VERSION, canonical.to_string_lossy());
    let digest = hash::sha256_hex(key.as_bytes());
    let stem = photo.file_stem().and_then(|s| s.to_str()).unwrap_or("thumb");
    paths::app_cache_dir()
        .join("thumbnails")
        .join(format!("{}-{}.jpg", stem, &digest[..16]))
}

#[derive(Serialize)]
pub struct InspectResult {
    pub path: String,
    pub metadata: raw::RawMetadata,
    pub thumbnail: String,
    pub recipe: Recipe,
    pub scene: ai::SceneAnalysis,
}

#[derive(Serialize)]
pub struct JevInspectResult {
    pub path: String,
    pub metadata: raw::RawMetadata,
    pub thumbnail: String,
    pub recipe: Recipe,
    pub scene: ai::SceneAnalysis,
    pub jev_decisions: Option<JevDecisions>,
}

#[derive(Serialize)]
pub struct FolderScanItem {
    pub name: String,
    pub path: String,
    pub thumbnail: String,
    pub is_remote: bool,
    pub is_dir: bool,
    pub rating: u8,
    pub flag: String,
}

pub fn scan_directory(dir_path: &str) -> Result<Vec<FolderScanItem>, String> {
    let expanded = if let Some(stripped) = dir_path.strip_prefix("~/") {
        paths::home_dir().join(stripped)
    } else if dir_path == "~" {
        paths::home_dir()
    } else {
        PathBuf::from(dir_path)
    };

    if !expanded.exists() {
        return Err(format!("Directory does not exist: {}", expanded.display()));
    }

    let thumb_dir = paths::app_cache_dir().join("thumbnails");
    let _ = security::ensure_secure_dir(&thumb_dir);
    let catalog = Catalog::load();

    let mut dirs = Vec::new();
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&expanded) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() && !name.starts_with('.') {
                dirs.push(FolderScanItem {
                    name,
                    path: path.to_string_lossy().to_string(),
                    thumbnail: String::new(),
                    is_remote: false,
                    is_dir: true,
                    rating: 0,
                    flag: "none".to_string(),
                });
                continue;
            }
            if path.is_file()
                && let Some(ext) = path.extension().and_then(|e| e.to_str())
            {
                let ext_lower = ext.to_lowercase();
                if raw::is_supported_extension(&ext_lower) {
                    let thumb_path = thumbnail_path_for(&path);
                    if !thumb_path.exists()
                        && let Ok(raw) = RawImage::open(&path)
                    {
                        let _ = raw.extract_thumbnail(&thumb_path);
                    }
                    let path_str = path.to_string_lossy().to_string();
                    let cat_item = catalog.get(&path_str);

                    items.push(FolderScanItem {
                        name: path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
                        path: path_str,
                        thumbnail: if thumb_path.exists() {
                            thumb_path.to_string_lossy().to_string()
                        } else {
                            String::new()
                        },
                        is_remote: false,
                        is_dir: false,
                        rating: cat_item.map_or(0, |c| c.rating),
                        flag: cat_item.map_or_else(|| "none".to_string(), |c| c.flag.clone()),
                    });
                }
            }
        }
    }

    dirs.sort_by_key(|a| a.name.to_lowercase());
    items.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.extend(items);
    Ok(dirs)
}

pub fn inspect_file(raw_path: &str) -> Result<InspectResult, String> {
    let raw = RawImage::open(raw_path)?;
    let meta = raw.get_metadata()?;

    let thumb_dir = paths::app_cache_dir().join("thumbnails");
    security::ensure_secure_dir(&thumb_dir)
        .map_err(|e| format!("Thumbnail dir error: {}", e))?;

    let file_stem = Path::new(raw_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    let thumb_path = thumbnail_path_for(Path::new(raw_path));

    let _ = raw.extract_thumbnail(&thumb_path);
    let recipe = Recipe::load_initial(raw_path);

    let preview = raw.process_preview(true)?;
    let scene = ai_classify_scene(preview.as_slice(), preview.width, preview.height, preview.channels, &meta);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Preserve culling data (rating / flag / colour label) already stored for this photo
    let prev = Catalog::load().get(raw_path).cloned();
    let item = CatalogItem {
        id: file_stem.to_string(),
        path: raw_path.to_string(),
        remote_path: prev.as_ref().and_then(|p| p.remote_path.clone()),
        thumbnail_path: thumb_path.to_string_lossy().to_string(),
        metadata: meta.clone(),
        rating: prev.as_ref().map_or(0, |p| p.rating),
        color_label: prev.as_ref().map_or_else(String::new, |p| p.color_label.clone()),
        flag: prev.as_ref().map_or_else(|| recipe.flag.clone(), |p| p.flag.clone()),
        recipe: Recipe { snapshots: Vec::new(), ..recipe.clone() },
        updated_at: now,
    };
    let _ = Catalog::upsert_indexed(&item);

    Ok(InspectResult {
        path: raw_path.to_string(),
        metadata: meta,
        thumbnail: thumb_path.to_string_lossy().to_string(),
        recipe,
        scene,
    })
}

pub fn render_file(raw_path: &str, recipe: &Recipe, split: f32, out_path: &str) -> Result<RenderResult, String> {
    let raw = RawImage::open(raw_path)?;
    let preview = raw.process_preview_16(true)?;
    let recipe = &Recipe { baseline_exposure: raw.baseline_exposure_from(&preview), ..recipe.clone() };

    let (final_buf, hist) = if let Some(f32_slice) = preview.as_slice_f32() {
        if split > 0.001 {
            omastudio_engine::pipeline::process_split_comparison_f32_to_8_ex(
                f32_slice,
                preview.width,
                preview.height,
                preview.channels,
                recipe,
                preview.transfer,
                split,
                false,
                false,
            )
        } else {
            omastudio_engine::pipeline::process_buffer_f32_to_8_proof(
                f32_slice,
                preview.width,
                preview.height,
                preview.channels,
                recipe,
                preview.transfer,
                false,
                false,
                None,
            )
        }
    } else if split > 0.001 {
        process_split_comparison_16_to_8_ex(
            preview.as_slice_u16(),
            preview.width,
            preview.height,
            preview.channels,
            recipe,
            preview.transfer,
            split,
            false,
            false,
        )
    } else {
        process_buffer_16_to_8_ex(
            preview.as_slice_u16(),
            preview.width,
            preview.height,
            preview.channels,
            recipe,
            preview.transfer,
            false,
            false,
        )
    };

    let dest_path = Path::new(out_path);
    if out_path.ends_with(".ppm") {
        let mut ppm_bytes = format!("P6\n{} {}\n255\n", preview.width, preview.height).into_bytes();
        ppm_bytes.extend_from_slice(&final_buf);
        security::atomic_write_user(dest_path, &ppm_bytes)
            .map_err(|e| format!("Failed to write PPM: {}", e))?;
    } else {
        let img: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> = image::ImageBuffer::from_raw(
            preview.width,
            preview.height,
            final_buf,
        )
        .ok_or_else(|| "Failed to create ImageBuffer".to_string())?;
        let mut encoded = Vec::new();
        let fmt = if out_path.ends_with(".png") {
            image::ImageFormat::Png
        } else {
            image::ImageFormat::Jpeg
        };
        img.write_to(&mut std::io::Cursor::new(&mut encoded), fmt)
            .map_err(|e| format!("Failed to encode preview: {}", e))?;
        security::atomic_write_user(dest_path, &encoded)
            .map_err(|e| format!("Failed to save preview: {}", e))?;
    }

    Ok(RenderResult::from_file(out_path.to_string(), hist, None))
}

pub fn run_ai_auto(raw_path: &str) -> Result<InspectResult, String> {
    let raw = RawImage::open(raw_path)?;
    let meta = raw.get_metadata()?;
    let preview = raw.process_preview(true)?;

    let auto_recipe = ai_auto_enhance(preview.as_slice(), preview.width, preview.height, preview.channels, &meta);
    let scene = ai_classify_scene(preview.as_slice(), preview.width, preview.height, preview.channels, &meta);

    let thumb_path = thumbnail_path_for(Path::new(raw_path));

    Ok(InspectResult {
        path: raw_path.to_string(),
        metadata: meta,
        thumbnail: thumb_path.to_string_lossy().to_string(),
        recipe: auto_recipe,
        scene,
    })
}

pub fn run_ai_jev(raw_path: &str) -> Result<JevInspectResult, String> {
    let raw = RawImage::open(raw_path)?;
    let meta = raw.get_metadata()?;
    let preview = raw.process_preview(true)?;

    let mut auto_recipe = ai_auto_enhance(preview.as_slice(), preview.width, preview.height, preview.channels, &meta);
    let (mut scene, mut jev_opt) = ai_classify_scene_with_jev(preview.as_slice(), preview.width, preview.height, preview.channels, &meta);

    if jev_opt.is_none() {
        let state = ai::jev::extract_photographic_state(preview.as_slice(), preview.width, preview.height, preview.channels, &meta);
        let offline_jev = ai::jev::generate_offline_jev_decisions(&state, &meta);
        apply_jev_decisions_to_recipe(&mut auto_recipe, &offline_jev);
        scene.recommended_preset = offline_jev.recommended_preset.clone();
        scene.description = format!(
            "Jev Offline Expert [Score: {:.1}/10, Contrast: {}]",
            offline_jev.aesthetic_score, offline_jev.recommended_contrast
        );
        jev_opt = Some(offline_jev);
    } else if let Some(ref jev) = jev_opt {
        apply_jev_decisions_to_recipe(&mut auto_recipe, jev);
    }

    let thumb_path = thumbnail_path_for(Path::new(raw_path));

    Ok(JevInspectResult {
        path: raw_path.to_string(),
        metadata: meta,
        thumbnail: thumb_path.to_string_lossy().to_string(),
        recipe: auto_recipe,
        scene,
        jev_decisions: jev_opt,
    })
}

pub fn run_ai_social(raw_path: &str, platform: &str) -> Result<ai::SocialOptimizationResult, String> {
    let raw = RawImage::open(raw_path)?;
    let preview = raw.process_preview(true)?;
    let sidecar = Recipe::load_initial(raw_path);
    Ok(ai_optimize_for_social(
        preview.as_slice(),
        preview.width,
        preview.height,
        preview.channels,
        platform,
        &sidecar,
    ))
}

/// Reports the active `wgpu`/WGSL compute and banded CPU pipeline status (superseding
/// the removed prototype `src/dag/` CPU graph).
pub fn run_dag() {
    #[derive(Serialize)]
    struct PipelineStatus {
        engine: String,
        status: String,
        architecture: String,
        deprecated: bool,
        gpu: gpu::GpuStatus,
        stages_supported: Vec<String>,
    }
    let gpu_status = gpu::status();
    let info = PipelineStatus {
        engine: "OmaStudio Banded wgpu/WGSL + Rayon Pipeline".to_string(),
        status: if gpu_status.enabled { "gpu_active".into() } else { "cpu_fallback".into() },
        architecture: "Rust 2024 + wgpu/WGSL Compute (Vulkan/GLES) + Rayon CPU".to_string(),
        deprecated: true,
        gpu: gpu_status,
        stages_supported: vec![
            "RawUnpack".into(),
            "Demosaic".into(),
            "WhiteBalanceAndExposure".into(),
            "ToneAndCurves".into(),
            "RgbMixerAndSkinTone".into(),
            "ColorWheelsAndHsl".into(),
            "Lut3D".into(),
            "FilmSimulationAndColorChrome".into(),
            "AcesGamutCompression".into(),
            "LocalMasks".into(),
            "PresenceDetailDefringe".into(),
            "FilmGrainAndVignette".into(),
            "GeometryRemap".into(),
            "IccAndExport".into(),
        ],
    };
    print_json(&ResponseWrapper::ok(info));
}

pub fn run_status() {
    #[derive(Serialize)]
    struct StatusInfo {
        app: String,
        version: String,
        status: String,
        active_mode: String,
        active_theme: String,
        supported_raw_formats: Vec<String>,
        supported_cameras_count: usize,
        icc_profiles_count: usize,
        local_only_mode: bool,
        gpu: gpu::GpuStatus,
        features: Vec<String>,
    }
    let status = StatusInfo {
        app: "OmaStudio".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: "ready".to_string(),
        active_mode: "studio".to_string(),
        active_theme: theme::active_theme_name(),
        supported_raw_formats: raw::RAW_EXTENSIONS.iter().map(|e| e.to_uppercase()).collect(),
        supported_cameras_count: raw::supported_cameras().len(),
        icc_profiles_count: icc::list_icc_profiles().len(),
        local_only_mode: true,
        gpu: gpu::status(),
        features: vec![
            "colour_grading".into(),
            "channel_mixer".into(),
            "hsl".into(),
            "tone_curve".into(),
            "presence".into(),
            "detail".into(),
            "grain".into(),
            "skin_tone".into(),
            "masks".into(),
            "crop_and_straighten".into(),
            "film_simulations".into(),
            "hasselblad_profiles".into(),
            "aces_tone_curve".into(),
            "gamut_compression".into(),
            "icc_export".into(),
            "watermark".into(),
            "platform_presets".into(),
            "exif_and_gps_control".into(),
            "xmp_sidecars".into(),
            "rating_and_flags".into(),
            "local_ai_assistant".into(),
            "ai_masks".into(),
            "gpu_compute_pipeline".into(),
            "banded_medium_format_pipeline".into(),
            "sqlite_wal_catalog".into(),
            "pixel_shift".into(),
            "omarchy_theme_and_font".into(),
            "control_socket".into(),
        ],
    };
    print_json(&ResponseWrapper::ok(status));
}
