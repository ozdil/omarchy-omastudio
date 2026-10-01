import QtQuick
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    implicitWidth: 260
    implicitHeight: 180
    radius: Theme.radiusMd
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1
    clip: true

    property string imageSource: ""
    onImageSourceChanged: {
        if (!root.imageSource || root.imageSource.length === 0) {
            miniImg0.source = "";
            miniImg1.source = "";
            miniImage.currentSource = "";
            return;
        }
        var nextSrc = "file://" + root.imageSource;
        if (nextSrc === miniImage.currentSource) return;

        if (miniImg0.status !== Image.Ready && miniImg1.status !== Image.Ready) {
            miniImg0.source = nextSrc;
        } else if (miniImage.activeBuffer === 0) {
            miniImg1.source = nextSrc;
        } else {
            miniImg0.source = nextSrc;
        }
    }
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
                text: "NAVIGATOR"
                textFormat: Text.PlainText
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
            }

            Item { Layout.fillWidth: true }

            // Preset Zoom Buttons: FIT, 100%, 200%, 300%
            Repeater {
                model: [
                    { label: "FIT", val: 0.0 },
                    { label: "100%", val: 1.0 },
                    { label: "200%", val: 2.0 },
                    { label: "300%", val: 3.0 }
                ]

                delegate: Rectangle {
                    implicitWidth: lbl.implicitWidth + 10
                    implicitHeight: 18
                    radius: 3
                    property bool isCur: (modelData.label === "FIT" && root.zoomFactor <= 1.05) || (modelData.label !== "FIT" && Math.abs(root.zoomFactor - modelData.val) < 0.1 && root.zoomFactor > 1.05)
                    color: isCur ? Theme.accent : Theme.bgCard
                    border.color: isCur ? Theme.accent : Theme.border

                    Text {
                        id: lbl
                        anchors.centerIn: parent
                        text: modelData.label
                        textFormat: Text.PlainText
                        font.pixelSize: 9
                        font.family: Theme.monoFont
                        font.weight: Font.Bold
                        color: parent.isCur ? Theme.bgBase : Theme.textMuted
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.zoomRequested(modelData.val)
                    }
                }
            }
        }

        // Mini Canvas Area
        Rectangle {
            id: miniContainer
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: 4
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
                readonly property real paintedWidth: currentActiveImage && currentActiveImage.paintedWidth > 0 ? currentActiveImage.paintedWidth : width
                readonly property real paintedHeight: currentActiveImage && currentActiveImage.paintedHeight > 0 ? currentActiveImage.paintedHeight : height

                Image {
                    id: miniImg0
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    smooth: true
                    visible: opacity > 0.001
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
                    asynchronous: true
                    cache: false
                    smooth: true
                    visible: opacity > 0.001
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
                    radius: 2

                    // Crosshair in center of viewfinder
                    Rectangle {
                        anchors.centerIn: parent
                        width: 4
                        height: 4
                        radius: 2
                        color: Theme.accent
                    }

                    MouseArea {
                        id: dragArea
                        anchors.fill: parent
                        anchors.margins: -8
                        cursorShape: Qt.SizeAllCursor
                        drag.target: viewFinder
                        drag.axis: Drag.XAndYAxis
                        drag.minimumX: viewFinder.px
                        drag.maximumX: viewFinder.px + viewFinder.pw - viewFinder.width
                        drag.minimumY: viewFinder.py
                        drag.maximumY: viewFinder.py + viewFinder.ph - viewFinder.height

                        onPositionChanged: {
                            if (drag.active) {
                                var centerX = (viewFinder.x + viewFinder.width / 2) - viewFinder.px;
                                var centerY = (viewFinder.y + viewFinder.height / 2) - viewFinder.py;
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
