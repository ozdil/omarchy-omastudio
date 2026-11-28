use std::path::{Path, PathBuf};
use std::process::Command;

use cxx_qt_build::{CxxQtBuilder, QResource, QResources, QmlModule};
use qt_build_utils::QResourceFile;

fn main() {
    // The interface is served from qrc:/omastudio/qml; one flat directory, so every file
    // sees the others and the Theme singleton through qml/qmldir
    let mut ui = QResource::new().prefix("/omastudio".to_string());
    let mut files: Vec<_> = std::fs::read_dir("qml")
        .expect("app/qml")
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with(".qml") || name == "qmldir" {
            println!("cargo:rerun-if-changed={}", path.display());
            ui = ui.file(path);
        }
    }
    println!("cargo:rerun-if-changed=qml");
    // The colour app icon, for the About window wherever OmaStudio is installed
    println!("cargo:rerun-if-changed=../assets/icon.svg");
    ui = ui.file(QResourceFile::new("../assets/icon.svg").alias("qml/icon.svg".to_string()));

    // Every component must be declared in qmldir, or other files can't use it
    let qmldir = std::fs::read_to_string("qml/qmldir").expect("app/qml/qmldir");
    for entry in std::fs::read_dir("qml").expect("app/qml").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".qml") && name != "Main.qml" {
            assert!(
                qmldir
                    .lines()
                    .any(|l| l.split_whitespace().last() == Some(name.as_str())),
                "{name} is missing from app/qml/qmldir"
            );
        }
    }

    // Translations: app/i18n/qml_<code>.ts compiled to qrc:/omastudio/qml/i18n/qml_<code>.qm,
    // where QQmlApplicationEngine looks for them when Qt.uiLanguage changes
    let mut codes = Vec::new();
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("i18n");
    std::fs::create_dir_all(&out).expect("OUT_DIR/i18n");
    let mut sources: Vec<_> = std::fs::read_dir("i18n")
        .expect("app/i18n")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ts"))
        .collect();
    sources.sort();
    println!("cargo:rerun-if-changed=i18n");
    let lrelease = find_lrelease();
    for ts in sources {
        println!("cargo:rerun-if-changed={}", ts.display());
        let stem = ts.file_stem().unwrap().to_string_lossy().to_string();
        let code = stem.strip_prefix("qml_").expect("translations are named qml_<code>.ts").to_string();
        let qm = out.join(format!("{stem}.qm"));
        let status = Command::new(&lrelease)
            .args(["-silent", "-nounfinished", "-removeidentical"])
            .arg(&ts)
            .arg("-qm")
            .arg(&qm)
            .status()
            .unwrap_or_else(|e| panic!("{}: {e}", lrelease.display()));
        assert!(status.success(), "lrelease failed for {}", ts.display());
        ui = ui.file(QResourceFile::new(&qm).alias(format!("qml/i18n/{stem}.qm")));
        codes.push(code);
    }
    assert!(codes.iter().any(|c| c == "en"), "app/i18n/qml_en.ts is missing");
    println!("cargo:rustc-env=OMASTUDIO_LANGUAGES={}", codes.join(","));

    println!("cargo:rerun-if-changed=../src/raw/supported_extensions.inc");
    println!("cargo:rerun-if-changed=../src/recipe/look_presets.inc");
    println!("cargo:rerun-if-changed=../src/preview_shm.rs");
    println!("cargo:rerun-if-changed=cpp/preview_provider.h");
    println!("cargo:rerun-if-changed=cpp/preview_provider.cpp");
    println!("cargo:rerun-if-changed=cpp/viewport_rhi_bridge.h");
    println!("cargo:rerun-if-changed=cpp/viewport_rhi_common.cpp");
    println!("cargo:rerun-if-changed=cpp/viewport_rhi_item.h");
    println!("cargo:rerun-if-changed=cpp/viewport_rhi_item.cpp");
    println!("cargo:rerun-if-changed=cpp/viewport_rhi_stub.h");
    println!("cargo:rerun-if-changed=cpp/viewport_rhi_stub.cpp");
    println!("cargo:rerun-if-changed=shaders/viewport_rhi.vert");
    println!("cargo:rerun-if-changed=shaders/viewport_rhi.frag");

    let qrhi_env = detect_qrhi_build();
    if let Some(ref qrhi) = qrhi_env {
        let shader_out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("shaders");
        std::fs::create_dir_all(&shader_out).expect("OUT_DIR/shaders");
        for name in ["viewport_rhi.vert", "viewport_rhi.frag"] {
            let src = Path::new("shaders").join(name);
            let dst = shader_out.join(format!("{name}.qsb"));
            let status = Command::new(&qrhi.qsb)
                .args(["--glsl", "150,300 es", "--vulkan", "100", "-o"])
                .arg(&dst)
                .arg(&src)
                .status()
                .unwrap_or_else(|e| panic!("{}: {e}", qrhi.qsb.display()));
            assert!(status.success(), "qsb failed for {}", src.display());
            ui = ui.file(QResourceFile::new(&dst).alias(format!("shaders/{name}.qsb")));
        }
    }

    let mut builder = CxxQtBuilder::new_qml_module(QmlModule::new("OmaStudio.Host"))
        .qt_module("Gui")
        .qt_module("Quick")
        .include_dir("cpp")
        .cpp_file("cpp/preview_provider.cpp")
        .cpp_file("cpp/viewport_rhi_common.cpp");

    if let Some(ref qrhi) = qrhi_env {
        builder = builder
            .include_dir(&qrhi.gui_private_inc)
            .include_dir(&qrhi.gui_private_qtgui_inc)
            .cpp_file("cpp/viewport_rhi_item.h")
            .cpp_file("cpp/viewport_rhi_item.cpp");
    } else {
        builder = builder
            .cpp_file("cpp/viewport_rhi_stub.h")
            .cpp_file("cpp/viewport_rhi_stub.cpp");
    }

    builder
        .files([
            "src/history.rs",
            "src/host.rs",
            "src/hyprland.rs",
            "src/preview_provider.rs",
            "src/process.rs",
            "src/viewport_rhi.rs",
            "src/watched_file.rs",
        ])
        .qrc_resources(QResources::new().resource(ui))
        .build();
}

