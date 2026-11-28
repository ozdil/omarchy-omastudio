#pragma once

#include <QtQml/QQmlApplicationEngine>

/// Requests Qt Quick's Vulkan RHI backend before `QGuiApplication` is constructed,
/// unless overridden by `QSG_RHI_BACKEND` or `QT_QUICK_BACKEND=software`.
void configure_vulkan_graphics_api(bool allow_vulkan);

/// Applies per-window Vulkan external-memory / DMA-BUF / SYNC_FD device extensions
/// via `QQuickGraphicsConfiguration::setDeviceExtensions` on the root `QQuickWindow`
/// after `Main.qml` (created with `visible: false`) is loaded, then calls `show()`.
void configure_window_and_show(QQmlApplicationEngine &engine, bool enable_dmabuf_extensions);
