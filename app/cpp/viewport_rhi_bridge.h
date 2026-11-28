#pragma once

#include <QtCore/QString>
#include <QtQml/QQmlApplicationEngine>

/// Requests Qt Quick's Vulkan RHI backend before `QGuiApplication` is constructed,
/// unless overridden by `QSG_RHI_BACKEND` or `QT_QUICK_BACKEND=software`.
void configure_vulkan_graphics_api(bool allow_vulkan);

/// Configures Qt's default `QSurfaceFormat` (10-bit wide-gamut or 16-bit floating-point
/// HDR buffer + matching `QColorSpace`) prior to `QGuiApplication` creation (`WP-16`).
void configure_surface_format(bool prefer_hdr_float, bool prefer_10bit, int color_space_code);

/// Applies per-window surface format and Vulkan external-memory / DMA-BUF / SYNC_FD
/// device extensions via `QQuickGraphicsConfiguration::setDeviceExtensions` on the root
/// `QQuickWindow` after `Main.qml` (created with `visible: false`) is loaded, then calls `show()`.
void configure_window_and_show(
    QQmlApplicationEngine &engine,
    bool enable_dmabuf_extensions,
    bool prefer_hdr_float,
    bool prefer_10bit,
    int color_space_code
);

/// Reads the system clipboard (Wayland or X11 via Qt) if its UTF-8 byte length does not
/// exceed `max_bytes`. Sets `exceeded` to true and returns an empty string when larger.
QString clipboard_text_bounded(int max_bytes, bool &exceeded);
