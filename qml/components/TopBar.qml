import QtQuick
import QtQuick.Layouts
import "../theme"

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

    signal toggleMode()
    signal openFileClicked()
    signal toggleSplit()
    signal toggleCrop()
    signal zoomFit()
    signal zoom100()
    signal zoom200()
    signal undoClicked()
    signal redoClicked()
    signal exportClicked()

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
                radius: 6
                color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
                border.color: Theme.accent
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconCamera
                    font.family: Theme.iconFont
                    font.pixelSize: 13
                    color: Theme.accent
                }
            }
            Text {
                text: "OmaStudio"
                textFormat: Text.PlainText
                font.pixelSize: 14
                font.weight: Font.Black
                color: Theme.textMain
            }
            Rectangle {
                width: 44
                height: 18
                radius: 4
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    anchors.centerIn: parent
                    text: "STUDIO"
                    textFormat: Text.PlainText
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    font.letterSpacing: 1
                    color: Theme.accent
                }
            }
        }

        // Active filename badge
        Rectangle {
            visible: root.activePhotoName !== ""
            implicitWidth: photoNameText.implicitWidth + 14
            implicitHeight: 22
            radius: 4
            color: Theme.bgCard
            border.color: Theme.border
            Text {
                id: photoNameText
                anchors.centerIn: parent
                text: root.activePhotoName
                textFormat: Text.PlainText
                font.pixelSize: 10
                font.family: Theme.monoFont
                color: Theme.textMain
            }
        }

        Item { Layout.fillWidth: true }

        // Mode Switcher: Basit (Simple AI) vs Pro Studio (Monochrome Themeable)
        Rectangle {
            implicitWidth: 170
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
                            text: Theme.iconAi
                            font.family: Theme.iconFont
                            font.pixelSize: 10
                            color: !root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                        Text {
                            text: "Basit (AI)"
                            textFormat: Text.PlainText
                            font.pixelSize: 10
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
                            font.pixelSize: 10
                            color: root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                        Text {
                            text: "Pro Studio"
                            textFormat: Text.PlainText
                            font.pixelSize: 10
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
                radius: 4
                color: undoMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconUndo
                    font.family: Theme.iconFont
                    font.pixelSize: 11
                    color: undoMouse.containsMouse ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    id: undoMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: root.undoClicked()
                }
            }

            Rectangle {
                implicitWidth: 28
                implicitHeight: 28
                radius: 4
                color: redoMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconRedo
                    font.family: Theme.iconFont
                    font.pixelSize: 11
                    color: redoMouse.containsMouse ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    id: redoMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: root.redoClicked()
                }
            }
        }

        // Before / After Split Button (A | B)
        Rectangle {
            implicitWidth: 70
            implicitHeight: 28
            radius: Theme.radiusSm
            color: root.isSplitView ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
            border.color: root.isSplitView ? Theme.accent : Theme.border

            RowLayout {
                anchors.centerIn: parent
                spacing: 5
                Text {
                    text: Theme.iconSplit
                    font.family: Theme.iconFont
                    font.pixelSize: 10
                    color: root.isSplitView ? Theme.accent : Theme.textMuted
                }
                Text {
                    text: "A | B"
                    textFormat: Text.PlainText
                    font.pixelSize: 10
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
            implicitWidth: 68
            implicitHeight: 28
            radius: Theme.radiusSm
            color: root.isCropMode ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
            border.color: root.isCropMode ? Theme.accent : Theme.border

            RowLayout {
                anchors.centerIn: parent
                spacing: 5
                Text {
                    text: Theme.iconCrop
                    font.family: Theme.iconFont
                    font.pixelSize: 10
                    color: root.isCropMode ? Theme.accent : Theme.textMuted
                }
                Text {
                    text: "Crop"
                    textFormat: Text.PlainText
                    font.pixelSize: 10
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
                width: 28
                height: 28
                radius: 4
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    anchors.centerIn: parent
                    text: "Fit"
                    textFormat: Text.PlainText
                    font.pixelSize: 9
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
                radius: 4
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    anchors.centerIn: parent
                    text: "1:1"
                    textFormat: Text.PlainText
                    font.pixelSize: 9
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
                radius: 4
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    anchors.centerIn: parent
                    text: "2:1"
                    textFormat: Text.PlainText
                    font.pixelSize: 9
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

        // Buy Me a Coffee / Sponsor Button
        Rectangle {
            implicitWidth: 32
            implicitHeight: 28
            radius: Theme.radiusSm
            color: mouseCoffee.containsMouse ? Qt.rgba(1.0, 0.86, 0.0, 0.25) : Qt.rgba(1.0, 0.86, 0.0, 0.12)
            border.color: mouseCoffee.containsMouse ? "#FFDD00" : Qt.rgba(1.0, 0.86, 0.0, 0.3)
            border.width: 1

            Text {
                anchors.centerIn: parent
                text: Theme.iconCoffee
                font.family: Theme.iconFont
                font.pixelSize: 13
                color: mouseCoffee.containsMouse ? "#FFDD00" : Theme.textMain
            }

            MouseArea {
                id: mouseCoffee
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: Qt.openUrlExternally("https://buymeacoffee.com/ozdil")
            }
        }

        // Open Photo Button
        Rectangle {
            implicitWidth: 92
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            RowLayout {
                anchors.centerIn: parent
                spacing: 6
                Text {
                    text: Theme.iconFolder
                    font.family: Theme.iconFont
                    font.pixelSize: 11
                    color: Theme.textMain
                }
                Text {
                    text: "Open RAW"
                    textFormat: Text.PlainText
                    font.pixelSize: 11
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
            implicitWidth: 88
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.accent

            RowLayout {
                anchors.centerIn: parent
                spacing: 6
                Text {
                    text: Theme.iconExport
                    font.family: Theme.iconFont
                    font.pixelSize: 11
                    color: Theme.bgBase
                }
                Text {
                    text: "Export"
                    textFormat: Text.PlainText
                    font.pixelSize: 11
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
