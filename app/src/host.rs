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

mod supported_extensions {
    include!("../../src/raw/supported_extensions.inc");
}

mod look_presets {
    include!("../../src/recipe/look_presets.inc");
}

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
        #[qproperty(QString, raw_extensions, READ, CONSTANT)]
        #[qproperty(QString, image_extensions, READ, CONSTANT)]
        #[qproperty(bool, software_renderer, READ, CONSTANT)]
        #[qproperty(QString, display_color_space, READ, CONSTANT)]
        #[qproperty(bool, display_hdr_active, READ, CONSTANT)]
        #[qproperty(i32, display_max_luminance, READ, CONSTANT)]
        #[qproperty(i32, display_sdr_white_nits, READ, CONSTANT)]
        #[qproperty(bool, display_color_management, READ, CONSTANT)]
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

        /// Applies one of the 5 built-in look presets (matching `Recipe::fuji_classic_chrome`
        /// and friends in `src/recipe/mod.rs`) on top of `current_recipe_json`, resetting
        /// look-specific fields while keeping WB (unless overridden by the preset), exposure,
        /// crop and film simulation. Returns the updated recipe JSON, or `""` for unknown names.
        #[qinvokable]
        fn apply_look_preset(self: &Host, current_recipe_json: &QString, name: &QString) -> QString;
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
    raw_extensions: QString,
    image_extensions: QString,
    software_renderer: bool,
    display_color_space: QString,
    display_hdr_active: bool,
    display_max_luminance: i32,
    display_sdr_white_nits: i32,
    display_color_management: bool,
}

impl Default for HostRust {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let raw_json = serde_json::to_string(supported_extensions::RAW_EXTENSIONS).unwrap_or_else(|_| "[]".into());
        let img_json = serde_json::to_string(supported_extensions::RASTER_EXTENSIONS).unwrap_or_else(|_| "[]".into());
        let software_renderer = std::env::var("QT_QUICK_BACKEND")
            .ok()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("software"));
        let display = crate::wayland_color::detected();
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
            raw_extensions: QString::from(raw_json.as_str()),
            image_extensions: QString::from(img_json.as_str()),
            software_renderer,
            display_color_space: QString::from(display.color_space.as_str()),
            display_hdr_active: display.hdr_active,
            display_max_luminance: display.max_luminance_nits as i32,
            display_sdr_white_nits: display.sdr_white_nits as i32,
            display_color_management: display.protocol_supported,
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

    pub fn apply_look_preset(&self, current_recipe_json: &QString, name: &QString) -> QString {
        let cur = String::from(current_recipe_json);
        let preset_name = String::from(name);
        match apply_look_preset_json(&cur, &preset_name) {
            Some(updated) => QString::from(updated.as_str()),
            None => QString::default(),
        }
    }
}

/// Applies one of the 5 shared look presets on top of `current_recipe_json`.
///
/// Resets look-related fields (tone, presence, parametric curve, HSL, grading wheels,
/// sharpening, vignette) to engine defaults before applying the preset so looks replace
/// rather than stack, while preserving exposure, crop, rotation, film simulation, and
/// (unless overridden by the preset) white balance.
fn apply_look_preset_json(current_recipe_json: &str, name: &str) -> Option<String> {
    let spec = look_presets::find_look_preset(name)?;
    let mut val: serde_json::Value = serde_json::from_str(current_recipe_json).ok()?;
    let obj = val.as_object_mut()?;

    let zero_keys = [
        "contrast",
        "highlights",
        "shadows",
        "whites",
        "blacks",
        "texture",
        "clarity",
        "dehaze",
        "vibrance",
        "saturation",
        "curve_highlights",
        "curve_lights",
        "curve_darks",
        "curve_shadows",
        "vignette",
        "lift_luma",
        "gamma_luma",
        "gain_luma",
        "offset_luma",
    ];
    for key in zero_keys {
        obj.insert(key.to_string(), serde_json::json!(0.0));
    }
    obj.insert("sharpness".to_string(), serde_json::json!(spec.sharpness));
    obj.insert("lift".to_string(), serde_json::json!(spec.lift));
    obj.insert("lift_luma".to_string(), serde_json::json!(spec.lift_luma));
    obj.insert("gamma".to_string(), serde_json::json!([0.0, 0.0, 0.0]));
    obj.insert("gain".to_string(), serde_json::json!(spec.gain));
    obj.insert("gain_luma".to_string(), serde_json::json!(spec.gain_luma));
    obj.insert("offset".to_string(), serde_json::json!([0.0, 0.0, 0.0]));
    obj.insert("hsl_hue".to_string(), serde_json::json!(spec.hsl_hue));
    obj.insert("hsl_sat".to_string(), serde_json::json!(spec.hsl_sat));
    obj.insert("hsl_lum".to_string(), serde_json::json!(spec.hsl_lum));

    if let Some(temp) = spec.wb_temperature {
        obj.insert("wb_temperature".to_string(), serde_json::json!(temp));
    }
    if let Some(tint) = spec.wb_tint {
        obj.insert("wb_tint".to_string(), serde_json::json!(tint));
    }
    obj.insert("contrast".to_string(), serde_json::json!(spec.contrast));
    obj.insert("highlights".to_string(), serde_json::json!(spec.highlights));
    obj.insert("shadows".to_string(), serde_json::json!(spec.shadows));
    obj.insert("whites".to_string(), serde_json::json!(spec.whites));
    obj.insert("blacks".to_string(), serde_json::json!(spec.blacks));
    obj.insert("texture".to_string(), serde_json::json!(spec.texture));
    obj.insert("clarity".to_string(), serde_json::json!(spec.clarity));
    obj.insert("vibrance".to_string(), serde_json::json!(spec.vibrance));
    obj.insert("saturation".to_string(), serde_json::json!(spec.saturation));
    obj.insert("vignette".to_string(), serde_json::json!(spec.vignette));
    obj.insert("preset_name".to_string(), serde_json::json!(spec.id));

    serde_json::to_string(&val).ok()
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

    #[test]
    fn apply_look_preset_replaces_previous_look_and_preserves_exposure_and_wb() {
        let base = r#"{"exposure":1.25,"wb_temperature":6200.0,"wb_tint":3.0,"contrast":40.0,"vignette":-30.0,"film_simulation":"fuji_provia"}"#;
        let out = apply_look_preset_json(base, "muted").expect("alias resolves to Fuji Classic Chrome");
        let val: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(val["preset_name"], "Fuji Classic Chrome");
        assert_eq!(val["exposure"], 1.25);
        assert_eq!(val["wb_temperature"], 6200.0);
        assert_eq!(val["film_simulation"], "fuji_provia");
        assert_eq!(val["contrast"], 15.0);
        assert_eq!(val["vignette"], -12.0);
        assert_eq!(val["hsl_hue"][5], -8.0);
        assert_eq!(val["hsl_sat"][1], 10.0);
        assert_eq!(val["hsl_sat"][5], -20.0);

        // Switching to Kodak Portra resets previous vignette and sets Portra's warm WB
        let portra = apply_look_preset_json(&out, "Kodak Portra 400").unwrap();
        let pval: serde_json::Value = serde_json::from_str(&portra).unwrap();
        assert_eq!(pval["vignette"], 0.0);
        assert_eq!(pval["wb_temperature"], 5700.0);
        assert_eq!(pval["wb_tint"], 4.0);
        assert_eq!(pval["hsl_lum"][1], 12.0);

        assert!(apply_look_preset_json(base, "No Such Preset").is_none());
    }
}
