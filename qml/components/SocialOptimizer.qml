import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

ColumnLayout {
    id: root
    spacing: 8
    Layout.fillWidth: true

    property string activePlatform: ""

    signal triggerSocialOptimize(string platformCode)
    signal resetSocial()

    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Text {
            text: "AI SOCIAL MEDIA OPTIMIZER"
            textFormat: Text.PlainText
            font.pixelSize: 10
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.accentCyan
            Layout.fillWidth: true
        }

        // Reset social preset button (always accessible)
        Rectangle {
            visible: true
            implicitWidth: 46
            implicitHeight: 18
            radius: Theme.radiusSm
            color: resetSocialMouse.containsMouse ? Qt.rgba(Theme.accentCyan.r, Theme.accentCyan.g, Theme.accentCyan.b, 0.2) : (root.activePlatform !== "" ? Qt.rgba(Theme.accentCyan.r, Theme.accentCyan.g, Theme.accentCyan.b, 0.1) : "transparent")
            border.color: root.activePlatform !== "" ? Theme.accentCyan : Theme.border
            border.width: 1

            Text {
                anchors.centerIn: parent
                text: "RESET"
                textFormat: Text.PlainText
                font.pixelSize: 8
                font.weight: Font.DemiBold
                color: root.activePlatform !== "" ? Theme.accentCyan : Theme.textDim
            }

            MouseArea {
                id: resetSocialMouse
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                hoverEnabled: true
                onClicked: {
                    root.activePlatform = ""
                    root.resetSocial()
                }
            }
        }
    }

    // Grid of Social Media Platform Targets
    GridLayout {
        Layout.fillWidth: true
        columns: 2
        rowSpacing: 6
        columnSpacing: 6

        property var platforms: [
            { id: "ig", name: "Instagram Feed", aspect: "4:5 / 1080x1350", tag: "Max Screen" },
            { id: "story", name: "Stories / Reels", aspect: "9:16 / 1080x1920", tag: "Full Mobile" },
            { id: "x", name: "X / Twitter", aspect: "16:9 / 1200x675", tag: "Crisp Feed" },
            { id: "ig_square", name: "Classic Square", aspect: "1:1 / 1080x1080", tag: "Grid Post" },
            { id: "fb", name: "Facebook HD", aspect: "1.91:1 / 2048px", tag: "High Res" },
            { id: "yt", name: "YouTube Thumb", aspect: "16:9 / 1280x720", tag: "High CTR" }
        ]

        Repeater {
            model: parent.platforms
            delegate: Rectangle {
                Layout.fillWidth: true
                implicitHeight: 46
                radius: Theme.radiusSm
                color: root.activePlatform === modelData.id
                       ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.22)
                       : (optMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                border.color: root.activePlatform === modelData.id
                              ? Theme.accent
                              : (optMouse.containsMouse ? Theme.borderLight : Theme.border)
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 6
                    spacing: 2

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 4
                        Text {
                            text: modelData.name
                            textFormat: Text.PlainText
                            font.pixelSize: 10
                            font.weight: Font.Bold
                            color: root.activePlatform === modelData.id ? Theme.accent : Theme.textMain
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }
                        Rectangle {
                            implicitWidth: tagText.implicitWidth + 6
                            implicitHeight: 14
                            radius: 3
                            color: root.activePlatform === modelData.id ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgDark
                            border.color: root.activePlatform === modelData.id ? Theme.accent : Theme.border
                            border.width: 1
                            Text {
                                id: tagText
                                anchors.centerIn: parent
                                text: modelData.tag
                                textFormat: Text.PlainText
                                font.pixelSize: 8
                                font.weight: Font.Bold
                                color: root.activePlatform === modelData.id ? Theme.accent : Theme.textDim
                            }
                        }
                    }

                    Text {
                        text: modelData.aspect
                        textFormat: Text.PlainText
                        font.pixelSize: 9
                        font.family: Theme.monoFont
                        color: Theme.textMuted
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }

                MouseArea {
                    id: optMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        root.activePlatform = modelData.id;
                        root.triggerSocialOptimize(modelData.id);
                    }
                }
            }
        }
    }
}
