//! `Process` for QML: runs a command without a shell and reports its output on the
//! Qt thread. Replaces Quickshell's Process for the standalone application.
//!
//! Output is delivered either line by line (`lineRead`, for the engine daemon) or in
//! one piece when the process ends (`finished`). `maxOutputBytes` bounds the whole
//! output, or each line when `splitLines` is set (a long-running daemon prints far more
//! than that over a session); a process that exceeds it is killed.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::pin::Pin;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QString, QStringList};

use crate::preview_provider;
use crate::preview_shm::{
    ENV_PREVIEW_FD, PreviewRingReader, create_preview_socketpair, memfd_disabled_by_env,
    recv_dmabuf_frame_with_ack,
};
use crate::viewport_rhi;

#[cxx_qt::bridge]
pub mod qobject {
    // SAFETY: Qt C++ headers and types match cxx-qt-lib bindings for the lifetime of the Qt application.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element = "Process"]
        #[qproperty(QStringList, command)]
        #[qproperty(bool, running, READ, WRITE = set_running, NOTIFY)]
        #[qproperty(bool, stdin_enabled)]
        #[qproperty(bool, split_lines)]
        #[qproperty(bool, preview_channel)]
        #[qproperty(i32, max_output_bytes)]
        type HostProcess = super::HostProcessRust;

        #[qsignal]
        fn started(self: Pin<&mut HostProcess>);

        /// One line of output (without the newline), when `splitLines` is set.
        #[qsignal]
        fn line_read(self: Pin<&mut HostProcess>, line: QString);

        /// One bounded line of standard error output (without the trailing newline).
        #[qsignal]
        fn stderr_line(self: Pin<&mut HostProcess>, line: QString);

        /// The process ended. `output` holds everything it printed unless
        /// `splitLines` is set. `exitCode` is -1 when it was killed or could not start,
        /// -2 when its output exceeded `maxOutputBytes`.
        #[qsignal]
        fn finished(self: Pin<&mut HostProcess>, exit_code: i32, output: QString);

        fn set_running(self: Pin<&mut HostProcess>, running: bool);

        /// Enqueues `text` for asynchronous delivery to the process's standard input
        /// (needs `stdinEnabled`) without blocking the Qt GUI thread.
        #[qinvokable]
        fn write(self: Pin<&mut HostProcess>, text: &QString) -> bool;

        /// Closes stdin so the child can flush pending work and exit cleanly within
        /// `timeout_ms`, escalating to SIGTERM then SIGKILL on its process group.
        #[qinvokable]
        fn graceful_stop(self: Pin<&mut HostProcess>, timeout_ms: i32);
    }

    impl cxx_qt::Threading for HostProcess {}
}

const DEFAULT_MAX_OUTPUT: i32 = 64 << 20;
const STDIN_QUEUE_CAPACITY: usize = 64;
const STDERR_LINE_LIMIT: usize = 4096;

/// Non-blocking handle to a dedicated stdin writer thread so `HostProcess::write`
/// never stalls the Qt GUI thread when a child's pipe buffer is full.
/// Dropping the handle closes the channel; the writer flushes queued chunks and
/// then drops `ChildStdin` to deliver clean EOF.
struct StdinWriter {
    tx: SyncSender<Vec<u8>>,
}

