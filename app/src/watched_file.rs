//! `WatchedFile` for QML: reads a small text file and, with `watch`, reloads it when
//! it changes (Omarchy swaps theme files when the theme changes). Replaces
//! Quickshell's FileView for the standalone application.
//!
//! All active `WatchedFile` instances share a single Linux `inotify` background hub
//! (`OnceLock<InotifyHub>`) watching parent directories (and symlink-ancestor directories)
//! with zero idle CPU wakeups, reading file contents off the GUI thread with a bounded
//! `O_RDONLY` read, and falling back to a single shared slow-poll loop only if `inotify`
//! cannot be initialized or a watched directory does not exist yet.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

#[cxx_qt::bridge]
pub mod qobject {
    // SAFETY: Qt C++ headers and types match cxx-qt-lib bindings for the lifetime of the Qt application.
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
const FALLBACK_POLL_MS: i32 = 2000;
const INOTIFY_MASK: u32 = libc::IN_CLOSE_WRITE
    | libc::IN_MOVED_TO
    | libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_ATTRIB
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;

/// Metadata identity of a watched file (following symlinks, including device+inode
/// so repointing a symlink to a different file of the same length/mtime still triggers).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
    dev: u64,
    ino: u64,
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some(FileStamp {
        modified: meta.modified().ok(),
        len: meta.len(),
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// Bounded `O_RDONLY` read avoiding TOCTOU between stat and read.
fn read_bounded_text(path: &Path) -> Option<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut buf = Vec::with_capacity((meta.len().min(MAX_BYTES)) as usize);
    file.take(MAX_BYTES + 1).read_to_end(&mut buf).ok()?;
    if buf.len() as u64 > MAX_BYTES {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Collects the directories to watch for `path`:
/// - `path`'s lexical parent directory (catches atomic rename/create/symlink swap on the file)
/// - Any parent directory of a symlink component along `path` (catches Omarchy repointing
///   `~/.local/state/omarchy/current/theme` when switching themes)
/// - The canonical parent directory of `path` if it differs (catches in-place writes to the target).
fn watch_directories_for(path: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push_unique = |d: PathBuf| {
        if !d.as_os_str().is_empty() && !dirs.contains(&d) {
            dirs.push(d);
        }
    };

    if let Some(parent) = path.parent() {
        push_unique(parent.to_path_buf());
    }

    // Walk prefix components to find intermediate symlinks (e.g. `.../current/theme`).
    let mut prefix = PathBuf::new();
    for comp in path.components() {
        let prev = prefix.clone();
        prefix.push(comp);
        if let Ok(meta) = std::fs::symlink_metadata(&prefix)
            && meta.file_type().is_symlink()
            && !prev.as_os_str().is_empty()
        {
            push_unique(prev);
        }
    }

    if let Ok(canon) = std::fs::canonicalize(path)
        && let Some(parent) = canon.parent()
    {
        push_unique(parent.to_path_buf());
    }

    dirs
}

type DeliverCallback = Box<dyn FnMut(Option<String>) -> bool + Send + 'static>;

struct WatchEntry {
    path: PathBuf,
    last_stamp: Option<FileStamp>,
    deliver: DeliverCallback,
}

struct HubState {
    entries: HashMap<u64, WatchEntry>,
    dirty: bool,
}

struct InotifyHub {
    state: Arc<Mutex<HubState>>,
    wake_fd: RawFd,
}

static NEXT_WATCH_ID: AtomicU64 = AtomicU64::new(1);
static HUB: OnceLock<InotifyHub> = OnceLock::new();

impl InotifyHub {
    fn global() -> &'static Self {
        HUB.get_or_init(Self::start)
    }

    fn start() -> Self {
        let state = Arc::new(Mutex::new(HubState {
            entries: HashMap::new(),
            dirty: false,
        }));
        // SAFETY: `eventfd` with valid flags (`EFD_NONBLOCK | EFD_CLOEXEC`) returns a new owned fd or -1.
        let wake_fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        let thread_state = Arc::clone(&state);
        let _ = std::thread::Builder::new()
            .name("omastudio-inotify".into())
            .spawn(move || hub_thread(thread_state, wake_fd));
        Self { state, wake_fd }
    }

    fn wake(&self) {
        if self.wake_fd >= 0 {
            let one: u64 = 1;
            // SAFETY: `wake_fd` is a valid eventfd owned for the lifetime of the process;
            // `one` is an initialized 8-byte buffer.
            let _ = unsafe {
                libc::write(
                    self.wake_fd,
                    (&one as *const u64).cast::<libc::c_void>(),
                    std::mem::size_of::<u64>(),
                )
            };
        }
    }

    fn register(&self, path: PathBuf, deliver: DeliverCallback) -> u64 {
        let id = NEXT_WATCH_ID.fetch_add(1, Ordering::Relaxed);
        let initial_stamp = file_stamp(&path);
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.entries.insert(
                id,
                WatchEntry {
                    path,
                    last_stamp: initial_stamp,
                    deliver,
                },
            );
            s.dirty = true;
        }
        self.wake();
        id
    }

    fn unregister(&self, id: u64) {
        if id == 0 {
            return;
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.entries.remove(&id).is_some() {
            s.dirty = true;
            drop(s);
            self.wake();
        }
    }
}

fn drain_eventfd(fd: RawFd) {
    if fd < 0 {
        return;
    }
    let mut val: u64 = 0;
    // SAFETY: `fd` is a valid non-blocking eventfd and `val` is an 8-byte writable buffer.
    let _ = unsafe {
        libc::read(
            fd,
            (&mut val as *mut u64).cast::<libc::c_void>(),
            std::mem::size_of::<u64>(),
        )
    };
}

fn add_inotify_watch(inotify_fd: RawFd, dir: &Path) -> Option<i32> {
    let c_dir = CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `inotify_fd` is a valid inotify fd and `c_dir` is NUL-terminated.
    let wd = unsafe { libc::inotify_add_watch(inotify_fd, c_dir.as_ptr(), INOTIFY_MASK) };
    (wd >= 0).then_some(wd)
}

fn rm_inotify_watch(inotify_fd: RawFd, wd: i32) {
    // SAFETY: `inotify_fd` is a valid inotify fd and `wd` was returned by `inotify_add_watch`.
    let _ = unsafe { libc::inotify_rm_watch(inotify_fd, wd) };
}

fn sync_watches(
    inotify_fd: RawFd,
    state: &Arc<Mutex<HubState>>,
    dir_to_wd: &mut HashMap<PathBuf, i32>,
    wd_to_dirs: &mut HashMap<i32, HashSet<PathBuf>>,
    entry_dirs: &mut HashMap<u64, Vec<PathBuf>>,
) -> bool {
    let snapshot: Vec<(u64, PathBuf)> = {
        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        s.dirty = false;
        s.entries.iter().map(|(&id, e)| (id, e.path.clone())).collect()
    };

    entry_dirs.clear();
    let mut wanted_dirs: HashSet<PathBuf> = HashSet::new();
    let mut any_missing_watch = false;

    for (id, path) in snapshot {
        let dirs = watch_directories_for(&path);
        if dirs.is_empty() {
            any_missing_watch = true;
        }
        for d in &dirs {
            wanted_dirs.insert(d.clone());
        }
        entry_dirs.insert(id, dirs);
    }

    // Remove watches for directories no longer referenced by any entry.
    let stale_dirs: Vec<PathBuf> = dir_to_wd
        .keys()
        .filter(|d| !wanted_dirs.contains(*d))
        .cloned()
        .collect();
    for dir in stale_dirs {
        if let Some(wd) = dir_to_wd.remove(&dir)
            && let Some(set) = wd_to_dirs.get_mut(&wd)
        {
            set.remove(&dir);
            if set.is_empty() {
                wd_to_dirs.remove(&wd);
                rm_inotify_watch(inotify_fd, wd);
            }
        }
    }

    // Add watches for newly needed directories.
    for dir in wanted_dirs {
        if dir_to_wd.contains_key(&dir) {
            continue;
        }
        if let Some(wd) = add_inotify_watch(inotify_fd, &dir) {
            dir_to_wd.insert(dir.clone(), wd);
            wd_to_dirs.entry(wd).or_default().insert(dir);
        } else {
            // Directory does not exist yet or watch limit reached: enable gentle slow fallback poll.
            any_missing_watch = true;
        }
    }

    any_missing_watch
}

fn check_entries(state: &Arc<Mutex<HubState>>, candidate_ids: Option<&HashSet<u64>>) -> bool {
    let checks: Vec<(u64, PathBuf, Option<FileStamp>)> = {
        let s = state.lock().unwrap_or_else(|e| e.into_inner());
        s.entries
            .iter()
            .filter(|(id, _)| candidate_ids.is_none_or(|set| set.contains(id)))
            .map(|(&id, e)| (id, e.path.clone(), e.last_stamp))
            .collect()
    };

    let mut any_changed = false;
    for (id, path, prev_stamp) in checks {
        let now_stamp = file_stamp(&path);
        if now_stamp == prev_stamp {
            continue;
        }
        any_changed = true;
        // Read file contents on the watcher thread (never on the GUI thread).
        let text = if now_stamp.is_some() {
            read_bounded_text(&path)
        } else {
            None
        };
        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = s.entries.get_mut(&id) {
            entry.last_stamp = now_stamp;
            let keep = (entry.deliver)(text);
            if !keep {
                s.entries.remove(&id);
                s.dirty = true;
            }
        }
    }
    any_changed
}

fn hub_thread(state: Arc<Mutex<HubState>>, wake_fd: RawFd) {
    // SAFETY: `inotify_init1` with valid flags (`IN_NONBLOCK | IN_CLOEXEC`) returns a new owned fd or -1.
    let inotify_fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if inotify_fd < 0 {
        fallback_poll_loop(state, wake_fd);
        return;
    }

    let mut dir_to_wd: HashMap<PathBuf, i32> = HashMap::new();
    let mut wd_to_dirs: HashMap<i32, HashSet<PathBuf>> = HashMap::new();
    let mut entry_dirs: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut need_fallback_timer = sync_watches(
        inotify_fd,
        &state,
        &mut dir_to_wd,
        &mut wd_to_dirs,
        &mut entry_dirs,
    );

    let mut ev_buf = [0u8; 4096];
    let header_size = std::mem::size_of::<libc::inotify_event>();

    loop {
        let mut fds = [
            libc::pollfd {
                fd: inotify_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let nfds = if wake_fd >= 0 { 2 } else { 1 };
        let timeout_ms = if need_fallback_timer || wake_fd < 0 {
            FALLBACK_POLL_MS
        } else {
            -1
        };

        // SAFETY: `fds` points to `nfds` valid `pollfd` entries.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), nfds as libc::nfds_t, timeout_ms) };
        if rc < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
            continue;
        }

        let mut resync = false;
        if nfds == 2 && (fds[1].revents & libc::POLLIN) != 0 {
            drain_eventfd(wake_fd);
            resync = true;
        }

        if rc == 0 {
            // Fallback timer tick for entries whose parent directories didn't exist yet.
            if check_entries(&state, None) || need_fallback_timer {
                resync = true;
            }
        } else if (fds[0].revents & libc::POLLIN) != 0 {
            let mut triggered_wds: HashSet<i32> = HashSet::new();
            let mut overflow = false;

            loop {
                // SAFETY: `inotify_fd` is non-blocking; `ev_buf` is a 4096-byte writable buffer.
                let n = unsafe {
                    libc::read(
                        inotify_fd,
                        ev_buf.as_mut_ptr().cast::<libc::c_void>(),
                        ev_buf.len(),
                    )
                };
                if n <= 0 {
                    break;
                }
                let n = n as usize;
                let mut offset = 0usize;
                while offset + header_size <= n {
                    // SAFETY: `ev_buf[offset..offset + header_size]` holds an `inotify_event`
                    // written by the Linux kernel; `read_unaligned` avoids alignment assumptions.
                    let ev = unsafe {
                        std::ptr::read_unaligned(
                            ev_buf.as_ptr().add(offset).cast::<libc::inotify_event>(),
                        )
                    };
                    if (ev.mask & libc::IN_Q_OVERFLOW) != 0 {
                        overflow = true;
                    }
                    if (ev.mask & (libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_IGNORED)) != 0 {
                        if let Some(dirs) = wd_to_dirs.remove(&ev.wd) {
                            for d in dirs {
                                dir_to_wd.remove(&d);
                            }
                        }
                        resync = true;
                    }
                    triggered_wds.insert(ev.wd);
                    let step = header_size.saturating_add(ev.len as usize);
                    if step == 0 {
                        break;
                    }
                    offset = offset.saturating_add(step);
                }
            }

            if overflow {
                check_entries(&state, None);
                resync = true;
            } else if !triggered_wds.is_empty() {
                let mut triggered_dirs: HashSet<PathBuf> = HashSet::new();
                for wd in &triggered_wds {
                    if let Some(dirs) = wd_to_dirs.get(wd) {
                        for d in dirs {
                            triggered_dirs.insert(d.clone());
                        }
                    }
                }
                let candidates: HashSet<u64> = entry_dirs
                    .iter()
                    .filter(|(_, dirs)| dirs.iter().any(|d| triggered_dirs.contains(d)))
                    .map(|(&id, _)| id)
                    .collect();
                let changed = if candidates.is_empty() {
                    check_entries(&state, None)
                } else {
                    check_entries(&state, Some(&candidates))
                };
                if changed {
                    // A symlink target may have switched; refresh canonical target watches.
                    resync = true;
                }
            }
        }

        if !resync {
            let s = state.lock().unwrap_or_else(|e| e.into_inner());
            resync = s.dirty;
        }
        if resync {
            need_fallback_timer = sync_watches(
                inotify_fd,
                &state,
                &mut dir_to_wd,
                &mut wd_to_dirs,
                &mut entry_dirs,
            );
        }
    }
}

fn fallback_poll_loop(state: Arc<Mutex<HubState>>, wake_fd: RawFd) {
    loop {
        if wake_fd >= 0 {
            let mut pfd = libc::pollfd {
                fd: wake_fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `pfd` is a valid single-element `pollfd` array.
            let _ = unsafe { libc::poll(&mut pfd, 1, FALLBACK_POLL_MS) };
            if (pfd.revents & libc::POLLIN) != 0 {
                drain_eventfd(wake_fd);
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(FALLBACK_POLL_MS as u64));
        }
        let _ = check_entries(&state, None);
    }
}

#[derive(Default)]
pub struct WatchedFileRust {
    path: QString,
    text: QString,
    watch: bool,
    /// Bumped whenever the path or watch flag changes; discards stale queued callbacks.
    generation: Arc<AtomicU64>,
    /// Active registration ID inside [`InotifyHub`], or 0 when not watching.
    watch_id: u64,
}

impl Drop for WatchedFileRust {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if self.watch_id != 0 {
            InotifyHub::global().unregister(self.watch_id);
            self.watch_id = 0;
        }
    }
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
        let loaded = read_bounded_text(&path);
        self.as_mut().apply_loaded(loaded);
    }

    fn apply_loaded(mut self: Pin<&mut Self>, loaded: Option<String>) {
        match loaded {
            Some(content) => {
                let text = QString::from(content.as_str());
                if text != self.text {
                    self.as_mut().rust_mut().get_mut().text = text;
                    self.as_mut().text_changed();
                }
                self.as_mut().loaded();
            }
            None => self.as_mut().load_failed(),
        }
    }

    fn restart(mut self: Pin<&mut Self>) {
        let prev_id = self.as_mut().rust_mut().get_mut().watch_id;
        if prev_id != 0 {
            InotifyHub::global().unregister(prev_id);
            self.as_mut().rust_mut().get_mut().watch_id = 0;
        }

        let current = self.generation.clone();
        let generation = current.fetch_add(1, Ordering::SeqCst) + 1;
        if self.path.is_empty() {
            return;
        }

        // Initial load queued through the event loop so handlers declared next to `path`
        // in QML are connected first.
        let path = PathBuf::from(String::from(&self.path));
        let initial_path = path.clone();
        let qt = self.qt_thread();
        let _ = qt.queue(move |mut obj: Pin<&mut qobject::WatchedFile>| {
            if obj.generation.load(Ordering::SeqCst) == generation {
                let content = read_bounded_text(&initial_path);
                obj.as_mut().apply_loaded(content);
            }
        });

        if !self.watch {
            return;
        }

        let watch_qt = self.qt_thread();
        let id = InotifyHub::global().register(path, Box::new(move |loaded| {
            if current.load(Ordering::SeqCst) != generation {
                return false;
            }
            let gen_ref = Arc::clone(&current);
            watch_qt
                .queue(move |mut obj: Pin<&mut qobject::WatchedFile>| {
                    if gen_ref.load(Ordering::SeqCst) == generation {
                        obj.as_mut().apply_loaded(loaded);
                    }
                })
                .is_ok()
        }));
        self.as_mut().rust_mut().get_mut().watch_id = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "omastudio_inotify_{label}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn inotify_hub_detects_atomic_rename_and_symlink_theme_switch() {
        let dir = test_dir("theme_switch");
        let theme_a = dir.join("theme_a");
        let theme_b = dir.join("theme_b");
        std::fs::create_dir_all(&theme_a).unwrap();
        std::fs::create_dir_all(&theme_b).unwrap();
        std::fs::write(theme_a.join("colors.toml"), "accent = \"#111111\"\n").unwrap();
        std::fs::write(theme_b.join("colors.toml"), "accent = \"#222222\"\n").unwrap();

        let current_link = dir.join("current");
        std::os::unix::fs::symlink(&theme_a, &current_link).unwrap();
        let watched_file = current_link.join("colors.toml");

        let (tx, rx) = mpsc::channel();
        let id = InotifyHub::global().register(
            watched_file.clone(),
            Box::new(move |text| {
                let _ = tx.send(text);
                true
            }),
        );
        // Give the hub a brief moment to attach inotify watches.
        std::thread::sleep(Duration::from_millis(80));

        // 1. Atomic rename inside theme_a (like an editor saving colors.toml).
        let tmp = theme_a.join(".colors.toml.tmp");
        std::fs::write(&tmp, "accent = \"#aaaaaa\"\n").unwrap();
        std::fs::rename(&tmp, theme_a.join("colors.toml")).unwrap();
        let ev1 = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("expected inotify event on atomic rename");
        assert_eq!(ev1.as_deref(), Some("accent = \"#aaaaaa\"\n"));

        // 2. Atomic symlink swap at ancestor directory (like Omarchy switching themes).
        let tmp_link = dir.join(".current.tmp");
        std::os::unix::fs::symlink(&theme_b, &tmp_link).unwrap();
        std::fs::rename(&tmp_link, &current_link).unwrap();
        let ev2 = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("expected inotify event on symlink theme switch");
        assert_eq!(ev2.as_deref(), Some("accent = \"#222222\"\n"));

        InotifyHub::global().unregister(id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bounded_read_rejects_oversized_files() {
        let dir = test_dir("oversized");
        let big = dir.join("huge.txt");
        std::fs::write(&big, vec![b'x'; (MAX_BYTES as usize) + 16]).unwrap();
        assert!(read_bounded_text(&big).is_none());

        let ok = dir.join("ok.txt");
        std::fs::write(&ok, "hello").unwrap();
        assert_eq!(read_bounded_text(&ok).as_deref(), Some("hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
