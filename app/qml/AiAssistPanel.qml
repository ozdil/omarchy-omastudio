import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

// Edit assistant: a vision model (Claude, OpenAI, OpenRouter, Gemini, Omarchy's default
// agent, Ollama or LM Studio) looks at the developed photo and sets the global
// adjustments from a request in plain words. Provider setup stores API keys in the Secret Service through the engine.
ColumnLayout {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
    spacing: 8
    Layout.fillWidth: true

    property string enginePath: ""
    property string activePhotoPath: ""
    // Returns the current recipe object
    property var recipeProvider: null

    property var providers: []
    property var choice: ({})
    property bool showSetup: false
    property string setupProvider: ""
    property string explanation: ""
    property bool busy: editProc.running
    property bool connecting: setupProc.running && setupProc.action === "local"
    // The saved choice is a local server (Ollama, LM Studio) that answers
    readonly property bool localConnected: {
        for (var i = 0; i < root.providers.length; i++) {
            var p = root.providers[i];
            if (p.id === root.choice.provider) return p.local && p.ready;
        }
        return false;
    }
    property alias promptText: promptInput.text

    signal recipeSuggested(var recipe, string explanation)
    signal notify(string text, bool isError)

    Component.onCompleted: refresh()

    function refresh() {
        if (root.enginePath === "" || listProc.running) return;
        listProc.command = [root.enginePath, "ai", "providers"];
        listProc.running = true;
    }

    function providerName(id) {
        for (var i = 0; i < root.providers.length; i++)
            if (root.providers[i].id === id) return root.providers[i].name;
        return id;
    }

    readonly property string statusText: {
        if (root.choice.provider)
            return root.providerName(root.choice.provider) + (root.choice.model ? " · " + root.choice.model : "");
        for (var i = 0; i < root.providers.length; i++)
            if (root.providers[i].ready) return qsTr("%1 (automatic)").arg(root.providers[i].name);
        return qsTr("No provider set up");
    }

    function runEdit() {
        if (root.activePhotoPath === "" || editProc.running) return;
        var recipe = root.recipeProvider ? root.recipeProvider() : {};
        editProc.command = [root.enginePath, "ai", "edit", root.activePhotoPath,
                            "--prompt", promptInput.text, "--recipe", JSON.stringify(recipe)];
        root.explanation = "";
        editProc.running = true;
    }

    function parse(output) {
        try {
            return JSON.parse(String(output || ""));
        } catch (e) {
            return { "success": false, "error": qsTr("The engine gave no answer") };
        }
    }

    Process {
        id: listProc
        onFinished: function(exitCode, output) {
            var resp = root.parse(output);
            if (resp.success) {
                root.providers = resp.data.providers || [];
                root.choice = resp.data.choice || {};
            }
        }
    }

    Process {
        id: editProc
        onFinished: function(exitCode, output) {
            var resp = root.parse(output);
            if (resp.success) {
                root.explanation = resp.data.explanation || "";
                root.recipeSuggested(resp.data.recipe, root.explanation);
            } else {
                root.notify(String(resp.error || qsTr("AI edit failed")).split("\n")[0], true);
            }
        }
    }

    Process {
        id: setupProc
        property string action: ""
        stdinEnabled: true
        onStarted: {
            // The key travels on stdin, never as an argument
            if (setupProc.action === "key") {
                setupProc.write(keyInput.text + "\n");
                keyInput.text = "";
            }
        }
        onFinished: function(exitCode, output) {
            var resp = root.parse(output);
            var ok = setupProc.action === "key" ? qsTr("API key stored in the keyring")
                   : setupProc.action === "local" ? qsTr("Connected to local AI: %1").arg(resp.success ? resp.data.model : "")
                   : qsTr("Provider saved");
            root.notify(resp.success ? ok : String(resp.error || qsTr("Failed")).split("\n")[0], !resp.success);
            root.refresh();
        }
    }

    function storeKey() {
        if (root.setupProvider === "" || keyInput.text.length === 0 || setupProc.running) return;
        setupProc.action = "key";
        setupProc.command = [root.enginePath, "ai", "key", root.setupProvider];
        setupProc.running = true;
    }

    // Connects to the AI set up on this machine: Omarchy's default agent, Ollama, LM Studio
    function connectLocal() {
        if (root.enginePath === "" || setupProc.running) return;
        setupProc.action = "local";
        setupProc.command = [root.enginePath, "ai", "local"];
        setupProc.running = true;
    }

    function useProvider() {
        if (root.setupProvider === "" || setupProc.running) return;
        setupProc.action = "use";
        var cmd = [root.enginePath, "ai", "use", root.setupProvider];
        if (modelInput.text.trim().length > 0) cmd.push(modelInput.text.trim());
        setupProc.command = cmd;
        setupProc.running = true;
    }

    component SmallButton: Rectangle {
        id: sb
        property string label: ""
        property bool primary: false
        signal clicked()
        implicitWidth: sbText.implicitWidth + 16
        implicitHeight: 24
        radius: Theme.r(4)
        opacity: enabled ? 1.0 : 0.4
        color: primary ? (sbMouse.containsMouse ? Theme.accentHover : Theme.accent) : (sbMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
        border.color: primary ? Theme.accent : Theme.border
        border.width: 1
        Text {
            id: sbText
            anchors.centerIn: parent
            text: sb.label
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(9)
            font.weight: Font.Bold
            color: sb.primary ? Theme.bgBase : Theme.textMain
        }
        MouseArea {
            id: sbMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: sb.clicked()
        }
    }

    component InputBox: Rectangle {
        Layout.fillWidth: true
        implicitHeight: 28
        radius: Theme.r(4)
        color: Theme.bgCard
        border.color: Theme.border
        border.width: 1
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 4
        Text {
            visible: !root.embedded
            font.family: Theme.fontFamily
            text: qsTr("Assistant")
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(10)
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
            Layout.fillWidth: true
        }
        Item { Layout.fillWidth: true; visible: root.embedded }
        SmallButton {
            label: root.showSetup ? qsTr("Done") : qsTr("Set up")
            onClicked: {
                root.showSetup = !root.showSetup;
                if (root.showSetup) root.refresh();
            }
        }
    }

    Text {
        font.family: Theme.fontFamily
        Layout.fillWidth: true
        text: root.statusText
        elide: Text.ElideRight
        textFormat: Text.PlainText
        font.pixelSize: Theme.px(9)
        color: Theme.textMuted
    }

    InputBox {
        BoundedTextInput {
            id: promptInput
            anchors.fill: parent
            anchors.margins: 6
            verticalAlignment: TextEdit.AlignVCenter
            maxLength: 400
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            color: Theme.textMain
            placeholderText: qsTr("e.g. warmer, moody evening light, recover the sky")
            placeholderTextColor: Theme.textDim
            onAccepted: root.runEdit()
        }
    }

    SmallButton {
        Layout.fillWidth: true
        primary: true
        label: root.busy ? qsTr("Working…") : qsTr("Edit")
        enabled: !root.busy && root.activePhotoPath !== ""
        onClicked: root.runEdit()
    }

    Text {
        font.family: Theme.fontFamily
        visible: root.explanation !== ""
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: root.explanation
        textFormat: Text.PlainText
        font.pixelSize: Theme.px(10)
        color: Theme.textMain
    }

    // Provider setup
    ColumnLayout {
        visible: root.showSetup
        Layout.fillWidth: true
        spacing: 6

        Flow {
            Layout.fillWidth: true
            spacing: 4
            Repeater {
                model: root.providers
                delegate: Rectangle {
                    property bool isCur: root.setupProvider === modelData.id
                    implicitWidth: pText.implicitWidth + 14
                    implicitHeight: 22
                    radius: Theme.r(4)
                    color: isCur ? Theme.accent : Theme.bgCard
                    border.color: modelData.ready ? Theme.accentGreen : Theme.border
                    border.width: 1
                    Text {
                        font.family: Theme.fontFamily
                        id: pText
                        anchors.centerIn: parent
                        text: modelData.name
                        textFormat: Text.PlainText
                        font.pixelSize: Theme.px(9)
                        font.weight: parent.isCur ? Font.Bold : Font.Normal
                        color: parent.isCur ? Theme.bgBase : Theme.textMain
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.setupProvider = modelData.id;
                            modelInput.text = root.choice.provider === modelData.id ? (root.choice.model || "") : modelData.default_model;
                        }
                    }
                }
            }
        }

        Text {
            font.family: Theme.fontFamily
            visible: root.setupProvider !== ""
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(9)
            color: Theme.textMuted
            text: {
                for (var i = 0; i < root.providers.length; i++) {
                    var p = root.providers[i];
                    if (p.id !== root.setupProvider) continue;
                    if (p.local) return p.ready ? qsTr("Running. Models: %1").arg(p.models.join(", ")) : qsTr("Not running. Start it to use local models.");
                    return p.ready ? qsTr("API key stored.") : qsTr("No API key stored yet.");
                }
                return "";
            }
        }

        InputBox {
            visible: root.setupProvider !== ""
            BoundedTextInput {
                id: modelInput
                anchors.fill: parent
                anchors.margins: 6
                verticalAlignment: TextEdit.AlignVCenter
                maxLength: 120
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(10)
                color: Theme.textMain
                placeholderText: qsTr("Model (empty: provider default or first local model)")
                placeholderTextColor: Theme.textDim
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            visible: {
                for (var i = 0; i < root.providers.length; i++)
                    if (root.providers[i].id === root.setupProvider) return !root.providers[i].local;
                return false;
            }
            InputBox {
                TextInput {
                    id: keyInput
                    anchors.fill: parent
                    anchors.margins: 6
                    verticalAlignment: TextInput.AlignVCenter
                    echoMode: TextInput.Password
                    maximumLength: 512
                    clip: true
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    color: Theme.textMain
                    onAccepted: root.storeKey()
                    Text {
                        font.family: Theme.fontFamily
                        visible: keyInput.text.length === 0
                        anchors.verticalCenter: parent.verticalCenter
                        text: qsTr("API key")
                        textFormat: Text.PlainText
                        font.pixelSize: Theme.px(10)
                        color: Theme.textDim
                    }
                }
            }
            SmallButton {
                label: qsTr("Store key")
                enabled: keyInput.text.length > 0 && !setupProc.running
                onClicked: root.storeKey()
            }
        }

        SmallButton {
            visible: root.setupProvider !== ""
            Layout.fillWidth: true
            label: qsTr("Use %1").arg(root.providerName(root.setupProvider))
            enabled: !setupProc.running
            onClicked: root.useProvider()
        }
    }
}