impl StdinWriter {
    fn spawn(mut stdin: ChildStdin) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(STDIN_QUEUE_CAPACITY);
        std::thread::spawn(move || {
            while let Ok(chunk) = rx.recv() {
                if stdin.write_all(&chunk).and_then(|_| stdin.flush()).is_err() {
                    break;
                }
            }
            drop(stdin);
        });
        Self { tx }
    }

    fn try_write(&self, data: Vec<u8>) -> bool {
        match self.tx.try_send(data) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

/// Shared handle to the parent half of the `SOCK_SEQPACKET` preview channel so stopping or
/// dropping `HostProcess` immediately wakes a blocked `recvmsg()` worker thread.
struct PreviewSocketGuard {
    fd: Arc<OwnedFd>,
}

impl PreviewSocketGuard {
    fn shutdown(&self) {
        // SAFETY: `self.fd` is an open Unix socket descriptor owned by this `Arc<OwnedFd>`.
        unsafe {
            libc::shutdown(self.fd.as_raw_fd(), libc::SHUT_RDWR);
        }
    }
}

impl Drop for PreviewSocketGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct HostProcessRust {
    command: QStringList,
    running: bool,
    stdin_enabled: bool,
    split_lines: bool,
    preview_channel: bool,
    max_output_bytes: i32,
    /// Bumped on every start and stop, so callbacks of an older run are ignored.
    generation: u64,
    child: Arc<Mutex<Option<Child>>>,
    stdin: Option<StdinWriter>,
    preview_sock: Option<PreviewSocketGuard>,
}

impl Default for HostProcessRust {
    fn default() -> Self {
        Self {
            command: QStringList::default(),
            running: false,
            stdin_enabled: false,
            split_lines: false,
            preview_channel: false,
            max_output_bytes: DEFAULT_MAX_OUTPUT,
            generation: 0,
            child: Arc::new(Mutex::new(None)),
            stdin: None,
            preview_sock: None,
        }
    }
}

impl Drop for HostProcessRust {
    fn drop(&mut self) {
        self.stdin = None;
        self.preview_sock = None;
        if self.preview_channel {
            preview_provider::clear_ring(self.generation);
            viewport_rhi::clear_frames(self.generation);
        }
        terminate(&self.child, Duration::ZERO);
    }
}

fn terminate(child: &Mutex<Option<Child>>, wait_for_exit: Duration) {
    let taken = child.lock().ok().and_then(|mut guard| guard.take());
    if let Some(mut c) = taken {
        reap_process_group(&mut c, wait_for_exit);
    }
}

/// Waits up to `wait_for_exit` for `child` to exit cleanly on its own (e.g. after stdin EOF),
/// then terminates its entire process group (`SIGTERM` -> grace -> `SIGKILL`) and reaps it.
fn reap_process_group(child: &mut Child, wait_for_exit: Duration) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    if !wait_for_exit.is_zero() {
        let deadline = Instant::now() + wait_for_exit;
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(Duration::from_millis(10).min(remaining));
        }
    }
    let pid = child.id() as libc::pid_t;
    if pid > 0 {
        // SAFETY: `child` was spawned with `process_group(0)` and has not been reaped yet,
        // so `pid` still identifies its own process group.
        unsafe { libc::kill(-pid, libc::SIGTERM) };
        let grace = if wait_for_exit.is_zero() {
            Duration::from_millis(10)
        } else {
            Duration::from_millis(200)
        };
        let term_deadline = Instant::now() + grace;
        while Instant::now() < term_deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            let remaining = term_deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(Duration::from_millis(5).min(remaining));
        }
        // SAFETY: `child` has still not been reaped, so `-pid` still targets its process group.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    } else {
        let _ = child.kill();
    }
    let _ = child.wait();
}

enum Event {
    Line(String),
    StderrLine(String),
    Finished(i32, String),
}

/// Reads lines from `reader` with a hard per-line byte bound while draining any
/// remaining bytes on overlong lines so child processes can never block on a full pipe.
fn read_bounded_stderr_lines<R: Read>(reader: R, limit: usize, mut on_line: impl FnMut(String)) {
    let mut br = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut truncated = false;
    loop {
        let chunk = match br.fill_buf() {
            Ok([]) | Err(_) => break,
            Ok(c) => c,
        };
        let mut consumed = 0;
        while consumed < chunk.len() {
            if let Some(rel) = chunk[consumed..].iter().position(|&b| b == b'\n') {
                let piece = &chunk[consumed..consumed + rel];
                if buf.len() < limit {
                    let take = (limit - buf.len()).min(piece.len());
                    buf.extend_from_slice(&piece[..take]);
                    if take < piece.len() {
                        truncated = true;
                    }
                } else if !piece.is_empty() {
                    truncated = true;
                }
                while buf.last().is_some_and(|&b| b == b'\r') {
                    buf.pop();
                }
                if !buf.is_empty() {
                    let mut line = String::from_utf8_lossy(&buf).into_owned();
                    if truncated {
                        line.push_str(" ...");
                    }
                    on_line(line);
                }
                buf.clear();
                truncated = false;
                consumed += rel + 1;
            } else {
                let piece = &chunk[consumed..];
                if buf.len() < limit {
                    let take = (limit - buf.len()).min(piece.len());
                    buf.extend_from_slice(&piece[..take]);
                    if take < piece.len() {
                        truncated = true;
                    }
                } else if !piece.is_empty() {
                    truncated = true;
                }
                consumed = chunk.len();
            }
        }
        br.consume(consumed);
    }
    while buf.last().is_some_and(|&b| b == b'\r') {
        buf.pop();
    }
    if !buf.is_empty() {
        let mut line = String::from_utf8_lossy(&buf).into_owned();
        if truncated {
            line.push_str(" ...");
        }
        on_line(line);
    }
}

