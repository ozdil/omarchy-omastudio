//! Omarchy system theme inspection (`theme.name` and `colors.toml`).

use std::collections::HashMap;
use std::path::PathBuf;

use omastudio_engine::{paths, security};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ThemeInfo {
    pub theme: String,
    pub colors_path: String,
    pub colors: HashMap<String, String>,
}

/// Returns the active Omarchy theme name (or `"default"` if unavailable).
pub fn active_theme_name() -> String {
    let path = paths::xdg_state_dir().join("omarchy/current/theme.name");
    security::read_secure_file(&path, 1024)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

/// Parses key/value pairs from an Omarchy `colors.toml` file.
pub fn parse_colors_toml(content: &str) -> HashMap<String, String> {
    let mut colors = HashMap::new();
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
    colors
}

/// Reads the current Omarchy theme name and parsed color palette.
pub fn read_theme_info() -> ThemeInfo {
    let colors_path: PathBuf = paths::xdg_state_dir().join("omarchy/current/theme/colors.toml");
    let colors = security::read_secure_file(&colors_path, 64 * 1024)
        .map(|bytes| parse_colors_toml(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();

    ThemeInfo {
        theme: active_theme_name(),
        colors_path: colors_path.to_string_lossy().to_string(),
        colors,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_colors_toml;

    #[test]
    fn parses_omarchy_colors_toml() {
        let sample = "# comment\nAccent = \"#7aa2f7\" # trailing\nbackground = '#1a1b26'\nplain = #ffffff\n";
        let map = parse_colors_toml(sample);
        assert_eq!(map.get("accent").map(String::as_str), Some("#7aa2f7"));
        assert_eq!(map.get("background").map(String::as_str), Some("#1a1b26"));
        assert_eq!(map.get("plain").map(String::as_str), Some("#ffffff"));
    }
}
