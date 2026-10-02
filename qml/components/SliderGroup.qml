import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

ColumnLayout {
    id: root
    spacing: 4
    Layout.fillWidth: true

    property string title: "Adjustment"
    property real from: -100.0
    property real to: 100.0
    property real value: 0.0
    property real defaultValue: 0.0
    property real stepSize: 1.0
    property string suffix: ""
    property int decimals: 0
    property color accentColor: Theme.accent

    readonly property real effectiveValue: slider.pressed ? slider.value : root.value

    signal sliderMoved(real newValue)

    onValueChanged: {
        if (!slider.pressed && slider.value !== root.value) {
            slider.value = root.value;
        }
    }

    Binding {
        target: slider
        property: "value"
        value: root.value
        when: !slider.pressed
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 8

        Text {
            text: root.title
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            color: root.effectiveValue !== root.defaultValue ? Theme.textMain : Theme.textMuted
            font.pixelSize: 12
            font.weight: root.effectiveValue !== root.defaultValue ? Font.DemiBold : Font.Medium
            Layout.fillWidth: true
            elide: Text.ElideRight

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onDoubleClicked: {
                    slider.value = root.defaultValue
                    root.sliderMoved(root.defaultValue)
                }
            }
        }

        RowLayout {
            spacing: 4

            // Dedicated Reset Button (visible whenever slider value diverges from default)
            Rectangle {
                visible: Math.abs(root.effectiveValue - root.defaultValue) > 0.001
                implicitWidth: 18
                implicitHeight: 18
                radius: 4
                color: sliderResetMouse.containsMouse ? root.accentColor : "transparent"
                border.color: root.accentColor
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconRotateLeft
                    font.family: Theme.iconFont
                    font.pixelSize: 9
                    color: sliderResetMouse.containsMouse ? Theme.bgBase : root.accentColor
                }

                MouseArea {
                    id: sliderResetMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        slider.value = root.defaultValue
                        root.sliderMoved(root.defaultValue)
                    }
                }
            }

            Rectangle {
                implicitWidth: valText.implicitWidth + 10
                implicitHeight: 18
                radius: Theme.radiusSm
                color: root.effectiveValue !== root.defaultValue ? Qt.rgba(root.accentColor.r, root.accentColor.g, root.accentColor.b, 0.18) : Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.04)
                border.color: root.effectiveValue !== root.defaultValue ? root.accentColor : Theme.border
                border.width: 1

                Text {
                    id: valText
                    anchors.centerIn: parent
                    text: (root.decimals === 0 ? Math.round(root.effectiveValue) : root.effectiveValue.toFixed(root.decimals)) + root.suffix
                    textFormat: Text.PlainText
                    color: root.effectiveValue !== root.defaultValue ? root.accentColor : Theme.textMuted
                    font.pixelSize: 10
                    font.family: Theme.monoFont
                    font.weight: Font.DemiBold
                }

                MouseArea {
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onDoubleClicked: {
                        slider.value = root.defaultValue
                        root.sliderMoved(root.defaultValue)
                    }
                }
            }
        }
    }

    Slider {
        id: slider
        Layout.fillWidth: true
        from: root.from
        to: root.to
        stepSize: root.stepSize
        value: root.value

        background: Rectangle {
            x: slider.leftPadding
            y: slider.topPadding + slider.availableHeight / 2 - height / 2
            implicitWidth: 200
            implicitHeight: 3
            width: slider.availableWidth
            height: implicitHeight
            radius: 1.5
            color: Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.12)

            Rectangle {
                // Fill from center if bidirectional, or from left
                property real zeroPos: root.from < 0 && root.to > 0 ? (0 - root.from) / (root.to - root.from) * parent.width : 0
                property real curPos: slider.visualPosition * parent.width

                x: Math.min(zeroPos, curPos)
                width: Math.abs(curPos - zeroPos)
                height: parent.height
                color: root.accentColor
                radius: 1.5
            }
        }

        handle: Rectangle {
            x: slider.leftPadding + slider.visualPosition * (slider.availableWidth - width)
            y: slider.topPadding + slider.availableHeight / 2 - height / 2
            implicitWidth: 12
            implicitHeight: 12
            radius: 6
            color: slider.pressed ? root.accentColor : (slider.hovered ? Theme.accent : Theme.textMain)
            border.color: Theme.bgDark
            border.width: 1.5

            Behavior on color { ColorAnimation { duration: 100 } }
        }

        onMoved: {
            root.sliderMoved(slider.value)
        }
    }
}
