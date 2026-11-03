import QtQuick

// Keyboard and screen-reader access for a control drawn with a Rectangle and a
// MouseArea. Put it inside the control: it fills it, takes keyboard focus (F6 /
// Shift+F6 move between controls, as Tab hides the panels), draws a focus ring while
// focused, and Space or Enter activates it. Screen readers get its role and name.
//
//     Rectangle {
//         MouseArea { anchors.fill: parent; onClicked: root.toggleCrop() }
//         KeyFocus { name: cropLabel; onActivated: root.toggleCrop() }
//     }
FocusScope {
    id: kf

    property string name: ""
    property string description: ""
    property int role: Accessible.Button
    property bool checkable: false
    property bool checked: false

    signal activated()

    anchors.fill: parent
    activeFocusOnTab: parent ? (parent.enabled && parent.visible) : false

    Accessible.role: kf.role
    Accessible.name: kf.name
    Accessible.description: kf.description
    Accessible.focusable: true
    Accessible.checkable: kf.checkable
    Accessible.checked: kf.checked
    Accessible.onPressAction: kf.activated()
    Accessible.onToggleAction: kf.activated()

    Keys.onSpacePressed: (event) => { kf.activated(); event.accepted = true; }
    Keys.onReturnPressed: (event) => { kf.activated(); event.accepted = true; }
    Keys.onEnterPressed: (event) => { kf.activated(); event.accepted = true; }

    Rectangle {
        anchors.fill: parent
        anchors.margins: -2
        radius: Theme.radiusSm
        color: "transparent"
        border.color: Theme.accent
        border.width: 2
        visible: kf.activeFocus
    }
}
