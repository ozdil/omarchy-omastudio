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

    property bool skinToneEnabled: false
    property real skinTargetHue: 50.0
    property real skinHueRange: 32.0
    property real skinUniformityHue: 0.0
    property real skinUniformitySat: 0.0
    property real skinAmountHue: 0.0
    property real skinAmountSat: 0.0

    signal skinToneChanged()

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        // Header & Enable Switch
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "CAPTURE ONE SKIN TONE"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            CustomSwitch {
                checked: root.skinToneEnabled
                activeColor: Theme.accentOrange
                onToggled: function(isChecked) {
                    root.skinToneEnabled = isChecked;
                    root.skinToneChanged();
                }
            }

            Rectangle {
                implicitWidth: resetSkinText.implicitWidth + 10
                implicitHeight: 18
                radius: 4
                color: resetSkinMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    id: resetSkinText
                    anchors.centerIn: parent
                    text: "RESET"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.textDim
                }

                MouseArea {
                    id: resetSkinMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        root.skinTargetHue = 50.0;
                        root.skinHueRange = 32.0;
                        root.skinUniformityHue = 0.0;
                        root.skinUniformitySat = 0.0;
                        root.skinAmountHue = 0.0;
                        root.skinAmountSat = 0.0;
                        root.skinToneChanged();
                    }
                }
            }
        }

        // Target Hue & Color Preview Indicator
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: root.skinToneEnabled

            Rectangle {
                width: 24
                height: 24
                radius: 12
                border.color: Theme.border
                border.width: 1
                // Approximate skin hue swatch: 50 deg in Oklch is warm peach/apricot
                color: {
                    var h = root.skinTargetHue;
                    if (h < 40) return "#d4755a";
                    if (h < 50) return "#df8865";
                    if (h < 60) return "#e39a73";
                    if (h < 70) return "#e2ad81";
                    return "#e4bf92";
                }
            }

            Text {
                text: "Vectorscope 123 Flesh Line (Target: " + root.skinTargetHue.toFixed(0) + " deg)"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 11
                color: Theme.textDim
                Layout.fillWidth: true
            }
        }

        // Sliders (active when enabled)
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8
            opacity: root.skinToneEnabled ? 1.0 : 0.4
            enabled: root.skinToneEnabled

            SliderGroup {
                title: "Target Skin Hue"
                from: 30.0
                to: 75.0
                value: root.skinTargetHue
                defaultValue: 50.0
                stepSize: 1.0
                suffix: " deg"
                accentColor: Theme.accentOrange
                onSliderMoved: function(v) {
                    root.skinTargetHue = v;
                    root.skinToneChanged();
                }
            }

            SliderGroup {
                title: "Hue Tolerance Range"
                from: 15.0
                to: 60.0
                value: root.skinHueRange
                defaultValue: 32.0
                stepSize: 1.0
                suffix: " deg"
                accentColor: Theme.accentYellow
                onSliderMoved: function(v) {
                    root.skinHueRange = v;
                    root.skinToneChanged();
                }
            }

            SliderGroup {
                title: "Hue Uniformity"
                from: 0.0
                to: 100.0
                value: root.skinUniformityHue
                defaultValue: 0.0
                suffix: " %"
                accentColor: Theme.accentCyan
                onSliderMoved: function(v) {
                    root.skinUniformityHue = v;
                    root.skinToneChanged();
                }
            }

            SliderGroup {
                title: "Saturation Uniformity"
                from: 0.0
                to: 100.0
                value: root.skinUniformitySat
                defaultValue: 0.0
                suffix: " %"
                accentColor: Theme.accent
                onSliderMoved: function(v) {
                    root.skinUniformitySat = v;
                    root.skinToneChanged();
                }
            }

            SliderGroup {
                title: "Hue Shift"
                from: -100.0
                to: 100.0
                value: root.skinAmountHue
                defaultValue: 0.0
                accentColor: Theme.accentMagenta
                onSliderMoved: function(v) {
                    root.skinAmountHue = v;
                    root.skinToneChanged();
                }
            }

            SliderGroup {
                title: "Saturation Shift"
                from: -100.0
                to: 100.0
                value: root.skinAmountSat
                defaultValue: 0.0
                accentColor: Theme.accentGreen
                onSliderMoved: function(v) {
                    root.skinAmountSat = v;
                    root.skinToneChanged();
                }
            }
        }
    }
}
