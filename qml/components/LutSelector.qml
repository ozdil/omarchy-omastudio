import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

Rectangle {
    id: root
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 16
    radius: Theme.radiusMd
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property string activeLut: ""
    property real lutIntensity: 1.0
    property string customLutPath: ""

    signal lutChanged(string name, real intensity, string path)

    ColumnLayout {
        id: mainCol
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 10
        spacing: 8

        // Header
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "DAVINCI 3D LUT ENGINE (.CUBE)"
                textFormat: Text.PlainText
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            // Reset LUT
            Rectangle {
                implicitWidth: resetText.implicitWidth + 10
                implicitHeight: 18
                radius: 4
                color: resetMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    id: resetText
                    anchors.centerIn: parent
                    text: "RESET"
                    textFormat: Text.PlainText
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.textDim
                }

                MouseArea {
                    id: resetMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        root.activeLut = "";
                        root.lutIntensity = 1.0;
                        root.customLutPath = "";
                        root.lutChanged("", 1.0, "");
                    }
                }
            }
        }

        // LUT Selection Buttons Grid
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            rowSpacing: 4
            columnSpacing: 4

            property var presets: [
                { name: "Kodak 2383", sub: "Hollywood Print", col: Theme.accentOrange },
                { name: "Teal & Orange", sub: "Cinema Blockbuster", col: Theme.accentCyan },
                { name: "Fuji Eterna", sub: "35mm Soft Stock", col: Theme.accentGreen },
                { name: "Silver Nitrate", sub: "High-Acutance B&W", col: Theme.accentPurple }
            ]

            Repeater {
                model: parent.presets
                delegate: Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 32
                    radius: 4
                    property bool isCur: root.activeLut === modelData.name
                    color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2) : (mouseArea.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                    border.color: isCur ? modelData.col : Theme.border
                    border.width: 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 6
                        spacing: 4

                        Rectangle {
                            implicitWidth: 6
                            implicitHeight: 6
                            radius: 3
                            color: modelData.col
                        }

                        ColumnLayout {
                            spacing: 0
                            Layout.fillWidth: true
                            Text {
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.pixelSize: 9
                                font.weight: Font.Bold
                                font.family: Theme.monoFont
                                color: parent.parent.parent.isCur ? modelData.col : Theme.textMain
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                text: modelData.sub
                                textFormat: Text.PlainText
                                font.pixelSize: 7
                                color: Theme.textDim
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }
                    }

                    MouseArea {
                        id: mouseArea
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        hoverEnabled: true
                        onClicked: {
                            if (root.activeLut === modelData.name) {
                                root.activeLut = "";
                            } else {
                                root.activeLut = modelData.name;
                                root.customLutPath = "";
                            }
                            root.lutChanged(root.activeLut, root.lutIntensity, root.customLutPath);
                        }
                    }
                }
            }
        }

        // Intensity Slider (Visible when a LUT is active)
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            visible: root.activeLut !== "" || root.customLutPath !== ""

            RowLayout {
                Layout.fillWidth: true
                Text {
                    text: "LUT Intensity (Mix)"
                    textFormat: Text.PlainText
                    font.pixelSize: 9
                    color: Theme.textDim
                    Layout.fillWidth: true
                }
                Text {
                    text: Math.round(root.lutIntensity * 100) + "%"
                    textFormat: Text.PlainText
                    font.pixelSize: 9
                    font.family: Theme.monoFont
                    color: Theme.accentCyan
                }
            }

            Slider {
                id: intensitySlider
                Layout.fillWidth: true
                from: 0.0
                to: 1.0
                stepSize: 0.02
                value: root.lutIntensity

                background: Rectangle {
                    x: intensitySlider.leftPadding
                    y: intensitySlider.topPadding + intensitySlider.availableHeight / 2 - height / 2
                    implicitWidth: 200
                    implicitHeight: 3
                    width: intensitySlider.availableWidth
                    height: implicitHeight
                    radius: 1.5
                    color: Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.12)

                    Rectangle {
                        width: intensitySlider.visualPosition * parent.width
                        height: parent.height
                        color: Theme.accentCyan
                        radius: 1.5
                    }
                }

                handle: Rectangle {
                    x: intensitySlider.leftPadding + intensitySlider.visualPosition * (intensitySlider.availableWidth - width)
                    y: intensitySlider.topPadding + intensitySlider.availableHeight / 2 - height / 2
                    implicitWidth: 12
                    implicitHeight: 12
                    radius: 6
                    color: intensitySlider.pressed ? Theme.accentCyan : (intensitySlider.hovered ? Theme.accent : Theme.textMain)
                    border.color: Theme.bgDark
                    border.width: 1.5
                    Behavior on color { ColorAnimation { duration: 100 } }
                }

                onMoved: {
                    root.lutIntensity = value;
                    root.lutChanged(root.activeLut, root.lutIntensity, root.customLutPath);
                }
            }
        }
    }
}
