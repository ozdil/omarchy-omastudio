//! `Process` for QML: runs a command without a shell and reports its output on the
//! Qt thread. Replaces Quickshell's Process for the standalone application.
//!
//! Output is delivered either line by line (`lineRead`, for the engine daemon) or in
//! one piece when the process ends (`finished`). `maxOutputBytes` bounds the whole
//! output, or each line when `splitLines` is set (a long-running daemon prints far more
//! than that over a session); a process that exceeds it is killed.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::pin::Pin;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QString, QStringList};

#[cxx_qt::bridge]
pub mod qobject {
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
        #[qproperty(i32, max_output_bytes)]
        type HostProcess = super::HostProcessRust;

        #[qsignal]
        fn started(self: Pin<&mut HostProcess>);

        /// One line of output (without the newline), when `splitLines` is set.
        #[qsignal]
        fn line_read(self: Pin<&mut HostProcess>, line: QString);

        /// The process ended. `output` holds everything it printed unless
        /// `splitLines` is set. `exitCode` is -1 when it was killed or could not start,
        /// -2 when its output exceeded `maxOutputBytes`.
        #[qsignal]
        fn finished(self: Pin<&mut HostProcess>, exit_code: i32, output: QString);

        fn set_running(self: Pin<&mut HostProcess>, running: bool);

        /// Writes `text` to the process's standard input (needs `stdinEnabled`).
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

pub struct HostProcessRust {
    command: QStringList,
    running: bool,
    stdin_enabled: bool,
    split_lines: bool,
    max_output_bytes: i32,
    /// Bumped on every start and stop, so callbacks of an older run are ignored.
    generation: u64,
    child: Arc<Mutex<Option<Child>>>,
    stdin: Option<ChildStdin>,
}

impl Default for HostProcessRust {
    fn default() -> Self {
        Self {
            command: QStringList::default(),
            running: false,
            stdin_enabled: false,
            split_lines: false,
            max_output_bytes: DEFAULT_MAX_OUTPUT,
            generation: 0,
            child: Arc::new(Mutex::new(None)),
            stdin: None,
        }
    }
}

impl Drop for HostProcessRust {
    fn drop(&mut self) {
        self.stdin = None;
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
    Finished(i32, String),
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
        let Some(stdin) = self.as_mut().rust_mut().get_mut().stdin.as_mut() else {
            return false;
        };
        stdin.write_all(String::from(text).as_bytes()).and_then(|_| stdin.flush()).is_ok()
    }

    pub fn graceful_stop(mut self: Pin<&mut Self>, timeout_ms: i32) {
        let wait = Duration::from_millis(timeout_ms.clamp(0, 10_000) as u64);
        let child = {
            let rust = self.as_mut().rust_mut().get_mut();
            rust.generation += 1;
            rust.stdin = None;
            rust.child.clone()
        };
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
        let child = {
            let rust = self.as_mut().rust_mut().get_mut();
            rust.generation += 1;
            rust.stdin = None;
            rust.child.clone()
        };
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
        cmd.args(rest).stdout(Stdio::piped()).stderr(Stdio::inherit());
        cmd.stdin(if self.stdin_enabled { Stdio::piped() } else { Stdio::null() });
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.as_mut().finished(-1, QString::from(format!("{program}: {e}").as_str()));
                return;
            }
        };
        let stdout = child.stdout.take().expect("piped stdout");
        let split = self.split_lines;
        let limit = self.max_output_bytes.max(0) as usize;
        let qt = self.qt_thread();
        {
            let rust = self.as_mut().rust_mut().get_mut();
            rust.generation += 1;
            rust.stdin = child.stdin.take();
            // A new slot per run: a reader thread of an earlier run keeps its own
            rust.child = Arc::new(Mutex::new(Some(child)));
        }
        let generation = self.generation;
        let child = self.child.clone();
        self.as_mut().set_state(true);
        self.as_mut().started();

        std::thread::spawn(move || {
            let deliver = |event: Event| {
                qt.queue(move |mut obj: Pin<&mut qobject::HostProcess>| {
                    if obj.generation != generation {
                        return;
                    }
                    match event {
                        Event::Line(line) => obj.as_mut().line_read(QString::from(line.as_str())),
                        Event::Finished(code, output) => {
                            obj.as_mut().rust_mut().get_mut().stdin = None;
                            obj.as_mut().set_state(false);
                            obj.as_mut().finished(code, QString::from(output.as_str()));
                        }
                    }
                })
                .is_ok()
            };
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
                            if !deliver(Event::Line(String::from_utf8_lossy(&line).into_owned())) {
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
            let code = match status {
                _ if overflow => -2,
                Some(s) => s.code().unwrap_or(-1),
                None => -1,
            };
            deliver(Event::Finished(code, String::from_utf8_lossy(&output).into_owned()));
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
        for _ in 0..20 {
            // SAFETY: signal 0 checks process existence without delivering a signal.
            if unsafe { libc::kill(grandchild, 0) } != 0 {
                alive = false;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive, "grandchild process {grandchild} survived process-group reap");
    }
}
