//! `HyprlandEvents` for QML: follows Hyprland's event socket (`.socket2.sock`) and
//! reports `configreloaded`, which Hyprland sends once a new configuration is in
//! effect: Omarchy's Appearance menu (window corners, animations), a theme switch or
//! the user's own config. Without Hyprland it stays idle.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};

const MAX_EVENT_LINE_BYTES: usize = 64 * 1024;

#[cxx_qt::bridge]
pub mod qobject {
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, connected, READ, NOTIFY)]
        type HyprlandEvents = super::HyprlandEventsRust;

        /// Hyprland applied a new configuration.
        #[qsignal]
        fn config_reloaded(self: Pin<&mut HyprlandEvents>);
    }

    impl cxx_qt::Threading for HyprlandEvents {}
    impl cxx_qt::Initialize for HyprlandEvents {}
}

#[derive(Default)]
pub struct HyprlandEventsRust {
    connected: bool,
}

/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`
fn event_socket() -> Option<PathBuf> {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .ok()
        .filter(|s| !s.is_empty())?;
    // The signature names a directory; anything that could leave it is refused
    if signature.contains('/') || signature.contains("..") {
        return None;
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty())?;
    Some(
        PathBuf::from(runtime)
            .join("hypr")
            .join(signature)
            .join(".socket2.sock"),
    )
}

/// The event name of a `name>>data` line.
fn event_name(line: &str) -> &str {
    line.split_once(">>").map_or(line, |(name, _)| name)
}

/// Reads one newline-delimited event line up to `max_bytes`.
/// Returns `Ok(None)` on EOF, `Ok(Some(true))` when `buf` contains a complete line
/// within `max_bytes`, and `Ok(Some(false))` when a line exceeded `max_bytes` and
/// its remainder was drained up to the next newline.
fn read_bounded_event_line<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_bytes: usize,
) -> std::io::Result<Option<bool>> {
    buf.clear();
    let n = reader.take(max_bytes as u64 + 1).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.len() > max_bytes {
        let ended = buf.last() == Some(&b'\n');
        buf.clear();
        if !ended {
            loop {
                let available = reader.fill_buf()?;
                if available.is_empty() {
                    break;
                }
                if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                    reader.consume(pos + 1);
                    break;
                }
                let len = available.len();
                reader.consume(len);
            }
        }
        return Ok(Some(false));
    }
    while buf.last().is_some_and(|&b| b == b'\n' || b == b'\r') {
        buf.pop();
    }
    Ok(Some(true))
}

impl cxx_qt::Initialize for qobject::HyprlandEvents {
    fn initialize(self: Pin<&mut Self>) {
        let Some(path) = event_socket() else { return };
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            // Reconnects after Hyprland restarts; stops once the QML object is gone
            let mut wait = Duration::from_millis(500);
            loop {
                if let Ok(stream) = UnixStream::connect(&path) {
                    wait = Duration::from_millis(500);
                    let set = |on: bool| {
                        qt.queue(move |mut obj: Pin<&mut qobject::HyprlandEvents>| {
                            if obj.connected != on {
                                obj.as_mut().rust_mut().get_mut().connected = on;
                                obj.as_mut().connected_changed();
                            }
                        })
                        .is_ok()
                    };
                    if !set(true) {
                        return;
                    }
                    // Events are short; lines exceeding 64 KiB are drained without unbounded allocation
                    let mut reader = BufReader::new(stream);
                    let mut buf = Vec::new();
                    while let Ok(Some(valid)) =
                        read_bounded_event_line(&mut reader, &mut buf, MAX_EVENT_LINE_BYTES)
                    {
                        if !valid {
                            continue;
                        }
                        let line = String::from_utf8_lossy(&buf);
                        if event_name(&line) == "configreloaded"
                            && qt
                                .queue(|obj: Pin<&mut qobject::HyprlandEvents>| {
                                    obj.config_reloaded()
                                })
                                .is_err()
                        {
                            return;
                        }
                    }
                    if !set(false) {
                        return;
                    }
                }
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_secs(10));
                // The object may be gone while Hyprland is down
                if qt
                    .queue(|_obj: Pin<&mut qobject::HyprlandEvents>| {})
                    .is_err()
                {
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_event_names() {
        assert_eq!(event_name("configreloaded>>"), "configreloaded");
        assert_eq!(event_name("workspace>>3"), "workspace");
        assert_eq!(
            event_name("activewindow>>omastudio,OmaStudio"),
            "activewindow"
        );
        assert_eq!(event_name("configreloaded"), "configreloaded");
    }

    #[test]
    fn bounded_event_reader_caps_memory_and_drains_oversized_lines() {
        let mut payload = vec![b'x'; 256 * 1024];
        payload.push(b'\n');
        payload.extend_from_slice(b"configreloaded>>\r\n");
        let mut reader = BufReader::new(payload.as_slice());
        let mut buf = Vec::new();

        // First line exceeds the 64 KiB cap and is drained without allocating 256 KiB
        assert_eq!(
            read_bounded_event_line(&mut reader, &mut buf, MAX_EVENT_LINE_BYTES).unwrap(),
            Some(false)
        );
        assert!(buf.capacity() <= MAX_EVENT_LINE_BYTES * 2);

        // Following valid line is read cleanly
        assert_eq!(
            read_bounded_event_line(&mut reader, &mut buf, MAX_EVENT_LINE_BYTES).unwrap(),
            Some(true)
        );
        assert_eq!(event_name(&String::from_utf8_lossy(&buf)), "configreloaded");

        // EOF
        assert_eq!(
            read_bounded_event_line(&mut reader, &mut buf, MAX_EVENT_LINE_BYTES).unwrap(),
            None
        );
    }
}
