//! AI edit assistant and ONNX Runtime mask segmentation CLI commands (`ai`, `ai-mask`).

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use omastudio_engine::{
    ai::{assistant, provider},
    ml, pipeline,
    raw::RawImage,
    recipe::Recipe,
    security,
};

use super::inspect::render_file;
use super::{ResponseWrapper, print_json, read_recipe_arg};

/// Segments the photo with a local model and stores the mask for a Bitmap layer.
pub fn run_mask(photo: &str, kind: &str, model: Option<String>) -> Result<serde_json::Value, String> {
    if !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || kind.is_empty() {
        return Err("Invalid mask kind".to_string());
    }
    let model = model.map(PathBuf::from).unwrap_or_else(|| ml::models_dir().join(format!("{kind}.onnx")));
    if !model.is_file() {
        return Err(format!(
            "No {kind} model: put an ONNX segmentation model at {} (for example U2-Net, IS-Net or BiRefNet)",
            model.display()
        ));
    }
    // The neutral development of the whole frame, which Bitmap layers are mapped onto
    let (guard, _file) = security::TempFileGuard::create_in_dir(
        &security::get_secure_preview_dir(),
        "omastudio_mask",
        ".ppm",
        0o600,
    )
    .map_err(|e| e.to_string())?;
    let rendered = render_file(photo, &Recipe::default(), 0.0, &guard.path().to_string_lossy());
    let image = rendered.and_then(|_| image::open(guard.path()).map_err(|e| e.to_string()));
    drop(guard);
    let image = image?.to_rgb8();
    let mask = ml::segment(&model, &image)?;

    let dir = pipeline::layers::masks_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = Path::new(photo).file_stem().and_then(|s| s.to_str()).unwrap_or("photo");
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let out = dir.join(format!("{stem}-{kind}-{stamp}.png"));
    let gray = image::GrayImage::from_raw(mask.width, mask.height, mask.data).ok_or("Mask size mismatch")?;
    let mut png = Vec::new();
    gray.write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    security::atomic_write_user(&out, &png).map_err(|e| format!("Cannot write {}: {e}", out.display()))?;
    Ok(serde_json::json!({
        "path": out,
        "kind": kind,
        "width": mask.width,
        "height": mask.height,
        "provider": mask.provider,
        "model": model,
    }))
}

/// `ai providers | local | key <provider> [--delete] | use <provider> [model] [--endpoint URL] |
/// edit <photo> [--prompt P] [--recipe JSON|-] [--provider X] [--model M]`.
pub fn run(args: &[String]) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let lookup = |id: Option<&String>| -> Result<&'static provider::Provider, String> {
        let id = id.ok_or("Missing provider: anthropic, openai, openrouter, gemini, omarchy, ollama or lmstudio")?;
        provider::find(id).ok_or_else(|| format!("Unknown provider {id}"))
    };
    let result: Result<serde_json::Value, String> = match args.first().map(String::as_str) {
        Some("providers") | None => Ok(serde_json::json!({
            "providers": provider::status(),
            "choice": provider::load_choice(),
        })),
        Some("local") => provider::connect_local().and_then(|c| serde_json::to_value(&c).map_err(|e| e.to_string())),
        Some("key") => lookup(args.get(1)).and_then(|p| {
            if args.iter().any(|a| a == "--delete") {
                return provider::delete_key(p).map(|_| serde_json::json!({ "provider": p.id, "stored": false }));
            }
            // The key comes on stdin so it never shows up in the process list
            let mut key = String::new();
            io::Read::take(io::stdin().lock(), 4096)
                .read_line(&mut key)
                .map_err(|e| e.to_string())?;
            provider::store_key(p, &key).map(|_| serde_json::json!({ "provider": p.id, "stored": true }))
        }),
        Some("use") => lookup(args.get(1)).and_then(|p| {
            let model = args.get(2).filter(|m| !m.starts_with("--")).cloned().unwrap_or_default();
            let choice = provider::Choice {
                provider: p.id.to_string(),
                model,
                endpoint: flag("--endpoint"),
            };
            provider::save_choice(&choice).and_then(|_| serde_json::to_value(&choice).map_err(|e| e.to_string()))
        }),
        Some("edit") => match args.get(1) {
            Some(photo) => ai_edit(
                photo,
                flag("--prompt").as_deref().unwrap_or(""),
                flag("--recipe"),
                flag("--provider"),
                flag("--model"),
            ),
            None => Err("Usage: ai edit <photo> [--prompt P] [--recipe JSON|-]".to_string()),
        },
        Some(other) => Err(format!("Unknown ai command: {other}")),
    };
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
    }
}

/// Shows the photo, as developed with `recipe`, to the chosen model and applies its edit.
fn ai_edit(
    photo: &str,
    prompt: &str,
    recipe: Option<String>,
    provider_id: Option<String>,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    let current: Recipe = match recipe {
        Some(r) => read_recipe_arg(&r)?,
        None => Recipe::default(),
    };
    let target = provider::resolve(provider_id.as_deref(), model.as_deref())?;

    let (guard, _file) = security::TempFileGuard::create_in_dir(
        &security::get_secure_preview_dir(),
        "omastudio_ai",
        ".ppm",
        0o600,
    )
    .map_err(|e| e.to_string())?;
    let rendered = render_file(photo, &current, 0.0, &guard.path().to_string_lossy());
    let image = rendered
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|_| image::open(guard.path()).map_err(|e| e.to_string()));
    drop(guard);
    let (rendered, image) = (rendered?, image?);
    let image = image.thumbnail(1280, 1280).to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode_image(&image)
        .map_err(|e| e.to_string())?;

    let meta = RawImage::open(photo).ok().and_then(|r| r.get_metadata().ok());
    let camera = match meta {
        Some(m) => serde_json::json!({
            "camera": format!("{} {}", m.make, m.model).trim(),
            "lens": m.lens,
            "iso": m.iso,
            "shutter_s": m.shutter,
            "aperture": m.aperture,
            "focal_length_mm": m.focal_length,
            "shadow_clipping_percent": rendered.histogram.shadow_clipping_percent,
            "highlight_clipping_percent": rendered.histogram.highlight_clipping_percent,
        }),
        None => serde_json::json!({}),
    };
    let answer = provider::ask_json(
        &target,
        assistant::SYSTEM,
        &jpeg,
        &assistant::request_text(prompt, &current, &camera),
        &assistant::schema(),
    )?;
    let (recipe, explanation) = assistant::merge(&current, &answer);
    Ok(serde_json::json!({
        "recipe": recipe,
        "explanation": explanation,
        "provider": target.provider.id,
        "model": target.model,
    }))
}
