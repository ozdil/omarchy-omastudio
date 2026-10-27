import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// A collapsible section of the adjustments panel. Click the header to open or close
// it, Ctrl+click to keep only this section open. The open/closed state lives in the
// window (`host.collapsedSections`) so it is remembered between sessions.
ColumnLayout {
    id: sec
    Layout.fillWidth: true
    spacing: 0

    property string key: ""
    property string title: ""
    // Shows the header's reset button; `reset()` restores the section's defaults
    property bool resettable: true
    // The window: collapsedSections, toggleSection(key), soloSection(key)
    property var host: null
    // Dimmed and inactive while no photo is open
    property bool needsPhoto: true
    // Shown only in Pro mode; the header carries a "Pro" tag
    property bool pro: false
    enabled: !needsPhoto || (host !== null && host.activePhotoPath !== "")
    opacity: enabled ? 1.0 : 0.45

    readonly property bool expanded: !(host && host.collapsedSections[key] === true)

    signal reset()

    default property alias content: body.data

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: 1
        color: Theme.border
    }

    Item {
        Layout.fillWidth: true
        implicitHeight: Theme.px(32)

        MouseArea {
            id: headerMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: function(mouse) {
                if (!sec.host) return;
                if (mouse.modifiers & Qt.ControlModifier) sec.host.soloSection(sec.key);
                else sec.host.toggleSection(sec.key);
            }
            Accessible.role: Accessible.Button
            Accessible.name: sec.title
        }

        RowLayout {
            anchors.fill: parent
            spacing: Theme.px(8)

            Text {
                text: sec.expanded ? "" : ""
                font.family: Theme.iconFont
                font.pixelSize: Theme.px(8)
                color: headerMouse.containsMouse ? Theme.accent : Theme.textDim
                Layout.preferredWidth: Theme.px(10)
            }

            Text {
                Layout.fillWidth: true
                text: sec.title
                // Explicit so right-to-left layouts mirror it; left implicit, a Latin
                // title (Fujifilm) stayed on the left in Arabic, Hebrew and Persian
                horizontalAlignment: Text.AlignLeft
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(11)
                font.weight: Font.DemiBold
                color: sec.expanded || headerMouse.containsMouse ? Theme.textMain : Theme.textMuted
            }

            Rectangle {
                visible: sec.pro
                implicitWidth: proTag.implicitWidth + Theme.px(10)
                implicitHeight: Theme.px(16)
                radius: Theme.radiusSm
                color: "transparent"
                border.color: Theme.accent

                Text {
                    id: proTag
                    anchors.centerIn: parent
                    text: qsTr("Pro")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.DemiBold
                    color: Theme.accent
                }
            }

            Rectangle {
                visible: sec.resettable && sec.expanded
                implicitWidth: Theme.px(22)
                implicitHeight: Theme.px(22)
                radius: Theme.radiusSm
                color: resetMouse.containsMouse ? Theme.bgCardHover : "transparent"

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconRotateLeft
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: resetMouse.containsMouse ? Theme.accent : Theme.textDim
                }

                MouseArea {
                    id: resetMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: sec.reset()
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Reset %1").arg(sec.title)
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Reset %1").arg(sec.title)
                }
            }
        }
    }

    ColumnLayout {
        id: body
        visible: sec.expanded
        Layout.fillWidth: true
        Layout.bottomMargin: Theme.px(14)
        spacing: Theme.px(12)
    }
}
