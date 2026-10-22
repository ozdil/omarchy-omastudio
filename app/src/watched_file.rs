//! `WatchedFile` for QML: reads a small text file and, with `watch`, reloads it when
//! it changes (Omarchy swaps theme files when the theme changes). Replaces
//! Quickshell's FileView for the standalone application.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

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
        #[qproperty(QString, path, READ, WRITE = set_path, NOTIFY)]
        #[qproperty(QString, text, READ, NOTIFY)]
        #[qproperty(bool, watch, READ, WRITE = set_watch, NOTIFY)]
        type WatchedFile = super::WatchedFileRust;

        /// The file was read; `text` holds it.
        #[qsignal]
        fn loaded(self: Pin<&mut WatchedFile>);

        /// The file is missing, unreadable or larger than 1 MiB.
        #[qsignal]
        fn load_failed(self: Pin<&mut WatchedFile>);

        fn set_path(self: Pin<&mut WatchedFile>, path: QString);
        fn set_watch(self: Pin<&mut WatchedFile>, watch: bool);

        #[qinvokable]
        fn reload(self: Pin<&mut WatchedFile>);
    }

    impl cxx_qt::Threading for WatchedFile {}
}

const MAX_BYTES: u64 = 1 << 20;
const POLL: Duration = Duration::from_millis(750);

#[derive(Default)]
pub struct WatchedFileRust {
    path: QString,
    text: QString,
    watch: bool,
    /// Bumped whenever the path or watch flag changes; stops older watcher threads.
    generation: Arc<AtomicU64>,
}

impl Drop for WatchedFileRust {
    fn drop(&mut self) {
        // Stops the watcher thread
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// What identifies a version of the file (symlinks are followed, so a theme switch
/// that repoints a link counts as a change).
fn stamp(path: &PathBuf) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl qobject::WatchedFile {
    pub fn set_path(mut self: Pin<&mut Self>, path: QString) {
        if path == self.path {
            return;
        }
        self.as_mut().rust_mut().get_mut().path = path;
        self.as_mut().path_changed();
        self.as_mut().restart();
    }

    pub fn set_watch(mut self: Pin<&mut Self>, watch: bool) {
        if watch == self.watch {
            return;
        }
        self.as_mut().rust_mut().get_mut().watch = watch;
        self.as_mut().watch_changed();
        self.as_mut().restart();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let path = PathBuf::from(String::from(&self.path));
        let read = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.len() <= MAX_BYTES)
            .and_then(|_| std::fs::read(&path).ok());
        match read {
            Some(bytes) => {
                let text = QString::from(String::from_utf8_lossy(&bytes).as_ref());
                if text != self.text {
                    self.as_mut().rust_mut().get_mut().text = text;
                    self.as_mut().text_changed();
                }
                self.as_mut().loaded();
            }
            None => self.as_mut().load_failed(),
        }
    }

    fn restart(self: Pin<&mut Self>) {
        let current = self.generation.clone();
        let generation = current.fetch_add(1, Ordering::SeqCst) + 1;
        if self.path.is_empty() {
            return;
        }
        // Loaded from the event loop, so handlers declared next to `path` in QML are
        // connected by then
        let qt = self.qt_thread();
        let _ = qt.queue(move |mut obj: Pin<&mut qobject::WatchedFile>| {
            if obj.generation.load(Ordering::SeqCst) == generation {
                obj.as_mut().reload();
            }
        });
        if !self.watch {
            return;
        }
        let path = PathBuf::from(String::from(&self.path));
        std::thread::spawn(move || {
            let mut last = stamp(&path);
            loop {
                std::thread::sleep(POLL);
                if current.load(Ordering::SeqCst) != generation {
                    break;
                }
                let now = stamp(&path);
                if now == last {
                    continue;
                }
                last = now;
                if current.load(Ordering::SeqCst) != generation {
                    break;
                }
                let queued = qt.queue(move |mut obj: Pin<&mut qobject::WatchedFile>| {
                    if obj.generation.load(Ordering::SeqCst) == generation {
                        obj.as_mut().reload();
                    }
                });
                // Fails once the object is gone
                if queued.is_err() {
                    break;
                }
            }
        });
    }
}
