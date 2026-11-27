//! Command-line dispatcher and shared JSON response/stdin helpers for `omastudio-engine`.

pub mod ai;
pub mod catalog;
pub mod cloud;
pub mod fuji;
pub mod inspect;
pub mod medium_format;
pub mod xmp;

use std::io::{self, BufRead, Write};

use omastudio_engine::{
    export::{ExportOptions, export_photo},
    gpu, icc, ml, pipeline, raw,
    recipe::Recipe,
    security,
};
use serde::Serialize;

use crate::{bench, daemon, mcp, theme};
use crate::daemon::protocol::ExportResult;

#[derive(Serialize)]
pub struct ResponseWrapper<T: Serialize> {
    pub success: bool,
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo_path: Option<String>,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T: Serialize> ResponseWrapper<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            action: None,
            request_id: None,
            photo_path: None,
            data: Some(data),
            error: None,
        }
    }

    pub fn ok_action(action: &str, data: T) -> Self {
        Self {
            success: true,
            action: Some(action.to_string()),
            request_id: None,
            photo_path: None,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            action: None,
            request_id: None,
            photo_path: None,
            data: None,
            error: Some(msg.into()),
        }
    }

    pub fn err_action(action: &str, msg: impl Into<String>) -> Self {
        Self {
            success: false,
            action: Some(action.to_string()),
            request_id: None,
            photo_path: None,
            data: None,
            error: Some(msg.into()),
        }
    }

    pub fn with_context(mut self, request_id: Option<u64>, photo_path: Option<String>) -> Self {
        self.request_id = request_id;
        self.photo_path = photo_path;
        self
    }
}

/// One line from stdin, at most `limit` bytes, without the newline.
pub fn read_stdin_line(limit: u64) -> Result<String, String> {
    let mut buf = Vec::new();
    io::Read::take(io::stdin().lock(), limit + 1)
        .read_until(b'\n', &mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > limit {
        return Err(format!("longer than {limit} bytes"));
    }
    String::from_utf8(buf)
        .map(|s| s.trim_end().to_string())
        .map_err(|_| "not valid UTF-8".to_string())
}

/// Parses a `--recipe` value: `"-"` reads one JSON line from stdin (avoiding Linux's 128 KiB
/// single-argv ceiling on recipes with brush strokes); anything else is parsed inline.
pub fn read_recipe_arg(arg: &str) -> Result<Recipe, String> {
    let text = if arg == "-" {
        read_stdin_line(16 << 20).map_err(|e| format!("Cannot read the recipe from stdin: {e}"))?
    } else {
        arg.to_string()
    };
    serde_json::from_str::<Recipe>(&text).map_err(|e| format!("Invalid recipe: {e}"))
}

pub fn print_json<T: Serialize>(resp: &ResponseWrapper<T>) {
    if let Ok(json) = serde_json::to_string(resp) {
        print_line(&json);
    }
}

/// Writes one output line. The app may close while a long command (a cloud
/// sign-in) still runs; with nobody reading, exit quietly instead of panicking.
pub fn print_line(line: &str) {
    let mut out = io::stdout().lock();
    if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
        std::process::exit(0);
    }
}

