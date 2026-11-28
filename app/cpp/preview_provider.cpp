#include "preview_provider.h"
#include "omastudio-app/src/preview_provider.cxx.h"

#include <QtCore/QByteArray>
#include <QtCore/QPointF>
#include <QtCore/QSize>
#include <QtCore/QString>
#include <QtCore/QtGlobal>
#include <QtGui/QColorSpace>
#include <QtGui/QImage>
#include <QtQuick/QQuickImageProvider>

#include <algorithm>
#include <cstring>

namespace {

QImage::Format to_qt_format(std::uint8_t fmt) noexcept {
    switch (fmt) {
    case 0:
        return QImage::Format_RGB888;
    case 1:
        return QImage::Format_RGBA8888;
    case 2:
        return QImage::Format_RGBA64;
    case 3:
        return QImage::Format_RGBA16FPx4;
    default:
        return QImage::Format_Invalid;
    }
}

QColorSpace to_qt_color_space(std::uint8_t cs) {
    switch (cs) {
    case 0:
        return QColorSpace(QColorSpace::SRgb);
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

class OmaPreviewImageProvider final : public QQuickImageProvider {
public:
    OmaPreviewImageProvider()
        : QQuickImageProvider(QQuickImageProvider::Image) {}

    QImage requestImage(const QString &id, QSize *size, const QSize &requestedSize) override {
        const QByteArray utf8 = id.toUtf8();
        const PreviewFrameCopy frame =
            fetch_preview_frame(rust::Str(utf8.constData(), static_cast<std::size_t>(utf8.size())));
        if (!frame.ok || frame.width == 0 || frame.height == 0 || frame.stride == 0
            || frame.width > 16384 || frame.height > 16384) {
            if (size) {
                *size = QSize();
            }
            return QImage();
        }

        const QImage::Format fmt = to_qt_format(frame.format);
        if (fmt == QImage::Format_Invalid) {
            if (size) {
                *size = QSize();
            }
            return QImage();
        }

        const std::size_t stride = static_cast<std::size_t>(frame.stride);
        const std::size_t height = static_cast<std::size_t>(frame.height);
        if (stride > (128ULL << 20) / height) {
            if (size) {
                *size = QSize();
            }
            return QImage();
        }
        const std::size_t expected = stride * height;
        if (frame.pixels.size() < expected) {
            if (size) {
                *size = QSize();
            }
            return QImage();
        }

        QImage img(static_cast<int>(frame.width), static_cast<int>(frame.height), fmt);
        if (img.isNull() || img.bytesPerLine() <= 0) {
            if (size) {
                *size = QSize();
            }
            return QImage();
        }

        const std::size_t copy_bytes = std::min(stride, static_cast<std::size_t>(img.bytesPerLine()));
        const std::uint8_t *src = frame.pixels.data();
        for (std::uint32_t y = 0; y < frame.height; ++y) {
            std::memcpy(
                img.scanLine(static_cast<int>(y)),
                src + static_cast<std::size_t>(y) * stride,
                copy_bytes
            );
        }

        img.setColorSpace(to_qt_color_space(frame.color_space));

        if (size) {
            *size = img.size();
        }
        if (requestedSize.isValid() && (requestedSize.width() > 0 || requestedSize.height() > 0)) {
            const int tw = requestedSize.width() > 0 ? requestedSize.width() : img.width();
            const int th = requestedSize.height() > 0 ? requestedSize.height() : img.height();
            if (tw < img.width() || th < img.height()) {
                return img.scaled(tw, th, Qt::KeepAspectRatio, Qt::SmoothTransformation);
            }
        }
        return img;
    }
};

} // namespace

void register_omastudio_image_provider(QQmlApplicationEngine &engine) {
    // QQmlEngine takes ownership of the QQuickImageProvider instance.
    engine.addImageProvider(QStringLiteral("omastudio"), new OmaPreviewImageProvider());
}
