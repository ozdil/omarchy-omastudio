import QtQuick
import QtQuick.Controls

// A button styled like the Omarchy shell's own controls: fill, border and their
// strengths come from the theme's [controls] tokens (normal, hover-cursor, focus,
// selected, pressed), so it changes with the theme and with the user's
// ~/.config/omarchy/shell.toml. Selected and primary buttons carry the accent in
// their text, as Omarchy's menus do, instead of a solid accent fill.
//
//     OmaButton { icon: Theme.iconCrop; text: cropLabel; checkable: true; checked: root.isCropMode
//                 tip: cropTip; onClicked: root.toggleCrop() }
Rectangle {
    id: root

    property string text: ""
    property string icon: ""
    // Tooltip; an icon-only button shows its label there
    property string tip: ""
    property string accessibleName: root.text !== "" ? root.text : root.tip
    property bool checkable: false
    property bool checked: false
    // The window's main action (Export): accent border and text
    property bool primary: false
    // A destructive action (Reset, Delete): the theme's urgent colour
    property bool danger: false
    // false draws the icon alone (narrow toolbars)
    property bool showLabel: true
    property real fontSize: Theme.px(10)
    property real iconSize: root.fontSize
    property int fontWeight: Font.DemiBold
    property real padding: Theme.sp(8)
    // Width with the label shown, whatever showLabel is: toolbars use it to decide
    // when to fall back to icons
    readonly property real fullWidth: Math.max(root.implicitHeight,
        (root.icon !== "" ? iconMetrics.advanceWidth : 0)
        + (root.text !== "" ? labelMetrics.advanceWidth + (root.icon !== "" ? row.spacing : 0) : 0)
        + 2 * root.padding)

    signal clicked()

    readonly property bool hovered: mouse.containsMouse
    readonly property bool down: mouse.pressed
    readonly property bool labelShown: root.text !== "" && (root.showLabel || root.icon === "")
    readonly property string controlState: !root.enabled ? "normal"
        : root.down ? "pressed"
        : root.checked ? "selected"
        : root.hovered ? "hover-cursor"
        : keyFocus.activeFocus ? "focus"
        : "normal"
    readonly property color contentColor: root.danger ? Theme.urgent
        : (root.primary || root.checked) ? Theme.selectedText
        : root.hovered ? Theme.textMain
        : Theme.textMain

    implicitHeight: Theme.sp(24)
    implicitWidth: root.labelShown ? root.fullWidth
        : Math.max(root.implicitHeight, row.implicitWidth + 2 * root.padding)
    radius: Theme.radiusSm
    opacity: root.enabled ? 1 : 0.45
    color: root.primary && root.controlState === "normal"
        ? Theme.withAlpha(Theme.accent, 0.10)
        : Theme.controlFill(root.controlState)
    border.width: root.primary ? Math.max(1, Theme.controlBorderWidth(root.controlState)) : Theme.controlBorderWidth(root.controlState)
    border.color: root.primary ? Theme.accent
        : root.danger && root.hovered ? Theme.withAlpha(Theme.urgent, 0.6)
        : Theme.controlBorder(root.controlState)

    Behavior on color { ColorAnimation { duration: Theme.duration(90) } }

    TextMetrics { id: labelMetrics; font.family: Theme.fontFamily; font.pixelSize: root.fontSize; font.weight: root.fontWeight; text: root.text }
    TextMetrics { id: iconMetrics; font.family: Theme.iconFont; font.pixelSize: root.iconSize; text: root.icon }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: Theme.sp(5)
        scale: root.down ? 0.96 : 1
        Behavior on scale { NumberAnimation { duration: Theme.duration(80) } }

        Text {
            visible: root.icon !== ""
            anchors.verticalCenter: parent.verticalCenter
            text: root.icon
            font.family: Theme.iconFont
            font.pixelSize: root.iconSize
            color: root.contentColor
        }
        Text {
            visible: root.labelShown
            anchors.verticalCenter: parent.verticalCenter
            text: root.text
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: root.fontSize
            font.weight: root.fontWeight
            color: root.contentColor
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: root.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: root.clicked()
        ToolTip.visible: containsMouse && (root.tip !== "" || !root.labelShown)
        ToolTip.delay: 500
        ToolTip.text: root.tip !== "" ? root.tip : root.text
    }
    KeyFocus {
        id: keyFocus
        name: root.accessibleName
        checkable: root.checkable
        checked: root.checked
        onActivated: root.clicked()
    }
}
