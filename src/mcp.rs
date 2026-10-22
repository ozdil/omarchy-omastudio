//! `omastudio-engine mcp`: a Model Context Protocol server on stdin/stdout, so any
//! MCP client (Claude Code, opencode, Codex, Gemini CLI, ...) can use OmaStudio.
//!
//! Messages are newline-delimited JSON-RPC 2.0. Engine tools run this binary's own
//! command line in a child process, so they behave exactly like the CLI and a crash
//! in one call cannot end the server. Window tools talk to the running OmaStudio
//! window through its control socket (`$XDG_RUNTIME_DIR/omastudio/ipc.sock`).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::recipe::Recipe;
use crate::security;

const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_LINE: u64 = 4 << 20;
/// Longest side of images returned to the client
const IMAGE_SIDE: u32 = 1600;
const WINDOW_TIMEOUT: Duration = Duration::from_secs(12);

const INSTRUCTIONS: &str = "OmaStudio is a RAW photo editor for Omarchy Linux. Engine tools \
(inspect_photo, render_preview, export_photo, the Fujifilm and catalog tools) work on files \
directly. Window tools (window_*, open_photo, apply_*) control the OmaStudio window the user \
has open and fail when it is not running. A recipe is OmaStudio's adjustment object: get one \
from window_status, auto_enhance or read_fuji_recipe (field `omastudio`), change fields and \
pass it back. Paths must be absolute.";

pub fn run() {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut input).take(MAX_LINE).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(batch)) => {
                let answers: Vec<Value> = batch.iter().filter_map(handle).collect();
                if answers.is_empty() { None } else { Some(Value::Array(answers)) }
            }
            Ok(message) => handle(&message),
            Err(e) => Some(error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        if let Some(response) = response {
            let mut out = io::stdout().lock();
            let _ = writeln!(out, "{response}");
            let _ = out.flush();
        }
    }
}

/// Answers one message; notifications get no answer.
fn handle(message: &Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let id = id?;
    let result = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == requested).unwrap_or(&PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "omastudio", "title": "OmaStudio", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(name, &args) {
                Some(Ok(content)) => Ok(json!({ "content": content, "isError": false })),
                Some(Err(e)) => Ok(json!({ "content": [{ "type": "text", "text": e }], "isError": true })),
                None => Err((-32602, format!("Unknown tool: {name}"))),
            }
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, msg)) => error(id, code, &msg),
    })
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn tool(name: &str, description: &str, input: Value, read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input,
        "annotations": { "readOnlyHint": read_only, "destructiveHint": false },
    })
}

