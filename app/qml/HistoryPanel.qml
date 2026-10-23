import QtQuick
import QtQuick.Layouts

// Edit history and snapshots for the open photo. History lists every state the undo
// stack holds, oldest first: a click goes back (or forward) to it. Snapshots are
// named states kept in the sidecar; a click on Restore brings one back as a new edit.
Rectangle {
    id: root
    implicitWidth: 260
    implicitHeight: column.implicitHeight + 20
    radius: Theme.radiusMd
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property var entries: []        // [{ label }], oldest first
    property int currentIndex: 0    // the entry shown now
    property var snapshots: []      // [{ name, created, recipe }]

    signal jumpRequested(int index)
    signal snapshotRequested()
    signal restoreRequested(int index)
    signal renameRequested(int index, string name)
    signal deleteRequested(int index)

    component Header: Text {
        textFormat: Text.PlainText
        font.family: Theme.fontFamily
        font.pixelSize: Theme.px(9)
        font.weight: Font.Bold
        color: Theme.textMuted
        Layout.fillWidth: true
    }

    component SmallButton: Rectangle {
        id: sb
        property string label: ""
        property color labelColor: Theme.accent
        signal clicked()
        implicitWidth: sbText.implicitWidth + 12
        implicitHeight: 20
        radius: Theme.r(4)
        color: sbMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
        border.color: Theme.border
        border.width: 1
        Text {
            id: sbText
            anchors.centerIn: parent
            text: sb.label
            textFormat: Text.PlainText
            font.family: Theme.monoFont
            font.pixelSize: Theme.px(8)
            font.weight: Font.Bold
            color: sb.labelColor
        }
        MouseArea {
            id: sbMouse
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            hoverEnabled: true
            onClicked: sb.clicked()
        }
    }

    ColumnLayout {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 10
        spacing: 6

        Header { text: qsTr("History") }

        Text {
            Layout.fillWidth: true
            text: qsTr("Click a step to go back to it. A new edit from there replaces the later steps.")
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(9)
            color: Theme.textDim
        }

        // The newest steps matter most: the list keeps its height and scrolls to the current one
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight, Theme.px(180))
            clip: true
            model: root.entries
            currentIndex: root.currentIndex
            highlightFollowsCurrentItem: false
            onCountChanged: positionViewAtIndex(root.currentIndex, ListView.Contain)
            onCurrentIndexChanged: positionViewAtIndex(root.currentIndex, ListView.Contain)

            delegate: Rectangle {
                id: row
                required property int index
                required property var modelData
                readonly property bool isCurrent: index === root.currentIndex
                readonly property bool isLater: index > root.currentIndex
                width: ListView.view.width
                height: Theme.px(20)
                radius: Theme.r(3)
                color: isCurrent ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
                                 : (rowMouse.containsMouse ? Theme.bgCardHover : "transparent")

                Text {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: 6
                    anchors.rightMargin: 6
                    text: row.modelData.label
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    font.weight: row.isCurrent ? Font.Bold : Font.Normal
                    color: row.isCurrent ? Theme.accent : (row.isLater ? Theme.textDim : Theme.textMain)
                    font.italic: row.isLater
                }
                MouseArea {
                    id: rowMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.jumpRequested(row.index)
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.topMargin: 4
            height: 1
            color: Theme.border
        }

        RowLayout {
            Layout.fillWidth: true
            Header { text: qsTr("Snapshots") }
            SmallButton {
                label: qsTr("New snapshot")
                labelColor: Theme.accentGreen
                onClicked: root.snapshotRequested()
            }
        }

        Text {
            Layout.fillWidth: true
            visible: root.snapshots.length === 0
            text: qsTr("A snapshot keeps the current edit under a name, in the sidecar, so you can come back to it later.")
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(9)
            color: Theme.textDim
        }

        Repeater {
            model: root.snapshots
            delegate: RowLayout {
                id: snap
                required property int index
                required property var modelData
                Layout.fillWidth: true
                spacing: 4

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: Theme.px(20)
                    radius: Theme.r(3)
                    color: Theme.bgCard
                    border.color: nameInput.activeFocus ? Theme.accent : Theme.border
                    border.width: 1

                    BoundedTextInput {
                        id: nameInput
                        anchors.fill: parent
                        anchors.leftMargin: 6
                        anchors.rightMargin: 6
                        verticalAlignment: TextEdit.AlignVCenter
                        maxLength: 80
                        text: snap.modelData.name
                        font.pixelSize: Theme.px(10)
                        color: Theme.textMain
                        onAccepted: focus = false
                        onEditingFinished: root.renameRequested(snap.index, text)
                    }
                }
                SmallButton {
                    label: qsTr("Restore")
                    labelColor: Theme.accentCyan
                    onClicked: root.restoreRequested(snap.index)
                }
                SmallButton {
                    label: "×"
                    labelColor: Theme.accentRed
                    onClicked: root.deleteRequested(snap.index)
                }
            }
        }
    }
}
