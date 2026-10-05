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

    property bool mixerEnabled: false
    property bool monochrome: false

    property real redInR: 1.0
    property real greenInR: 0.0
    property real blueInR: 0.0

    property real redInG: 0.0
    property real greenInG: 1.0
    property real blueInG: 0.0

    property real redInB: 0.0
    property real greenInB: 0.0
    property real blueInB: 1.0

    signal mixerChanged()

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        // Header & Switches
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "DAVINCI RGB PRIMARY MIXER"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            CustomSwitch {
                checked: root.mixerEnabled
                activeColor: Theme.accentCyan
                onToggled: function(isChecked) {
                    root.mixerEnabled = isChecked;
                    root.mixerChanged();
                }
            }

            Rectangle {
                implicitWidth: resetMixerText.implicitWidth + 10
                implicitHeight: 18
                radius: 4
                color: resetMixerMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    id: resetMixerText
                    anchors.centerIn: parent
                    text: "RESET"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.textDim
                }

                MouseArea {
                    id: resetMixerMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        root.monochrome = false;
                        root.redInR = 1.0;
                        root.greenInR = 0.0;
                        root.blueInR = 0.0;
                        root.redInG = 0.0;
                        root.greenInG = 1.0;
                        root.blueInG = 0.0;
                        root.redInB = 0.0;
                        root.greenInB = 0.0;
                        root.blueInB = 1.0;
                        root.mixerChanged();
                    }
                }
            }
        }

        // Monochrome Toggle
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: root.mixerEnabled

            Text {
                text: "Monochrome Mixing"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 11
                color: root.monochrome ? Theme.accentCyan : Theme.textDim
                Layout.fillWidth: true
            }

            CustomSwitch {
                checked: root.monochrome
                activeColor: Theme.accentCyan
                onToggled: function(isChecked) {
                    root.monochrome = isChecked;
                    root.mixerChanged();
                }
            }
        }

        // 3x3 Matrix Sliders
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 10
            opacity: root.mixerEnabled ? 1.0 : 0.4
            enabled: root.mixerEnabled

            // RED OUTPUT CHANNEL
            Text {
                text: root.monochrome ? "MONOCHROME LUMA MIX" : "RED OUTPUT CHANNEL"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 9
                font.weight: Font.Bold
                color: Theme.accentRed
            }

            SliderGroup {
                title: "Red in Red"
                from: -2.0
                to: 2.0
                value: root.redInR
                defaultValue: 1.0
                stepSize: 0.02
                decimals: 2
                accentColor: Theme.accentRed
                onSliderMoved: function(v) { root.redInR = v; root.mixerChanged(); }
            }

            SliderGroup {
                title: "Green in Red"
                from: -2.0
                to: 2.0
                value: root.greenInR
                defaultValue: 0.0
                stepSize: 0.02
                decimals: 2
                accentColor: Theme.accentGreen
                onSliderMoved: function(v) { root.greenInR = v; root.mixerChanged(); }
            }

            SliderGroup {
                title: "Blue in Red"
                from: -2.0
                to: 2.0
                value: root.blueInR
                defaultValue: 0.0
                stepSize: 0.02
                decimals: 2
                accentColor: Theme.accentBlue
                onSliderMoved: function(v) { root.blueInR = v; root.mixerChanged(); }
            }

            // GREEN OUTPUT CHANNEL (Hidden in monochrome)
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 8
                visible: !root.monochrome

                Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }

                Text {
                    text: "GREEN OUTPUT CHANNEL"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 9
                    font.weight: Font.Bold
                    color: Theme.accentGreen
                }

                SliderGroup {
                    title: "Red in Green"
                    from: -2.0
                    to: 2.0
                    value: root.redInG
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentRed
                    onSliderMoved: function(v) { root.redInG = v; root.mixerChanged(); }
                }

                SliderGroup {
                    title: "Green in Green"
                    from: -2.0
                    to: 2.0
                    value: root.greenInG
                    defaultValue: 1.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentGreen
                    onSliderMoved: function(v) { root.greenInG = v; root.mixerChanged(); }
                }

                SliderGroup {
                    title: "Blue in Green"
                    from: -2.0
                    to: 2.0
                    value: root.blueInG
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentBlue
                    onSliderMoved: function(v) { root.blueInG = v; root.mixerChanged(); }
                }
            }

            // BLUE OUTPUT CHANNEL (Hidden in monochrome)
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 8
                visible: !root.monochrome

                Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }

                Text {
                    text: "BLUE OUTPUT CHANNEL"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 9
                    font.weight: Font.Bold
                    color: Theme.accentBlue
                }

                SliderGroup {
                    title: "Red in Blue"
                    from: -2.0
                    to: 2.0
                    value: root.redInB
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentRed
                    onSliderMoved: function(v) { root.redInB = v; root.mixerChanged(); }
                }

                SliderGroup {
                    title: "Green in Blue"
                    from: -2.0
                    to: 2.0
                    value: root.greenInB
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentGreen
                    onSliderMoved: function(v) { root.greenInB = v; root.mixerChanged(); }
                }

                SliderGroup {
                    title: "Blue in Blue"
                    from: -2.0
                    to: 2.0
                    value: root.blueInB
                    defaultValue: 1.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentBlue
                    onSliderMoved: function(v) { root.blueInB = v; root.mixerChanged(); }
                }
            }
        }
    }
}
