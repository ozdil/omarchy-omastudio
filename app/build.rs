use cxx_qt_build::{CxxQtBuilder, QResource, QResources, QmlModule};

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

    CxxQtBuilder::new_qml_module(QmlModule::new("OmaStudio.Host"))
        .qt_module("Gui")
        .qt_module("Quick")
        .files(["src/host.rs", "src/process.rs", "src/watched_file.rs"])
        .qrc_resources(QResources::new().resource(ui))
        .build();
}
