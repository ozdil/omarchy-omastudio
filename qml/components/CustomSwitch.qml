import QtQuick
import QtQuick.Layouts
import "../theme"

Item {
    id: root
    implicitWidth: 36
    implicitHeight: 20
    Layout.preferredWidth: 36
    Layout.preferredHeight: 20
    Layout.minimumWidth: 36
    Layout.alignment: Qt.AlignVCenter

    property bool checked: false
    property color activeColor: Theme.accent
    property color inactiveColor: Theme.border
    property color thumbColor: "#ffffff"

    signal toggled(bool isChecked)

    Rectangle {
        id: track
        anchors.fill: parent
        radius: height / 2
        color: root.checked ? root.activeColor : Theme.bgCard
        border.color: root.checked ? root.activeColor : root.inactiveColor
        border.width: 1

        Behavior on color { ColorAnimation { duration: 120 } }
        Behavior on border.color { ColorAnimation { duration: 120 } }

        Rectangle {
            id: thumb
            width: parent.height - 4
            height: width
            radius: width / 2
            anchors.verticalCenter: parent.verticalCenter
            x: root.checked ? parent.width - width - 2 : 2
            color: root.thumbColor

            Behavior on x { NumberAnimation { duration: 120; easing.type: Easing.OutQuad } }
        }
    }

    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            root.checked = !root.checked;
            root.toggled(root.checked);
        }
    }
}
