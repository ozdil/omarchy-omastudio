import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    implicitWidth: 260
    implicitHeight: 92
    radius: Theme.radiusMd
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property var metadata: null

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 10
        spacing: 6

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Rectangle {
                width: 24
                height: 24
                radius: Theme.r(6)
                color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
                border.color: Theme.accent
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconCamera
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: Theme.accent
                }
            }

            ColumnLayout {
                spacing: 1
                Layout.fillWidth: true

                Text {
                    font.family: Theme.fontFamily
                    text: root.metadata ? (root.metadata.make + " " + root.metadata.model) : "No Camera Loaded"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(12)
                    font.weight: Font.Bold
                    color: Theme.textMain
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }

                Text {
                    font.family: Theme.fontFamily
                    text: root.metadata ? root.metadata.lens : "Unknown Lens"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    color: Theme.textMuted
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }
        }

        // Shot parameter badges
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            function formatShutter(s) {
                if (!s || s <= 0) return "--";
                if (s < 1.0) {
                    var denom = Math.round(1.0 / s);
                    return "1/" + denom + "s";
                }
                return s.toFixed(1) + "s";
            }

            // ISO Badge
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 22
                radius: Theme.r(4)
                color: Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: "ISO " + (root.metadata ? Math.round(root.metadata.iso) : "--")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.family: Theme.monoFont
                    font.weight: Font.DemiBold
                    color: Theme.accentYellow
                }
            }

            // Focal Length Badge
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 22
                radius: Theme.r(4)
                color: Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: (root.metadata ? Math.round(root.metadata.focal_length) : "--") + "mm"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.family: Theme.monoFont
                    color: Theme.textMain
                }
            }

            // Aperture Badge
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 22
                radius: Theme.r(4)
                color: Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: "f/" + (root.metadata ? root.metadata.aperture.toFixed(1) : "--")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.family: Theme.monoFont
                    color: Theme.accentCyan
                }
            }

            // Shutter Speed Badge
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 22
                radius: Theme.r(4)
                color: Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: root.metadata ? parent.parent.formatShutter(root.metadata.shutter) : "--"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.family: Theme.monoFont
                    color: Theme.accentGreen
                }
            }
        }
    }
}
