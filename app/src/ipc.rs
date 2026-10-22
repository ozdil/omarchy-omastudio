//! Local control socket: `omastudio ipc call <method> [args]`, Omarchy agents and a
//! second launch talk to the running window through it.
//!
//! The socket lives in `$XDG_RUNTIME_DIR/omastudio/ipc.sock` (directory 0700, socket
//! 0600) and only answers processes of the same user. A request is one JSON line,
//! `{"method": "openPhoto", "args": ["/path/DSCF0001.RAF"]}`; the answer is one line
//! of text from the window's handler.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const MAX_REQUEST: u64 = 256 << 10;
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    #[serde(default)]
    pub args: Vec<serde_json::Value>,
}

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    runtime.join("omastudio").join("ipc.sock")
}

/// Sends a request to the running window. None when no window is running.
pub fn call(request: &Request) -> Option<String> {
    let mut stream = UnixStream::connect(socket_path()).ok()?;
    stream.set_read_timeout(Some(REPLY_TIMEOUT + Duration::from_secs(1))).ok()?;
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

/// Binds the socket and serves it on a background thread. `dispatch` runs on that
/// thread and must hand the request to the Qt thread; it returns false once the
/// window is gone.
pub fn serve(dispatch: impl Fn(i32, Request) -> bool + Send + 'static) -> std::io::Result<()> {
    let path = socket_path();
    let dir = path.parent().expect("socket directory");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    if UnixStream::connect(&path).is_ok() {
        return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "another OmaStudio window owns the socket"));
    }
    // A socket left behind by a crashed window
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

    std::thread::spawn(move || {
        let mut next_id = 0i32;
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if !same_user(&stream) {
                continue;
            }
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new((&stream).take(MAX_REQUEST)).read_line(&mut line).is_err() {
                continue;
            }
            let answer = match serde_json::from_str::<Request>(&line) {
                Ok(request) => {
                    next_id = next_id.wrapping_add(1);
                    let (tx, rx) = mpsc::channel();
                    if let Ok(mut p) = pending().lock() {
                        p.insert(next_id, tx);
                    }
                    if !dispatch(next_id, request) {
                        break;
                    }
                    let answer = rx.recv_timeout(REPLY_TIMEOUT).unwrap_or_else(|_| "Error: no answer from the window".to_string());
                    if let Ok(mut p) = pending().lock() {
                        p.remove(&next_id);
                    }
                    answer
                }
                Err(e) => format!("Error: malformed request: {e}"),
            };
            let mut stream = stream;
            let _ = stream.write_all(format!("{}\n", answer.replace('\n', " ")).as_bytes());
        }
    });
    Ok(())
}

/// True when the peer runs as the same user as this process.
fn same_user(stream: &UnixStream) -> bool {
    let mut cred = libc::ucred { pid: 0, uid: u32::MAX, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred and len describe a valid ucred buffer for SO_PEERCRED
    let rc = unsafe {
        libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len)
    };
    rc == 0 && cred.uid == unsafe { libc::getuid() }
}

/// Removes the socket when the window closes.
pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_single_owner() {
        let dir = std::env::temp_dir().join(format!("omastudio-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the only test touching the environment
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &dir) };
        assert!(call(&Request { method: "show".into(), args: vec![] }).is_none());

        serve(|id, request| {
            std::thread::spawn(move || reply(id, format!("{} {}", request.method, request.args.len())));
            true
        })
        .unwrap();
        let mode = std::fs::metadata(socket_path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let answer = call(&Request { method: "openPhoto".into(), args: vec!["/a.RAF".into(), 2.into()] });
        assert_eq!(answer.as_deref(), Some("openPhoto 2"));
        assert!(serve(|_, _| true).is_err(), "a second window must not take over the socket");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
