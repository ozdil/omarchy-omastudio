import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// Top-bar button that picks the interface language. The list is drawn like an
// Omarchy menu ([menu] tokens of the theme's shell.toml).
OmaButton {
    id: root
    icon: Theme.iconLanguage
    text: qsTr("Language")
    showLabel: false
    checked: menu.visible
    onClicked: menu.visible ? menu.close() : menu.open()

    Popup {
        id: menu
        y: root.height + Theme.sp(4)
        x: root.LayoutMirroring.enabled ? 0 : root.width - width
        width: Theme.px(220)
        height: Math.min(list.contentHeight + 2 * padding + header.height + Theme.sp(6), Theme.px(480))
        padding: Theme.sp(6)
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent
        // Keyboard: the list takes focus on open and walks with the arrow keys
        onOpened: list.forceActiveFocus()

        background: Rectangle {
            radius: Theme.radiusMd
            color: Theme.menuBg
            border.color: Theme.menuBorder
            border.width: Theme.borderWidth
        }

        contentItem: ColumnLayout {
            spacing: Theme.sp(6)

            Text {
                id: header
                Layout.fillWidth: true
                Layout.leftMargin: Theme.sp(6)
                Layout.rightMargin: Theme.sp(6)
                text: qsTr("INTERFACE LANGUAGE")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(9)
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textMuted
                elide: Text.ElideRight
            }

            ListView {
                id: list
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: I18n.languages
                boundsBehavior: Flickable.StopAtBounds
                keyNavigationEnabled: true
                currentIndex: {
                    for (var i = 0; i < I18n.languages.length; i++) if (I18n.languages[i].code === I18n.choice) return i;
                    return 0;
                }
                highlightMoveDuration: Theme.duration(80)
                Keys.onReturnPressed: { I18n.setLanguage(I18n.languages[list.currentIndex].code); menu.close(); }
                Keys.onEnterPressed: { I18n.setLanguage(I18n.languages[list.currentIndex].code); menu.close(); }
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Language")

                delegate: Rectangle {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property bool isCur: I18n.choice === modelData.code
                    readonly property bool isCursor: ListView.isCurrentItem && list.activeFocus
                    width: ListView.view.width
                    height: Theme.sp(22)
                    radius: Theme.radiusSm
                    color: rowMouse.containsMouse || row.isCursor ? Theme.menuSelectedBg : "transparent"
                    Accessible.role: Accessible.ListItem
                    Accessible.name: modelData.name
                    Accessible.selected: row.isCur

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: Theme.sp(8)
                        anchors.rightMargin: Theme.sp(8)
                        spacing: Theme.sp(6)

                        Text {
                            Layout.fillWidth: true
                            text: row.modelData.name
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(11)
                            font.weight: row.isCur ? Font.Bold : Font.Normal
                            color: row.isCur ? Theme.selectedText : Theme.menuText
                            elide: Text.ElideRight
                        }
                        Text {
                            visible: row.isCur
                            text: Theme.iconCheck
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(10)
                            color: Theme.selectedText
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
