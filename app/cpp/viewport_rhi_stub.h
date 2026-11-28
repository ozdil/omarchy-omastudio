#pragma once

#include <QtQuick/QQuickItem>
#include <QtQml/qqmlregistration.h>

/// Fallback QML stub for Qt < 6.7 (e.g. Ubuntu 24.04 Qt 6.4): keeps the exact QML surface
/// while reporting `available == false` so `Viewport.qml` stays on Stage 4A `memfd`.
class OmaRhiViewport : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool available READ available CONSTANT)
    Q_PROPERTY(bool dmaBufActive READ dmaBufActive NOTIFY dmaBufActiveChanged)
    Q_PROPERTY(int frameWidth READ frameWidth NOTIFY frameMetricsChanged)
    Q_PROPERTY(int frameHeight READ frameHeight NOTIFY frameMetricsChanged)
    Q_PROPERTY(int colorSpace READ colorSpace NOTIFY frameMetricsChanged)

public:
    explicit OmaRhiViewport(QQuickItem *parent = nullptr);
    ~OmaRhiViewport() override = default;

    bool available() const noexcept { return false; }
    bool dmaBufActive() const noexcept { return false; }
    int frameWidth() const noexcept { return 0; }
    int frameHeight() const noexcept { return 0; }
    int colorSpace() const noexcept { return 0; }

    Q_INVOKABLE void presentFrame(qint64 generation, int width, int height, int colorSpace);
    Q_INVOKABLE void clearFrame();

signals:
    void dmaBufActiveChanged();
    void frameMetricsChanged();
    void dmaBufFailed();
};
