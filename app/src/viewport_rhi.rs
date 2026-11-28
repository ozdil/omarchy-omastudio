//! Rust <-> Qt 6 Vulkan RHI (`QQuickRhiItem`) bridge and DMA-BUF / `SYNC_FD` inbox (`WP-15`).

use std::os::fd::IntoRawFd;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};

use cxx_qt_lib::QQmlApplicationEngine;

use crate::preview_shm::{ReceivedDmaBufFrame, dmabuf_enabled_by_env};

#[cxx_qt::bridge]
pub mod ffi {
    pub struct PendingDmaBufFrame {
        pub ok: bool,
        pub generation: u64,
        pub width: u32,
        pub height: u32,
        pub stride: u32,
        pub offset: u32,
        pub vk_format: u32,
        pub drm_format: u32,
        pub drm_modifier: u64,
        pub alloc_size: u64,
        pub color_space: u8,
        pub dmabuf_fd: i32,
        pub sync_fd: i32,
        pub device_uuid: Vec<u8>,
    }

    // SAFETY: Qt C++ headers and helper functions in `viewport_rhi_bridge.h` match our C++
    // implementation for the lifetime of the Qt application and `QQmlApplicationEngine`.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;

        include!("viewport_rhi_bridge.h");
        fn configure_vulkan_graphics_api(allow_vulkan: bool);
        fn configure_surface_format(prefer_hdr_float: bool, prefer_10bit: bool, color_space_code: i32);
        fn configure_window_and_show(
            engine: Pin<&mut QQmlApplicationEngine>,
            enable_dmabuf_extensions: bool,
            prefer_hdr_float: bool,
            prefer_10bit: bool,
            color_space_code: i32,
        );
    }

    extern "Rust" {
        fn take_pending_dmabuf_frame() -> PendingDmaBufFrame;
    }
}

struct ParkedFrame {
    process_generation: u64,
    frame: ReceivedDmaBufFrame,
}

fn inbox() -> &'static Mutex<Option<ParkedFrame>> {
    static INBOX: OnceLock<Mutex<Option<ParkedFrame>>> = OnceLock::new();
    INBOX.get_or_init(|| Mutex::new(None))
}

/// Configures Qt Quick to use the Vulkan RHI backend and 10-bit/FP16 `QSurfaceFormat`
/// prior to `QGuiApplication::new()` unless overridden by `QSG_RHI_BACKEND` or software mode.
pub fn configure_graphics_api() {
    let explicit_backend = std::env::var("QSG_RHI_BACKEND")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    let software = std::env::var("QT_QUICK_BACKEND")
        .ok()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("software"));
    ffi::configure_vulkan_graphics_api(!explicit_backend && !software);
    if !software {
        let info = crate::wayland_color::detected();
        ffi::configure_surface_format(info.prefer_hdr_float, info.prefer_10bit, info.color_space_code);
    }
}

/// Applies per-window surface format and Vulkan DMA-BUF/SYNC_FD extensions on the root
/// `QQuickWindow` before showing the window (`Main.qml` starts with `visible: false`).
pub fn configure_window_and_show(engine: Pin<&mut QQmlApplicationEngine>) {
    let info = crate::wayland_color::detected();
    ffi::configure_window_and_show(
        engine,
        dmabuf_enabled_by_env(),
        info.prefer_hdr_float,
        info.prefer_10bit,
        info.color_space_code,
    );
}

/// Parks a newly received DMA-BUF + `SYNC_FD` frame for `OmaRhiViewportRenderer` to consume,
/// cleanly dropping (closing) any superseded unconsumed frame's file descriptors.
pub fn park_frame(process_generation: u64, frame: ReceivedDmaBufFrame) {
    let mut guard = inbox().lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().is_some_and(|cur| cur.process_generation > process_generation) {
        return;
    }
    *guard = Some(ParkedFrame { process_generation, frame });
}

