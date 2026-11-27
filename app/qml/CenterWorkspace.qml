import QtQuick
import QtQuick.Layouts

// Center workspace column: Grade Versions toolstrip, soft-proof badge, and Viewport canvas.
ColumnLayout {
    id: centerColumn

    property var host: null
    property var layersPanel: null
    property var retouchPanel: null
    property var filmstrip: null

    readonly property alias gradeStrip: gradeStrip
    readonly property alias viewport: viewport

    Layout.fillWidth: true
    Layout.fillHeight: true
    spacing: 0

    // DaVinci Resolve Secondary Toolstrip: Grade Versions & Look tools
    Rectangle {
        id: gradeStrip
        Layout.fillWidth: true
        implicitHeight: Theme.sp(32)
        color: Theme.bgDark
        border.color: Theme.border
        border.width: 1

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            spacing: 8

            GradeVersionsBar {
                currentVersion: centerColumn.host ? centerColumn.host.currentGradeVersion : "A"
                activeVersions: centerColumn.host ? ({
                    "A": centerColumn.host.gradeVersions["A"] !== null,
                    "B": centerColumn.host.gradeVersions["B"] !== null,
                    "C": centerColumn.host.gradeVersions["C"] !== null,
                    "D": centerColumn.host.gradeVersions["D"] !== null
                }) : ({ "A": false, "B": false, "C": false, "D": false })
                onSwitchVersion: function(v) { if (centerColumn.host) centerColumn.host.switchGradeVersion(v) }
                onCopyCurrentToVersion: function(v) { if (centerColumn.host) centerColumn.host.copyCurrentToVersion(v) }
            }

            Item { Layout.fillWidth: true }

            // The preview is simulated while soft proofing is on; a click turns it off
            Rectangle {
                visible: centerColumn.host ? centerColumn.host.softProof : false
                implicitWidth: proofBadge.implicitWidth + 14
                implicitHeight: Theme.sp(20)
                radius: Theme.r(4)
                color: Qt.rgba(Theme.accentYellow.r, Theme.accentYellow.g, Theme.accentYellow.b, 0.15)
                border.color: Theme.accentYellow
                border.width: 1
                Text {
                    id: proofBadge
                    anchors.centerIn: parent
                    text: qsTr("Soft proof: %1").arg(centerColumn.host ? centerColumn.host.proofProfileName() : "")
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: Theme.px(9)
                    font.weight: Font.Bold
                    color: Theme.accentYellow
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: if (centerColumn.host) centerColumn.host.softProof = false
                }
                KeyFocus {
                    name: proofBadge.text
                    onActivated: if (centerColumn.host) centerColumn.host.softProof = false
                }
            }
        }
    }

    Viewport {
        id: viewport
        surround: centerColumn.host ? centerColumn.host.surround : "dark"
        onSurroundCycled: function(next) { if (centerColumn.host) centerColumn.host.surround = next }
        Layout.fillWidth: true
        Layout.fillHeight: true
        isSplitView: centerColumn.host ? centerColumn.host.isSplitView : false
        splitRatio: centerColumn.host ? centerColumn.host.splitRatio : 0.5
        watermarkEnabled: centerColumn.host ? centerColumn.host.watermarkEnabled : false
        watermarkType: centerColumn.host ? centerColumn.host.watermarkType : "text"
        watermarkText: centerColumn.host ? centerColumn.host.watermarkText : ""
        watermarkLogoPath: centerColumn.host ? centerColumn.host.watermarkLogoPath : ""
        watermarkPositionIndex: centerColumn.host ? centerColumn.host.watermarkPositionIndex : 8
        watermarkOpacity: centerColumn.host ? centerColumn.host.watermarkOpacity : 0.85
        watermarkSize: centerColumn.host ? centerColumn.host.watermarkSize : 14
        watermarkMargin: centerColumn.host ? centerColumn.host.watermarkMargin : 24
        watermarkColorHex: centerColumn.host ? centerColumn.host.watermarkColorHex : "#ffffff"
        watermarkDropShadow: centerColumn.host ? centerColumn.host.watermarkDropShadow : true
        exifMetadata: centerColumn.host ? centerColumn.host.activeMetadata : null
        perspectiveVertical: centerColumn.host ? centerColumn.host.perspectiveVertical : 0.0
        perspectiveHorizontal: centerColumn.host ? centerColumn.host.perspectiveHorizontal : 0.0
        perspectiveScale: centerColumn.host ? centerColumn.host.perspectiveScale : 100.0
        // Mask editing follows the selected layer while the Masks tab is open
        maskTool: (centerColumn.host && centerColumn.retouchPanel && centerColumn.layersPanel
                   && centerColumn.host.activePhotoPath !== "" && centerColumn.host.showRightPanel
                   && centerColumn.host.rightTab === "masks" && !centerColumn.retouchPanel.editing)
                  ? centerColumn.layersPanel.overlayTool : ""
        retouchActive: Boolean(centerColumn.host && centerColumn.retouchPanel
                               && centerColumn.host.activePhotoPath !== "" && centerColumn.host.showRightPanel
                               && centerColumn.host.rightTab === "masks" && centerColumn.retouchPanel.editing)
        retouchSpots: centerColumn.host ? centerColumn.host.retouchSpots : []
        retouchSelected: centerColumn.retouchPanel ? centerColumn.retouchPanel.selected : -1
        retouchMode: centerColumn.retouchPanel ? centerColumn.retouchPanel.mode : "heal"
        retouchRadius: centerColumn.retouchPanel ? centerColumn.retouchPanel.radius : 0.02
        retouchFeather: centerColumn.retouchPanel ? centerColumn.retouchPanel.feather : 0.5
        retouchOpacity: centerColumn.retouchPanel ? centerColumn.retouchPanel.opacity_ : 1.0
        onRetouchSpotsEdited: function(spots) {
            if (!centerColumn.host) return;
            centerColumn.host.retouchSpots = spots;
            centerColumn.host.requestRender();
        }
        onRetouchSpotSelected: function(index) { if (centerColumn.retouchPanel) centerColumn.retouchPanel.selected = index }
        maskShape: centerColumn.layersPanel ? centerColumn.layersPanel.editShape : null
        maskBrushSize: centerColumn.layersPanel ? centerColumn.layersPanel.brushSize : 0.05
        maskBrushFeather: centerColumn.layersPanel ? centerColumn.layersPanel.brushFeather : 0.5
        maskBrushErase: centerColumn.layersPanel ? centerColumn.layersPanel.brushErase : false
        onMaskShapeEdited: function(shape) { if (centerColumn.layersPanel) centerColumn.layersPanel.setEditShape(shape) }
        onMaskStrokePainted: function(points, aspect) { if (centerColumn.layersPanel) centerColumn.layersPanel.addStroke(points, aspect) }
        onMaskColourPicked: function(r, g, b) { if (centerColumn.layersPanel) centerColumn.layersPanel.setPickedColour(r, g, b) }
        onSplitRatioChangedByUser: function(r) {
            if (!centerColumn.host) return;
            centerColumn.host.splitRatio = r;
            centerColumn.host.requestRender();
        }
        onRotationChangedByUser: function(a) {
            if (centerColumn.host) centerColumn.host.requestRender();
        }
        onDetailRequested: function(region, outWidth, outHeight) {
            if (!centerColumn.host) return;
            centerColumn.host.sendDaemonCommand({
                cmd: "detail",
                recipe: centerColumn.host.buildRecipeObject(),
                region: region,
                out_width: outWidth,
                out_height: outHeight,
                proof: centerColumn.host.proofRequest(),
                focus_peaking: centerColumn.host.focusPeaking
            });
        }
        onLoupeRequested: function(region, outWidth, outHeight) {
            if (!centerColumn.host) return;
            centerColumn.host.sendDaemonCommand({
                cmd: "detail",
                tag: "loupe",
                recipe: centerColumn.host.buildRecipeObject(),
                region: region,
                out_width: outWidth,
                out_height: outHeight,
                proof: centerColumn.host.proofRequest(),
                focus_peaking: centerColumn.host.focusPeaking
            });
        }
        focusPeaking: centerColumn.host ? centerColumn.host.focusPeaking : false
        onFocusPeakingToggled: if (centerColumn.host) centerColumn.host.focusPeaking = !centerColumn.host.focusPeaking
        compareActive: centerColumn.host ? centerColumn.host.compareMode : false
        compareImage: centerColumn.host ? centerColumn.host.referenceImage : ""
        compareChoices: centerColumn.host ? centerColumn.host.compareChoices : []
        compareChoice: centerColumn.host ? centerColumn.host.compareRef : "before"
        onCompareToggled: if (centerColumn.host) centerColumn.host.compareMode = !centerColumn.host.compareMode
        onCompareChoicePicked: function(id) { if (centerColumn.host) centerColumn.host.compareRef = id }
        onCropChangedByUser: function(cx, cy, cw, ch, aspect) {
            if (centerColumn.host) centerColumn.host.onCropEdited(cx, cy, cw, ch, aspect);
        }
        onFileDropped: function(filePath) {
            if (!centerColumn.host) return;
            centerColumn.host.loadPhoto(filePath);
            centerColumn.host.showToast(qsTr("Opened %1").arg(filePath.split("/").pop()), Theme.accentCyan);
        }
        onFolderDropped: function(folderPath) {
            if (!centerColumn.host) return;
            if (centerColumn.filmstrip) centerColumn.filmstrip.currentFolder = folderPath;
            centerColumn.host.scanLocalFolder(folderPath);
            centerColumn.host.showToast(qsTr("Scanning folder %1").arg(folderPath.split("/").pop()), Theme.accentCyan);
        }
    }
}
