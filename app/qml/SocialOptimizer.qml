import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

ColumnLayout {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
    spacing: 8
    Layout.fillWidth: true

    property string activePlatform: ""

    signal triggerSocialOptimize(string platformCode)
    signal resetSocial()

    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Text {
            visible: !root.embedded
            font.family: Theme.fontFamily
            text: qsTr("Prepare for")
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(10)
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
                font.family: Theme.fontFamily
                anchors.centerIn: parent
                text: qsTr("Reset")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(8)
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
            { id: "ig", name: "Instagram post", aspect: "4:5 / 1080x1350" },
            { id: "story", name: "Stories and Reels", aspect: "9:16 / 1080x1920" },
            { id: "x", name: "X / Twitter", aspect: "16:9 / 1200x675" },
            { id: "ig_square", name: qsTr("Square"), aspect: "1:1 / 1080x1080" },
            { id: "fb", name: "Facebook", aspect: "1.91:1 / 2048px" },
            { id: "yt", name: qsTr("YouTube thumbnail"), aspect: "16:9 / 1280x720" }
        ]

        Repeater {
            model: parent.platforms
            delegate: Rectangle {
                Layout.fillWidth: true
                implicitHeight: 38
                radius: Theme.radiusSm
                color: root.activePlatform === modelData.id
                       ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.20)
                       : (optMouse.containsMouse ? Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.08) : Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.03))
                border.color: root.activePlatform === modelData.id
                              ? Theme.accent
                              : (optMouse.containsMouse ? Theme.borderLight : Theme.border)
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 8
                    anchors.topMargin: 4
                    anchors.bottomMargin: 4
                    spacing: 1

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 4
                        Text {
                            text: modelData.name
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(10)
                            font.weight: root.activePlatform === modelData.id ? Font.Bold : Font.DemiBold
                            color: root.activePlatform === modelData.id ? Theme.accent : Theme.textMain
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }
                    }

                    Text {
                        text: modelData.aspect
                        textFormat: Text.PlainText
                        font.pixelSize: Theme.px(8)
                        font.family: Theme.monoFont
                        color: Theme.textDim
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
