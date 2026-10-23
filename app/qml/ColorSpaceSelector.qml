import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// Gamut compression and the ACES tone curve. The output colour space is chosen
// in the Export window, where the image is converted with ICC profiles.
Rectangle {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 16
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property bool gamutCompression: false
    property bool acesTonemap: false

    signal gamutCompressionToggled(bool enabled)
    signal acesTonemapToggled(bool enabled)

    component OptionRow: Rectangle {
        id: row
        property string title
        property string description
        property bool checked: false
        signal toggled(bool isChecked)

        Layout.fillWidth: true
        implicitHeight: rowLayout.implicitHeight + 12
        radius: Theme.radiusSm
        color: row.checked ? Theme.bgCardHover : Theme.bgCard
        border.color: row.checked ? Theme.accent : Theme.border
        border.width: row.checked ? 1.5 : 1

        RowLayout {
            id: rowLayout
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            anchors.topMargin: 6
            anchors.bottomMargin: 6
            spacing: 8

            ColumnLayout {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                Layout.alignment: Qt.AlignVCenter
                spacing: 2

                Text {
                    text: row.title
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    font.weight: row.checked ? Font.Bold : Font.Normal
                    color: row.checked ? Theme.accent : Theme.textMain
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                }

                Text {
                    text: row.description
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    color: Theme.textDim
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                }
            }

            CustomSwitch {
                Layout.alignment: Qt.AlignVCenter | Qt.AlignRight
                Layout.preferredWidth: 36
                Layout.preferredHeight: 20
                checked: row.checked
                onToggled: function(isChecked) { row.toggled(isChecked); }
            }
        }
    }

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 6

        Text {
            visible: !root.embedded
            text: qsTr("Colour management")
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
            Layout.fillWidth: true
        }

        OptionRow {
            title: qsTr("Gamut compression")
            description: qsTr("Brings colours pushed outside the gamut back in smoothly instead of clipping them.")
            checked: root.gamutCompression
            onToggled: function(isChecked) {
                root.gamutCompression = isChecked;
                root.gamutCompressionToggled(isChecked);
            }
        }

        OptionRow {
            title: qsTr("ACES tone curve")
            description: qsTr("Filmic highlight roll-off, a fitted approximation of the ACES output transform.")
            checked: root.acesTonemap
            onToggled: function(isChecked) {
                root.acesTonemap = isChecked;
                root.acesTonemapToggled(isChecked);
            }
        }

        Text {
            text: qsTr("The output colour space is set in the Export window.")
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            color: Theme.textDim
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
        }
    }
}