struct QrhiBuildEnv {
    qsb: PathBuf,
    gui_private_inc: PathBuf,
    gui_private_qtgui_inc: PathBuf,
}

fn qmake_query(var: &str) -> Option<String> {
    let qmakes: Vec<String> = match std::env::var("QMAKE") {
        Ok(q) if !q.is_empty() => vec![q],
        _ => vec!["qmake6".into(), "qmake".into()],
    };
    for qmake in qmakes {
        let Ok(out) = Command::new(&qmake).args(["-query", var]).output() else {
            continue;
        };
        if out.status.success() {
            let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

fn find_qsb() -> Option<PathBuf> {
    println!("cargo:rerun-if-env-changed=QSB");
    if let Some(p) = std::env::var_os("QSB").filter(|p| !p.is_empty()) {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    for var in ["QT_HOST_BINS", "QT_HOST_LIBEXECS", "QT_INSTALL_BINS"] {
        if let Some(dir) = qmake_query(var) {
            let candidate = Path::new(&dir).join("qsb");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for name in ["qsb-qt6", "qsb6", "qsb"] {
        if Command::new(name).arg("--version").output().is_ok_and(|o| o.status.success()) {
            return Some(PathBuf::from(name));
        }
    }
    None
}

fn detect_qrhi_build() -> Option<QrhiBuildEnv> {
    let ver = qmake_query("QT_VERSION")?;
    let mut parts = ver.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    if (major, minor) < (6, 7) {
        return None;
    }
    let headers = PathBuf::from(qmake_query("QT_INSTALL_HEADERS")?);
    let gui_private_inc = headers.join("QtGui").join(&ver);
    let gui_private_qtgui_inc = gui_private_inc.join("QtGui");
    let has_qrhi_hdr = gui_private_qtgui_inc.join("rhi/qrhi.h").is_file();
    let has_rhi_item = headers.join("QtQuick/QQuickRhiItem").is_file();
    let has_vulkan_hdr = Path::new("/usr/include/vulkan/vulkan.h").is_file();
    if !has_qrhi_hdr || !has_rhi_item || !has_vulkan_hdr {
        return None;
    }
    let qsb = find_qsb()?;
    Some(QrhiBuildEnv {
        qsb,
        gui_private_inc,
        gui_private_qtgui_inc,
    })
}

/// Qt's lrelease: `$LRELEASE`, else next to qmake (`qmake -query QT_HOST_BINS`), else
/// the usual names on PATH.
fn find_lrelease() -> PathBuf {
    println!("cargo:rerun-if-env-changed=LRELEASE");
    println!("cargo:rerun-if-env-changed=QMAKE");
    if let Some(p) = std::env::var_os("LRELEASE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let qmakes: Vec<String> = match std::env::var("QMAKE") {
        Ok(q) if !q.is_empty() => vec![q],
        _ => vec!["qmake6".into(), "qmake".into()],
    };
    for qmake in qmakes {
        for var in ["QT_HOST_BINS", "QT_HOST_LIBEXECS", "QT_INSTALL_BINS"] {
            let Ok(out) = Command::new(&qmake).args(["-query", var]).output() else { continue };
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let candidate = Path::new(&dir).join("lrelease");
            if out.status.success() && candidate.is_file() {
                return candidate;
            }
        }
    }
    for name in ["lrelease-qt6", "lrelease6", "lrelease"] {
        if Command::new(name).arg("-version").output().is_ok_and(|o| o.status.success()) {
            return PathBuf::from(name);
        }
    }
    panic!("Qt's lrelease was not found (Arch: pacman -S qt6-tools); set LRELEASE to its path");
}
