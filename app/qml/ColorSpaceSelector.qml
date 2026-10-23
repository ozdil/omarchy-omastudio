import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

Rectangle {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 16
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property string activeColorSpace: "sRGB"
    property bool acesTonemap: false

    signal colorSpaceSelected(string space)
    signal acesTonemapToggled(bool enabled)

    readonly property var spaces: [
        { "id": "sRGB", "name": "sRGB (D65)", "desc": qsTr("Standard SDR web gamut"), "coverage": qsTr("100% sRGB"), "badgeColor": "#7aa2f7" },
        { "id": "DisplayP3", "name": "Display P3", "desc": qsTr("Wide gamut, Apple Retina displays"), "coverage": qsTr("125% sRGB"), "badgeColor": "#bb9af7" },
        { "id": "Rec2020", "name": "Rec.2020", "desc": qsTr("Ultra-wide gamut HDR master"), "coverage": qsTr("150% sRGB"), "badgeColor": "#9ece6a" },
        { "id": "ACEScg", "name": "ACEScg (AP1)", "desc": qsTr("Academy linear working space"), "coverage": qsTr("Wide AP1"), "badgeColor": "#e0af68" }
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
                visible: !root.embedded
                text: qsTr("Colour management")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(10)
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            Rectangle {
                implicitWidth: acesBadgeText.implicitWidth + 8
                implicitHeight: 16
                radius: Theme.r(3)
                color: Theme.bgCard
                border.color: Theme.accentYellow
                border.width: 1

                Text {
                    id: acesBadgeText
                    anchors.centerIn: parent
                    text: qsTr("Gamut compression")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(8)
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
                            radius: Theme.r(4)
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
                                font.pixelSize: Theme.px(10)
                                font.weight: root.activeColorSpace === modelData.id ? Font.Bold : Font.Normal
                                color: root.activeColorSpace === modelData.id ? Theme.accent : Theme.textMain
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }

                            Text {
                                text: qsTr("%1 (%2)").arg(modelData.desc).arg(modelData.coverage)
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.px(8)
                                color: Theme.textDim
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }

                        Text {
                            text: Theme.iconCheck
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(10)
                            color: Theme.accent
                            visible: root.activeColorSpace === modelData.id
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.activeColorSpace = modelData.id;
                            root.colorSpaceSelected(root.activeColorSpace);
                        }
                    }
                }
            }
        }

        // ACES 1.3 Fitted Tonemapper Toggle
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 38
            radius: Theme.radiusSm
            color: root.acesTonemap ? Theme.bgCardHover : Theme.bgCard
            border.color: root.acesTonemap ? Theme.accentYellow : Theme.border
            border.width: root.acesTonemap ? 1.5 : 1

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                anchors.topMargin: 4
                anchors.bottomMargin: 4
                spacing: 8

                Text {
                    text: Theme.iconAi
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(12)
                    color: root.acesTonemap ? Theme.accentYellow : Theme.textDim
                    Layout.alignment: Qt.AlignVCenter
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.alignment: Qt.AlignVCenter
                    spacing: 1

                    Text {
                        text: qsTr("ACES 1.3 RRT/ODT Tonemapper")
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(10)
                        font.weight: root.acesTonemap ? Font.Bold : Font.Normal
                        color: root.acesTonemap ? Theme.accentYellow : Theme.textMain
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                    }

                    Text {
                        text: qsTr("Filmic shoulder roll-off & dynamic range mapping")
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(8)
                        color: Theme.textDim
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                    }
                }

                CustomSwitch {
                    Layout.alignment: Qt.AlignVCenter | Qt.AlignRight
                    Layout.preferredWidth: 36
                    Layout.preferredHeight: 20
                    checked: root.acesTonemap
                    activeColor: Theme.accentYellow
                    onToggled: function(isChecked) {
                        root.acesTonemap = isChecked;
                        root.acesTonemapToggled(root.acesTonemap);
                    }
                }
            }
        }
    }
}
