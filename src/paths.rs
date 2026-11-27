//! Centralized XDG Base Directory resolution for OmaStudio.
//!
//! Per the XDG Base Directory specification, environment variables that are unset,
//! empty, or not absolute paths are ignored and fall back to the `$HOME`-relative
//! defaults.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

fn abs_env(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).and_then(|v| abs_path_opt(Some(v.as_os_str())))
}

fn abs_path_opt(val: Option<&OsStr>) -> Option<PathBuf> {
    let raw = val?;
    if raw.is_empty() {
        return None;
    }
    // Trim ASCII whitespace in UTF-8 values while preserving non-UTF-8 paths verbatim.
    let path = match raw.to_str() {
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return None;
            }
            PathBuf::from(trimmed)
        }
        None => PathBuf::from(raw),
    };
    path.is_absolute().then_some(path)
}

fn resolve_xdg_with(var_val: Option<&OsStr>, home: &Path, fallback_rel: &str) -> PathBuf {
    abs_path_opt(var_val).unwrap_or_else(|| home.join(fallback_rel))
}

/// Returns `$HOME` if non-empty, otherwise `.` as a defensive fallback.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Returns `$XDG_CONFIG_HOME` (if absolute) or `$HOME/.config`.
pub fn xdg_config_dir() -> PathBuf {
    resolve_xdg_with(std::env::var_os("XDG_CONFIG_HOME").as_deref(), &home_dir(), ".config")
}

/// Returns `$XDG_DATA_HOME` (if absolute) or `$HOME/.local/share`.
pub fn xdg_data_dir() -> PathBuf {
    resolve_xdg_with(std::env::var_os("XDG_DATA_HOME").as_deref(), &home_dir(), ".local/share")
}

/// Returns `$XDG_CACHE_HOME` (if absolute) or `$HOME/.cache`.
pub fn xdg_cache_dir() -> PathBuf {
    resolve_xdg_with(std::env::var_os("XDG_CACHE_HOME").as_deref(), &home_dir(), ".cache")
}

/// Returns `$XDG_STATE_HOME` (if absolute) or `$HOME/.local/state`.
pub fn xdg_state_dir() -> PathBuf {
    resolve_xdg_with(std::env::var_os("XDG_STATE_HOME").as_deref(), &home_dir(), ".local/state")
}

/// Returns `$XDG_RUNTIME_DIR` (if absolute), falling back to `/run/user/<uid>`.
pub fn xdg_runtime_dir() -> PathBuf {
    abs_env("XDG_RUNTIME_DIR")
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", crate::security::current_uid())))
}

/// `$XDG_CONFIG_HOME/omastudio`
pub fn app_config_dir() -> PathBuf {
    xdg_config_dir().join("omastudio")
}

/// `$XDG_DATA_HOME/omastudio`
pub fn app_data_dir() -> PathBuf {
    xdg_data_dir().join("omastudio")
}

/// `$XDG_CACHE_HOME/omastudio`
pub fn app_cache_dir() -> PathBuf {
    xdg_cache_dir().join("omastudio")
}

/// `$XDG_RUNTIME_DIR/omastudio`
pub fn app_runtime_dir() -> PathBuf {
    xdg_runtime_dir().join("omastudio")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_resolution_rejects_empty_and_relative_paths() {
        let home = Path::new("/home/tester");
        assert_eq!(resolve_xdg_with(None, home, ".config"), PathBuf::from("/home/tester/.config"));
        assert_eq!(resolve_xdg_with(Some(OsStr::new("")), home, ".config"), PathBuf::from("/home/tester/.config"));
        assert_eq!(resolve_xdg_with(Some(OsStr::new("   ")), home, ".local/share"), PathBuf::from("/home/tester/.local/share"));
        assert_eq!(resolve_xdg_with(Some(OsStr::new("relative/dir")), home, ".cache"), PathBuf::from("/home/tester/.cache"));
        assert_eq!(resolve_xdg_with(Some(OsStr::new("/srv/xdg/data")), home, ".local/share"), PathBuf::from("/srv/xdg/data"));
        assert_eq!(resolve_xdg_with(Some(OsStr::new(" /srv/xdg/cache ")), home, ".cache"), PathBuf::from("/srv/xdg/cache"));
    }
}
