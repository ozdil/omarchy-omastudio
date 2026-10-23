//! `Host` singleton for QML: environment, the engine binary, the photo to open at
//! start and calls arriving on the control socket.

use std::path::{Path, PathBuf};
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use crate::{ipc, language};

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
        #[qproperty(QString, ui_language, READ, NOTIFY)]
        #[qproperty(QString, language_choice, READ, NOTIFY)]
        #[qproperty(QString, system_language, READ, CONSTANT)]
        #[qproperty(QString, available_languages, READ, CONSTANT)]
        type Host = super::HostRust;

        /// A control-socket call; answer it with `ipcReply(id, text)`. `args` is a
        /// JSON array.
        #[qsignal]
        fn ipc_call(self: Pin<&mut Host>, id: i32, method: QString, args: QString);

        #[qinvokable]
        fn env(self: &Host, name: &QString) -> QString;

        #[qinvokable]
        fn ipc_reply(self: &Host, id: i32, text: &QString);

        /// Stores the interface language ("" follows the system) and updates
        /// `uiLanguage`; false when the choice could not be saved.
        #[qinvokable]
        fn set_ui_language(self: Pin<&mut Host>, code: &QString) -> bool;
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
    pub fn env(&self, name: &QString) -> QString {
        QString::from(std::env::var(String::from(name)).unwrap_or_default().as_str())
    }

    pub fn ipc_reply(&self, id: i32, text: &QString) {
        ipc::reply(id, String::from(text));
    }

    pub fn set_ui_language(mut self: Pin<&mut Self>, code: &QString) -> bool {
        let requested = String::from(code);
        let choice = if requested.is_empty() { String::new() } else { language::best_match(&requested).unwrap_or_default() };
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
}
