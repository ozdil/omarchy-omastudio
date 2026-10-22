#![allow(clippy::excessive_precision, clippy::too_many_arguments)]

pub mod ai;
pub mod dag;
pub mod export;
pub mod fuji;
pub mod gpu;
pub mod icc;
mod mcp;
pub mod pipeline;
pub mod ptp;
pub mod raw;
pub mod recipe;
pub mod hash;
pub mod security;
pub mod xmp;

use ai::{
    ai_auto_enhance, ai_classify_scene, ai_classify_scene_with_jev, ai_optimize_for_social,
    apply_jev_decisions_to_recipe, JevDecisions,
};
use export::{export_photo, ExportOptions};
use pipeline::transfer::Transfer;
use pipeline::{process_buffer_16_to_8_ex, process_split_comparison_16_to_8_ex};
use raw::RawImage;
use rayon::prelude::*;
use recipe::{Catalog, CatalogItem, Recipe};
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
struct ResponseWrapper<T: Serialize> {
    success: bool,
    action: Option<String>,
    data: Option<T>,
    error: Option<String>,
}

impl<T: Serialize> ResponseWrapper<T> {
    fn ok(data: T) -> Self {
        Self {
            success: true,
            action: None,
            data: Some(data),
            error: None,
        }
    }

    fn ok_action(action: &str, data: T) -> Self {
        Self {
            success: true,
            action: Some(action.to_string()),
            data: Some(data),
            error: None,
        }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            action: None,
            data: None,
            error: Some(msg.into()),
        }
    }

    fn err_action(action: &str, msg: impl Into<String>) -> Self {
        Self {
            success: false,
            action: Some(action.to_string()),
            data: None,
            error: Some(msg.into()),
        }
    }
}

fn print_json<T: Serialize>(resp: &ResponseWrapper<T>) {
    if let Ok(json) = serde_json::to_string(resp) {
        println!("{}", json);
        let _ = io::stdout().flush();
    }
}

#[derive(Serialize)]
struct InspectResult {
    path: String,
    metadata: raw::RawMetadata,
    thumbnail: String,
    recipe: Recipe,
    scene: ai::SceneAnalysis,
}

#[derive(Serialize)]
struct JevInspectResult {
    path: String,
    metadata: raw::RawMetadata,
    thumbnail: String,
    recipe: Recipe,
    scene: ai::SceneAnalysis,
    jev_decisions: Option<JevDecisions>,
}

#[derive(Serialize)]
struct RenderResult {
    image: String,
    histogram: pipeline::histogram::HistogramData,
}

#[derive(Serialize)]
struct ExportResult {
    exported_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BatchExportItem {
    path: String,
    #[serde(default)]
    recipe: Option<Recipe>,
}

#[derive(Serialize)]
struct BatchExportItemResult {
    path: String,
    success: bool,
    exported_path: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct BatchExportResult {
    total: usize,
    succeeded: usize,
    failed: usize,
    results: Vec<BatchExportItemResult>,
}

struct DaemonCache {
    current_path: Option<String>,
    raw_buffer_16: Option<Arc<Vec<u16>>>,
    raw_buffer_8: Option<Arc<Vec<u8>>>,
    /// Transfer curve of `raw_buffer_16` (linear light for LibRaw decodes).
    transfer: Transfer,
    width: u32,
    height: u32,
    channels: u32,
    metadata: Option<raw::RawMetadata>,
    ping_pong: usize,
    /// The photo at full resolution for the viewport's 1:1 detail view, decoded in the
    /// background after `load` (see `full_ready`).
    full: Option<Arc<FullFrame>>,
    detail_ping_pong: usize,
}

struct FullFrame {
    data: Vec<u16>,
    width: usize,
    height: usize,
    transfer: Transfer,
}

#[derive(Serialize)]
struct FullReady {
    path: String,
    width: usize,
    height: usize,
}

#[derive(Serialize)]
struct DetailResult {
    image: String,
    /// The rendered part of the photo, normalised: x, y, width, height.
    region: [f64; 4],
    width: usize,
    height: usize,
}

/// Decodes `path` at full resolution on a background thread and stores it in the cache
/// if that photo is still the current one.
fn spawn_full_decode(cache: Arc<Mutex<DaemonCache>>, path: String) {
    std::thread::spawn(move || {
        let Ok(raw) = RawImage::open(&path) else { return };
        let Ok(full) = raw.process_full_16(3) else { return };
        drop(raw);
        if full.channels != 3 {
            return;
        }
        let frame = FullFrame {
            width: full.width as usize,
            height: full.height as usize,
            transfer: full.transfer,
            data: full.to_vec_u16(),
        };
        let ready = FullReady { path: path.clone(), width: frame.width, height: frame.height };
        let Ok(mut c) = cache.lock() else { return };
        if c.current_path.as_deref() != Some(path.as_str()) {
            return;
        }
        c.full = Some(Arc::new(frame));
        drop(c);
        print_json(&ResponseWrapper::ok_action("full_ready", ready));
    });
}

/// Pixels of `frame` scaled by `scale`, for the rectangle (x, y, w, h) of the scaled
/// frame: box-filtered when reducing, bilinear when enlarging.
fn resample_rect(frame: &FullFrame, scale: f64, rect: (usize, usize, usize, usize)) -> Vec<u16> {
    let (rx, ry, rw, rh) = rect;
    let (fw, fh) = (frame.width, frame.height);
    let mut out = vec![0u16; rw * rh * 3];
    out.par_chunks_mut(rw * 3).enumerate().for_each(|(j, row)| {
        let vy = ry + j;
        for i in 0..rw {
            let vx = rx + i;
            let px = &mut row[i * 3..i * 3 + 3];
            if scale < 1.0 {
                let (x0, x1) = ((vx as f64 / scale) as usize, (((vx + 1) as f64 / scale) as usize).max((vx as f64 / scale) as usize + 1));
                let (y0, y1) = ((vy as f64 / scale) as usize, (((vy + 1) as f64 / scale) as usize).max((vy as f64 / scale) as usize + 1));
                let (x1, y1) = (x1.min(fw), y1.min(fh));
                let (x0, y0) = (x0.min(x1.saturating_sub(1)), y0.min(y1.saturating_sub(1)));
                let mut sum = [0u64; 3];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let k = (y * fw + x) * 3;
                        for (acc, &v) in sum.iter_mut().zip(&frame.data[k..k + 3]) {
                            *acc += v as u64;
                        }
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)).max(1) as u64;
                for (p, acc) in px.iter_mut().zip(sum) {
                    *p = (acc / n) as u16;
                }
            } else {
                let sx = ((vx as f64 + 0.5) / scale - 0.5).clamp(0.0, (fw - 1) as f64);
                let sy = ((vy as f64 + 0.5) / scale - 0.5).clamp(0.0, (fh - 1) as f64);
                let (x0, y0) = (sx as usize, sy as usize);
                let (x1, y1) = ((x0 + 1).min(fw - 1), (y0 + 1).min(fh - 1));
                let (fx, fy) = (sx - x0 as f64, sy - y0 as f64);
                for (c, p) in px.iter_mut().enumerate() {
                    let at = |y: usize, x: usize| frame.data[(y * fw + x) * 3 + c] as f64;
                    let v = (at(y0, x0) * (1.0 - fx) + at(y0, x1) * fx) * (1.0 - fy) + (at(y1, x0) * (1.0 - fx) + at(y1, x1) * fx) * fy;
                    *p = v.round() as u16;
                }
            }
        }
    });
    out
}


/// Bumped when cached thumbnails must be regenerated. v2: earlier scans copied raster
/// files that failed to decode into the cache unchanged, so those entries are not
/// guaranteed to be engine-encoded and are no longer used.
const THUMBNAIL_CACHE_VERSION: &str = "v2";

