import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

Rectangle {
    id: root
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 16
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property string activeColorSpace: "sRGB"
    property bool acesTonemap: false

    signal colorSpaceChanged(string space)
    signal acesTonemapChanged(bool enabled)

    readonly property var spaces: [
        { "id": "sRGB", "name": "sRGB (D65)", "desc": "Standard SDR Web gamut", "coverage": "100% sRGB", "badgeColor": "#7aa2f7" },
        { "id": "DisplayP3", "name": "Display P3", "desc": "Wide gamut, Apple Retina displays", "coverage": "125% sRGB", "badgeColor": "#bb9af7" },
        { "id": "Rec2020", "name": "Rec.2020", "desc": "Ultra-wide gamut HDR master", "coverage": "150% sRGB", "badgeColor": "#9ece6a" },
        { "id": "ACEScg", "name": "ACEScg (AP1)", "desc": "Academy linear working space", "coverage": "Wide AP1", "badgeColor": "#e0af68" }
    ]

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        // Header
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "COLOR MANAGEMENT & ACES 1.3"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            Rectangle {
                implicitWidth: acesBadgeText.implicitWidth + 8
                implicitHeight: 16
                radius: 3
                color: Theme.bgCard
                border.color: Theme.accentYellow
                border.width: 1

                Text {
                    id: acesBadgeText
                    anchors.centerIn: parent
                    text: "ACES RGC"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.accentYellow
                }
            }
        }

        // Color Space Options
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 5

            Repeater {
                model: root.spaces

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 38
                    radius: Theme.radiusSm
                    color: root.activeColorSpace === modelData.id ? Theme.bgCardHover : Theme.bgCard
                    border.color: root.activeColorSpace === modelData.id ? Theme.accent : Theme.border
                    border.width: root.activeColorSpace === modelData.id ? 1.5 : 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 8

                        Rectangle {
                            width: 8
                            height: 8
                            radius: 4
                            color: modelData.badgeColor
                            Layout.alignment: Qt.AlignVCenter
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1

                            Text {
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: 10
                                font.weight: root.activeColorSpace === modelData.id ? Font.Bold : Font.Normal
                                color: root.activeColorSpace === modelData.id ? Theme.accent : Theme.textMain
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }

                            Text {
                                text: modelData.desc + " (" + modelData.coverage + ")"
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: 8
                                color: Theme.textDim
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }

                        Text {
                            text: Theme.iconCheck
                            font.family: Theme.iconFont
                            font.pixelSize: 10
                            color: Theme.accent
                            visible: root.activeColorSpace === modelData.id
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.activeColorSpace = modelData.id;
                            root.colorSpaceChanged(root.activeColorSpace);
                        }
                    }
                }
            }
        }

        // ACES 1.3 Fitted Tonemapper Toggle
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 36
            radius: Theme.radiusSm
            color: root.acesTonemap ? Theme.bgCardHover : Theme.bgCard
            border.color: root.acesTonemap ? Theme.accentYellow : Theme.border
            border.width: root.acesTonemap ? 1.5 : 1

            RowLayout {
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8

                Text {
                    text: Theme.iconAi
                    font.family: Theme.iconFont
                    font.pixelSize: 12
                    color: root.acesTonemap ? Theme.accentYellow : Theme.textDim
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1

                    Text {
                        text: "ACES 1.3 RRT/ODT Tonemapper"
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 10
                        font.weight: root.acesTonemap ? Font.Bold : Font.Normal
                        color: root.acesTonemap ? Theme.accentYellow : Theme.textMain
                    }

                    Text {
                        text: "Filmic shoulder roll-off & dynamic range mapping"
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 8
                        color: Theme.textDim
                    }
                }

                CheckBox {
                    checked: root.acesTonemap
                    onToggled: {
                        root.acesTonemap = checked;
                        root.acesTonemapChanged(root.acesTonemap);
                    }
                }
            }
        }
    }
}