pub fn dispatch(args: &[String]) {
    if args.len() < 2 {
        eprintln!(
            "OmaStudio Engine v{}: The Complete Medium Format RAW Editor for Omarchy",
            env!("CARGO_PKG_VERSION")
        );
        eprintln!("Usage: omastudio-engine <command> [arguments]");
        eprintln!("Commands:");
        eprintln!("  inspect <raw_file>");
        eprintln!("  render <raw_file> [--recipe <json|->] [--split <0.0-1.0>] [--out <dest>]");
        eprintln!("  ai-auto <raw_file>");
        eprintln!("  ai-jev <raw_file>");
        eprintln!("  export <raw_file> [--recipe <json|->] [--options <json>]");
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
            match inspect::inspect_file(&args[2]) {
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
            let mut out_path = security::get_secure_preview_dir()
                .join("omastudio_preview.ppm")
                .to_string_lossy()
                .to_string();

            let mut idx = 3;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--recipe" if idx + 1 < args.len() => {
                        match read_recipe_arg(&args[idx + 1]) {
                            Ok(r) => recipe = r,
                            Err(e) => {
                                print_json::<()>(&ResponseWrapper::err(e));
                                return;
                            }
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

            match inspect::render_file(raw_path, &recipe, split, &out_path) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "ai-auto" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            match inspect::run_ai_auto(&args[2]) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "ai-jev" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            match inspect::run_ai_jev(&args[2]) {
                Ok(res) => print_json(&ResponseWrapper::ok(res)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "ai-social" => {
            if args.len() < 3 {
                print_json::<()>(&ResponseWrapper::err("Missing RAW file path"));
                return;
            }
            let platform = if args.len() >= 4 { &args[3] } else { "ig" };
            match inspect::run_ai_social(&args[2], platform) {
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
                        match read_recipe_arg(&args[idx + 1]) {
                            Ok(r) => recipe = r,
                            Err(e) => {
                                print_json::<()>(&ResponseWrapper::err(e));
                                return;
                            }
                        }
                        idx += 2;
                    }
                    "--options" if idx + 1 < args.len() => {
                        match serde_json::from_str::<ExportOptions>(&args[idx + 1]) {
                            Ok(o) => options = o,
                            Err(e) => {
                                print_json::<()>(&ResponseWrapper::err(format!("Invalid export options: {e}")));
                                return;
                            }
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
        "catalog" => catalog::run(&args[2..]),
        "fuji-recipe" => fuji::run_recipe(&args[2..]),
        "fuji-camera" => fuji::run_camera(&args[2..]),
        "fuji-pixel-shift" => fuji::run_pixel_shift(&args[2..]),
        "fuji-compat" => fuji::run_compat(&args[2..]),
        "fuji-calibrate" => fuji::run_calibrate(&args[2..]),
        "medium-format" => medium_format::run(&args[2..]),
        "icc" => {
            if args.len() < 3 || args[2] == "list" {
                print_json(&ResponseWrapper::ok(icc::list_icc_profiles()));
            } else {
                print_json::<()>(&ResponseWrapper::err("Unknown icc subcommand"));
            }
        }
        "scan" => {
            let dir = if args.len() >= 3 { &args[2] } else { "." };
            match inspect::scan_directory(dir) {
                Ok(items) => print_json(&ResponseWrapper::ok(items)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "dag" => inspect::run_dag(),
        "cameras" => {
            let filter = args.get(2).map(|f| f.to_lowercase());
            let cameras: Vec<String> = raw::supported_cameras()
                .into_iter()
                .filter(|c| filter.as_ref().is_none_or(|f| c.to_lowercase().contains(f)))
                .collect();
            print_json(&ResponseWrapper::ok(cameras));
        }
        "status" | "--status" => inspect::run_status(),
        "gpu" => print_json(&ResponseWrapper::ok(gpu::status())),
        "bench" => {
            let runs = args
                .iter()
                .position(|a| a == "--runs")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(5);
            let photo = args.get(2).filter(|a| !a.starts_with("--")).map(String::as_str);
            match bench::run(photo, runs) {
                Ok(v) => print_json(&ResponseWrapper::ok(v)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "mcp" => mcp::run(),
        "ai" => ai::run(&args[2..]),
        "xmp" => xmp::run(&args[2..]),
        "cloud" => cloud::run(&args[2..]),
        "npu" => print_json(&ResponseWrapper::ok(ml::status())),
        "ai-mask" => {
            let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
            let result = match args.get(2).filter(|a| !a.starts_with("--")) {
                Some(photo) => ai::run_mask(photo, flag("--kind").as_deref().unwrap_or("subject"), flag("--model")),
                None => Err("Usage: ai-mask <photo> [--kind subject|sky] [--model file.onnx]".to_string()),
            };
            match result {
                Ok(v) => print_json(&ResponseWrapper::ok(v)),
                Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
            }
        }
        "version" | "--version" | "-v" => {
            println!("omastudio-engine v{}", env!("CARGO_PKG_VERSION"));
        }
        "help" | "--help" | "-h" => print_help(),
        "theme" => print_json(&ResponseWrapper::ok(theme::read_theme_info())),
        "daemon" => {
            // The daemon serves the editor's viewport; exports go through export_photo
            pipeline::set_interactive(true);
            daemon::run();
        }
        cmd => {
            print_json::<()>(&ResponseWrapper::err(format!("Unknown command: {}", cmd)));
        }
    }
}

fn print_help() {
    println!(
        "OmaStudio Engine v{} - Professional Photo RAW Engine for Omarchy Linux",
        env!("CARGO_PKG_VERSION")
    );
    println!("\nUsage: omastudio-engine <command> [arguments]\n");
    println!("Commands for Omarchy Agents & CLI:");
    println!("  status                                Output engine and feature status as JSON");
    println!("  cameras [FILTER]                      List cameras the RAW decoder supports");
    println!("  inspect <raw_file>                    Parse EXIF, dynamic range, AI scene classification, histogram");
    println!("  ai-auto <raw_file>                    Calculate optimal tone, white balance, and contrast via AI");
    println!("  ai-jev <raw_file>                     Auto, refined by a configured decision model when one is set up");
    println!("  ai-social <raw_file> [platform]       Calculate social media crop & OLED recipe (ig, story, x, ig_square, fb, yt)");
    println!("  render <raw_file> [options]           Process RAW image to preview PPM/PNG/JPG");
    println!("  export <raw_file> [options]           High-res demosaic & export (jpg, png, tif, webp, avif, heic, jxl, exr)");
    println!("  fuji-recipe <raf|jpg|fp1|txt>         Read a Fujifilm recipe from a photo, X RAW Studio profile or recipe text");
    println!("  fuji-recipe list                      List the recipes saved in $XDG_DATA_HOME/omastudio/fuji-recipes");
    println!("  fuji-recipe save <file> [--label L]   Save a recipe into the library as .FP1");
    println!("  fuji-recipe export <file> <out.fp1>   Write a recipe as an X RAW Studio profile [--label L] [--device X-T30]");
    println!("  fuji-camera list|info|presets         Fujifilm cameras on USB (PTP mode) and their C1-C7 settings");
    println!("  fuji-camera convert <raf> [--recipe F] [--out J]  Convert a RAF in the camera (X RAW Studio protocol)");
    println!("  fuji-camera write-preset <C1-C7> <recipe> [--label L]  Store a recipe in a custom setting slot");
    println!("  fuji-calibrate <raf>... [--any]       Measure film simulation LUTs from your RAFs' camera JPEGs");
    println!("  fuji-compat <raf>... [--full]         Sensor, settings and camera-JPEG match report per file");
    println!("  fuji-pixel-shift <raf>... [--out F]   Merge a GFX Pixel Shift sequence into a linear DNG");
    println!("  medium-format info|lcc-create|multishot|tether  Medium format diagnostics, flat-field LCC and 4-shot merge");
    println!("  ai providers                          AI providers, stored keys and local servers (Ollama, LM Studio)");
    println!("  ai key <provider> [--delete]          Store the API key read from stdin in the Secret Service");
    println!("  ai use <provider> [model]             Choose the provider and model for the edit assistant");
    println!("  ai edit <photo> [--prompt P] [--recipe JSON|-]  Let the chosen model edit the photo");
    println!("  xmp export <photo> [--out F] [--force] Write the edit, rating and label as an XMP sidecar");
    println!("  xmp import <photo> [--from F] [--save] Read an XMP sidecar from Lightroom, Capture One, darktable");
    println!("  cloud status                          Cloud services, the tools they need and connected accounts");
    println!("  cloud connect <service>               Sign in in the browser (google-drive, onedrive, dropbox, github, ...)");
    println!("  cloud list <account> [path]           Folders and photos in a connected account");
    println!("  cloud fetch <account> <path>          Download a photo into the cache and print its local path");
    println!("  cloud upload <account> <file> [dir]   Upload a file (default: the account's OmaStudio folder)");
    println!("  cloud disconnect <account>            Forget an account and its keys");
    println!("  npu                                   ONNX Runtime and the NPU/GPU/CPU it can use for local models");
    println!("  ai-mask <photo> [--kind subject|sky]  Segment the photo with a local model into a mask layer image");
    println!("  mcp                                   Model Context Protocol server on stdin/stdout for AI agents");
    println!("  gpu                                   GPU compute pipeline status (OMASTUDIO_GPU=off uses the CPU)");
    println!("  bench [photo] [--runs N]              Time the preview pipeline stage by stage");
    println!("  icc list                              List all detected system and bundled ICC color profiles");
    println!("  scan [dir]                            Scan local folder for supported RAW photos");
    println!("  catalog                               Retrieve or query persistent SQLite photo catalog database");
    println!("  dag                                   Inspect banded wgpu/WGSL pipeline status (deprecated alias)");
    println!("  daemon                                Start persistent JSON-over-stdin processing daemon");
    println!("  theme                                 Display current Omarchy system theme and colors");
    println!("  version                               Print engine version");
    println!("  help                                  Show this help reference");
}
