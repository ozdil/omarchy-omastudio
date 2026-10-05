import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import "../theme"

Rectangle {
    id: root
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 20
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property var layersList: []
    property int selectedLayerIndex: -1

    signal layersChanged()

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        // Header & Add Buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "LOCAL ADJUSTMENT LAYERS"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            // Linear Gradient Mask Button
            Rectangle {
                implicitWidth: addLinText.implicitWidth + 10
                implicitHeight: 20
                radius: 4
                color: addLinMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border
                border.width: 1

                Text {
                    id: addLinText
                    anchors.centerIn: parent
                    text: "+ GRADIENT"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.accentCyan
                }

                MouseArea {
                    id: addLinMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        var list = root.layersList.slice();
                        var newL = {
                            "id": "layer_" + Date.now(),
                            "name": "Linear Gradient " + (list.length + 1),
                            "enabled": true,
                            "invert": false,
                            "opacity": 1.0,
                            "shape": {
                                "type": "linear_gradient",
                                "start_x": 0.5,
                                "start_y": 0.0,
                                "end_x": 0.5,
                                "end_y": 0.5
                            },
                            "exposure": 0.0,
                            "contrast": 0.0,
                            "highlights": 0.0,
                            "shadows": 0.0,
                            "whites": 0.0,
                            "blacks": 0.0,
                            "clarity": 0.0,
                            "saturation": 0.0,
                            "tint_r": 0.0,
                            "tint_g": 0.0,
                            "tint_b": 0.0
                        };
                        list.push(newL);
                        root.layersList = list;
                        root.selectedLayerIndex = list.length - 1;
                        root.layersChanged();
                    }
                }
            }

            // Radial Mask Button
            Rectangle {
                implicitWidth: addRadText.implicitWidth + 10
                implicitHeight: 20
                radius: 4
                color: addRadMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border
                border.width: 1

                Text {
                    id: addRadText
                    anchors.centerIn: parent
                    text: "+ RADIAL"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.accentOrange
                }

                MouseArea {
                    id: addRadMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        var list = root.layersList.slice();
                        var newL = {
                            "id": "layer_" + Date.now(),
                            "name": "Radial Mask " + (list.length + 1),
                            "enabled": true,
                            "invert": false,
                            "opacity": 1.0,
                            "shape": {
                                "type": "radial_gradient",
                                "center_x": 0.5,
                                "center_y": 0.5,
                                "radius_x": 0.25,
                                "radius_y": 0.25,
                                "feather": 0.5
                            },
                            "exposure": 0.0,
                            "contrast": 0.0,
                            "highlights": 0.0,
                            "shadows": 0.0,
                            "whites": 0.0,
                            "blacks": 0.0,
                            "clarity": 0.0,
                            "saturation": 0.0,
                            "tint_r": 0.0,
                            "tint_g": 0.0,
                            "tint_b": 0.0
                        };
                        list.push(newL);
                        root.layersList = list;
                        root.selectedLayerIndex = list.length - 1;
                        root.layersChanged();
                    }
                }
            }

            // Luma Range Mask Button
            Rectangle {
                implicitWidth: addLumaText.implicitWidth + 10
                implicitHeight: 20
                radius: 4
                color: addLumaMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border
                border.width: 1

                Text {
                    id: addLumaText
                    anchors.centerIn: parent
                    text: "+ LUMA"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.accentYellow
                }

                MouseArea {
                    id: addLumaMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        var list = root.layersList.slice();
                        var newL = {
                            "id": "layer_" + Date.now(),
                            "name": "Luma Range " + (list.length + 1),
                            "enabled": true,
                            "invert": false,
                            "opacity": 1.0,
                            "shape": {
                                "type": "luma_range",
                                "min_luma": 0.6,
                                "max_luma": 1.0,
                                "falloff": 0.1
                            },
                            "exposure": 0.0,
                            "contrast": 0.0,
                            "highlights": 0.0,
                            "shadows": 0.0,
                            "whites": 0.0,
                            "blacks": 0.0,
                            "clarity": 0.0,
                            "saturation": 0.0,
                            "tint_r": 0.0,
                            "tint_g": 0.0,
                            "tint_b": 0.0
                        };
                        list.push(newL);
                        root.layersList = list;
                        root.selectedLayerIndex = list.length - 1;
                        root.layersChanged();
                    }
                }
            }
        }

        // Layers List View
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            visible: root.layersList.length > 0

            Repeater {
                model: root.layersList.length

                delegate: Rectangle {
                    id: layerItem
                    Layout.fillWidth: true
                    implicitHeight: 28
                    radius: 4
                    color: root.selectedLayerIndex === index ? Theme.bgCardHover : Theme.bgCard
                    border.color: root.selectedLayerIndex === index ? Theme.accent : Theme.border
                    border.width: 1

                    readonly property var curLayer: root.layersList[index]

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        spacing: 8

                        CustomSwitch {
                            checked: layerItem.curLayer ? layerItem.curLayer.enabled : false
                            activeColor: Theme.accent
                            onToggled: function(c) {
                                var list = root.layersList.slice();
                                list[index].enabled = c;
                                root.layersList = list;
                                root.layersChanged();
                            }
                        }

                        Text {
                            text: layerItem.curLayer ? layerItem.curLayer.name : ""
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: 11
                            font.weight: root.selectedLayerIndex === index ? Font.Bold : Font.Medium
                            color: layerItem.curLayer && layerItem.curLayer.enabled ? Theme.textMain : Theme.textDim
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }

                        // Invert indicator
                        Rectangle {
                            implicitWidth: 16
                            implicitHeight: 16
                            radius: 3
                            color: layerItem.curLayer && layerItem.curLayer.invert ? Theme.accentCyan : "transparent"
                            border.color: Theme.border
                            border.width: 1

                            Text {
                                anchors.centerIn: parent
                                text: "I"
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: 8
                                font.weight: Font.Bold
                                color: layerItem.curLayer && layerItem.curLayer.invert ? "#ffffff" : Theme.textDim
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    var list = root.layersList.slice();
                                    list[index].invert = !list[index].invert;
                                    root.layersList = list;
                                    root.layersChanged();
                                }
                            }
                        }

                        // Delete button
                        Rectangle {
                            implicitWidth: 16
                            implicitHeight: 16
                            radius: 3
                            color: "transparent"

                            Text {
                                anchors.centerIn: parent
                                text: "X"
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: 9
                                font.weight: Font.Bold
                                color: Theme.accentRed
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    var list = root.layersList.slice();
                                    list.splice(index, 1);
                                    root.layersList = list;
                                    if (root.selectedLayerIndex >= list.length) {
                                        root.selectedLayerIndex = list.length - 1;
                                    }
                                    root.layersChanged();
                                }
                            }
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        propagateComposedEvents: true
                        onClicked: function(mouse) {
                            root.selectedLayerIndex = index;
                            mouse.accepted = false;
                        }
                    }
                }
            }
        }

        // Active Layer Adjustments Controls
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: root.selectedLayerIndex >= 0 && root.selectedLayerIndex < root.layersList.length

            readonly property var activeL: (root.selectedLayerIndex >= 0 && root.selectedLayerIndex < root.layersList.length) ? root.layersList[root.selectedLayerIndex] : null

            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }

            Text {
                text: "SELECTED LAYER CONTROLS (" + (activeL ? activeL.name : "") + ")"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 9
                font.weight: Font.Bold
                color: Theme.accent
            }

            SliderGroup {
                title: "Layer Opacity"
                from: 0.0
                to: 1.0
                value: activeL ? activeL.opacity : 1.0
                defaultValue: 1.0
                stepSize: 0.05
                decimals: 2
                accentColor: Theme.accentCyan
                onSliderMoved: function(v) {
                    if (!activeL) return;
                    var list = root.layersList.slice();
                    list[root.selectedLayerIndex].opacity = v;
                    root.layersList = list;
                    root.layersChanged();
                }
            }

            SliderGroup {
                title: "Target Exposure"
                from: -4.0
                to: 4.0
                value: activeL ? activeL.exposure : 0.0
                defaultValue: 0.0
                stepSize: 0.1
                decimals: 1
                suffix: " EV"
                accentColor: Theme.accentYellow
                onSliderMoved: function(v) {
                    if (!activeL) return;
                    var list = root.layersList.slice();
                    list[root.selectedLayerIndex].exposure = v;
                    root.layersList = list;
                    root.layersChanged();
                }
            }

            SliderGroup {
                title: "Target Contrast"
                from: -100.0
                to: 100.0
                value: activeL ? activeL.contrast : 0.0
                defaultValue: 0.0
                stepSize: 1.0
                accentColor: Theme.accent
                onSliderMoved: function(v) {
                    if (!activeL) return;
                    var list = root.layersList.slice();
                    list[root.selectedLayerIndex].contrast = v;
                    root.layersList = list;
                    root.layersChanged();
                }
            }

            SliderGroup {
                title: "Target Saturation"
                from: -100.0
                to: 100.0
                value: activeL ? activeL.saturation : 0.0
                defaultValue: 0.0
                stepSize: 1.0
                accentColor: Theme.accentGreen
                onSliderMoved: function(v) {
                    if (!activeL) return;
                    var list = root.layersList.slice();
                    list[root.selectedLayerIndex].saturation = v;
                    root.layersList = list;
                    root.layersChanged();
                }
            }
        }
    }
}
