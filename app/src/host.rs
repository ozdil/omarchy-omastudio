//! `Host` singleton for QML: environment, the engine binary, the photo to open at
//! start and calls arriving on the control socket.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use crate::{ipc, language};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cxx_qt::bridge]
pub mod qobject {
    // SAFETY: Qt C++ headers and types match cxx-qt-lib bindings for the lifetime of the Qt app.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(QString, open_path, READ, CONSTANT)]
        #[qproperty(QString, home_dir, READ, CONSTANT)]
        #[qproperty(QString, engine_path, READ, CONSTANT)]
        #[qproperty(QString, version, READ, CONSTANT)]
        #[qproperty(QString, ui_language, READ, NOTIFY)]
        #[qproperty(QString, language_choice, READ, NOTIFY)]
        #[qproperty(QString, system_language, READ, NOTIFY)]
        #[qproperty(QString, omarchy_language_file, READ, CONSTANT)]
        #[qproperty(QString, available_languages, READ, CONSTANT)]
        type Host = super::HostRust;

        /// A control-socket call; answer it with `ipcReply(id, text)`. `args` is a
        /// JSON array.
        #[qsignal]
        fn ipc_call(self: Pin<&mut Host>, id: i32, method: QString, args: QString);

        #[qinvokable]
        fn ipc_reply(self: &Host, id: i32, text: &QString);

        /// Validates an IPC screenshot output path (`.png` only, no symlinks or `..`,
        /// inside an allowlisted runtime/cache/Pictures directory). Returns the canonical
        /// safe path, or `""` if rejected.
        #[qinvokable]
        fn validate_screenshot_path(self: &Host, path: &QString) -> QString;

        /// The local path of a `file://` URL from a file dialog, "" for anything else.
        #[qinvokable]
        fn local_path(self: &Host, url: &QUrl) -> QString;

        /// A `file://` URL for a local path ("~/" is the home folder), for a file
        /// dialog's starting folder; empty for anything that is not a folder.
        #[qinvokable]
        fn folder_url(self: &Host, path: &QString) -> QUrl;

        /// Stores the interface language ("" follows the system) and updates
        /// `uiLanguage`; false when the choice could not be saved.
        #[qinvokable]
        fn set_ui_language(self: Pin<&mut Host>, code: &QString) -> bool;

        /// Reads the system language again (Omarchy's language file changed) and
        /// switches the interface when it follows the system.
        #[qinvokable]
        fn refresh_system_language(self: Pin<&mut Host>);

        /// Interface state saved by `saveUiState` (panels, sections), "" when none.
        #[qinvokable]
        fn ui_state(self: &Host) -> QString;

        #[qinvokable]
        fn save_ui_state(self: &Host, json: &QString);
    }

    impl cxx_qt::Threading for Host {}
    impl cxx_qt::Initialize for Host {}
}

pub struct HostRust {
    open_path: QString,
    home_dir: QString,
    engine_path: QString,
    version: QString,
    ui_language: QString,
    language_choice: QString,
    system_language: QString,
    omarchy_language_file: QString,
    available_languages: QString,
}

impl Default for HostRust {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        Self {
            open_path: QString::from(std::env::var("OMASTUDIO_OPEN_PATH").unwrap_or_default().as_str()),
            engine_path: QString::from(engine_path(&home).to_string_lossy().as_ref()),
            home_dir: QString::from(home.as_str()),
            version: QString::from(env!("CARGO_PKG_VERSION")),
            ui_language: QString::from(language::effective().as_str()),
            language_choice: QString::from(language::stored().as_str()),
            system_language: QString::from(language::system().as_str()),
            omarchy_language_file: QString::from(
                language::omarchy_file().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default().as_str(),
            ),
            available_languages: QString::from(language::AVAILABLE),
        }
    }
}

