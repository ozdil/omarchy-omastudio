import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// Heal and clone spots: the settings for new spots and the selected one. The spots
// themselves are placed on the photo (RetouchOverlay) while editing is on.
ColumnLayout {
    id: root
    spacing: 8

    property var spots: []
    property int selected: -1
    property bool editing: false
    property bool enabledForPhoto: true

    // Settings for new spots; editing them also changes the selected spot
    property string mode: "heal"
    property real radius: 0.02
    property real feather: 0.3
    property real opacity_: 1.0

    signal spotsEdited(var spots)

    readonly property var selectedSpot: selected >= 0 && selected < spots.length ? spots[selected] : null

    // Selecting a spot shows its settings
    onSelectedSpotChanged: {
        if (!selectedSpot) return;
        mode = selectedSpot.mode === "clone" ? "clone" : "heal";
        radius = selectedSpot.radius;
        feather = selectedSpot.feather;
        opacity_ = selectedSpot.opacity;
    }

    function setValue(key, v) {
        if (key === "mode") root.mode = v;
        else if (key === "radius") root.radius = v;
        else if (key === "feather") root.feather = v;
        else if (key === "opacity") root.opacity_ = v;
        if (!root.selectedSpot) return;
        var list = JSON.parse(JSON.stringify(root.spots));
        list[root.selected][key] = v;
        root.spotsEdited(list);
    }

    component SmallButton: Rectangle {
        id: sb
        property string label: ""
        property color labelColor: Theme.textMain
        property bool active: false
        signal clicked()
        implicitWidth: sbText.implicitWidth + 12
        implicitHeight: 20
        radius: Theme.r(4)
        opacity: enabled ? 1.0 : 0.4
        color: active || sbMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
        border.color: active ? sb.labelColor : Theme.border
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

    RowLayout {
        Layout.fillWidth: true
        spacing: 6
        SmallButton {
            label: root.editing ? qsTr("Done") : qsTr("Edit spots")
            active: root.editing
            enabled: root.enabledForPhoto
            labelColor: Theme.accent
            onClicked: root.editing = !root.editing
        }
        Item { Layout.fillWidth: true }
        SmallButton {
            label: qsTr("Heal")
            active: root.mode === "heal"
            labelColor: Theme.accentGreen
            onClicked: root.setValue("mode", "heal")
        }
        SmallButton {
            label: qsTr("Clone")
            active: root.mode === "clone"
            labelColor: Theme.accentCyan
            onClicked: root.setValue("mode", "clone")
        }
    }

    Text {
        Layout.fillWidth: true
        visible: root.editing
        text: qsTr("Click a spot on the photo to remove it. Drag the solid circle to move it and the dashed circle to pick another source.")
        textFormat: Text.PlainText
        wrapMode: Text.WordWrap
        font.family: Theme.fontFamily
        font.pixelSize: Theme.px(10)
        color: Theme.textDim
    }

    SliderGroup {
        title: qsTr("Size")
        from: 0.2; to: 10; stepSize: 0.1; decimals: 1; suffix: " %"
        defaultValue: 2
        value: root.radius * 100
        onSliderMoved: function(v) { root.setValue("radius", v / 100) }
    }
    SliderGroup {
        title: qsTr("Feather")
        from: 0; to: 100; defaultValue: 30
        value: root.feather * 100
        onSliderMoved: function(v) { root.setValue("feather", v / 100) }
    }
    SliderGroup {
        title: qsTr("Opacity")
        from: 0; to: 100; defaultValue: 100
        value: root.opacity_ * 100
        onSliderMoved: function(v) { root.setValue("opacity", v / 100) }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 6
        Text {
            Layout.fillWidth: true
            text: qsTr("Spots: %1").arg(root.spots.length)
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            color: Theme.textMuted
        }
        SmallButton {
            label: qsTr("Delete spot")
            enabled: root.selectedSpot !== null
            labelColor: Theme.accentRed
            onClicked: {
                var list = JSON.parse(JSON.stringify(root.spots));
                list.splice(root.selected, 1);
                root.selected = -1;
                root.spotsEdited(list);
            }
        }
        SmallButton {
            label: qsTr("Clear all")
            enabled: root.spots.length > 0
            labelColor: Theme.accentRed
            onClicked: {
                root.selected = -1;
                root.spotsEdited([]);
            }
        }
    }
}