/// Drops any parked DMA-BUF descriptors belonging to `process_generation` when the daemon exits.
pub fn clear_frames(process_generation: u64) {
    let mut guard = inbox().lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().is_some_and(|cur| cur.process_generation == process_generation) {
        *guard = None;
    }
}

fn empty_pending() -> ffi::PendingDmaBufFrame {
    ffi::PendingDmaBufFrame {
        ok: false,
        generation: 0,
        width: 0,
        height: 0,
        stride: 0,
        offset: 0,
        vk_format: 0,
        drm_format: 0,
        drm_modifier: 0,
        alloc_size: 0,
        color_space: 0,
        dmabuf_fd: -1,
        sync_fd: -1,
        device_uuid: Vec::new(),
    }
}

/// Transfers ownership of the latest parked DMA-BUF + `SYNC_FD` file descriptors into C++
/// (`OmaRhiViewportRenderer::render`).
#[allow(dead_code)]
fn take_pending_dmabuf_frame() -> ffi::PendingDmaBufFrame {
    let taken = inbox().lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(ParkedFrame { frame, .. }) = taken else {
        return empty_pending();
    };
    let d = frame.desc;
    ffi::PendingDmaBufFrame {
        ok: true,
        generation: d.generation,
        width: d.width,
        height: d.height,
        stride: d.stride,
        offset: d.offset,
        vk_format: d.vk_format,
        drm_format: d.drm_format,
        drm_modifier: d.drm_modifier,
        alloc_size: d.alloc_size,
        color_space: d.color_space as u8,
        dmabuf_fd: frame.dmabuf_fd.into_raw_fd(),
        sync_fd: frame.sync_fd.map_or(-1, IntoRawFd::into_raw_fd),
        device_uuid: d.device_uuid.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::{FromRawFd, OwnedFd};
    use crate::preview_shm::{
        ColorSpaceTag, DRM_FORMAT_ABGR16161616F, DmaBufFrameDesc, VK_FORMAT_R16G16B16A16_SFLOAT,
    };

    #[test]
    fn dmabuf_inbox_parks_supersedes_and_transfers_fds_cleanly() {
        // SAFETY: standalone `eventfd` descriptors used strictly to verify FD handoff/drop lifecycle.
        let fd1 = unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC)) };
        // SAFETY: standalone `eventfd` descriptor.
        let fd2 = unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC)) };
        // SAFETY: standalone `eventfd` descriptor.
        let sync2 = unsafe { OwnedFd::from_raw_fd(libc::eventfd(1, libc::EFD_CLOEXEC)) };

        let desc1 = DmaBufFrameDesc {
            generation: 1,
            width: 640,
            height: 480,
            stride: 640 * 8,
            offset: 0,
            vk_format: VK_FORMAT_R16G16B16A16_SFLOAT,
            drm_format: DRM_FORMAT_ABGR16161616F,
            drm_modifier: 0,
            alloc_size: 640 * 480 * 8,
            color_space: ColorSpaceTag::Srgb,
            has_sync_fd: false,
            device_uuid: [0x22; 16],
        };
        let desc2 = DmaBufFrameDesc { generation: 2, has_sync_fd: true, ..desc1 };

        park_frame(77, ReceivedDmaBufFrame { desc: desc1, dmabuf_fd: fd1, sync_fd: None });
        park_frame(77, ReceivedDmaBufFrame { desc: desc2, dmabuf_fd: fd2, sync_fd: Some(sync2) });

        let taken = take_pending_dmabuf_frame();
        assert!(taken.ok);
        assert_eq!(taken.generation, 2);
        assert!(taken.dmabuf_fd >= 0);
        assert!(taken.sync_fd >= 0);
        assert_eq!(taken.device_uuid, vec![0x22; 16]);
        // Close the raw FDs returned to caller
        // SAFETY: `take_pending_dmabuf_frame` transferred ownership of both valid FDs.
        unsafe {
            drop(OwnedFd::from_raw_fd(taken.dmabuf_fd));
            drop(OwnedFd::from_raw_fd(taken.sync_fd));
        }
        assert!(!take_pending_dmabuf_frame().ok);
    }
}
