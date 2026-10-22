//! `Process` for QML: runs a command without a shell and reports its output on the
//! Qt thread. Replaces Quickshell's Process for the standalone application.
//!
//! Output is delivered either line by line (`lineRead`, for the engine daemon) or in
//! one piece when the process ends (`finished`); `maxOutputBytes` bounds both, and a
//! process that exceeds it is killed.

use std::io::{BufRead, BufReader, Read, Write};
use std::pin::Pin;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

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
        kill(&self.child);
    }
}

fn kill(child: &Mutex<Option<Child>>) {
    if let Ok(mut guard) = child.lock()
        && let Some(c) = guard.as_mut()
    {
        let _ = c.kill();
    }
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

    fn set_state(mut self: Pin<&mut Self>, running: bool) {
        if self.running != running {
            self.as_mut().rust_mut().get_mut().running = running;
            self.as_mut().running_changed();
        }
    }

    fn stop(mut self: Pin<&mut Self>) {
        let rust = self.as_mut().rust_mut().get_mut();
        rust.generation += 1;
        rust.stdin = None;
        kill(&rust.child);
        self.set_state(false);
    }

    fn start(mut self: Pin<&mut Self>) {
        let args: Vec<String> = QList::<QString>::from(&self.command).iter().map(String::from).collect();
        let Some((program, rest)) = args.split_first() else {
            self.as_mut().finished(-1, QString::from("empty command"));
            return;
        };
        let mut cmd = Command::new(program);
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
            let mut total = 0usize;
            let mut overflow = false;
            let mut output = Vec::new();
            if split {
                let mut reader = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match reader.read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            total += n;
                            if total > limit {
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
                kill(&child);
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
