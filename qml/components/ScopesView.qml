import QtQuick
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    implicitWidth: 260
    implicitHeight: 165
    radius: Theme.radiusMd
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property var histData: null
    property bool showClippingHighlights: false
    property bool showClippingShadows: false
    property int activeScope: 0 // 0: Histogram, 1: Waveform, 2: Parade, 3: Vectorscope

    signal toggleHighlightMask()
    signal toggleShadowMask()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 6

        // Top Control Bar: Mode Tabs + Clipping Toggles
        RowLayout {
            Layout.fillWidth: true
            spacing: 4

            // Scope mode tabs
            RowLayout {
                spacing: 2
                property var modes: [
                    { name: "HIST", idx: 0 },
                    { name: "WAVE", idx: 1 },
                    { name: "PARADE", idx: 2 },
                    { name: "VECTOR", idx: 3 }
                ]

                Repeater {
                    model: parent.modes
                    delegate: Rectangle {
                        implicitWidth: modeText.implicitWidth + 8
                        implicitHeight: 18
                        radius: 3
                        property bool isCur: root.activeScope === modelData.idx
                        color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
                        border.color: isCur ? Theme.accent : Theme.border
                        border.width: 1

                        Text {
                            id: modeText
                            anchors.centerIn: parent
                            text: modelData.name
                            textFormat: Text.PlainText
                            font.pixelSize: 8
                            font.weight: Font.Bold
                            font.family: Theme.monoFont
                            color: parent.isCur ? Theme.accent : Theme.textDim
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.activeScope = modelData.idx;
                                scopeCanvas.requestPaint();
                            }
                        }
                    }
                }
            }

            Item { Layout.fillWidth: true }

            // Shadow clipping button
            Rectangle {
                implicitWidth: shadowText.implicitWidth + 8
                implicitHeight: 18
                radius: 3
                color: root.showClippingShadows ? Theme.shadowClip : Qt.rgba(0.2, 0.5, 1.0, 0.15)
                border.color: Theme.shadowClip

                Text {
                    id: shadowText
                    anchors.centerIn: parent
                    text: "[S] " + (root.histData ? root.histData.shadow_clipping_percent.toFixed(1) + "%" : "0.0%")
                    textFormat: Text.PlainText
                    font.pixelSize: 8
                    font.family: Theme.monoFont
                    color: root.showClippingShadows ? "#ffffff" : Theme.shadowClip
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        root.showClippingShadows = !root.showClippingShadows;
                        root.toggleShadowMask();
                    }
                }
            }

            // Highlight clipping button
            Rectangle {
                implicitWidth: highText.implicitWidth + 8
                implicitHeight: 18
                radius: 3
                color: root.showClippingHighlights ? Theme.highlightClip : Qt.rgba(1.0, 0.3, 0.4, 0.15)
                border.color: Theme.highlightClip

                Text {
                    id: highText
                    anchors.centerIn: parent
                    text: "[H] " + (root.histData ? root.histData.highlight_clipping_percent.toFixed(1) + "%" : "0.0%")
                    textFormat: Text.PlainText
                    font.pixelSize: 8
                    font.family: Theme.monoFont
                    color: root.showClippingHighlights ? "#ffffff" : Theme.highlightClip
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        root.showClippingHighlights = !root.showClippingHighlights;
                        root.toggleHighlightMask();
                    }
                }
            }
        }

        // Scope Display Canvas
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: Theme.radiusSm
            color: "#0d0e14"
            border.color: Theme.border
            border.width: 1
            clip: true

            Canvas {
                id: scopeCanvas
                anchors.fill: parent
                anchors.margins: 4
                antialiasing: true

                onPaint: {
                    var ctx = getContext("2d");
                    ctx.clearRect(0, 0, width, height);

                    if (!root.histData) {
                        ctx.fillStyle = "#545c7e";
                        ctx.font = "9px " + Theme.monoFont;
                        ctx.textAlign = "center";
                        ctx.textBaseline = "middle";
                        ctx.fillText("NO SIGNAL / LOAD RAW", width / 2, height / 2);
                        return;
                    }

                    if (root.activeScope === 0) {
                        drawHistogram(ctx, width, height);
                    } else if (root.activeScope === 1) {
                        drawWaveform(ctx, width, height);
                    } else if (root.activeScope === 2) {
                        drawParade(ctx, width, height);
                    } else if (root.activeScope === 3) {
                        drawVectorscope(ctx, width, height);
                    }
                }

                // 1. Classic RGB + Luma Histogram
                function drawHistogram(ctx, w, h) {
                    var maxVal = root.histData.max_count || 1;
                    var red = root.histData.red || [];
                    var green = root.histData.green || [];
                    var blue = root.histData.blue || [];
                    var luma = root.histData.luma || [];

                    // Grid lines (25%, 50%, 75%)
                    ctx.strokeStyle = "#1f2335";
                    ctx.lineWidth = 1;
                    for (var g = 1; g < 4; g++) {
                        var gx = (w * g) / 4;
                        ctx.beginPath();
                        ctx.moveTo(gx, 0);
                        ctx.lineTo(gx, h);
                        ctx.stroke();
                    }

                    function drawCurve(data, strokeCol, fillCol) {
                        if (!data || data.length === 0) return;
                        ctx.beginPath();
                        ctx.moveTo(0, h);
                        for (var i = 0; i < 256; i++) {
                            var x = (i / 255.0) * w;
                            var v = data[i] || 0;
                            var y = h - (v / maxVal) * (h - 6);
                            ctx.lineTo(x, y);
                        }
                        ctx.lineTo(w, h);
                        ctx.closePath();
                        if (fillCol) {
                            ctx.fillStyle = fillCol;
                            ctx.fill();
                        }
                        ctx.strokeStyle = strokeCol;
                        ctx.lineWidth = 1.2;
                        ctx.stroke();
                    }

                    ctx.globalCompositeOperation = "screen";
                    drawCurve(red, "#f7768e", "rgba(247, 118, 142, 0.12)");
                    drawCurve(green, "#9ece6a", "rgba(158, 206, 106, 0.12)");
                    drawCurve(blue, "#7aa2f7", "rgba(122, 162, 247, 0.12)");
                    drawCurve(luma, "rgba(255, 255, 255, 0.4)", null);
                    ctx.globalCompositeOperation = "source-over";
                }

                // 2. DaVinci Luma Waveform (0 - 100 IRE)
                function drawWaveform(ctx, w, h) {
                    var wf = root.histData.waveform_luma || [];
                    if (wf.length < 64 * 32) return;

                    // Draw IRE grid lines (0, 20, 40, 60, 80, 100 IRE)
                    ctx.strokeStyle = "#1b1d28";
                    ctx.lineWidth = 1;
                    ctx.fillStyle = "#414868";
                    ctx.font = "7px " + Theme.monoFont;
                    ctx.textAlign = "left";

                    var ireLabels = [100, 80, 60, 40, 20, 0];
                    for (var k = 0; k < ireLabels.length; k++) {
                        var gy = (k / 5.0) * (h - 10) + 5;
                        ctx.beginPath();
                        ctx.moveTo(0, gy);
                        ctx.lineTo(w, gy);
                        ctx.stroke();
                        ctx.fillText(ireLabels[k] + "%", 2, gy - 2);
                    }

                    // Plot waveform column bins
                    var colW = w / 64.0;
                    var rowH = (h - 10) / 32.0;

                    for (var r = 0; r < 32; r++) {
                        var py = h - 5 - (r + 1) * rowH;
                        for (var c = 0; c < 64; c++) {
                            var intensity = wf[r * 64 + c];
                            if (intensity > 2) {
                                var alpha = intensity / 255.0;
                                ctx.fillStyle = "rgba(125, 207, 255, " + (alpha * 0.85).toFixed(3) + ")";
                                ctx.fillRect(c * colW, py, colW, rowH + 0.5);
                            }
                        }
                    }
                }

                // 3. DaVinci RGB Parade (Red, Green, Blue side-by-side)
                function drawParade(ctx, w, h) {
                    var pr = root.histData.parade_r || [];
                    var pg = root.histData.parade_g || [];
                    var pb = root.histData.parade_b || [];
                    if (pr.length < 32 * 32) return;

                    var channelW = w / 3.0;
                    var colW = channelW / 32.0;
                    var rowH = (h - 10) / 32.0;

                    // Channel dividers and labels
                    ctx.strokeStyle = "#2f354a";
                    ctx.lineWidth = 1;
                    ctx.beginPath();
                    ctx.moveTo(channelW, 0); ctx.lineTo(channelW, h);
                    ctx.moveTo(channelW * 2, 0); ctx.lineTo(channelW * 2, h);
                    ctx.stroke();

                    ctx.font = "8px " + Theme.monoFont;
                    ctx.textAlign = "center";
                    ctx.fillStyle = "#f7768e"; ctx.fillText("RED", channelW * 0.5, 9);
                    ctx.fillStyle = "#9ece6a"; ctx.fillText("GREEN", channelW * 1.5, 9);
                    ctx.fillStyle = "#7aa2f7"; ctx.fillText("BLUE", channelW * 2.5, 9);

                    function renderChannelData(data, offsetX, baseCol) {
                        for (var r = 0; r < 32; r++) {
                            var py = h - 3 - (r + 1) * rowH;
                            for (var c = 0; c < 32; c++) {
                                var val = data[r * 32 + c];
                                if (val > 2) {
                                    var alpha = (val / 255.0) * 0.8;
                                    ctx.fillStyle = baseCol.replace("ALPHA", alpha.toFixed(3));
                                    ctx.fillRect(offsetX + c * colW, py, colW, rowH + 0.5);
                                }
                            }
                        }
                    }

                    renderChannelData(pr, 0, "rgba(247, 118, 142, ALPHA)");
                    renderChannelData(pg, channelW, "rgba(158, 206, 106, ALPHA)");
                    renderChannelData(pb, channelW * 2, "rgba(122, 162, 247, ALPHA)");
                }

                // 4. DaVinci Vectorscope with Skin Tone Line (I-Bar)
                function drawVectorscope(ctx, w, h) {
                    var vec = root.histData.vectorscope || [];
                    if (vec.length < 48 * 48) return;

                    var cx = w / 2;
                    var cy = h / 2;
                    var maxRadius = Math.min(cx, cy) - 8;

                    // Draw outer reticle and target circles
                    ctx.strokeStyle = "#222538";
                    ctx.lineWidth = 1;
                    ctx.beginPath();
                    ctx.arc(cx, cy, maxRadius, 0, 2 * Math.PI);
                    ctx.arc(cx, cy, maxRadius * 0.75, 0, 2 * Math.PI);
                    ctx.arc(cx, cy, maxRadius * 0.35, 0, 2 * Math.PI);
                    ctx.stroke();

                    // Crosshairs
                    ctx.beginPath();
                    ctx.moveTo(cx - maxRadius, cy); ctx.lineTo(cx + maxRadius, cy);
                    ctx.moveTo(cx, cy - maxRadius); ctx.lineTo(cx, cy + maxRadius);
                    ctx.stroke();

                    // DaVinci Skin Tone Indicator Line (I-Bar at ~123 degrees / -57 degrees)
                    // The standard vector across all human ethnicities
                    ctx.strokeStyle = "rgba(255, 180, 100, 0.75)";
                    ctx.lineWidth = 1.5;
                    ctx.beginPath();
                    ctx.moveTo(cx, cy);
                    var skinAngle = -57.0 * (Math.PI / 180.0);
                    ctx.lineTo(cx + Math.cos(skinAngle) * maxRadius, cy + Math.sin(skinAngle) * maxRadius);
                    ctx.stroke();

                    // Target label for skin line
                    ctx.fillStyle = "rgba(255, 180, 100, 0.85)";
                    ctx.font = "7px " + Theme.monoFont;
                    ctx.textAlign = "left";
                    ctx.fillText("SKIN", cx + Math.cos(skinAngle) * (maxRadius - 14), cy + Math.sin(skinAngle) * (maxRadius - 14));

                    // Standard 75% broadcast color targets (R, Mg, B, Cy, G, Yl)
                    var targets = [
                        { name: "R", angle: 104, col: "#f7768e" },
                        { name: "Mg", angle: 161, col: "#bb9af7" },
                        { name: "B", angle: -14, col: "#7aa2f7" },
                        { name: "Cy", angle: -76, col: "#7dcfff" },
                        { name: "G", angle: -19, col: "#9ece6a" },
                        { name: "Yl", angle: 45, col: "#e0af68" }
                    ];

                    for (var t = 0; t < targets.length; t++) {
                        var tg = targets[t];
                        var rad = tg.angle * (Math.PI / 180.0);
                        var tx = cx + Math.cos(rad) * (maxRadius * 0.75);
                        var ty = cy - Math.sin(rad) * (maxRadius * 0.75);

                        ctx.strokeStyle = tg.col;
                        ctx.strokeRect(tx - 3, ty - 3, 6, 6);
                        ctx.fillStyle = tg.col;
                        ctx.fillText(tg.name, tx + 4, ty + 2);
                    }

                    // Plot vectorscope UV / CbCr pixel points
                    var stepU = (maxRadius * 2) / 48.0;
                    var stepV = (maxRadius * 2) / 48.0;

                    for (var v = 0; v < 48; v++) {
                        var py = cy - maxRadius + v * stepV;
                        for (var u = 0; u < 48; u++) {
                            var val = vec[v * 48 + u];
                            if (val > 4) {
                                var alpha = (val / 255.0) * 0.9;
                                ctx.fillStyle = "rgba(158, 206, 106, " + alpha.toFixed(3) + ")";
                                var px = cx - maxRadius + u * stepU;
                                ctx.fillRect(px, py, 2.5, 2.5);
                            }
                        }
                    }
                }
            }
        }
    }

    onHistDataChanged: {
        scopeCanvas.requestPaint();
    }
}
