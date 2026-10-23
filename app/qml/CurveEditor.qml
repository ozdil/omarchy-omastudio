import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// Point curve editor: an RGB curve and one per channel, drawn over the histogram.
// Points are [x, y] pairs on 0..1; an empty list is the straight line. The line is
// a monotone cubic through the points, the same curve the engine applies.
ColumnLayout {
    id: root
    spacing: 6

    property var pointsRgb: []
    property var pointsRed: []
    property var pointsGreen: []
    property var pointsBlue: []
    property var histData: null

    // "rgb", "red", "green", "blue"
    property string channel: "rgb"

    signal pointsEdited(string channel, var points)

    // Channel colours mean the channel, so they stay the same in every theme
    readonly property var channelColors: ({ "rgb": Theme.textMain, "red": "#ff5c5c", "green": "#5cd65c", "blue": "#5c9dff" })

    function pointsFor(ch) {
        var p = ch === "red" ? pointsRed : ch === "green" ? pointsGreen : ch === "blue" ? pointsBlue : pointsRgb;
        return (p && p.length >= 2) ? p : [[0, 0], [1, 1]];
    }

    function isStraight(p) {
        if (!p || p.length < 2) return true;
        for (var i = 0; i < p.length; ++i)
            if (Math.abs(p[i][0] - p[i][1]) > 1e-4) return false;
        return p[0][0] < 1e-4 && p[p.length - 1][0] > 1 - 1e-4;
    }

    function publish(ch, p) {
        root.pointsEdited(ch, isStraight(p) ? [] : p);
    }

    // Fritsch–Carlson tangents, mirrored from src/pipeline/curves.rs
    function tangents(p) {
        var n = p.length, d = [], m = [];
        for (var i = 0; i < n - 1; ++i) d.push((p[i + 1][1] - p[i][1]) / (p[i + 1][0] - p[i][0]));
        m.push(d[0]);
        for (i = 1; i < n - 1; ++i) m.push(d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2);
        m.push(d[n - 2]);
        for (i = 0; i < n - 1; ++i) {
            if (d[i] === 0) { m[i] = 0; m[i + 1] = 0; continue; }
            var a = m[i] / d[i], b = m[i + 1] / d[i], s = a * a + b * b;
            if (s > 9) { var t = 3 / Math.sqrt(s); m[i] = t * a * d[i]; m[i + 1] = t * b * d[i]; }
        }
        return m;
    }

    function valueAt(p, m, x) {
        var n = p.length;
        if (x <= p[0][0]) return p[0][1];
        if (x >= p[n - 1][0]) return p[n - 1][1];
        var i = 0;
        while (i < n - 2 && x >= p[i + 1][0]) ++i;
        var h = p[i + 1][0] - p[i][0], t = (x - p[i][0]) / h, t2 = t * t, t3 = t2 * t;
        var y = (2 * t3 - 3 * t2 + 1) * p[i][1] + (t3 - 2 * t2 + t) * h * m[i]
              + (-2 * t3 + 3 * t2) * p[i + 1][1] + (t3 - t2) * h * m[i + 1];
        return Math.max(0, Math.min(1, y));
    }

    // Channel buttons and reset
    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Repeater {
            model: [
                { "id": "rgb", "label": qsTr("RGB") },
                { "id": "red", "label": qsTr("Red") },
                { "id": "green", "label": qsTr("Green") },
                { "id": "blue", "label": qsTr("Blue") }
            ]
            delegate: Rectangle {
                required property var modelData
                readonly property bool active: root.channel === modelData.id
                Layout.fillWidth: true
                implicitHeight: Theme.px(10) + 12
                radius: Theme.radiusSm
                color: active ? Theme.bgCardHover : Theme.bgCard
                border.color: active ? root.channelColors[modelData.id] : Theme.border
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    width: parent.width - 6
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: modelData.label
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    font.weight: parent.active ? Font.Bold : Font.Normal
                    color: parent.active ? root.channelColors[modelData.id] : Theme.textMuted
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: { root.channel = modelData.id; canvas.requestPaint(); }
                }
            }
        }
    }

    Item {
        id: plot
        Layout.fillWidth: true
        Layout.preferredHeight: width

        Rectangle {
            anchors.fill: parent
            color: Theme.bgCard
            border.color: Theme.border
            border.width: 1
            radius: Theme.radiusSm
        }

        Canvas {
            id: canvas
            anchors.fill: parent
            anchors.margins: 6

            onPaint: {
                var ctx = getContext("2d");
                var w = width, h = height;
                ctx.clearRect(0, 0, w, h);

                // Histogram of the channel being edited
                var hd = root.histData;
                if (hd) {
                    var data = root.channel === "red" ? hd.red : root.channel === "green" ? hd.green : root.channel === "blue" ? hd.blue : hd.luma;
                    var maxVal = hd.max_count || 1;
                    if (data && data.length > 0) {
                        ctx.beginPath();
                        ctx.moveTo(0, h);
                        for (var i = 0; i < data.length; ++i)
                            ctx.lineTo(i / (data.length - 1) * w, h - Math.min(1, (data[i] || 0) / maxVal) * h * 0.9);
                        ctx.lineTo(w, h);
                        ctx.closePath();
                        ctx.fillStyle = Qt.rgba(Theme.textDim.r, Theme.textDim.g, Theme.textDim.b, 0.35);
                        ctx.fill();
                    }
                }

                // Quarter grid and the straight reference line
                ctx.strokeStyle = String(Theme.border);
                ctx.lineWidth = 1;
                for (var g = 1; g < 4; ++g) {
                    ctx.beginPath(); ctx.moveTo(w * g / 4, 0); ctx.lineTo(w * g / 4, h); ctx.stroke();
                    ctx.beginPath(); ctx.moveTo(0, h * g / 4); ctx.lineTo(w, h * g / 4); ctx.stroke();
                }
                ctx.beginPath(); ctx.moveTo(0, h); ctx.lineTo(w, 0); ctx.stroke();

                // Other channels' curves, faint, so their combined effect stays visible
                var chans = ["rgb", "red", "green", "blue"];
                for (var c = 0; c < chans.length; ++c) {
                    var ch = chans[c];
                    var pts = root.pointsFor(ch);
                    if (ch === root.channel || root.isStraight(pts)) continue;
                    drawCurve(ctx, pts, w, h, root.channelColors[ch], 0.35, 1);
                }

                var p = root.pointsFor(root.channel);
                var col = root.channelColors[root.channel];
                drawCurve(ctx, p, w, h, col, 1.0, 2);
                ctx.fillStyle = String(col);
                for (var k = 0; k < p.length; ++k) {
                    var px = p[k][0] * w, py = (1 - p[k][1]) * h;
                    ctx.fillRect(px - 4, py - 4, 8, 8);
                    if (k === mouse.dragIndex) {
                        ctx.strokeStyle = String(Theme.textMain);
                        ctx.strokeRect(px - 6, py - 6, 12, 12);
                    }
                }
            }

            function drawCurve(ctx, p, w, h, col, alpha, lineWidth) {
                var m = root.tangents(p);
                ctx.strokeStyle = Qt.rgba(Qt.color(col).r, Qt.color(col).g, Qt.color(col).b, alpha);
                ctx.lineWidth = lineWidth;
                ctx.beginPath();
                for (var s = 0; s <= 128; ++s) {
                    var x = s / 128;
                    var y = root.valueAt(p, m, x);
                    if (s === 0) ctx.moveTo(x * w, (1 - y) * h);
                    else ctx.lineTo(x * w, (1 - y) * h);
                }
                ctx.stroke();
            }
        }

        MouseArea {
            id: mouse
            anchors.fill: canvas
            hoverEnabled: true
            property int dragIndex: -1
            property var working: []

            function toValue(mx, my) {
                return [Math.max(0, Math.min(1, mx / width)), Math.max(0, Math.min(1, 1 - my / height))];
            }
            function hit(p, mx, my) {
                var r = 9;
                for (var i = 0; i < p.length; ++i)
                    if (Math.abs(p[i][0] * width - mx) <= r && Math.abs((1 - p[i][1]) * height - my) <= r) return i;
                return -1;
            }

            cursorShape: dragIndex >= 0 ? Qt.ClosedHandCursor : Qt.CrossCursor

            onPressed: function(ev) {
                var p = root.pointsFor(root.channel).map(function(q) { return [q[0], q[1]]; });
                var i = hit(p, ev.x, ev.y);
                if (i < 0) {
                    // New point on the curve under the cursor's x
                    var v = toValue(ev.x, ev.y);
                    var at = 0;
                    while (at < p.length && p[at][0] < v[0]) ++at;
                    if ((at > 0 && v[0] - p[at - 1][0] < 0.02) || (at < p.length && p[at][0] - v[0] < 0.02)) return;
                    p.splice(at, 0, v);
                    i = at;
                }
                working = p;
                dragIndex = i;
                root.publish(root.channel, working);
                canvas.requestPaint();
            }
            onPositionChanged: function(ev) {
                if (!pressed || dragIndex < 0) return;
                var p = working.slice();
                var v = toValue(ev.x, ev.y);
                // A point stays between its neighbours
                var lo = dragIndex > 0 ? p[dragIndex - 1][0] + 0.01 : 0;
                var hi = dragIndex < p.length - 1 ? p[dragIndex + 1][0] - 0.01 : 1;
                p[dragIndex] = [Math.max(lo, Math.min(hi, v[0])), v[1]];
                working = p;
                root.publish(root.channel, working);
                canvas.requestPaint();
            }
            onReleased: { dragIndex = -1; canvas.requestPaint(); }
            onDoubleClicked: function(ev) {
                var p = root.pointsFor(root.channel).map(function(q) { return [q[0], q[1]]; });
                var i = hit(p, ev.x, ev.y);
                // End points can be moved but not removed
                if (i > 0 && i < p.length - 1) {
                    p.splice(i, 1);
                    root.publish(root.channel, p);
                    canvas.requestPaint();
                }
            }
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 6

        Text {
            Layout.fillWidth: true
            text: qsTr("Click to add a point, drag to move it, double-click to remove it.")
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            color: Theme.textDim
        }

        Rectangle {
            id: resetChannel
            enabled: !root.isStraight(root.pointsFor(root.channel))
            opacity: enabled ? 1.0 : 0.45
            implicitWidth: resetLabel.implicitWidth + 16
            implicitHeight: Theme.px(10) + 14
            radius: Theme.radiusSm
            color: resetMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
            border.color: Theme.border

            Text {
                id: resetLabel
                anchors.centerIn: parent
                text: qsTr("Reset channel")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(10)
                color: Theme.textMain
            }
            MouseArea {
                id: resetMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: { root.publish(root.channel, []); canvas.requestPaint(); }
            }
        }
    }

    onPointsRgbChanged: canvas.requestPaint()
    onPointsRedChanged: canvas.requestPaint()
    onPointsGreenChanged: canvas.requestPaint()
    onPointsBlueChanged: canvas.requestPaint()
    onHistDataChanged: canvas.requestPaint()
}
