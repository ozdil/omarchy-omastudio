import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

Rectangle {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
    function reset() {
        root.activeSimulation = "none";
        root.intensity = 1.0;
        root.simulationChanged("none", 1.0);
    }
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
        { "id": "none", "name": qsTr("Standard / Off"), "brand": "Standard", "desc": qsTr("No film simulation"), "color": "#7982a9" },
        // Fujifilm Color
        { "id": "fuji_provia", "name": "Provia 100F", "brand": "Fujifilm", "desc": qsTr("Standard natural daylight color"), "color": "#7aa2f7" },
        { "id": "fuji_velvia", "name": "Velvia 50", "brand": "Fujifilm", "desc": qsTr("Ultra-vivid landscapes & high saturation"), "color": "#f7768e" },
        { "id": "fuji_astia", "name": "Astia 100F", "brand": "Fujifilm", "desc": qsTr("Soft portraits with smooth skin gradations"), "color": "#ff9e64" },
        { "id": "fuji_classic_chrome", "name": "Classic Chrome", "brand": "Fujifilm", "desc": qsTr("Documentary tones, muted colors & deep shadows"), "color": "#e0af68" },
        { "id": "fuji_classic_neg", "name": "Classic Neg", "brand": "Fujifilm", "desc": qsTr("Superia color negative film nostalgia"), "color": "#9ece6a" },
        { "id": "fuji_eterna", "name": "Eterna Cinema", "brand": "Fujifilm", "desc": qsTr("Cinematic flat gamma with soft highlight roll-off"), "color": "#7dcfff" },
        { "id": "fuji_reala_ace", "name": "Reala Ace", "brand": "Fujifilm", "desc": qsTr("Faithful colour with slightly firmer contrast"), "color": "#7aa2f7" },
        { "id": "fuji_nostalgic_neg", "name": "Nostalgic Neg.", "brand": "Fujifilm", "desc": qsTr("Amber highlights and rich shadows"), "color": "#ff9e64" },
        { "id": "fuji_pro_neg_hi", "name": "PRO Neg. Hi", "brand": "Fujifilm", "desc": qsTr("Studio portraits with a little more contrast"), "color": "#e0af68" },
        { "id": "fuji_pro_neg_std", "name": "PRO Neg. Std", "brand": "Fujifilm", "desc": qsTr("Soft studio portraits with even skin tones"), "color": "#c0caf5" },
        { "id": "fuji_eterna_bleach_bypass", "name": "Eterna Bleach Bypass", "brand": "Fujifilm", "desc": qsTr("Hard contrast with very low saturation"), "color": "#545c7e" },
        // Fujifilm Monochrome
        { "id": "fuji_acros_standard", "name": "Acros 100", "brand": "Acros B&W", "desc": qsTr("Very fine grain black and white"), "color": "#c0caf5" },
        { "id": "fuji_acros_yellow", "name": "Acros (+Ye)", "brand": "Acros B&W", "desc": qsTr("Slight contrast lift for skies and portraits"), "color": "#e0af68" },
        { "id": "fuji_acros_red", "name": "Acros (+R)", "brand": "Acros B&W", "desc": qsTr("Deep dramatic skies and high micro-contrast"), "color": "#f7768e" },
        { "id": "fuji_acros_green", "name": "Acros (+G)", "brand": "Acros B&W", "desc": qsTr("Enhanced foliage separation and soft skin tones"), "color": "#9ece6a" },
        { "id": "fuji_monochrome", "name": qsTr("Monochrome"), "brand": "Acros B&W", "desc": qsTr("Standard black and white"), "color": "#c0caf5" },
        { "id": "fuji_monochrome_yellow", "name": qsTr("Monochrome (+Ye)"), "brand": "Acros B&W", "desc": qsTr("Yellow filter: darker skies"), "color": "#e0af68" },
        { "id": "fuji_monochrome_red", "name": qsTr("Monochrome (+R)"), "brand": "Acros B&W", "desc": qsTr("Red filter: dramatic skies"), "color": "#f7768e" },
        { "id": "fuji_monochrome_green", "name": qsTr("Monochrome (+G)"), "brand": "Acros B&W", "desc": qsTr("Green filter: lighter foliage"), "color": "#9ece6a" },
        { "id": "fuji_sepia", "name": qsTr("Sepia"), "brand": "Acros B&W", "desc": qsTr("Warm brown toned monochrome"), "color": "#ff9e64" },
        // Hasselblad
        { "id": "hasselblad_hncs", "name": "HNCS Neutral", "brand": "Hasselblad", "desc": qsTr("Natural Colour Solution 16-bit medium format"), "color": "#bb9af7" },
        { "id": "hasselblad_xpan", "name": "XPan Panoramic", "brand": "Hasselblad", "desc": qsTr("35mm panoramic cinematic contrast"), "color": "#545c7e" }
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
                visible: !root.embedded
                text: qsTr("Profile")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(10)
                font.weight: Font.DemiBold
                font.letterSpacing: 1
                color: Theme.textDim
                Layout.fillWidth: true
            }

            Rectangle {
                visible: !root.embedded
                implicitWidth: resetSimText.implicitWidth + 10
                implicitHeight: 18
                radius: Theme.r(4)
                color: resetSimMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    id: resetSimText
                    anchors.centerIn: parent
                    text: qsTr("Reset")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(8)
                    font.weight: Font.Bold
                    color: Theme.textDim
                }

                MouseArea {
                    id: resetSimMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: root.reset()
                }
            }
        }

        // Category Filter Pills
        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Repeater {
                model: [{ "key": "All", "label": qsTr("All") }, { "key": "Fujifilm", "label": "Fujifilm" }, { "key": "Acros B&W", "label": qsTr("Acros B&W") }, { "key": "Hasselblad", "label": "Hasselblad" }]
                Rectangle {
                    implicitWidth: brandText.implicitWidth + 12
                    implicitHeight: 20
                    radius: Theme.r(4)
                    color: root.selectedBrand === modelData.key ? Theme.accent : Theme.bgCard
                    border.color: root.selectedBrand === modelData.key ? Theme.accent : Theme.border
                    border.width: 1

                    Text {
                        id: brandText
                        anchors.centerIn: parent
                        text: modelData.label
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(9)
                        font.weight: root.selectedBrand === modelData.key ? Font.Bold : Font.Normal
                        color: root.selectedBrand === modelData.key ? Theme.bgBase : Theme.textMain
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.selectedBrand = modelData.key
                    }
                }
            }
        }

        // Profile list: one compact row each, scrolls past about eight rows
        Flickable {
            id: simList
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(simColumn.implicitHeight, Theme.px(232))
            contentHeight: simColumn.implicitHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: simList.contentHeight > simList.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }

            Column {
                id: simColumn
                width: simList.width
                spacing: 1

                Repeater {
                    model: root.filmSimulations.filter(function(item) {
                        if (root.selectedBrand === "All") return true;
                        return item.brand === root.selectedBrand;
                    })

                    Rectangle {
                        required property var modelData
                        readonly property bool isCur: root.activeSimulation === modelData.id
                        width: simColumn.width
                        height: Theme.px(28)
                        radius: Theme.radiusSm
                        color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15) : (simMouse.containsMouse ? Theme.bgCardHover : "transparent")

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: Theme.px(8)
                            anchors.rightMargin: Theme.px(8)
                            spacing: Theme.px(8)

                            Text {
                                text: modelData.name
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.px(11)
                                font.weight: parent.parent.isCur ? Font.DemiBold : Font.Normal
                                color: parent.parent.isCur ? Theme.accent : Theme.textMain
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                text: modelData.brand
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.px(9)
                                color: Theme.textDim
                            }
                        }

                        MouseArea {
                            id: simMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.activeSimulation = modelData.id;
                                root.simulationChanged(root.activeSimulation, root.intensity);
                            }
                        }
                        ToolTip.visible: simMouse.containsMouse && modelData.desc !== ""
                        ToolTip.delay: 600
                        ToolTip.text: modelData.desc
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
                    text: qsTr("Amount")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(9)
                    color: Theme.textDim
                    Layout.fillWidth: true
                }
                Text {
                    text: Math.round(root.intensity * 100) + "%"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(9)
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

                background: Rectangle {
                    x: intensitySlider.leftPadding
                    y: intensitySlider.topPadding + intensitySlider.availableHeight / 2 - height / 2
                    implicitWidth: 200
                    implicitHeight: 3
                    width: intensitySlider.availableWidth
                    height: implicitHeight
                    radius: Theme.r(1.5)
                    color: Qt.rgba(Theme.textMain.r, Theme.textMain.g, Theme.textMain.b, 0.12)

                    Rectangle {
                        width: intensitySlider.visualPosition * parent.width
                        height: parent.height
                        color: Theme.accent
                        radius: Theme.r(1.5)
                    }
                }

                handle: Rectangle {
                    x: intensitySlider.leftPadding + intensitySlider.visualPosition * (intensitySlider.availableWidth - width)
                    y: intensitySlider.topPadding + intensitySlider.availableHeight / 2 - height / 2
                    implicitWidth: 12
                    implicitHeight: 12
                    radius: Theme.r(6)
                    color: intensitySlider.pressed ? Theme.accent : (intensitySlider.hovered ? Theme.accentHover : Theme.textMain)
                    border.color: Theme.bgDark
                    border.width: 1.5
                    Behavior on color { ColorAnimation { duration: 100 } }
                }

                onMoved: {
                    root.intensity = value;
                    root.simulationChanged(root.activeSimulation, root.intensity);
                }
            }
        }
    }
}