fn tools() -> Vec<Value> {
    let path = json!({ "type": "string", "description": "Absolute path of the photo" });
    let recipe = json!({ "type": "object", "description": "OmaStudio recipe; omitted fields keep their defaults" });
    vec![
        tool("inspect_photo", "Metadata (camera, lens, exposure), histogram and scene analysis of a RAW or image file.", schema(json!({ "path": path }), &["path"]), true),
        tool("auto_enhance", "Suggests a recipe for a photo: tone, white balance and contrast from its analysis.", schema(json!({ "path": path }), &["path"]), true),
        tool("render_preview", "Renders a photo with a recipe and returns the preview image and its histogram.", schema(json!({ "path": path, "recipe": recipe }), &["path"]), true),
        tool("export_photo", "Develops a photo at full resolution and writes it to disk. options: format (jxl, avif, webp, tiff, png, jpeg), quality, scale_percent, max_dimension, icc_profile (sRGB, DisplayP3, AdobeRGB1998, ProPhotoRGB, Rec2020), rendering_intent, output_dir.",
            schema(json!({ "path": path, "recipe": recipe, "options": { "type": "object" } }), &["path"]), false),
        tool("scan_folder", "Lists the photos OmaStudio can open in a folder.", schema(json!({ "dir": { "type": "string" } }), &["dir"]), true),
        tool("catalog_query", "Searches the photo catalog by rating, flag, camera model or text.",
            schema(json!({ "min_rating": { "type": "integer", "minimum": 0, "maximum": 5 }, "flag": { "type": "string", "enum": ["pick", "reject"] }, "camera_model": { "type": "string" }, "search": { "type": "string" } }), &[]), true),
        tool("set_culling", "Sets a photo's star rating, pick/reject flag or colour label in the catalog.",
            schema(json!({ "path": path, "rating": { "type": "integer", "minimum": 0, "maximum": 5 }, "flag": { "type": "string", "enum": ["pick", "reject", "none"] }, "color_label": { "type": "string" } }), &["path"]), false),
        tool("read_fuji_recipe", "Reads the Fujifilm recipe of a RAF/JPEG, X RAW Studio profile (.FP1/.FP2/.FP3) or recipe text. Field `omastudio` is the matching OmaStudio recipe.", schema(json!({ "path": path }), &["path"]), true),
        tool("list_fuji_recipes", "Lists the recipes in the user's Fujifilm recipe library.", schema(json!({}), &[]), true),
        tool("save_fuji_recipe", "Saves a recipe from a photo, profile or recipe text into the Fujifilm recipe library.",
            schema(json!({ "path": path, "label": { "type": "string" } }), &["path"]), false),
        tool("merge_pixel_shift", "Merges the RAF frames of a Fujifilm GFX Pixel Shift sequence into a linear DNG.",
            schema(json!({ "paths": { "type": "array", "items": { "type": "string" }, "minItems": 2 }, "out": { "type": "string" } }), &["paths"]), false),
        tool("fuji_camera", "Fujifilm camera on USB: `list` connected cameras, `info` about the camera, `presets` reads custom settings C1-C7.",
            schema(json!({ "action": { "type": "string", "enum": ["list", "info", "presets"] } }), &["action"]), true),
        tool("fuji_camera_convert", "Converts a RAF in the connected camera (X RAW Studio protocol), optionally with a recipe file, and writes the camera's JPEG.",
            schema(json!({ "path": path, "recipe_file": { "type": "string" }, "out": { "type": "string" } }), &["path"]), false),
        tool("fuji_camera_write_preset", "Writes a recipe file into a custom setting slot (1-7) of the connected camera. Changes the camera's settings.",
            schema(json!({ "slot": { "type": "integer", "minimum": 1, "maximum": 7 }, "recipe_file": { "type": "string" }, "label": { "type": "string" } }), &["slot", "recipe_file"]), false),
        tool("gpu_status", "The GPU OmaStudio uses for demosaicing, or why it falls back to the CPU.", schema(json!({}), &[]), true),
        tool("window_status", "State of the OmaStudio window: open photo, current recipe, view and theme.", schema(json!({}), &[]), true),
        tool("window_screenshot", "Returns a screenshot of the OmaStudio window.", schema(json!({}), &[]), true),
        tool("open_photo", "Opens a photo in the OmaStudio window.", schema(json!({ "path": path }), &["path"]), false),
        tool("apply_recipe", "Replaces the open photo's adjustments with a recipe.", schema(json!({ "recipe": recipe }), &["recipe"]), false),
        tool("adjust", "Changes exposure (EV) and/or white balance (Kelvin) of the open photo.",
            schema(json!({ "exposure": { "type": "number", "minimum": -5, "maximum": 5 }, "temperature": { "type": "number", "minimum": 2000, "maximum": 50000 } }), &[]), false),
        tool("apply_preset", "Applies one of OmaStudio's built-in presets by name to the open photo.", schema(json!({ "name": { "type": "string" } }), &["name"]), false),
        tool("apply_fuji_recipe", "Applies a recipe from the Fujifilm recipe library (by label or path) to the open photo.", schema(json!({ "name": { "type": "string" } }), &["name"]), false),
        tool("apply_camera_recipe", "Applies the recipe a Fujifilm photo was shot with to the open photo (the open photo's own by default).", schema(json!({ "path": path }), &[]), false),
        tool("set_zoom", "Zooms the window's viewport: a number is photo pixels per screen pixel (1 = 100%), or \"fit\".",
            schema(json!({ "scale": { "type": ["number", "string"] } }), &["scale"]), false),
    ]
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("Missing argument: {key}"))
}

