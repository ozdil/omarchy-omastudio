//! Local control socket: `omastudio ipc call <method> [args]`, Omarchy agents and a
//! second launch talk to the running window through it.
//!
//! The socket lives in `$XDG_RUNTIME_DIR/omastudio/ipc.sock` (directory 0700, socket
//! 0600) and only answers processes of the same user. A request is one JSON line,
//! `{"method": "openPhoto", "args": ["/path/DSCF0001.RAF"]}`; the answer is one line
//! of text from the window's handler.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const MAX_REQUEST: u64 = 256 << 10;
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Connections answered at the same time
const MAX_CONNECTIONS: usize = 16;

static OWNS_SOCKET: AtomicBool = AtomicBool::new(false);
static LOCK_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn current_uid() -> libc::uid_t {
    // SAFETY: libc::getuid is thread-safe and infallible on Unix.
    unsafe { libc::getuid() }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    #[serde(default)]
    pub args: Vec<serde_json::Value>,
}

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", current_uid())));
    runtime.join("omastudio").join("ipc.sock")
}

/// Sends a request to the running window. None when no window is running.
pub fn call(request: &Request) -> Option<String> {
    let mut stream = UnixStream::connect(socket_path()).ok()?;
    stream.set_read_timeout(Some(REPLY_TIMEOUT + Duration::from_secs(1))).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
    let mut line = serde_json::to_string(request).ok()?;
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;
    let mut reply = String::new();
    BufReader::new(stream.take(MAX_REQUEST)).read_line(&mut reply).ok()?;
    Some(reply.trim_end().to_string())
}

type Pending = Mutex<HashMap<i32, Sender<String>>>;

fn pending() -> &'static Pending {
    static PENDING: OnceLock<Pending> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// Hands the window's answer to request `id` back to the waiting connection.
pub fn reply(id: i32, text: String) {
    if let Some(tx) = pending().lock().ok().and_then(|mut p| p.remove(&id)) {
        let _ = tx.send(text);
    }
}

/// Binds the socket and serves it on a background thread. `dispatch` runs on a
/// connection's thread and must hand the request to the Qt thread; it returns false
/// once the window is gone. Each connection gets its own thread, so a call that waits
/// for the window (a screenshot of a hidden workspace) doesn't hold up the others.
pub fn serve(dispatch: impl Fn(i32, Request) -> bool + Send + Sync + 'static) -> std::io::Result<()> {
    let path = socket_path();
    let dir = path.parent().expect("socket directory");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    if std::fs::symlink_metadata(dir)?.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refusing to bind IPC socket inside a symlinked directory",
        ));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;

    // Hold a process-lifetime advisory lock on ipc.lock so concurrent launches cannot
    // race between connect(), unlink(), and bind().
    let lock_path = dir.join("ipc.lock");
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&lock_path)?;
    // SAFETY: lock_file.as_raw_fd() is a valid open file descriptor owned by lock_file.
    let rc = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) || err.raw_os_error() == Some(libc::EAGAIN) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another OmaStudio window owns the socket",
            ));
        }
        return Err(err);
    }

    // Remove any stale socket left behind by a crashed window now that we hold the lock.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

    if let Ok(mut slot) = LOCK_FILE.get_or_init(|| Mutex::new(None)).lock() {
        *slot = Some(lock_file);
    }
    OWNS_SOCKET.store(true, Ordering::SeqCst);

    let dispatch = Arc::new(dispatch);
    let window_gone = Arc::new(AtomicBool::new(false));
    let next_id = Arc::new(AtomicI32::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if window_gone.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { continue };
            if !same_user(&stream) {
                continue;
            }
            // Bounded: a client opening connections in a loop can't start unlimited threads,
            // and non-blocking write ensures a silent peer cannot stall the accept loop.
            if active.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                let mut stream = stream;
                let _ = stream.set_nonblocking(true);
                let _ = stream.write(b"Error: too many requests\n");
                continue;
            }
            active.fetch_add(1, Ordering::Relaxed);
            let (dispatch, window_gone, next_id, active) =
                (Arc::clone(&dispatch), Arc::clone(&window_gone), Arc::clone(&next_id), Arc::clone(&active));
            std::thread::spawn(move || {
                handle(stream, &*dispatch, &window_gone, &next_id);
                active.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Ok(())
}

/// Reads one request, hands it to the window and writes its answer.
fn handle(stream: UnixStream, dispatch: &dyn Fn(i32, Request) -> bool, window_gone: &AtomicBool, next_id: &AtomicI32) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let mut line = String::new();
    if BufReader::new((&stream).take(MAX_REQUEST)).read_line(&mut line).is_err() {
        return;
    }
    let answer = match serde_json::from_str::<Request>(&line) {
        Ok(request) => {
            let id = next_id.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
            let (tx, rx) = mpsc::channel();
            if let Ok(mut p) = pending().lock() {
                p.insert(id, tx);
            }
            if !dispatch(id, request) {
                window_gone.store(true, Ordering::Relaxed);
                if let Ok(mut p) = pending().lock() {
                    p.remove(&id);
                }
                return;
            }
            let answer = rx.recv_timeout(REPLY_TIMEOUT).unwrap_or_else(|_| "Error: no answer from the window".to_string());
            if let Ok(mut p) = pending().lock() {
                p.remove(&id);
            }
            answer
        }
        Err(e) => format!("Error: malformed request: {e}"),
    };
    let mut stream = stream;
    let _ = stream.write_all(format!("{}\n", answer.replace('\n', " ")).as_bytes());
}