fn deliver_event(qt: &cxx_qt::CxxQtThread<qobject::HostProcess>, generation: u64, event: Event) -> bool {
    qt.queue(move |mut obj: Pin<&mut qobject::HostProcess>| {
        if obj.generation != generation {
            return;
        }
        match event {
            Event::Line(line) => obj.as_mut().line_read(QString::from(line.as_str())),
            Event::StderrLine(line) => obj.as_mut().stderr_line(QString::from(line.as_str())),
            Event::Finished(code, output) => {
                let rust = obj.as_mut().rust_mut().get_mut();
                rust.stdin = None;
                rust.preview_sock = None;
                if rust.preview_channel {
                    preview_provider::clear_ring(generation);
                    viewport_rhi::clear_frames(generation);
                }
                obj.as_mut().set_state(false);
                obj.as_mut().finished(code, QString::from(output.as_str()));
            }
        }
    })
    .is_ok()
}

impl qobject::HostProcess {
    pub fn set_running(mut self: Pin<&mut Self>, running: bool) {
        if running == self.running {
            return;
        }
        if running {
            self.as_mut().start();
        } else {
            self.as_mut().stop();
        }
    }

    pub fn write(mut self: Pin<&mut Self>, text: &QString) -> bool {
        let Some(stdin) = self.as_mut().rust_mut().get_mut().stdin.as_ref() else {
            return false;
        };
        stdin.try_write(String::from(text).into_bytes())
    }

    pub fn graceful_stop(mut self: Pin<&mut Self>, timeout_ms: i32) {
        let wait = Duration::from_millis(timeout_ms.clamp(0, 10_000) as u64);
        let (child, prev_gen, has_preview) = {
            let rust = self.as_mut().rust_mut().get_mut();
            let prev = rust.generation;
            rust.generation += 1;
            rust.stdin = None;
            rust.preview_sock = None;
            (rust.child.clone(), prev, rust.preview_channel)
        };
        if has_preview {
            preview_provider::clear_ring(prev_gen);
            viewport_rhi::clear_frames(prev_gen);
        }
        terminate(&child, wait);
        self.set_state(false);
    }

    fn set_state(mut self: Pin<&mut Self>, running: bool) {
        if self.running != running {
            self.as_mut().rust_mut().get_mut().running = running;
            self.as_mut().running_changed();
        }
    }

    fn stop(mut self: Pin<&mut Self>) {
        let (child, prev_gen, has_preview) = {
            let rust = self.as_mut().rust_mut().get_mut();
            let prev = rust.generation;
            rust.generation += 1;
            rust.stdin = None;
            rust.preview_sock = None;
            (rust.child.clone(), prev, rust.preview_channel)
        };
        if has_preview {
            preview_provider::clear_ring(prev_gen);
            viewport_rhi::clear_frames(prev_gen);
        }
        terminate(&child, Duration::ZERO);
        self.set_state(false);
    }

