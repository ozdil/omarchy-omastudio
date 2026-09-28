import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

ColumnLayout {
    id: root
    spacing: 8
    Layout.fillWidth: true

    property var liftRgb: [0.0, 0.0, 0.0]
    property real liftLuma: 0.0

    property var gammaRgb: [0.0, 0.0, 0.0]
    property real gammaLuma: 0.0

    property var gainRgb: [0.0, 0.0, 0.0]
    property real gainLuma: 0.0

    property var offsetRgb: [0.0, 0.0, 0.0]
    property real offsetLuma: 0.0

    // DaVinci Resolve Primaries
    property real colorBoost: 0.0
    property real midtoneDetail: 0.0
    property real contrastPivot: 0.435

    property int activeTab: 0 // 0: Lift, 1: Gamma, 2: Gain, 3: Offset
    property bool fourWheelsMode: false

    signal gradingChanged()

    function resetAllWheels() {
        liftRgb = [0.0, 0.0, 0.0];
        liftLuma = 0.0;
        gammaRgb = [0.0, 0.0, 0.0];
        gammaLuma = 0.0;
        gainRgb = [0.0, 0.0, 0.0];
        gainLuma = 0.0;
        offsetRgb = [0.0, 0.0, 0.0];
        offsetLuma = 0.0;
        colorBoost = 0.0;
        midtoneDetail = 0.0;
        contrastPivot = 0.435;
        liftWheel.resetWheel();
        gammaWheel.resetWheel();
        gainWheel.resetWheel();
        offsetWheel.resetWheel();
        root.gradingChanged();
    }

    // Header with Title, Mode Switcher, and Reset All
    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Text {
            text: "DAVINCI PRIMARIES WHEELS"
            textFormat: Text.PlainText
            font.pixelSize: 10
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
            Layout.fillWidth: true
        }

        // Layout Toggle (Single / 4-Wheels)
        Rectangle {
            implicitWidth: viewToggleText.implicitWidth + 8
            implicitHeight: 18
            radius: 4
            color: viewToggleMouse.containsMouse ? Theme.bgCardHover : "transparent"
            border.color: Theme.border
            border.width: 1

            Text {
                id: viewToggleText
                anchors.centerIn: parent
                text: root.fourWheelsMode ? "4-WHEEL" : "TABBED"
                textFormat: Text.PlainText
                font.pixelSize: 8
                font.weight: Font.Bold
                font.family: Theme.monoFont
                color: root.fourWheelsMode ? Theme.accentCyan : Theme.textDim
            }

            MouseArea {
                id: viewToggleMouse
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                hoverEnabled: true
                onClicked: root.fourWheelsMode = !root.fourWheelsMode
            }
        }

        // Reset Button
        Rectangle {
            implicitWidth: resetAllText.implicitWidth + 8
            implicitHeight: 18
            radius: 4
            color: resetAllMouse.containsMouse ? Theme.bgCardHover : "transparent"
            border.color: Theme.border
            border.width: 1

            Text {
                id: resetAllText
                anchors.centerIn: parent
                text: "RESET"
                textFormat: Text.PlainText
                font.pixelSize: 8
                font.weight: Font.Bold
                font.family: Theme.monoFont
                color: Theme.textDim
            }

            MouseArea {
                id: resetAllMouse
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                hoverEnabled: true
                onClicked: root.resetAllWheels()
            }
        }
    }

    // Tab Switcher (Visible in Tabbed Mode)
    RowLayout {
        Layout.fillWidth: true
        spacing: 4
        visible: !root.fourWheelsMode

        property var tabs: [
            { name: "LIFT", sub: "Shadows" },
            { name: "GAMMA", sub: "Mids" },
            { name: "GAIN", sub: "Highlights" },
            { name: "OFFSET", sub: "Global" }
        ]

        Repeater {
            model: parent.tabs
            delegate: Rectangle {
                Layout.fillWidth: true
                implicitHeight: 26
                radius: 4
                property bool isCur: root.activeTab === index
                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2) : Theme.bgCard
                border.color: isCur ? Theme.accent : Theme.border
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: modelData.name
                    textFormat: Text.PlainText
                    font.pixelSize: 9
                    font.weight: Font.Bold
                    font.family: Theme.monoFont
                    color: parent.isCur ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.activeTab = index
                }
            }
        }
    }

    // 1. Single Wheel Display (Tabbed)
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: 175
        radius: Theme.radiusSm
        color: Theme.bgCard
        border.color: Theme.border
        border.width: 1
        visible: !root.fourWheelsMode

        Item {
            anchors.fill: parent
            anchors.margins: 10

            ColorWheel {
                id: liftWheel
                anchors.fill: parent
                visible: root.activeTab === 0
                title: "Shadows Lift"
                rgbOffset: root.liftRgb
                lumaOffset: root.liftLuma
                wheelAccent: Theme.accentCyan
                onWheelChanged: function(rgb, luma) {
                    root.liftRgb = rgb;
                    root.liftLuma = luma;
                    root.gradingChanged();
                }
            }

            ColorWheel {
                id: gammaWheel
                anchors.fill: parent
                visible: root.activeTab === 1
                title: "Midtones Gamma"
                rgbOffset: root.gammaRgb
                lumaOffset: root.gammaLuma
                wheelAccent: Theme.accentYellow
                onWheelChanged: function(rgb, luma) {
                    root.gammaRgb = rgb;
                    root.gammaLuma = luma;
                    root.gradingChanged();
                }
            }

            ColorWheel {
                id: gainWheel
                anchors.fill: parent
                visible: root.activeTab === 2
                title: "Highlights Gain"
                rgbOffset: root.gainRgb
                lumaOffset: root.gainLuma
                wheelAccent: Theme.accentOrange
                onWheelChanged: function(rgb, luma) {
                    root.gainRgb = rgb;
                    root.gainLuma = luma;
                    root.gradingChanged();
                }
            }

            ColorWheel {
                id: offsetWheel
                anchors.fill: parent
                visible: root.activeTab === 3
                title: "Master Offset"
                rgbOffset: root.offsetRgb
                lumaOffset: root.offsetLuma
                wheelAccent: Theme.accentPurple
                onWheelChanged: function(rgb, luma) {
                    root.offsetRgb = rgb;
                    root.offsetLuma = luma;
                    root.gradingChanged();
                }
            }
        }
    }

    // 2. 4-Wheels 2x2 Grid View
    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: 6
        rowSpacing: 6
        visible: root.fourWheelsMode

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 160
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            Item {
                anchors.fill: parent
                anchors.margins: 6
                ColorWheel {
                    anchors.fill: parent
                    title: "Lift"
                    rgbOffset: root.liftRgb
                    lumaOffset: root.liftLuma
                    wheelAccent: Theme.accentCyan
                    onWheelChanged: function(rgb, luma) {
                        root.liftRgb = rgb;
                        root.liftLuma = luma;
                        root.gradingChanged();
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 160
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            Item {
                anchors.fill: parent
                anchors.margins: 6
                ColorWheel {
                    anchors.fill: parent
                    title: "Gamma"
                    rgbOffset: root.gammaRgb
                    lumaOffset: root.gammaLuma
                    wheelAccent: Theme.accentYellow
                    onWheelChanged: function(rgb, luma) {
                        root.gammaRgb = rgb;
                        root.gammaLuma = luma;
                        root.gradingChanged();
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 160
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            Item {
                anchors.fill: parent
                anchors.margins: 6
                ColorWheel {
                    anchors.fill: parent
                    title: "Gain"
                    rgbOffset: root.gainRgb
                    lumaOffset: root.gainLuma
                    wheelAccent: Theme.accentOrange
                    onWheelChanged: function(rgb, luma) {
                        root.gainRgb = rgb;
                        root.gainLuma = luma;
                        root.gradingChanged();
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 160
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            Item {
                anchors.fill: parent
                anchors.margins: 6
                ColorWheel {
                    anchors.fill: parent
                    title: "Offset"
                    rgbOffset: root.offsetRgb
                    lumaOffset: root.offsetLuma
                    wheelAccent: Theme.accentPurple
                    onWheelChanged: function(rgb, luma) {
                        root.offsetRgb = rgb;
                        root.offsetLuma = luma;
                        root.gradingChanged();
                    }
                }
            }
        }
    }

    // DaVinci Resolve Signature Primaries Sliders
    ColumnLayout {
        Layout.fillWidth: true
        spacing: 6

        SliderGroup {
            title: "Color Boost"
            from: -100.0
            to: 100.0
            value: root.colorBoost
            defaultValue: 0.0
            stepSize: 1.0
            suffix: "%"
            accentColor: Theme.accentOrange
            onSliderMoved: function(v) {
                root.colorBoost = v;
                root.gradingChanged();
            }
        }

        SliderGroup {
            title: "Midtone Detail (MD)"
            from: -100.0
            to: 100.0
            value: root.midtoneDetail
            defaultValue: 0.0
            stepSize: 1.0
            suffix: "%"
            accentColor: Theme.accentCyan
            onSliderMoved: function(v) {
                root.midtoneDetail = v;
                root.gradingChanged();
            }
        }

        SliderGroup {
            title: "Contrast Pivot"
            from: 0.05
            to: 0.95
            value: root.contrastPivot
            defaultValue: 0.435
            stepSize: 0.01
            decimals: 3
            accentColor: Theme.accentPurple
            onSliderMoved: function(v) {
                root.contrastPivot = v;
                root.gradingChanged();
            }
        }
    }
}
