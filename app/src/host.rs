//! `Host` singleton for QML: environment, the engine binary, the photo to open at
//! start and calls arriving on the control socket.

use std::path::{Path, PathBuf};
use std::pin::Pin;

use cxx_qt::Threading;
use cxx_qt_lib::QString;

use crate::ipc;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
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
        type Host = super::HostRust;

        /// A control-socket call; answer it with `ipcReply(id, text)`. `args` is a
        /// JSON array.
        #[qsignal]
        fn ipc_call(self: Pin<&mut Host>, id: i32, method: QString, args: QString);

        #[qinvokable]
        fn env(self: &Host, name: &QString) -> QString;

        #[qinvokable]
        fn ipc_reply(self: &Host, id: i32, text: &QString);

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
}

impl Default for HostRust {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        Self {
            open_path: QString::from(std::env::var("OMASTUDIO_OPEN_PATH").unwrap_or_default().as_str()),
            engine_path: QString::from(engine_path(&home).to_string_lossy().as_ref()),
            home_dir: QString::from(home.as_str()),
            version: QString::from(env!("CARGO_PKG_VERSION")),
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
    pub fn env(&self, name: &QString) -> QString {
        QString::from(std::env::var(String::from(name)).unwrap_or_default().as_str())
    }

    pub fn ipc_reply(&self, id: i32, text: &QString) {
        ipc::reply(id, String::from(text));
    }

    pub fn ui_state(&self) -> QString {
        let text = std::fs::metadata(ui_state_path())
            .ok()
            .filter(|m| m.is_file() && m.len() <= UI_STATE_MAX)
            .and_then(|_| std::fs::read_to_string(ui_state_path()).ok())
            .unwrap_or_default();
        QString::from(text.as_str())
    }

    pub fn save_ui_state(&self, json: &QString) {
        let text = String::from(json);
        if text.len() as u64 > UI_STATE_MAX || serde_json::from_str::<serde_json::Value>(&text).is_err() {
            return;
        }
        let path = ui_state_path();
        let tmp = path.with_extension("json.tmp");
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&tmp, text))
            .and_then(|_| std::fs::rename(&tmp, &path));
        if let Err(e) = written {
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
