import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

ColumnLayout {
    id: root
    spacing: 6
    Layout.fillWidth: true

    property string title: "Wheel"
    property var rgbOffset: [0.0, 0.0, 0.0] // [R, G, B] each -1.0 to 1.0
    property real lumaOffset: 0.0           // -1.0 to 1.0
    property color wheelAccent: Theme.accent

    signal wheelChanged(var rgb, real luma)

    // Convert (Hue, Sat) to RGB offset
    function setFromPolar(normRadius, angleRad) {
        var sat = Math.max(0.0, Math.min(1.0, normRadius));
        var h = angleRad * (180.0 / Math.PI);
        if (h < 0) h += 360.0;

        // HSV to RGB offset
        var c = sat;
        var x = c * (1.0 - Math.abs(((h / 60.0) % 2.0) - 1.0));
        var r = 0.0, g = 0.0, b = 0.0;
        if (h >= 0 && h < 60) { r = c; g = x; b = 0; }
        else if (h >= 60 && h < 120) { r = x; g = c; b = 0; }
        else if (h >= 120 && h < 180) { r = 0; g = c; b = x; }
        else if (h >= 180 && h < 240) { r = 0; g = x; b = c; }
        else if (h >= 240 && h < 300) { r = x; g = 0; b = c; }
        else { r = c; g = 0; b = x; }

        // Shift so center is 0.0 (neutral)
        root.rgbOffset = [
            (r - 0.5 * sat) * 2.0,
            (g - 0.5 * sat) * 2.0,
            (b - 0.5 * sat) * 2.0
        ];
        root.wheelChanged(root.rgbOffset, root.lumaOffset);
    }

    function syncPuckFromRgb() {
        if (wheelMouse.pressed) return;
        var r = root.rgbOffset && root.rgbOffset.length >= 3 ? root.rgbOffset[0] : 0.0;
        var g = root.rgbOffset && root.rgbOffset.length >= 3 ? root.rgbOffset[1] : 0.0;
        var b = root.rgbOffset && root.rgbOffset.length >= 3 ? root.rgbOffset[2] : 0.0;

        var cx = wheelContainer.width / 2;
        var cy = wheelContainer.height / 2;
        var maxR = wheelContainer.width / 2 - 6;

        var xVec = r - 0.5 * g - 0.5 * b;
        var yVec = (Math.sqrt(3) / 2.0) * (g - b);
        var mag = Math.sqrt(xVec * xVec + yVec * yVec);

        if (mag < 0.001) {
            puck.x = cx - puck.width / 2;
            puck.y = cy - puck.height / 2;
        } else {
            var angle = Math.atan2(-yVec, xVec);
            var normDist = Math.min(1.0, mag);
            puck.x = cx + Math.cos(angle) * (normDist * maxR) - puck.width / 2;
            puck.y = cy + Math.sin(angle) * (normDist * maxR) - puck.height / 2;
        }
    }

    onRgbOffsetChanged: syncPuckFromRgb()

    function resetWheel() {
        root.rgbOffset = [0.0, 0.0, 0.0];
        root.lumaOffset = 0.0;
        lumaSlider.value = 0.0;
        puck.x = wheelContainer.width / 2 - puck.width / 2;
        puck.y = wheelContainer.height / 2 - puck.height / 2;
        root.wheelChanged(root.rgbOffset, root.lumaOffset);
    }

    // Title and Values Header
    RowLayout {
        Layout.fillWidth: true
        spacing: 6

        Text {
            text: root.title
            textFormat: Text.PlainText
            color: root.wheelAccent
            font.pixelSize: 11
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }

        // Reset Button
        Rectangle {
            implicitWidth: 18
            implicitHeight: 18
            radius: 9
            color: resetMouse.containsMouse ? Theme.bgCardHover : "transparent"
            Text {
                anchors.centerIn: parent
                text: Theme.iconRotateLeft
                font.family: Theme.iconFont
                font.pixelSize: 10
                color: resetMouse.containsMouse ? root.wheelAccent : Theme.textDim
            }
            MouseArea {
                id: resetMouse
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                hoverEnabled: true
                onClicked: root.resetWheel()
            }
        }
    }

    // Circular Color Wheel Canvas
    Item {
        id: wheelContainer
        Layout.alignment: Qt.AlignHCenter
        implicitWidth: 120
        implicitHeight: 120

        Canvas {
            id: wheelDisc
            anchors.fill: parent

            onPaint: {
                var ctx = getContext("2d");
                var cx = width / 2;
                var cy = height / 2;
                var r = width / 2 - 2;

                ctx.clearRect(0, 0, width, height);

                // Draw circular wheel segments
                var numSegments = 60;
                for (var i = 0; i < numSegments; i++) {
                    var startAngle = (i / numSegments) * Math.PI * 2;
                    var endAngle = ((i + 1.1) / numSegments) * Math.PI * 2;

                    var grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
                    var deg = (i / numSegments) * 360;
                    grad.addColorStop(0, Theme.bgCard);
                    grad.addColorStop(1, "hsl(" + Math.round(deg) + ", 85%, 55%)");

                    ctx.beginPath();
                    ctx.moveTo(cx, cy);
                    ctx.arc(cx, cy, r, startAngle, endAngle);
                    ctx.closePath();
                    ctx.fillStyle = grad;
                    ctx.fill();
                }

                // Center crosshair
                ctx.strokeStyle = "rgba(255, 255, 255, 0.25)";
                ctx.lineWidth = 1;
                ctx.beginPath();
                ctx.moveTo(cx - 10, cy);
                ctx.lineTo(cx + 10, cy);
                ctx.moveTo(cx, cy - 10);
                ctx.lineTo(cx, cy + 10);
                ctx.stroke();

                // Outer border ring
                ctx.beginPath();
                ctx.arc(cx, cy, r, 0, Math.PI * 2);
                ctx.strokeStyle = "rgba(255, 255, 255, 0.15)";
                ctx.lineWidth = 1.5;
                ctx.stroke();
            }

            Connections {
                target: Theme
                function onBgCardChanged() { wheelDisc.requestPaint(); }
            }

            Component.onCompleted: requestPaint()
        }

        // Draggable Center Puck
        Rectangle {
            id: puck
            width: 12
            height: 12
            radius: 6
            x: wheelContainer.width / 2 - width / 2
            y: wheelContainer.height / 2 - height / 2
            color: "#ffffff"
            border.color: Theme.bgBase
            border.width: 2
            z: 10

            Behavior on x { NumberAnimation { duration: 40; easing.type: Easing.OutQuad } }
            Behavior on y { NumberAnimation { duration: 40; easing.type: Easing.OutQuad } }
        }

        MouseArea {
            id: wheelMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.CrossCursor

            function handlePosition(mx, my) {
                var cx = wheelContainer.width / 2;
                var cy = wheelContainer.height / 2;
                var maxR = wheelContainer.width / 2 - 6;

                var dx = mx - cx;
                var dy = my - cy;
                var dist = Math.sqrt(dx * dx + dy * dy);
                var angle = Math.atan2(dy, dx);

                var clampedDist = Math.min(dist, maxR);
                var px = cx + Math.cos(angle) * clampedDist;
                var py = cy + Math.sin(angle) * clampedDist;

                puck.x = px - puck.width / 2;
                puck.y = py - puck.height / 2;

                root.setFromPolar(clampedDist / maxR, angle);
            }

            onPressed: function(mouse) {
                handlePosition(mouse.x, mouse.y);
            }

            onPositionChanged: function(mouse) {
                if (pressed) {
                    handlePosition(mouse.x, mouse.y);
                }
            }

            onDoubleClicked: {
                root.resetWheel();
            }
        }
    }

    // Master Luma Ring Slider
    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Text {
            text: "Y"
            textFormat: Text.PlainText
            font.pixelSize: 10
            font.weight: Font.Bold
            color: Theme.textDim
        }

        Slider {
            id: lumaSlider
            Layout.fillWidth: true
            from: -1.0
            to: 1.0
            stepSize: 0.01
            value: root.lumaOffset

            background: Rectangle {
                x: lumaSlider.leftPadding
                y: lumaSlider.topPadding + lumaSlider.availableHeight / 2 - height / 2
                width: lumaSlider.availableWidth
                height: 3
                radius: 1.5
                color: Theme.bgCard

                Rectangle {
                    property real zeroPos: parent.width / 2
                    property real curPos: lumaSlider.visualPosition * parent.width
                    x: Math.min(zeroPos, curPos)
                    width: Math.abs(curPos - zeroPos)
                    height: parent.height
                    color: root.wheelAccent
                    radius: 1.5
                }
            }

            handle: Rectangle {
                x: lumaSlider.leftPadding + lumaSlider.visualPosition * (lumaSlider.availableWidth - width)
                y: lumaSlider.topPadding + lumaSlider.availableHeight / 2 - height / 2
                width: 10
                height: 10
                radius: 5
                color: lumaSlider.pressed ? root.wheelAccent : Theme.textMain
                border.color: Theme.bgDark
                border.width: 1
            }

            onMoved: {
                root.lumaOffset = lumaSlider.value;
                root.wheelChanged(root.rgbOffset, root.lumaOffset);
            }
        }

        Binding {
            target: lumaSlider
            property: "value"
            value: root.lumaOffset
            when: !lumaSlider.pressed
        }

        Text {
            text: (root.lumaOffset >= 0 ? "+" : "") + Math.round(root.lumaOffset * 100)
            textFormat: Text.PlainText
            font.pixelSize: 9
            font.family: Theme.monoFont
            color: root.lumaOffset !== 0.0 ? root.wheelAccent : Theme.textDim
        }
    }
}
