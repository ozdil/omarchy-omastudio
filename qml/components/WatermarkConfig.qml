import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

Rectangle {
    id: root
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 16
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property bool enabled: false
    property string watermarkType: "text" // "text" or "logo"
    property string text: "OmaStudio Photography"
    property string logoPath: ""
    property int positionIndex: 8 // 0..8 (bottom-right default)
    property real opacity: 0.85
    property int size: 14
    property int margin: 24
    property string colorHex: "#ffffff"
    property bool dropShadow: true

    signal watermarkChanged(var options)

    function notifyChanged() {
        var opts = {
            "enabled": root.enabled,
            "watermark_type": root.watermarkType,
            "text": root.text,
            "logo_path": root.logoPath ? root.logoPath : null,
            "position_index": root.positionIndex,
            "opacity": root.opacity,
            "size": root.size,
            "margin": root.margin,
            "color": root.colorHex,
            "drop_shadow": root.dropShadow
        };
        root.watermarkChanged(opts);
    }

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        // Header & Enable Toggle
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "WATERMARK & BRANDING"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            CheckBox {
                checked: root.enabled
                onToggled: {
                    root.enabled = checked;
                    root.notifyChanged();
                }
            }
        }

        // Expanded Configuration Options (Visible when enabled)
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 10
            visible: root.enabled

            // Mode Selector: Text vs Logo
            RowLayout {
                Layout.fillWidth: true
                spacing: 6

                Repeater {
                    model: [
                        { "id": "text", "label": "Text / EXIF Tag" },
                        { "id": "logo", "label": "Custom Logo PNG" }
                    ]

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 26
                        radius: Theme.radiusSm
                        color: root.watermarkType === modelData.id ? Theme.accent : Theme.bgCard
                        border.color: root.watermarkType === modelData.id ? Theme.accent : Theme.border
                        border.width: 1

                        Text {
                            anchors.centerIn: parent
                            text: modelData.label
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: 9
                            font.weight: root.watermarkType === modelData.id ? Font.Bold : Font.Normal
                            color: root.watermarkType === modelData.id ? Theme.bgBase : Theme.textMain
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.watermarkType = modelData.id;
                                root.notifyChanged();
                            }
                        }
                    }
                }
            }

            // Text / EXIF Template Input
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                visible: root.watermarkType === "text"

                Text {
                    text: "WATERMARK TEXT OR TEMPLATE"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 9
                    color: Theme.textDim
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 28
                    radius: Theme.radiusSm
                    color: Theme.bgCard
                    border.color: Theme.border
                    border.width: 1

                    TextInput {
                        id: textInput
                        anchors.fill: parent
                        anchors.margins: 6
                        text: root.text
                        font.family: Theme.fontFamily
                        font.pixelSize: 10
                        color: Theme.textMain
                        selectByMouse: true
                        onEditingFinished: {
                            root.text = text;
                            root.notifyChanged();
                        }
                    }
                }

                // Quick EXIF Template Chips
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 4

                    Repeater {
                        model: [
                            "{camera} | {lens}",
                            "{aperture} {shutter} ISO {iso}",
                            "(c) OmaStudio"
                        ]

                        Rectangle {
                            implicitWidth: chipText.implicitWidth + 8
                            implicitHeight: 18
                            radius: 3
                            color: Theme.bgCard
                            border.color: Theme.border

                            Text {
                                id: chipText
                                anchors.centerIn: parent
                                text: modelData
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: 8
                                color: Theme.textMuted
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    root.text = modelData;
                                    textInput.text = modelData;
                                    root.notifyChanged();
                                }
                            }
                        }
                    }
                }
            }

            // Logo File Path Input
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                visible: root.watermarkType === "logo"

                Text {
                    text: "LOGO FILE PATH (.PNG)"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 9
                    color: Theme.textDim
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 28
                    radius: Theme.radiusSm
                    color: Theme.bgCard
                    border.color: Theme.border
                    border.width: 1

                    TextInput {
                        anchors.fill: parent
                        anchors.margins: 6
                        text: root.logoPath
                        font.family: Theme.fontFamily
                        font.pixelSize: 10
                        color: Theme.textMain
                        selectByMouse: true
                        onEditingFinished: {
                            root.logoPath = text;
                            root.notifyChanged();
                        }
                    }
                }
            }

            // 9-Point Grid Alignment & Parameters
            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                // 3x3 Grid
                ColumnLayout {
                    spacing: 4
                    Text {
                        text: "POSITION"
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 9
                        color: Theme.textDim
                    }

                    GridLayout {
                        columns: 3
                        rowSpacing: 3
                        columnSpacing: 3

                        Repeater {
                            model: 9
                            Rectangle {
                                width: 22
                                height: 22
                                radius: 3
                                color: root.positionIndex === index ? Theme.accent : Theme.bgCard
                                border.color: root.positionIndex === index ? Theme.accent : Theme.border
                                border.width: 1

                                Rectangle {
                                    anchors.centerIn: parent
                                    width: 4
                                    height: 4
                                    radius: 2
                                    color: root.positionIndex === index ? Theme.bgBase : Theme.textDim
                                }

                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        root.positionIndex = index;
                                        root.notifyChanged();
                                    }
                                }
                            }
                        }
                    }
                }

                // Drop-Shadow & Color Hex
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 6

                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            text: "Drop Shadow"
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: 9
                            color: Theme.textMain
                            Layout.fillWidth: true
                        }
                        CheckBox {
                            checked: root.dropShadow
                            onToggled: {
                                root.dropShadow = checked;
                                root.notifyChanged();
                            }
                        }
                    }

                    // Quick Color Palette
                    RowLayout {
                        spacing: 6
                        Repeater {
                            model: ["#ffffff", "#c0caf5", "#e0af68", "#f7768e", "#16161e"]
                            Rectangle {
                                width: 18
                                height: 18
                                radius: 9
                                color: modelData
                                border.color: root.colorHex === modelData ? Theme.accent : Theme.border
                                border.width: root.colorHex === modelData ? 2 : 1

                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        root.colorHex = modelData;
                                        root.notifyChanged();
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Opacity & Size Sliders
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 6

                // Opacity
                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        text: "OPACITY"
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 9
                        color: Theme.textDim
                        Layout.fillWidth: true
                    }
                    Text {
                        text: Math.round(root.opacity * 100) + "%"
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 9
                        font.weight: Font.Bold
                        color: Theme.accent
                    }
                }

                Slider {
                    Layout.fillWidth: true
                    from: 0.1
                    to: 1.0
                    value: root.opacity
                    stepSize: 0.05
                    onMoved: {
                        root.opacity = value;
                        root.notifyChanged();
                    }
                }

                // Size
                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        text: "SIZE MULTIPLIER"
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 9
                        color: Theme.textDim
                        Layout.fillWidth: true
                    }
                    Text {
                        text: root.size + "px"
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 9
                        font.weight: Font.Bold
                        color: Theme.accent
                    }
                }

                Slider {
                    Layout.fillWidth: true
                    from: 8
                    to: 32
                    value: root.size
                    stepSize: 1
                    onMoved: {
                        root.size = Math.round(value);
                        root.notifyChanged();
                    }
                }
            }
        }
    }
}
