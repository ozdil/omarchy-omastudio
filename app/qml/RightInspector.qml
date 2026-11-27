import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// Right adjustment inspector: accessible tab bar, persistent Copy/Paste/Reset/XMP
// actions, and scrollable tab host (Adjust, Masks, Looks, AI).
Rectangle {
    id: inspector

    property var host: null
    property var filmstrip: null

    readonly property alias adjustScroll: adjustScroll
    readonly property alias fujiPanel: adjustTab.fujiPanel
    readonly property alias curveSection: adjustTab.curveSection
    readonly property alias retouchPanel: masksTab.retouchPanel
    readonly property alias layersPanel: masksTab.layersPanel
    readonly property alias presetSel: looksTab.presetSel
    readonly property alias aiAssist: aiTab.aiAssist

    visible: inspector.host ? inspector.host.showRightPanel : false
    implicitWidth: (inspector.host && inspector.host.showRightPanel) ? inspector.host.rightPanelWidth : 0
    Layout.minimumWidth: implicitWidth
    Layout.preferredWidth: implicitWidth
    Layout.fillHeight: true
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: (inspector.host && inspector.host.showRightPanel) ? 1 : 0

    RowLayout {
        id: rightTabs
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 1
        height: Theme.px(36)
        spacing: 0

        Repeater {
            model: (inspector.host && inspector.host.isProMode)
                ? [{ "id": "adjust", "label": qsTr("Adjust") }, { "id": "masks", "label": qsTr("Masks") },
                   { "id": "looks", "label": qsTr("Looks") }, { "id": "ai", "label": qsTr("AI") }]
                : [{ "id": "adjust", "label": qsTr("Adjust") }, { "id": "looks", "label": qsTr("Looks") },
                   { "id": "ai", "label": qsTr("AI") }]
            delegate: Item {
                required property var modelData
                readonly property bool active: inspector.host ? inspector.host.rightTab === modelData.id : false
                // Tabs share the width by the length of their labels, so a
                // long translation (Arabic, Hebrew) gets the room it needs
                Layout.preferredWidth: tabLabel.implicitWidth + 12
                Layout.fillWidth: true
                Layout.fillHeight: true

                // Shrinks, then elides, rather than running past the tab
                Text {
                    id: tabLabel
                    anchors.centerIn: parent
                    width: parent.width - 6
                    horizontalAlignment: Text.AlignHCenter
                    fontSizeMode: Text.HorizontalFit
                    minimumPixelSize: Math.round(8 * Theme.fontScale)
                    elide: Text.ElideRight
                    text: modelData.label
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(11)
                    font.weight: parent.active ? Font.DemiBold : Font.Medium
                    color: parent.active ? Theme.textMain : (tabMouse.containsMouse ? Theme.textMain : Theme.textMuted)
                }
                Rectangle {
                    anchors.bottom: parent.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: 2
                    color: parent.active ? Theme.accent : "transparent"
                }
                MouseArea {
                    id: tabMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: if (inspector.host) inspector.host.rightTab = modelData.id
                }
                KeyFocus {
                    name: modelData.label
                    role: Accessible.PageTab
                    checkable: true
                    checked: parent.active
                    onActivated: if (inspector.host) inspector.host.rightTab = modelData.id
                }
            }
        }
    }

    Rectangle {
        id: rightTabsLine
        anchors.top: rightTabs.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        height: 1
        color: Theme.border
    }

    // Copy, paste and reset the edit, and XMP sidecars: the same in every tab
    Rectangle {
        id: editActionsLine
        anchors.bottom: editActions.top
        anchors.bottomMargin: 10
        anchors.left: parent.left
        anchors.right: parent.right
        height: 1
        color: Theme.border
    }

    ColumnLayout {
        id: editActions
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 12
        spacing: 6
        enabled: inspector.host ? inspector.host.activePhotoPath !== "" : false
        opacity: enabled ? 1.0 : 0.45

        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: Theme.sp(28)
                radius: Theme.radiusSm
                color: copyBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                RowLayout {
                    anchors.centerIn: parent
                    spacing: 4
                    Text {
                        id: copyBtnText
                        text: qsTr("Copy")
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: Theme.px(10)
                        font.weight: Font.Medium
                        color: Theme.textMain
                    }
                }
                MouseArea {
                    id: copyBtnMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: if (inspector.host) inspector.host.copyRecipe()
                }
                KeyFocus {
                    name: copyBtnText.text
                    onActivated: if (inspector.host) inspector.host.copyRecipe()
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: Theme.sp(28)
                radius: Theme.radiusSm
                color: pasteBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                RowLayout {
                    anchors.centerIn: parent
                    spacing: 4
                    Text {
                        id: pasteBtnText
                        text: qsTr("Paste")
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: Theme.px(10)
                        font.weight: Font.Medium
                        color: Theme.textMain
                    }
                }
                MouseArea {
                    id: pasteBtnMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: if (inspector.host) inspector.host.pasteRecipe()
                }
                KeyFocus {
                    name: pasteBtnText.text
                    onActivated: if (inspector.host) inspector.host.pasteRecipe()
                }
            }

            Rectangle {
                // Wide enough for the longest translation of Reset
                implicitWidth: Math.max(70, resetBtnText.implicitWidth + 16)
                implicitHeight: Theme.sp(28)
                radius: Theme.radiusSm
                color: resetBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                Text {
                    id: resetBtnText
                    anchors.centerIn: parent
                    text: qsTr("Reset")
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.Bold
                    color: Theme.highlightClip
                }
                MouseArea {
                    id: resetBtnMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: if (inspector.host) inspector.host.resetRecipe()
                }
                KeyFocus {
                    name: resetBtnText.text
                    onActivated: if (inspector.host) inspector.host.resetRecipe()
                }
            }
        }

        // Edits exchanged with other editors as XMP sidecars
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Repeater {
                model: [
                    { "label": qsTr("Import XMP"), "tip": qsTr("Read the photo's .xmp sidecar written by another editor") },
                    { "label": qsTr("Write XMP"), "tip": qsTr("Write the edit, rating and label to an .xmp sidecar for other editors") }
                ]
                delegate: Rectangle {
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    implicitHeight: Theme.sp(28)
                    radius: Theme.radiusSm
                    opacity: (inspector.host && inspector.host.activePhotoPath !== "") ? 1.0 : 0.4
                    color: xmpBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                    border.color: Theme.border

                    Text {
                        anchors.centerIn: parent
                        text: modelData.label
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: Theme.px(10)
                        font.weight: Font.Medium
                        color: Theme.textMain
                    }
                    MouseArea {
                        id: xmpBtnMouse
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        hoverEnabled: true
                        onClicked: {
                            if (!inspector.host) return;
                            if (index === 0) inspector.host.importXmp();
                            else inspector.host.writeXmp(false);
                        }
                    }
                    KeyFocus {
                        name: modelData.label
                        description: modelData.tip
                        onActivated: {
                            if (!inspector.host) return;
                            if (index === 0) inspector.host.importXmp();
                            else inspector.host.writeXmp(false);
                        }
                    }
                    ToolTip.visible: xmpBtnMouse.containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: modelData.tip
                }
            }
        }
    }

    ScrollView {
        id: adjustScroll
        anchors.top: rightTabsLine.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: editActionsLine.top
        anchors.margins: 12
        clip: true
        contentWidth: availableWidth

        ColumnLayout {
            width: parent.width
            spacing: 12

            AdjustTab {
                id: adjustTab
                host: inspector.host
                filmstrip: inspector.filmstrip
            }
            MasksTab {
                id: masksTab
                host: inspector.host
            }
            LooksTab {
                id: looksTab
                host: inspector.host
            }
            AiTab {
                id: aiTab
                host: inspector.host
            }
        }
    }
}
