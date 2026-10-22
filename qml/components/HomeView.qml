import QtQuick
import QtQuick.Layouts
import "../theme"

// Start screen shown in the canvas area while no photo is being edited:
// a carousel of the current folder's photos plus the main entry points.
Rectangle {
    id: root
    color: Theme.bgDark
    clip: true

    property var photoList: []
    property string currentFolder: "~/Pictures"
    property bool isCatalogMode: false
    property bool isListingFolder: false
    // Photo left open in the editor when the user came back to the start screen
    property string activePhotoPath: ""

    signal openPhoto(string path)
    signal openFileRequested()
    signal openFolderRequested()
    signal catalogRequested()
    signal resumeRequested()

    // Folders and unreadable files are left out of the carousel
    readonly property var slides: {
        var out = [];
        var list = root.photoList || [];
        for (var i = 0; i < list.length; i++) {
            var it = list[i];
            if (!it.is_dir && it.thumbnail && it.thumbnail !== "") out.push(it);
        }
        return out;
    }
    readonly property int photoCount: {
        var n = 0;
        var list = root.photoList || [];
        for (var i = 0; i < list.length; i++) {
            if (!list[i].is_dir) n++;
        }
        return n;
    }
    readonly property bool compact: width < 720

    onSlidesChanged: carousel.positionViewAtBeginning()

    function fileName(p) {
        var parts = String(p || "").split("/");
        return parts[parts.length - 1];
    }

    function extOf(name) {
        var parts = String(name || "").split(".");
        return parts.length > 1 ? parts[parts.length - 1].toUpperCase() : "RAW";
    }

    function step(delta) {
        if (root.slides.length === 0) return;
        carousel.currentIndex = (carousel.currentIndex + delta + root.slides.length) % root.slides.length;
    }

    Shortcut {
        sequence: "Left"
        enabled: root.visible && root.slides.length > 1
        onActivated: root.step(-1)
    }
    Shortcut {
        sequence: "Right"
        enabled: root.visible && root.slides.length > 1
        onActivated: root.step(1)
    }
    Shortcut {
        sequences: ["Return", "Enter"]
        enabled: root.visible && root.slides.length > 0
        onActivated: root.openPhoto(root.slides[carousel.currentIndex].path)
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: root.compact ? 20 : 32
        spacing: root.compact ? 16 : 22

        // HEADER: product line and the folder the carousel is showing
        RowLayout {
            Layout.fillWidth: true
            spacing: 16

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                Text {
                    text: "OmaStudio"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: root.compact ? 20 : 24
                    font.weight: Font.Bold
                    color: Theme.textMain
                }
                Text {
                    text: "The Complete Medium Format RAW Editor for Omarchy"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 12
                    color: Theme.textMuted
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }

            // Source summary pill
            Rectangle {
                Layout.alignment: Qt.AlignBottom
                Layout.maximumWidth: root.width * 0.45
                implicitWidth: sourceRow.implicitWidth + 20
                implicitHeight: 26
                radius: 13
                color: Theme.bgSurface
                border.color: Theme.border
                border.width: 1
                visible: !root.compact

                RowLayout {
                    id: sourceRow
                    anchors.centerIn: parent
                    spacing: 8
                    Text {
                        text: root.isCatalogMode ? "\uf009" : Theme.iconFolder
                        font.family: Theme.iconFont
                        font.pixelSize: 10
                        color: Theme.textMuted
                    }
                    Text {
                        text: root.isCatalogMode ? "Studio Catalog" : root.currentFolder
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: 10
                        color: Theme.textMuted
                        elide: Text.ElideMiddle
                        Layout.maximumWidth: 260
                    }
                    Rectangle { width: 1; height: 12; color: Theme.border }
                    Text {
                        text: root.isListingFolder ? "Scanning…" : (root.photoCount === 1 ? "1 photo" : root.photoCount + " photos")
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: 10
                        color: Theme.textMuted
                    }
                }
            }
        }

        // CAROUSEL
        Rectangle {
            id: stage
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 200
            radius: Theme.radiusLg
            color: Theme.bgBase
            border.color: Theme.border
            border.width: 1
            clip: true

            HoverHandler { id: stageHover }

            ListView {
                id: carousel
                anchors.fill: parent
                anchors.margins: 1
                orientation: ListView.Horizontal
                snapMode: ListView.SnapOneItem
                highlightRangeMode: ListView.StrictlyEnforceRange
                preferredHighlightBegin: 0
                preferredHighlightEnd: width
                highlightMoveDuration: 420
                boundsBehavior: Flickable.StopAtBounds
                clip: true
                model: root.slides
                visible: root.slides.length > 0

                delegate: Item {
                    id: slide
                    width: carousel.width
                    height: carousel.height
                    readonly property bool isCurrent: ListView.isCurrentItem

                    // Ambient backdrop: same frame, cropped and dimmed
                    Image {
                        anchors.fill: parent
                        source: "file://" + modelData.thumbnail
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        cache: true
                        opacity: 0.18
                        sourceSize.width: 256
                    }
                    Rectangle {
                        anchors.fill: parent
                        color: Qt.rgba(0, 0, 0, Theme.isDarkTheme ? 0.35 : 0.15)
                    }

                    // The photo itself, never cropped
                    Image {
                        id: hero
                        anchors.fill: parent
                        anchors.topMargin: 20
                        anchors.bottomMargin: 64
                        anchors.leftMargin: 56
                        anchors.rightMargin: 56
                        source: "file://" + modelData.thumbnail
                        fillMode: Image.PreserveAspectFit
                        asynchronous: true
                        cache: true
                        smooth: true
                        mipmap: true
                        opacity: status === Image.Ready ? 1.0 : 0.0
                        Behavior on opacity { NumberAnimation { duration: 240 } }
                    }

                    // Caption bar
                    Rectangle {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        height: 56
                        gradient: Gradient {
                            GradientStop { position: 0.0; color: Qt.rgba(0, 0, 0, 0.0) }
                            GradientStop { position: 1.0; color: Qt.rgba(0, 0, 0, 0.55) }
                        }

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 20
                            anchors.rightMargin: 20
                            spacing: 10

                            Rectangle {
                                implicitWidth: extText.implicitWidth + 10
                                implicitHeight: 18
                                radius: 4
                                color: Qt.rgba(0, 0, 0, 0.6)
                                border.color: Qt.rgba(1, 1, 1, 0.12)
                                border.width: 1
                                Text {
                                    id: extText
                                    anchors.centerIn: parent
                                    text: root.extOf(modelData.name)
                                    textFormat: Text.PlainText
                                    font.family: Theme.monoFont
                                    font.pixelSize: 9
                                    font.weight: Font.Bold
                                    color: Theme.accentYellow
                                }
                            }
                            Text {
                                text: modelData.name || ""
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                                color: "#ffffff"
                                elide: Text.ElideMiddle
                                Layout.fillWidth: true
                            }
                            Text {
                                text: "Click or press Enter to edit"
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: 10
                                color: Qt.rgba(1, 1, 1, 0.78)
                                visible: slide.isCurrent && !root.compact
                            }
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.openPhoto(modelData.path)
                    }
                }
            }

            // Auto-advance, paused while the pointer is over the stage
            Timer {
                interval: 6000
                repeat: true
                running: root.visible && root.slides.length > 1 && !stageHover.hovered && !carousel.moving
                onTriggered: root.step(1)
            }

            // Previous / next
            Repeater {
                model: [-1, 1]
                delegate: Rectangle {
                    required property int modelData
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.verticalCenterOffset: -22
                    x: modelData < 0 ? 12 : stage.width - width - 12
                    width: 34
                    height: 34
                    radius: 17
                    visible: root.slides.length > 1
                    opacity: stageHover.hovered ? 1.0 : 0.0
                    Behavior on opacity { NumberAnimation { duration: 160 } }
                    color: navMouse.containsMouse ? Qt.rgba(0, 0, 0, 0.8) : Qt.rgba(0, 0, 0, 0.55)
                    border.color: Qt.rgba(1, 1, 1, 0.18)
                    border.width: 1

                    Text {
                        anchors.centerIn: parent
                        text: modelData < 0 ? "" : ""
                        font.family: Theme.iconFont
                        font.pixelSize: 12
                        color: "#ffffff"
                    }
                    MouseArea {
                        id: navMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.step(modelData)
                    }
                }
            }

            // Position: dots for short lists, a counter for long ones
            Row {
                anchors.top: parent.top
                anchors.right: parent.right
                anchors.margins: 14
                spacing: 6
                visible: root.slides.length > 1 && root.slides.length <= 12

                Repeater {
                    model: root.slides.length
                    delegate: Rectangle {
                        required property int index
                        width: index === carousel.currentIndex ? 18 : 6
                        height: 6
                        radius: 3
                        color: index === carousel.currentIndex ? Theme.accent : Qt.rgba(1, 1, 1, 0.35)
                        Behavior on width { NumberAnimation { duration: 200 } }
                        MouseArea {
                            anchors.fill: parent
                            anchors.margins: -4
                            cursorShape: Qt.PointingHandCursor
                            onClicked: carousel.currentIndex = index
                        }
                    }
                }
            }
            Rectangle {
                anchors.top: parent.top
                anchors.right: parent.right
                anchors.margins: 12
                visible: root.slides.length > 12
                implicitWidth: counterText.implicitWidth + 14
                implicitHeight: 20
                radius: 10
                color: Qt.rgba(0, 0, 0, 0.6)
                Text {
                    id: counterText
                    anchors.centerIn: parent
                    text: (carousel.currentIndex + 1) + " / " + root.slides.length
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 10
                    color: "#ffffff"
                }
            }

            // Empty folder / still scanning
            ColumnLayout {
                anchors.centerIn: parent
                width: Math.min(parent.width - 48, 420)
                spacing: 10
                visible: root.slides.length === 0

                Rectangle {
                    Layout.alignment: Qt.AlignHCenter
                    width: 56
                    height: 56
                    radius: 28
                    color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.10)
                    border.color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.45)
                    border.width: 1
                    Text {
                        anchors.centerIn: parent
                        text: root.isListingFolder ? Theme.iconRefresh : Theme.iconCamera
                        font.family: Theme.iconFont
                        font.pixelSize: 22
                        color: Theme.accent
                    }
                }
                Text {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: root.isListingFolder ? "Scanning photos…" : "No photos to show yet"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 14
                    font.weight: Font.DemiBold
                    color: Theme.textMain
                }
                Text {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    visible: !root.isListingFolder
                    text: "Open a RAW file or a folder, or drop one here. Photos from the current folder appear in this carousel."
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 11
                    color: Theme.textMuted
                    wrapMode: Text.WordWrap
                    lineHeight: 1.3
                }
            }
        }

        // ENTRY POINTS
        GridLayout {
            id: actionGrid
            Layout.fillWidth: true
            columns: root.compact ? 2 : items.length
            columnSpacing: 12
            rowSpacing: 12

            property var items: {
                var a = [];
                if (root.activePhotoPath !== "") {
                    a.push({ key: "resume", icon: Theme.iconSliders, title: "Continue editing",
                             detail: root.fileName(root.activePhotoPath), hint: "", accent: true });
                }
                a.push({ key: "file", icon: Theme.iconImage, title: "Open RAW file",
                         detail: "Single photo", hint: "Ctrl+O", accent: root.activePhotoPath === "" });
                a.push({ key: "folder", icon: Theme.iconFolder, title: "Open folder",
                         detail: "Browse a whole shoot", hint: "", accent: false });
                a.push({ key: "catalog", icon: "\uf009", title: "Studio Catalog",
                         detail: "Rated and flagged photos", hint: "", accent: false });
                return a;
            }

            Repeater {
                model: actionGrid.items
                delegate: Rectangle {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    implicitHeight: 64
                    radius: Theme.radiusMd
                    color: cardMouse.containsMouse ? Theme.bgCardHover : Theme.bgSurface
                    border.color: modelData.accent ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.55)
                                                   : (cardMouse.containsMouse ? Theme.borderLight : Theme.border)
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 120 } }

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 14
                        anchors.rightMargin: 14
                        spacing: 12

                        Rectangle {
                            width: 34
                            height: 34
                            radius: 8
                            color: modelData.accent ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.16) : Theme.bgCard
                            Text {
                                anchors.centerIn: parent
                                text: modelData.icon
                                font.family: Theme.iconFont
                                font.pixelSize: 14
                                color: modelData.accent ? Theme.accent : Theme.textMuted
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: modelData.title
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                                color: Theme.textMain
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                text: modelData.detail
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: 11
                                color: Theme.textMuted
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }

                        Rectangle {
                            visible: modelData.hint !== ""
                            implicitWidth: hintText.implicitWidth + 10
                            implicitHeight: 18
                            radius: 4
                            color: Theme.bgDark
                            border.color: Theme.border
                            border.width: 1
                            Text {
                                id: hintText
                                anchors.centerIn: parent
                                text: modelData.hint
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: 9
                                color: Theme.textMuted
                            }
                        }
                    }

                    MouseArea {
                        id: cardMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            if (modelData.key === "resume") root.resumeRequested();
                            else if (modelData.key === "file") root.openFileRequested();
                            else if (modelData.key === "folder") root.openFolderRequested();
                            else if (modelData.key === "catalog") root.catalogRequested();
                        }
                    }
                }
            }
        }

        // FOOTER: formats and drag-and-drop hint
        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            Text {
                text: "RAF · 3FR · DNG · NEF · CR3 · ARW · ORF · RW2 · TIFF"
                textFormat: Text.PlainText
                font.family: Theme.monoFont
                font.pixelSize: 10
                font.letterSpacing: 0.6
                color: Theme.textMuted
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            Text {
                visible: !root.compact
                text: "Drop a photo or folder here  ·  ← → to browse"
                textFormat: Text.PlainText
                font.family: Theme.monoFont
                font.pixelSize: 10
                color: Theme.textMuted
            }
        }
    }
}