/// A recipe argument checked against the engine's recipe type, as JSON text.
fn recipe_arg(args: &Value) -> Result<Option<String>, String> {
    match args.get("recipe") {
        None | Some(Value::Null) => Ok(None),
        Some(r) => serde_json::from_value::<Recipe>(r.clone())
            .map_err(|e| format!("Invalid recipe: {e}"))
            .and_then(|r| serde_json::to_string(&r).map_err(|e| e.to_string()))
            .map(Some),
    }
}

fn text(value: impl std::fmt::Display) -> Vec<Value> {
    vec![json!({ "type": "text", "text": value.to_string() })]
}

fn call_tool(name: &str, args: &Value) -> Option<Result<Vec<Value>, String>> {
    let result = match name {
        "inspect_photo" => required(args, "path").and_then(|p| engine(&["inspect", p])).map(text),
        "auto_enhance" => required(args, "path").and_then(|p| engine(&["ai-auto", p])).map(text),
        "render_preview" => render_preview(args),
        "export_photo" => (|| {
            let path = required(args, "path")?;
            let mut cli = vec!["export".to_string(), path.to_string()];
            if let Some(r) = recipe_arg(args)? {
                cli.extend(["--recipe".into(), r]);
            }
            if let Some(o) = args.get("options").filter(|o| o.is_object()) {
                cli.extend(["--options".into(), o.to_string()]);
            }
            engine(&cli).map(text)
        })(),
        "scan_folder" => required(args, "dir").and_then(|d| engine(&["scan", d])).map(text),
        "catalog_query" => {
            let mut cli = vec!["catalog".to_string(), "query".into()];
            if let Some(r) = args.get("min_rating").and_then(Value::as_u64) {
                cli.extend(["--rating".into(), r.to_string()]);
            }
            for (key, flag) in [("flag", "--flag"), ("camera_model", "--model"), ("search", "--search")] {
                if let Some(v) = str_arg(args, key) {
                    cli.extend([flag.into(), v.into()]);
                }
            }
            engine(&cli).map(text)
        }
        "set_culling" => (|| {
            let path = required(args, "path")?;
            let mut cli = vec!["catalog".to_string(), "cull".into(), path.into()];
            if let Some(r) = args.get("rating").and_then(Value::as_u64) {
                cli.extend(["--rating".into(), r.to_string()]);
            }
            for (key, flag) in [("flag", "--flag"), ("color_label", "--color")] {
                if let Some(v) = str_arg(args, key) {
                    cli.extend([flag.into(), v.into()]);
                }
            }
            engine(&cli).map(text)
        })(),
        "read_fuji_recipe" => required(args, "path").and_then(|p| engine(&["fuji-recipe", p])).map(text),
        "list_fuji_recipes" => engine(&["fuji-recipe", "list"]).map(text),
        "save_fuji_recipe" => required(args, "path")
            .and_then(|p| match str_arg(args, "label") {
                Some(label) => engine(&["fuji-recipe", "save", p, "--label", label]),
                None => engine(&["fuji-recipe", "save", p]),
            })
            .map(text),
        "merge_pixel_shift" => (|| {
            let paths: Vec<String> = args
                .get("paths")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
                .unwrap_or_default();
            if paths.len() < 2 {
                return Err("paths needs at least two RAF files".to_string());
            }
            let mut cli = vec!["fuji-pixel-shift".to_string()];
            cli.extend(paths);
            if let Some(out) = str_arg(args, "out") {
                cli.extend(["--out".into(), out.into()]);
            }
            engine(&cli).map(text)
        })(),
        "fuji_camera" => match str_arg(args, "action") {
            Some(a @ ("list" | "info" | "presets")) => engine(&["fuji-camera", a]).map(text),
            _ => Err("action must be list, info or presets".to_string()),
        },
        "fuji_camera_convert" => (|| {
            let path = required(args, "path")?;
            let mut cli = vec!["fuji-camera".to_string(), "convert".into(), path.into()];
            for (key, flag) in [("recipe_file", "--recipe"), ("out", "--out")] {
                if let Some(v) = str_arg(args, key) {
                    cli.extend([flag.into(), v.into()]);
                }
            }
            engine(&cli).map(text)
        })(),
        "fuji_camera_write_preset" => (|| {
            let slot = args.get("slot").and_then(Value::as_u64).filter(|s| (1..=7).contains(s)).ok_or("slot must be 1-7")?;
            let file = required(args, "recipe_file")?;
            let mut cli = vec!["fuji-camera".to_string(), "write-preset".into(), slot.to_string(), file.into()];
            if let Some(label) = str_arg(args, "label") {
                cli.extend(["--label".into(), label.into()]);
            }
            engine(&cli).map(text)
        })(),
        "gpu_status" => engine(&["gpu"]).map(text),
        "window_status" => window("getStatus", vec![]).map(text),
        "window_screenshot" => window_screenshot(),
        "open_photo" => required(args, "path").and_then(|p| window("openPhoto", vec![p.into()])).map(text),
        "apply_recipe" => recipe_arg(args)
            .and_then(|r| r.ok_or_else(|| "Missing argument: recipe".to_string()))
            .and_then(|r| window("applyRecipe", vec![r.into()]))
            .map(text),
        "adjust" => (|| {
            let mut done = Vec::new();
            if let Some(ev) = args.get("exposure").and_then(Value::as_f64) {
                window("setExposure", vec![ev.into()])?;
                done.push(format!("exposure {ev:+} EV"));
            }
            if let Some(k) = args.get("temperature").and_then(Value::as_f64) {
                window("setWarmth", vec![k.into()])?;
                done.push(format!("white balance {k} K"));
            }
            if done.is_empty() {
                return Err("Give exposure and/or temperature".to_string());
            }
            Ok(text(format!("Set {}", done.join(", "))))
        })(),
        "apply_preset" => required(args, "name").and_then(|n| window("applyPreset", vec![n.into()])).map(text),
        "apply_fuji_recipe" => required(args, "name").and_then(|n| window("applyFujiRecipe", vec![n.into()])).map(text),
        "apply_camera_recipe" => window("applyCameraRecipe", vec![str_arg(args, "path").unwrap_or("").into()]).map(text),
        "set_zoom" => match args.get("scale") {
            Some(s @ (Value::Number(_) | Value::String(_))) => window("setZoom", vec![s.clone()]).map(text),
            _ => Err("scale must be a number or \"fit\"".to_string()),
        },
        _ => return None,
    };
    Some(result)
}

