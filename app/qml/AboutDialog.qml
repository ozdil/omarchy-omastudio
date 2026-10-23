import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

// About OmaStudio: product, version, capabilities, project links and support.
Rectangle {
    id: root
    implicitWidth: Theme.px(560)
    implicitHeight: content.implicitHeight + 2 * pad
    radius: Theme.radiusLg
    color: Theme.bgSurface
    border.color: Theme.borderLight
    border.width: 1
    clip: true

    readonly property int pad: Theme.px(24)

    signal closeRequested()

    focus: visible
    onVisibleChanged: if (visible) { root.forceActiveFocus(); root.refreshSystem(); }
    Keys.onEscapePressed: root.closeRequested()

    // Swallow clicks on the dialog body so they don't reach the overlay's close handler
    MouseArea {
        anchors.fill: parent
        z: -1
    }

    readonly property string version: Host.version
    readonly property string repoUrl: "https://github.com/ozdil/omarchy-omastudio"

    property string gpuInfo: ""
    property string aiAccel: ""
    property bool showThirdParty: false

    readonly property string systemInfo: [
        "OmaStudio " + root.version,
        "Omarchy theme: " + Theme.themeName + ", font: " + Theme.systemFont + " " + Theme.fontBase + " px",
        "GPU: " + (root.gpuInfo || "unknown"),
        "AI acceleration: " + (root.aiAccel || "CPU"),
        "Session: " + (Qt.platform.pluginName || "unknown")
    ].join("\n")

    function refreshSystem() {
        if (Host.enginePath === "") return;
        if (!gpuProc.running) gpuProc.running = true;
        if (!npuProc.running) npuProc.running = true;
    }

    function parseData(output) {
        try {
            var r = JSON.parse(String(output || ""));
            return r.success ? r.data : null;
        } catch (e) {
            return null;
        }
    }

    Process {
        id: gpuProc
        command: [Host.enginePath, "gpu"]
        onFinished: function(exitCode, output) {
            var d = root.parseData(output);
            root.gpuInfo = d && d.enabled ? d.name + " (" + d.backend + ")" : qsTr("CPU only");
        }
    }

    Process {
        id: npuProc
        command: [Host.enginePath, "npu"]
        onFinished: function(exitCode, output) {
            var d = root.parseData(output);
            root.aiAccel = d && d.available && d.providers.length > 0 ? "ONNX Runtime: " + d.providers.join(", ") : "CPU";
        }
    }

    function copyVersionInfo() {
        clipboardHelper.text = root.systemInfo;
        clipboardHelper.selectAll();
        clipboardHelper.copy();
        copiedTimer.restart();
    }

    TextEdit {
        id: clipboardHelper
        visible: false
    }

    Timer {
        id: copiedTimer
        interval: 1600
    }

    // Small text button used for the project links
    component LinkButton: Rectangle {
        id: lb
        property string label: ""
        property string url: ""
        implicitWidth: lbText.implicitWidth + Theme.px(20)
        implicitHeight: Theme.px(28)
        radius: Theme.radiusSm
        color: lbMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
        border.color: lbMouse.containsMouse ? Theme.accent : Theme.border
        border.width: 1

        Text {
            id: lbText
            anchors.centerIn: parent
            text: lb.label
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            font.weight: Font.DemiBold
            color: lbMouse.containsMouse ? Theme.accent : Theme.textMain
        }

        MouseArea {
            id: lbMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: Qt.openUrlExternally(lb.url)
            Accessible.role: Accessible.Link
            Accessible.name: lb.label
        }
    }

    // Section heading in the Omarchy style: small, spaced capitals
    component SectionLabel: Text {
        textFormat: Text.PlainText
        font.family: Theme.fontFamily
        font.pixelSize: Theme.px(9)
        font.weight: Font.Bold
        font.letterSpacing: 1.2
        font.capitalization: Font.AllUppercase
        color: Theme.textDim
    }

    ColumnLayout {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: root.pad
        spacing: Theme.px(18)

        // Identity: icon, name, tagline, version
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.px(16)

            Item {
                Layout.alignment: Qt.AlignTop
                implicitWidth: Theme.px(56)
                implicitHeight: Theme.px(56)

                Image {
                    id: appIcon
                    anchors.fill: parent
                    // Built into the binary, so it shows for package and local installs alike
                    source: "icon.svg"
                    sourceSize.width: width * 2
                    sourceSize.height: height * 2
                    fillMode: Image.PreserveAspectFit
                    smooth: true
                }

                // Without an installed icon: the camera glyph in an accent frame
                Rectangle {
                    anchors.fill: parent
                    visible: appIcon.status !== Image.Ready
                    radius: Theme.radiusMd
                    color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12)
                    border.color: Theme.accent
                    border.width: 1
                    Text {
                        anchors.centerIn: parent
                        text: Theme.iconCamera
                        font.family: Theme.iconFont
                        font.pixelSize: Theme.px(24)
                        color: Theme.accent
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Theme.px(4)

                Text {
                    text: "OmaStudio"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(20)
                    font.weight: Font.Bold
                    color: Theme.textMain
                }
                Text {
                    Layout.fillWidth: true
                    text: qsTr("The Complete Medium Format RAW Editor for Omarchy")
                    textFormat: Text.PlainText
                    wrapMode: Text.WordWrap
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(11)
                    color: Theme.textMuted
                }
                RowLayout {
                    spacing: Theme.px(8)
                    Layout.topMargin: Theme.px(2)

                    Text {
                        text: qsTr("Version %1").arg(root.version)
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: Theme.px(10)
                        color: Theme.textMain
                    }

                    Text {
                        text: copiedTimer.running ? qsTr("Copied") : qsTr("Copy system info")
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(9)
                        font.underline: copyMouse.containsMouse
                        color: copiedTimer.running ? Theme.accentGreen : Theme.accent

                        MouseArea {
                            id: copyMouse
                            anchors.fill: parent
                            anchors.margins: -4
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.copyVersionInfo()
                            ToolTip.visible: containsMouse
                            ToolTip.delay: 500
                            ToolTip.text: qsTr("Copy version, GPU and Omarchy details for a bug report")
                        }
                    }
                }
            }
        }

        Text {
            Layout.fillWidth: true
            text: qsTr("OmaStudio develops RAW files with its own engine: X-Trans and Bayer demosaicing on linear light, GPU acceleration and colour-managed output, with full support for Fujifilm GFX, Hasselblad and other medium format cameras.")
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            lineHeight: 1.25
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            color: Theme.textMuted
        }

        // Capabilities
        ColumnLayout {
            Layout.fillWidth: true
            spacing: Theme.px(10)

            SectionLabel { text: qsTr("Capabilities") }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Theme.px(16)
                rowSpacing: Theme.px(10)

                Repeater {
                    model: [
                        { title: qsTr("RAW engine"), detail: qsTr("X-Trans and Bayer demosaicing, 16-bit linear pipeline, GPU via Vulkan") },
                        { title: qsTr("Colour management"), detail: "Little CMS 2; sRGB, Display P3, Adobe RGB, ProPhoto, Rec. 2020" },
                        { title: "Fujifilm", detail: qsTr("Camera recipes, film simulations, lens corrections, Pixel Shift") },
                        { title: qsTr("Medium format"), detail: "Fujifilm GFX, Hasselblad, Phase One, Leica S, Pentax 645" },
                        { title: qsTr("AI"), detail: qsTr("Omarchy's default agent, Ollama, LM Studio or a cloud provider; local masks") },
                        { title: qsTr("Library"), detail: qsTr("Local catalogue with ratings, flags and colour labels; camera over USB") }
                    ]

                    ColumnLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        Layout.alignment: Qt.AlignTop
                        spacing: Theme.px(2)

                        Text {
                            text: modelData.title
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(10)
                            font.weight: Font.DemiBold
                            color: Theme.textMain
                        }
                        Text {
                            Layout.fillWidth: true
                            text: modelData.detail
                            textFormat: Text.PlainText
                            wrapMode: Text.WordWrap
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(9)
                            color: Theme.textMuted
                        }
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: Theme.border
        }

        // Details
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: Theme.px(16)
            rowSpacing: Theme.px(6)

            Repeater {
                model: [
                    [qsTr("Developer"), "Ozan Özdil · @ozdil"],
                    [qsTr("License"), qsTr("MIT License")],
                    [qsTr("Platform"), "Omarchy Linux · Wayland · Hyprland"],
                    [qsTr("Graphics"), root.gpuInfo || "…"],
                    [qsTr("Appearance"), Theme.themeName + " · " + Theme.systemFont],
                    [qsTr("Source"), "github.com/ozdil/omarchy-omastudio"]
                ]

                delegate: Item {
                    required property var modelData
                    required property int index
                    Layout.columnSpan: 2
                    Layout.fillWidth: true
                    implicitHeight: detailRow.implicitHeight

                    RowLayout {
                        id: detailRow
                        anchors.left: parent.left
                        anchors.right: parent.right
                        spacing: Theme.px(16)
                        Text {
                            Layout.preferredWidth: Theme.px(80)
                            text: modelData[0]
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(10)
                            color: Theme.textDim
                        }
                        Text {
                            Layout.fillWidth: true
                            text: modelData[1]
                            textFormat: Text.PlainText
                            wrapMode: Text.Wrap
                            font.family: Theme.monoFont
                            font.pixelSize: Theme.px(10)
                            color: Theme.textMain
                        }
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: Theme.border
        }

        // Privacy
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.px(8)
            Text {
                Layout.alignment: Qt.AlignTop
                text: Theme.iconShield
                font.family: Theme.iconFont
                font.pixelSize: Theme.px(11)
                color: Theme.accent
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("Runs entirely on this computer. No telemetry. AI features contact only the provider you choose.")
                textFormat: Text.PlainText
                wrapMode: Text.WordWrap
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(9)
                color: Theme.textMuted
            }
        }

        // Third-party software
        ColumnLayout {
            Layout.fillWidth: true
            spacing: Theme.px(6)

            Text {
                text: (root.showThirdParty ? "\u25BE " : "\u25B8 ") + qsTr("Third-party software")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(9)
                font.weight: Font.DemiBold
                color: tpMouse.containsMouse ? Theme.accent : Theme.textDim
                MouseArea {
                    id: tpMouse
                    anchors.fill: parent
                    anchors.margins: -4
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.showThirdParty = !root.showThirdParty
                }
            }

            Text {
                visible: root.showThirdParty
                Layout.fillWidth: true
                text: "Qt 6 (LGPLv3) · cxx-qt (MIT/Apache-2.0) · LibRaw (LGPL-2.1/CDDL-1.0) · Little CMS 2 (MIT) · "
                    + "wgpu (MIT/Apache-2.0) · ONNX Runtime (MIT) · image-rs (MIT/Apache-2.0)\n"
                    + qsTr("Fujifilm, GFX, X-Trans, Hasselblad and other camera names are trademarks of their owners and are used only to describe compatibility.")
                textFormat: Text.PlainText
                wrapMode: Text.WordWrap
                lineHeight: 1.2
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(9)
                color: Theme.textMuted
            }
        }

        // Links and support
        Flow {
            Layout.fillWidth: true
            spacing: Theme.px(8)

            LinkButton { label: qsTr("Project page"); url: root.repoUrl }
            LinkButton { label: qsTr("Release notes"); url: root.repoUrl + "/releases/tag/v" + root.version }
            LinkButton { label: qsTr("Report an issue"); url: root.repoUrl + "/issues/new" }
            LinkButton { label: qsTr("Sponsor"); url: "https://github.com/sponsors/ozdil" }
            LinkButton { label: qsTr("Buy Me a Coffee"); url: "https://buymeacoffee.com/ozdil" }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.px(12)

            Text {
                Layout.fillWidth: true
                text: qsTr("© %1 %2 · MIT License").arg(2026).arg("Ozan Özdil")
                textFormat: Text.PlainText
                wrapMode: Text.WordWrap
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(9)
                color: Theme.textDim
            }

            Rectangle {
                implicitWidth: Math.max(Theme.px(80), okText.implicitWidth + Theme.px(24))
                implicitHeight: Theme.px(30)
                radius: Theme.radiusSm
                color: okMouse.containsMouse ? Theme.accentHover : Theme.accent

                Text {
                    id: okText
                    anchors.centerIn: parent
                    text: qsTr("Close")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.Bold
                    color: Theme.onAccent
                }

                MouseArea {
                    id: okMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: root.closeRequested()
                }
            }
        }
    }
}
