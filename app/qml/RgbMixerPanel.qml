import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

Rectangle {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
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

    signal mixerChanged(var patch)

    function resetMixer() {
        root.mixerChanged({
            "monochrome": false,
            "redInR": 1.0, "greenInR": 0.0, "blueInR": 0.0,
            "redInG": 0.0, "greenInG": 1.0, "blueInG": 0.0,
            "redInB": 0.0, "greenInB": 0.0, "blueInB": 1.0
        });
    }

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
                id: headerTitle
                visible: !root.embedded
                text: qsTr("Channel mixer")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(10)
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            CustomSwitch {
                checked: root.mixerEnabled
                accessibleName: headerTitle.text
                activeColor: Theme.accentCyan
                onToggled: function(isChecked) {
                    root.mixerChanged({ "mixerEnabled": isChecked });
                }
            }

            Rectangle {
                implicitWidth: resetMixerText.implicitWidth + 10
                implicitHeight: Theme.sp(18)
                radius: Theme.r(4)
                color: resetMixerMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    id: resetMixerText
                    anchors.centerIn: parent
                    text: qsTr("Reset")
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: Theme.px(8)
                    font.weight: Font.Bold
                    color: Theme.textDim
                }

                MouseArea {
                    id: resetMixerMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: root.resetMixer()
                }
                KeyFocus {
                    name: resetMixerText.text
                    onActivated: root.resetMixer()
                }
            }
        }

        // Monochrome Toggle
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: root.mixerEnabled

            Text {
                id: monoLabel
                text: qsTr("Monochrome Mixing")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(11)
                color: root.monochrome ? Theme.accentCyan : Theme.textDim
                Layout.fillWidth: true
            }

            CustomSwitch {
                checked: root.monochrome
                accessibleName: monoLabel.text
                activeColor: Theme.accentCyan
                onToggled: function(isChecked) {
                    // The monochrome mix uses the red row; untouched, that would be the
                    // red channel alone, so start from luminance weights instead
                    if (isChecked && root.redInR === 1.0 && root.greenInR === 0.0 && root.blueInR === 0.0) {
                        root.mixerChanged({
                            "monochrome": true,
                            "redInR": 0.21,
                            "greenInR": 0.72,
                            "blueInR": 0.07
                        });
                    } else {
                        root.mixerChanged({ "monochrome": isChecked });
                    }
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
                text: root.monochrome ? qsTr("Monochrome mix") : qsTr("Red output")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(9)
                font.weight: Font.Bold
                color: Theme.accentRed
            }

            SliderGroup {
                title: qsTr("Red in Red")
                from: -2.0
                to: 2.0
                value: root.redInR
                defaultValue: 1.0
                stepSize: 0.02
                decimals: 2
                accentColor: Theme.accentRed
                onSliderMoved: function(v) { root.mixerChanged({ "redInR": v }); }
            }

            SliderGroup {
                title: qsTr("Green in Red")
                from: -2.0
                to: 2.0
                value: root.greenInR
                defaultValue: 0.0
                stepSize: 0.02
                decimals: 2
                accentColor: Theme.accentGreen
                onSliderMoved: function(v) { root.mixerChanged({ "greenInR": v }); }
            }

            SliderGroup {
                title: qsTr("Blue in Red")
                from: -2.0
                to: 2.0
                value: root.blueInR
                defaultValue: 0.0
                stepSize: 0.02
                decimals: 2
                accentColor: Theme.accentBlue
                onSliderMoved: function(v) { root.mixerChanged({ "blueInR": v }); }
            }

            // GREEN OUTPUT CHANNEL (Hidden in monochrome)
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 8
                visible: !root.monochrome

                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Theme.border }

                Text {
                    text: qsTr("Green output")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: Theme.accentGreen
                }

                SliderGroup {
                    title: qsTr("Red in Green")
                    from: -2.0
                    to: 2.0
                    value: root.redInG
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentRed
                    onSliderMoved: function(v) { root.mixerChanged({ "redInG": v }); }
                }

                SliderGroup {
                    title: qsTr("Green in Green")
                    from: -2.0
                    to: 2.0
                    value: root.greenInG
                    defaultValue: 1.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentGreen
                    onSliderMoved: function(v) { root.mixerChanged({ "greenInG": v }); }
                }

                SliderGroup {
                    title: qsTr("Blue in Green")
                    from: -2.0
                    to: 2.0
                    value: root.blueInG
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentBlue
                    onSliderMoved: function(v) { root.mixerChanged({ "blueInG": v }); }
                }
            }

            // BLUE OUTPUT CHANNEL (Hidden in monochrome)
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 8
                visible: !root.monochrome

                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Theme.border }

                Text {
                    text: qsTr("Blue output")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: Theme.accentBlue
                }

                SliderGroup {
                    title: qsTr("Red in Blue")
                    from: -2.0
                    to: 2.0
                    value: root.redInB
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentRed
                    onSliderMoved: function(v) { root.mixerChanged({ "redInB": v }); }
                }

                SliderGroup {
                    title: qsTr("Green in Blue")
                    from: -2.0
                    to: 2.0
                    value: root.greenInB
                    defaultValue: 0.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentGreen
                    onSliderMoved: function(v) { root.mixerChanged({ "greenInB": v }); }
                }

                SliderGroup {
                    title: qsTr("Blue in Blue")
                    from: -2.0
                    to: 2.0
                    value: root.blueInB
                    defaultValue: 1.0
                    stepSize: 0.02
                    decimals: 2
                    accentColor: Theme.accentBlue
                    onSliderMoved: function(v) { root.mixerChanged({ "blueInB": v }); }
                }
            }
        }
    }
}