/// Cache path of a photo's thumbnail. Keyed by a hash of the full path (not just the
/// file stem) so DSC_0001.NEF / DSC_0001.JPG or same-named files from two cards don't
/// share or overwrite each other's thumbnail.
fn thumbnail_path_for(photo: &Path) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let canonical = std::fs::canonicalize(photo).unwrap_or_else(|_| photo.to_path_buf());
    let key = format!("{}:{}", THUMBNAIL_CACHE_VERSION, canonical.to_string_lossy());
    let digest = hash::sha256_hex(key.as_bytes());
    let stem = photo.file_stem().and_then(|s| s.to_str()).unwrap_or("thumb");
    PathBuf::from(home)
        .join(".cache/omastudio/thumbnails")
        .join(format!("{}-{}.jpg", stem, &digest[..16]))
}

#[derive(Serialize)]
struct FolderScanItem {
    name: String,
    path: String,
    thumbnail: String,
    is_remote: bool,
    is_dir: bool,
}

fn scan_directory(dir_path: &str) -> Result<Vec<FolderScanItem>, String> {
    let expanded = if let Some(stripped) = dir_path.strip_prefix("~/") {
        let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(stripped)
    } else if dir_path == "~" {
        PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".to_string()))
    } else {
        PathBuf::from(dir_path)
    };

    if !expanded.exists() {
        return Err(format!("Directory does not exist: {}", expanded.display()));
    }

    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let thumb_dir = PathBuf::from(home).join(".cache/omastudio/thumbnails");
    let _ = security::ensure_secure_dir(&thumb_dir);

    let mut dirs = Vec::new();
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&expanded) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Subfolders are listed (first) so the filmstrip can navigate into them
            if path.is_dir() && !name.starts_with('.') {
                dirs.push(FolderScanItem {
                    name,
                    path: path.to_string_lossy().to_string(),
                    thumbnail: String::new(),
                    is_remote: false,
                    is_dir: true,
                });
                continue;
            }
            if path.is_file()
                && let Some(ext) = path.extension().and_then(|e| e.to_str())
            {
                let ext_lower = ext.to_lowercase();
                let is_supported = ["nef", "nrw", "raf", "cr2", "cr3", "arw", "dng", "rwl", "orf", "rw2", "3fr", "jpg", "jpeg", "png", "webp", "tif", "tiff", "heic"].contains(&ext_lower.as_str());
                if is_supported {
                    let thumb_path = thumbnail_path_for(&path);
                    if !thumb_path.exists() {
                        // Thumbnails are always re-encoded by the engine. A file that
                        // fails to decode gets no thumbnail (the filmstrip shows a
                        // placeholder): handing the original to Qt would let a crafted
                        // image reach its decoders without the engine's limits.
                        if let Ok(raw) = RawImage::open(&path) {
                            let _ = raw.extract_thumbnail(&thumb_path);
                        }
                    }

                    items.push(FolderScanItem {
                        name: path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
                        path: path.to_string_lossy().to_string(),
                        thumbnail: if thumb_path.exists() { thumb_path.to_string_lossy().to_string() } else { String::new() },
                        is_remote: false,
                        is_dir: false,
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

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("OmaStudio Engine v{} - Lightroom-Grade Photo RAW Engine for Omarchy Linux", env!("CARGO_PKG_VERSION"));
        eprintln!("Usage: omastudio-engine <command> [arguments]");
        eprintln!("Commands:");
        eprintln!("  inspect <raw_file>");
        eprintln!("  render <raw_file> [--recipe <json>] [--split <0.0-1.0>] [--out <dest>]");
        eprintln!("  ai-auto <raw_file>");
        eprintln!("  ai-jev <raw_file>");
        eprintln!("  export <raw_file> [--recipe <json>] [--options <json>]");
        eprintln!("  catalog list");
        eprintln!("  catalog query [--rating <0-5>] [--flag <pick|reject>] [--search <str>]");
        eprintln!("  catalog cull <path> [--rating <0-5>] [--flag <pick|reject>]");
        eprintln!("  fuji-recipe <raf|jpg|fp1|txt> | list | save <file> | export <file> <out.fp1>");
        eprintln!("  fuji-camera list | info | presets | convert <raf> | write-preset <slot> <recipe>");
        eprintln!("  scan [folder]");
        eprintln!("  daemon");
        return;
    }

    match args[1].as_str() {
        "inspect" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            let raw_path = &args[2];
            match inspect_file(raw_path) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "render" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            let raw_path = &args[2];
            let mut recipe = Recipe::default();
            let mut split = 0.0f32;
            let mut out_path = security::get_secure_preview_dir().join("omastudio_preview.ppm").to_string_lossy().to_string();

            let mut idx = 3;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--recipe" if idx + 1 < args.len() => {
                        if let Ok(r) = serde_json::from_str::<Recipe>(&args[idx + 1]) {
                            recipe = r;
                        }
                        idx += 2;
                    }
                    "--split" if idx + 1 < args.len() => {
                        split = args[idx + 1].parse().unwrap_or(0.0);
                        idx += 2;
                    }
                    "--out" if idx + 1 < args.len() => {
                        out_path = args[idx + 1].clone();
                        idx += 2;
                    }
                    _ => idx += 1,
                }
            }

            match render_file(raw_path, &recipe, split, &out_path) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "ai-auto" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            let raw_path = &args[2];
            match run_ai_auto(raw_path) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "ai-jev" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            let raw_path = &args[2];
            match run_ai_jev(raw_path) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "ai-social" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            let raw_path = &args[2];
            let platform = if args.len() >= 4 { &args[3] } else { "ig" };
            match run_ai_social(raw_path, platform) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "export" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            let raw_path = &args[2];
            let mut recipe = Recipe::default();
            let mut options = ExportOptions::default();

            let mut idx = 3;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--recipe" if idx + 1 < args.len() => {
                        if let Ok(r) = serde_json::from_str::<Recipe>(&args[idx + 1]) {
                            recipe = r;
                        }
                        idx += 2;
                    }
                    "--options" if idx + 1 < args.len() => {
                        if let Ok(o) = serde_json::from_str::<ExportOptions>(&args[idx + 1]) {
                            options = o;
                        }
                        idx += 2;
                    }
                    _ => idx += 1,
                }
            }

            match export_photo(raw_path, &recipe, &options) {
                Ok(dest) => print_json(&ResponseWrapper::ok(ExportResult {
                    exported_path: dest.to_string_lossy().to_string(),
                })),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "catalog" => {
            if args.len() < 3 || args[2] == "list" {
                let cat = Catalog::load();
                print_json(&ResponseWrapper::ok(cat));
            } else if args[2] == "query" {
                let mut filter = recipe::CatalogFilter::default();
                let mut idx = 3;
                while idx < args.len() {
                    match args[idx].as_str() {
                        "--rating" if idx + 1 < args.len() => {
                            filter.min_rating = args[idx + 1].parse().ok();
                            idx += 2;
                        }
                        "--flag" if idx + 1 < args.len() => {
                            filter.flag = Some(args[idx + 1].clone());
                            idx += 2;
                        }
                        "--search" if idx + 1 < args.len() => {
                            filter.search_query = Some(args[idx + 1].clone());
                            idx += 2;
                        }
                        "--model" if idx + 1 < args.len() => {
                            filter.camera_model = Some(args[idx + 1].clone());
                            idx += 2;
                        }
                        _ => idx += 1,
                    }
                }
                let cat = Catalog::load();
                let results = cat.filter_items(&filter);
                print_json(&ResponseWrapper::ok(results));
            } else if args[2] == "cull" {
                if args.len() < 4 {
                    print_json::<()>(&ResponseWrapper::err("Usage: catalog cull <path> [--rating <0-5>] [--flag <pick|reject>] [--color <label>]"));
                    return;
                }
                let path = &args[3];
                let mut rating = None;
                let mut flag = None;
                let mut color = None;
                let mut idx = 4;
                while idx < args.len() {
                    match args[idx].as_str() {
                        "--rating" if idx + 1 < args.len() => {
                            rating = args[idx + 1].parse().ok();
                            idx += 2;
                        }
                        "--flag" if idx + 1 < args.len() => {
                            flag = Some(args[idx + 1].as_str());
                            idx += 2;
                        }
                        "--color" if idx + 1 < args.len() => {
                            color = Some(args[idx + 1].as_str());
                            idx += 2;
                        }
                        _ => idx += 1,
                    }
                }
                let mut cat = Catalog::load();
                if cat.update_culling(path, rating, flag, color) {
                    let _ = cat.save();
                    print_json(&ResponseWrapper::ok("Culling metadata saved"));
                } else {
                    print_json::<()>(&ResponseWrapper::err("Item not found in catalog"));
                }
            } else {
                print_json::<()>(&ResponseWrapper::err("Unknown catalog subcommand: list, query, cull"));
            }
        }
        "fuji-recipe" => {
            run_fuji_recipe(&args[2..]);
        }
        "fuji-camera" => {
            run_fuji_camera(&args[2..]);
        }
        "fuji-pixel-shift" => {
            let mut paths = Vec::new();
            let mut out: Option<PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                if args[i] == "--out" && i + 1 < args.len() {
                    out = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    paths.push(PathBuf::from(&args[i]));
                    i += 1;
                }
            }
            if paths.len() < 2 {
                print_json::<()>(&ResponseWrapper::err("Usage: fuji-pixel-shift <raf>... [--out <file.dng>]"));
                return;
            }
            let out = out.unwrap_or_else(|| {
                let stem = paths[0].file_stem().and_then(|s| s.to_str()).unwrap_or("pixelshift");
                paths[0].with_file_name(format!("{stem}_pixelshift.dng"))
            });
            if out.exists() {
                print_json::<()>(&ResponseWrapper::err(format!("{} already exists", out.display())));
                return;
            }
            match fuji::pixel_shift::combine(&paths, &out) {
                Ok(report) => print_json(&ResponseWrapper::ok(report)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "fuji-calibrate" => {
            let paths: Vec<PathBuf> = args[2..].iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
            if paths.is_empty() {
                print_json::<()>(&ResponseWrapper::err("Usage: fuji-calibrate <raf>... [--any] [--dry-run]"));
                return;
            }
            let any = args.iter().any(|a| a == "--any");
            let write = !args.iter().any(|a| a == "--dry-run");
            let (reports, skipped) = fuji::calibrate::calibrate(&paths, any, write);
            print_json(&ResponseWrapper::ok(serde_json::json!({ "calibrated": reports, "skipped": skipped })));
        }
        "icc" => {
            if args.len() < 3 || args[2] == "list" {
                let profiles = icc::list_icc_profiles();
                print_json(&ResponseWrapper::ok(profiles));
            } else {
                print_json::<()>(&ResponseWrapper::err("Unknown icc subcommand"));
            }
        }
        "scan" => {
            let dir = if args.len() >= 3 { &args[2] } else { "." };
            match scan_directory(dir) {
                Ok(items) => print_json(&ResponseWrapper::ok(items)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "dag" => {
            #[derive(Serialize)]
            struct DagStatus {
                engine: String,
                status: String,
                architecture: String,
                tiling_default_tile_size: u32,
                nodes_supported: Vec<String>,
            }
            let dag_info = DagStatus {
                engine: "Demand-Driven Tiled DAG Engine".to_string(),
                status: "active".to_string(),
                architecture: "Rust + Slang/Vulkan SPIR-V Compute".to_string(),
                tiling_default_tile_size: dag::tiling::DEFAULT_TILE_SIZE,
                nodes_supported: vec![
                    "RawSource".into(),
                    "Demosaic".into(),
                    "WhiteBalance".into(),
                    "Exposure".into(),
                    "ToneCurve".into(),
                    "ColorWheels".into(),
                    "FilmSimulation".into(),
                    "AcesColorManagement".into(),
                    "Lut3D".into(),
                    "Denoise".into(),
                    "Watermark".into(),
                    "DisplaySink".into(),
                ],
            };
            print_json(&ResponseWrapper::ok(dag_info));
        }
        "status" | "--status" => {
            #[derive(Serialize)]
            struct StatusInfo {
                app: String,
                version: String,
                status: String,
                active_mode: String,
                active_theme: String,
                supported_raw_formats: Vec<String>,
                icc_profiles_count: usize,
                local_only_mode: bool,
                gpu: gpu::GpuStatus,
                features: Vec<String>,
            }
            let icc_count = icc::list_icc_profiles().len();
            let active_theme = env::var("HOME")
                .ok()
                .and_then(|h| security::read_secure_file(&PathBuf::from(h).join(".local/state/omarchy/current/theme.name"), 1024).ok())
                .map(|b| String::from_utf8_lossy(&b).trim().to_string())
                .unwrap_or_else(|| "default".to_string());

            let status = StatusInfo {
                app: "OmaStudio".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                status: "ready".to_string(),
                active_mode: "studio".to_string(),
                active_theme,
                supported_raw_formats: vec![
                    "RAF".into(), "NEF".into(), "NRW".into(), "CR2".into(), "CR3".into(),
                    "ARW".into(), "SR2".into(), "DNG".into(), "RWL".into(), "ORF".into(),
                    "RW2".into(), "3FR".into(),
                ],
                icc_profiles_count: icc_count,
                local_only_mode: true,
                gpu: gpu::status(),
                features: vec![
                    "davinci_3way_color_wheels".into(),
                    "ai_social_media_optimizer".into(),
                    "lightroom_hsl_and_curves".into(),
                    "interactive_crop_and_guides".into(),
                    "icc_color_profile_management".into(),
                    "exif_preservation_and_gps_strip".into(),
                    "touchpad_mac_pinch_rotate_pan".into(),
                    "control_socket_ipc".into(),
                    "omarchy_system_theme_sync".into(),
                    "presence_engine_texture_clarity_dehaze".into(),
                    "detail_and_optics_engine".into(),
                    "medium_format_16bit_raw_pipeline".into(),
                    "perceptual_shadow_retinal_toe".into(),
                    "typesafe_ai_jev_decision_engine".into(),
                    "aces_1_3_color_management".into(),
                    "aces_gamut_compression".into(),
                    "fujifilm_film_simulations".into(),
                    "hasselblad_hncs_and_xpan".into(),
                    "watermark_and_branding_engine".into(),
                    "dag_demand_driven_pipeline_engine".into(),
                    "tiled_region_of_interest_execution".into(),
                    "vulkan_slang_gpu_compute_architecture".into(),
                    "linux_wayland_color_management_v1".into(),
                    "capture_one_skin_tone_uniformity".into(),
                    "davinci_rgb_primary_matrix_mixer".into(),
                    "silver_halide_photochemical_film_grain".into(),
                    "local_layered_mask_adjustments".into(),
                    "studio_culling_and_fast_rating".into(),
                ],
            };
            print_json(&ResponseWrapper::ok(status));
        }
        "gpu" => {
            print_json(&ResponseWrapper::ok(gpu::status()));
        }
        "mcp" => mcp::run(),
        "ai" => run_ai(&args[2..]),
        "xmp" => run_xmp(&args[2..]),
        "version" | "--version" | "-v" => {
            println!("omastudio-engine v{}", env!("CARGO_PKG_VERSION"));
        }
        "help" | "--help" | "-h" => {
            println!("OmaStudio Engine v{} - Professional Photo RAW Engine for Omarchy Linux", env!("CARGO_PKG_VERSION"));
            println!("\nUsage: omastudio-engine <command> [arguments]\n");
            println!("Commands for Omarchy Agents & CLI:");
            println!("  status                                Output engine and feature status as JSON");
            println!("  inspect <raw_file>                    Parse EXIF, dynamic range, AI scene classification, histogram");
            println!("  ai-auto <raw_file>                    Calculate optimal tone, white balance, and contrast via AI");
            println!("  ai-jev <raw_file>                     Execute TypeSafe AI Jev System 1 typed decision engine");
            println!("  ai-social <raw_file> [platform]       Calculate social media crop & OLED recipe (ig, story, x, ig_square, fb, yt)");
            println!("  render <raw_file> [options]           Process RAW image to preview PPM/PNG/JPG");
            println!("  export <raw_file> [options]           High-res demosaic & export (jxl, avif, webp, tiff, png, jpg)");
            println!("  fuji-recipe <raf|jpg|fp1|txt>         Read a Fujifilm recipe from a photo, X RAW Studio profile or recipe text");
            println!("  fuji-recipe list                      List the recipes saved in ~/.local/share/omastudio/fuji-recipes");
            println!("  fuji-recipe save <file> [--label L]   Save a recipe into the library as .FP1");
            println!("  fuji-recipe export <file> <out.fp1>   Write a recipe as an X RAW Studio profile [--label L] [--device X-T30]");
            println!("  fuji-camera list|info|presets         Fujifilm cameras on USB (PTP mode) and their C1-C7 settings");
            println!("  fuji-camera convert <raf> [--recipe F] [--out J]  Convert a RAF in the camera (X RAW Studio protocol)");
            println!("  fuji-camera write-preset <C1-C7> <recipe> [--label L]  Store a recipe in a custom setting slot");
            println!("  fuji-calibrate <raf>... [--any]       Measure film simulation LUTs from your RAFs' camera JPEGs");
            println!("  fuji-pixel-shift <raf>... [--out F]   Merge a GFX Pixel Shift sequence into a linear DNG");
            println!("  ai providers                          AI providers, stored keys and local servers (Ollama, LM Studio)");
            println!("  ai key <provider> [--delete]          Store the API key read from stdin in the Secret Service");
            println!("  ai use <provider> [model]             Choose the provider and model for the edit assistant");
            println!("  ai edit <photo> [--prompt P] [--recipe JSON]  Let the chosen model edit the photo");
            println!("  xmp export <photo> [--out F] [--force] Write the edit, rating and label as an XMP sidecar");
            println!("  xmp import <photo> [--from F] [--save] Read an XMP sidecar from Lightroom, Capture One, darktable");
            println!("  mcp                                   Model Context Protocol server on stdin/stdout for AI agents");
            println!("  gpu                                   GPU used for demosaicing (OMASTUDIO_GPU=off uses the CPU)");
            println!("  icc list                              List all detected system and bundled ICC color profiles");
            println!("  scan [dir]                            Scan local folder for supported RAW photos");
            println!("  catalog                               Retrieve persistent photo catalog database");
            println!("  dag                                   Inspect Demand-Driven Tiled DAG Engine status");
            println!("  daemon                                Start persistent JSON-over-stdin processing daemon");
            println!("  theme                                 Display current Omarchy system theme and colors");
            println!("  version                               Print engine version");
            println!("  help                                  Show this help reference");
        }
        "theme" => {
            let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let theme_name_path = PathBuf::from(&home).join(".local/state/omarchy/current/theme.name");
            let colors_path = PathBuf::from(&home).join(".local/state/omarchy/current/theme/colors.toml");

            let theme_name = security::read_secure_file(&theme_name_path, 1024)
                .map(|b| String::from_utf8_lossy(&b).trim().to_string())
                .unwrap_or_else(|_| "default".to_string());

            let mut colors = std::collections::HashMap::new();
            if let Ok(bytes) = security::read_secure_file(&colors_path, 64 * 1024) {
                let content = String::from_utf8_lossy(&bytes);
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = trimmed.split_once('=') {
                        let key = k.trim().to_lowercase();
                        let val_raw = v.trim();
                        let val_clean = if let Some(first_quote) = val_raw.find('"') {
                            if let Some(second_quote) = val_raw[first_quote + 1..].find('"') {
                                &val_raw[first_quote + 1..first_quote + 1 + second_quote]
                            } else {
                                val_raw.trim_matches('"').trim_matches('\'')
                            }
                        } else if let Some(first_quote) = val_raw.find('\'') {
                            if let Some(second_quote) = val_raw[first_quote + 1..].find('\'') {
                                &val_raw[first_quote + 1..first_quote + 1 + second_quote]
                            } else {
                                val_raw.trim_matches('"').trim_matches('\'')
                            }
                        } else {
                            val_raw.split_whitespace().next().unwrap_or(val_raw)
                        };
                        colors.insert(key, val_clean.to_string());
                    }
                }
            }

            #[derive(Serialize)]
            struct ThemeInfo {
                theme: String,
                colors_path: String,
                colors: std::collections::HashMap<String, String>,
            }

            print_json(&ResponseWrapper::ok(ThemeInfo {
                theme: theme_name,
                colors_path: colors_path.to_string_lossy().to_string(),
                colors,
            }));
        }
        "daemon" => {
            run_daemon();
        }
        cmd => {
            print_json::<()>(&ResponseWrapper::err(format!("Unknown command: {}", cmd)));
        }
    }
}

fn inspect_file(raw_path: &str) -> Result<InspectResult, String> {
    let raw = RawImage::open(raw_path)?;
    let meta = raw.get_metadata()?;

    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let thumb_dir = PathBuf::from(home).join(".cache/omastudio/thumbnails");
    security::ensure_secure_dir(&thumb_dir)
        .map_err(|e| format!("Thumbnail dir error: {}", e))?;

    let file_stem = Path::new(raw_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    let thumb_path = thumbnail_path_for(Path::new(raw_path));

    // Fast thumbnail extraction
    let _ = raw.extract_thumbnail(&thumb_path);

    // Load existing sidecar or create default
    let recipe = Recipe::load_initial(raw_path);

    // Fast preview for AI scene analysis
    let preview = raw.process_preview(true)?;
    let scene = ai_classify_scene(preview.as_slice(), preview.width, preview.height, preview.channels, &meta);

    // Update catalog
    let mut catalog = Catalog::load();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Preserve culling data (rating / flag / colour label) already stored for this photo
    let prev = catalog.get(raw_path).cloned();
    catalog.upsert(CatalogItem {
        id: file_stem.to_string(),
        path: raw_path.to_string(),
        remote_path: prev.as_ref().and_then(|p| p.remote_path.clone()),
        thumbnail_path: thumb_path.to_string_lossy().to_string(),
        metadata: meta.clone(),
        rating: prev.as_ref().map_or(0, |p| p.rating),
        color_label: prev.as_ref().map_or_else(String::new, |p| p.color_label.clone()),
        flag: prev.as_ref().map_or_else(|| "unflagged".to_string(), |p| p.flag.clone()),
        recipe: recipe.clone(),
        updated_at: now,
    });
    let _ = catalog.save();

    Ok(InspectResult {
        path: raw_path.to_string(),
        metadata: meta,
        thumbnail: thumb_path.to_string_lossy().to_string(),
        recipe,
        scene,
    })
}

fn render_file(raw_path: &str, recipe: &Recipe, split: f32, out_path: &str) -> Result<RenderResult, String> {
    let raw = RawImage::open(raw_path)?;
    let preview = raw.process_preview_16(true)?;

    let (final_buf, hist) = if split > 0.001 {
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

    // Write output image (PPM or PNG/JPG)
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
        ).ok_or_else(|| "Failed to create ImageBuffer".to_string())?;
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

    Ok(RenderResult {
        image: out_path.to_string(),
        histogram: hist,
    })
}

fn run_ai_auto(raw_path: &str) -> Result<InspectResult, String> {
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

fn run_ai_jev(raw_path: &str) -> Result<JevInspectResult, String> {
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
        scene.description = format!("Jev Offline Expert [Score: {:.1}/10, Contrast: {}]", offline_jev.aesthetic_score, offline_jev.recommended_contrast);
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

fn run_ai_social(raw_path: &str, platform: &str) -> Result<ai::SocialOptimizationResult, String> {
    let raw = RawImage::open(raw_path)?;
    let preview = raw.process_preview(true)?;
    let sidecar = Recipe::load_initial(raw_path);
    let result = ai_optimize_for_social(
        preview.as_slice(),
        preview.width,
        preview.height,
        preview.channels,
        platform,
        &sidecar,
    );
    Ok(result)
}

#[derive(Deserialize)]
struct DaemonCommand {
    cmd: String, // "load", "adjust", "ai_auto", "ai_social", "save_recipe", "export", "ping", "exit"
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    recipe: Option<Recipe>,
    #[serde(default)]
    split: Option<f32>,
    #[serde(default)]
    out: Option<String>,
    #[serde(default)]
    options: Option<ExportOptions>,
    #[serde(default)]
    highlight_mask: Option<bool>,
    #[serde(default)]
    shadow_mask: Option<bool>,
    #[serde(default)]
    pub items: Option<Vec<BatchExportItem>>,
    #[serde(default)]
    pub rating: Option<u8>,
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub color_label: Option<String>,
    #[serde(default)]
    pub search_query: Option<String>,
    /// `detail`: normalised x, y, width, height of the photo to render.
    #[serde(default)]
    region: Option<[f64; 4]>,
    /// `detail`: size of the rendered image in device pixels.
    #[serde(default)]
    out_width: Option<u32>,
    #[serde(default)]
    out_height: Option<u32>,
}

#[derive(Serialize)]
struct FujiRecipeResult {
    #[serde(flatten)]
    profile: fuji::fp1::Profile,
    omastudio: Recipe,
}

impl FujiRecipeResult {
    fn new(profile: fuji::fp1::Profile) -> Self {
        let omastudio = profile.recipe.to_recipe();
        Self { profile, omastudio }
    }
}

/// `fuji-recipe <file>`, `fuji-recipe list`, `fuji-recipe save <file> [--label L]`,
/// `fuji-recipe export <file> <out.fp1> [--label L] [--device D]`.
fn run_fuji_recipe(args: &[String]) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let import = |path: &str| -> Result<fuji::fp1::Profile, String> {
        let mut profile = fuji::library::import(Path::new(path))?;
        if let Some(label) = flag("--label") {
            profile.label = label;
        }
        if let Some(device) = flag("--device") {
            profile.device = device;
        }
        Ok(profile)
    };
    let result: Result<serde_json::Value, String> = match args.first().map(String::as_str) {
        None => Err("Usage: fuji-recipe <raf|jpg|fp1|txt> | list | save <file> | export <file> <out.fp1>".to_string()),
        Some("list") => serde_json::to_value(
            fuji::library::list()
                .into_iter()
                .map(|e| serde_json::json!({ "path": e.path, "recipe": FujiRecipeResult::new(e.profile) }))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string()),
        Some("save") => match args.get(1) {
            Some(src) => import(src)
                .and_then(|p| fuji::library::save(&p))
                .map(|path| serde_json::json!({ "path": path })),
            None => Err("Missing recipe source file".to_string()),
        },
        Some("export") => match (args.get(1), args.get(2)) {
            (Some(src), Some(out)) => import(src)
                .and_then(|p| fuji::library::export(&p, Path::new(out)))
                .map(|_| serde_json::json!({ "path": out })),
            _ => Err("Usage: fuji-recipe export <file> <out.fp1> [--label L] [--device D]".to_string()),
        },
        Some(path) => import(path).and_then(|p| serde_json::to_value(FujiRecipeResult::new(p)).map_err(|e| e.to_string())),
    };
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
    }
}

/// `fuji-camera list | info | presets | convert <raf> [--recipe <file>] [--out <jpg>] |
/// write-preset <slot> <file> [--label L]`: Fujifilm cameras on USB.
fn run_fuji_camera(args: &[String]) {
    use fuji::ptp::Camera;
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let result: Result<serde_json::Value, String> = (|| {
        let sub = args.first().map(String::as_str).unwrap_or("info");
        if sub == "list" {
            let cams = ptp::usb::list_cameras(Some(fuji::ptp::VENDOR_ID)).map_err(|e| e.to_string())?;
            return serde_json::to_value(cams).map_err(|e| e.to_string());
        }
        let mut cam = Camera::connect().map_err(|e| e.to_string())?;
        let out = match sub {
            "info" => Ok(serde_json::json!({ "device": cam.info, "raw_conversion": cam.can_convert() })),
            "presets" => cam.presets().map_err(|e| e.to_string()).map(|presets| {
                presets
                    .into_iter()
                    .map(|p| {
                        let omastudio = p.recipe.to_recipe();
                        serde_json::json!({ "slot": p.slot, "name": p.name, "recipe": p.recipe, "omastudio": omastudio })
                    })
                    .collect()
            }),
            "convert" => {
                let raf_path = args.get(1).ok_or("Missing RAF file")?;
                let raf = security::read_secure_file(Path::new(raf_path), 1024 * 1024 * 1024).map_err(|e| format!("Cannot read {}: {}", raf_path, e))?;
                let recipe = match flag("--recipe") {
                    Some(f) => Some(fuji::library::import(Path::new(&f))?.recipe),
                    None => None,
                };
                let out_path = flag("--out").unwrap_or_else(|| {
                    Path::new(raf_path).with_extension("camera.jpg").to_string_lossy().into_owned()
                });
                let jpeg = cam.convert_raf(&raf, recipe.as_ref()).map_err(|e| e.to_string())?;
                security::atomic_write_user(Path::new(&out_path), &jpeg).map_err(|e| format!("Cannot write {}: {}", out_path, e))?;
                Ok(serde_json::json!({ "path": out_path, "bytes": jpeg.len() }))
            }
            "write-preset" => {
                let slot: u8 = args.get(1).and_then(|s| s.trim_start_matches(['C', 'c']).parse().ok()).ok_or("Slot must be 1-7 (or C1-C7)")?;
                let src = args.get(2).ok_or("Missing recipe file")?;
                let profile = fuji::library::import(Path::new(src))?;
                let name = flag("--label").unwrap_or(profile.label);
                cam.write_preset(slot, &name, &profile.recipe)
                    .map_err(|e| e.to_string())
                    .map(|refused| serde_json::json!({ "slot": slot, "name": name, "refused_properties": refused }))
            }
            other => Err(format!("Unknown fuji-camera command: {}", other)),
        };
        cam.close();
        out
    })();
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
    }
}

/// `xmp export <photo> [--recipe JSON] [--out F] [--force]` and
/// `xmp import <photo> [--from F] [--save]`: XMP sidecars for other editors.
fn run_xmp(args: &[String]) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let has = |name: &str| args.iter().any(|a| a == name);
    let result: Result<serde_json::Value, String> = (|| {
        let photo = args.get(1).ok_or("Usage: xmp export|import <photo>")?;
        let path = Path::new(photo);
        match args.first().map(String::as_str) {
            Some("export") => {
                let recipe = match flag("--recipe") {
                    Some(j) => serde_json::from_str(&j).map_err(|e| format!("Invalid recipe: {e}"))?,
                    None => Recipe::load_initial(path),
                };
                let item = Catalog::load().get(photo).cloned();
                let culling = xmp::Culling {
                    rating: match &item {
                        Some(i) if i.flag == "reject" => -1,
                        Some(i) => i.rating as i32,
                        None => recipe.rating as i32,
                    },
                    label: item.map(|i| i.color_label).unwrap_or_default(),
                };
                let out = flag("--out").map(PathBuf::from);
                let written = xmp::export(path, &recipe, &culling, out.as_deref(), has("--force"))?;
                Ok(serde_json::json!({ "path": written }))
            }
            Some("import") => {
                let from = flag("--from").map(PathBuf::from);
                let (recipe, culling, source) = xmp::read(path, from.as_deref())?;
                let mut saved = None;
                if has("--save") {
                    saved = Some(recipe.save_sidecar(path).map_err(|e| e.to_string())?);
                    let mut cat = Catalog::load();
                    let flag = (culling.rating < 0).then_some("reject");
                    let label = (!culling.label.is_empty()).then_some(culling.label.as_str());
                    if cat.update_culling(photo, Some(culling.rating.clamp(0, 5) as u8), flag, label) {
                        let _ = cat.save();
                    }
                }
                Ok(serde_json::json!({ "source": source, "recipe": recipe, "culling": culling, "sidecar": saved }))
            }
            _ => Err("Usage: xmp export|import <photo>".to_string()),
        }
    })();
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
    }
}

/// `ai providers | key <provider> [--delete] | use <provider> [model] [--endpoint URL] |
/// edit <photo> [--prompt P] [--recipe JSON] [--provider X] [--model M]`.
fn run_ai(args: &[String]) {
    use ai::provider;
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let lookup = |id: Option<&String>| -> Result<&'static provider::Provider, String> {
        let id = id.ok_or("Missing provider: anthropic, openai, openrouter, gemini, ollama or lmstudio")?;
        provider::find(id).ok_or_else(|| format!("Unknown provider {id}"))
    };
    let result: Result<serde_json::Value, String> = match args.first().map(String::as_str) {
        Some("providers") | None => Ok(serde_json::json!({ "providers": provider::status(), "choice": provider::load_choice() })),
        Some("key") => lookup(args.get(1)).and_then(|p| {
            if args.iter().any(|a| a == "--delete") {
                return provider::delete_key(p).map(|_| serde_json::json!({ "provider": p.id, "stored": false }));
            }
            // The key comes on stdin so it never shows up in the process list
            let mut key = String::new();
            io::Read::take(io::stdin().lock(), 4096).read_line(&mut key).map_err(|e| e.to_string())?;
            provider::store_key(p, &key).map(|_| serde_json::json!({ "provider": p.id, "stored": true }))
        }),
        Some("use") => lookup(args.get(1)).and_then(|p| {
            let model = args.get(2).filter(|m| !m.starts_with("--")).cloned().unwrap_or_default();
            let choice = provider::Choice { provider: p.id.to_string(), model, endpoint: flag("--endpoint") };
            provider::save_choice(&choice).and_then(|_| serde_json::to_value(&choice).map_err(|e| e.to_string()))
        }),
        Some("edit") => match args.get(1) {
            Some(photo) => ai_edit(photo, flag("--prompt").as_deref().unwrap_or(""), flag("--recipe"), flag("--provider"), flag("--model")),
            None => Err("Usage: ai edit <photo> [--prompt P] [--recipe JSON]".to_string()),
        },
        Some(other) => Err(format!("Unknown ai command: {other}")),
    };
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
    }
}

