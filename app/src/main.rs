//! OmaStudio desktop application: a native Qt 6 window for Wayland/Hyprland that
//! hosts the QML interface and drives `omastudio-engine`.
//!
//! ```text
//! omastudio-app [photo]                 open (or bring forward) the window
//! omastudio-app ipc <method> [args...]  call the running window, e.g. `ipc openPhoto F.RAF`
//! ```

mod host;
mod ipc;
mod process;
mod watched_file;

use std::path::Path;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-v") => println!("omastudio-app v{}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h") => usage(),
        Some("ipc") => std::process::exit(ipc_command(&args[1..])),
        photo => {
            let photo = photo.map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.into()));
            // A window is already open: hand it the photo, or bring it forward
            let request = match &photo {
                Some(p) => ipc::Request { method: "openPhoto".into(), args: vec![p.to_string_lossy().into()] },
                None => ipc::Request { method: "show".into(), args: vec![] },
            };
            if ipc::call(&request).is_some() {
                return;
            }
            std::process::exit(run(photo.as_deref()));
        }
    }
}

fn usage() {
    println!("OmaStudio v{}: The Complete Medium Format RAW Editor for Omarchy", env!("CARGO_PKG_VERSION"));
    println!("\nUsage:");
    println!("  omastudio-app [photo]                 Open the editor, or bring the running one forward");
    println!("  omastudio-app ipc <method> [args...]  Call the running editor (openPhoto, applyRecipe, getStatus, ...)");
}

/// `ipc <method> [args...]`; arguments that parse as JSON (numbers, true/false,
/// objects) are passed as such, anything else as a string.
fn ipc_command(args: &[String]) -> i32 {
    let args = match args.first().map(String::as_str) {
        Some("call") => &args[1..],
        _ => args,
    };
    let Some((method, rest)) = args.split_first() else {
        eprintln!("Usage: omastudio-app ipc <method> [args...]");
        return 2;
    };
    let args = rest.iter().map(|a| serde_json::from_str(a).unwrap_or_else(|_| serde_json::Value::String(a.clone()))).collect();
    match ipc::call(&ipc::Request { method: method.clone(), args }) {
        Some(reply) => {
            println!("{reply}");
            i32::from(reply.starts_with("Error"))
        }
        None => {
            eprintln!("OmaStudio is not running");
            1
        }
    }
}

fn run(photo: Option<&Path>) -> i32 {
    // Native Wayland first (fractional scaling, Hyprland window rules), X11 only as
    // a fallback; a fixed Controls style so the platform theme can't restyle the UI
    set_default_env("QT_QPA_PLATFORM", "wayland;xcb");
    set_default_env("QT_QUICK_CONTROLS_STYLE", "Basic");
    if let Some(p) = photo {
        // SAFETY: single-threaded at this point, before Qt starts
        unsafe { std::env::set_var("OMASTUDIO_OPEN_PATH", p) };
    }

    let mut app = QGuiApplication::new();
    if let Some(mut a) = app.as_mut() {
        a.as_mut().set_application_name(&QString::from("OmaStudio"));
        a.as_mut().set_organization_name(&QString::from("omastudio"));
        a.as_mut().set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
    }
    // The Wayland app id, which Hyprland window rules and the dock match against
    QGuiApplication::set_desktop_file_name(&QString::from("omastudio"));

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&main_qml());
    }
    let code = app.as_mut().map_or(1, |a| a.exec());
    ipc::cleanup();
    code
}

/// The interface compiled into the binary, or `$OMASTUDIO_QML_DIR/Main.qml` while
/// developing the QML.
fn main_qml() -> QUrl {
    match std::env::var("OMASTUDIO_QML_DIR") {
        Ok(dir) if !dir.is_empty() => QUrl::from(&QString::from(format!("file://{}/Main.qml", dir.trim_end_matches('/')).as_str())),
        _ => QUrl::from("qrc:/omastudio/qml/Main.qml"),
    }
}

fn set_default_env(name: &str, value: &str) {
    if std::env::var_os(name).is_none_or(|v| v.is_empty()) {
        // SAFETY: called before Qt or any other thread starts
        unsafe { std::env::set_var(name, value) };
    }
}