/// Runs this binary's command line and returns the `data` of its JSON answer.
fn engine<S: AsRef<str>>(args: &[S]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Cannot find omastudio-engine: {e}"))?;
    let output = Command::new(exe)
        .args(args.iter().map(AsRef::as_ref))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("Cannot run omastudio-engine: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let answer: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        let first = stdout.lines().next().unwrap_or("").trim();
        if first.is_empty() { format!("omastudio-engine exited with {}", output.status) } else { first.to_string() }
    })?;
    if answer.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(answer.get("error").and_then(Value::as_str).unwrap_or("Command failed").to_string());
    }
    let data = answer.get("data").cloned().unwrap_or(answer);
    Ok(serde_json::to_string_pretty(&data).unwrap_or_default())
}

fn render_preview(args: &Value) -> Result<Vec<Value>, String> {
    let path = required(args, "path")?;
    let out = security::get_secure_preview_dir().join(format!("omastudio_mcp_{}.ppm", std::process::id()));
    let out_str = out.to_string_lossy().into_owned();
    let mut cli = vec!["render".to_string(), path.into(), "--out".into(), out_str];
    if let Some(r) = recipe_arg(args)? {
        cli.extend(["--recipe".into(), r]);
    }
    let result = engine(&cli);
    let image = result.as_ref().ok().map(|_| image_content(&out));
    let _ = std::fs::remove_file(&out);
    let summary = result?;
    // Clipping figures as text; the image path is a temporary file and the
    // histogram bins are visible in the image anyway
    let data: Value = serde_json::from_str(&summary).unwrap_or(Value::Null);
    let clipping = json!({
        "shadow_clipping_percent": data["histogram"]["shadow_clipping_percent"],
        "highlight_clipping_percent": data["histogram"]["highlight_clipping_percent"],
    });
    let mut content = vec![image.ok_or("Preview missing")??];
    content.extend(text(clipping));
    Ok(content)
}

