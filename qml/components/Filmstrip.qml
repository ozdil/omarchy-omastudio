import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

Rectangle {
    id: root
    implicitHeight: 110
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property var photoList: []
    property string activePhotoPath: ""
    property var selectedPaths: []
    property bool isGdriveMode: false
    property string currentFolder: "~/Downloads/yurt"
    property bool isDownloadingRemote: false
    property bool isListingFolder: false

    signal selectPhoto(string path, bool isRemote)
    signal navigateFolder(string remotePath)
    signal parentFolderRequested()
    signal refreshRequested()
    signal switchSource(bool gdrive)
    signal batchExportRequested()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 6
        spacing: 6

        // Source Switcher & Path Bar
        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            // Local Tab
            Rectangle {
                implicitWidth: localRow.implicitWidth + 16
                implicitHeight: 22
                radius: 4
                color: !root.isGdriveMode ? Theme.accent : Theme.bgCard

                RowLayout {
                    id: localRow
                    anchors.centerIn: parent
                    spacing: 6
                    Text {
                        text: Theme.iconFolder
                        font.family: Theme.iconFont
                        font.pixelSize: 10
                        color: !root.isGdriveMode ? Theme.bgBase : Theme.textMuted
                    }
                    Text {
                        text: "Local Photos"
                        textFormat: Text.PlainText
                        font.pixelSize: 10
                        font.weight: Font.DemiBold
                        color: !root.isGdriveMode ? Theme.bgBase : Theme.textMuted
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        if (root.isGdriveMode) {
                            root.isGdriveMode = false
                            root.switchSource(false)
                        }
                    }
                }
            }

            // Google Drive Tab
            Rectangle {
                implicitWidth: gdriveRow.implicitWidth + 16
                implicitHeight: 22
                radius: 4
                color: root.isGdriveMode ? Theme.accentCyan : Theme.bgCard

                RowLayout {
                    id: gdriveRow
                    anchors.centerIn: parent
                    spacing: 6
                    Text {
                        text: Theme.iconCloud
                        font.family: Theme.iconFont
                        font.pixelSize: 10
                        color: root.isGdriveMode ? Theme.bgBase : Theme.textMuted
                    }
                    Text {
                        text: "Google Drive"
                        textFormat: Text.PlainText
                        font.pixelSize: 10
                        font.weight: Font.DemiBold
                        color: root.isGdriveMode ? Theme.bgBase : Theme.textMuted
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        if (!root.isGdriveMode) {
                            root.isGdriveMode = true
                            root.switchSource(true)
                        }
                    }
                }
            }

            // Parent folder button for Google Drive subfolders
            Rectangle {
                visible: root.isGdriveMode && root.currentFolder !== "Photos"
                implicitWidth: parentBtnRow.implicitWidth + 12
                implicitHeight: 22
                radius: 4
                color: Theme.bgCard
                border.color: Theme.accentCyan
                border.width: 1

                RowLayout {
                    id: parentBtnRow
                    anchors.centerIn: parent
                    spacing: 4
                    Text {
                        text: Theme.iconArrowUp
                        font.family: Theme.iconFont
                        font.pixelSize: 10
                        color: Theme.accentCyan
                    }
                    Text {
                        text: "Up / Parent"
                        textFormat: Text.PlainText
                        font.pixelSize: 9
                        font.weight: Font.DemiBold
                        color: Theme.accentCyan
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.parentFolderRequested()
                }
            }

            Text {
                text: root.currentFolder
                textFormat: Text.PlainText
                font.pixelSize: 10
                font.family: Theme.monoFont
                color: Theme.textDim
                elide: Text.ElideMiddle
                Layout.fillWidth: true
            }

            // Scanning / Listing Folder Spinner / Badge
            Rectangle {
                visible: root.isListingFolder
                implicitWidth: listRow.implicitWidth + 12
                implicitHeight: 22
                radius: 4
                color: Qt.rgba(Theme.accentYellow.r, Theme.accentYellow.g, Theme.accentYellow.b, 0.2)
                border.color: Theme.accentYellow
                border.width: 1

                RowLayout {
                    id: listRow
                    anchors.centerIn: parent
                    spacing: 6
                    Text {
                        text: Theme.iconRefresh
                        font.family: Theme.iconFont
                        font.pixelSize: 10
                        color: Theme.accentYellow
                        transformOrigin: Item.Center
                        RotationAnimator on rotation {
                            from: 0
                            to: 360
                            duration: 1000
                            loops: Animation.Infinite
                            running: root.isListingFolder
                        }
                    }
                    Text {
                        text: root.isGdriveMode ? "Scanning Cloud..." : "Scanning Folder..."
                        textFormat: Text.PlainText
                        font.pixelSize: 9
                        font.weight: Font.Bold
                        color: Theme.accentYellow
                    }
                }
            }

            // Downloading Spinner / Badge
            Rectangle {
                visible: root.isDownloadingRemote
                implicitWidth: dlRow.implicitWidth + 12
                implicitHeight: 22
                radius: 4
                color: Qt.rgba(Theme.accentCyan.r, Theme.accentCyan.g, Theme.accentCyan.b, 0.2)
                border.color: Theme.accentCyan
                border.width: 1

                RowLayout {
                    id: dlRow
                    anchors.centerIn: parent
                    spacing: 6
                    Text {
                        text: Theme.iconRefresh
                        font.family: Theme.iconFont
                        font.pixelSize: 10
                        color: Theme.accentCyan
                        transformOrigin: Item.Center
                        RotationAnimator on rotation {
                            from: 0
                            to: 360
                            duration: 1000
                            loops: Animation.Infinite
                            running: root.isDownloadingRemote
                        }
                    }
                    Text {
                        text: "Downloading RAW..."
                        textFormat: Text.PlainText
                        font.pixelSize: 9
                        font.weight: Font.Bold
                        color: Theme.accentCyan
                    }
                }
            }

            // Batch Export Button
            Rectangle {
                visible: root.selectedPaths.length > 1
                implicitWidth: batchExportRow.implicitWidth + 14
                implicitHeight: 22
                radius: 4
                color: Theme.accent
                border.color: Theme.accent

                RowLayout {
                    id: batchExportRow
                    anchors.centerIn: parent
                    spacing: 5
                    Text {
                        text: Theme.iconExport
                        font.family: Theme.iconFont
                        font.pixelSize: 10
                        color: Theme.bgBase
                    }
                    Text {
                        text: "Batch (" + root.selectedPaths.length + ")"
                        textFormat: Text.PlainText
                        font.pixelSize: 9
                        font.weight: Font.Bold
                        color: Theme.bgBase
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.batchExportRequested()
                }
            }

            Rectangle {
                width: 22
                height: 22
                radius: 4
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    anchors.centerIn: parent
                    text: Theme.iconRefresh
                    font.family: Theme.iconFont
                    font.pixelSize: 10
                    color: Theme.textMuted
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.refreshRequested()
                }
            }
        }

        // Horizontal Thumbnail Strip or Empty State
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            // Empty state notice
            Rectangle {
                anchors.fill: parent
                visible: (!root.photoList || root.photoList.length === 0) && !root.isListingFolder
                color: "transparent"

                RowLayout {
                    anchors.centerIn: parent
                    spacing: 8
                    Text {
                        text: root.isGdriveMode ? Theme.iconCloud : Theme.iconFolder
                        font.family: Theme.iconFont
                        font.pixelSize: 14
                        color: Theme.textDim
                    }
                    Text {
                        text: root.isGdriveMode ? "No photos found in this Google Drive folder." : "No photos found in this directory."
                        textFormat: Text.PlainText
                        font.pixelSize: 10
                        font.family: Theme.monoFont
                        color: Theme.textDim
                    }
                }
            }

            ListView {
                id: listView
                anchors.fill: parent
                orientation: ListView.Horizontal
                spacing: 8
                clip: true
                model: root.photoList

            WheelHandler {
                id: filmstripWheel
                target: null
                onWheel: function(event) {
                    var delta = event.angleDelta.y !== 0 ? event.angleDelta.y : (event.pixelDelta.y !== 0 ? event.pixelDelta.y : (event.angleDelta.x !== 0 ? event.angleDelta.x : event.pixelDelta.x));
                    listView.contentX = Math.max(0, Math.min(Math.max(0, listView.contentWidth - listView.width), listView.contentX - delta));
                    event.accepted = true;
                }
            }

            delegate: Rectangle {
                readonly property bool isSelected: root.selectedPaths.indexOf(modelData.path) !== -1 || root.activePhotoPath === modelData.path
                width: 90
                height: listView.height
                radius: Theme.radiusSm
                color: isSelected ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
                border.color: isSelected ? Theme.accent : Theme.border
                border.width: isSelected ? 2 : 1

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 4
                    spacing: 2

                    // Card Content: Folder or RAW Photo
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        radius: 4
                        color: modelData.is_dir ? Theme.bgSurface : Theme.bgDark
                        clip: true

                        // If Folder: Show Folder Glyph
                        Item {
                            anchors.fill: parent
                            visible: modelData.is_dir === true

                            ColumnLayout {
                                anchors.centerIn: parent
                                spacing: 4
                                Text {
                                    text: Theme.iconFolder
                                    font.family: Theme.iconFont
                                    font.pixelSize: 22
                                    color: Theme.accentCyan
                                    Layout.alignment: Qt.AlignHCenter
                                }
                            }

                            // DIR Badge
                            Rectangle {
                                anchors.top: parent.top
                                anchors.right: parent.right
                                anchors.margins: 2
                                width: 22
                                height: 14
                                radius: 3
                                color: Qt.rgba(0, 0, 0, 0.75)
                                Text {
                                    anchors.centerIn: parent
                                    text: "DIR"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 8
                                    font.weight: Font.Bold
                                    color: Theme.accentCyan
                                }
                            }
                        }

                        // If Photo: Show Thumbnail or Cloud Placeholder
                        Item {
                            anchors.fill: parent
                            visible: !modelData.is_dir

                            // Fallback / Placeholder when thumbnail is not yet cached locally
                            Rectangle {
                                anchors.fill: parent
                                color: Theme.bgSurface
                                visible: !modelData.thumbnail || modelData.thumbnail === ""

                                ColumnLayout {
                                    anchors.centerIn: parent
                                    spacing: 4
                                    Text {
                                        text: root.isDownloadingRemote && root.activePhotoPath === modelData.path ? Theme.iconRefresh : Theme.iconImage
                                        font.family: Theme.iconFont
                                        font.pixelSize: 18
                                        color: root.activePhotoPath === modelData.path ? Theme.accent : Theme.textDim
                                        Layout.alignment: Qt.AlignHCenter
                                    }
                                    Text {
                                        text: root.isDownloadingRemote && root.activePhotoPath === modelData.path ? "FETCHING" : "RAW CLOUD"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 7
                                        font.weight: Font.Bold
                                        font.family: Theme.monoFont
                                        color: Theme.textDim
                                        Layout.alignment: Qt.AlignHCenter
                                    }
                                }
                            }

                            Image {
                                anchors.fill: parent
                                fillMode: Image.PreserveAspectCrop
                                source: (modelData.thumbnail && modelData.thumbnail !== "") ? ("file://" + modelData.thumbnail) : ""
                                asynchronous: true
                                cache: true
                                visible: modelData.thumbnail && modelData.thumbnail !== ""
                            }

                            // Downloading Spinner Overlay on Active Remote Card
                            Rectangle {
                                anchors.fill: parent
                                color: Qt.rgba(0, 0, 0, 0.65)
                                visible: root.isDownloadingRemote && root.activePhotoPath === modelData.path

                                ColumnLayout {
                                    anchors.centerIn: parent
                                    spacing: 2
                                    Text {
                                        text: Theme.iconRefresh
                                        font.family: Theme.iconFont
                                        font.pixelSize: 14
                                        color: Theme.accentCyan
                                        Layout.alignment: Qt.AlignHCenter
                                    }
                                    Text {
                                        text: "Loading..."
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.family: Theme.monoFont
                                        color: Theme.accentCyan
                                        Layout.alignment: Qt.AlignHCenter
                                    }
                                }
                            }

                            // Format Badge (RAF, NEF, CR3, etc.)
                            Rectangle {
                                anchors.top: parent.top
                                anchors.right: parent.right
                                anchors.margins: 2
                                width: fmtText.implicitWidth + 6
                                height: 14
                                radius: 3
                                color: Qt.rgba(0, 0, 0, 0.75)

                                Text {
                                    id: fmtText
                                    anchors.centerIn: parent
                                    text: {
                                        var parts = String(modelData.name || "").split(".");
                                        return parts.length > 1 ? parts[parts.length - 1].toUpperCase() : "RAW";
                                    }
                                    textFormat: Text.PlainText
                                    font.pixelSize: 8
                                    font.weight: Font.Bold
                                    color: Theme.accentYellow
                                }
                            }
                        }
                    }

                    // Filename or Folder name
                    Text {
                        text: modelData.name || ""
                        textFormat: Text.PlainText
                        font.pixelSize: 9
                        font.family: Theme.monoFont
                        color: root.activePhotoPath === modelData.path ? Theme.accent : (modelData.is_dir ? Theme.accentCyan : Theme.textMuted)
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: function(mouse) {
                        if (modelData.is_dir) {
                            root.navigateFolder(modelData.path);
                        } else {
                            if (mouse.modifiers & Qt.ControlModifier) {
                                var sList = root.selectedPaths.slice();
                                var idx = sList.indexOf(modelData.path);
                                if (idx !== -1) {
                                    sList.splice(idx, 1);
                                } else {
                                    sList.push(modelData.path);
                                }
                                root.selectedPaths = sList;
                            } else if (mouse.modifiers & Qt.ShiftModifier) {
                                var sel = [root.activePhotoPath];
                                if (sel.indexOf(modelData.path) === -1) sel.push(modelData.path);
                                root.selectedPaths = sel;
                            } else {
                                root.selectedPaths = [modelData.path];
                                root.activePhotoPath = modelData.path;
                                root.selectPhoto(modelData.path, modelData.is_remote || false);
                            }
                        }
                    }
                }
            }
        }
    }
}
}
