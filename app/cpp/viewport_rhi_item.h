#pragma once

#include <QtQuick/QQuickRhiItem>
#include <QtQml/qqmlregistration.h>

class OmaRhiViewport : public QQuickRhiItem {
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

    QQuickRhiItemRenderer *createRenderer() override;

    bool available() const noexcept { return true; }
    bool dmaBufActive() const noexcept { return m_dmaBufActive; }
    int frameWidth() const noexcept { return m_frameWidth; }
    int frameHeight() const noexcept { return m_frameHeight; }
    int colorSpace() const noexcept { return m_colorSpace; }

    Q_INVOKABLE void presentFrame(qint64 generation, int width, int height, int colorSpace);
    Q_INVOKABLE void clearFrame();

    void reportImportResult(bool ok, int width, int height, int colorSpace);
    qint64 requestedGeneration() const noexcept { return m_requestedGen; }

signals:
    void dmaBufActiveChanged();
    void frameMetricsChanged();
    void dmaBufFailed();

private:
    bool m_dmaBufActive = false;
    qint64 m_requestedGen = 0;
    int m_frameWidth = 0;
    int m_frameHeight = 0;
    int m_colorSpace = 0;
};
