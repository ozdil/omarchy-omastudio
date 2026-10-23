import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// Top-bar button that picks the interface language
Rectangle {
    id: root
    implicitWidth: Math.max(32, label8.implicitWidth + 16)
    implicitHeight: 28
    radius: Theme.radiusSm
    color: langMouse.containsMouse || menu.visible ? Theme.bgCardHover : Theme.bgCard
    border.color: langMouse.containsMouse || menu.visible ? Theme.accent : Theme.border
    border.width: 1

    Text {
        id: label8
        anchors.centerIn: parent
        text: Theme.iconLanguage
        font.family: Theme.iconFont
        font.pixelSize: 13
        color: langMouse.containsMouse || menu.visible ? Theme.accent : Theme.textDim
    }

    MouseArea {
        id: langMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        ToolTip.visible: containsMouse && !menu.visible
        ToolTip.delay: 500
        ToolTip.text: qsTr("Language")
        Accessible.role: Accessible.Button
        Accessible.name: qsTr("Language")
        onClicked: menu.visible ? menu.close() : menu.open()
    }

    Popup {
        id: menu
        y: root.height + 4
        x: root.LayoutMirroring.enabled ? 0 : root.width - width
        width: 240
        height: Math.min(list.contentHeight + 2 * padding + header.height + 6, 520)
        padding: 6
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent

        background: Rectangle {
            radius: Theme.radiusMd
            color: Theme.bgSurface
            border.color: Theme.borderLight
            border.width: 1
        }

        contentItem: ColumnLayout {
            spacing: 6

            Text {
                id: header
                Layout.fillWidth: true
                Layout.leftMargin: 6
                Layout.rightMargin: 6
                text: qsTr("INTERFACE LANGUAGE")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 9
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                elide: Text.ElideRight
            }

            ListView {
                id: list
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: I18n.languages
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

                delegate: Rectangle {
                    id: row
                    required property var modelData
                    readonly property bool isCur: I18n.choice === modelData.code
                    width: ListView.view.width
                    height: 26
                    radius: 4
                    color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : (rowMouse.containsMouse ? Theme.bgCardHover : "transparent")

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        spacing: 6

                        Text {
                            Layout.fillWidth: true
                            text: row.modelData.name
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: 11
                            font.weight: row.isCur ? Font.Bold : Font.Normal
                            color: row.isCur ? Theme.accent : Theme.textMain
                            elide: Text.ElideRight
                        }
                        Text {
                            visible: row.isCur
                            text: Theme.iconCheck
                            font.family: Theme.iconFont
                            font.pixelSize: 10
                            color: Theme.accent
                        }
                    }

                    MouseArea {
                        id: rowMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            I18n.setLanguage(row.modelData.code);
                            menu.close();
                        }
                    }
                }
            }
        }
    }
}