/// Shows the photo, as developed with `recipe`, to the chosen model and applies its edit.
fn ai_edit(photo: &str, prompt: &str, recipe: Option<String>, provider: Option<String>, model: Option<String>) -> Result<serde_json::Value, String> {
    use ai::{assistant, provider as prov};
    let current: Recipe = match recipe {
        Some(r) => serde_json::from_str(&r).map_err(|e| format!("Invalid recipe: {e}"))?,
        None => Recipe::default(),
    };
    let target = prov::resolve(provider.as_deref(), model.as_deref())?;

    let ppm = security::get_secure_preview_dir().join(format!("omastudio_ai_{}.ppm", std::process::id()));
    let rendered = render_file(photo, &current, 0.0, &ppm.to_string_lossy());
    let image = rendered.as_ref().map_err(Clone::clone).and_then(|_| image::open(&ppm).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&ppm);
    let (rendered, image) = (rendered?, image?);
    let image = image.thumbnail(1280, 1280).to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode_image(&image)
        .map_err(|e| e.to_string())?;

    let meta = RawImage::open(photo).ok().and_then(|r| r.get_metadata().ok());
    let camera = match meta {
        Some(m) => serde_json::json!({
            "camera": format!("{} {}", m.make, m.model).trim(), "lens": m.lens, "iso": m.iso,
            "shutter_s": m.shutter, "aperture": m.aperture, "focal_length_mm": m.focal_length,
            "shadow_clipping_percent": rendered.histogram.shadow_clipping_percent,
            "highlight_clipping_percent": rendered.histogram.highlight_clipping_percent,
        }),
        None => serde_json::json!({}),
    };
    let answer = prov::ask_json(&target, assistant::SYSTEM, &jpeg, &assistant::request_text(prompt, &current, &camera), &assistant::schema())?;
    let (recipe, explanation) = assistant::merge(&current, &answer);
    Ok(serde_json::json!({
        "recipe": recipe,
        "explanation": explanation,
        "provider": target.provider.id,
        "model": target.model,
    }))
}

