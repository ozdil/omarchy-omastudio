import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

Rectangle {
    id: root
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 20
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property real grainAmount: 0.0
    property real grainSize: 1.0
    property real grainRoughness: 50.0

    signal grainChanged()

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        // Header & Reset
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "SILVER-HALIDE FILM GRAIN"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            Rectangle {
                implicitWidth: resetGrainText.implicitWidth + 10
                implicitHeight: 18
                radius: 4
                color: resetGrainMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    id: resetGrainText
                    anchors.centerIn: parent
                    text: "RESET"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.textDim
                }

                MouseArea {
                    id: resetGrainMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        root.grainAmount = 0.0;
                        root.grainSize = 1.0;
                        root.grainRoughness = 50.0;
                        root.grainChanged();
                    }
                }
            }
        }

        // Sliders
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8

            SliderGroup {
                title: "Grain Amount"
                from: 0.0
                to: 100.0
                value: root.grainAmount
                defaultValue: 0.0
                stepSize: 1.0
                accentColor: Theme.accentYellow
                onSliderMoved: function(v) {
                    root.grainAmount = v;
                    root.grainChanged();
                }
            }

            SliderGroup {
                title: "Grain Size"
                from: 1.0
                to: 3.0
                value: root.grainSize
                defaultValue: 1.0
                stepSize: 0.1
                decimals: 1
                accentColor: Theme.accentCyan
                opacity: root.grainAmount > 0 ? 1.0 : 0.4
                enabled: root.grainAmount > 0
                onSliderMoved: function(v) {
                    root.grainSize = v;
                    root.grainChanged();
                }
            }

            SliderGroup {
                title: "Grain Roughness"
                from: 0.0
                to: 100.0
                value: root.grainRoughness
                defaultValue: 50.0
                stepSize: 1.0
                accentColor: Theme.accentOrange
                opacity: root.grainAmount > 0 ? 1.0 : 0.4
                enabled: root.grainAmount > 0
                onSliderMoved: function(v) {
                    root.grainRoughness = v;
                    root.grainChanged();
                }
            }
        }
    }
}
