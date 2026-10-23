import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    implicitWidth: 260
    implicitHeight: 120
    radius: Theme.radiusMd
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property var histData: null
    property bool showClippingHighlights: false
    property bool showClippingShadows: false

    signal toggleHighlightMask()
    signal toggleShadowMask()

    RowLayout {
        id: headerRow
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 8
        z: 2

        // Shadow clipping button
        Rectangle {
            implicitWidth: shadowText.implicitWidth + 12
            implicitHeight: 18
            radius: Theme.r(4)
            color: root.showClippingShadows ? Theme.shadowClip : Qt.rgba(0.2, 0.5, 1.0, 0.15)
            border.color: Theme.shadowClip

            Text {
                id: shadowText
                anchors.centerIn: parent
                text: qsTr("[S] %1%", "S: shadow clipping").arg(root.histData ? root.histData.shadow_clipping_percent.toFixed(1) : "0.0")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.family: Theme.monoFont
                color: root.showClippingShadows ? "#ffffff" : Theme.shadowClip
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    root.showClippingShadows = !root.showClippingShadows
                    root.toggleShadowMask()
                }
            }
        }

        Item { Layout.fillWidth: true }

        Text {
            font.family: Theme.fontFamily
            text: qsTr("RGB HISTOGRAM")
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(10)
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
            Layout.fillWidth: true
            elide: Text.ElideRight
        }

        // Highlight clipping button
        Rectangle {
            implicitWidth: highText.implicitWidth + 12
            implicitHeight: 18
            radius: Theme.r(4)
            color: root.showClippingHighlights ? Theme.highlightClip : Qt.rgba(1.0, 0.3, 0.4, 0.15)
            border.color: Theme.highlightClip

            Text {
                id: highText
                anchors.centerIn: parent
                text: qsTr("[H] %1%", "H: highlight clipping").arg(root.histData ? root.histData.highlight_clipping_percent.toFixed(1) : "0.0")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.family: Theme.monoFont
                color: root.showClippingHighlights ? "#ffffff" : Theme.highlightClip
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    root.showClippingHighlights = !root.showClippingHighlights
                    root.toggleHighlightMask()
                }
            }
        }
    }

    Canvas {
        id: canvas
        anchors.fill: parent
        anchors.topMargin: 28
        anchors.bottomMargin: 6
        anchors.leftMargin: 8
        anchors.rightMargin: 8

        onPaint: {
            var ctx = getContext("2d");
            ctx.clearRect(0, 0, width, height);

            if (!root.histData) return;

            var rArr = root.histData.red;
            var gArr = root.histData.green;
            var bArr = root.histData.blue;
            var lArr = root.histData.luma;
            var maxVal = Math.max(1, root.histData.max_count);

            var numBins = rArr.length;
            var binWidth = width / numBins;

            ctx.globalCompositeOperation = "screen";

            // Draw Red
            ctx.beginPath();
            ctx.fillStyle = "rgba(247, 118, 142, 0.45)";
            ctx.moveTo(0, height);
            for (var i = 0; i < numBins; i++) {
                var hR = Math.min(height, (rArr[i] / maxVal) * height);
                ctx.lineTo(i * binWidth, height - hR);
            }
            ctx.lineTo(width, height);
            ctx.closePath();
            ctx.fill();

            // Draw Green
            ctx.beginPath();
            ctx.fillStyle = "rgba(158, 206, 106, 0.45)";
            ctx.moveTo(0, height);
            for (var j = 0; j < numBins; j++) {
                var hG = Math.min(height, (gArr[j] / maxVal) * height);
                ctx.lineTo(j * binWidth, height - hG);
            }
            ctx.lineTo(width, height);
            ctx.closePath();
            ctx.fill();

            // Draw Blue
            ctx.beginPath();
            ctx.fillStyle = "rgba(122, 162, 247, 0.45)";
            ctx.moveTo(0, height);
            for (var k = 0; k < numBins; k++) {
                var hB = Math.min(height, (bArr[k] / maxVal) * height);
                ctx.lineTo(k * binWidth, height - hB);
            }
            ctx.lineTo(width, height);
            ctx.closePath();
            ctx.fill();

            // Draw Luminance outline
            ctx.globalCompositeOperation = "source-over";
            ctx.beginPath();
            ctx.strokeStyle = "rgba(255, 255, 255, 0.6)";
            ctx.lineWidth = 1.2;
            for (var m = 0; m < numBins; m++) {
                var hL = Math.min(height, (lArr[m] / maxVal) * height);
                if (m === 0) ctx.moveTo(0, height - hL);
                else ctx.lineTo(m * binWidth, height - hL);
            }
            ctx.stroke();
        }
    }

    onHistDataChanged: {
        canvas.requestPaint();
    }
}
