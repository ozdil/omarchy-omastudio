import QtQuick
import QtQuick.Layouts
import QtQuick.Window
import QtQuick.Controls

Rectangle {
    id: root
    // Neutral surround so the theme's tint does not shift how colours are judged:
    // "dark" (default), "grey" (middle grey) or "theme"
    property string surround: "dark"
    signal surroundCycled(string next)
    readonly property color surroundColor: surround === "grey" ? "#5a5a5a"
        : surround === "theme" ? Theme.bgDark : "#1c1c1c"
    color: surroundColor
    clip: true

    property string imageSource: ""
    property int revisionCounter: 0
    onImageSourceChanged: {
        if (!root.imageSource || root.imageSource.length === 0) {
            imgBuffer0.source = "";
            imgBuffer1.source = "";
            mainImage.currentSource = "";
            return;
        }
        root.revisionCounter++;
        var nextSrc = "file://" + root.imageSource + "?v=" + root.revisionCounter;
        // A new preview means new adjustments: the detail view follows
        if (root.detailWanted) detailTimer.restart();

        if (imgBuffer0.status !== Image.Ready && imgBuffer1.status !== Image.Ready) {
            imgBuffer0.source = nextSrc;
        } else if (mainImage.activeBuffer === 0) {
            imgBuffer1.source = nextSrc;
        } else {
            imgBuffer0.source = nextSrc;
        }
    }

    property real zoomFactor: 1.0

    // Pixel-exact detail view. The preview is a reduced development; once the photo
    // is shown larger than the preview's own pixels, the engine renders the visible
    // part from the full-resolution frame at the screen's physical pixel size, and it
    // is drawn unscaled over the preview (1 rendered pixel = 1 screen pixel).
    property int fullWidth: 0
    property int fullHeight: 0
    readonly property real dpr: Screen.devicePixelRatio > 0 ? Screen.devicePixelRatio : 1.0
    // Screen pixels per full-resolution pixel (1.0 = 100%)
    readonly property real pixelScale: (root.fullWidth > 0 && root.fitWidth > 0) ? root.fitWidth * root.zoomFactor * root.dpr / root.fullWidth : root.zoomFactor
    readonly property bool detailWanted: root.fullWidth > 0 && root.imageSource !== ""
        && Math.abs(root.rotationAngle) < 0.01 && !root.isCropMode && !root.isSplitView
        && !root.showHighlightMask && !root.showShadowMask
        && root.fitWidth * root.zoomFactor * root.dpr > mainImage.implicitWidth * 1.05
    property var detailRegion: null          // normalised x, y, w, h of the shown detail
    property var detailRequest: null         // what the next request asks for
    property bool detailPending: false
    readonly property bool detailShown: detailImage.visible
    readonly property real previewWidth: mainImage.implicitWidth
    signal detailRequested(var region, int outWidth, int outHeight)

    // zoomFactor at which one full-resolution pixel covers `scale` screen pixels
    function zoomForPixelScale(scale) {
        if (root.fullWidth <= 0 || root.fitWidth <= 0) return scale;
        return scale * root.fullWidth / (root.fitWidth * root.dpr);
    }

    // The visible part of the photo, snapped to physical pixels
    function visibleDetail() {
        var effW = root.fitWidth * root.zoomFactor;
        var effH = root.fitHeight * root.zoomFactor;
        if (effW <= 0 || effH <= 0) return null;
        var left = root.width / 2 + root.panX - effW / 2;
        var top = root.height / 2 + root.panY - effH / 2;
        var d = root.dpr;
        var x0 = Math.ceil(Math.max(0, left) * d) / d;
        var y0 = Math.ceil(Math.max(0, top) * d) / d;
        var x1 = Math.floor(Math.min(root.width, left + effW) * d) / d;
        var y1 = Math.floor(Math.min(root.height, top + effH) * d) / d;
        if (x1 - x0 < 2 || y1 - y0 < 2) return null;
        return {
            screen: [x0, y0, x1 - x0, y1 - y0],
            region: [(x0 - left) / effW, (y0 - top) / effH, (x1 - x0) / effW, (y1 - y0) / effH],
            outWidth: Math.round((x1 - x0) * d),
            outHeight: Math.round((y1 - y0) * d)
        };
    }

    function scheduleDetail() {
        detailImage.visible = false;
        root.detailRegion = null;
        if (root.detailWanted) detailTimer.restart(); else detailTimer.stop();
    }

    function sendDetailRequest() {
        var v = root.visibleDetail();
        if (!v || !root.detailWanted) return;
        root.detailRequest = v;
        if (root.detailPending) return;   // sent again when the running one answers
        root.detailPending = true;
        root.detailRequested(v.region, v.outWidth, v.outHeight);
    }

    // Answer from the engine; shown only if the view hasn't moved since
    function showDetail(path, region) {
        root.detailPending = false;
        var want = root.detailRequest;
        if (!want || !root.detailWanted) return;
        var same = Math.abs(want.region[0] - region[0]) < 1e-3 && Math.abs(want.region[1] - region[1]) < 1e-3
            && Math.abs(want.region[2] - region[2]) < 1e-3 && Math.abs(want.region[3] - region[3]) < 1e-3;
        if (!same) {
            root.sendDetailRequest();
            return;
        }
        root.detailRegion = want;
        detailImage.source = "file://" + path + "?v=" + (++root.revisionCounter);
    }

    function detailFailed() {
        root.detailPending = false;
    }

    onZoomFactorChanged: scheduleDetail()
    onPanXChanged: scheduleDetail()
    onPanYChanged: scheduleDetail()
    onRotationAngleChanged: scheduleDetail()
    onDetailWantedChanged: scheduleDetail()
    onDprChanged: scheduleDetail()
    onFullWidthChanged: scheduleDetail()

    Timer {
        id: detailTimer
        interval: 140
        onTriggered: root.sendDetailRequest()
    }
    property real rotationAngle: 0.0
    property bool flipH: false
    property bool flipV: false
    property real panX: 0.0
    property real panY: 0.0

    property bool isSplitView: false
    property real splitRatio: 0.5
    property bool showHighlightMask: false
    property bool showShadowMask: false

    // Navigator sync properties (normalized 0.0 to 1.0)
    property real normX: 0.5
    property real normY: 0.5
    property real normW: 1.0
    property real normH: 1.0

    // Crop and Composition Guides State
    property bool isCropMode: false
    property real cropX: 0.0
    property real cropY: 0.0
    property real cropW: 1.0
    property real cropH: 1.0
    property string cropAspect: "Original"
    property int guideMode: 1 // 1: Rule of Thirds, 2: Golden Ratio, 3: Golden Spiral, 0: Off

    // Live Watermark Preview Properties
    property bool watermarkEnabled: false
    property string watermarkType: "text"
    property string watermarkText: "OmaStudio Photography"
    property string watermarkLogoPath: ""
    property int watermarkPositionIndex: 8
    property real watermarkOpacity: 0.85
    property int watermarkSize: 14
    property int watermarkMargin: 24
    property string watermarkColorHex: "#ffffff"
    property bool watermarkDropShadow: true
    property var exifMetadata: null

    signal splitRatioChangedByUser(real ratio)
    signal rotationChangedByUser(real angle)
    signal cropChangedByUser(real cx, real cy, real cw, real ch, string aspect)
    signal fileDropped(string filePath)
    signal folderDropped(string folderPath)

    function resetCrop() {
        root.cropX = 0.0;
        root.cropY = 0.0;
        root.cropW = 1.0;
        root.cropH = 1.0;
        root.cropAspect = "Original";
        root.cropChangedByUser(0.0, 0.0, 1.0, 1.0, "Original");
    }

    function applyAspectPreset(aspect) {
        root.cropAspect = aspect;
        var baseAspect = (mainImage.implicitWidth > 0 && mainImage.implicitHeight > 0)
            ? (mainImage.implicitWidth / mainImage.implicitHeight) : 1.0;

        var targetRatio = baseAspect;
        if (aspect === "1:1") targetRatio = 1.0;
        else if (aspect === "4:5") targetRatio = 4.0 / 5.0;
        else if (aspect === "16:9") targetRatio = 16.0 / 9.0;
        else if (aspect === "9:16") targetRatio = 9.0 / 16.0;
        else if (aspect === "3:2") targetRatio = 3.0 / 2.0;
        else if (aspect === "Free") {
            root.cropChangedByUser(root.cropX, root.cropY, root.cropW, root.cropH, "Free");
            return;
        }

        var newW = 1.0;
        var newH = 1.0;
        if (targetRatio > baseAspect) {
            newW = 1.0;
            newH = (baseAspect / targetRatio);
        } else {
            newH = 1.0;
            newW = (targetRatio / baseAspect);
        }
        root.cropW = Math.max(0.1, Math.min(1.0, newW));
        root.cropH = Math.max(0.1, Math.min(1.0, newH));
        root.cropX = (1.0 - root.cropW) / 2.0;
        root.cropY = (1.0 - root.cropH) / 2.0;
        root.cropChangedByUser(root.cropX, root.cropY, root.cropW, root.cropH, aspect);
    }

    // Base fitted size of image inside viewport
    readonly property real fitWidth: {
        if (mainImage.implicitWidth <= 0 || mainImage.implicitHeight <= 0 || width <= 0 || height <= 0) return width;
        var imgAspect = mainImage.implicitWidth / mainImage.implicitHeight;
        var vpAspect = width / height;
        return imgAspect > vpAspect ? width : height * imgAspect;
    }

    readonly property real fitHeight: {
        if (mainImage.implicitWidth <= 0 || mainImage.implicitHeight <= 0 || width <= 0 || height <= 0) return height;
        var imgAspect = mainImage.implicitWidth / mainImage.implicitHeight;
        var vpAspect = width / height;
        return imgAspect > vpAspect ? width / imgAspect : height;
    }

    function updateNormCoordinates() {
        var effW = fitWidth * root.zoomFactor;
        var effH = fitHeight * root.zoomFactor;
        if (effW <= 0 || effH <= 0) return;

        root.normW = Math.max(0.1, Math.min(1.0, root.width / effW));
        root.normH = Math.max(0.1, Math.min(1.0, root.height / effH));

        var nx = 0.5 - (root.panX / effW);
        var ny = 0.5 - (root.panY / effH);

        root.normX = Math.max(0.0, Math.min(1.0, nx));
        root.normY = Math.max(0.0, Math.min(1.0, ny));
    }

    function clampPan() {
        var effW = fitWidth * root.zoomFactor;
        var effH = fitHeight * root.zoomFactor;
        if (root.zoomFactor <= 1.02 && Math.abs(root.rotationAngle) < 0.5) {
            // Soft center when fit
            root.panX = 0;
            root.panY = 0;
            return;
        }
        // Keep photo safely bounded inside viewport so it never flies off-screen
        var maxPanX = Math.max(40, (effW - root.width) / 2 + 100);
        var maxPanY = Math.max(40, (effH - root.height) / 2 + 100);
        root.panX = Math.max(-maxPanX, Math.min(maxPanX, root.panX));
        root.panY = Math.max(-maxPanY, Math.min(maxPanY, root.panY));
    }

    function setNormCenter(targetX, targetY) {
        var effW = fitWidth * root.zoomFactor;
        var effH = fitHeight * root.zoomFactor;
        root.panX = (0.5 - targetX) * effW;
        root.panY = (0.5 - targetY) * effH;
        clampPan();
        updateNormCoordinates();
    }

    function setZoomAtPoint(cx, cy, targetZoom) {
        var oldZoom = root.zoomFactor;
        // Up to 8x the fit size, or 4:1 on the photo's own pixels for large files
        var newZoom = Math.max(0.2, Math.min(Math.max(8.0, root.zoomForPixelScale(4.0)), targetZoom));
        if (Math.abs(newZoom - oldZoom) < 0.0005) return;

        var curCenterX = root.width / 2 + root.panX;
        var curCenterY = root.height / 2 + root.panY;
        var dx = cx - curCenterX;
        var dy = cy - curCenterY;

        root.zoomFactor = newZoom;
        root.panX += dx * (1.0 - newZoom / oldZoom);
        root.panY += dy * (1.0 - newZoom / oldZoom);
        clampPan();
        updateNormCoordinates();
    }

    function zoomRelativeAt(cx, cy, factor) {
        setZoomAtPoint(cx, cy, root.zoomFactor * factor);
    }

    function resetTransform() {
        resetAnim.start();
    }

    function resetZoomFit() {
        resetZoomAnim.start();
    }

    function resetRotation() {
        resetRotAnim.start();
    }

    function rotateStep(deltaDegrees) {
        var cur = root.rotationAngle;
        var target = Math.round((cur + deltaDegrees) / 90.0) * 90.0;
        while (target > 180.0) target -= 360.0;
        while (target <= -180.0) target += 360.0;
        rotStepAnim.to = target;
        rotStepAnim.start();
    }

    function setZoomAbsolute(z) {
        absZoomAnim.to = z;
        absZoomAnim.start();
    }

    // Smooth Animations
    ParallelAnimation {
        id: resetAnim
        NumberAnimation { target: root; property: "zoomFactor"; to: 1.0; duration: 220; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "rotationAngle"; to: 0.0; duration: 220; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "panX"; to: 0.0; duration: 220; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "panY"; to: 0.0; duration: 220; easing.type: Easing.OutCubic }
        onFinished: {
            root.updateNormCoordinates();
            root.rotationChangedByUser(0.0);
        }
    }

    ParallelAnimation {
        id: resetZoomAnim
        NumberAnimation { target: root; property: "zoomFactor"; to: 1.0; duration: 220; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "panX"; to: 0.0; duration: 220; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "panY"; to: 0.0; duration: 220; easing.type: Easing.OutCubic }
        onFinished: root.updateNormCoordinates()
    }

    NumberAnimation {
        id: resetRotAnim
        target: root
        property: "rotationAngle"
        to: 0.0
        duration: 220
        easing.type: Easing.OutCubic
        onFinished: {
            root.updateNormCoordinates();
            root.rotationChangedByUser(0.0);
        }
    }

    NumberAnimation {
        id: rotStepAnim
        target: root
        property: "rotationAngle"
        duration: 200
        easing.type: Easing.OutCubic
        onFinished: {
            root.updateNormCoordinates();
            root.rotationChangedByUser(root.rotationAngle);
        }
    }

    ParallelAnimation {
        id: absZoomAnim
        property alias to: zoomPropAnim.to
        NumberAnimation { id: zoomPropAnim; target: root; property: "zoomFactor"; duration: 220; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "panX"; to: 0.0; duration: 220; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "panY"; to: 0.0; duration: 220; easing.type: Easing.OutCubic }
        onFinished: root.updateNormCoordinates()
    }

    // MAIN CANVAS AREA
    Item {
        id: canvasArea
        anchors.fill: parent
        clip: true

        // Solid opaque studio canvas backdrop (Guarantees zero compositor leak)
        Rectangle {
            anchors.fill: parent
            color: root.surroundColor
            z: -100
        }

        // TRANSFORM CONTAINER (Target of scale, rotation, and pan)
        Item {
            id: transformContainer
            width: root.fitWidth
            height: root.fitHeight
            x: (parent.width - root.fitWidth) / 2 + root.panX
            y: (parent.height - root.fitHeight) / 2 + root.panY
            scale: root.zoomFactor
            rotation: root.rotationAngle
            transformOrigin: Item.Center

            // 100% Solid Underlay directly under photo bounding box
            Rectangle {
                anchors.fill: parent
                color: root.surroundColor
                z: -10
            }

            // MAIN IMAGE CANVAS (Double-Buffered Zero-Flicker Architecture)
            Item {
                id: mainImage
                anchors.fill: parent

                property int activeBuffer: 0
                property string currentSource: ""
                readonly property var currentActiveImage: activeBuffer === 0 ? imgBuffer0 : imgBuffer1
                readonly property var pendingImage: activeBuffer === 0 ? imgBuffer1 : imgBuffer0
                readonly property real implicitWidth: (currentActiveImage && currentActiveImage.implicitWidth > 0)
                    ? currentActiveImage.implicitWidth
                    : ((pendingImage && pendingImage.implicitWidth > 0) ? pendingImage.implicitWidth : 1)
                readonly property real implicitHeight: (currentActiveImage && currentActiveImage.implicitHeight > 0)
                    ? currentActiveImage.implicitHeight
                    : ((pendingImage && pendingImage.implicitHeight > 0) ? pendingImage.implicitHeight : 1)

                Image {
                    id: imgBuffer0
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    smooth: true
                    mipmap: true
                    visible: opacity > 0.001
                    opacity: mainImage.activeBuffer === 0 ? 1.0 : 0.0
                    z: mainImage.activeBuffer === 0 ? 2 : 1

                    onStatusChanged: {
                        if (status === Image.Ready) {
                            if (mainImage.activeBuffer === 1 || mainImage.currentSource === "") {
                                mainImage.activeBuffer = 0;
                                mainImage.currentSource = source;
                            }
                        }
                    }
                }

                Image {
                    id: imgBuffer1
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    smooth: true
                    mipmap: true
                    visible: opacity > 0.001
                    opacity: mainImage.activeBuffer === 1 ? 1.0 : 0.0
                    z: mainImage.activeBuffer === 1 ? 2 : 1

                    onStatusChanged: {
                        if (status === Image.Ready) {
                            if (mainImage.activeBuffer === 0 || mainImage.currentSource === "") {
                                mainImage.activeBuffer = 1;
                                mainImage.currentSource = source;
                            }
                        }
                    }
                }

                // Live Watermark Preview Overlay
                WatermarkOverlay {
                    id: liveWatermarkOverlay
                    anchors.fill: parent
                    enabled: root.watermarkEnabled
                    watermarkType: root.watermarkType
                    text: root.watermarkText
                    logoPath: root.watermarkLogoPath
                    positionIndex: root.watermarkPositionIndex
                    watermarkOpacity: root.watermarkOpacity
                    size: root.watermarkSize
                    margin: root.watermarkMargin
                    colorHex: root.watermarkColorHex
                    dropShadow: root.watermarkDropShadow
                    exifMetadata: root.exifMetadata
                    z: 5
                }

                // Interactive Split Comparison Curtain
                Rectangle {
                    id: splitLine
                    visible: root.isSplitView
                    x: mainImage.width * root.splitRatio - width / 2
                    y: 0
                    width: 3
                    height: mainImage.height
                    color: "#ffffff"
                    z: 10

                    Rectangle {
                        width: 26
                        height: 26
                        radius: 13
                        anchors.centerIn: parent
                        color: "#ffffff"
                        border.color: Theme.bgBase
                        border.width: 2

                        Text {
                            font.family: Theme.fontFamily
                            anchors.centerIn: parent
                            text: "< | >"
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(8)
                            font.weight: Font.Bold
                            color: Theme.bgBase
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        anchors.margins: -12
                        cursorShape: Qt.SplitHCursor
                        drag.target: splitLine
                        drag.axis: Drag.XAxis
                        drag.minimumX: 0
                        drag.maximumX: mainImage.width

                        onPositionChanged: {
                            if (drag.active) {
                                var newRatio = (splitLine.x + splitLine.width / 2) / mainImage.width;
                                root.splitRatio = Math.max(0.05, Math.min(0.95, newRatio));
                                root.splitRatioChangedByUser(root.splitRatio);
                            }
                        }
                    }
                }

                // Interactive Crop Overlay & Composition Guides
                Item {
                    id: cropOverlayContainer
                    anchors.fill: parent
                    visible: root.isCropMode || root.cropW < 0.999 || root.cropH < 0.999

                    readonly property real curCropX: mainImage.width * root.cropX
                    readonly property real curCropY: mainImage.height * root.cropY
                    readonly property real curCropW: mainImage.width * root.cropW
                    readonly property real curCropH: mainImage.height * root.cropH

                    // Dimmed outer masks
                    Rectangle {
                        x: 0; y: 0; width: parent.width; height: cropOverlayContainer.curCropY
                        color: Qt.rgba(0, 0, 0, root.isCropMode ? 0.65 : 0.40)
                    }
                    Rectangle {
                        x: 0; y: cropOverlayContainer.curCropY + cropOverlayContainer.curCropH
                        width: parent.width; height: Math.max(0, parent.height - y)
                        color: Qt.rgba(0, 0, 0, root.isCropMode ? 0.65 : 0.40)
                    }
                    Rectangle {
                        x: 0; y: cropOverlayContainer.curCropY
                        width: cropOverlayContainer.curCropX; height: cropOverlayContainer.curCropH
                        color: Qt.rgba(0, 0, 0, root.isCropMode ? 0.65 : 0.40)
                    }
                    Rectangle {
                        x: cropOverlayContainer.curCropX + cropOverlayContainer.curCropW
                        y: cropOverlayContainer.curCropY
                        width: Math.max(0, parent.width - x); height: cropOverlayContainer.curCropH
                        color: Qt.rgba(0, 0, 0, root.isCropMode ? 0.65 : 0.40)
                    }

                    // The Crop Box
                    Rectangle {
                        id: cropBox
                        x: cropOverlayContainer.curCropX
                        y: cropOverlayContainer.curCropY
                        width: cropOverlayContainer.curCropW
                        height: cropOverlayContainer.curCropH
                        color: "transparent"
                        border.color: "#ffffff"
                        border.width: root.isCropMode ? 1.5 : 1.0

                        // Composition Guide Lines Canvas
                        Canvas {
                            id: guideCanvas
                            anchors.fill: parent
                            visible: root.isCropMode && root.guideMode > 0

                            onPaint: {
                                var ctx = getContext("2d");
                                ctx.clearRect(0, 0, width, height);
                                ctx.strokeStyle = "rgba(255, 255, 255, 0.45)";
                                ctx.lineWidth = 1;

                                if (root.guideMode === 1) {
                                    // Rule of Thirds (3x3 grid)
                                    ctx.beginPath();
                                    ctx.moveTo(width / 3, 0); ctx.lineTo(width / 3, height);
                                    ctx.moveTo((width * 2) / 3, 0); ctx.lineTo((width * 2) / 3, height);
                                    ctx.moveTo(0, height / 3); ctx.lineTo(width, height / 3);
                                    ctx.moveTo(0, (height * 2) / 3); ctx.lineTo(width, (height * 2) / 3);
                                    ctx.stroke();
                                } else if (root.guideMode === 2) {
                                    // Golden Ratio (Phi Grid: 0.382 and 0.618)
                                    var phi1 = 0.382 * width, phi2 = 0.618 * width;
                                    var phiY1 = 0.382 * height, phiY2 = 0.618 * height;
                                    ctx.beginPath();
                                    ctx.moveTo(phi1, 0); ctx.lineTo(phi1, height);
                                    ctx.moveTo(phi2, 0); ctx.lineTo(phi2, height);
                                    ctx.moveTo(0, phiY1); ctx.lineTo(width, phiY1);
                                    ctx.moveTo(0, phiY2); ctx.lineTo(width, phiY2);
                                    ctx.stroke();
                                } else if (root.guideMode === 3) {
                                    // Golden Spiral (Fibonacci)
                                    ctx.beginPath();
                                    ctx.ellipse(width * 0.618, height * 0.618, width * 0.382, height * 0.382);
                                    ctx.stroke();
                                }
                            }

                            Connections {
                                target: root
                                function onGuideModeChanged() { guideCanvas.requestPaint(); }
                                function onCropWChanged() { guideCanvas.requestPaint(); }
                                function onCropHChanged() { guideCanvas.requestPaint(); }
                            }
                        }

                        // Drag Whole Crop Box
                        MouseArea {
                            anchors.fill: parent
                            enabled: root.isCropMode
                            cursorShape: Qt.SizeAllCursor

                            property real startMouseX: 0
                            property real startMouseY: 0
                            property real startCropX: 0
                            property real startCropY: 0

                            onPressed: function(mouse) {
                                startMouseX = mouse.x;
                                startMouseY = mouse.y;
                                startCropX = root.cropX;
                                startCropY = root.cropY;
                            }

                            onPositionChanged: function(mouse) {
                                if (pressed && mainImage.width > 0 && mainImage.height > 0) {
                                    var dx = (mouse.x - startMouseX) / mainImage.width;
                                    var dy = (mouse.y - startMouseY) / mainImage.height;
                                    root.cropX = Math.max(0.0, Math.min(1.0 - root.cropW, startCropX + dx));
                                    root.cropY = Math.max(0.0, Math.min(1.0 - root.cropH, startCropY + dy));
                                    root.cropChangedByUser(root.cropX, root.cropY, root.cropW, root.cropH, root.cropAspect);
                                }
                            }
                        }

                        // Corner Handles
                        Rectangle {
                            width: 14; height: 14; color: "#ffffff"; border.color: Theme.bgBase; border.width: 1
                            x: -2; y: -2
                            visible: root.isCropMode
                            MouseArea {
                                anchors.fill: parent; anchors.margins: -4
                                cursorShape: Qt.SizeFDiagCursor
                                property real sX: 0; property real sY: 0
                                onPressed: function(m) { sX = m.x; sY = m.y; }
                                onPositionChanged: function(m) {
                                    if (pressed && mainImage.width > 0) {
                                        var deltaX = (m.x - sX) / mainImage.width;
                                        var newW = Math.max(0.1, root.cropW - deltaX);
                                        var newX = Math.max(0.0, root.cropX + (root.cropW - newW));
                                        root.cropX = newX;
                                        root.cropW = newW;
                                        root.cropChangedByUser(root.cropX, root.cropY, root.cropW, root.cropH, root.cropAspect);
                                    }
                                }
                            }
                        }
                        Rectangle {
                            width: 14; height: 14; color: "#ffffff"; border.color: Theme.bgBase; border.width: 1
                            x: parent.width - 12; y: parent.height - 12
                            visible: root.isCropMode
                            MouseArea {
                                anchors.fill: parent; anchors.margins: -4
                                cursorShape: Qt.SizeFDiagCursor
                                property real sX: 0; property real sY: 0
                                onPressed: function(m) { sX = m.x; sY = m.y; }
                                onPositionChanged: function(m) {
                                    if (pressed && mainImage.width > 0) {
                                        var deltaX = (m.x - sX) / mainImage.width;
                                        root.cropW = Math.max(0.1, Math.min(1.0 - root.cropX, root.cropW + deltaX));
                                        var deltaY = (m.y - sY) / mainImage.height;
                                        root.cropH = Math.max(0.1, Math.min(1.0 - root.cropY, root.cropH + deltaY));
                                        root.cropChangedByUser(root.cropX, root.cropY, root.cropW, root.cropH, root.cropAspect);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // TRACKPAD 2-FINGER PINCH-ZOOM & GESTURE ROTATION
        PinchHandler {
            id: pinchHandler
            target: null
            grabPermissions: PointerHandler.CanTakeOverFromItems | PointerHandler.ApprovesTakeOverByAnything

            property real startZoom: 1.0
            property real startAngle: 0.0

            onActiveChanged: {
                if (active) {
                    startZoom = root.zoomFactor;
                    startAngle = root.rotationAngle;
                } else {
                    root.clampPan();
                    root.updateNormCoordinates();
                    root.rotationChangedByUser(root.rotationAngle);
                }
            }

            onScaleChanged: {
                if (!active) return;
                var targetZoom = Math.max(0.2, Math.min(8.0, startZoom * pinchHandler.scale));
                var cx = pinchHandler.centroid.position.x;
                var cy = pinchHandler.centroid.position.y;
                root.setZoomAtPoint(cx, cy, targetZoom);
            }

            onRotationChanged: {
                if (!active) return;
                // Deadzone to prevent accidental rotation when user only intends to pinch-zoom
                if (Math.abs(pinchHandler.rotation) > 3.5) {
                    var rotOffset = pinchHandler.rotation > 0 ? (pinchHandler.rotation - 3.5) : (pinchHandler.rotation + 3.5);
                    var angle = (startAngle + rotOffset);
                    while (angle > 180.0) angle -= 360.0;
                    while (angle <= -180.0) angle += 360.0;
                    root.rotationAngle = angle;
                }
            }
        }

        // MOUSE WHEEL & TOUCHPAD ZOOM / PAN HANDLER
        WheelHandler {
            id: wheelHandler
            target: null
            grabPermissions: PointerHandler.CanTakeOverFromItems | PointerHandler.ApprovesTakeOverByAnything
            onWheel: function(event) {
                var curX = (typeof event.x !== "undefined") ? event.x
                         : (event.position ? event.position.x
                         : (wheelHandler.point ? wheelHandler.point.position.x : root.width / 2));
                var curY = (typeof event.y !== "undefined") ? event.y
                         : (event.position ? event.position.y
                         : (wheelHandler.point ? wheelHandler.point.position.y : root.height / 2));

                if (event.modifiers & (Qt.AltModifier | Qt.MetaModifier)) {
                    // Option/Alt key + wheel = smooth micro-rotation (1.5 deg per step)
                    var rotDelta = event.angleDelta.y !== 0 ? (event.angleDelta.y / 120.0) * 1.5 : (event.pixelDelta.y * 0.05);
                    var newAngle = root.rotationAngle + rotDelta;
                    while (newAngle > 180.0) newAngle -= 360.0;
                    while (newAngle <= -180.0) newAngle += 360.0;
                    root.rotationAngle = newAngle;
                    root.rotationChangedByUser(root.rotationAngle);
                } else if (event.modifiers & Qt.ShiftModifier) {
                    // Shift + wheel = horizontal pan
                    var panDelta = event.angleDelta.y !== 0 ? event.angleDelta.y : (event.pixelDelta.y !== 0 ? event.pixelDelta.y : event.pixelDelta.x);
                    root.panX += panDelta * 0.75;
                    root.clampPan();
                    root.updateNormCoordinates();
                } else {
                    // Default mouse wheel: smooth zoom in / zoom out centered at cursor position
                    var dy = event.angleDelta.y !== 0 ? (event.angleDelta.y / 120.0) : (event.pixelDelta.y / 40.0);
                    if (Math.abs(dy) > 0.001) {
                        var zoomMult = Math.pow(1.15, dy);
                        root.zoomRelativeAt(curX, curY, zoomMult);
                    } else if (event.pixelDelta.x !== 0) {
                        root.panX += event.pixelDelta.x * 0.75;
                        root.clampPan();
                        root.updateNormCoordinates();
                    }
                }
                event.accepted = true;
            }
        }

        // MOUSE PAN (CLICK & DRAG WITH DELTA TRACKING)
        DragHandler {
            id: dragHandler
            target: null
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
            cursorShape: (root.zoomFactor > 1.05 || Math.abs(root.rotationAngle) > 0.5) ? (active ? Qt.ClosedHandCursor : Qt.OpenHandCursor) : Qt.ArrowCursor

            property real lastDragX: 0
            property real lastDragY: 0

            onActiveChanged: {
                if (active) {
                    lastDragX = translation.x;
                    lastDragY = translation.y;
                }
            }

            onTranslationChanged: {
                if (!active) return;
                var dx = translation.x - lastDragX;
                var dy = translation.y - lastDragY;
                lastDragX = translation.x;
                lastDragY = translation.y;

                root.panX += dx;
                root.panY += dy;
                root.clampPan();
                root.updateNormCoordinates();
            }
        }

        // MAC DOUBLE-TAP TO TOGGLE FIT <-> 200% ZOOM
        TapHandler {
            acceptedButtons: Qt.LeftButton
            onDoubleTapped: function(event) {
                if (root.zoomFactor > 1.05 || Math.abs(root.rotationAngle) > 0.5) {
                    root.resetTransform();
                } else {
                    root.setZoomAtPoint(event.position.x, event.position.y, 2.0);
                }
            }
        }

        // Full-resolution detail of the visible area, drawn 1:1 in physical pixels
        Image {
            id: detailImage
            visible: false
            x: root.detailRegion ? root.detailRegion.screen[0] : 0
            y: root.detailRegion ? root.detailRegion.screen[1] : 0
            width: root.detailRegion ? root.detailRegion.screen[2] : 0
            height: root.detailRegion ? root.detailRegion.screen[3] : 0
            asynchronous: true
            cache: false
            smooth: false
            mipmap: false
            z: 1
            onStatusChanged: {
                if (status === Image.Ready && root.detailRegion && root.detailWanted) visible = true;
            }
        }
    }

    // FLOATING ROTATION HUD BADGE (Appears when photo is rotated)
    Rectangle {
        id: rotationBadge
        visible: Math.abs(root.rotationAngle) > 0.4
        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.topMargin: 14
        implicitHeight: 28
        implicitWidth: rotRow.implicitWidth + 20
        radius: Theme.r(14)
        color: Theme.isDarkTheme ? Qt.rgba(Theme.bgDark.r, Theme.bgDark.g, Theme.bgDark.b, 0.92) : Qt.rgba(Theme.bgSurface.r, Theme.bgSurface.g, Theme.bgSurface.b, 0.96)
        border.color: Theme.borderLight
        border.width: 1
        z: 30

        RowLayout {
            id: rotRow
            anchors.centerIn: parent
            spacing: 8

            Text {
                text: Theme.iconRotate
                font.family: Theme.iconFont
                font.pixelSize: Theme.px(11)
                color: Theme.accent
            }

            Text {
                text: (root.rotationAngle > 0 ? "+" : "") + root.rotationAngle.toFixed(1) + "°"
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(11)
                font.family: Theme.monoFont
                font.weight: Font.Bold
                color: Theme.textMain
            }

            Rectangle {
                implicitWidth: resetTxt.implicitWidth + 10
                implicitHeight: 18
                radius: Theme.r(9)
                color: resetMouse.containsMouse ? Theme.accentHover : Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2)

                Text {
                    font.family: Theme.fontFamily
                    id: resetTxt
                    anchors.centerIn: parent
                    text: qsTr("Reset 0°")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: resetMouse.containsMouse ? Theme.bgBase : Theme.accent
                }

                MouseArea {
                    id: resetMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.resetRotation()
                }
            }
        }
    }

    // FLOATING QUICK TRANSFORM TOOLBAR (Bottom Center)
    Rectangle {
        anchors.bottom: parent.bottom
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottomMargin: 14
        implicitHeight: 34
        implicitWidth: transformRow.implicitWidth + 20
        radius: Theme.r(17)
        color: Theme.isDarkTheme ? Qt.rgba(Theme.bgDark.r, Theme.bgDark.g, Theme.bgDark.b, 0.94) : Qt.rgba(Theme.bgSurface.r, Theme.bgSurface.g, Theme.bgSurface.b, 0.98)
        border.color: Theme.borderLight
        border.width: 1
        z: 25
        visible: root.imageSource !== ""

        RowLayout {
            id: transformRow
            anchors.centerIn: parent
            spacing: 6

            // Rotate 90° CCW
            // Labelled "90°" so it does not read as undo / redo
            Rectangle {
                implicitWidth: ccwMouseLabel.implicitWidth + 12
                implicitHeight: 24
                radius: Theme.r(4)
                color: ccwMouse.containsMouse ? Theme.bgCardHover : "transparent"
                Text {
                    id: ccwMouseLabel
                    anchors.centerIn: parent
                    text: Theme.iconRotateLeft + " 90°"
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: ccwMouse.containsMouse ? Theme.accent : Theme.textMain
                }
                MouseArea {
                    id: ccwMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Rotate left")
                    onClicked: root.rotateStep(-90)
                }
            }

            // Rotate 90° CW
            // Labelled "90°" so it does not read as undo / redo
            Rectangle {
                implicitWidth: cwMouseLabel.implicitWidth + 12
                implicitHeight: 24
                radius: Theme.r(4)
                color: cwMouse.containsMouse ? Theme.bgCardHover : "transparent"
                Text {
                    id: cwMouseLabel
                    anchors.centerIn: parent
                    text: Theme.iconRotateRight + " 90°"
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: cwMouse.containsMouse ? Theme.accent : Theme.textMain
                }
                MouseArea {
                    id: cwMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Rotate right")
                    onClicked: root.rotateStep(90)
                }
            }

            Rectangle {
                implicitWidth: 1
                implicitHeight: 16
                color: Theme.borderLight
            }

            // Fit Button
            Rectangle {
                implicitWidth: Math.max(34, label978.implicitWidth + 16)
                implicitHeight: 24
                radius: Theme.r(4)
                color: root.zoomFactor <= 1.05 ? Theme.accent : (fitMouse.containsMouse ? Theme.bgCardHover : "transparent")
                Text {
                    id: label978
                    anchors.centerIn: parent
                    text: qsTr("Fit")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.family: Theme.monoFont
                    font.weight: Font.Bold
                    color: root.zoomFactor <= 1.05 ? Theme.onAccent : Theme.textMain
                }
                MouseArea {
                    id: fitMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.resetZoomFit()
                }
            }

            // 100% Button
            Rectangle {
                implicitWidth: Math.max(42, label1003.implicitWidth + 16)
                implicitHeight: 24
                radius: Theme.r(4)
                color: Math.abs(root.pixelScale - 1.0) < 0.02 ? Theme.accent : (pct100Mouse.containsMouse ? Theme.bgCardHover : "transparent")
                Text {
                    id: label1003
                    anchors.centerIn: parent
                    text: "100%"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.family: Theme.monoFont
                    font.weight: Font.Bold
                    color: Math.abs(root.pixelScale - 1.0) < 0.02 ? Theme.onAccent : Theme.textMain
                }
                MouseArea {
                    id: pct100Mouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.setZoomAtPoint(root.width / 2, root.height / 2, root.zoomForPixelScale(1.0))
                }
            }

            // 200% Button
            Rectangle {
                implicitWidth: Math.max(42, label1028.implicitWidth + 16)
                implicitHeight: 24
                radius: Theme.r(4)
                color: Math.abs(root.pixelScale - 2.0) < 0.04 ? Theme.accent : (pct200Mouse.containsMouse ? Theme.bgCardHover : "transparent")
                Text {
                    id: label1028
                    anchors.centerIn: parent
                    text: "200%"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.family: Theme.monoFont
                    font.weight: Font.Bold
                    color: Math.abs(root.pixelScale - 2.0) < 0.04 ? Theme.onAccent : Theme.textMain
                }
                MouseArea {
                    id: pct200Mouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.setZoomAtPoint(root.width / 2, root.height / 2, root.zoomForPixelScale(2.0))
                }
            }

            Rectangle {
                implicitWidth: 1
                implicitHeight: 16
                color: Theme.borderLight
            }

            // Current zoom
            Text {
                Layout.minimumWidth: Theme.px(34)
                horizontalAlignment: Text.AlignHCenter
                text: Math.round(root.pixelScale * 100) + "%"
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.family: Theme.monoFont
                color: Theme.textMuted
            }

            Rectangle {
                implicitWidth: 1
                implicitHeight: 16
                color: Theme.borderLight
            }

            // Surround: dark, middle grey or theme colour
            Rectangle {
                implicitWidth: 28
                implicitHeight: 24
                radius: Theme.r(4)
                color: surroundMouse.containsMouse ? Theme.bgCardHover : "transparent"
                Text {
                    anchors.centerIn: parent
                    text: "\uf042"
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(12)
                    color: surroundMouse.containsMouse ? Theme.accent : Theme.textMain
                }
                MouseArea {
                    id: surroundMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Surround: %1").arg(root.surround === "grey" ? qsTr("middle grey")
                        : root.surround === "theme" ? qsTr("theme colour") : qsTr("dark grey"))
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Surround colour")
                    onClicked: root.surroundCycled(root.surround === "dark" ? "grey"
                        : root.surround === "grey" ? "theme" : "dark")
                }
            }
        }
    }

    // FLOATING TOP CROP & COMPOSITION TOOLBAR (Active in Crop Mode)
    Rectangle {
        visible: root.isCropMode
        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.topMargin: 14
        implicitHeight: 34
        implicitWidth: cropRow.implicitWidth + 20
        radius: Theme.r(17)
        color: Theme.isDarkTheme ? Qt.rgba(Theme.bgDark.r, Theme.bgDark.g, Theme.bgDark.b, 0.94) : Qt.rgba(Theme.bgSurface.r, Theme.bgSurface.g, Theme.bgSurface.b, 0.98)
        border.color: Theme.accent
        border.width: 1
        z: 35

        RowLayout {
            id: cropRow
            anchors.centerIn: parent
            spacing: 6

            Text {
                text: Theme.iconCrop
                font.family: Theme.iconFont
                font.pixelSize: Theme.px(11)
                color: Theme.accent
            }

            Text {
                font.family: Theme.fontFamily
                text: qsTr("Crop")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.weight: Font.Bold
                color: Theme.textMain
            }

            Rectangle { implicitWidth: 1; implicitHeight: 16; color: Theme.border }

            // Aspect Presets
            property var aspects: ["Original", "1:1", "4:5", "9:16", "16:9", "Free"]
            Repeater {
                model: parent.aspects
                delegate: Rectangle {
                    implicitWidth: aspText.implicitWidth + 10
                    implicitHeight: 22
                    radius: Theme.r(4)
                    property bool isCur: root.cropAspect.indexOf(modelData) !== -1
                    color: isCur ? Theme.accent : (aspMouse.containsMouse ? Theme.bgCardHover : "transparent")

                    Text {
                        font.family: Theme.fontFamily
                        id: aspText
                        anchors.centerIn: parent
                        text: modelData === "Original" ? qsTr("Original") : (modelData === "Free" ? qsTr("Free") : modelData)
                        textFormat: Text.PlainText
                        font.pixelSize: Theme.px(9)
                        font.weight: Font.DemiBold
                        color: parent.isCur ? Theme.bgBase : Theme.textMain
                    }
                    MouseArea {
                        id: aspMouse
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.applyAspectPreset(modelData)
                    }
                }
            }

            Rectangle { implicitWidth: 1; implicitHeight: 16; color: Theme.border }

            // Composition Guides Cycle
            Rectangle {
                implicitWidth: guideText.implicitWidth + 12
                implicitHeight: 22
                radius: Theme.r(4)
                color: root.guideMode > 0 ? Qt.rgba(Theme.accentCyan.r, Theme.accentCyan.g, Theme.accentCyan.b, 0.2) : "transparent"
                border.color: root.guideMode > 0 ? Theme.accentCyan : Theme.border
                border.width: 1

                Text {
                    font.family: Theme.fontFamily
                    id: guideText
                    anchors.centerIn: parent
                    text: {
                        if (root.guideMode === 1) return qsTr("Grid: 3x3 Thirds");
                        if (root.guideMode === 2) return qsTr("Grid: Golden Ratio");
                        if (root.guideMode === 3) return qsTr("Grid: Golden Spiral");
                        return qsTr("Grid: Off");
                    }
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.DemiBold
                    color: root.guideMode > 0 ? Theme.accentCyan : Theme.textDim
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        root.guideMode = (root.guideMode + 1) % 4;
                    }
                }
            }

            // Reset Crop
            Rectangle {
                implicitWidth: Math.max(46, label1212.implicitWidth + 16)
                implicitHeight: 22
                radius: Theme.r(4)
                color: rstCropMouse.containsMouse ? Theme.bgCardHover : "transparent"
                Text {
                    id: label1212
                    font.family: Theme.fontFamily
                    anchors.centerIn: parent
                    text: qsTr("Reset")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: Theme.textMuted
                }
                MouseArea {
                    id: rstCropMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.resetCrop()
                }
            }

            // Done Button
            Rectangle {
                implicitWidth: Math.max(46, label1236.implicitWidth + 16)
                implicitHeight: 22
                radius: Theme.r(4)
                color: Theme.accent
                Text {
                    id: label1236
                    font.family: Theme.fontFamily
                    anchors.centerIn: parent
                    text: qsTr("Done")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: Theme.bgBase
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.isCropMode = false
                }
            }
        }
    }

    // Floating Before / After Header
    RowLayout {
        visible: root.isSplitView
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 14
        z: 20

        Rectangle {
            implicitWidth: beforeText.implicitWidth + 16
            implicitHeight: 22
            radius: Theme.r(4)
            color: Qt.rgba(0, 0, 0, 0.75)
            Text {
                font.family: Theme.fontFamily
                id: beforeText
                anchors.centerIn: parent
                text: qsTr("ORIGINAL RAW (Before)")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(9)
                font.weight: Font.Bold
                color: "#ffffff"
            }
        }

        Item { Layout.fillWidth: true }

        Rectangle {
            implicitWidth: afterText.implicitWidth + 16
            implicitHeight: 22
            radius: Theme.r(4)
            color: Qt.rgba(0, 0, 0, 0.75)
            Text {
                font.family: Theme.fontFamily
                id: afterText
                anchors.centerIn: parent
                text: qsTr("EDITED RECIPE (After)")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(9)
                font.weight: Font.Bold
                color: Theme.accent
            }
        }
    }

    // Floating Clipping Warning Badges (Top Right)
    RowLayout {
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: 14
        spacing: 6
        z: 25

        Rectangle {
            visible: root.showShadowMask
            implicitWidth: shadowClipTxt.implicitWidth + 12
            implicitHeight: 22
            radius: Theme.r(4)
            color: Qt.rgba(Theme.shadowClip.r, Theme.shadowClip.g, Theme.shadowClip.b, 0.85)
            border.color: Theme.shadowClip
            border.width: 1

            Text {
                id: shadowClipTxt
                anchors.centerIn: parent
                text: qsTr("Shadow clipping")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(9)
                font.family: Theme.monoFont
                font.weight: Font.Bold
                color: "#ffffff"
            }
        }

        Rectangle {
            visible: root.showHighlightMask
            implicitWidth: highClipTxt.implicitWidth + 12
            implicitHeight: 22
            radius: Theme.r(4)
            color: Qt.rgba(Theme.highlightClip.r, Theme.highlightClip.g, Theme.highlightClip.b, 0.85)
            border.color: Theme.highlightClip
            border.width: 1

            Text {
                id: highClipTxt
                anchors.centerIn: parent
                text: qsTr("Highlight clipping")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(9)
                font.family: Theme.monoFont
                font.weight: Font.Bold
                color: "#ffffff"
            }
        }
    }

    // Empty state: guide first-time users instead of showing a blank canvas
    Column {
        anchors.centerIn: parent
        spacing: 8
        visible: root.imageSource === ""
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: Theme.iconCamera
            font.family: Theme.iconFont
            font.pixelSize: Theme.px(34)
            color: Theme.textDim
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("Open a RAW (Ctrl+O) or drop a photo / folder here")
            textFormat: Text.PlainText
            font.family: Theme.monoFont
            font.pixelSize: Theme.px(12)
            color: Theme.textDim
        }
    }

    // Wayland & Linux Native Drag and Drop Area
    DropArea {
        id: dropArea
        anchors.fill: parent
        z: 50

        onEntered: function(drag) {
            if (drag.hasUrls) {
                drag.acceptProposedAction();
            }
        }

        onDropped: function(drop) {
            if (!drop.hasUrls || drop.urls.length === 0) return;
            var urlStr = String(drop.urls[0]);
            var path = urlStr;
            if (path.indexOf("file://") === 0) {
                path = decodeURIComponent(path.substring(7));
            }
            if (!path || path.trim() === "") return;

            var lower = path.toLowerCase();
            var rawExts = [".3fr", ".ari", ".arw", ".bay", ".cap", ".cr2", ".cr3", ".crw", ".cs1", ".dc2", ".dcr", ".dcs", ".dng", ".drf", ".eip", ".erf", ".fff", ".gpr", ".iiq", ".k25", ".kc2", ".kdc", ".mdc", ".mef", ".mfw", ".mos", ".mrw", ".nef", ".nrw", ".orf", ".ori", ".pef", ".ptx", ".pxn", ".qtk", ".raf", ".raw", ".rdc", ".rw2", ".rwl", ".rwz", ".sr2", ".srf", ".srw", ".sti", ".x3f", ".jpg", ".jpeg", ".jpe", ".png", ".webp", ".tif", ".tiff", ".heic", ".heif", ".hif", ".avif", ".jxl", ".exr", ".hdr", ".pnm", ".ppm", ".pgm", ".pam"];
            var isFile = false;
            for (var i = 0; i < rawExts.length; i++) {
                if (lower.endsWith(rawExts[i])) {
                    isFile = true;
                    break;
                }
            }

            if (isFile) {
                root.fileDropped(path);
            } else {
                root.folderDropped(path);
            }
            drop.acceptProposedAction();
        }
    }

    // Visual Drop Highlight Overlay
    Rectangle {
        anchors.fill: parent
        color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
        border.color: Theme.accent
        border.width: 3
        radius: Theme.r(8)
        visible: dropArea.containsDrag
        z: 60

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 12

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                width: 64
                height: 64
                radius: 32
                color: Qt.rgba(0, 0, 0, 0.6)
                border.color: Theme.accent
                border.width: 2

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconFolder
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(28)
                    color: Theme.accent
                }
            }

            Text {
                text: qsTr("Drop RAW Photo or Folder to Open")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(14)
                font.weight: Font.Bold
                font.family: Theme.monoFont
                color: Theme.textMain
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }

    onWidthChanged: { updateNormCoordinates(); scheduleDetail(); }
    onHeightChanged: { updateNormCoordinates(); scheduleDetail(); }
}
