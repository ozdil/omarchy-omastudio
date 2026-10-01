import QtQuick
import QtQuick.Layouts
import "../theme"

Item {
    id: root
    anchors.fill: parent

    property bool enabled: false
    property string watermarkType: "text"
    property string text: "OmaStudio Photography"
    property string logoPath: ""
    property int positionIndex: 8 // 0..8
    property real opacity: 0.85
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
            var shutterStr = (s && s < 1.0 && s > 0.0) ? ("1/" + Math.round(1.0 / s) + "s") : ((s ? s.toFixed(1) : "") + "s");
            str = str.replace(/\{shutter\}/g, shutterStr);
            str = str.replace(/\{focal\}/g, root.exifMetadata.focal_length ? (Math.round(root.exifMetadata.focal_length) + "mm") : "");
        }
        return str;
    }

    Item {
        id: watermarkItem
        visible: root.enabled && (root.watermarkType === "text" ? root.text.length > 0 : root.logoPath.length > 0)
        opacity: root.opacity

        // 9-Point Anchor Placement
        anchors.left: (root.positionIndex === 0 || root.positionIndex === 3 || root.positionIndex === 6) ? parent.left : undefined
        anchors.horizontalCenter: (root.positionIndex === 1 || root.positionIndex === 4 || root.positionIndex === 7) ? parent.horizontalCenter : undefined
        anchors.right: (root.positionIndex === 2 || root.positionIndex === 5 || root.positionIndex === 8) ? parent.right : undefined

        anchors.top: (root.positionIndex === 0 || root.positionIndex === 1 || root.positionIndex === 2) ? parent.top : undefined
        anchors.verticalCenter: (root.positionIndex === 3 || root.positionIndex === 4 || root.positionIndex === 5) ? parent.verticalCenter : undefined
        anchors.bottom: (root.positionIndex === 6 || root.positionIndex === 7 || root.positionIndex === 8) ? parent.bottom : undefined

        anchors.margins: root.margin

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
                height: root.size * 2
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