fn run_daemon() {
    let cache = Arc::new(Mutex::new(DaemonCache {
        current_path: None,
        raw_buffer_16: None,
        raw_buffer_8: None,
        transfer: Transfer::default(),
        width: 0,
        height: 0,
        channels: 0,
        metadata: None,
        ping_pong: 0,
        full: None,
        detail_ping_pong: 0,
    }));

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut raw_buf: Vec<u8> = Vec::new();
    const MAX_LINE_BYTES: usize = 1024 * 1024; // 1 MiB JSON line limit

    loop {
        raw_buf.clear();
        // Bounded byte read: an oversized line never lands in memory whole, and a stray
        // invalid UTF-8 byte yields an error response instead of silently ending the daemon.
        let bytes_read = match io::Read::take(&mut stdin_lock, MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut raw_buf)
        {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };

        if bytes_read > MAX_LINE_BYTES {
            // Discard the rest of the oversized line
            if raw_buf.last() != Some(&b'\n') {
                let mut sink = Vec::new();
                loop {
                    sink.clear();
                    match io::Read::take(&mut stdin_lock, 64 * 1024).read_until(b'\n', &mut sink) {
                        Ok(0) => break,
                        Ok(_) if sink.last() == Some(&b'\n') => break,
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
            }
            print_json::<()>(&ResponseWrapper::err_action(
                "error",
                "JSON line exceeded maximum limit of 1 MiB",
            ));
            continue;
        }

        let line_buf = match std::str::from_utf8(&raw_buf) {
            Ok(s) => s,
            Err(_) => {
                print_json::<()>(&ResponseWrapper::err_action("error", "Command line is not valid UTF-8"));
                continue;
            }
        };

        let trimmed = line_buf.trim();
        if trimmed.is_empty() {
            continue;
        }

        let cmd_obj: DaemonCommand = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                print_json::<()>(&ResponseWrapper::err_action("error", format!("Invalid JSON command: {}", e)));
                continue;
            }
        };

        match cmd_obj.cmd.as_str() {
            "ping" => {
                print_json(&ResponseWrapper::ok_action("ping", "pong"));
            }
            "load" => {
                let p = match cmd_obj.path {
                    Some(ref path) => path,
                    None => {
                        print_json::<()>(&ResponseWrapper::err_action("load", "Missing path in load command"));
                        continue;
                    }
                };

                match RawImage::open(p) {
                    Ok(raw) => {
                        let meta = raw.get_metadata().unwrap_or(raw::RawMetadata {
                            width: 0,
                            height: 0,
                            raw_width: 0,
                            raw_height: 0,
                            make: String::new(),
                            model: String::new(),
                            lens: String::new(),
                            iso: 0.0,
                            shutter: 0.0,
                            aperture: 0.0,
                            focal_length: 0.0,
                            timestamp: 0,
                            cam_mul: [1.0, 1.0, 1.0, 1.0],
                        });

                        match raw.process_preview_16(true) {
                            Ok(prev) => {
                                let u16_slice = prev.as_slice_u16();
                                let u16_vec = Arc::new(u16_slice.to_vec());
                                let transfer = prev.transfer;
                                let u8_vec: Vec<u8> = u16_slice
                                    .iter()
                                    .map(|&x| (transfer.encode(x as f32 / 65535.0).clamp(0.0, 1.0) * 255.0).round() as u8)
                                    .collect();
                                let u8_arc = Arc::new(u8_vec);

                                let mut c = cache.lock().unwrap();
                                c.current_path = Some(p.clone());
                                c.width = prev.width;
                                c.height = prev.height;
                                c.channels = prev.channels;
                                c.raw_buffer_16 = Some(u16_vec);
                                c.raw_buffer_8 = Some(Arc::clone(&u8_arc));
                                c.transfer = transfer;
                                c.metadata = Some(meta.clone());
                                c.full = None;
                                drop(c);
                                spawn_full_decode(Arc::clone(&cache), p.clone());
                                // Keep ping_pong running across loads: resetting it made the first
                                // render of every photo reuse the same preview path, so the UI saw
                                // no source change and kept showing the previous photo.

                                let thumb_path = thumbnail_path_for(Path::new(p));
                                let _ = raw.extract_thumbnail(&thumb_path);

                                let sidecar = Recipe::load_initial(p);
                                let scene = ai_classify_scene(&u8_arc, prev.width, prev.height, prev.channels, &meta);

                                print_json(&ResponseWrapper::ok_action("load", InspectResult {
                                    path: p.clone(),
                                    metadata: meta,
                                    thumbnail: thumb_path.to_string_lossy().to_string(),
                                    recipe: sidecar,
                                    scene,
                                }));
                            }
                            Err(e) => print_json::<()>(&ResponseWrapper::err_action("load", e)),
                        }
                    }
                    Err(e) => print_json::<()>(&ResponseWrapper::err_action("load", e)),
                }
            }
            "adjust" => {
                let mut c = cache.lock().unwrap();
                if c.raw_buffer_16.is_none() {
                    print_json::<()>(&ResponseWrapper::err_action("adjust", "No RAW image currently loaded in daemon cache"));
                    continue;
                }

                let width = c.width;
                let height = c.height;
                let channels = c.channels;
                let transfer = c.transfer;
                let recipe = cmd_obj.recipe.unwrap_or_default();
                let split = cmd_obj.split.unwrap_or(0.0);

                let next_slot = (c.ping_pong + 1) % 2;
                c.ping_pong = next_slot;

                let buffer_arc = match c.raw_buffer_16.as_ref() {
                    Some(b) => Arc::clone(b),
                    None => {
                        print_json::<()>(&ResponseWrapper::err_action("adjust", "Buffer unavailable"));
                        continue;
                    }
                };

                // Drop cache lock immediately so other daemon queries or inspection are never blocked
                drop(c);

                let base_dir = security::get_secure_preview_dir();

                let out_dest = cmd_obj.out.unwrap_or_else(|| {
                    base_dir.join(format!("omastudio_preview_{}.ppm", next_slot)).to_string_lossy().to_string()
                });

                let buffer = buffer_arc.as_ref();

                let highlight_mask = cmd_obj.highlight_mask.unwrap_or(false);
                let shadow_mask = cmd_obj.shadow_mask.unwrap_or(false);

                let (final_buf, hist) = if split > 0.001 {
                    process_split_comparison_16_to_8_ex(
                        buffer,
                        width,
                        height,
                        channels,
                        &recipe,
                        transfer,
                        split,
                        highlight_mask,
                        shadow_mask,
                    )
                } else {
                    process_buffer_16_to_8_ex(
                        buffer,
                        width,
                        height,
                        channels,
                        &recipe,
                        transfer,
                        highlight_mask,
                        shadow_mask,
                    )
                };

                let dest_path = Path::new(&out_dest);
                let write_result: Result<(), String> = if out_dest.ends_with(".ppm") {
                    let mut ppm_bytes = format!("P6\n{} {}\n255\n", width, height).into_bytes();
                    ppm_bytes.extend_from_slice(&final_buf);
                    security::atomic_write_secure(dest_path, &ppm_bytes).map_err(|e| e.to_string())
                } else {
                    match image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(width, height, final_buf) {
                        Some(img) => {
                            let mut encoded = Vec::new();
                            let fmt = if out_dest.ends_with(".png") {
                                image::ImageFormat::Png
                            } else {
                                image::ImageFormat::Jpeg
                            };
                            img.write_to(&mut std::io::Cursor::new(&mut encoded), fmt)
                                .map_err(|e| e.to_string())
                                .and_then(|_| security::atomic_write_secure(dest_path, &encoded).map_err(|e| e.to_string()))
                        }
                        None => Err("Rendered buffer size does not match image dimensions".to_string()),
                    }
                };

                // Report write failures instead of handing the UI a stale preview path
                if let Err(e) = write_result {
                    print_json::<()>(&ResponseWrapper::err_action("adjust", format!("Failed to write preview: {}", e)));
                    continue;
                }

                print_json(&ResponseWrapper::ok_action("adjust", RenderResult {
                    image: out_dest,
                    histogram: hist,
                }));
            }
            "detail" => {
                let (full, slot) = {
                    let mut c = cache.lock().unwrap();
                    c.detail_ping_pong = (c.detail_ping_pong + 1) % 2;
                    (c.full.clone(), c.detail_ping_pong)
                };
                let Some(full) = full else {
                    print_json::<()>(&ResponseWrapper::err_action("detail", "Full resolution not ready"));
                    continue;
                };
                let [rx, ry, rw, rh] = cmd_obj.region.unwrap_or([0.0, 0.0, 1.0, 1.0]);
                let (ow, oh) = (cmd_obj.out_width.unwrap_or(0) as usize, cmd_obj.out_height.unwrap_or(0) as usize);
                if !(rw > 0.0 && rh > 0.0) || ow == 0 || oh == 0 || ow * oh > 64 << 20 {
                    print_json::<()>(&ResponseWrapper::err_action("detail", "Invalid detail region"));
                    continue;
                }
                // The frame at the requested scale; the region is cut from it
                let scale = (ow as f64 / (rw * full.width as f64)).min(8.0);
                let (vw, vh) = ((full.width as f64 * scale).round() as usize, (full.height as f64 * scale).round() as usize);
                let ox = ((rx * vw as f64).round().max(0.0) as usize).min(vw.saturating_sub(1));
                let oy = ((ry * vh as f64).round().max(0.0) as usize).min(vh.saturating_sub(1));
                let (ow, oh) = (ow.min(vw - ox), oh.min(vh - oy));
                let recipe = cmd_obj.recipe.unwrap_or_default();
                let out = (ox, oy, ow, oh);
                let src = pipeline::region_source(out, vw, vh, &recipe);
                let input = resample_rect(&full, scale, src);
                let window = pipeline::Window { x0: src.0, y0: src.1, frame_w: vw, frame_h: vh };
                let rgb = pipeline::render_region_8(&input, src.2, src.3, window, out, &recipe, full.transfer);

                let dest = security::get_secure_preview_dir().join(format!("omastudio_detail_{}.ppm", slot));
                let mut ppm = format!("P6\n{} {}\n255\n", ow, oh).into_bytes();
                ppm.extend_from_slice(&rgb);
                if let Err(e) = security::atomic_write_secure(&dest, &ppm) {
                    print_json::<()>(&ResponseWrapper::err_action("detail", format!("Failed to write detail view: {}", e)));
                    continue;
                }
                print_json(&ResponseWrapper::ok_action("detail", DetailResult {
                    image: dest.to_string_lossy().to_string(),
                    region: [ox as f64 / vw as f64, oy as f64 / vh as f64, ow as f64 / vw as f64, oh as f64 / vh as f64],
                    width: ow,
                    height: oh,
                }));
            }
            "ai_auto" => {
                let c = cache.lock().unwrap();
                if let (Some(buffer), Some(meta)) = (&c.raw_buffer_8, &c.metadata) {
                    let auto_recipe = ai_auto_enhance(buffer, c.width, c.height, c.channels, meta);
                    let scene = ai_classify_scene(buffer, c.width, c.height, c.channels, meta);
                    let path_str = c.current_path.clone().unwrap_or_default();
                    print_json(&ResponseWrapper::ok_action("ai_auto", InspectResult {
                        path: path_str,
                        metadata: meta.clone(),
                        thumbnail: String::new(),
                        recipe: auto_recipe,
                        scene,
                    }));
                } else {
                    print_json::<()>(&ResponseWrapper::err_action("ai_auto", "No image loaded in cache for AI Auto"));
                }
            }
            "ai_jev" => {
                let c = cache.lock().unwrap();
                if let (Some(buffer), Some(meta)) = (&c.raw_buffer_8, &c.metadata) {
                    let mut auto_recipe = ai_auto_enhance(buffer, c.width, c.height, c.channels, meta);
                    let (mut scene, mut jev_opt) = ai_classify_scene_with_jev(buffer, c.width, c.height, c.channels, meta);
                    if jev_opt.is_none() {
                        let state = ai::jev::extract_photographic_state(buffer, c.width, c.height, c.channels, meta);
                        let offline_jev = ai::jev::generate_offline_jev_decisions(&state, meta);
                        apply_jev_decisions_to_recipe(&mut auto_recipe, &offline_jev);
                        scene.recommended_preset = offline_jev.recommended_preset.clone();
                        scene.description = format!("Jev Offline Expert [Score: {:.1}/10, Contrast: {}]", offline_jev.aesthetic_score, offline_jev.recommended_contrast);
                        jev_opt = Some(offline_jev);
                    } else if let Some(ref jev) = jev_opt {
                        apply_jev_decisions_to_recipe(&mut auto_recipe, jev);
                    }
                    let path_str = c.current_path.clone().unwrap_or_default();
                    print_json(&ResponseWrapper::ok_action("ai_jev", JevInspectResult {
                        path: path_str,
                        metadata: meta.clone(),
                        thumbnail: String::new(),
                        recipe: auto_recipe,
                        scene,
                        jev_decisions: jev_opt,
                    }));
                } else {
                    print_json::<()>(&ResponseWrapper::err_action("ai_jev", "No image loaded in cache for AI Jev"));
                }
            }
            "ai_social" => {
                let platform = cmd_obj.platform.unwrap_or_else(|| "ig".to_string());
                let c = cache.lock().unwrap();
                if let Some(ref buffer) = c.raw_buffer_8 {
                    let sidecar = c.current_path.as_ref()
                        .map(Recipe::load_initial)
                        .unwrap_or_default();
                    let result = ai_optimize_for_social(
                        buffer,
                        c.width,
                        c.height,
                        c.channels,
                        &platform,
                        &sidecar,
                    );
                    print_json(&ResponseWrapper::ok_action("ai_social", result));
                } else {
                    print_json::<()>(&ResponseWrapper::err_action("ai_social", "No image loaded in cache for AI Social"));
                }
            }
            "save_recipe" => {
                let c = cache.lock().unwrap();
                if let (Some(p), Some(recipe)) = (&c.current_path, &cmd_obj.recipe) {
                    match recipe.save_sidecar(p) {
                        Ok(sidecar_path) => print_json(&ResponseWrapper::ok_action("save_recipe", sidecar_path.to_string_lossy().to_string())),
                        Err(e) => print_json::<()>(&ResponseWrapper::err_action("save_recipe", format!("Failed to save sidecar: {}", e))),
                    }
                } else {
                    print_json::<()>(&ResponseWrapper::err_action("save_recipe", "No image loaded or recipe missing"));
                }
            }
            "export" => {
                let c = cache.lock().unwrap();
                let p = match &c.current_path {
                    Some(p) => p.clone(),
                    None => {
                        print_json::<()>(&ResponseWrapper::err_action("export", "No image loaded"));
                        continue;
                    }
                };
                let recipe = cmd_obj.recipe.unwrap_or_default();
                let options = cmd_obj.options.unwrap_or_default();
                drop(c); // release lock during full export

                match export_photo(&p, &recipe, &options) {
                    Ok(dest) => print_json(&ResponseWrapper::ok_action("export", ExportResult {
                        exported_path: dest.to_string_lossy().to_string(),
                    })),
                    Err(e) => print_json::<()>(&ResponseWrapper::err_action("export", e)),
                }
            }
            "batch_export" => {
                let items = match cmd_obj.items {
                    Some(its) if !its.is_empty() => its,
                    _ => {
                        print_json::<()>(&ResponseWrapper::err_action("batch_export", "No items provided for batch export"));
                        continue;
                    }
                };

                if items.len() > 100 {
                    print_json::<()>(&ResponseWrapper::err_action(
                        "batch_export",
                        "Batch export exceeds maximum limit of 100 items",
                    ));
                    continue;
                }
                let options = cmd_obj.options.unwrap_or_default();
                let item_paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
                let name_tags = export::batch_name_tags(&item_paths);

                // Multi-threaded parallel batch export using Rayon
                let results: Vec<BatchExportItemResult> = items
                    .par_iter()
                    .zip(name_tags.par_iter())
                    .map(|(item, tag)| {
                        let recipe = item.recipe.clone().unwrap_or_else(|| Recipe::load_initial(&item.path));
                        let mut item_options = options.clone();
                        item_options.name_tag = tag.clone();
                        match export_photo(&item.path, &recipe, &item_options) {
                            Ok(dest) => BatchExportItemResult {
                                path: item.path.clone(),
                                success: true,
                                exported_path: Some(dest.to_string_lossy().to_string()),
                                error: None,
                            },
                            Err(e) => BatchExportItemResult {
                                path: item.path.clone(),
                                success: false,
                                exported_path: None,
                                error: Some(e),
                            },
                        }
                    })
                    .collect();

                let succeeded = results.iter().filter(|r| r.success).count();
                let failed = results.len() - succeeded;

                print_json(&ResponseWrapper::ok_action("batch_export", BatchExportResult {
                    total: results.len(),
                    succeeded,
                    failed,
                    results,
                }));
            }
            "fuji_recipes" => {
                let list: Vec<_> = fuji::library::list()
                    .into_iter()
                    .map(|e| serde_json::json!({ "path": e.path, "recipe": FujiRecipeResult::new(e.profile) }))
                    .collect();
                print_json(&ResponseWrapper::ok_action("fuji_recipes", list));
            }
            "fuji_import" => match cmd_obj.path.as_deref().map(|p| fuji::library::import(Path::new(p))) {
                Some(Ok(profile)) => print_json(&ResponseWrapper::ok_action("fuji_import", FujiRecipeResult::new(profile))),
                Some(Err(e)) => print_json::<()>(&ResponseWrapper::err_action("fuji_import", e)),
                None => print_json::<()>(&ResponseWrapper::err_action("fuji_import", "Missing path")),
            },
            "catalog_query" => {
                let cat = Catalog::load();
                let filter = recipe::CatalogFilter {
                    min_rating: cmd_obj.rating,
                    flag: cmd_obj.flag.clone(),
                    camera_make: None,
                    camera_model: None,
                    lens: None,
                    min_iso: None,
                    max_iso: None,
                    search_query: cmd_obj.search_query.clone(),
                };
                let results = cat.filter_items(&filter);
                print_json(&ResponseWrapper::ok_action("catalog_query", results));
            }
            "catalog_cull" => {
                let target_path = match cmd_obj.path {
                    Some(ref p) => p.clone(),
                    None => {
                        let c = cache.lock().unwrap();
                        match c.current_path.as_ref() {
                            Some(p) => p.clone(),
                            None => {
                                print_json::<()>(&ResponseWrapper::err_action("catalog_cull", "No path specified and no image loaded"));
                                continue;
                            }
                        }
                    }
                };

                let mut cat = Catalog::load();
                let updated = cat.update_culling(
                    &target_path,
                    cmd_obj.rating,
                    cmd_obj.flag.as_deref(),
                    cmd_obj.color_label.as_deref(),
                );

                if updated {
                    let _ = cat.save();
                    print_json(&ResponseWrapper::ok_action("catalog_cull", "Culling metadata saved successfully"));
                } else {
                    print_json::<()>(&ResponseWrapper::err_action("catalog_cull", "Item not found in catalog"));
                }
            }
            "exit" => {
                print_json(&ResponseWrapper::ok_action("exit", "Daemon exiting"));
                break;
            }
            unknown => {
                print_json::<()>(&ResponseWrapper::err_action("error", format!("Unknown daemon command: {}", unknown)));
            }
        }
    }
}
