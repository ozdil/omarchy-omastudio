import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

Rectangle {
    id: root
    implicitHeight: 46
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    property bool isProMode: false
    property bool isSplitView: false
    property bool isCropMode: false
    property real currentZoom: 1.0
    property string activePhotoName: ""
    readonly property bool hasPhoto: activePhotoName !== ""
    property int photoRating: 0
    property string photoFlag: "none"
    // Local AI connection, shown next to the mode switcher
    property bool aiConnected: false
    property bool aiBusy: false
    property string aiModel: ""
    property string aiProvider: ""
    property string aiProviderName: ""
    // Who answers, as shown on the button: the agent behind Omarchy's default agent
    // (claude, codex), else the local server's name and model
    readonly property string aiLabel: {
        if (root.aiProvider === "omarchy") {
            if (root.aiModel === "claude") return "Claude";
            if (root.aiModel === "codex") return "Codex";
            return root.aiModel !== "" ? root.aiModel : root.aiProviderName;
        }
        var name = root.aiProviderName !== "" ? root.aiProviderName : root.aiProvider;
        return root.aiModel !== "" ? name + " · " + root.aiModel : name;
    }
    // Our own monogram badge per provider; no vendor logos are shipped
    readonly property string aiMonogram: {
        if (root.aiProvider === "omarchy") return root.aiModel === "codex" ? "Cx" : (root.aiModel === "claude" ? "C" : "A");
        if (root.aiProvider === "ollama") return "O";
        if (root.aiProvider === "lmstudio") return "LM";
        return root.aiProviderName !== "" ? root.aiProviderName.charAt(0).toUpperCase() : "A";
    }
    readonly property color aiBadgeColor: {
        if (root.aiProvider === "omarchy") return root.aiModel === "codex" ? Theme.accentGreen : Theme.accentOrange;
        if (root.aiProvider === "ollama") return Theme.accentCyan;
        if (root.aiProvider === "lmstudio") return Theme.accentMagenta;
        return Theme.accent;
    }

    signal ratingChanged(int rating)
    signal flagChanged(string flag)

    signal toggleMode()
    signal connectLocalAi()
    // Opens or closes the AI chat once an AI is connected
    signal toggleAiChat()
    property bool aiChatOpen: false
    signal openFileClicked()
    signal toggleSplit()
    signal toggleCrop()
    signal undoClicked()
    signal redoClicked()
    signal exportClicked()
    signal infoClicked()


    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 12

        // App Emblem & Title (Theme-reactive)
        RowLayout {
            spacing: 8
            Rectangle {
                width: 26
                height: 26
                radius: Theme.r(6)
                color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
                border.color: Theme.accent
                border.width: 1

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconCamera
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(13)
                    color: Theme.accent
                }
            }
            Text {
                font.family: Theme.fontFamily
                text: "OmaStudio"
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(14)
                font.weight: Font.Black
                color: Theme.textMain
            }
        }

        // Active filename badge
        Rectangle {
            visible: root.activePhotoName !== ""
            // Cap the badge so long camera filenames don't push the toolbar off-screen
            implicitWidth: Math.min(photoNameText.implicitWidth + 14, 280)
            implicitHeight: 22
            radius: Theme.r(4)
            color: Theme.bgCard
            border.color: Theme.border
            Text {
                id: photoNameText
                anchors.fill: parent
                anchors.leftMargin: 7
                anchors.rightMargin: 7
                elide: Text.ElideMiddle
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                text: root.activePhotoName
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.family: Theme.monoFont
                color: Theme.textMain
            }
        }

        // Lightroom Studio Culling & Rating Widget (P / X Flags & 1-5 Stars)
        RowLayout {
            visible: root.activePhotoName !== ""
            spacing: 3

            // Pick Flag (P)
            Rectangle {
                implicitWidth: 20
                implicitHeight: 20
                radius: Theme.r(3)
                color: root.photoFlag === "pick" ? Theme.accentGreen : Theme.bgCard
                border.color: root.photoFlag === "pick" ? Theme.accentGreen : Theme.border
                Text {
                    anchors.centerIn: parent
                    text: Theme.iconFlag
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: root.photoFlag === "pick" ? Theme.onAccent : Theme.textMuted
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Pick (P)")
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Pick")
                    onClicked: root.flagChanged(root.photoFlag === "pick" ? "none" : "pick")
                }
            }

            // Reject Flag (X)
            Rectangle {
                implicitWidth: 20
                implicitHeight: 20
                radius: Theme.r(3)
                color: root.photoFlag === "reject" ? Theme.highlightClip : Theme.bgCard
                border.color: root.photoFlag === "reject" ? Theme.highlightClip : Theme.border
                Text {
                    anchors.centerIn: parent
                    text: Theme.iconBan
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: root.photoFlag === "reject" ? "#ffffff" : Theme.textMuted
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Reject (X)")
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Reject")
                    onClicked: root.flagChanged(root.photoFlag === "reject" ? "none" : "reject")
                }
            }

            // Star Rating (1..5)
            RowLayout {
                spacing: 2
                Repeater {
                    model: 5
                    Rectangle {
                        implicitWidth: 16
                        implicitHeight: 20
                        radius: Theme.r(2)
                        color: "transparent"
                        Text {
                            anchors.centerIn: parent
                            text: (index + 1) <= root.photoRating ? Theme.iconStar : Theme.iconStarEmpty
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(11)
                            color: (index + 1) <= root.photoRating ? Theme.accentYellow : Theme.textDim
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            hoverEnabled: true
                            ToolTip.visible: containsMouse
                            ToolTip.delay: 500
                            ToolTip.text: qsTr("%n star(s)", "", index + 1)
                            onClicked: {
                                var newR = (root.photoRating === (index + 1)) ? 0 : (index + 1);
                                root.ratingChanged(newR);
                            }
                        }
                    }
                }
            }
        }

        Item { Layout.fillWidth: true }

        // Local AI: Omarchy's default agent, else Ollama or LM Studio on this machine
        Rectangle {
            implicitWidth: aiRow.implicitWidth + 16
            implicitHeight: 28
            radius: Theme.radiusSm
            color: root.aiChatOpen ? Theme.bgCardHover : (aiMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
            border.color: root.aiChatOpen ? Theme.accent : (root.aiConnected ? Theme.accentGreen : Theme.border)

            RowLayout {
                id: aiRow
                anchors.centerIn: parent
                spacing: 6
                Text {
                    visible: !root.aiConnected || root.aiBusy
                    text: Theme.iconAi
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: Theme.textMuted
                }
                Rectangle {
                    visible: root.aiConnected && !root.aiBusy
                    implicitWidth: Math.max(implicitHeight, monogram.implicitWidth + Theme.px(8))
                    implicitHeight: Theme.px(18)
                    radius: implicitHeight / 2
                    color: root.aiBadgeColor

                    Text {
                        id: monogram
                        anchors.centerIn: parent
                        text: root.aiMonogram
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(9)
                        font.weight: Font.Bold
                        color: Theme.bgBase
                    }
                }
                Text {
                    font.family: Theme.fontFamily
                    text: root.aiBusy ? qsTr("Connecting…")
                        : root.aiConnected ? root.aiLabel : qsTr("Connect local AI")
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    Layout.maximumWidth: Theme.px(180)
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.DemiBold
                    color: root.aiConnected ? Theme.textMain : Theme.textMuted
                }
            }

            MouseArea {
                id: aiMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                enabled: !root.aiBusy
                onClicked: root.aiConnected ? root.toggleAiChat() : root.connectLocalAi()
                ToolTip.visible: containsMouse
                ToolTip.delay: 500
                ToolTip.text: root.aiConnected ? (root.aiChatOpen ? qsTr("Connected: %1. Click to close the chat.").arg(root.aiLabel)
                                                                    : qsTr("Connected: %1. Click to open the chat.").arg(root.aiLabel))
                                               : qsTr("Use Omarchy's default AI agent, or Ollama or LM Studio on this computer")
                Accessible.role: Accessible.Button
                Accessible.name: root.aiConnected ? qsTr("AI chat") : qsTr("Connect local AI")
            }
        }

        // Mode Switcher: Essentials vs Pro Studio
        Rectangle {
            implicitWidth: Theme.px(170)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            RowLayout {
                anchors.fill: parent
                spacing: 0

                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    radius: Theme.radiusSm
                    color: !root.isProMode ? Theme.accent : "transparent"

                    RowLayout {
                        anchors.centerIn: parent
                        spacing: 6
                        Text {
                            text: Theme.iconImage
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(10)
                            color: !root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                        Text {
                            font.family: Theme.fontFamily
                            text: qsTr("Essentials")
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            font.weight: Font.DemiBold
                            color: !root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        hoverEnabled: true
                        onClicked: if (root.isProMode) root.toggleMode()
                        ToolTip.visible: containsMouse
                        ToolTip.delay: 500
                        ToolTip.text: qsTr("Essentials: white balance, tone, presence, film simulation, presets and LUT")
                        Accessible.role: Accessible.Button
                        Accessible.name: qsTr("Essentials")
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    radius: Theme.radiusSm
                    color: root.isProMode ? Theme.accent : "transparent"

                    RowLayout {
                        anchors.centerIn: parent
                        spacing: 6
                        Text {
                            text: Theme.iconSliders
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(10)
                            color: root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                        Text {
                            font.family: Theme.fontFamily
                            text: qsTr("Pro")
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            font.weight: Font.DemiBold
                            color: root.isProMode ? Theme.bgBase : Theme.textMuted
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        hoverEnabled: true
                        onClicked: if (!root.isProMode) root.toggleMode()
                        ToolTip.visible: containsMouse
                        ToolTip.delay: 500
                        ToolTip.text: qsTr("Pro adds tone curve, colour mixer, colour grading, detail, grain, colour management and masks")
                        Accessible.role: Accessible.Button
                        Accessible.name: qsTr("Pro")
                    }
                }
            }
        }

        // Undo / Redo Quick Actions
        RowLayout {
            enabled: root.hasPhoto
            opacity: enabled ? 1 : 0.4
            spacing: 2

            Rectangle {
                implicitWidth: 28
                implicitHeight: 28
                radius: Theme.r(4)
                color: undoMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconUndo
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: undoMouse.containsMouse ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    id: undoMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Undo (Ctrl+Z)")
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Undo")
                    onClicked: root.undoClicked()
                }
            }

            Rectangle {
                implicitWidth: 28
                implicitHeight: 28
                radius: Theme.r(4)
                color: redoMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                border.color: Theme.border

                Text {
                    anchors.centerIn: parent
                    text: Theme.iconRedo
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: redoMouse.containsMouse ? Theme.accent : Theme.textMain
                }

                MouseArea {
                    id: redoMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 500
                    ToolTip.text: qsTr("Redo (Ctrl+Shift+Z)")
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Redo")
                    onClicked: root.redoClicked()
                }
            }
        }

        // Before / After Split Button (A | B)
        Rectangle {
            enabled: root.hasPhoto
            opacity: enabled ? 1 : 0.4
            implicitWidth: Math.max(70, label374.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: root.isSplitView ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
            border.color: root.isSplitView ? Theme.accent : Theme.border

            RowLayout {
                id: label374
                anchors.centerIn: parent
                spacing: 5
                Text {
                    text: Theme.iconSplit
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: root.isSplitView ? Theme.accent : Theme.textMuted
                }
                Text {
                    font.family: Theme.fontFamily
                    text: "A | B"
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.DemiBold
                    color: root.isSplitView ? Theme.accent : Theme.textMain
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.toggleSplit()
            }
        }

        // Crop Mode Toggle Button (C)
        Rectangle {
            enabled: root.hasPhoto
            opacity: enabled ? 1 : 0.4
            implicitWidth: Math.max(68, label409.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: root.isCropMode ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.bgCard
            border.color: root.isCropMode ? Theme.accent : Theme.border

            RowLayout {
                id: label409
                anchors.centerIn: parent
                spacing: 5
                Text {
                    text: Theme.iconCrop
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(10)
                    color: root.isCropMode ? Theme.accent : Theme.textMuted
                }
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Crop")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.DemiBold
                    color: root.isCropMode ? Theme.accent : Theme.textMain
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.toggleCrop()
            }
        }

        Item { Layout.fillWidth: true }

        LanguageMenu {}

        // Omarchy Studio Info & About Button
        Rectangle {
            implicitWidth: Math.max(32, label513.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: mouseInfo.containsMouse ? Theme.bgCardHover : Theme.bgCard
            border.color: mouseInfo.containsMouse ? Theme.accent : Theme.border
            border.width: 1

            Text {
                id: label513
                anchors.centerIn: parent
                text: Theme.iconInfo
                font.family: Theme.iconFont
                font.pixelSize: Theme.px(13)
                color: mouseInfo.containsMouse ? Theme.accent : Theme.textDim
            }

            MouseArea {
                id: mouseInfo
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.infoClicked()
            }
        }


        // Open Photo Button
        Rectangle {
            implicitWidth: Math.max(92, label541.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.bgCard
            border.color: Theme.border

            RowLayout {
                id: label541
                anchors.centerIn: parent
                spacing: 6
                Text {
                    text: Theme.iconFolder
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: Theme.textMain
                }
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Open RAW")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(11)
                    font.weight: Font.Medium
                    color: Theme.textMain
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.openFileClicked()
            }
        }

        // Export Button (Bright CTA)
        Rectangle {
            implicitWidth: Math.max(88, label576.implicitWidth + 16)
            implicitHeight: 28
            radius: Theme.radiusSm
            color: Theme.accent

            RowLayout {
                id: label576
                anchors.centerIn: parent
                spacing: 6
                Text {
                    text: Theme.iconExport
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(11)
                    color: Theme.bgBase
                }
                Text {
                    font.family: Theme.fontFamily
                    text: qsTr("Export")
                    textFormat: Text.PlainText
                    font.pixelSize: Theme.px(11)
                    font.weight: Font.Bold
                    color: Theme.bgBase
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.exportClicked()
            }
        }
    }
}
