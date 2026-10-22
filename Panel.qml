import QtQuick
import Quickshell
import Quickshell.Io

// Omarchy bar widget: opens or hides the OmaStudio window. The editor itself is the
// standalone omastudio-app; this file only needs Quickshell.
Scope {
    id: root

    // Foreground and accent of the active Omarchy theme
    property color foreground: "#c0caf5"
    property color accent: "#7aa2f7"
    property color hover: Qt.rgba(foreground.r, foreground.g, foreground.b, 0.12)

    function readColors(raw) {
        var lines = String(raw || "").split("\n");
        for (var i = 0; i < lines.length; i++) {
            var m = lines[i].trim().match(/^([A-Za-z0-9_]+)\s*=\s*["']?(#[0-9A-Fa-f]{6,8})["']?/);
            if (!m) continue;
            if (m[1] === "foreground") root.foreground = m[2];
            else if (m[1] === "accent") root.accent = m[2];
        }
    }

    FileView {
        path: Quickshell.env("HOME") + "/.local/state/omarchy/current/theme/colors.toml"
        watchChanges: true
        printErrors: false
        onLoaded: root.readColors(text())
        onFileChanged: reload()
    }

    Rectangle {
        id: barBtn
        implicitWidth: 32
        implicitHeight: 28
        radius: 6
        color: btnMouse.containsMouse ? root.hover : "transparent"

        Text {
            anchors.centerIn: parent
            text: "󰄄"
            font.family: "JetBrainsMono Nerd Font"
            font.pixelSize: 14
            color: btnMouse.containsMouse ? root.accent : root.foreground
        }

        MouseArea {
            id: btnMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: toggleProc.running = true
        }
    }

    Process {
        id: toggleProc
        command: ["omastudio-dashboard"]
    }
}