/// `OMASTUDIO_ENGINE`, else the engine next to this binary, else ~/.local/bin.
fn engine_path(home: &str) -> PathBuf {
    if let Some(p) = std::env::var_os("OMASTUDIO_ENGINE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let sibling = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("omastudio-engine")));
    if let Some(p) = sibling.filter(|p| p.is_file()) {
        return p;
    }
    let local = Path::new(home).join(".local/bin/omastudio-engine");
    if local.is_file() {
        return local;
    }
    PathBuf::from("/usr/bin/omastudio-engine")
}

impl cxx_qt::Initialize for qobject::Host {
    fn initialize(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let served = ipc::serve(move |id, request| {
            let args = serde_json::to_string(&request.args).unwrap_or_else(|_| "[]".to_string());
            qt.queue(move |host: Pin<&mut qobject::Host>| {
                host.ipc_call(id, QString::from(request.method.as_str()), QString::from(args.as_str()));
            })
            .is_ok()
        });
        if let Err(e) = served {
            eprintln!("omastudio: control socket unavailable: {e}");
        }
    }
}

impl qobject::Host {
    pub fn ipc_reply(&self, id: i32, text: &QString) {
        ipc::reply(id, String::from(text));
    }

    pub fn validate_screenshot_path(&self, path: &QString) -> QString {
        let raw = String::from(path);
        match validate_screenshot_path_with_roots(&raw, &screenshot_allowed_roots()) {
            Some(safe) => QString::from(safe.to_string_lossy().as_ref()),
            None => QString::default(),
        }
    }

    pub fn local_path(&self, url: &cxx_qt_lib::QUrl) -> QString {
        url.to_local_file().unwrap_or_default()
    }

    pub fn folder_url(&self, path: &QString) -> cxx_qt_lib::QUrl {
        let path = String::from(path);
        let path = match path.strip_prefix("~/") {
            Some(rest) => Path::new(&String::from(&self.home_dir)).join(rest),
            None => PathBuf::from(path),
        };
        if path.is_absolute() && path.is_dir() {
            cxx_qt_lib::QUrl::from_local_file(&QString::from(path.to_string_lossy().as_ref()))
        } else {
            cxx_qt_lib::QUrl::default()
        }
    }

    pub fn set_ui_language(mut self: Pin<&mut Self>, code: &QString) -> bool {
        let requested = String::from(code);
        let mut choice = if requested.is_empty() { String::new() } else { language::best_match(&requested).unwrap_or_default() };
        // Picking the system's own language keeps following the system
        if choice == language::system() {
            choice.clear();
        }
        let saved = language::store(&choice);
        if let Err(e) = &saved {
            eprintln!("omastudio: could not save the language: {e}");
        }
        let effective = if choice.is_empty() { language::system() } else { choice.clone() };
        let choice = QString::from(choice.as_str());
        if choice != self.language_choice {
            self.as_mut().rust_mut().get_mut().language_choice = choice;
            self.as_mut().language_choice_changed();
        }
        let effective = QString::from(effective.as_str());
        if effective != self.ui_language {
            self.as_mut().rust_mut().get_mut().ui_language = effective;
            self.as_mut().ui_language_changed();
        }
        saved.is_ok()
    }

    pub fn refresh_system_language(mut self: Pin<&mut Self>) {
        let system = QString::from(language::system().as_str());
        if system == self.system_language {
            return;
        }
        self.as_mut().rust_mut().get_mut().system_language = system.clone();
        self.as_mut().system_language_changed();
        if self.language_choice.is_empty() && system != self.ui_language {
            self.as_mut().rust_mut().get_mut().ui_language = system;
            self.as_mut().ui_language_changed();
        }
    }

    pub fn ui_state(&self) -> QString {
        let text = read_private_file_bounded(&ui_state_path(), UI_STATE_MAX).unwrap_or_default();
        QString::from(text.as_str())
    }

    pub fn save_ui_state(&self, json: &QString) {
        let text = String::from(json);
        if text.len() as u64 > UI_STATE_MAX || serde_json::from_str::<serde_json::Value>(&text).is_err() {
            return;
        }
        let path = ui_state_path();
        if let Err(e) = write_private_file_atomic(&path, text.as_bytes()) {
            eprintln!("omastudio: cannot save {}: {e}", path.display());
        }
    }
}

const UI_STATE_MAX: u64 = 64 * 1024;

/// `$XDG_CONFIG_HOME/omastudio/ui-state.json`
fn ui_state_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(&std::env::var("HOME").unwrap_or_default()).join(".config"));
    config.join("omastudio/ui-state.json")
}

fn screenshot_allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    // SAFETY: libc::getuid is thread-safe and infallible on Unix.
    let uid = unsafe { libc::getuid() };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
    roots.push(runtime.join("omastudio"));

    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|p| !p.is_empty()).map(|h| PathBuf::from(h).join(".cache")));
    if let Some(c) = cache {
        roots.push(c.join("omastudio"));
    }

    // MCP `window_screenshot` stages PNG captures in the engine's private /dev/shm dir.
    roots.push(PathBuf::from(format!("/dev/shm/omastudio-{uid}")));

    if let Some(pic) = std::env::var_os("XDG_PICTURES_DIR").filter(|p| !p.is_empty()) {
        roots.push(PathBuf::from(pic));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|p| !p.is_empty()) {
        roots.push(PathBuf::from(home).join("Pictures"));
    }
    roots
}

