import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

Rectangle {
    id: root
    width: 440
    implicitHeight: col.implicitHeight + 36
    radius: Theme.radiusLg
    color: Theme.bgSurface
    border.color: Theme.borderLight
    border.width: 1

    property string selectedFormat: "jxl"
    property int quality: 90
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

    signal closeRequested()
    signal doExport(var options)

    // Swallow clicks on the dialog body so they don't reach the overlay's close handler
    MouseArea {
        anchors.fill: parent
        z: -1
    }

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: 18
        spacing: 14

        // Header
        RowLayout {
            Layout.fillWidth: true
            RowLayout {
                spacing: 8
                Text {
                    text: Theme.iconExport
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(14)
                    color: Theme.accent
                }
                Text {
                    font.family: Theme.fontFamily
                    text: root.isBatchMode ? qsTr("Batch Export (%n Photos)", "", root.batchCount) : qsTr("Export Master Photo")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(15)
                    font.weight: Font.Bold
                    color: Theme.textMain
                }
            }
            Item { Layout.fillWidth: true }
            Rectangle {
                width: 24
                height: 24
                radius: 12
                color: Theme.bgCard
                border.color: Theme.border
                Text {
                    anchors.centerIn: parent
                    text: Theme.iconTimes
                    font.family: Theme.iconFont
                    color: Theme.textMuted
                    font.pixelSize: Theme.px(10)
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.closeRequested()
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Theme.border
        }

        // Format Selector
        ColumnLayout {
            spacing: 6
            Layout.fillWidth: true
            Text {
                font.family: Theme.fontFamily
                text: qsTr("MODERN OPEN FORMAT")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.weight: Font.DemiBold
                color: Theme.textDim
            }

            GridLayout {
                columns: 3
                Layout.fillWidth: true
                columnSpacing: 6
                rowSpacing: 6

                property var formats: [
                    { id: "jxl", name: "JPEG XL (.jxl)", badge: qsTr("Next-Gen 16-bit HDR") },
                    { id: "avif", name: "AVIF (.avif)", badge: qsTr("AV1 Web/Mobile") },
                    { id: "webp", name: "WebP (.webp)", badge: qsTr("Ultra Compact") },
                    { id: "tiff", name: qsTr("16-bit TIFF"), badge: qsTr("Master Print Lossless") },
                    { id: "png", name: "PNG (.png)", badge: qsTr("Graphic Lossless") },
                    { id: "jpeg", name: "JPEG (.jpg)", badge: qsTr("Universal MozJPEG") }
                ]

                Repeater {
                    model: parent.formats
                    delegate: Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 44
                        radius: Theme.radiusSm
                        color: root.selectedFormat === modelData.id ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2) : Theme.bgCard
                        border.color: root.selectedFormat === modelData.id ? Theme.accent : Theme.border
                        border.width: 1

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 6
                            spacing: 1
                            Text {
                                font.family: Theme.fontFamily
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.pixelSize: Theme.px(11)
                                font.weight: Font.Bold
                                color: root.selectedFormat === modelData.id ? Theme.accent : Theme.textMain
                            }
                            Text {
                                font.family: Theme.fontFamily
                                text: modelData.badge
                                textFormat: Text.PlainText
                                font.pixelSize: Theme.px(9)
                                color: Theme.textMuted
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.selectedFormat = modelData.id
                        }
                    }
                }
            }
        }

        // Quality Slider
        SliderGroup {
            title: qsTr("Export Compression Quality")
            from: 10
            to: 100
            value: root.quality
            defaultValue: 90
            suffix: "%"
            onSliderMoved: function(v) { root.quality = Math.round(v) }
        }

        // Sizing & Scale
        ColumnLayout {
            spacing: 6
            Layout.fillWidth: true
            Text {
                font.family: Theme.fontFamily
                text: qsTr("RESOLUTION & SCALING")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.weight: Font.DemiBold
                color: Theme.textDim
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 6

                property var scales: [
                    { label: qsTr("100% Full"), pct: 100, maxD: null },
                    { label: "75%", pct: 75, maxD: null },
                    { label: "50%", pct: 50, maxD: null },
                    { label: "4K (3840p)", pct: 100, maxD: 3840 },
                    { label: qsTr("Social (2048p)"), pct: 100, maxD: 2048 }
                ]

                Repeater {
                    model: parent.scales
                    delegate: Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 28
                        radius: Theme.r(4)
                        property bool isCur: root.scalePercent === modelData.pct && root.maxDimension === modelData.maxD
                        color: isCur ? Theme.accent : Theme.bgCard
                        border.color: isCur ? Theme.accent : Theme.border
                        border.width: 1

                        Text {
                            font.family: Theme.fontFamily
                            anchors.centerIn: parent
                            text: modelData.label
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            font.weight: Font.Medium
                            color: parent.isCur ? Theme.bgBase : Theme.textMain
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.scalePercent = modelData.pct
                                root.maxDimension = modelData.maxD
                            }
                        }
                    }
                }
            }
        }

        // Color Space / ICC Profile Selector
        ColumnLayout {
            spacing: 6
            Layout.fillWidth: true
            Text {
                font.family: Theme.fontFamily
                text: qsTr("COLOR SPACE & ICC PROFILE")
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.weight: Font.DemiBold
                color: Theme.textDim
            }

            GridLayout {
                columns: 2
                Layout.fillWidth: true
                columnSpacing: 6
                rowSpacing: 6

                property var profiles: [
                    { id: "sRGB", name: "sRGB", desc: qsTr("Universal Web / Social") },
                    { id: "DisplayP3", name: "Display P3", desc: qsTr("Apple / Wide OLED") },
                    { id: "Rec2020", name: "Rec.2020", desc: qsTr("Ultra-Wide Gamut HDR") },
                    { id: "ACEScg", name: "ACEScg (AP1)", desc: qsTr("Academy Color Standard") },
                    { id: "AdobeRGB1998", name: "Adobe RGB 1998", desc: qsTr("Pro Photography / Print") },
                    { id: "ProPhotoRGB", name: "ProPhoto RGB", desc: qsTr("16-Bit Master Archival") }
                ]

                Repeater {
                    model: parent.profiles
                    delegate: Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 34
                        radius: Theme.radiusSm
                        property bool isCur: root.selectedIcc === modelData.id
                        color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2) : Theme.bgCard
                        border.color: isCur ? Theme.accent : Theme.border
                        border.width: 1

                        RowLayout {
                            anchors.fill: parent
                            anchors.margins: 6
                            spacing: 6
                            Text {
                                font.family: Theme.fontFamily
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.pixelSize: Theme.px(11)
                                font.weight: Font.Bold
                                color: parent.parent.isCur ? Theme.accent : Theme.textMain
                            }
                            Item { Layout.fillWidth: true }
                            Text {
                                font.family: Theme.fontFamily
                                text: modelData.desc
                                textFormat: Text.PlainText
                                font.pixelSize: Theme.px(9)
                                color: Theme.textMuted
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.selectedIcc = modelData.id
                        }
                    }
                }
            }

            // How colours the destination profile cannot hold are mapped
            RowLayout {
                Layout.fillWidth: true
                spacing: 6

                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Intent")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    color: Theme.textDim
                    Layout.preferredWidth: 44
                }

                Repeater {
                    model: [
                        { id: "perceptual", name: qsTr("Perceptual"), tip: qsTr("Compresses the whole gamut; smooth, saturated colours keep their relations") },
                        { id: "relative_colorimetric", name: qsTr("Relative"), tip: qsTr("In-gamut colours stay exact; out-of-gamut colours clip to the edge") },
                        { id: "saturation", name: qsTr("Saturation"), tip: qsTr("Keeps colours vivid at the cost of accuracy; for graphics") },
                        { id: "absolute_colorimetric", name: qsTr("Absolute"), tip: qsTr("Exact colours including the source white point; for proofing") }
                    ]
                    delegate: Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 26
                        radius: Theme.radiusSm
                        property bool isCur: root.renderingIntent === modelData.id
                        color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2) : Theme.bgCard
                        border.color: isCur ? Theme.accent : Theme.border
                        border.width: 1

                        Text {
                            font.family: Theme.fontFamily
                            anchors.centerIn: parent
                            text: modelData.name
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            font.weight: parent.isCur ? Font.Bold : Font.Normal
                            color: parent.isCur ? Theme.accent : Theme.textMain
                        }

                        MouseArea {
                            id: intentMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.renderingIntent = modelData.id
                        }

                        ToolTip.visible: intentMouse.containsMouse
                        ToolTip.delay: 400
                        ToolTip.text: modelData.tip
                    }
                }
            }
        }

        // Privacy & Metadata Options
        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            // EXIF Camera Metadata Preservation
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 34
                radius: Theme.radiusSm
                color: Theme.bgCard
                border.color: root.preserveExif ? Theme.accent : Theme.border

                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 8
                    Text {
                        text: Theme.iconCamera
                        font.family: Theme.iconFont
                        font.pixelSize: Theme.px(12)
                        color: Theme.accent
                    }
                    Text {
                        font.family: Theme.fontFamily
                        text: qsTr("Preserve Camera EXIF")
                        textFormat: Text.PlainText
                        font.pixelSize: Theme.px(10)
                        color: Theme.textMain
                        Layout.fillWidth: true
                    }
                    CustomSwitch {
                        checked: root.preserveExif
                        activeColor: Theme.accent
                        onToggled: function(c) { root.preserveExif = c }
                    }
                }
            }

            // GPS Privacy Strip
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 34
                radius: Theme.radiusSm
                color: Theme.bgCard
                border.color: root.stripGps ? Theme.accentGreen : Theme.border

                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 8
                    Text {
                        text: Theme.iconShield
                        font.family: Theme.iconFont
                        font.pixelSize: Theme.px(12)
                        color: Theme.accentGreen
                    }
                    Text {
                        font.family: Theme.fontFamily
                        text: qsTr("Strip GPS (Privacy)")
                        textFormat: Text.PlainText
                        font.pixelSize: Theme.px(10)
                        color: Theme.textMain
                        Layout.fillWidth: true
                    }
                    CustomSwitch {
                        checked: root.stripGps
                        activeColor: Theme.accentGreen
                        onToggled: function(c) { root.stripGps = c }
                    }
                }
            }
        }

        // Watermark Selection Card
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 34
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: root.watermarkEnabled ? Theme.accent : Theme.border

            RowLayout {
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8
                Text {
                    text: Theme.iconImage
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(12)
                    color: root.watermarkEnabled ? Theme.accent : Theme.textDim
                }
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Apply Photography Watermark / EXIF Signature")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(11)
                    color: Theme.textMain
                    Layout.fillWidth: true
                }
                CustomSwitch {
                    checked: root.watermarkEnabled
                    activeColor: Theme.accent
                    onToggled: function(c) { root.watermarkEnabled = c }
                }
            }
        }

        // Export Button
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 42
            radius: Theme.radiusSm
            color: root.isExporting ? Theme.bgCard : Theme.accent
            border.color: Theme.accent

            RowLayout {
                anchors.centerIn: parent
                spacing: 8
                Text {
                    text: root.isExporting ? Theme.iconRefresh : Theme.iconExport
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(13)
                    color: root.isExporting ? Theme.textMuted : Theme.bgBase
                }
                Text {
                    font.family: Theme.fontFamily
                    text: root.isExporting ? (root.isBatchMode ? qsTr("Exporting Batch...") : qsTr("Exporting Full Resolution...")) : (root.isBatchMode ? qsTr("Start Parallel Batch Export (%n Photos)", "", root.batchCount) : qsTr("Start Export (%1 / %2)").arg(root.selectedFormat.toUpperCase()).arg(root.selectedIcc))
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(13)
                    font.weight: Font.Bold
                    color: root.isExporting ? Theme.textMuted : Theme.bgBase
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: root.isExporting ? Qt.ArrowCursor : Qt.PointingHandCursor
                enabled: !root.isExporting
                onClicked: {
                    var opts = {
                        "format": root.selectedFormat,
                        "quality": root.quality,
                        "scale_percent": root.scalePercent,
                        "max_dimension": root.maxDimension,
                        "strip_gps": root.stripGps,
                        "preserve_exif": root.preserveExif,
                        "icc_profile": root.selectedIcc,
                        "rendering_intent": root.renderingIntent,
                        "output_dir": root.localOutputDir,
                        // The options object carries the global "enabled" flag; force it on so
                        // this dialog's own switch actually takes effect.
                        "watermark": root.watermarkEnabled ? Object.assign({}, root.watermarkOptions, { "enabled": true }) : null
                    };
                    root.doExport(opts);
                }
            }
        }

        Text {
            font.family: Theme.fontFamily
            text: root.exportStatusText
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(11)
            color: root.exportStatusIsError ? Theme.highlightClip : Theme.accentGreen
            visible: root.exportStatusText !== ""
            wrapMode: Text.WrapAnywhere
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
        }
    }
}
