import QtQuick
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    implicitHeight: 28
    implicitWidth: contentRow.implicitWidth + 16
    radius: Theme.radiusSm
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property string currentVersion: "A" // "A", "B", "C", "D"
    property var activeVersions: ({ "A": true }) // Map of versions that hold grades

    signal switchVersion(string ver)
    signal copyCurrentToVersion(string ver)

    RowLayout {
        id: contentRow
        anchors.centerIn: parent
        spacing: 6

        Text {
            text: "GRADE VERSION"
            textFormat: Text.PlainText
            font.pixelSize: 8
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
        }

        Repeater {
            model: ["A", "B", "C", "D"]
            delegate: Rectangle {
                implicitWidth: 22
                implicitHeight: 20
                radius: 4
                property bool isCur: root.currentVersion === modelData
                property bool hasData: root.activeVersions && root.activeVersions[modelData] === true
                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : (mouseArea.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                border.color: isCur ? Theme.accent : (hasData ? Theme.borderLight : Theme.border)
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: modelData
                    textFormat: Text.PlainText
                    font.pixelSize: 9
                    font.weight: Font.Bold
                    font.family: Theme.monoFont
                    color: parent.isCur ? Theme.accent : (parent.hasData ? Theme.textMain : Theme.textDim)
                }

                // Small dot indicator for versions containing saved grades
                Rectangle {
                    visible: parent.hasData && !parent.isCur
                    width: 3
                    height: 3
                    radius: 1.5
                    color: Theme.accentGreen
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 2
                    anchors.horizontalCenter: parent.horizontalCenter
                }

                MouseArea {
                    id: mouseArea
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    onClicked: function(mouse) {
                        if (mouse.button === Qt.RightButton) {
                            root.copyCurrentToVersion(modelData);
                        } else {
                            root.switchVersion(modelData);
                        }
                    }
                }
            }
        }
    }
}
