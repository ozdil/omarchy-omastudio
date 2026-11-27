//! `xmp export|import` CLI subcommands.

use std::path::{Path, PathBuf};

use omastudio_engine::{
    recipe::{Catalog, Recipe},
    xmp,
};

use super::{ResponseWrapper, print_json, read_recipe_arg};

/// `xmp export <photo> [--recipe JSON|-] [--out F] [--force]` and
/// `xmp import <photo> [--from F] [--save]`: XMP sidecars for other editors.
pub fn run(args: &[String]) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let has = |name: &str| args.iter().any(|a| a == name);
    let result: Result<serde_json::Value, String> = (|| {
        let photo = args.get(1).ok_or("Usage: xmp export|import <photo>")?;
        let path = Path::new(photo);
        match args.first().map(String::as_str) {
            Some("export") => {
                let given = flag("--recipe");
                let recipe: Recipe = match &given {
                    Some(j) => read_recipe_arg(j)?,
                    None => Recipe::load_initial(path),
                };
                let item = Catalog::load().get(photo).cloned();
                // The editor's recipe carries the rating and flag as they are now; the
                // catalogue only supplies them when exporting from the command line
                let (rating, reject) = match (&given, &item) {
                    (None, Some(i)) => (i.rating as i32, i.flag == "reject"),
                    _ => (recipe.rating as i32, recipe.flag == "reject"),
                };
                let culling = xmp::Culling {
                    rating: if reject { -1 } else { rating },
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
                    let flag = (culling.rating < 0).then_some("reject");
                    let label = (!culling.label.is_empty()).then_some(culling.label.as_str());
                    let _ = Catalog::update_culling_indexed(
                        photo,
                        Some(culling.rating.clamp(0, 5) as u8),
                        flag,
                        label,
                    );
                }
                Ok(serde_json::json!({
                    "source": source,
                    "recipe": recipe,
                    "culling": culling,
                    "sidecar": saved,
                }))
            }
            _ => Err("Usage: xmp export|import <photo>".to_string()),
        }
    })();
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
    }
}
