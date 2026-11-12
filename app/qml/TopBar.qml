import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

Rectangle {
    id: root
    implicitHeight: Theme.sp(38)
    color: Theme.bgDark

    // The bar's lower edge only; the window frame is Hyprland's
    Rectangle { anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom; height: 1; color: Theme.border }

    // Labels give way to icons when the bar is narrower than it needs: a tiled
    // window, a scrolling-layout column or a large Omarchy font
    readonly property real requiredWidth: brand.implicitWidth + (root.hasPhoto ? photoBadge.implicitWidth + culling.implicitWidth : 0)
        + aiButton.fullWidth + essentialsBtn.fullWidth + proBtn.fullWidth + undoBtn.implicitWidth + redoBtn.implicitWidth
        + splitBtn.fullWidth + cropBtn.fullWidth + langMenu.implicitWidth + infoBtn.implicitWidth
        + openBtn.fullWidth + exportBtn.fullWidth + 14 * bar.spacing + bar.anchors.leftMargin + bar.anchors.rightMargin
    readonly property bool compact: root.requiredWidth > root.width
    // Still too narrow with icons: the app name goes too
    readonly property bool tight: root.compact && (root.requiredWidth - splitBtn.fullWidth - cropBtn.fullWidth - openBtn.fullWidth
        - aiButton.fullWidth + 4 * root.height) > root.width

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
        id: bar
        anchors.fill: parent
        anchors.leftMargin: Theme.sp(10)
        anchors.rightMargin: Theme.sp(10)
        spacing: Theme.sp(6)

        // App emblem and name
        RowLayout {
            id: brand
            spacing: Theme.sp(7)
            Text {
                text: Theme.iconCamera
                font.family: Theme.iconFont
                font.pixelSize: Theme.px(13)
                color: Theme.accent
                Accessible.ignored: true
            }
            Text {
                visible: !root.tight
                font.family: Theme.fontFamily
                text: "OmaStudio"
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(12)
                font.weight: Font.Bold
                color: Theme.textMain
            }
        }

        // Active filename
        Rectangle {
            id: photoBadge
            visible: root.hasPhoto
            // Capped so long camera filenames don't push the toolbar off-screen
            implicitWidth: Math.min(photoNameText.implicitWidth + Theme.sp(14), Theme.px(220))
            Layout.maximumWidth: implicitWidth
            Layout.minimumWidth: Theme.px(60)
            Layout.fillWidth: true
            implicitHeight: Theme.sp(20)
            radius: Theme.radiusSm
            color: Theme.controlFill("normal")
            Text {
                id: photoNameText
                anchors.fill: parent
                anchors.leftMargin: Theme.sp(7)
                anchors.rightMargin: Theme.sp(7)
                elide: Text.ElideMiddle
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                text: root.activePhotoName
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(10)
                font.family: Theme.monoFont
                color: Theme.textMuted
            }
        }

        // Culling: pick and reject flags, 1-5 stars
        RowLayout {
            id: culling
            visible: root.hasPhoto
            spacing: Theme.sp(2)

            OmaButton {
                implicitHeight: Theme.sp(20)
                implicitWidth: implicitHeight
                padding: 0
                icon: Theme.iconFlag
                checkable: true
                checked: root.photoFlag === "pick"
                text: qsTr("Pick")
                showLabel: false
                tip: qsTr("Pick (P)")
                onClicked: root.flagChanged(root.photoFlag === "pick" ? "none" : "pick")
            }
            OmaButton {
                implicitHeight: Theme.sp(20)
                implicitWidth: implicitHeight
                padding: 0
                icon: Theme.iconBan
                checkable: true
                checked: root.photoFlag === "reject"
                danger: root.photoFlag === "reject"
                text: qsTr("Reject")
                showLabel: false
                tip: qsTr("Reject (X)")
                onClicked: root.flagChanged(root.photoFlag === "reject" ? "none" : "reject")
            }

            Row {
                spacing: 0
                Repeater {
                    model: 5
                    Item {
                        width: Theme.px(15)
                        height: Theme.sp(20)
                        Text {
                            anchors.centerIn: parent
                            text: (index + 1) <= root.photoRating || starMouse.containsMouse ? Theme.iconStar : Theme.iconStarEmpty
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(11)
                            color: (index + 1) <= root.photoRating ? Theme.accentYellow
                                 : starMouse.containsMouse ? Theme.textMuted : Theme.textDim
                        }
                        MouseArea {
                            id: starMouse
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            hoverEnabled: true
                            ToolTip.visible: containsMouse
                            ToolTip.delay: 500
                            ToolTip.text: qsTr("%n star(s)", "", index + 1)
                            onClicked: root.ratingChanged(root.photoRating === (index + 1) ? 0 : (index + 1))
                        }
                        KeyFocus {
                            name: qsTr("%n star(s)", "", index + 1)
                            checkable: true
                            checked: root.photoRating === index + 1
                            onActivated: root.ratingChanged(root.photoRating === (index + 1) ? 0 : (index + 1))
                        }
                    }
                }
            }
        }

        Item { Layout.fillWidth: true }

        // Local AI: Omarchy's default agent, else Ollama or LM Studio on this machine
        Rectangle {
            id: aiButton
            readonly property string state_: aiMouse.pressed ? "pressed" : root.aiChatOpen ? "selected"
                : aiMouse.containsMouse ? "hover-cursor" : aiKey.activeFocus ? "focus" : "normal"
            readonly property real fullWidth: aiIcon.implicitWidth + aiName.implicitWidth + aiRow.spacing + Theme.sp(16)
            implicitWidth: aiRow.implicitWidth + Theme.sp(16)
            implicitHeight: Theme.sp(24)
            radius: Theme.radiusSm
            color: Theme.controlFill(state_)
            border.width: Theme.controlBorderWidth(state_)
            border.color: Theme.controlBorder(state_)

            RowLayout {
                id: aiRow
                anchors.centerIn: parent
                spacing: Theme.sp(6)
                Item {
                    id: aiIcon
                    implicitWidth: root.aiConnected && !root.aiBusy ? badge.implicitWidth : glyph.implicitWidth
                    implicitHeight: Theme.px(16)
                    Text {
                        id: glyph
                        anchors.centerIn: parent
                        visible: !root.aiConnected || root.aiBusy
                        text: Theme.iconAi
                        font.family: Theme.iconFont
                        font.pixelSize: Theme.px(10)
                        color: Theme.textMuted
                    }
                    Rectangle {
                        id: badge
                        anchors.centerIn: parent
                        visible: root.aiConnected && !root.aiBusy
                        implicitWidth: Math.max(implicitHeight, monogram.implicitWidth + Theme.px(8))
                        implicitHeight: Theme.px(16)
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
                            color: Theme.contrast(root.aiBadgeColor, "#ffffff") >= Theme.contrast(root.aiBadgeColor, "#0b0c10") ? "#ffffff" : "#0b0c10"
                        }
                    }
                }
                Text {
                    id: aiName
                    visible: !root.compact
                    font.family: Theme.fontFamily
                    text: root.aiBusy ? qsTr("Connecting…")
                        : root.aiConnected ? root.aiLabel : qsTr("Connect local AI")
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    Layout.maximumWidth: Theme.px(180)
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.DemiBold
                    color: root.aiChatOpen ? Theme.selectedText : root.aiConnected ? Theme.textMain : Theme.textMuted
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
            }
            KeyFocus {
                id: aiKey
                name: root.aiConnected ? qsTr("AI chat") : qsTr("Connect local AI")
                checkable: root.aiConnected
                checked: root.aiChatOpen
                onActivated: root.aiConnected ? root.toggleAiChat() : root.connectLocalAi()
            }
        }

        // Mode switcher: Essentials or Pro
        Row {
            spacing: Theme.sp(2)
            OmaButton {
                id: essentialsBtn
                icon: Theme.iconImage
                text: qsTr("Essentials")
                showLabel: !root.tight
                checkable: true
                checked: !root.isProMode
                tip: qsTr("Essentials: white balance, tone, presence, film simulation, presets and LUT")
                onClicked: if (root.isProMode) root.toggleMode()
            }
            OmaButton {
                id: proBtn
                icon: Theme.iconSliders
                text: qsTr("Pro")
                showLabel: !root.tight
                checkable: true
                checked: root.isProMode
                tip: qsTr("Pro adds tone curve, colour mixer, colour grading, detail, grain, colour management and masks")
                onClicked: if (!root.isProMode) root.toggleMode()
            }
        }

        // Undo and redo
        Row {
            enabled: root.hasPhoto
            spacing: Theme.sp(2)
            OmaButton {
                id: undoBtn
                icon: Theme.iconUndo
                text: qsTr("Undo")
                showLabel: false
                tip: qsTr("Undo (Ctrl+Z)")
                onClicked: root.undoClicked()
            }
            OmaButton {
                id: redoBtn
                icon: Theme.iconRedo
                text: qsTr("Redo")
                showLabel: false
                tip: qsTr("Redo (Ctrl+Shift+Z)")
                onClicked: root.redoClicked()
            }
        }

        // Before / after split
        OmaButton {
            id: splitBtn
            enabled: root.hasPhoto
            icon: Theme.iconSplit
            text: "A | B"
            accessibleName: qsTr("Before and after")
            showLabel: !root.compact
            checkable: true
            checked: root.isSplitView
            tip: qsTr("Before and after")
            onClicked: root.toggleSplit()
        }

        // Crop mode (C)
        OmaButton {
            id: cropBtn
            enabled: root.hasPhoto
            icon: Theme.iconCrop
            text: qsTr("Crop")
            showLabel: !root.compact
            checkable: true
            checked: root.isCropMode
            onClicked: root.toggleCrop()
        }

        Item { Layout.fillWidth: true }

        LanguageMenu { id: langMenu }

        OmaButton {
            id: infoBtn
            icon: Theme.iconInfo
            text: qsTr("About OmaStudio")
            showLabel: false
            onClicked: root.infoClicked()
        }

        OmaButton {
            id: openBtn
            icon: Theme.iconFolder
            text: qsTr("Open RAW")
            accessibleName: qsTr("Open photo")
            showLabel: !root.compact
            onClicked: root.openFileClicked()
        }

        // The main action
        OmaButton {
            id: exportBtn
            primary: true
            icon: Theme.iconExport
            text: qsTr("Export")
            showLabel: !root.tight
            onClicked: root.exportClicked()
        }
    }
}
