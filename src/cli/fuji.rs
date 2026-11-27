//! Fujifilm CLI commands (`fuji-recipe`, `fuji-camera`, `fuji-pixel-shift`, `fuji-compat`, `fuji-calibrate`).

use std::path::{Path, PathBuf};

use omastudio_engine::{fuji, ptp, recipe::Recipe, security};
use serde::Serialize;

use super::{ResponseWrapper, print_json};

#[derive(Serialize)]
pub struct FujiRecipeResult {
    #[serde(flatten)]
    pub profile: fuji::fp1::Profile,
    pub omastudio: Recipe,
}

impl FujiRecipeResult {
    pub fn new(profile: fuji::fp1::Profile) -> Self {
        let omastudio = profile.recipe.to_recipe();
        Self { profile, omastudio }
    }
}

/// `fuji-recipe <file>`, `fuji-recipe list`, `fuji-recipe save <file> [--label L]`,
/// `fuji-recipe export <file> <out.fp1> [--label L] [--device D]`.
pub fn run_recipe(args: &[String]) {
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
pub fn run_camera(args: &[String]) {
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
                let raf = security::read_secure_file(Path::new(raf_path), 1024 * 1024 * 1024)
                    .map_err(|e| format!("Cannot read {}: {}", raf_path, e))?;
                let recipe = match flag("--recipe") {
                    Some(f) => Some(fuji::library::import(Path::new(&f))?.recipe),
                    None => None,
                };
                let out_path = flag("--out").unwrap_or_else(|| {
                    Path::new(raf_path).with_extension("camera.jpg").to_string_lossy().into_owned()
                });
                let jpeg = cam.convert_raf(&raf, recipe.as_ref()).map_err(|e| e.to_string())?;
                security::atomic_write_user(Path::new(&out_path), &jpeg)
                    .map_err(|e| format!("Cannot write {}: {}", out_path, e))?;
                Ok(serde_json::json!({ "path": out_path, "bytes": jpeg.len() }))
            }
            "write-preset" => {
                let slot: u8 = args
                    .get(1)
                    .and_then(|s| s.trim_start_matches(['C', 'c']).parse().ok())
                    .ok_or("Slot must be 1-7 (or C1-C7)")?;
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

pub fn run_pixel_shift(args: &[String]) {
    let mut paths = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
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

pub fn run_compat(args: &[String]) {
    let paths: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    if paths.is_empty() {
        print_json::<()>(&ResponseWrapper::err("Usage: fuji-compat <raf>... [--full]"));
        return;
    }
    let full = args.iter().any(|a| a == "--full");
    print_json(&ResponseWrapper::ok(fuji::compat::check_all(&paths, full)));
}

pub fn run_calibrate(args: &[String]) {
    let paths: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    if paths.is_empty() {
        print_json::<()>(&ResponseWrapper::err("Usage: fuji-calibrate <raf>... [--any] [--dry-run]"));
        return;
    }
    let any = args.iter().any(|a| a == "--any");
    let write = !args.iter().any(|a| a == "--dry-run");
    let (reports, skipped) = fuji::calibrate::calibrate(&paths, any, write);
    print_json(&ResponseWrapper::ok(serde_json::json!({ "calibrated": reports, "skipped": skipped })));
}
