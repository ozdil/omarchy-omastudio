#include "viewport_rhi_bridge.h"

#include <QtCore/QByteArray>
#include <QtCore/QByteArrayList>
#include <QtCore/QObject>
#include <QtQuick/QQuickGraphicsConfiguration>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGRendererInterface>

void configure_vulkan_graphics_api(bool allow_vulkan) {
    if (!allow_vulkan) {
        return;
    }
    QQuickWindow::setGraphicsApi(QSGRendererInterface::Vulkan);
}

void configure_window_and_show(QQmlApplicationEngine &engine, bool enable_dmabuf_extensions) {
    const auto roots = engine.rootObjects();
    for (QObject *obj : roots) {
        auto *window = qobject_cast<QQuickWindow *>(obj);
        if (!window) {
            continue;
        }
        if (enable_dmabuf_extensions
            && QQuickWindow::graphicsApi() == QSGRendererInterface::Vulkan) {
            QQuickGraphicsConfiguration cfg = window->graphicsConfiguration();
            QByteArrayList exts = cfg.deviceExtensions();
            const QByteArrayList needed = {
                QByteArrayLiteral("VK_KHR_external_memory"),
                QByteArrayLiteral("VK_KHR_external_memory_fd"),
                QByteArrayLiteral("VK_EXT_external_memory_dma_buf"),
                QByteArrayLiteral("VK_EXT_image_drm_format_modifier"),
                QByteArrayLiteral("VK_KHR_external_semaphore_fd"),
            };
            for (const QByteArray &ext : needed) {
                if (!exts.contains(ext)) {
                    exts.append(ext);
                }
            }
            cfg.setDeviceExtensions(exts);
            window->setGraphicsConfiguration(cfg);
        }
        window->show();
    }
}
