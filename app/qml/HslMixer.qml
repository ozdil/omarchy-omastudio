import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

ColumnLayout {
    id: root
    spacing: 8
    Layout.fillWidth: true

    property var hslHue: [0, 0, 0, 0, 0, 0, 0, 0]
    property var hslSat: [0, 0, 0, 0, 0, 0, 0, 0]
    property var hslLum: [0, 0, 0, 0, 0, 0, 0, 0]

    property int activeChannel: 0

    signal colorChanged()
    signal hslValuesChanged(var h, var s, var l)

    function setChannelHue(val) {
        var arr = (root.hslHue ? root.hslHue.slice() : [0,0,0,0,0,0,0,0]);
        arr[root.activeChannel] = val;
        root.hslHue = arr;
        root.hslValuesChanged(root.hslHue, root.hslSat, root.hslLum);
        root.colorChanged();
    }

    function setChannelSat(val) {
        var arr = (root.hslSat ? root.hslSat.slice() : [0,0,0,0,0,0,0,0]);
        arr[root.activeChannel] = val;
        root.hslSat = arr;
        root.hslValuesChanged(root.hslHue, root.hslSat, root.hslLum);
        root.colorChanged();
    }

    function setChannelLum(val) {
        var arr = (root.hslLum ? root.hslLum.slice() : [0,0,0,0,0,0,0,0]);
        arr[root.activeChannel] = val;
        root.hslLum = arr;
        root.hslValuesChanged(root.hslHue, root.hslSat, root.hslLum);
        root.colorChanged();
    }

    function resetCurrentChannel() {
        var h = (root.hslHue ? root.hslHue.slice() : [0,0,0,0,0,0,0,0]);
        var s = (root.hslSat ? root.hslSat.slice() : [0,0,0,0,0,0,0,0]);
        var l = (root.hslLum ? root.hslLum.slice() : [0,0,0,0,0,0,0,0]);
        h[root.activeChannel] = 0;
        s[root.activeChannel] = 0;
        l[root.activeChannel] = 0;
        root.hslHue = h;
        root.hslSat = s;
        root.hslLum = l;
        root.hslValuesChanged(root.hslHue, root.hslSat, root.hslLum);
        root.colorChanged();
    }

    function resetAll() {
        root.hslHue = [0, 0, 0, 0, 0, 0, 0, 0];
        root.hslSat = [0, 0, 0, 0, 0, 0, 0, 0];
        root.hslLum = [0, 0, 0, 0, 0, 0, 0, 0];
        root.hslValuesChanged(root.hslHue, root.hslSat, root.hslLum);
        root.colorChanged();
    }

    readonly property var channelNames: [qsTr("Red"), qsTr("Orange"), qsTr("Yellow"), qsTr("Green"), qsTr("Aqua"), qsTr("Blue"), qsTr("Purple"), qsTr("Magenta")]
    readonly property var channelColors: [
        "#f7768e", "#ff9e64", "#e0af68", "#9ece6a",
        "#7dcfff", "#7aa2f7", "#bb9af7", "#f778ba"
    ]

    // Color Band Selector Pills
    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Repeater {
            model: 8
            delegate: Rectangle {
                Layout.fillWidth: true
                implicitHeight: 24
                radius: Theme.r(4)
                color: root.activeChannel === index ? root.channelColors[index] : Theme.bgCard
                border.color: root.channelColors[index]
                border.width: root.activeChannel === index ? 0 : 1

                Rectangle {
                    width: 8
                    height: 8
                    radius: Theme.r(4)
                    anchors.centerIn: parent
                    color: root.activeChannel === index ? Theme.bgBase : root.channelColors[index]
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        root.activeChannel = index
                    }
                }
            }
        }
    }

    // Active color title and Resets
    RowLayout {
        Layout.fillWidth: true
        spacing: 6

        Text {
            font.family: Theme.fontFamily
            text: qsTr("%1 Channel").arg(root.channelNames[root.activeChannel])
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(11)
            font.weight: Font.DemiBold
            color: root.channelColors[root.activeChannel]
            Layout.fillWidth: true
        }

        // Reset Channel Button
        Rectangle {
            implicitWidth: resetChText.implicitWidth + 10
            implicitHeight: 18
            radius: Theme.r(4)
            color: resetChMouse.containsMouse ? Theme.bgCardHover : "transparent"
            border.color: Theme.border
            border.width: 1

            Text {
                font.family: Theme.fontFamily
                id: resetChText
                anchors.centerIn: parent
                text: qsTr("RESET CH")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(8)
                font.weight: Font.Bold
                color: Theme.textDim
            }

            MouseArea {
                id: resetChMouse
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                hoverEnabled: true
                onClicked: root.resetCurrentChannel()
            }
        }

        // Reset All Channels Button
        Rectangle {
            implicitWidth: resetAllHslText.implicitWidth + 10
            implicitHeight: 18
            radius: Theme.r(4)
            color: resetAllHslMouse.containsMouse ? Theme.bgCardHover : "transparent"
            border.color: Theme.border
            border.width: 1

            Text {
                font.family: Theme.fontFamily
                id: resetAllHslText
                anchors.centerIn: parent
                text: qsTr("RESET ALL")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(8)
                font.weight: Font.Bold
                color: Theme.textDim
            }

            MouseArea {
                id: resetAllHslMouse
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                hoverEnabled: true
                onClicked: root.resetAll()
            }
        }
    }

    SliderGroup {
        title: qsTr("Hue Shift")
        from: -100.0
        to: 100.0
        value: (root.hslHue && root.hslHue.length > root.activeChannel) ? root.hslHue[root.activeChannel] : 0.0
        defaultValue: 0.0
        accentColor: root.channelColors[root.activeChannel]
        onSliderMoved: function(newVal) {
            root.setChannelHue(newVal)
        }
    }

    SliderGroup {
        title: qsTr("Saturation")
        from: -100.0
        to: 100.0
        value: (root.hslSat && root.hslSat.length > root.activeChannel) ? root.hslSat[root.activeChannel] : 0.0
        defaultValue: 0.0
        accentColor: root.channelColors[root.activeChannel]
        onSliderMoved: function(newVal) {
            root.setChannelSat(newVal)
        }
    }

    SliderGroup {
        title: qsTr("Luminance")
        from: -100.0
        to: 100.0
        value: (root.hslLum && root.hslLum.length > root.activeChannel) ? root.hslLum[root.activeChannel] : 0.0
        defaultValue: 0.0
        accentColor: root.channelColors[root.activeChannel]
        onSliderMoved: function(newVal) {
            root.setChannelLum(newVal)
        }
    }
}
