import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

Rectangle {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
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

    Process {
        id: cubePick
        command: ["zenity", "--file-selection", "--title=" + qsTr("Load 3D LUT"),
                  "--file-filter=" + qsTr("3D LUT") + " | *.cube *.CUBE"]
        onFinished: function(exitCode, output) {
            var p = String(output || "").trim();
            if (exitCode !== 0 || p.length === 0) return;
            root.activeLut = p.split("/").pop().replace(/\.cube$/i, "");
            root.customLutPath = p;
            root.lutChanged(root.activeLut, root.lutIntensity, p);
        }
    }

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
                visible: !root.embedded
                font.family: Theme.fontFamily
                text: qsTr("3D LUT")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            // Reset LUT
            Rectangle {
                implicitWidth: resetText.implicitWidth + 10
                implicitHeight: 18
                radius: Theme.r(4)
                color: resetMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    font.family: Theme.fontFamily
                    id: resetText
                    anchors.centerIn: parent
                    text: qsTr("Reset")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
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

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: Theme.px(28)
            radius: Theme.radiusSm
            color: loadMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
            border.color: Theme.border
            Text {
                anchors.centerIn: parent
                text: root.customLutPath !== "" ? root.customLutPath.split("/").pop() : qsTr("Load .cube file…")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(11)
                color: root.customLutPath !== "" ? Theme.accent : Theme.textMain
                elide: Text.ElideMiddle
                width: parent.width - Theme.px(16)
                horizontalAlignment: Text.AlignHCenter
            }
            MouseArea {
                id: loadMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: if (!cubePick.running) cubePick.running = true
            }
        }

        // A look saved by an older version (no file); shown so it can be removed
        Text {
            visible: root.activeLut !== "" && root.customLutPath === ""
            text: qsTr("Saved look: %1").arg(root.activeLut)
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(11)
            color: Theme.textMuted
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        // Intensity Slider (Visible when a LUT is active)
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            visible: root.activeLut !== "" || root.customLutPath !== ""

            RowLayout {
                Layout.fillWidth: true
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Amount")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    color: Theme.textDim
                    Layout.fillWidth: true
                }
                Text {
                    text: Math.round(root.lutIntensity * 100) + "%"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
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
                    radius: Theme.r(1.5)
                    color: Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.12)

                    Rectangle {
                        width: intensitySlider.visualPosition * parent.width
                        height: parent.height
                        color: Theme.accentCyan
                        radius: Theme.r(1.5)
                    }
                }

                handle: Rectangle {
                    x: intensitySlider.leftPadding + intensitySlider.visualPosition * (intensitySlider.availableWidth - width)
                    y: intensitySlider.topPadding + intensitySlider.availableHeight / 2 - height / 2
                    implicitWidth: 12
                    implicitHeight: 12
                    radius: Theme.r(6)
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