/// True when the peer runs as the same user as this process.
fn same_user(stream: &UnixStream) -> bool {
    let mut cred = libc::ucred { pid: 0, uid: u32::MAX, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred and len describe a valid ucred buffer for SO_PEERCRED
    let rc = unsafe {
        libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len)
    };
    rc == 0 && cred.uid == current_uid()
}

/// Removes the socket when the owning window closes. Secondary instances that failed
/// to acquire ownership never unlink the live primary window's socket.
pub fn cleanup() {
    if OWNS_SOCKET.swap(false, Ordering::SeqCst) {
        let _ = std::fs::remove_file(socket_path());
        if let Some(lock) = LOCK_FILE.get()
            && let Ok(mut slot) = lock.lock()
        {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_single_owner() {
        let dir = std::env::temp_dir().join(format!("omastudio-ipc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the only test touching the environment
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &dir) };
        assert!(call(&Request { method: "show".into(), args: vec![] }).is_none());

        // cleanup() when not owning the socket must not remove a pre-existing file
        let sock_dir = socket_path().parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&sock_dir).unwrap();
        std::fs::write(socket_path(), b"sentinel").unwrap();
        cleanup();
        assert!(socket_path().exists(), "non-owner cleanup must not remove existing socket");
        let _ = std::fs::remove_file(socket_path());

        serve(|id, request| {
            std::thread::spawn(move || {
                if request.method == "slow" {
                    std::thread::sleep(Duration::from_secs(3));
                }
                reply(id, format!("{} {}", request.method, request.args.len()))
            });
            true
        })
        .unwrap();
        // A call the window is slow to answer doesn't hold up the next one
        let slow = std::thread::spawn(|| call(&Request { method: "slow".into(), args: vec![] }));
        std::thread::sleep(Duration::from_millis(200));
        let started = std::time::Instant::now();
        assert_eq!(call(&Request { method: "fast".into(), args: vec![] }).as_deref(), Some("fast 0"));
        assert!(started.elapsed() < Duration::from_secs(2), "the fast call waited for the slow one");
        assert_eq!(slow.join().unwrap().as_deref(), Some("slow 0"));
        let mode = std::fs::metadata(socket_path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let answer = call(&Request { method: "openPhoto".into(), args: vec!["/a.RAF".into(), 2.into()] });
        assert_eq!(answer.as_deref(), Some("openPhoto 2"));
        assert!(serve(|_, _| true).is_err(), "a second window must not take over the socket");
        cleanup();
        assert!(!socket_path().exists(), "owner cleanup must remove socket");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
