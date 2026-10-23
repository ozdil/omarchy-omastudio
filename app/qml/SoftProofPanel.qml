import QtQuick
import QtQuick.Layouts

// Soft proofing: the preview shows the photo as it would come out with an output
// profile (a print profile, or an export space) and can mark the colours that
// profile cannot reproduce. A view setting: the edit and the export are unchanged.
ColumnLayout {
    id: root
    spacing: 6

    property bool active: false
    property string profile: "sRGB"
    property string intent: "relative_colorimetric"
    property bool gamutWarning: false
    property var profiles: []   // [{ id, name }] from the engine

    signal activeToggled(bool on)
    signal profilePicked(string id)
    signal intentPicked(string id)
    signal gamutWarningToggled(bool on)

    component SwitchRow: RowLayout {
        id: sw
        property string label: ""
        property bool checked: false
        signal toggled(bool on)
        Layout.fillWidth: true
        spacing: 8
        Text {
            Layout.fillWidth: true
            text: sw.label
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            font.weight: sw.checked ? Font.Bold : Font.Normal
            color: sw.checked ? Theme.accent : Theme.textMain
            wrapMode: Text.WordWrap
        }
        CustomSwitch {
            Layout.preferredWidth: 36
            Layout.preferredHeight: 20
            checked: sw.checked
            onToggled: function(on) { sw.toggled(on) }
        }
    }

    component Hint: Text {
        Layout.fillWidth: true
        textFormat: Text.PlainText
        wrapMode: Text.WordWrap
        font.family: Theme.fontFamily
        font.pixelSize: Theme.px(9)
        color: Theme.textDim
    }

    SwitchRow {
        label: qsTr("Soft proof (S)")
        checked: root.active
        onToggled: function(on) { root.activeToggled(on) }
    }

    Hint {
        text: qsTr("Shows the photo as it will look with the profile below, for example a printer and paper profile. The edit and the export do not change.")
    }

    // Profiles: the built-in spaces and every ICC file found (see the ICC folders)
    Flow {
        Layout.fillWidth: true
        spacing: 4
        opacity: root.active ? 1 : 0.6
        Repeater {
            model: root.profiles
            delegate: Rectangle {
                id: chip
                required property var modelData
                readonly property bool isCur: root.profile === modelData.id
                width: chipText.implicitWidth + 12
                height: Theme.px(20)
                radius: Theme.r(4)
                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18)
                             : (chipMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                border.color: isCur ? Theme.accent : Theme.border
                border.width: 1
                Text {
                    id: chipText
                    anchors.centerIn: parent
                    text: chip.modelData.name
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(9)
                    color: chip.isCur ? Theme.accent : Theme.textMain
                }
                MouseArea {
                    id: chipMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.profilePicked(chip.modelData.id)
                }
            }
        }
    }

    Text {
        text: qsTr("Rendering intent")
        textFormat: Text.PlainText
        font.family: Theme.fontFamily
        font.pixelSize: Theme.px(9)
        font.weight: Font.Bold
        color: Theme.textMuted
    }
    RowLayout {
        Layout.fillWidth: true
        spacing: 4
        opacity: root.active ? 1 : 0.6
        Repeater {
            model: [
                { "id": "perceptual", "label": qsTr("Perceptual") },
                { "id": "relative_colorimetric", "label": qsTr("Relative") },
                { "id": "saturation", "label": qsTr("Saturation") },
                { "id": "absolute_colorimetric", "label": qsTr("Absolute") }
            ]
            delegate: Rectangle {
                id: seg
                required property var modelData
                readonly property bool isCur: root.intent === modelData.id
                Layout.fillWidth: true
                implicitHeight: Theme.px(20)
                radius: Theme.r(4)
                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18)
                             : (segMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                border.color: isCur ? Theme.accent : Theme.border
                border.width: 1
                Text {
                    anchors.centerIn: parent
                    width: parent.width - 4
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: seg.modelData.label
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(9)
                    color: seg.isCur ? Theme.accent : Theme.textMain
                }
                MouseArea {
                    id: segMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.intentPicked(seg.modelData.id)
                }
            }
        }
    }

    SwitchRow {
        label: qsTr("Gamut warning")
        checked: root.gamutWarning
        onToggled: function(on) { root.gamutWarningToggled(on) }
    }

    Hint {
        visible: root.gamutWarning
        text: qsTr("Magenta marks colours the profile cannot reproduce. The preview is sRGB, so wider spaces such as Adobe RGB show no warnings.")
    }
}