/// Encodes an image file as JPEG content, at most IMAGE_SIDE pixels on the long side.
fn image_content(path: &Path) -> Result<Value, String> {
    let img = image::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let img = if img.width().max(img.height()) > IMAGE_SIDE { img.thumbnail(IMAGE_SIDE, IMAGE_SIDE) } else { img };
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 88)
        .encode_image(&img.to_rgb8())
        .map_err(|e| e.to_string())?;
    Ok(json!({ "type": "image", "data": base64(&jpeg), "mimeType": "image/jpeg" }))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn socket_path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    runtime.join("omastudio").join("ipc.sock")
}

/// Calls a method of the running window (same protocol as `omastudio ipc`).
fn window(method: &str, args: Vec<Value>) -> Result<String, String> {
    let mut stream = UnixStream::connect(socket_path())
        .map_err(|_| "The OmaStudio window is not running. Start it with `omastudio`.".to_string())?;
    stream.set_read_timeout(Some(WINDOW_TIMEOUT)).map_err(|e| e.to_string())?;
    let request = json!({ "method": method, "args": args });
    stream.write_all(format!("{request}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream.take(MAX_LINE)).read_line(&mut reply).map_err(|e| e.to_string())?;
    let reply = reply.trim_end().to_string();
    if reply.starts_with("Error:") {
        return Err(reply);
    }
    // Pretty-print JSON answers, pass plain text through
    Ok(serde_json::from_str::<Value>(&reply)
        .ok()
        .filter(|v| v.is_object() || v.is_array())
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or(reply))
}

fn window_screenshot() -> Result<Vec<Value>, String> {
    let out = security::get_secure_preview_dir().join(format!("omastudio_mcp_window_{}.png", std::process::id()));
    let _ = std::fs::remove_file(&out);
    window("screenshot", vec![out.to_string_lossy().into_owned().into()])?;
    // The window saves the image just after answering
    let start = Instant::now();
    let mut last_size = 0;
    let image = loop {
        let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
        if size > 0 && size == last_size {
            break image_content(&out);
        }
        if start.elapsed() > Duration::from_secs(5) {
            break Err("The window did not save a screenshot".to_string());
        }
        last_size = size;
        std::thread::sleep(Duration::from_millis(150));
    };
    let _ = std::fs::remove_file(&out);
    Ok(vec![image?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn initialize_and_list() {
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        let unknown = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } })).unwrap();
        assert_eq!(unknown["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let list = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
        let missing = handle(&json!({ "jsonrpc": "2.0", "id": 4, "method": "nope" })).unwrap();
        assert_eq!(missing["error"]["code"], -32601);
        let bad = handle(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "apply_recipe", "arguments": { "recipe": { "exposure": "bright" } } } })).unwrap();
        assert_eq!(bad["result"]["isError"], true);
    }
}
