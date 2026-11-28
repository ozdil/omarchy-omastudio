//! Thread-safe `memfd` preview ring store and Qt `QQuickImageProvider` bridge.

use std::pin::Pin;
use std::sync::{Arc, OnceLock, RwLock};

use cxx_qt_lib::QQmlApplicationEngine;

use crate::preview_shm::{PreviewRingReader, parse_provider_id};

#[cxx_qt::bridge]
pub mod ffi {
    pub struct PreviewFrameCopy {
        pub ok: bool,
        pub width: u32,
        pub height: u32,
        pub stride: u32,
        pub format: u8,
        pub color_space: u8,
        pub pixels: Vec<u8>,
    }

    // SAFETY: Qt C++ headers and `register_omastudio_image_provider` match our C++ implementation
    // for the lifetime of `QQmlApplicationEngine`.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;

        include!("preview_provider.h");
        fn register_omastudio_image_provider(engine: Pin<&mut QQmlApplicationEngine>);
    }

    extern "Rust" {
        fn fetch_preview_frame(id: &str) -> PreviewFrameCopy;
    }
}

struct ActiveRing {
    generation: u64,
    reader: Arc<PreviewRingReader>,
}

fn ring_slot() -> &'static RwLock<Option<ActiveRing>> {
    static RING: OnceLock<RwLock<Option<ActiveRing>>> = OnceLock::new();
    RING.get_or_init(|| RwLock::new(None))
}

/// Registers the `image://omastudio/...` provider on `engine` before QML loads.
pub fn register(engine: Pin<&mut QQmlApplicationEngine>) {
    ffi::register_omastudio_image_provider(engine);
}

/// Installs a newly negotiated `memfd` ring for a `HostProcess` daemon run.
pub fn install_ring(process_generation: u64, reader: PreviewRingReader) {
    let mut guard = ring_slot().write().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().is_some_and(|cur| cur.generation > process_generation) {
        return;
    }
    *guard = Some(ActiveRing {
        generation: process_generation,
        reader: Arc::new(reader),
    });
}

/// Clears the installed ring if it still belongs to `process_generation`.
pub fn clear_ring(process_generation: u64) {
    let mut guard = ring_slot().write().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().is_some_and(|cur| cur.generation == process_generation) {
        *guard = None;
    }
}

fn empty_copy() -> ffi::PreviewFrameCopy {
    ffi::PreviewFrameCopy {
        ok: false,
        width: 0,
        height: 0,
        stride: 0,
        format: 0,
        color_space: 0,
        pixels: Vec::new(),
    }
}

/// Called from Qt's image-provider worker threads to fetch a tear-free frame copy from `memfd`.
fn fetch_preview_frame(id: &str) -> ffi::PreviewFrameCopy {
    let Some((kind, slot, _gen)) = parse_provider_id(id) else {
        return empty_copy();
    };
    let reader = {
        let guard = ring_slot().read().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|r| Arc::clone(&r.reader))
    };
    let Some(reader) = reader else {
        return empty_copy();
    };
    let Some(frame) = reader.read_frame(kind, slot) else {
        return empty_copy();
    };
    ffi::PreviewFrameCopy {
        ok: true,
        width: frame.width,
        height: frame.height,
        stride: frame.stride,
        format: frame.format as u8,
        color_space: frame.color_space as u8,
        pixels: frame.pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use crate::preview_shm::{
        ColorSpaceTag, PixelFormat, PreviewRingWriter, SlotKind, create_preview_socketpair,
    };

    #[test]
    fn provider_bridge_fetches_installed_memfd_frame_and_clears_cleanly() {
        let (parent, child) = create_preview_socketpair().expect("socketpair");
        let t = std::thread::spawn(move || {
            PreviewRingReader::recv_and_ack(parent.as_raw_fd())
                .expect("recv_and_ack")
                .expect("ring")
        });
        let mut writer = PreviewRingWriter::create().expect("writer");
        writer.send_and_wait_ack(child.as_raw_fd(), 5_000).expect("ack");
        let reader = t.join().expect("join");

        let rgb = vec![12u8, 34, 56, 78, 90, 123];
        let published = writer
            .write_rgb8_as_rgba8(SlotKind::Preview, &rgb, 2, 1, ColorSpaceTag::Srgb)
            .expect("write");
        let id = published
            .url
            .strip_prefix("image://omastudio/")
            .expect("image:// prefix");

        install_ring(999, reader);
        let fetched = fetch_preview_frame(id);
        assert!(fetched.ok);
        assert_eq!(fetched.width, 2);
        assert_eq!(fetched.height, 1);
        assert_eq!(fetched.stride, 8);
        assert_eq!(fetched.format, PixelFormat::Rgba8888 as u8);
        assert_eq!(fetched.color_space, ColorSpaceTag::Srgb as u8);
        assert_eq!(fetched.pixels, vec![12, 34, 56, 255, 78, 90, 123, 255]);

        // Clearing an unrelated generation must leave the active ring intact
        clear_ring(123);
        assert!(fetch_preview_frame(id).ok);

        clear_ring(999);
        assert!(!fetch_preview_frame(id).ok);
    }
}
