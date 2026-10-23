//! Interface language: the choice stored in `~/.config/omastudio/language`, else the
//! system locale, matched against the translations compiled into the binary.

use std::io::Read;
use std::path::PathBuf;

/// Codes of the compiled translations (`app/i18n/qml_<code>.ts`), set by build.rs.
pub const AVAILABLE: &str = env!("OMASTUDIO_LANGUAGES");

const MAX_BYTES: u64 = 64;

pub fn available() -> impl Iterator<Item = &'static str> {
    AVAILABLE.split(',').filter(|c| !c.is_empty())
}

fn config_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("omastudio").join("language"))
}

/// The language the user picked in the application ("" when it follows the system).
pub fn stored() -> String {
    let Some(path) = config_file() else { return String::new() };
    // A regular file of a few bytes; symlinks are not followed
    let ok = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file() && m.len() <= MAX_BYTES);
    if !ok {
        return String::new();
    }
    let mut text = String::new();
    let read = std::fs::File::open(&path).and_then(|f| f.take(MAX_BYTES).read_to_string(&mut text));
    match read {
        Ok(_) => best_match(text.trim()).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Stores the user's pick; "" removes it so the system language applies again.
pub fn store(code: &str) -> std::io::Result<()> {
    let Some(path) = config_file() else { return Ok(()) };
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::other("refusing to write through a symlink"));
    }
    if code.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let Some(dir) = path.parent() else { return Ok(()) };
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(".language.tmp");
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(&tmp, format!("{code}\n"))?;
    std::fs::rename(&tmp, &path)
}

/// The system language from the usual locale variables. The launcher passes the
/// user's own locale as `OMASTUDIO_SYSTEM_LANG`, since it runs the application under
/// a fixed C locale.
pub fn system() -> String {
    let vars = ["OMASTUDIO_SYSTEM_LANG", "LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"];
    for var in vars {
        let Ok(value) = std::env::var(var) else { continue };
        // LANGUAGE is a priority list such as "tr:en"
        for candidate in value.split(':') {
            if let Some(code) = best_match(candidate) {
                return code;
            }
        }
    }
    "en".to_string()
}

/// The language the interface starts in.
pub fn effective() -> String {
    let picked = stored();
    if picked.is_empty() { system() } else { picked }
}

/// Maps a locale name ("tr_TR.UTF-8", "pt-PT", "zh_HK") to an available translation.
pub fn best_match(locale: &str) -> Option<String> {
    let name = locale.split(['.', '@']).next().unwrap_or("").replace('-', "_");
    let mut parts = name.split('_');
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    if lang.is_empty() || lang == "c" || lang == "posix" || !lang.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    let region = parts.next().unwrap_or("").to_ascii_uppercase();
    let exact = if region.is_empty() { lang.clone() } else { format!("{lang}_{region}") };
    if let Some(code) = available().find(|c| *c == exact) {
        return Some(code.to_string());
    }
    // Traditional Chinese for Hong Kong, Macau and Taiwan, Simplified otherwise
    if lang == "zh" {
        let traditional = matches!(region.as_str(), "TW" | "HK" | "MO") || name.contains("Hant");
        let want = if traditional { "zh_TW" } else { "zh_CN" };
        return available().find(|c| *c == want).map(str::to_string);
    }
    if let Some(code) = available().find(|c| *c == lang) {
        return Some(code.to_string());
    }
    // Another region of the same language ("pt_PT" -> "pt_BR")
    available().find(|c| c.split('_').next() == Some(lang.as_str())).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_locale_names() {
        assert_eq!(best_match("tr_TR.UTF-8").as_deref(), Some("tr"));
        assert_eq!(best_match("en_US.UTF-8").as_deref(), Some("en"));
        assert_eq!(best_match("de-AT").as_deref(), Some("de"));
        assert_eq!(best_match("pt_PT").as_deref(), Some("pt_BR"));
        assert_eq!(best_match("zh_HK.UTF-8").as_deref(), Some("zh_TW"));
        assert_eq!(best_match("zh_SG").as_deref(), Some("zh_CN"));
        assert_eq!(best_match("C.UTF-8"), None);
        assert_eq!(best_match("POSIX"), None);
        assert_eq!(best_match(""), None);
        assert_eq!(best_match("xx_YY"), None);
    }

    #[test]
    fn english_is_always_available() {
        assert!(available().any(|c| c == "en"));
    }
}
