import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

// Export window: file format, bit depth, size, colour, metadata, and on the right
// the platform crop presets and the watermark. `host` is the main window, which owns
// the watermark settings and applies the platform presets to the recipe.
Rectangle {
    id: root
    width: Math.min(Theme.px(820), parent ? parent.width - 32 : Theme.px(820))
    height: Math.min(outer.implicitHeight + 2 * pad, parent ? parent.height - 32 : outer.implicitHeight + 2 * pad)
    radius: Theme.radiusLg
    color: Theme.bgSurface
    border.color: Theme.borderLight
    border.width: 1

    readonly property int pad: Theme.px(18)

    property var host: null
    // Added to the file name: photo_<tag>_omastudio.jpg
    property string nameTag: ""
    readonly property string exampleName: {
        var stem = host && host.activePhotoPath ? host.activePhotoPath.split("/").pop().replace(/\.[^.]*$/, "") : "photo";
        // Same rule as the engine: letters and digits, at most 24, lower case
        var tag = nameTag.replace(/[^A-Za-z0-9]/g, "").slice(0, 24).toLowerCase();
        var ext = selectedFormat === "jpeg" ? "jpg" : selectedFormat === "tiff" ? "tif" : selectedFormat;
        var suffix = "_omastudio.";
        var name = stem + (tag ? "_" + tag : "") + suffix + ext;
        return name;
    }

    Process {
        id: folderPick
        command: ["zenity", "--file-selection", "--directory", "--title=" + qsTr("Export folder")]
        onFinished: function(exitCode, output) {
            var p = String(output || "").trim();
            if (exitCode === 0 && p.length > 0) root.localOutputDir = p;
        }
    }
    property string selectedFormat: "jpeg"
    property int quality: 90
    // 0 = the format's best depth
    property int bitDepth: 0
    property string tiffCompression: "deflate"
    property int scalePercent: 100
    property var maxDimension: null
    property bool stripGps: true
    property bool preserveExif: true
    property string selectedIcc: "sRGB"
    property string renderingIntent: "relative_colorimetric"
    property string localOutputDir: "~/Pictures/OmaStudio_Exports"
    property bool isExporting: false
    property string exportStatusText: ""
    property bool exportStatusIsError: false
    property bool isBatchMode: false
    property int batchCount: 0
    property bool watermarkEnabled: false
    property var watermarkOptions: null
    property alias socialOptimizer: socialOpt
    // Connected cloud accounts; exported files are copied into `uploadFolder` of
    // `uploadAccount` ("" keeps them on this computer only)
    property var cloudAccounts: []
    property string uploadFolder: "OmaStudio"
    property string uploadAccount: ""
    signal uploadAccountPicked(string account)

    signal closeRequested()
    signal doExport(var options)

    readonly property var formats: [
        { id: "jpeg", name: "JPEG", note: qsTr("8-bit"), tip: qsTr("Opens everywhere") },
        { id: "png", name: "PNG", note: qsTr("16-bit"), tip: qsTr("Lossless") },
        { id: "tiff", name: "TIFF", note: qsTr("16-bit"), tip: qsTr("For print and archive") },
        { id: "webp", name: "WebP", note: qsTr("8-bit"), tip: qsTr("Small files for the web") },
        { id: "avif", name: "AVIF", note: qsTr("12-bit"), tip: qsTr("Small files with high bit depth") },
        { id: "heic", name: "HEIF", note: qsTr("10-bit"), tip: qsTr("HEVC, as used by phones") },
        { id: "jxl", name: "JPEG XL", note: qsTr("16-bit"), tip: qsTr("Small files, lossless at quality 100") },
        { id: "exr", name: "OpenEXR", note: qsTr("Float"), tip: qsTr("Scene-linear, for compositing and grading") }
    ]

    // Mirrors effective_bit_depth() in the engine
    function depthsFor(fmt) {
        switch (fmt) {
        case "png": case "tiff": case "jxl": return [8, 16];
        case "avif": return [8, 10, 12];
        case "heic": return [8, 10, 12];
        case "exr": return [16, 32];
        default: return [8];
        }
    }
    function bestDepth(fmt) {
        switch (fmt) {
        case "png": case "tiff": case "jxl": return 16;
        case "avif": return 12;
        case "heic": return 10;
        case "exr": return 16;
        default: return 8;
        }
    }
    readonly property int effectiveDepth: depthsFor(selectedFormat).indexOf(bitDepth) >= 0 ? bitDepth : bestDepth(selectedFormat)
    readonly property bool hasQuality: ["jpeg", "webp", "avif", "heic", "jxl"].indexOf(selectedFormat) >= 0
    readonly property bool hasLossless: ["webp", "avif", "heic", "jxl"].indexOf(selectedFormat) >= 0
    readonly property bool statusIsError: exportStatusIsError || exportStatusText.indexOf("[ERR]") === 0

    onSelectedFormatChanged: bitDepth = 0

    function startExport() {
        if (root.isExporting) return;
        root.doExport({
            "format": root.selectedFormat,
            "quality": root.quality,
            "bit_depth": root.effectiveDepth,
            "tiff_compression": root.tiffCompression,
            "scale_percent": root.scalePercent,
            "max_dimension": root.maxDimension,
            "strip_gps": root.stripGps,
            "preserve_exif": root.preserveExif,
            "icc_profile": root.selectedIcc,
            "rendering_intent": root.renderingIntent,
            "output_dir": root.localOutputDir,
            "name_tag": root.nameTag.length > 0 ? root.nameTag : null,
            // The options object carries the global "enabled" flag; force it on so
            // the watermark switch actually takes effect.
            "watermark": root.watermarkEnabled ? Object.assign({}, root.watermarkOptions, { "enabled": true }) : null
        });
    }

    // Swallow clicks on the dialog body so they don't reach the overlay's close handler
    MouseArea {
        anchors.fill: parent
        z: -1
    }

    component Caption: Text {
        font.family: Theme.fontFamily
        textFormat: Text.PlainText
        font.pixelSize: Theme.px(11)
        font.weight: Font.DemiBold
        color: Theme.textMuted
    }

    component Hint: Text {
        font.family: Theme.fontFamily
        textFormat: Text.PlainText
        font.pixelSize: Theme.px(10)
        color: Theme.textDim
        wrapMode: Text.WordWrap
        Layout.fillWidth: true
    }

    // A row of equal buttons; `options` is [{ id, label, tip? }]
    component Segmented: RowLayout {
        id: seg
        property var options: []
        property var current
        signal picked(var id)
        Layout.fillWidth: true
        spacing: Theme.px(4)
        Repeater {
            model: seg.options
            delegate: Rectangle {
                required property var modelData
                readonly property bool isCur: seg.current === modelData.id
                Layout.fillWidth: true
                implicitHeight: Theme.px(26)
                radius: Theme.radiusSm
                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : (segMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                border.color: isCur ? Theme.accent : Theme.border
                Text {
                    anchors.centerIn: parent
                    text: modelData.label
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    font.weight: parent.isCur ? Font.DemiBold : Font.Normal
                    color: parent.isCur ? Theme.accent : Theme.textMain
                }
                MouseArea {
                    id: segMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: seg.picked(modelData.id)
                }
                ToolTip.visible: segMouse.containsMouse && modelData.tip !== undefined
                ToolTip.delay: 400
                ToolTip.text: modelData.tip || ""
            }
        }
    }

    component SwitchRow: RowLayout {
        id: sw
        property string label
        property bool checked
        signal toggled(bool on)
        Layout.fillWidth: true
        spacing: Theme.px(8)
        Text {
            text: sw.label
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(11)
            color: Theme.textMain
            Layout.fillWidth: true
        }
        CustomSwitch {
            checked: sw.checked
            onToggled: function(c) { sw.toggled(c) }
        }
    }

    ColumnLayout {
        id: outer
        anchors.fill: parent
        anchors.margins: root.pad
        spacing: Theme.px(14)

        // Header
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.px(8)
            Text {
                font.family: Theme.fontFamily
                text: root.isBatchMode ? qsTr("Export %n photos", "", root.batchCount) : qsTr("Export")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(15)
                font.weight: Font.DemiBold
                color: Theme.textMain
                Layout.fillWidth: true
            }
            Rectangle {
                implicitWidth: Theme.px(26)
                implicitHeight: Theme.px(26)
                radius: Theme.radiusSm
                color: closeMouse.containsMouse ? Theme.bgCardHover : "transparent"
                Text {
                    anchors.centerIn: parent
                    text: Theme.iconTimes
                    font.family: Theme.iconFont
                    color: Theme.textMuted
                    font.pixelSize: Theme.px(11)
                }
                MouseArea {
                    id: closeMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.closeRequested()
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Close")
                }
            }
        }

        Flickable {
            id: flick
            Layout.fillWidth: true
            Layout.fillHeight: true
            implicitHeight: content.implicitHeight
            contentHeight: content.implicitHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: flick.contentHeight > flick.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }

            RowLayout {
                id: content
                width: flick.width
                spacing: Theme.px(20)

                // LEFT: file
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.px(10)

                    Caption { text: qsTr("Format") }
                    GridLayout {
                        columns: 4
                        Layout.fillWidth: true
                        columnSpacing: Theme.px(6)
                        rowSpacing: Theme.px(6)
                        Repeater {
                            model: root.formats
                            delegate: Rectangle {
                                required property var modelData
                                readonly property bool isCur: root.selectedFormat === modelData.id
                                Layout.fillWidth: true
                                implicitHeight: Theme.px(42)
                                radius: Theme.radiusSm
                                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : (fmtMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                                border.color: isCur ? Theme.accent : Theme.border
                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: Theme.px(6)
                                    spacing: 0
                                    Text {
                                        text: modelData.name
                                        textFormat: Text.PlainText
                                        font.family: Theme.fontFamily
                                        font.pixelSize: Theme.px(11)
                                        font.weight: Font.DemiBold
                                        color: parent.parent.isCur ? Theme.accent : Theme.textMain
                                    }
                                    Text {
                                        text: modelData.note
                                        textFormat: Text.PlainText
                                        font.family: Theme.fontFamily
                                        font.pixelSize: Theme.px(9)
                                        color: Theme.textDim
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                }
                                MouseArea {
                                    id: fmtMouse
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    ToolTip.visible: containsMouse
                                    ToolTip.delay: 500
                                    ToolTip.text: modelData.tip
                                    onClicked: root.selectedFormat = modelData.id
                                }
                            }
                        }
                    }

                    SliderGroup {
                        visible: root.hasQuality
                        title: qsTr("Quality")
                        from: 10
                        to: 100
                        value: root.quality
                        defaultValue: 90
                        onSliderMoved: function(v) { root.quality = Math.round(v) }
                    }
                    Hint {
                        visible: root.hasLossless
                        text: qsTr("100 is lossless.")
                    }

                    Caption { text: qsTr("Bit depth") }
                    Segmented {
                        options: root.depthsFor(root.selectedFormat).map(function(d) {
                            return { "id": d, "label": (root.selectedFormat === "exr" ? (d === 16 ? qsTr("Half float") : qsTr("Full float")) : qsTr("%1-bit").arg(d)) };
                        })
                        current: root.effectiveDepth
                        onPicked: function(id) { root.bitDepth = id }
                    }

                    Caption { visible: root.selectedFormat === "tiff"; text: qsTr("Compression") }
                    Segmented {
                        visible: root.selectedFormat === "tiff"
                        options: [{ "id": "none", "label": qsTr("None") }, { "id": "lzw", "label": "LZW" }, { "id": "deflate", "label": "ZIP" }]
                        current: root.tiffCompression
                        onPicked: function(id) { root.tiffCompression = id }
                    }

                    Caption { text: qsTr("Size") }
                    Segmented {
                        options: [
                            { "id": "100", "label": qsTr("Full") },
                            { "id": "75", "label": "75 %" },
                            { "id": "50", "label": "50 %" },
                            { "id": "3840", "label": qsTr("3840 px"), "tip": qsTr("Long edge at most 3840 px") },
                            { "id": "2048", "label": qsTr("2048 px"), "tip": qsTr("Long edge at most 2048 px") }
                        ].concat(root.maxDimension && root.maxDimension !== 3840 && root.maxDimension !== 2048
                                 ? [{ "id": String(root.maxDimension), "label": root.maxDimension + " px" }] : [])
                        current: root.maxDimension ? String(root.maxDimension) : String(root.scalePercent)
                        onPicked: function(id) {
                            var n = Number(id);
                            if (n > 100) { root.scalePercent = 100; root.maxDimension = n; }
                            else { root.scalePercent = n; root.maxDimension = null; }
                        }
                    }

                    Caption { text: qsTr("Colour profile") }
                    GridLayout {
                        columns: 3
                        Layout.fillWidth: true
                        columnSpacing: Theme.px(6)
                        rowSpacing: Theme.px(6)
                        Repeater {
                            model: [
                                { id: "sRGB", name: "sRGB", desc: qsTr("Web, screens") },
                                { id: "DisplayP3", name: "Display P3", desc: qsTr("Modern screens") },
                                { id: "AdobeRGB1998", name: "Adobe RGB", desc: qsTr("Print") },
                                { id: "ProPhotoRGB", name: "ProPhoto RGB", desc: qsTr("Editing, archive") },
                                { id: "Rec2020", name: "Rec. 2020", desc: qsTr("HDR, video") },
                                { id: "ACEScg", name: "ACEScg", desc: qsTr("Linear, VFX") }
                            ]
                            delegate: Rectangle {
                                required property var modelData
                                readonly property bool isCur: root.selectedIcc === modelData.id
                                Layout.fillWidth: true
                                implicitHeight: Theme.px(38)
                                radius: Theme.radiusSm
                                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : (iccMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                                border.color: isCur ? Theme.accent : Theme.border
                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: Theme.px(6)
                                    spacing: 0
                                    Text {
                                        text: modelData.name
                                        textFormat: Text.PlainText
                                        font.family: Theme.fontFamily
                                        font.pixelSize: Theme.px(11)
                                        font.weight: Font.DemiBold
                                        color: parent.parent.isCur ? Theme.accent : Theme.textMain
                                    }
                                    Text {
                                        text: modelData.desc
                                        textFormat: Text.PlainText
                                        font.family: Theme.fontFamily
                                        font.pixelSize: Theme.px(9)
                                        color: Theme.textDim
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                }
                                MouseArea {
                                    id: iccMouse
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.selectedIcc = modelData.id
                                }
                            }
                        }
                    }
                    Hint {
                        visible: root.selectedFormat === "exr"
                        text: qsTr("OpenEXR is written scene-linear in the chosen primaries.")
                    }

                    Caption { text: qsTr("Rendering intent") }
                    Segmented {
                        options: [
                            { "id": "perceptual", "label": qsTr("Perceptual"), "tip": qsTr("Compresses the whole gamut; colour relations are kept") },
                            { "id": "relative_colorimetric", "label": qsTr("Relative"), "tip": qsTr("In-gamut colours stay exact; out-of-gamut colours clip to the edge") },
                            { "id": "saturation", "label": qsTr("Saturation"), "tip": qsTr("Keeps colours vivid at the cost of accuracy; for graphics") },
                            { "id": "absolute_colorimetric", "label": qsTr("Absolute"), "tip": qsTr("Exact colours including the source white point; for proofing") }
                        ]
                        current: root.renderingIntent
                        onPicked: function(id) { root.renderingIntent = id }
                    }

                    Caption { text: qsTr("Metadata") }
                    SwitchRow {
                        label: qsTr("Keep camera metadata (EXIF)")
                        checked: root.preserveExif
                        onToggled: function(c) { root.preserveExif = c }
                    }
                    SwitchRow {
                        label: qsTr("Remove location (GPS)")
                        checked: root.stripGps
                        enabled: root.preserveExif
                        opacity: enabled ? 1 : 0.5
                        onToggled: function(c) { root.stripGps = c }
                    }
                }

                Rectangle {
                    Layout.fillHeight: true
                    implicitWidth: 1
                    color: Theme.border
                }

                // RIGHT: presentation
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.px(10)

                    Caption { text: qsTr("Prepare for") }
                    Hint { text: qsTr("Sets the crop and output sharpening for a platform. Click it again to go back to your own.") }
                    SocialOptimizer {
                        id: socialOpt
                        embedded: true
                        Layout.fillWidth: true
                        Layout.fillHeight: false
                        onTriggerSocialOptimize: function(code) { if (root.host) root.host.triggerSocial(code) }
                        onResetSocial: if (root.host) root.host.resetSocialOptimization()
                    }

                    Caption { text: qsTr("Watermark") }
                    WatermarkConfig {
                        embedded: true
                        Layout.fillWidth: true
                        Layout.fillHeight: false
                        watermarkEnabled: root.host ? root.host.watermarkEnabled : false
                        watermarkType: root.host ? root.host.watermarkType : "text"
                        text: root.host ? root.host.watermarkText : ""
                        logoPath: root.host ? root.host.watermarkLogoPath : ""
                        positionIndex: root.host ? root.host.watermarkPositionIndex : 8
                        watermarkOpacity: root.host ? root.host.watermarkOpacity : 0.85
                        size: root.host ? root.host.watermarkSize : 14
                        margin: root.host ? root.host.watermarkMargin : 24
                        colorHex: root.host ? root.host.watermarkColorHex : "#ffffff"
                        dropShadow: root.host ? root.host.watermarkDropShadow : true
                        onWatermarkChanged: function(opts) {
                            var h = root.host;
                            if (!h) return;
                            h.watermarkEnabled = opts.enabled;
                            h.watermarkType = opts.watermark_type;
                            h.watermarkText = opts.text;
                            h.watermarkLogoPath = opts.logo_path || "";
                            h.watermarkPositionIndex = opts.position_index;
                            h.watermarkOpacity = opts.opacity;
                            h.watermarkSize = opts.size;
                            h.watermarkMargin = opts.margin;
                            h.watermarkColorHex = opts.color;
                            h.watermarkDropShadow = opts.drop_shadow;
                        }
                    }

                    Caption { text: qsTr("Folder") }
                    RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.px(6)
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: Theme.px(30)
                        radius: Theme.radiusSm
                        color: Theme.bgCard
                        border.color: folderInput.activeFocus ? Theme.accent : Theme.border
                        BoundedTextInput {
                            id: folderInput
                            anchors.fill: parent
                            anchors.leftMargin: Theme.px(8)
                            anchors.rightMargin: Theme.px(8)
                            verticalAlignment: TextEdit.AlignVCenter
                            font.pixelSize: Theme.px(11)
                            color: Theme.textMain
                            maxLength: 1024
                            text: root.localOutputDir
                            onTextChanged: if (text.length > 0) root.localOutputDir = text
                        }
                    }
                    Rectangle {
                        implicitWidth: browseText.implicitWidth + Theme.px(20)
                        implicitHeight: Theme.px(30)
                        radius: Theme.radiusSm
                        color: browseMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                        border.color: Theme.border
                        Text {
                            id: browseText
                            anchors.centerIn: parent
                            text: qsTr("Choose…")
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(11)
                            color: Theme.textMain
                        }
                        MouseArea {
                            id: browseMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: if (!folderPick.running) folderPick.running = true
                        }
                    }
                    }

                    Caption { text: qsTr("File name") }
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: Theme.px(30)
                        radius: Theme.radiusSm
                        color: Theme.bgCard
                        border.color: nameTagInput.activeFocus ? Theme.accent : Theme.border
                        BoundedTextInput {
                            id: nameTagInput
                            anchors.fill: parent
                            anchors.leftMargin: Theme.px(8)
                            anchors.rightMargin: Theme.px(8)
                            verticalAlignment: TextEdit.AlignVCenter
                            font.pixelSize: Theme.px(11)
                            color: Theme.textMain
                            maxLength: 64
                            text: root.nameTag
                            onTextChanged: root.nameTag = text
                        }
                    }
                    Hint {
                        text: qsTr("Saved as %1").arg(root.exampleName)
                        Layout.fillWidth: true
                        elide: Text.ElideMiddle
                    }

                    Caption {
                        visible: root.cloudAccounts.length > 0
                        text: qsTr("Upload to")
                    }
                    Flow {
                        visible: root.cloudAccounts.length > 0
                        Layout.fillWidth: true
                        spacing: Theme.px(4)
                        Repeater {
                            model: [{ "id": "", "label": qsTr("This computer only") }].concat(root.cloudAccounts)
                            delegate: Rectangle {
                                required property var modelData
                                readonly property bool isCur: root.uploadAccount === modelData.id
                                implicitWidth: upText.implicitWidth + Theme.px(16)
                                implicitHeight: Theme.px(26)
                                radius: Theme.radiusSm
                                color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : (upMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                                border.color: isCur ? Theme.accent : Theme.border
                                Text {
                                    id: upText
                                    anchors.centerIn: parent
                                    text: modelData.label
                                    textFormat: Text.PlainText
                                    font.family: Theme.fontFamily
                                    font.pixelSize: Theme.px(10)
                                    font.weight: parent.isCur ? Font.DemiBold : Font.Normal
                                    color: parent.isCur ? Theme.accent : Theme.textMain
                                }
                                MouseArea {
                                    id: upMouse
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.uploadAccountPicked(modelData.id)
                                }
                                ToolTip.visible: upMouse.containsMouse && (modelData.user || "") !== ""
                                ToolTip.delay: 400
                                ToolTip.text: modelData.user || ""
                            }
                        }
                    }
                    Hint {
                        visible: root.cloudAccounts.length > 0 && root.uploadAccount !== ""
                        text: qsTr("After export, the files are also copied into the %1 folder of the account (on GitHub: the private %1 repository).").arg(root.uploadFolder)
                    }
                    Item { Layout.fillHeight: true }
                }
            }
        }

        // Footer
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.px(10)

            Text {
                text: root.exportStatusText.replace(/^\[(OK|ERR)\]\s*/, "")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(11)
                color: root.statusIsError ? Theme.highlightClip : Theme.textMuted
                elide: Text.ElideMiddle
                Layout.fillWidth: true
            }

            Rectangle {
                implicitWidth: cancelText.implicitWidth + Theme.px(28)
                implicitHeight: Theme.px(34)
                radius: Theme.radiusSm
                color: cancelMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border
                Text {
                    id: cancelText
                    anchors.centerIn: parent
                    text: root.isExporting ? qsTr("Hide") : qsTr("Cancel")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(12)
                    color: Theme.textMain
                }
                MouseArea {
                    id: cancelMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.closeRequested()
                }
            }

            Rectangle {
                implicitWidth: exportText.implicitWidth + Theme.px(36)
                implicitHeight: Theme.px(34)
                radius: Theme.radiusSm
                color: root.isExporting ? Theme.bgCard : (exportMouse.containsMouse ? Qt.lighter(Theme.accent, 1.1) : Theme.accent)
                border.color: Theme.accent
                Text {
                    id: exportText
                    anchors.centerIn: parent
                    text: root.isExporting ? qsTr("Exporting…") : (root.isBatchMode ? qsTr("Export %n photos", "", root.batchCount) : qsTr("Export"))
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(12)
                    font.weight: Font.DemiBold
                    color: root.isExporting ? Theme.textMuted : Theme.onAccent
                }
                MouseArea {
                    id: exportMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    enabled: !root.isExporting
                    cursorShape: root.isExporting ? Qt.ArrowCursor : Qt.PointingHandCursor
                    onClicked: root.startExport()
                }
            }
        }
    }

    Keys.onReturnPressed: root.startExport()
    Keys.onEscapePressed: root.closeRequested()
    onVisibleChanged: if (visible) forceActiveFocus()
}
