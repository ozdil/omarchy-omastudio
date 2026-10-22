use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IccProfileInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: String,
    pub is_wide_gamut: bool,
    pub is_default: bool,
}

pub fn get_icc_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    
    // User OmaStudio ICC folder
    dirs.push(PathBuf::from(&home).join(".local/share/omastudio/icc"));
    // Project bundled assets (source checkout)
    dirs.push(PathBuf::from("assets/icc"));
    // Bundled assets of installed builds: <prefix>/share/omastudio/icc next to <prefix>/bin
    if let Ok(exe) = env::current_exe()
        && let Some(prefix) = exe.parent().and_then(|bin| bin.parent())
    {
        dirs.push(prefix.join("share/omastudio/icc"));
    }
    dirs.push(PathBuf::from("/usr/share/omastudio/icc"));
    // System Ghostscript ICC folder
    dirs.push(PathBuf::from("/usr/share/ghostscript/iccprofiles"));
    // System color ICC folder
    dirs.push(PathBuf::from("/usr/share/color/icc"));

    dirs
}

pub fn list_icc_profiles() -> Vec<IccProfileInfo> {
    let mut profiles = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // Standard profiles
    let standards = vec![
        ("sRGB", "sRGB (IEC 61966-2.1)", "Standard gamut for web and general displays", false, true),
        ("DisplayP3", "Display P3", "Apple and modern wide-gamut OLED displays (DCI-P3 D65)", true, false),
        ("AdobeRGB1998", "Adobe RGB (1998)", "Wide-gamut standard for high-end photography & print", true, false),
        ("ProPhotoRGB", "ProPhoto RGB (ROMM)", "Ultra-wide 16-bit master archival working space", true, false),
        ("scRGB_Linear", "scRGB Linear", "High Dynamic Range (HDR) linear color space", true, false),
        ("e-sRGB", "e-sRGB Extended", "Extended gamut sRGB with extended dynamic range", true, false),
    ];

    for (id, name, desc, wide, def) in standards {
        if let Some(path) = resolve_icc_path(id) {
            seen_ids.insert(id.to_lowercase());
            profiles.push(IccProfileInfo {
                id: id.to_string(),
                name: name.to_string(),
                description: desc.to_string(),
                path: path.to_string_lossy().to_string(),
                is_wide_gamut: wide,
                is_default: def,
            });
        }
    }

    // Scan directories for additional custom user ICC profiles
    for dir in get_icc_directories() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && let Some(ext) = path.extension().and_then(|s| s.to_str())
                {
                    let ext_lower = ext.to_lowercase();
                    if (ext_lower == "icc" || ext_lower == "icm")
                        && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                    {
                        let id_lower = stem.to_lowercase();
                        if !seen_ids.contains(&id_lower) {
                            seen_ids.insert(id_lower);
                            profiles.push(IccProfileInfo {
                                id: stem.to_string(),
                                name: stem.replace('_', " "),
                                description: "Custom ICC Color Profile".to_string(),
                                path: path.to_string_lossy().to_string(),
                                is_wide_gamut: true,
                                is_default: false,
                            });
                        }
                    }
                }
            }
        }
    }

    profiles
}

pub fn resolve_icc_path(name_or_id: &str) -> Option<PathBuf> {
    let lower = name_or_id.to_lowercase();

    // Direct path check
    let direct = Path::new(name_or_id);
    if direct.exists() && direct.is_file() {
        return Some(direct.to_path_buf());
    }

    for dir in get_icc_directories() {
        if !dir.exists() {
            continue;
        }

        // Exact match
        let candidate = dir.join(format!("{}.icc", name_or_id));
        if candidate.exists() {
            return Some(candidate);
        }
        let candidate_icm = dir.join(format!("{}.icm", name_or_id));
        if candidate_icm.exists() {
            return Some(candidate_icm);
        }

        // Case-insensitive scan
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                    && stem.to_lowercase() == lower
                {
                    return Some(p);
                }
            }
        }
    }

    // Fallbacks for standard names
    if lower.contains("srgb") {
        let fallback = PathBuf::from("/usr/share/ghostscript/iccprofiles/srgb.icc");
        if fallback.exists() { return Some(fallback); }
    } else if lower.contains("adobe") || lower.contains("a98") {
        let fallback = PathBuf::from("/usr/share/ghostscript/iccprofiles/a98.icc");
        if fallback.exists() { return Some(fallback); }
    } else if lower.contains("prophoto") || lower.contains("romm") {
        let fallback = PathBuf::from("/usr/share/ghostscript/iccprofiles/rommrgb.icc");
        if fallback.exists() { return Some(fallback); }
    }

    None
}
