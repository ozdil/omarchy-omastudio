#include "viewport_rhi_bridge.h"

#include <QtCore/QByteArray>
#include <QtCore/QByteArrayList>
#include <QtCore/QObject>
#include <QtCore/QPointF>
#include <QtCore/QtGlobal>
#include <QtGui/QColorSpace>
#include <QtGui/QSurfaceFormat>
#include <QtQuick/QQuickGraphicsConfiguration>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGRendererInterface>

namespace {

QColorSpace surface_color_space(int code) {
    switch (code) {
    case 1:
        return QColorSpace(QColorSpace::DisplayP3);
    case 2:
#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
        return QColorSpace(QColorSpace::Bt2020);
#else
        return QColorSpace(
            QPointF(0.3127, 0.3290),
            QPointF(0.7080, 0.2920),
            QPointF(0.1700, 0.7970),
            QPointF(0.1310, 0.0460),
            QColorSpace::TransferFunction::Gamma,
            2.4f
        );
#endif
    case 3:
#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
        return QColorSpace(QColorSpace::Bt2100Pq);
#else
        return QColorSpace(QColorSpace::DisplayP3);
#endif
    case 4:
#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
        return QColorSpace(QColorSpace::Bt2100Hlg);
#else
        return QColorSpace(QColorSpace::DisplayP3);
#endif
    default:
        return QColorSpace(QColorSpace::SRgb);
    }
}

void apply_color_format(QSurfaceFormat &fmt, bool prefer_hdr_float, bool prefer_10bit, int color_space_code) {
    if (prefer_hdr_float) {
        fmt.setRedBufferSize(16);
        fmt.setGreenBufferSize(16);
        fmt.setBlueBufferSize(16);
        fmt.setAlphaBufferSize(16);
    } else if (prefer_10bit) {
        fmt.setRedBufferSize(10);
        fmt.setGreenBufferSize(10);
        fmt.setBlueBufferSize(10);
        fmt.setAlphaBufferSize(2);
    }
    if (color_space_code != 0 || prefer_hdr_float || prefer_10bit) {
        fmt.setColorSpace(surface_color_space(color_space_code));
    }
}

} // namespace

void configure_vulkan_graphics_api(bool allow_vulkan) {
    if (!allow_vulkan) {
        return;
    }
    QQuickWindow::setGraphicsApi(QSGRendererInterface::Vulkan);
}

void configure_surface_format(bool prefer_hdr_float, bool prefer_10bit, int color_space_code) {
    if (!prefer_hdr_float && !prefer_10bit && color_space_code == 0) {
        return;
    }
    QSurfaceFormat fmt = QSurfaceFormat::defaultFormat();
    apply_color_format(fmt, prefer_hdr_float, prefer_10bit, color_space_code);
    QSurfaceFormat::setDefaultFormat(fmt);
}

void configure_window_and_show(
    QQmlApplicationEngine &engine,
    bool enable_dmabuf_extensions,
    bool prefer_hdr_float,
    bool prefer_10bit,
    int color_space_code
) {
    const auto roots = engine.rootObjects();
    for (QObject *obj : roots) {
        auto *window = qobject_cast<QQuickWindow *>(obj);
        if (!window) {
            continue;
        }
        if (prefer_hdr_float || prefer_10bit || color_space_code != 0) {
            QSurfaceFormat fmt = window->format();
            apply_color_format(fmt, prefer_hdr_float, prefer_10bit, color_space_code);
            window->setFormat(fmt);
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