/// Validates that `raw` is an absolute `.png` path with no `..` traversal, no symlinked
/// target file, and a canonicalized parent directory inside one of `allowed_roots`.
fn validate_screenshot_path_with_roots(raw: &str, allowed_roots: &[PathBuf]) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    let path = Path::new(raw);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let ext = path.extension().and_then(|e| e.to_str())?;
    if !ext.eq_ignore_ascii_case("png") {
        return None;
    }
    let file_name = path.file_name()?;
    let parent = path.parent()?;
    // Parent must already exist and canonicalize inside an existing allowlisted root.
    let canon_parent = std::fs::canonicalize(parent).ok()?;
    if !canon_parent.is_dir() {
        return None;
    }
    let allowed = allowed_roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .ok()
            .is_some_and(|canon_root| canon_parent.starts_with(&canon_root))
    });
    if !allowed {
        return None;
    }
    let target = canon_parent.join(file_name);
    if let Ok(meta) = std::fs::symlink_metadata(&target)
        && (meta.file_type().is_symlink() || !meta.file_type().is_file())
    {
        return None;
    }
    Some(target)
}

/// Reads a private regular file without following symlinks (`O_NOFOLLOW`) and enforces a hard byte cap.
pub(crate) fn read_private_file_bounded(path: &Path, max_bytes: u64) -> io::Result<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "target is not a regular file"));
    }
    let mut text = String::new();
    file.take(max_bytes + 1).read_to_string(&mut text)?;
    if text.len() as u64 > max_bytes {
        return Err(io::Error::new(io::ErrorKind::OutOfMemory, "file exceeded maximum size"));
    }
    Ok(text)
}

/// Atomically writes a private config file (`0700` dir, unique `O_CREAT|O_EXCL|O_NOFOLLOW` `0600` temp file,
/// `sync_all()`, and `rename()`), refusing to write through symlinks.
pub(crate) fn write_private_file_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "parent traversal not permitted"));
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refusing to write through a symlink"));
    }
    let dir = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory"))?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    if std::fs::symlink_metadata(dir)?.file_type().is_symlink() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refusing to write inside a symlinked directory"));
    }
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));

    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".tmp.{}.{seq}.omastudio", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&tmp)?;
    let write_res = file.write_all(data).and_then(|_| file.sync_all());
    drop(file);
    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_path_validator_enforces_png_allowlist_and_no_symlinks() {
        let base = std::env::temp_dir().join(format!("omastudio-shot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let allowed_dir = base.join("allowed");
        let outside_dir = base.join("outside");
        std::fs::create_dir_all(&allowed_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();
        let roots = vec![allowed_dir.clone()];

        let ok = allowed_dir.join("capture.png");
        assert!(validate_screenshot_path_with_roots(&ok.to_string_lossy(), &roots).is_some());

        let wrong_ext = allowed_dir.join("capture.jpg");
        assert!(validate_screenshot_path_with_roots(&wrong_ext.to_string_lossy(), &roots).is_none());

        let outside = outside_dir.join("capture.png");
        assert!(validate_screenshot_path_with_roots(&outside.to_string_lossy(), &roots).is_none());

        let traversal = format!("{}/../outside/capture.png", allowed_dir.display());
        assert!(validate_screenshot_path_with_roots(&traversal, &roots).is_none());

        let target = outside_dir.join("victim.png");
        std::fs::write(&target, b"victim").unwrap();
        let symlink_out = allowed_dir.join("link.png");
        std::os::unix::fs::symlink(&target, &symlink_out).unwrap();
        assert!(validate_screenshot_path_with_roots(&symlink_out.to_string_lossy(), &roots).is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn private_file_write_is_mode_0600_and_rejects_symlinks() {
        let base = std::env::temp_dir().join(format!("omastudio-host-priv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let cfg_dir = base.join("omastudio");
        let file = cfg_dir.join("ui-state.json");

        write_private_file_atomic(&file, br#"{"pro":true}"#).unwrap();
        assert_eq!(std::fs::metadata(&cfg_dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(read_private_file_bounded(&file, UI_STATE_MAX).unwrap(), r#"{"pro":true}"#);

        let link = cfg_dir.join("link.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(write_private_file_atomic(&link, b"{}").is_err());
        assert!(read_private_file_bounded(&link, UI_STATE_MAX).is_err());

        let _ = std::fs::remove_dir_all(&base);
    }
}
