import QtQuick
import QtQuick.Layouts
import QtQuick.Controls

// Chat with the connected AI about the open photo. Each message is an edit request:
// the AI answers with a short explanation and the edit is applied (Ctrl+Z undoes it).
// Opened and closed from the AI button in the top bar.
Rectangle {
    id: root
    radius: Theme.radiusSm
    color: Theme.bgDark
    border.color: Theme.border
    border.width: 1

    // The AiAssistPanel that talks to the engine
    property var assistant: null
    property string label: ""
    property string activePhotoPath: ""
    readonly property bool busy: root.assistant !== null && root.assistant.busy
    // { "role": "user" | "ai" | "error", "text": ... }
    property var messages: []

    signal closeRequested()
    signal reconnectRequested()

    function append(role, text) {
        var m = root.messages.slice();
        m.push({ "role": role, "text": text });
        root.messages = m;
        Qt.callLater(function() { list.positionViewAtEnd(); });
    }

    function send() {
        var text = input.text.trim();
        if (text === "" || root.busy || root.assistant === null) return;
        if (root.activePhotoPath === "") {
            root.append("error", qsTr("Open a photo first"));
            return;
        }
        root.append("user", text);
        root.assistant.promptText = text;
        root.assistant.runEdit();
        input.text = "";
    }

    function focusInput() { input.forceActiveFocus(); }

    Connections {
        target: root.assistant
        function onRecipeSuggested(recipe, explanation) {
            root.append("ai", explanation !== "" ? explanation : qsTr("Edit applied."));
        }
        function onEditFailed(error) {
            root.append("error", error);
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Theme.px(10)
        spacing: Theme.px(8)

        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            Text {
                Layout.fillWidth: true
                text: root.label !== "" ? qsTr("Chat · %1").arg(root.label) : qsTr("Chat")
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(11)
                font.weight: Font.DemiBold
                color: Theme.textMain
            }
            Repeater {
                model: [
                    { "icon": Theme.iconRefresh, "tip": qsTr("Reconnect"), "act": "reconnect" },
                    { "icon": Theme.iconRotateLeft, "tip": qsTr("Clear chat"), "act": "clear" },
                    { "icon": Theme.iconTimes, "tip": qsTr("Close chat"), "act": "close" }
                ]
                delegate: Rectangle {
                    required property var modelData
                    implicitWidth: Theme.px(22)
                    implicitHeight: Theme.px(22)
                    radius: Theme.radiusSm
                    color: hdrMouse.containsMouse ? Theme.bgCardHover : "transparent"
                    Text {
                        anchors.centerIn: parent
                        text: modelData.icon
                        font.family: Theme.iconFont
                        font.pixelSize: Theme.px(10)
                        color: hdrMouse.containsMouse ? Theme.accent : Theme.textDim
                    }
                    MouseArea {
                        id: hdrMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            if (modelData.act === "close") root.closeRequested();
                            else if (modelData.act === "clear") root.messages = [];
                            else root.reconnectRequested();
                        }
                        ToolTip.visible: containsMouse
                        ToolTip.delay: 500
                        ToolTip.text: modelData.tip
                        Accessible.role: Accessible.Button
                        Accessible.name: modelData.tip
                    }
                }
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: Theme.px(6)
            model: root.messages
            ScrollBar.vertical: ScrollBar {}

            delegate: Rectangle {
                required property var modelData
                readonly property bool mine: modelData.role === "user"
                x: mine ? list.width - width : 0
                width: Math.min(list.width * 0.85, Math.ceil(measure.implicitWidth) + Theme.px(20))
                // Unwrapped copy that gives the bubble its natural width
                Text {
                    id: measure
                    visible: false
                    font: msg.font
                    text: modelData.text
                    textFormat: Text.PlainText
                }
                implicitHeight: msg.implicitHeight + Theme.px(12)
                radius: Theme.radiusSm
                color: mine ? Theme.accent : Theme.bgCard
                border.color: modelData.role === "error" ? Theme.accentMagenta : "transparent"
                Text {
                    id: msg
                    anchors.fill: parent
                    anchors.margins: Theme.px(6)
                    anchors.leftMargin: Theme.px(8)
                    anchors.rightMargin: Theme.px(8)
                    text: modelData.text
                    textFormat: Text.PlainText
                    verticalAlignment: Text.AlignVCenter
                    wrapMode: Text.Wrap
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    color: parent.mine ? Theme.bgBase : (modelData.role === "error" ? Theme.accentMagenta : Theme.textMain)
                }
            }

            Text {
                anchors.centerIn: parent
                width: parent.width - Theme.px(20)
                visible: root.messages.length === 0
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                text: qsTr("Write below what to change in the photo. The AI sets the adjustments; Ctrl+Z undoes each edit.")
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(10)
                color: Theme.textMuted
            }
        }

        Text {
            visible: root.busy
            text: qsTr("Working…")
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(9)
            color: Theme.textMuted
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: Theme.px(30)
                radius: Theme.radiusSm
                color: Theme.bgCard
                border.color: input.activeFocus ? Theme.accent : Theme.border
                BoundedTextInput {
                    id: input
                    anchors.fill: parent
                    anchors.margins: 7
                    verticalAlignment: TextEdit.AlignVCenter
                    maxLength: 400
                    font.pixelSize: Theme.px(10)
                    color: Theme.textMain
                    placeholderText: qsTr("e.g. warmer, moody evening light, recover the sky")
                    onAccepted: root.send()
                    Accessible.name: qsTr("Message to the AI")
                }
            }

            Rectangle {
                implicitWidth: sendText.implicitWidth + Theme.px(16)
                implicitHeight: Theme.px(30)
                radius: Theme.radiusSm
                enabled: !root.busy && input.text.trim() !== ""
                opacity: enabled ? 1.0 : 0.4
                color: sendMouse.containsMouse ? Theme.accentHover : Theme.accent
                Text {
                    id: sendText
                    anchors.centerIn: parent
                    text: qsTr("Send")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    font.weight: Font.Bold
                    color: Theme.bgBase
                }
                MouseArea {
                    id: sendMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.send()
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Send")
                }
            }
        }
    }
}