    fn start(mut self: Pin<&mut Self>) {
        let args: Vec<String> = QList::<QString>::from(&self.command).iter().map(String::from).collect();
        let Some((program, rest)) = args.split_first() else {
            self.as_mut().finished(-1, QString::from("empty command"));
            return;
        };
        let mut cmd = Command::new(program);
        cmd.process_group(0);
        cmd.args(rest).stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.stdin(if self.stdin_enabled { Stdio::piped() } else { Stdio::null() });

        let use_preview = self.preview_channel && !memfd_disabled_by_env();
        let mut preview_pair: Option<(OwnedFd, OwnedFd)> = None;
        if use_preview {
            match create_preview_socketpair() {
                Ok((parent_fd, child_fd)) => {
                    let child_raw = child_fd.as_raw_fd();
                    cmd.env(ENV_PREVIEW_FD, child_raw.to_string());
                    // SAFETY: `fcntl(F_SETFD, 0)` is async-signal-safe and clears FD_CLOEXEC
                    // strictly on the inherited child half of the socketpair before `exec`.
                    unsafe {
                        cmd.pre_exec(move || {
                            if libc::fcntl(child_raw, libc::F_SETFD, 0) != 0 {
                                return Err(std::io::Error::last_os_error());
                            }
                            Ok(())
                        });
                    }
                    preview_pair = Some((parent_fd, child_fd));
                }
                Err(e) => {
                    eprintln!("omastudio-app: failed to create preview socketpair: {e}");
                }
            }
        }

        let spawn_result = cmd.spawn();
        // Close parent's copy of `child_fd` immediately regardless of spawn outcome.
        let parent_preview_fd = preview_pair.map(|(parent_fd, child_fd)| {
            drop(child_fd);
            Arc::new(parent_fd)
        });

        let mut child = match spawn_result {
            Ok(c) => c,
            Err(e) => {
                self.as_mut().finished(-1, QString::from(format!("{program}: {e}").as_str()));
                return;
            }
        };
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let split = self.split_lines;
        let limit = self.max_output_bytes.max(0) as usize;
        let qt = self.qt_thread();
        {
            let rust = self.as_mut().rust_mut().get_mut();
            rust.generation += 1;
            rust.stdin = child.stdin.take().map(StdinWriter::spawn);
            // A new slot per run: a reader thread of an earlier run keeps its own
            rust.child = Arc::new(Mutex::new(Some(child)));
            rust.preview_sock = parent_preview_fd
                .as_ref()
                .map(|fd| PreviewSocketGuard { fd: Arc::clone(fd) });
        }
        let generation = self.generation;
        if let Some(parent_fd) = parent_preview_fd {
            std::thread::spawn(move || {
                match PreviewRingReader::recv_and_ack(parent_fd.as_raw_fd()) {
                    Ok(Some(reader)) => {
                        preview_provider::install_ring(generation, reader);
                        loop {
                            match recv_dmabuf_frame_with_ack(parent_fd.as_raw_fd(), |frame| {
                                viewport_rhi::park_frame(generation, frame);
                            }) {
                                Ok(true) => {}
                                Ok(false) => break,
                                Err(e) => {
                                    eprintln!("omastudio-app: dma-buf preview channel closed: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("omastudio-app: memfd preview handshake failed: {e}"),
                }
            });
        }
        let child = self.child.clone();
        self.as_mut().set_state(true);
        self.as_mut().started();

        let qt_err = qt.clone();
        let stderr_thread = std::thread::spawn(move || {
            read_bounded_stderr_lines(stderr, STDERR_LINE_LIMIT, |line| {
                eprintln!("{line}");
                let _ = deliver_event(&qt_err, generation, Event::StderrLine(line));
            });
        });

        std::thread::spawn(move || {
            let mut overflow = false;
            let mut output = Vec::new();
            if split {
                let mut reader = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match (&mut reader).take(limit as u64 + 1).read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if n > limit {
                                overflow = true;
                                break;
                            }
                            while line.last().is_some_and(|&b| b == b'\n' || b == b'\r') {
                                line.pop();
                            }
                            if !deliver_event(&qt, generation, Event::Line(String::from_utf8_lossy(&line).into_owned())) {
                                break;
                            }
                        }
                    }
                }
            } else {
                let mut reader = stdout.take(limit as u64 + 1);
                let _ = reader.read_to_end(&mut output);
                overflow = output.len() > limit;
            }
            if overflow {
                terminate(&child, Duration::ZERO);
                output.clear();
            }
            let status = child.lock().ok().and_then(|mut g| g.take()).and_then(|mut c| c.wait().ok());
            let _ = stderr_thread.join();
            let code = match status {
                _ if overflow => -2,
                Some(s) => s.code().unwrap_or(-1),
                None => -1,
            };
            deliver_event(&qt, generation, Event::Finished(code, String::from_utf8_lossy(&output).into_owned()));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graceful_reap_lets_child_flush_on_stdin_eof() {
        let out = std::env::temp_dir().join(format!("omastudio-proc-flush-{}", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let mut cmd = Command::new("/bin/sh");
        cmd.process_group(0)
            .args(["-c", "sleep 0.05; cat > \"$1\"", "sh"])
            .arg(&out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().expect("spawn flush helper");
        {
            let mut stdin = child.stdin.take().expect("piped stdin");
            stdin.write_all(b"autosave_ok\n").expect("write stdin");
            stdin.flush().expect("flush stdin");
        }
        reap_process_group(&mut child, Duration::from_secs(2));
        let written = std::fs::read_to_string(&out).expect("child flushed output before exit");
        let _ = std::fs::remove_file(&out);
        assert_eq!(written, "autosave_ok\n");
    }

    fn is_process_alive(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 checks process existence without delivering a signal.
        if unsafe { libc::kill(pid, 0) } != 0 {
            return false;
        }
        // In minimal containers where PID 1 does not reap reparented orphans, a
        // SIGKILLed grandchild remains a zombie (State: Z) even though it is dead.
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let Some(rest) = stat.rsplit_once(')').map(|(_, s)| s.trim_start()) else {
            return false;
        };
        !matches!(rest.as_bytes().first(), Some(b'Z' | b'X' | b'x'))
    }

    #[test]
    fn process_group_reap_kills_sigterm_ignoring_descendants() {
        let mut cmd = Command::new("/bin/sh");
        cmd.process_group(0)
            .args(["-c", "trap '' TERM; sleep 30 & echo $!; wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().expect("spawn group helper");
        let mut line = String::new();
        let mut reader = BufReader::new(child.stdout.take().expect("piped stdout"));
        reader.read_line(&mut line).expect("read grandchild pid");
        let grandchild: libc::pid_t = line.trim().parse().expect("valid grandchild pid");

        let started = Instant::now();
        reap_process_group(&mut child, Duration::ZERO);
        assert!(started.elapsed() < Duration::from_secs(1), "reap took too long");

        // Give the kernel a brief moment to finish tearing down the killed grandchild
        let mut alive = true;
        for _ in 0..50 {
            if !is_process_alive(grandchild) {
                alive = false;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive, "grandchild process {grandchild} survived process-group reap");
    }

    #[test]
    fn async_stdin_writer_flushes_queued_bytes_before_eof_on_drop() {
        let out = std::env::temp_dir().join(format!("omastudio-async-stdin-{}", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let mut cmd = Command::new("/bin/sh");
        cmd.process_group(0)
            .args(["-c", "sleep 0.05; cat > \"$1\"", "sh"])
            .arg(&out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().expect("spawn async stdin helper");
        let writer = StdinWriter::spawn(child.stdin.take().expect("piped stdin"));
        assert!(writer.try_write(b"line1\n".to_vec()));
        assert!(writer.try_write(b"line2\n".to_vec()));
        drop(writer);
        reap_process_group(&mut child, Duration::from_secs(2));
        let written = std::fs::read_to_string(&out).expect("child flushed async stdin");
        let _ = std::fs::remove_file(&out);
        assert_eq!(written, "line1\nline2\n");
    }

    #[test]
    fn async_stdin_writer_does_not_block_caller_when_pipe_is_full() {
        let mut cmd = Command::new("/bin/sh");
        cmd.process_group(0)
            .args(["-c", "sleep 10"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().expect("spawn sleeper");
        let writer = StdinWriter::spawn(child.stdin.take().expect("piped stdin"));

        // Pushing >1 MiB into a child that never reads must return immediately (non-blocking)
        // and reject cleanly once the bounded queue fills instead of stalling the caller.
        let started = Instant::now();
        let mut rejected = false;
        let chunk = vec![b'a'; 64 * 1024];
        for _ in 0..200 {
            if !writer.try_write(chunk.clone()) {
                rejected = true;
                break;
            }
        }
        assert!(started.elapsed() < Duration::from_millis(500), "try_write blocked caller");
        assert!(rejected, "bounded queue must report full instead of blocking");

        drop(writer);
        reap_process_group(&mut child, Duration::ZERO);
    }

    #[test]
    fn stderr_reader_truncates_overlong_lines_and_drains_large_stream() {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"warn: line one\r\n");
        payload.extend(std::iter::repeat_n(b'x', 16_384));
        payload.extend_from_slice(b"\nfinal line");

        let mut lines = Vec::new();
        read_bounded_stderr_lines(std::io::Cursor::new(payload), 64, |l| lines.push(l));
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "warn: line one");
        assert!(lines[1].ends_with(" ..."), "overlong line should be marked truncated: {}", lines[1]);
        assert_eq!(lines[1].len(), 64 + 4);
        assert_eq!(lines[2], "final line");
    }
}
