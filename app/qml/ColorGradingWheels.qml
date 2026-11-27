import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

ColumnLayout {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
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

    signal gradingChanged(var patch)

    function resetAllWheels() {
        root.gradingChanged({
            "liftRgb": [0.0, 0.0, 0.0],
            "liftLuma": 0.0,
            "gammaRgb": [0.0, 0.0, 0.0],
            "gammaLuma": 0.0,
            "gainRgb": [0.0, 0.0, 0.0],
            "gainLuma": 0.0,
            "offsetRgb": [0.0, 0.0, 0.0],
            "offsetLuma": 0.0,
            "colorBoost": 0.0,
            "midtoneDetail": 0.0,
            "contrastPivot": 0.435
        });
    }

    // Header with Title, Mode Switcher, and Reset All
    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Text {
            visible: !root.embedded
            font.family: Theme.fontFamily
            text: qsTr("Colour grading")
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(10)
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
            Layout.fillWidth: true
        }

        // Layout Toggle (Single / 4-Wheels)
        Rectangle {
            implicitWidth: viewToggleText.implicitWidth + 8
            implicitHeight: Theme.sp(18)
            radius: Theme.r(4)
            color: viewToggleMouse.containsMouse ? Theme.bgCardHover : "transparent"
            border.color: Theme.border
            border.width: 1

            Text {
                id: viewToggleText
                anchors.centerIn: parent
                text: root.fourWheelsMode ? qsTr("4 wheels") : qsTr("Tabs")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(8)
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
            KeyFocus {
                name: viewToggleText.text
                checkable: true
                checked: root.fourWheelsMode
                onActivated: root.fourWheelsMode = !root.fourWheelsMode
            }
        }

        // Reset Button
        Rectangle {
            implicitWidth: resetAllText.implicitWidth + 8
            implicitHeight: Theme.sp(18)
            radius: Theme.r(4)
            color: resetAllMouse.containsMouse ? Theme.bgCardHover : "transparent"
            border.color: Theme.border
            border.width: 1

            Text {
                id: resetAllText
                anchors.centerIn: parent
                text: qsTr("Reset")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(8)
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
            KeyFocus {
                name: resetAllText.text
                onActivated: root.resetAllWheels()
            }
        }
    }

    // Tab Switcher (Visible in Tabbed Mode)
    RowLayout {
        Layout.fillWidth: true
        spacing: 4
        visible: !root.fourWheelsMode

        property var tabs: [
            { name: qsTr("Lift"), sub: qsTr("Shadows") },
            { name: qsTr("Gamma"), sub: qsTr("Mids") },
            { name: qsTr("Gain"), sub: qsTr("Highlights") },
            { name: qsTr("Offset"), sub: qsTr("Global") }
        ]

        Repeater {
            model: parent.tabs
            delegate: Rectangle {
                required property var modelData
                required property int index
                Layout.fillWidth: true
                implicitHeight: Theme.sp(26)
                radius: Theme.r(4)
                property bool isCur: root.activeTab === index
                color: isCur ? Theme.controlFill("selected") : Theme.bgCard
                border.color: isCur ? "transparent" : Theme.controlBorder("normal")
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: modelData.name
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    font.family: Theme.monoFont
                    color: parent.isCur ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.activeTab = index
                }
                KeyFocus {
                    name: modelData.name
                    description: modelData.sub
                    role: Accessible.PageTab
                    checkable: true
                    checked: root.activeTab === index
                    onActivated: root.activeTab = index
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
                title: qsTr("Shadows Lift")
                rgbOffset: root.liftRgb
                lumaOffset: root.liftLuma
                wheelAccent: Theme.accentCyan
                onWheelChanged: function(rgb, luma) {
                    root.gradingChanged({ "liftRgb": rgb, "liftLuma": luma });
                }
            }

            ColorWheel {
                id: gammaWheel
                anchors.fill: parent
                visible: root.activeTab === 1
                title: qsTr("Midtones Gamma")
                rgbOffset: root.gammaRgb
                lumaOffset: root.gammaLuma
                wheelAccent: Theme.accentYellow
                onWheelChanged: function(rgb, luma) {
                    root.gradingChanged({ "gammaRgb": rgb, "gammaLuma": luma });
                }
            }

            ColorWheel {
                id: gainWheel
                anchors.fill: parent
                visible: root.activeTab === 2
                title: qsTr("Highlights Gain")
                rgbOffset: root.gainRgb
                lumaOffset: root.gainLuma
                wheelAccent: Theme.accentOrange
                onWheelChanged: function(rgb, luma) {
                    root.gradingChanged({ "gainRgb": rgb, "gainLuma": luma });
                }
            }

            ColorWheel {
                id: offsetWheel
                anchors.fill: parent
                visible: root.activeTab === 3
                title: qsTr("Master Offset")
                rgbOffset: root.offsetRgb
                lumaOffset: root.offsetLuma
                wheelAccent: Theme.accentPurple
                onWheelChanged: function(rgb, luma) {
                    root.gradingChanged({ "offsetRgb": rgb, "offsetLuma": luma });
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
                    id: liftWheelGrid
                    anchors.fill: parent
                    title: qsTr("Lift")
                    rgbOffset: root.liftRgb
                    lumaOffset: root.liftLuma
                    wheelAccent: Theme.accentCyan
                    onWheelChanged: function(rgb, luma) {
                        root.gradingChanged({ "liftRgb": rgb, "liftLuma": luma });
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
                    id: gammaWheelGrid
                    anchors.fill: parent
                    title: qsTr("Gamma")
                    rgbOffset: root.gammaRgb
                    lumaOffset: root.gammaLuma
                    wheelAccent: Theme.accentYellow
                    onWheelChanged: function(rgb, luma) {
                        root.gradingChanged({ "gammaRgb": rgb, "gammaLuma": luma });
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
                    id: gainWheelGrid
                    anchors.fill: parent
                    title: qsTr("Gain")
                    rgbOffset: root.gainRgb
                    lumaOffset: root.gainLuma
                    wheelAccent: Theme.accentOrange
                    onWheelChanged: function(rgb, luma) {
                        root.gradingChanged({ "gainRgb": rgb, "gainLuma": luma });
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
                    id: offsetWheelGrid
                    anchors.fill: parent
                    title: qsTr("Offset")
                    rgbOffset: root.offsetRgb
                    lumaOffset: root.offsetLuma
                    wheelAccent: Theme.accentPurple
                    onWheelChanged: function(rgb, luma) {
                        root.gradingChanged({ "offsetRgb": rgb, "offsetLuma": luma });
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
            title: qsTr("Color Boost")
            from: -100.0
            to: 100.0
            value: root.colorBoost
            defaultValue: 0.0
            stepSize: 1.0
            suffix: "%"
            accentColor: Theme.accentOrange
            onSliderMoved: function(v) {
                root.gradingChanged({ "colorBoost": v });
            }
        }

        SliderGroup {
            title: qsTr("Midtone Detail (MD)")
            from: -100.0
            to: 100.0
            value: root.midtoneDetail
            defaultValue: 0.0
            stepSize: 1.0
            suffix: "%"
            accentColor: Theme.accentCyan
            onSliderMoved: function(v) {
                root.gradingChanged({ "midtoneDetail": v });
            }
        }

        SliderGroup {
            title: qsTr("Contrast Pivot")
            from: 0.05
            to: 0.95
            value: root.contrastPivot
            defaultValue: 0.435
            stepSize: 0.01
            decimals: 3
            accentColor: Theme.accentPurple
            onSliderMoved: function(v) {
                root.gradingChanged({ "contrastPivot": v });
            }
        }
    }
}
