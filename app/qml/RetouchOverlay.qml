import QtQuick

// Heal and clone spots on the photo. A click on the photo adds a spot there, with
// its source beside it; dragging a spot's solid circle moves the spot, dragging its
// dashed circle moves the source. Positions are normalised to the frame and the
// radius is a share of the frame width, as in the recipe.
Item {
    id: root

    property bool active: false
    property var spots: []
    property int selected: -1
    // Settings for a new spot
    property string mode: "heal"
    property real radius: 0.02
    property real feather: 0.3
    property real opacity_: 1.0

    signal spotsEdited(var spots)
    signal spotSelected(int index)

    visible: active

    property point hoverPos: Qt.point(-1, -1)

    function clamp01(x) { return Math.max(0, Math.min(1, x)); }

    // A source beside the spot, inside the frame: to the right, else to the left,
    // else below or above
    function defaultSource(u, v, r) {
        var du = r * 2.5, dv = r * 2.5 * width / Math.max(1, height);
        var tries = [[u + du, v], [u - du, v], [u, v + dv], [u, v - dv]];
        for (var i = 0; i < tries.length; ++i) {
            var t = tries[i];
            if (t[0] - r >= 0 && t[0] + r <= 1 && t[1] >= 0 && t[1] <= 1) return t;
        }
        return [clamp01(u + du), v];
    }

    // Which circle is under (x, y): [index, "target" | "source"], or null
    function hit(x, y) {
        var order = [];
        if (selected >= 0 && selected < spots.length) order.push(selected);
        for (var i = spots.length - 1; i >= 0; --i) if (i !== selected) order.push(i);
        for (var k = 0; k < order.length; ++k) {
            var s = spots[order[k]];
            var r = Math.max(6, s.radius * width);
            if (Math.hypot(x - s.x * width, y - s.y * height) <= r) return [order[k], "target"];
            if (Math.hypot(x - s.source_x * width, y - s.source_y * height) <= r) return [order[k], "source"];
        }
        return null;
    }

    onSpotsChanged: canvas.requestPaint()
    onSelectedChanged: canvas.requestPaint()
    onWidthChanged: canvas.requestPaint()
    onHeightChanged: canvas.requestPaint()

    Canvas {
        id: canvas
        anchors.fill: parent

        function circle(ctx, x, y, r) {
            ctx.beginPath();
            ctx.arc(x, y, r, 0, 2 * Math.PI);
            ctx.stroke();
        }

        onPaint: {
            var ctx = getContext("2d");
            ctx.clearRect(0, 0, width, height);
            for (var i = 0; i < root.spots.length; ++i) {
                var s = root.spots[i];
                var sel = i === root.selected;
                var r = s.radius * width;
                var tx = s.x * width, ty = s.y * height, sx = s.source_x * width, sy = s.source_y * height;
                // Dark outline under the white one, so the circles show on light and dark areas
                for (var pass = 0; pass < 2; ++pass) {
                    ctx.strokeStyle = pass === 0 ? "rgba(0,0,0,0.6)" : (sel ? "#ffffff" : "rgba(255,255,255,0.7)");
                    ctx.lineWidth = pass === 0 ? (sel ? 4 : 3) : (sel ? 2 : 1);
                    ctx.setLineDash([]);
                    circle(ctx, tx, ty, r);
                    if (sel || root.spots.length < 40) {
                        ctx.setLineDash([4, 4]);
                        circle(ctx, sx, sy, r);
                        // Arrow from the source to the spot
                        var len = Math.hypot(tx - sx, ty - sy);
                        if (len > 2 * r) {
                            var ux = (tx - sx) / len, uy = (ty - sy) / len;
                            ctx.setLineDash([]);
                            ctx.beginPath();
                            ctx.moveTo(sx + ux * r, sy + uy * r);
                            ctx.lineTo(tx - ux * r, ty - uy * r);
                            ctx.stroke();
                        }
                    }
                }
            }
            ctx.setLineDash([]);
        }
    }

    // Size of a new spot under the pointer
    Rectangle {
        visible: root.hoverPos.x >= 0
        width: root.radius * root.width * 2
        height: width
        radius: width / 2
        x: root.hoverPos.x - width / 2
        y: root.hoverPos.y - height / 2
        color: "transparent"
        border.color: "#ffffff"
        border.width: 1
        opacity: 0.6
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        preventStealing: true
        property var dragging: null
        property point grabOffset: Qt.point(0, 0)

        cursorShape: dragging ? Qt.ClosedHandCursor : Qt.CrossCursor

        onExited: root.hoverPos = Qt.point(-1, -1)
        onPressed: function(ev) {
            var h = root.hit(ev.x, ev.y);
            if (h) {
                var s = root.spots[h[0]];
                var cx = (h[1] === "target" ? s.x : s.source_x) * width;
                var cy = (h[1] === "target" ? s.y : s.source_y) * height;
                grabOffset = Qt.point(ev.x - cx, ev.y - cy);
                dragging = h;
                root.spotSelected(h[0]);
                return;
            }
            var u = root.clamp01(ev.x / width), v = root.clamp01(ev.y / height);
            var src = root.defaultSource(u, v, root.radius);
            var list = JSON.parse(JSON.stringify(root.spots));
            list.push({ "mode": root.mode, "x": u, "y": v, "source_x": src[0], "source_y": src[1],
                        "radius": root.radius, "feather": root.feather, "opacity": root.opacity_ });
            root.spotsEdited(list);
            root.spotSelected(list.length - 1);
        }
        onPositionChanged: function(ev) {
            root.hoverPos = dragging || root.hit(ev.x, ev.y) ? Qt.point(-1, -1) : Qt.point(ev.x, ev.y);
            if (!pressed || !dragging) return;
            var list = JSON.parse(JSON.stringify(root.spots));
            var s = list[dragging[0]];
            if (!s) return;
            var u = root.clamp01((ev.x - grabOffset.x) / width), v = root.clamp01((ev.y - grabOffset.y) / height);
            if (dragging[1] === "target") { s.x = u; s.y = v; }
            else { s.source_x = u; s.source_y = v; }
            root.spotsEdited(list);
        }
        onReleased: dragging = null
    }
}
