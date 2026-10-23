import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

Rectangle {
    id: root
    implicitHeight: 46
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property bool isProMode: false
    property bool isSplitView: false
    property bool isCropMode: false
    property real currentZoom: 1.0
    property string activePhotoName: ""
    property int photoRating: 0
    property string photoFlag: "none"
    // Local AI connection, shown next to the mode switcher
    property bool aiConnected: false
    property bool aiBusy: false
    property string aiModel: ""

    signal ratingChanged(int rating)
    signal flagChanged(string flag)

    signal toggleMode()
    signal connectLocalAi()
    signal openFileClicked()
    signal toggleSplit()
    signal toggleCrop()
    signal zoomFit()
    signal zoom100()
    signal zoom200()
    signal undoClicked()
    signal redoClicked()
    signal exportClicked()
    signal infoClicked()


    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 12

        // App Emblem & Title (Theme-reactive)
        RowLayout {
            spacing: 8
            Rectangle {
                width: 26
                height: 26
                radius: Theme.r(6)
                color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
                border.color: Theme.accent
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconCamera
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(13)
                    color: Theme.accent
                }
            }
            Text {
                font.family: Theme.fontFamily
                text: "OmaStudio"
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(14)
                font.weight: Font.Black
                color: Theme.textMain
            }
        }

        // Active filename badge
        Rectangle {
            visible: root.activePhotoName !== ""
            // Cap the badge so long camera filenames don't push the toolbar off-screen
            implicitWidth: Math.min(photoNameText.implicitWidth + 14, 280)
            implicitHeight: 22
            radius: Theme.r(4)
            color: Theme.bgCard
            border.color: Theme.border
            Text {
                id: photoNameText
                anchors.fill: parent
                anchors.leftMargin: 7
                anchors.rightMargin: 7
                elide: Text.ElideMiddle
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                text: root.activePhotoName
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.family: Theme.monoFont
                color: Theme.textMain
            }
        }

        // Lightroom Studio Culling & Rating Widget (P / X Flags & 1-5 Stars)
        RowLayout {
            visible: root.activePhotoName !== ""
            spacing: 3

            // Pick Flag (P)
            Rectangle {
                implicitWidth: 20
                implicitHeight: 20
                radius: Theme.r(3)
                color: root.photoFlag === "pick" ? Theme.accentGreen : Theme.bgCard
                border.color: root.photoFlag === "pick" ? Theme.accentGreen : Theme.border
                Text {
                    anchors.centerIn: parent
                    text: qsTr("P", "pick flag, first letter of Pick")
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: root.photoFlag === "pick" ? Theme.bgBase : Theme.accentGreen
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.flagChanged(root.photoFlag === "pick" ? "none" : "pick")
                }
            }

            // Reject Flag (X)
            Rectangle {
                implicitWidth: 20
                implicitHeight: 20
                radius: Theme.r(3)
                color: root.photoFlag === "reject" ? Theme.highlightClip : Theme.bgCard
                border.color: root.photoFlag === "reject" ? Theme.highlightClip : Theme.border
                Text {
                    anchors.centerIn: parent
                    text: "X"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: root.photoFlag === "reject" ? "#ffffff" : Theme.highlightClip
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.flagChanged(root.photoFlag === "reject" ? "none" : "reject")
                }
            }

            // Star Rating (1..5)
            RowLayout {
                spacing: 2
                Repeater {
                    model: 5
                    Rectangle {
                        implicitWidth: 16
                        implicitHeight: 20
                        radius: Theme.r(2)
                        color: "transparent"
                        Text {
                            anchors.centerIn: parent
                            text: (index + 1) <= root.photoRating ? "*" : "-"
                            textFormat: Text.PlainText
                            font.family: Theme.monoFont
                            font.pixelSize: Theme.px(11)
                            font.weight: Font.Bold
                            color: (index + 1) <= root.photoRating ? Theme.accentYellow : Theme.textDim
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                var newR = (root.photoRating === (index + 1)) ? 0 : (index + 1);
                                root.ratingChanged(newR);
                            }
                        }
                    }
                }
            }
        }

        Item { Layout.fillWidth: true }

        // Local AI: Omarchy's default agent, else Ollama or LM Studio on this machine
        Rectangle {
            implicitWidth: aiRow.implicitWidth + 16
            implicitHeight: 28
            radius: Theme.radiusSm
            color: aiMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
            border.color: root.aiConnected ? Theme.accentGreen : Theme.border

            RowLayout {
                id: aiRow
                anchors.centerIn: parent
                spacing: 6
                Text {
                    text: Theme.iconAi
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: root.aiConnected ? Theme.accentGreen : Theme.textMuted
                }
                Text {
                    font.family: Theme.fontFamily
                    text: root.aiBusy ? qsTr("Connecting…")
                        : root.aiConnected ? qsTr("Local AI") : qsTr("Connect local AI")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.DemiBold
                    color: root.aiConnected ? Theme.textMain : Theme.textMuted
                }
            }

            MouseArea {
                id: aiMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                enabled: !root.aiBusy
                onClicked: root.connectLocalAi()
                ToolTip.visible: containsMouse
                ToolTip.delay: 500
                ToolTip.text: root.aiConnected ? qsTr("Connected: %1. Click to reconnect.").arg(root.aiModel)
                                               : qsTr("Use Omarchy's default AI agent, or Ollama or LM Studio on this computer")
                Accessible.role: Accessible.Button
                Accessible.name: qsTr("Connect local AI")
            }
        }

        // Mode Switcher: Essentials vs Pro Studio
        Rectangle {
            implicitWidth: Theme.px(170)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            RowLayout {
                anchors.fill: parent
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    radius: Theme.radiusSm
                    color: !root.isProMode ? Theme.accent : "transparent"

                    RowLayout {
                        anchors.centerIn: parent
                        spacing: 6
                        Text {
                            text: Theme.iconImage
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(10)
                            color: !root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                        Text {
                            font.family: Theme.fontFamily
                            text: qsTr("Essentials")
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            font.weight: Font.DemiBold
                            color: !root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: if (root.isProMode) root.toggleMode()
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    radius: Theme.radiusSm
                    color: root.isProMode ? Theme.accent : "transparent"

                    RowLayout {
                        anchors.centerIn: parent
                        spacing: 6
                        Text {
                            text: Theme.iconSliders
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(10)
                            color: root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                        Text {
                            font.family: Theme.fontFamily
                            text: qsTr("Pro")
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            font.weight: Font.DemiBold
                            color: root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: if (!root.isProMode) root.toggleMode()
                    }
                }
            }
        }

        // Undo / Redo Quick Actions
        RowLayout {
            spacing: 2

            Rectangle {
                implicitWidth: 28
                implicitHeight: 28
                radius: Theme.r(4)
                color: undoMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconUndo
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: undoMouse.containsMouse ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    id: undoMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Undo (Ctrl+Z)")
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Undo")
                    onClicked: root.undoClicked()
                }
            }

            Rectangle {
                implicitWidth: 28
                implicitHeight: 28
                radius: Theme.r(4)
                color: redoMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconRedo
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: redoMouse.containsMouse ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    id: redoMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Redo (Ctrl+Shift+Z)")
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Redo")
                    onClicked: root.redoClicked()
                }
            }
        }

        // Before / After Split Button (A | B)
        Rectangle {
            implicitWidth: Math.max(70, label374.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: root.isSplitView ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
            border.color: root.isSplitView ? Theme.accent : Theme.border

            RowLayout {
                id: label374
                anchors.centerIn: parent
                spacing: 5
                Text {
                    text: Theme.iconSplit
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: root.isSplitView ? Theme.accent : Theme.textMuted
                }
                Text {
                    font.family: Theme.fontFamily
                    text: "A | B"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.DemiBold
                    color: root.isSplitView ? Theme.accent : Theme.textMain
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.toggleSplit()
            }
        }

        // Crop Mode Toggle Button (C)
        Rectangle {
            implicitWidth: Math.max(68, label409.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: root.isCropMode ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
            border.color: root.isCropMode ? Theme.accent : Theme.border

            RowLayout {
                id: label409
                anchors.centerIn: parent
                spacing: 5
                Text {
                    text: Theme.iconCrop
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: root.isCropMode ? Theme.accent : Theme.textMuted
                }
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Crop")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.DemiBold
                    color: root.isCropMode ? Theme.accent : Theme.textMain
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.toggleCrop()
            }
        }

        // Zoom Controls
        RowLayout {
            spacing: 2
            Rectangle {
                implicitWidth: Math.max(28, fitLabel.implicitWidth + 12)
                implicitHeight: 28
                radius: Theme.r(4)
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    id: fitLabel
                    font.family: Theme.fontFamily
                    anchors.centerIn: parent
                    text: qsTr("Fit")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    color: Theme.textMain
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.zoomFit()
                }
            }
            Rectangle {
                width: 32
                height: 28
                radius: Theme.r(4)
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    font.family: Theme.fontFamily
                    anchors.centerIn: parent
                    text: "1:1"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    color: Theme.textMain
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.zoom100()
                }
            }
            Rectangle {
                width: 32
                height: 28
                radius: Theme.r(4)
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    font.family: Theme.fontFamily
                    anchors.centerIn: parent
                    text: "2:1"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    color: Theme.textMain
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.zoom200()
                }
            }
        }

        Item { Layout.fillWidth: true }

        LanguageMenu {}

        // Omarchy Studio Info & About Button
        Rectangle {
            implicitWidth: Math.max(32, label513.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: mouseInfo.containsMouse ? Theme.bgCardHover : Theme.bgCard
            border.color: mouseInfo.containsMouse ? Theme.accent : Theme.border
            border.width: 1

            Text {
                id: label513
                anchors.centerIn: parent
                text: Theme.iconInfo
                font.family: Theme.iconFont
                font.pixelSize: Theme.px(13)
                color: mouseInfo.containsMouse ? Theme.accent : Theme.textDim
            }

            MouseArea {
                id: mouseInfo
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.infoClicked()
            }
        }


        // Open Photo Button
        Rectangle {
            implicitWidth: Math.max(92, label541.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            RowLayout {
                id: label541
                anchors.centerIn: parent
                spacing: 6
                Text {
                    text: Theme.iconFolder
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: Theme.textMain
                }
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Open RAW")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(11)
                    font.weight: Font.Medium
                    color: Theme.textMain
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.openFileClicked()
            }
        }

        // Export Button (Bright CTA)
        Rectangle {
            implicitWidth: Math.max(88, label576.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.accent

            RowLayout {
                id: label576
                anchors.centerIn: parent
                spacing: 6
                Text {
                    text: Theme.iconExport
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: Theme.bgBase
                }
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Export")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(11)
                    font.weight: Font.Bold
                    color: Theme.bgBase
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.exportClicked()
            }
        }
    }
}
