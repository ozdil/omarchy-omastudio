import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    implicitWidth: 260
    implicitHeight: 180
    radius: Theme.radiusMd
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1
    clip: true

    property Item sourceItem: null
    property real sourceAspect: 1.5
    property bool useSoftwareFallback: false
    property string imageSource: ""
    property int revisionCounter: 0
    readonly property bool useLiveMirror: !root.useSoftwareFallback && root.sourceItem !== null && root.imageSource.length > 0

    function toImageUrl(src, rev) {
        if (!src || src.length === 0) return "";
        if (src.indexOf("image://") === 0 || src.indexOf("file://") === 0 || src.indexOf("qrc:/") === 0)
            return src;
        var url = "file://" + src + "?v=" + rev;
        return url;
    }

    function syncFallbackImages() {
        if (!root.imageSource || root.imageSource.length === 0 || root.useLiveMirror) {
            miniImg0.source = "";
            miniImg1.source = "";
            miniImage.currentSource = "";
            return;
        }
        root.revisionCounter++;
        var nextSrc = root.toImageUrl(root.imageSource, root.revisionCounter);

        if (miniImg0.status !== Image.Ready && miniImg1.status !== Image.Ready) {
            miniImg0.source = nextSrc;
        } else if (miniImage.activeBuffer === 0) {
            miniImg1.source = nextSrc;
        } else {
            miniImg0.source = nextSrc;
        }
    }

    onImageSourceChanged: root.syncFallbackImages()
    onUseLiveMirrorChanged: root.syncFallbackImages()
    property real zoomFactor: 1.0

    property real rotationAngle: 0.0
    property real normX: 0.0     // 0.0 to 1.0 (visible center X)
    property real normY: 0.0     // 0.0 to 1.0 (visible center Y)
    property real normW: 1.0     // 0.0 to 1.0 (visible width ratio)
    property real normH: 1.0     // 0.0 to 1.0 (visible height ratio)

    signal zoomRequested(real zoom)
    signal panRequested(real targetNormX, real targetNormY)

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 6
        spacing: 4

        // Top Navigation Bar
        RowLayout {
            Layout.fillWidth: true
            spacing: 4

            Text {
                font.family: Theme.fontFamily
                text: qsTr("Navigator")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(11)
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
                elide: Text.ElideRight
            }

            Item { Layout.fillWidth: true }

        }

        // Mini Canvas Area
        Rectangle {
            id: miniContainer
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: Theme.r(4)
            color: Theme.bgDark
            clip: true

            Item {
                id: miniImage
                anchors.fill: parent
                anchors.margins: 4
                rotation: root.rotationAngle

                property int activeBuffer: 0
                property string currentSource: ""
                readonly property var currentActiveImage: activeBuffer === 0 ? miniImg0 : miniImg1
                readonly property real safeAspect: root.sourceAspect > 0.01 ? root.sourceAspect : 1.5
                readonly property real mirrorFitW: (width / Math.max(1, height) > safeAspect) ? (height * safeAspect) : width
                readonly property real mirrorFitH: (width / Math.max(1, height) > safeAspect) ? height : (width / safeAspect)
                readonly property real paintedWidth: root.useLiveMirror
                    ? mirrorFitW
                    : (currentActiveImage && currentActiveImage.paintedWidth > 0 ? currentActiveImage.paintedWidth : width)
                readonly property real paintedHeight: root.useLiveMirror
                    ? mirrorFitH
                    : (currentActiveImage && currentActiveImage.paintedHeight > 0 ? currentActiveImage.paintedHeight : height)

                // Zero-decode live GPU mirror of Viewport's preview surface (saves ~100+ MB vs duplicate decode)
                ShaderEffectSource {
                    id: miniMirror
                    visible: root.useLiveMirror
                    sourceItem: root.useLiveMirror ? root.sourceItem : null
                    live: root.useLiveMirror
                    hideSource: false
                    smooth: true
                    textureSize: Qt.size(256, 256)
                    anchors.centerIn: parent
                    width: miniImage.mirrorFitW
                    height: miniImage.mirrorFitH
                }

                // Software-renderer fallback (QT_QUICK_BACKEND=software does not support ShaderEffectSource)
                Image {
                    id: miniImg0
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectFit
                    sourceSize: Qt.size(256, 256)
                    asynchronous: true
                    cache: false
                    smooth: true
                    visible: !root.useLiveMirror && opacity > 0.001
                    opacity: miniImage.activeBuffer === 0 ? 1.0 : 0.0
                    z: miniImage.activeBuffer === 0 ? 2 : 1
                    onStatusChanged: {
                        if (status === Image.Ready) {
                            if (miniImage.activeBuffer === 1 || miniImage.currentSource === "") {
                                miniImage.activeBuffer = 0;
                                miniImage.currentSource = source;
                            }
                        }
                    }
                }

                Image {
                    id: miniImg1
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectFit
                    sourceSize: Qt.size(256, 256)
                    asynchronous: true
                    cache: false
                    smooth: true
                    visible: !root.useLiveMirror && opacity > 0.001
                    opacity: miniImage.activeBuffer === 1 ? 1.0 : 0.0
                    z: miniImage.activeBuffer === 1 ? 2 : 1
                    onStatusChanged: {
                        if (status === Image.Ready) {
                            if (miniImage.activeBuffer === 0 || miniImage.currentSource === "") {
                                miniImage.activeBuffer = 1;
                                miniImage.currentSource = source;
                            }
                        }
                    }
                }


                // Viewfinder highlight box representing visible viewport area
                Rectangle {
                    id: viewFinder
                    visible: root.zoomFactor > 1.05

                    // Calculate position and size based on painted area
                    property real pw: miniImage.paintedWidth > 0 ? miniImage.paintedWidth : miniImage.width
                    property real ph: miniImage.paintedHeight > 0 ? miniImage.paintedHeight : miniImage.height
                    property real px: (miniImage.width - pw) / 2
                    property real py: (miniImage.height - ph) / 2

                    width: Math.max(16, pw * Math.min(1.0, root.normW))
                    height: Math.max(16, ph * Math.min(1.0, root.normH))
                    x: px + (pw * root.normX) - width / 2
                    y: py + (ph * root.normY) - height / 2

                    color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2)
                    border.color: Theme.accent
                    border.width: 1.5
                    radius: Theme.r(2)

                    // Crosshair in center of viewfinder
                    Rectangle {
                        anchors.centerIn: parent
                        width: 4
                        height: 4
                        radius: Theme.r(2)
                        color: Theme.accent
                    }

                    MouseArea {
                        id: dragArea
                        anchors.fill: parent
                        anchors.margins: -8
                        cursorShape: Qt.SizeAllCursor
                        property real grabOffsetX: 0
                        property real grabOffsetY: 0

                        onPressed: function(mouse) {
                            var pt = dragArea.mapToItem(miniImage, mouse.x, mouse.y);
                            grabOffsetX = pt.x - (viewFinder.x + viewFinder.width / 2);
                            grabOffsetY = pt.y - (viewFinder.y + viewFinder.height / 2);
                        }

                        onPositionChanged: function(mouse) {
                            if (pressed && viewFinder.pw > 0 && viewFinder.ph > 0) {
                                var pt = dragArea.mapToItem(miniImage, mouse.x, mouse.y);
                                var centerX = (pt.x - grabOffsetX) - viewFinder.px;
                                var centerY = (pt.y - grabOffsetY) - viewFinder.py;
                                var targetX = Math.max(0.0, Math.min(1.0, centerX / viewFinder.pw));
                                var targetY = Math.max(0.0, Math.min(1.0, centerY / viewFinder.ph));
                                root.panRequested(targetX, targetY);
                            }
                        }
                    }
                }

                // Click anywhere on mini image to jump there
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    acceptedButtons: Qt.LeftButton
                    z: -1
                    onClicked: function(mouse) {
                        var pw = miniImage.paintedWidth > 0 ? miniImage.paintedWidth : miniImage.width;
                        var ph = miniImage.paintedHeight > 0 ? miniImage.paintedHeight : miniImage.height;
                        var px = (miniImage.width - pw) / 2;
                        var py = (miniImage.height - ph) / 2;

                        var relX = (mouse.x - px) / pw;
                        var relY = (mouse.y - py) / ph;

                        if (relX >= 0.0 && relX <= 1.0 && relY >= 0.0 && relY <= 1.0) {
                            root.panRequested(relX, relY);
                        }
                    }
                }
            }
        }
    }
}
