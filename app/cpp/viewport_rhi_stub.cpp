#include "viewport_rhi_stub.h"

OmaRhiViewport::OmaRhiViewport(QQuickItem *parent)
    : QQuickItem(parent) {}

void OmaRhiViewport::presentFrame(qint64 generation, int width, int height, int colorSpace) {
    Q_UNUSED(generation);
    Q_UNUSED(width);
    Q_UNUSED(height);
    Q_UNUSED(colorSpace);
    emit dmaBufFailed();
}

void OmaRhiViewport::clearFrame() {}
