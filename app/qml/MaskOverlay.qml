import QtQuick

// Mask editing on the photo: brush painting, colour picking and the handles of
// linear and radial masks. Positions are normalised to the frame (0..1 across its
// width and height), the same coordinates the engine uses for layer masks.
Item {
    id: root

    // "", "brush", "pick", "linear" or "radial"
    property string tool: ""
    // The mask shape being edited, as stored in the recipe
    property var shape: null
    // Brush radius in frame widths
    property real brushSize: 0.05
    property real brushFeather: 0.5
    property bool brushErase: false
    // Preview image, sampled by the colour picker
    property string imageSource: ""

    signal shapeEdited(var shape)
    signal strokePainted(var points)
    signal colourPicked(real r, real g, real b)

    visible: tool !== "" && shape !== null

    property var livePoints: []
    property point hoverPos: Qt.point(-1, -1)

    function clamp01(x) { return Math.max(0, Math.min(1, x)); }
    // A numeric field of the shape, 0 while another kind of shape is shown
    function field(key) { return root.shape && typeof root.shape[key] === "number" ? root.shape[key] : 0; }
    function copyShape() { return JSON.parse(JSON.stringify(root.shape)); }

    onShapeChanged: canvas.requestPaint()
    onToolChanged: {
        canvas.requestPaint();
        if (tool === "pick") prepareSampler();
    }
    onWidthChanged: canvas.requestPaint()
    onHeightChanged: canvas.requestPaint()

    // Painted area, drawn opaque and shown through the item's opacity so
    // overlapping strokes do not build up
    Canvas {
        id: canvas
        anchors.fill: parent
        opacity: 0.4
        visible: root.tool === "brush"

        function drawStroke(ctx, pts, radius, erase) {
            if (!pts || pts.length === 0) return;
            ctx.globalCompositeOperation = erase ? "destination-out" : "source-over";
            ctx.fillStyle = "#ff3c3c";
            ctx.strokeStyle = "#ff3c3c";
            if (pts.length === 1) {
                ctx.beginPath();
                ctx.arc(pts[0][0] * width, pts[0][1] * height, radius, 0, 2 * Math.PI);
                ctx.fill();
                return;
            }
            ctx.lineWidth = 2 * radius;
            ctx.lineCap = "round";
            ctx.lineJoin = "round";
            ctx.beginPath();
            ctx.moveTo(pts[0][0] * width, pts[0][1] * height);
            for (var i = 1; i < pts.length; ++i)
                ctx.lineTo(pts[i][0] * width, pts[i][1] * height);
            ctx.stroke();
        }

        onPaint: {
            var ctx = getContext("2d");
            ctx.globalCompositeOperation = "source-over";
            ctx.clearRect(0, 0, width, height);
            var s = root.shape;
            if (!s || s.type !== "brush") return;
            var strokes = s.strokes || [];
            for (var i = 0; i < strokes.length; ++i)
                drawStroke(ctx, strokes[i].points, (strokes[i].size || 0.05) * width, !!strokes[i].erase);
            drawStroke(ctx, root.livePoints, root.brushSize * width, root.brushErase);
            ctx.globalCompositeOperation = "source-over";
        }
    }

    // Brush outline under the pointer
    Rectangle {
        visible: root.tool === "brush" && root.hoverPos.x >= 0
        width: root.brushSize * root.width * 2
        height: width
        radius: width / 2
        x: root.hoverPos.x - width / 2
        y: root.hoverPos.y - height / 2
        color: "transparent"
        border.color: root.brushErase ? "#ffffff" : "#ff3c3c"
        border.width: 1.5
        Rectangle {
            anchors.centerIn: parent
            width: parent.width * (1 - root.brushFeather)
            height: width
            radius: width / 2
            color: "transparent"
            border.color: parent.border.color
            border.width: 1
            opacity: 0.5
        }
    }

    // Hidden copy of the preview for the colour picker, scaled down
    Canvas {
        id: sampler
        visible: false
        width: 256
        height: Math.max(1, Math.round(256 * root.height / Math.max(1, root.width)))
        property string loaded: ""
        // A click that came before the image was ready
        property var pending: null
        onImageLoaded: requestPaint()
        onPaint: {
            if (!isImageLoaded(loaded)) return;
            var ctx = getContext("2d");
            ctx.drawImage(loaded, 0, 0, width, height);
            if (pending) {
                var p = pending;
                pending = null;
                root.pickAt(p[0], p[1]);
            }
        }
        function sample(u, v) {
            if (!isImageLoaded(loaded)) return null;
            var ctx = getContext("2d");
            var x = Math.min(width - 2, Math.max(1, Math.round(u * width)));
            var y = Math.min(height - 2, Math.max(1, Math.round(v * height)));
            // 3x3 average so one noisy pixel does not decide the colour
            var d = ctx.getImageData(x - 1, y - 1, 3, 3).data;
            var r = 0, g = 0, b = 0;
            for (var i = 0; i < 9; ++i) { r += d[i * 4]; g += d[i * 4 + 1]; b += d[i * 4 + 2]; }
            return [r / 9 / 255, g / 9 / 255, b / 9 / 255];
        }
    }

    function prepareSampler() {
        if (root.imageSource === "" || sampler.loaded === root.imageSource) return;
        if (sampler.loaded !== "") sampler.unloadImage(sampler.loaded);
        sampler.loaded = root.imageSource;
        sampler.loadImage(root.imageSource);
    }
    onImageSourceChanged: if (tool === "pick") prepareSampler()

    function pickAt(u, v) {
        root.prepareSampler();
        var c = sampler.sample(u, v);
        if (c) root.colourPicked(c[0], c[1], c[2]);
        else sampler.pending = [u, v];
    }

    // Painting and picking
    MouseArea {
        anchors.fill: parent
        enabled: root.tool === "brush" || root.tool === "pick"
        hoverEnabled: true
        preventStealing: true
        cursorShape: root.tool === "pick" ? Qt.CrossCursor : Qt.BlankCursor

        onEntered: if (root.tool === "pick") root.prepareSampler()
        onExited: root.hoverPos = Qt.point(-1, -1)
        onPositionChanged: function(ev) {
            root.hoverPos = Qt.point(ev.x, ev.y);
            if (root.tool !== "brush" || !pressed) return;
            var p = [root.clamp01(ev.x / width), root.clamp01(ev.y / height)];
            var pts = root.livePoints;
            var last = pts[pts.length - 1];
            // A point every quarter radius keeps the stroke smooth and the recipe small
            var dx = (p[0] - last[0]) * width, dy = (p[1] - last[1]) * height;
            if (Math.sqrt(dx * dx + dy * dy) < root.brushSize * width * 0.25) return;
            pts = pts.slice();
            pts.push([Math.round(p[0] * 1e4) / 1e4, Math.round(p[1] * 1e4) / 1e4]);
            root.livePoints = pts;
            canvas.requestPaint();
        }
        onPressed: function(ev) {
            var u = root.clamp01(ev.x / width), v = root.clamp01(ev.y / height);
            if (root.tool === "pick") {
                root.pickAt(u, v);
                return;
            }
            root.livePoints = [[Math.round(u * 1e4) / 1e4, Math.round(v * 1e4) / 1e4]];
            canvas.requestPaint();
        }
        onReleased: {
            if (root.tool !== "brush" || root.livePoints.length === 0) return;
            var pts = root.livePoints;
            root.livePoints = [];
            root.strokePainted(pts);
        }
    }

    // Linear gradient: the line runs from no effect (start) to full effect (end)
    Item {
        anchors.fill: parent
        visible: root.tool === "linear" && root.shape !== null && root.shape.type === "linear_gradient"

        Canvas {
            id: linCanvas
            anchors.fill: parent
            property var s: root.shape
            onSChanged: requestPaint()
            onWidthChanged: requestPaint()
            onHeightChanged: requestPaint()
            onPaint: {
                var ctx = getContext("2d");
                ctx.clearRect(0, 0, width, height);
                if (!s || s.type !== "linear_gradient") return;
                var x0 = s.start_x * width, y0 = s.start_y * height, x1 = s.end_x * width, y1 = s.end_y * height;
                var dx = x1 - x0, dy = y1 - y0, len = Math.sqrt(dx * dx + dy * dy) || 1;
                // Perpendicular guides across the frame at both ends
                var px = -dy / len * (width + height), py = dx / len * (width + height);
                ctx.lineWidth = 1;
                ctx.strokeStyle = "rgba(255,255,255,0.85)";
                ctx.beginPath(); ctx.moveTo(x1 - px, y1 - py); ctx.lineTo(x1 + px, y1 + py); ctx.stroke();
                ctx.setLineDash([4, 4]);
                ctx.beginPath(); ctx.moveTo(x0 - px, y0 - py); ctx.lineTo(x0 + px, y0 + py); ctx.stroke();
                ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x1, y1); ctx.stroke();
                ctx.setLineDash([]);
            }
        }

        Handle { u: root.field("start_x"); v: root.field("start_y"); keyX: "start_x"; keyY: "start_y"; hollow: true }
        Handle { u: root.field("end_x"); v: root.field("end_y"); keyX: "end_x"; keyY: "end_y" }
    }

    // Radial gradient: full effect inside the inner ellipse, none outside the outer one
    Item {
        anchors.fill: parent
        visible: root.tool === "radial" && root.shape !== null && root.shape.type === "radial_gradient"

        Canvas {
            anchors.fill: parent
            property var s: root.shape
            onSChanged: requestPaint()
            onWidthChanged: requestPaint()
            onHeightChanged: requestPaint()
            function ellipse(ctx, cx, cy, rx, ry) {
                ctx.beginPath();
                for (var i = 0; i <= 64; ++i) {
                    var a = i / 64 * 2 * Math.PI;
                    var x = cx + Math.cos(a) * rx, y = cy + Math.sin(a) * ry;
                    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
                }
                ctx.stroke();
            }
            onPaint: {
                var ctx = getContext("2d");
                ctx.clearRect(0, 0, width, height);
                if (!s || s.type !== "radial_gradient") return;
                var cx = s.center_x * width, cy = s.center_y * height;
                var rx = s.radius_x * width, ry = s.radius_y * height;
                var inner = 1 - Math.max(0.01, Math.min(1, s.feather));
                ctx.lineWidth = 1;
                ctx.strokeStyle = "rgba(255,255,255,0.85)";
                ellipse(ctx, cx, cy, rx, ry);
                ctx.setLineDash([4, 4]);
                ellipse(ctx, cx, cy, rx * inner, ry * inner);
                ctx.setLineDash([]);
            }
        }

        Handle { u: root.field("center_x"); v: root.field("center_y"); keyX: "center_x"; keyY: "center_y" }
        Handle {
            u: root.field("center_x") + root.field("radius_x")
            v: root.field("center_y")
            keyX: "radius_x"; hollow: true
        }
        Handle {
            u: root.field("center_x")
            v: root.field("center_y") + root.field("radius_y")
            keyY: "radius_y"; hollow: true
        }
    }

    // A draggable point that sets the shape fields keyX and keyY. The radius handles
    // set their distance from the centre, so moving the centre carries the ellipse.
    component Handle: Rectangle {
        id: h
        property real u: 0
        property real v: 0
        property string keyX: ""
        property string keyY: ""
        property bool hollow: false
        width: 14
        height: 14
        radius: 7
        x: u * root.width - width / 2
        y: v * root.height - height / 2
        color: hollow ? "transparent" : "#ffffff"
        border.color: hollow ? "#ffffff" : "#000000"
        border.width: hollow ? 2 : 1

        MouseArea {
            anchors.fill: parent
            anchors.margins: -6
            preventStealing: true
            cursorShape: Qt.SizeAllCursor
            onPositionChanged: function(ev) {
                if (!pressed || !root.shape) return;
                var p = mapToItem(root, ev.x, ev.y);
                var u = root.clamp01(p.x / root.width), v = root.clamp01(p.y / root.height);
                var s = root.copyShape();
                if (h.keyX === "radius_x") s.radius_x = Math.max(0.01, Math.abs(u - s.center_x));
                else if (h.keyX !== "") s[h.keyX] = u;
                if (h.keyY === "radius_y") s.radius_y = Math.max(0.01, Math.abs(v - s.center_y));
                else if (h.keyY !== "") s[h.keyY] = v;
                root.shapeEdited(s);
            }
        }
    }
}
