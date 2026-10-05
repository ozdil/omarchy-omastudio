import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import "../theme"

Rectangle {
    id: root
    implicitWidth: 480
    implicitHeight: 460
    radius: Theme.radiusLg
    color: Theme.bgSurface
    border.color: Theme.borderLight
    border.width: 1
    clip: true


    signal closeRequested()

    property string pluginName: "OmaStudio"
    property string pluginVersion: "v2.1.0"
    property string pluginDescription: "Lightroom & DaVinci Resolve-Grade RAW Studio"
    property string pluginAuthor: "Ozan Özdil (@ozdil)"
    property string pluginLicense: "MIT Open Source License"
    property bool isVerified: true

    readonly property string manifestPath: Qt.resolvedUrl("../../manifest.json").toString().replace(/^file:\/\//, "")
    readonly property string manifestFallbackPath: (Quickshell.env("HOME") || "/home/ozdil") + "/.config/omarchy/plugins/ozdil.omastudio/manifest.json"

    function loadManifest(raw) {
        if (!raw || typeof raw !== "string") return;
        // HANCORE DoS / Memory Guard: limit size to 64 KiB
        if (raw.length > 65536) return;
        try {
            var parsed = JSON.parse(raw);
            if (parsed.name && typeof parsed.name === "string") root.pluginName = parsed.name.slice(0, 64);
            if (parsed.version && typeof parsed.version === "string") root.pluginVersion = "v" + parsed.version.slice(0, 16);
            if (parsed.description && typeof parsed.description === "string") root.pluginDescription = parsed.description.slice(0, 256);
            if (parsed.author && typeof parsed.author === "string") root.pluginAuthor = parsed.author.slice(0, 64);
            if (parsed.license && typeof parsed.license === "string") root.pluginLicense = parsed.license.slice(0, 32) + " Open Source License";
            if (typeof parsed.verified === "boolean") root.isVerified = parsed.verified;
        } catch (e) {
            // Safe fallback to default values on malformed JSON
        }
    }

    FileView {
        id: manifestWatcher
        path: root.manifestPath
        watchChanges: true
        atomicWrites: true
        printErrors: false
        onLoaded: root.loadManifest(text())
        onLoadFailed: {
            manifestFallbackWatcher.reload();
        }
        onFileChanged: reload()
    }

    FileView {
        id: manifestFallbackWatcher
        path: root.manifestFallbackPath
        watchChanges: true
        atomicWrites: true
        printErrors: false
        onLoaded: root.loadManifest(text())
        onFileChanged: reload()
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 22
        spacing: 14

        // Header: Icon + Title + Version + Close Button
        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            Rectangle {
                width: 38
                height: 38
                radius: Theme.radiusMd
                color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
                border.color: Theme.accent
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconCamera
                    font.family: Theme.iconFont
                    font.pixelSize: 18
                    color: Theme.accent
                }
            }

            ColumnLayout {
                spacing: 2
                Layout.fillWidth: true

                RowLayout {
                    spacing: 8
                    Text {
                        text: root.pluginName
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: 16
                        font.weight: Font.Bold
                        color: Theme.textMain
                    }

                    Rectangle {
                        implicitWidth: verText.implicitWidth + 10
                        implicitHeight: 18
                        radius: 4
                        color: Qt.rgba(Theme.accentGreen.r, Theme.accentGreen.g, Theme.accentGreen.b, 0.18)
                        border.color: Theme.accentGreen
                        border.width: 1

                        Text {
                            id: verText
                            anchors.centerIn: parent
                            text: root.pluginVersion
                            textFormat: Text.PlainText
                            font.family: Theme.monoFont
                            font.pixelSize: 9
                            font.weight: Font.Bold
                            color: Theme.accentGreen
                        }
                    }

                    Rectangle {
                        visible: root.isVerified
                        implicitWidth: verifText.implicitWidth + 8
                        implicitHeight: 18
                        radius: 4
                        color: Qt.rgba(Theme.accentCyan.r, Theme.accentCyan.g, Theme.accentCyan.b, 0.15)
                        border.color: Theme.accentCyan
                        border.width: 1

                        Text {
                            id: verifText
                            anchors.centerIn: parent
                            text: "VERIFIED"
                            textFormat: Text.PlainText
                            font.family: Theme.monoFont
                            font.pixelSize: 8
                            font.weight: Font.Bold
                            color: Theme.accentCyan
                        }
                    }
                }

                Text {
                    text: root.pluginDescription
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    color: Theme.textMuted
                    Layout.maximumWidth: 360
                    wrapMode: Text.WordWrap
                }
            }

            Rectangle {
                implicitWidth: 26
                implicitHeight: 26
                radius: 13
                color: closeMouse.containsMouse ? Theme.bgCardHover : "transparent"
                border.color: Theme.border
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: "\u2715"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 11
                    color: closeMouse.containsMouse ? Theme.highlightClip : Theme.textDim
                }

                MouseArea {
                    id: closeMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: root.closeRequested()
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Theme.border
        }

        // Architecture Features
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8

            RowLayout {
                spacing: 8
                Text {
                    text: "\u2022"
                    font.family: Theme.monoFont
                    font.pixelSize: 12
                    color: Theme.accent
                }
                Text {
                    text: "16-Bit Half-Float Color Science & AgX Filmic Tone Curve"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    color: Theme.textMain
                }
            }

            RowLayout {
                spacing: 8
                Text {
                    text: "\u2022"
                    font.family: Theme.monoFont
                    font.pixelSize: 12
                    color: Theme.accent
                }
                Text {
                    text: "Double-Buffered Zero-Flicker Hardware Accelerated Viewport"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    color: Theme.textMain
                }
            }

            RowLayout {
                spacing: 8
                Text {
                    text: "\u2022"
                    font.family: Theme.monoFont
                    font.pixelSize: 12
                    color: Theme.accent
                }
                Text {
                    text: "Zero-Trust Process Isolation & HANCORE Hardened Storage"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    color: Theme.textMain
                }
            }

            RowLayout {
                spacing: 8
                Text {
                    text: "\u2022"
                    font.family: Theme.monoFont
                    font.pixelSize: 12
                    color: Theme.accent
                }
                Text {
                    text: "Jev AI Vision Engine & TypeSafe Photographic Decision Pipeline"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    color: Theme.textMain
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Theme.border
        }

        // Metadata: Developer & License
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4

            RowLayout {
                Text {
                    text: "Developer:"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                    color: Theme.textDim
                }
                Text {
                    text: root.pluginAuthor
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 10
                    font.weight: Font.Bold
                    color: Theme.textMain
                }
            }

            RowLayout {
                Text {
                    text: "License:"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                    color: Theme.textDim
                }
                Text {
                    text: root.pluginLicense
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 10
                    color: Theme.textMuted
                }
            }

            RowLayout {
                Text {
                    text: "Platform:"
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                    color: Theme.textDim
                }
                Text {
                    text: "Omarchy Linux (Wayland / Hyprland)"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 10
                    color: Theme.textMuted
                }
            }
        }

        Item { Layout.fillHeight: true }

        // Action Buttons: GitHub, Buy Me a Coffee & Close
        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 32
                radius: Theme.radiusSm
                color: ghMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.borderLight
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: "GitHub"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 10
                    font.weight: Font.Medium
                    color: Theme.accent
                }

                MouseArea {
                    id: ghMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: Qt.openUrlExternally("https://github.com/ozdil/omarchy-omastudio")
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 32
                radius: Theme.radiusSm
                color: bmacMouse.containsMouse ? "#FFE433" : "#FFDD00"
                border.color: "#E6C700"
                border.width: 1

                RowLayout {
                    anchors.centerIn: parent
                    spacing: 5
                    Text {
                        text: "\uf0f4"
                        font.family: Theme.iconFont
                        font.pixelSize: 11
                        color: "#000000"
                    }
                    Text {
                        text: "Buy Me a Coffee"
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: 10
                        font.weight: Font.Bold
                        color: "#000000"
                    }
                }

                MouseArea {
                    id: bmacMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    onClicked: Qt.openUrlExternally("https://buymeacoffee.com/ozdil")
                }
            }

            Rectangle {
                implicitWidth: 70
                implicitHeight: 32
                radius: Theme.radiusSm
                color: okMouse.containsMouse ? Theme.accentHover : Theme.accent

                Text {
                    anchors.centerIn: parent
                    text: "OK"
                    textFormat: Text.PlainText
                    font.family: Theme.monoFont
                    font.pixelSize: 11
                    font.weight: Font.Bold
                    color: Theme.bgBase
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
