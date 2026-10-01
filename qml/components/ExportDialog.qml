import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

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
    property bool uploadToGdrive: false
    property string localOutputDir: "~/Pictures/OmaStudio_Exports"
    property string gdriveFolder: "Photos/Exports"
    property bool isExporting: false
    property string exportStatusText: ""
    property bool isBatchMode: false
    property int batchCount: 0
    property bool watermarkEnabled: false
    property var watermarkOptions: null

    signal closeRequested()
    signal doExport(var options)

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
                    font.pixelSize: 14
                    color: Theme.accent
                }
                Text {
                    text: root.isBatchMode ? ("Batch Export (" + root.batchCount + " Photos)") : "Export Master Photo"
                    textFormat: Text.PlainText
                    font.pixelSize: 15
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
                    font.pixelSize: 10
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
                text: "MODERN OPEN FORMAT"
                textFormat: Text.PlainText
                font.pixelSize: 10
                font.weight: Font.DemiBold
                color: Theme.textDim
            }

            GridLayout {
                columns: 3
                Layout.fillWidth: true
                columnSpacing: 6
                rowSpacing: 6

                property var formats: [
                    { id: "jxl", name: "JPEG XL (.jxl)", badge: "Next-Gen 16-bit HDR" },
                    { id: "avif", name: "AVIF (.avif)", badge: "AV1 Web/Mobile" },
                    { id: "webp", name: "WebP (.webp)", badge: "Ultra Compact" },
                    { id: "tiff", name: "16-bit TIFF", badge: "Master Print Lossless" },
                    { id: "png", name: "PNG (.png)", badge: "Graphic Lossless" },
                    { id: "jpeg", name: "JPEG (.jpg)", badge: "Universal MozJPEG" }
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
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.pixelSize: 11
                                font.weight: Font.Bold
                                color: root.selectedFormat === modelData.id ? Theme.accent : Theme.textMain
                            }
                            Text {
                                text: modelData.badge
                                textFormat: Text.PlainText
                                font.pixelSize: 9
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
            title: "Export Compression Quality"
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
                text: "RESOLUTION & SCALING"
                textFormat: Text.PlainText
                font.pixelSize: 10
                font.weight: Font.DemiBold
                color: Theme.textDim
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 6

                property var scales: [
                    { label: "100% Full", pct: 100, maxD: null },
                    { label: "75%", pct: 75, maxD: null },
                    { label: "50%", pct: 50, maxD: null },
                    { label: "4K (3840p)", pct: 100, maxD: 3840 },
                    { label: "Social (2048p)", pct: 100, maxD: 2048 }
                ]

                Repeater {
                    model: parent.scales
                    delegate: Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 28
                        radius: 4
                        property bool isCur: root.scalePercent === modelData.pct && root.maxDimension === modelData.maxD
                        color: isCur ? Theme.accent : Theme.bgCard
                        border.color: isCur ? Theme.accent : Theme.border
                        border.width: 1

                        Text {
                            anchors.centerIn: parent
                            text: modelData.label
                            textFormat: Text.PlainText
                            font.pixelSize: 10
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
                text: "COLOR SPACE & ICC PROFILE"
                textFormat: Text.PlainText
                font.pixelSize: 10
                font.weight: Font.DemiBold
                color: Theme.textDim
            }

            GridLayout {
                columns: 2
                Layout.fillWidth: true
                columnSpacing: 6
                rowSpacing: 6

                property var profiles: [
                    { id: "sRGB", name: "sRGB", desc: "Universal Web / Social" },
                    { id: "DisplayP3", name: "Display P3", desc: "Apple / Wide OLED" },
                    { id: "Rec2020", name: "Rec.2020", desc: "Ultra-Wide Gamut HDR" },
                    { id: "ACEScg", name: "ACEScg (AP1)", desc: "Academy Color Standard" },
                    { id: "AdobeRGB1998", name: "Adobe RGB 1998", desc: "Pro Photography / Print" },
                    { id: "ProPhotoRGB", name: "ProPhoto RGB", desc: "16-Bit Master Archival" }
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
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.pixelSize: 11
                                font.weight: Font.Bold
                                color: parent.parent.isCur ? Theme.accent : Theme.textMain
                            }
                            Item { Layout.fillWidth: true }
                            Text {
                                text: modelData.desc
                                textFormat: Text.PlainText
                                font.pixelSize: 9
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
                        font.pixelSize: 12
                        color: Theme.accent
                    }
                    Text {
                        text: "Preserve Camera EXIF"
                        textFormat: Text.PlainText
                        font.pixelSize: 10
                        color: Theme.textMain
                        Layout.fillWidth: true
                    }
                    CheckBox {
                        checked: root.preserveExif
                        onToggled: root.preserveExif = checked
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
                        font.pixelSize: 12
                        color: Theme.accentGreen
                    }
                    Text {
                        text: "Strip GPS (Privacy)"
                        textFormat: Text.PlainText
                        font.pixelSize: 10
                        color: Theme.textMain
                        Layout.fillWidth: true
                    }
                    CheckBox {
                        checked: root.stripGps
                        onToggled: root.stripGps = checked
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
                    font.pixelSize: 12
                    color: root.watermarkEnabled ? Theme.accent : Theme.textDim
                }
                Text {
                    text: "Apply Photography Watermark / EXIF Signature"
                    textFormat: Text.PlainText
                    font.pixelSize: 11
                    color: Theme.textMain
                    Layout.fillWidth: true
                }
                CheckBox {
                    checked: root.watermarkEnabled
                    onToggled: root.watermarkEnabled = checked
                }
            }
        }

        // Google Drive Direct Upload Row
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 34
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: root.uploadToGdrive ? Theme.accentCyan : Theme.border

            RowLayout {
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8
                Text {
                    text: Theme.iconCloud
                    font.family: Theme.iconFont
                    font.pixelSize: 12
                    color: Theme.accentCyan
                }
                Text {
                    text: "Auto-Upload to Google Drive (Photos/Exports)"
                    textFormat: Text.PlainText
                    font.pixelSize: 11
                    color: Theme.textMain
                    Layout.fillWidth: true
                }
                CheckBox {
                    checked: root.uploadToGdrive
                    onToggled: root.uploadToGdrive = checked
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
                    font.pixelSize: 13
                    color: root.isExporting ? Theme.textMuted : Theme.bgBase
                }
                Text {
                    text: root.isExporting ? (root.isBatchMode ? "Exporting Batch..." : "Exporting Full Resolution...") : (root.isBatchMode ? ("Start Parallel Batch Export (" + root.batchCount + " Photos)") : ("Start Export (" + root.selectedFormat.toUpperCase() + " / " + root.selectedIcc + ")"))
                    textFormat: Text.PlainText
                    font.pixelSize: 13
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
                        "output_dir": root.localOutputDir,
                        "upload_to_gdrive": root.uploadToGdrive,
                        "gdrive_folder": root.gdriveFolder,
                        "watermark": root.watermarkEnabled ? root.watermarkOptions : null
                    };
                    root.doExport(opts);
                }
            }
        }

        Text {
            text: root.exportStatusText
            textFormat: Text.PlainText
            font.pixelSize: 11
            color: Theme.accentGreen
            visible: root.exportStatusText !== ""
            Layout.alignment: Qt.AlignHCenter
        }
    }
}
