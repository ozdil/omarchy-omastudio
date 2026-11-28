import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

// Left workspace inspector: Navigator, Scopes, EXIF metadata, Scene suggestion,
// and History/Snapshots panel.
Rectangle {
    id: panel

    property var host: null
    property var viewport: null

    readonly property alias navigator: navigator

    visible: panel.host ? panel.host.leftPanelShown : false
    implicitWidth: (panel.host && panel.host.leftPanelShown) ? panel.host.leftPanelWidth : 0
    Layout.minimumWidth: implicitWidth
    Layout.preferredWidth: implicitWidth
    Layout.fillHeight: true
    color: Theme.bgBase
    border.color: Theme.border
    border.width: (panel.host && panel.host.leftPanelShown) ? 1 : 0

    ScrollView {
        anchors.fill: parent
        anchors.margins: 10
        clip: true
        contentWidth: availableWidth

        ColumnLayout {
            width: parent.width
            spacing: 10

            // 1. NAVIGATOR (Miniature Viewport with Draggable Crop Box)
            Navigator {
                id: navigator
                Layout.fillWidth: true
                sourceItem: panel.viewport ? panel.viewport.previewSourceItem : null
                sourceAspect: (panel.viewport && panel.viewport.previewHeight > 0)
                    ? (panel.viewport.previewWidth / panel.viewport.previewHeight)
                    : 1.5
                useSoftwareFallback: Host.softwareRenderer
                zoomFactor: panel.viewport ? panel.viewport.zoomFactor : 1.0
                rotationAngle: panel.viewport ? panel.viewport.rotationAngle : 0.0
                normX: panel.viewport ? panel.viewport.normX : 0.0
                normY: panel.viewport ? panel.viewport.normY : 0.0
                normW: panel.viewport ? panel.viewport.normW : 1.0
                normH: panel.viewport ? panel.viewport.normH : 1.0
                onZoomRequested: function(z) {
                    if (!panel.viewport) return;
                    if (z <= 0.05) panel.viewport.resetZoomFit();
                    else panel.viewport.setZoomAbsolute(z);
                }
                onPanRequested: function(nx, ny) {
                    if (panel.viewport) panel.viewport.setNormCenter(nx, ny);
                }
            }

            // 2. DAVINCI RESOLVE VIDEO SCOPES (Waveform, RGB Parade, Vectorscope, Histogram)
            ScopesView {
                id: scopesView
                Layout.fillWidth: true
                histData: panel.host ? panel.host.activeHistogram : null
                showClippingHighlights: panel.viewport ? panel.viewport.showHighlightMask : false
                showClippingShadows: panel.viewport ? panel.viewport.showShadowMask : false
                onToggleHighlightMask: {
                    if (!panel.viewport || !panel.host) return;
                    panel.viewport.showHighlightMask = !panel.viewport.showHighlightMask;
                    panel.host.requestRender();
                }
                onToggleShadowMask: {
                    if (!panel.viewport || !panel.host) return;
                    panel.viewport.showShadowMask = !panel.viewport.showShadowMask;
                    panel.host.requestRender();
                }
            }

            // 3. EXIF CAMERA METADATA
            ExifCard {
                Layout.fillWidth: true
                metadata: panel.host ? panel.host.activeMetadata : null
            }

            // 4. Scene analysis (after Auto)
            Rectangle {
                // A guess from simple image statistics: shown only when it is fairly sure
                visible: panel.host && panel.host.activeScene !== null && panel.host.activeScene.confidence >= 0.9
                Layout.fillWidth: true
                implicitHeight: 70
                radius: Theme.radiusMd
                color: Theme.bgDark
                border.color: Theme.border
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 2

                    RowLayout {
                        Text {
                            text: Theme.iconAi
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(11)
                            color: Theme.accentPurple
                        }
                        Text {
                            text: qsTr("Suggested scene")
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(9)
                            font.weight: Font.Bold
                            color: Theme.accentPurple
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }
                        Text {
                            text: (panel.host && panel.host.activeScene) ? Math.round(panel.host.activeScene.confidence * 100) + "%" : ""
                            textFormat: Text.PlainText
                            font.family: Theme.monoFont
                            font.pixelSize: Theme.px(9)
                            color: Theme.textDim
                        }
                    }

                    Text {
                        text: (panel.host && panel.host.activeScene) ? I18n.sceneType(panel.host.activeScene.scene_type) : qsTr("Analyzing...")
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(11)
                        font.weight: Font.Bold
                        color: Theme.textMain
                    }

                    Text {
                        text: (panel.host && panel.host.activeScene) ? I18n.sceneDescription(panel.host.activeScene.scene_type, panel.host.activeScene.description) : qsTr("Extracting dynamic range...")
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(9)
                        color: Theme.textMuted
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }
            }

            // 5. History and snapshots
            HistoryPanel {
                Layout.fillWidth: true
                visible: panel.host ? panel.host.activePhotoPath !== "" : false
                entries: panel.host ? panel.host.historyModel : []
                currentIndex: (panel.host && panel.host.historyCtrl) ? panel.host.historyCtrl.undoCount : 0
                snapshots: panel.host ? panel.host.snapshots : []
                onJumpRequested: function(index) { if (panel.host) panel.host.jumpToHistory(index) }
                onSnapshotRequested: if (panel.host) panel.host.takeSnapshot()
                onRestoreRequested: function(index) { if (panel.host) panel.host.restoreSnapshot(index) }
                onRenameRequested: function(index, name) { if (panel.host) panel.host.renameSnapshot(index, name) }
                onDeleteRequested: function(index) { if (panel.host) panel.host.deleteSnapshot(index) }
            }

            Item { Layout.fillHeight: true }
        }
    }
}
