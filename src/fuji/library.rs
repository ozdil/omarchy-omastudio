//! The user's Fujifilm recipe library: X RAW Studio profiles kept in
//! `~/.local/share/omastudio/fuji-recipes`, plus importing a recipe from any source
//! (a photo, an .FP1/.FP2/.FP3 profile or shared recipe text).

use super::{CameraRecipe, fp1, text};
use crate::security;
use std::path::{Path, PathBuf};

/// Recipe text and profiles are small; this bounds what is read.
const MAX_TEXT: usize = 256 * 1024;
const MAX_ENTRIES: usize = 500;

pub fn dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".local/share/omastudio/fuji-recipes")
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn stem(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("Recipe").to_string()
}

/// Reads a recipe from a RAF/JPEG photo, an X RAW Studio profile or a text file.
pub fn import(path: &Path) -> Result<fp1::Profile, String> {
    match extension(path).as_str() {
        "raf" | "jpg" | "jpeg" => {
            let recipe = CameraRecipe::read(path).ok_or("No Fujifilm camera recipe found in file")?;
            Ok(fp1::Profile { label: stem(path), device: recipe.camera.clone(), recipe })
        }
        ext => {
            let bytes = security::read_secure_file(path, MAX_TEXT).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
            let content = String::from_utf8_lossy(&bytes);
            if matches!(ext, "fp1" | "fp2" | "fp3") || content.contains("<ConversionProfile") {
                let mut profile = fp1::parse(&content)?;
                if profile.label.is_empty() {
                    profile.label = stem(path);
                }
                Ok(profile)
            } else {
                Ok(fp1::Profile { label: stem(path), device: String::new(), recipe: text::parse(&content)? })
            }
        }
    }
}

/// One saved recipe.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    pub path: String,
    #[serde(flatten)]
    pub profile: fp1::Profile,
}

/// Saved recipes, sorted by name. Unreadable files are skipped.
pub fn list() -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir()) else { return Vec::new() };
    let mut entries: Vec<Entry> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(extension(p).as_str(), "fp1" | "fp2" | "fp3" | "txt"))
        .take(MAX_ENTRIES)
        .filter_map(|p| import(&p).ok().map(|profile| Entry { path: p.to_string_lossy().into_owned(), profile }))
        .collect();
    entries.sort_by_key(|e| e.profile.label.to_lowercase());
    entries
}

/// File name for a label: letters, digits, space, '-', '_' and '+' only.
fn file_name(label: &str) -> String {
    let name: String = label
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_+".contains(c) { c } else { '_' })
        .collect::<String>()
        .trim()
        .chars()
        .take(80)
        .collect();
    if name.is_empty() { "Recipe".to_string() } else { name }
}

/// Saves a profile into the library as .FP1 and returns its path. An existing
/// recipe with the same name is kept; the new one gets a numbered name.
pub fn save(profile: &fp1::Profile) -> Result<PathBuf, String> {
    let dir = dir();
    security::ensure_secure_dir(&dir).map_err(|e| format!("Cannot create {}: {}", dir.display(), e))?;
    let base = file_name(&profile.label);
    let path = (1..1000)
        .map(|n| if n == 1 { dir.join(format!("{}.FP1", base)) } else { dir.join(format!("{} ({}).FP1", base, n)) })
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .ok_or("Too many recipes with this name")?;
    security::atomic_write_secure(&path, fp1::write(profile).as_bytes()).map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;
    Ok(path)
}

/// Writes a profile to a path the user chose (for X RAW Studio or sharing).
pub fn export(profile: &fp1::Profile, out: &Path) -> Result<(), String> {
    security::atomic_write_user(out, fp1::write(profile).as_bytes()).map_err(|e| format!("Cannot write {}: {}", out.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("../../etc/passwd"), "______etc_passwd");
        assert_eq!(file_name("Kodak Portra 400"), "Kodak Portra 400");
        assert_eq!(file_name("  "), "Recipe");
    }
}
