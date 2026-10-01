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

    property string activeSimulation: "none"
    property real intensity: 1.0

    signal simulationChanged(string id, real intensity)

    readonly property var filmSimulations: [
        { "id": "none", "name": "Standard / Off", "brand": "Standard", "desc": "Neutral camera linear profile", "color": "#7982a9" },
        // Fujifilm Color
        { "id": "fuji_provia", "name": "Provia 100F", "brand": "Fujifilm", "desc": "Standard natural daylight color", "color": "#7aa2f7" },
        { "id": "fuji_velvia", "name": "Velvia 50", "brand": "Fujifilm", "desc": "Ultra-vivid landscapes & high saturation", "color": "#f7768e" },
        { "id": "fuji_astia", "name": "Astia 100F", "brand": "Fujifilm", "desc": "Soft portraits with smooth skin gradations", "color": "#ff9e64" },
        { "id": "fuji_classic_chrome", "name": "Classic Chrome", "brand": "Fujifilm", "desc": "Documentary tones, muted colors & deep shadows", "color": "#e0af68" },
        { "id": "fuji_classic_neg", "name": "Classic Neg", "brand": "Fujifilm", "desc": "Superia color negative film nostalgia", "color": "#9ece6a" },
        { "id": "fuji_eterna", "name": "Eterna Cinema", "brand": "Fujifilm", "desc": "Cinematic flat gamma with soft highlight roll-off", "color": "#7dcfff" },
        // Fujifilm Monochrome
        { "id": "fuji_acros_standard", "name": "Acros 100", "brand": "Acros B&W", "desc": "Legendary ultra-fine grain monochrome", "color": "#c0caf5" },
        { "id": "fuji_acros_yellow", "name": "Acros (+Ye)", "brand": "Acros B&W", "desc": "Slight contrast lift for skies and portraits", "color": "#e0af68" },
        { "id": "fuji_acros_red", "name": "Acros (+R)", "brand": "Acros B&W", "desc": "Deep dramatic skies and high micro-contrast", "color": "#f7768e" },
        { "id": "fuji_acros_green", "name": "Acros (+G)", "brand": "Acros B&W", "desc": "Enhanced foliage separation and soft skin tones", "color": "#9ece6a" },
        // Hasselblad
        { "id": "hasselblad_hncs", "name": "HNCS Neutral", "brand": "Hasselblad", "desc": "Natural Colour Solution 16-bit medium format", "color": "#bb9af7" },
        { "id": "hasselblad_xpan", "name": "XPan Panoramic", "brand": "Hasselblad", "desc": "35mm panoramic cinematic contrast", "color": "#545c7e" }
    ]

    property string selectedBrand: "All"

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        // Header & Reset
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Text {
                text: "FILM SIMULATION & SENSOR PROFILES"
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: 10
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            Rectangle {
                implicitWidth: resetSimText.implicitWidth + 10
                implicitHeight: 18
                radius: 4
                color: resetSimMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    id: resetSimText
                    anchors.centerIn: parent
                    text: "RESET"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    color: Theme.textDim
                }

                MouseArea {
                    id: resetSimMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: {
                        root.activeSimulation = "none";
                        root.intensity = 1.0;
                        root.simulationChanged("none", 1.0);
                    }
                }
            }
        }

        // Category Filter Pills
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Repeater {
                model: ["All", "Fujifilm", "Acros B&W", "Hasselblad"]
                Rectangle {
                    implicitWidth: brandText.implicitWidth + 12
                    implicitHeight: 20
                    radius: 4
                    color: root.selectedBrand === modelData ? Theme.accent : Theme.bgCard
                    border.color: root.selectedBrand === modelData ? Theme.accent : Theme.border
                    border.width: 1

                    Text {
                        id: brandText
                        anchors.centerIn: parent
                        text: modelData
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 9
                        font.weight: root.selectedBrand === modelData ? Font.Bold : Font.Normal
                        color: root.selectedBrand === modelData ? Theme.bgBase : Theme.textMain
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.selectedBrand = modelData
                    }
                }
            }
        }

        // Simulation Cards Grid
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            rowSpacing: 6
            columnSpacing: 6

            Repeater {
                model: root.filmSimulations.filter(function(item) {
                    if (root.selectedBrand === "All") return true;
                    return item.brand === root.selectedBrand;
                })

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 46
                    radius: Theme.radiusSm
                    color: root.activeSimulation === modelData.id ? Theme.bgCardHover : Theme.bgCard
                    border.color: root.activeSimulation === modelData.id ? Theme.accent : Theme.border
                    border.width: root.activeSimulation === modelData.id ? 1.5 : 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 6
                        spacing: 8

                        Rectangle {
                            width: 10
                            height: 10
                            radius: 5
                            color: modelData.color
                            Layout.alignment: Qt.AlignVCenter
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1

                            Text {
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: 10
                                font.weight: root.activeSimulation === modelData.id ? Font.Bold : Font.Normal
                                color: root.activeSimulation === modelData.id ? Theme.accent : Theme.textMain
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }

                            Text {
                                text: modelData.desc
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: 8
                                color: Theme.textDim
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }

                        Text {
                            text: Theme.iconCheck
                            font.family: Theme.iconFont
                            font.pixelSize: 10
                            color: Theme.accent
                            visible: root.activeSimulation === modelData.id
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.activeSimulation = modelData.id;
                            root.simulationChanged(root.activeSimulation, root.intensity);
                        }
                    }
                }
            }
        }

        // Intensity Slider (visible when a simulation is active)
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            visible: root.activeSimulation !== "none"

            RowLayout {
                Layout.fillWidth: true
                Text {
                    text: "SIMULATION INTENSITY"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 9
                    color: Theme.textDim
                    Layout.fillWidth: true
                }
                Text {
                    text: Math.round(root.intensity * 100) + "%"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 9
                    font.weight: Font.Bold
                    color: Theme.accent
                }
            }

            Slider {
                id: intensitySlider
                Layout.fillWidth: true
                from: 0.0
                to: 1.0
                value: root.intensity
                stepSize: 0.01

                onMoved: {
                    root.intensity = value;
                    root.simulationChanged(root.activeSimulation, root.intensity);
                }
            }
        }
    }
}
