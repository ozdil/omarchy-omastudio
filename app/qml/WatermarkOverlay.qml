import QtQuick
import QtQuick.Layouts

Item {
    id: root
    anchors.fill: parent

    property bool shown: false
    property string watermarkType: "text"
    property string text: "OmaStudio Photography"
    property string logoPath: ""
    property int positionIndex: 8 // 0..8
    property real watermarkOpacity: 0.85
    property int size: 14
    property int margin: 24
    property string colorHex: "#ffffff"
    property bool dropShadow: true

    property var exifMetadata: null

    function getResolvedText() {
        var str = root.text;
        if (root.exifMetadata) {
            var cam = root.exifMetadata.make ? (root.exifMetadata.make + " " + root.exifMetadata.model) : (root.exifMetadata.model || "OmaStudio");
            str = str.replace(/\{camera\}/g, cam);
            str = str.replace(/\{lens\}/g, root.exifMetadata.lens || "");
            str = str.replace(/\{iso\}/g, root.exifMetadata.iso ? ("ISO " + Math.round(root.exifMetadata.iso)) : "");
            str = str.replace(/\{aperture\}/g, root.exifMetadata.aperture ? ("f/" + root.exifMetadata.aperture.toFixed(1)) : "");
            var s = root.exifMetadata.shutter;
            var shutterStr = (s && s > 0.0) ? (s < 1.0 ? ("1/" + Math.round(1.0 / s) + "s") : (s.toFixed(1) + "s")) : "";
            str = str.replace(/\{shutter\}/g, shutterStr);
            str = str.replace(/\{focal\}/g, root.exifMetadata.focal_length ? (Math.round(root.exifMetadata.focal_length) + "mm") : "");
        }
        return str;
    }

    Item {
        id: watermarkItem
        visible: root.shown && (root.watermarkType === "text" ? root.text.length > 0 : root.logoPath.length > 0)
        opacity: root.watermarkOpacity

        // 9-Point Placement
        readonly property int col: root.positionIndex % 3
        readonly property int row: Math.floor(root.positionIndex / 3)
        x: col === 0 ? root.margin
           : (col === 1 ? Math.round((parent.width - width) / 2)
           : Math.max(root.margin, parent.width - width - root.margin))
        y: row === 0 ? root.margin
           : (row === 1 ? Math.round((parent.height - height) / 2)
           : Math.max(root.margin, parent.height - height - root.margin))

        implicitWidth: contentRow.implicitWidth
        implicitHeight: contentRow.implicitHeight

        RowLayout {
            id: contentRow
            spacing: 6

            // Logo Image Mode
            Image {
                visible: root.watermarkType === "logo" && root.logoPath !== ""
                source: root.logoPath ? ("file://" + root.logoPath) : ""
                fillMode: Image.PreserveAspectFit
                Layout.preferredHeight: root.size * 2
                mipmap: true
            }

            // Text Mode
            Item {
                visible: root.watermarkType === "text"
                implicitWidth: mainText.implicitWidth
                implicitHeight: mainText.implicitHeight

                // Drop-Shadow layer
                Text {
                    id: shadowText
                    visible: root.dropShadow
                    x: 1.5
                    y: 1.5
                    text: root.getResolvedText()
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: root.size
                    font.weight: Font.DemiBold
                    color: "#11111b"
                }

                // Main Foreground Text
                Text {
                    id: mainText
                    text: root.getResolvedText()
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: root.size
                    font.weight: Font.DemiBold
                    color: root.colorHex
                }
            }
        }
    }
}
