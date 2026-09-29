import QtQuick
import QtQuick.Layouts
import "../theme"

ColumnLayout {
    id: root
    spacing: 8
    Layout.fillWidth: true

    property string activePreset: ""

    signal applyPreset(string name)
    signal triggerAiAuto()
    signal resetPreset()

    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Text {
            text: "FILM LOOKS & STYLES"
            textFormat: Text.PlainText
            font.pixelSize: 10
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
            Layout.fillWidth: true
        }

        // Reset preset button (always accessible)
        Rectangle {
            visible: true
            implicitWidth: 46
            implicitHeight: 18
            radius: Theme.radiusSm
            color: resetPresetMouse.containsMouse ? Qt.rgba(Theme.accentMagenta.r, Theme.accentMagenta.g, Theme.accentMagenta.b, 0.2) : (root.activePreset !== "" ? Qt.rgba(Theme.accentMagenta.r, Theme.accentMagenta.g, Theme.accentMagenta.b, 0.1) : "transparent")
            border.color: root.activePreset !== "" ? Theme.accentMagenta : Theme.border
            border.width: 1

            Text {
                anchors.centerIn: parent
                text: "RESET"
                textFormat: Text.PlainText
                font.pixelSize: 8
                font.weight: Font.DemiBold
                color: root.activePreset !== "" ? Theme.accentMagenta : Theme.textDim
            }

            MouseArea {
                id: resetPresetMouse
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                hoverEnabled: true
                onClicked: {
                    root.activePreset = ""
                    root.resetPreset()
                }
            }
        }
    }

    // AI Auto Quick Action
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: 32
        radius: Theme.radiusSm
        color: aiAutoMouse.containsMouse ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.08)
        border.color: aiAutoMouse.containsMouse ? Theme.accent : Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.3)
        border.width: 1

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 8

            Text {
                text: Theme.iconAi
                font.family: Theme.iconFont
                font.pixelSize: 11
                color: Theme.accent
            }

            Text {
                text: "AI Auto Enhance"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 11
                font.weight: Font.DemiBold
                color: Theme.textMain
                Layout.fillWidth: true
            }

            Text {
                text: "RUN"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 9
                font.weight: Font.Bold
                color: Theme.accent
            }
        }

        MouseArea {
            id: aiAutoMouse
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            hoverEnabled: true
            onClicked: {
                root.triggerAiAuto()
            }
        }
    }

    // Grid of visual film simulations (Clean Omabeats style)
    GridLayout {
        Layout.fillWidth: true
        columns: 2
        rowSpacing: 5
        columnSpacing: 5

        property var presets: [
            { name: "Fuji Classic Chrome", sub: "Documentary Muted" },
            { name: "Fuji Velvia 50", sub: "Vivid Landscapes" },
            { name: "Kodak Portra 400", sub: "Warm Skin Tones" },
            { name: "Leica Monochrom HC", sub: "High Contrast B&W" },
            { name: "Cinematic Teal & Orange", sub: "Film Grade" }
        ]

        Repeater {
            model: parent.presets
            delegate: Rectangle {
                Layout.fillWidth: true
                implicitHeight: 34
                radius: Theme.radiusSm
                color: root.activePreset === modelData.name
                       ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.20)
                       : (presetMouse.containsMouse ? Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.08) : Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.03))
                border.color: root.activePreset === modelData.name
                              ? Theme.accent
                              : (presetMouse.containsMouse ? Theme.borderLight : Theme.border)
                border.width: 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 8
                    anchors.topMargin: 4
                    anchors.bottomMargin: 4
                    spacing: 0

                    Text {
                        text: modelData.name
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 10
                        font.weight: root.activePreset === modelData.name ? Font.Bold : Font.DemiBold
                        color: root.activePreset === modelData.name ? Theme.accent : Theme.textMain
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }

                    Text {
                        text: modelData.sub
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 8
                        color: Theme.textDim
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }

                MouseArea {
                    id: presetMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        root.activePreset = modelData.name
                        root.applyPreset(modelData.name)
                    }
                }
            }
        }
    }
}
