import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

// Fujifilm workspace: the recipe library, a camera on USB (custom settings C1-C7 and
// in-camera RAW conversion) and Pixel Shift Multi-Shot merging. Every action runs a
// one-shot `omastudio-engine fuji-*` command.
Rectangle {
    id: root
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 16
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property string enginePath: ""
    property string activePhotoPath: ""
    // Filmstrip selection, for Pixel Shift
    property var selectedPaths: []

    property var recipes: []
    property string selectedRecipePath: ""
    property var camera: null
    property var presets: []
    property int targetSlot: 1
    property string busyAction: ""

    readonly property bool activeIsFuji: /\.(raf|jpe?g)$/i.test(activePhotoPath)
    readonly property bool activeIsRaf: /\.raf$/i.test(activePhotoPath)
    readonly property var shiftFrames: selectedPaths.filter(function(p) { return /\.raf$/i.test(p); })

    // A develop recipe to apply; label names it in the toast
    signal recipeApplied(var recipe, string label)
    // A file the panel produced (camera JPEG, Pixel Shift DNG)
    signal fileProduced(string path, bool openIt)
    signal notify(string text, bool isError)

    Component.onCompleted: refreshLibrary()

    function run(action, args) {
        if (root.enginePath === "" || engineProc.running) return false;
        root.busyAction = action;
        engineProc.action = action;
        engineProc.command = [root.enginePath].concat(args);
        engineProc.running = true;
        return true;
    }

    function refreshLibrary() { run("list", ["fuji-recipe", "list"]); }

    function selectedRecipe() {
        for (var i = 0; i < root.recipes.length; i++)
            if (root.recipes[i].path === root.selectedRecipePath) return root.recipes[i];
        return null;
    }

    function handle(action, resp) {
        if (!resp.success) {
            root.notify(String(resp.error || qsTr("Fujifilm command failed")).split("\n")[0], true);
            return;
        }
        var d = resp.data;
        switch (action) {
        case "list":
            root.recipes = Array.isArray(d) ? d : [];
            break;
        case "read":
            root.recipeApplied(d.omastudio, d.label || qsTr("Camera recipe"));
            break;
        case "save":
        case "import":
            root.selectedRecipePath = d.path || "";
            root.notify(qsTr("Recipe saved to the library"), false);
            refreshLibrary();
            return;
        case "camera":
            root.camera = d;
            root.presets = [];
            break;
        case "presets":
            root.presets = Array.isArray(d) ? d : [];
            break;
        case "write":
            var refused = d.refused_properties || [];
            root.notify(refused.length ? qsTr("Saved to C%1 (%n setting(s) refused by the camera)", "", refused.length).arg(d.slot) : qsTr("Saved to C%1").arg(d.slot), refused.length > 0);
            break;
        case "convert":
            root.notify(qsTr("Camera JPEG: %1").arg(d.path), false);
            root.fileProduced(d.path, false);
            break;
        case "shift":
            root.notify((d.coverage < 1 ? qsTr("Pixel Shift DNG: %1 x %2, incomplete sequence") : qsTr("Pixel Shift DNG: %1 x %2")).arg(d.width).arg(d.height), d.coverage < 1);
            root.fileProduced(d.output, true);
            break;
        }
    }

    Process {
        id: engineProc
        property string action: ""
        onFinished: function(exitCode, output) {
            var action = engineProc.action;
            root.busyAction = "";
            var resp;
            try {
                resp = JSON.parse(String(output || ""));
            } catch (e) {
                root.notify(qsTr("Fujifilm command gave no answer"), true);
                return;
            }
            root.handle(action, resp);
        }
    }

    // Recipe files to import: photos, X RAW Studio profiles and recipe text
    Process {
        id: pickProc
        command: ["zenity", "--file-selection", "--title=" + qsTr("Import Fujifilm Recipe"),
                  "--file-filter=" + qsTr("Fujifilm recipes") + " | *.RAF *.raf *.JPG *.jpg *.FP1 *.fp1 *.FP2 *.fp2 *.FP3 *.fp3 *.txt"]
        onFinished: function(exitCode, output) {
            var p = String(output || "").trim();
            if (exitCode === 0 && p.length > 0) root.run("import", ["fuji-recipe", "save", p]);
        }
    }

    component PanelButton: Rectangle {
        id: btn
        property string label: ""
        property bool primary: false
        property bool busy: false
        signal clicked()
        implicitWidth: btnText.implicitWidth + 16
        implicitHeight: 24
        radius: Theme.r(4)
        opacity: enabled ? 1.0 : 0.4
        color: primary ? (btnMouse.containsMouse ? Theme.accentHover : Theme.accent)
                       : (btnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
        border.color: primary ? Theme.accent : Theme.border
        border.width: 1

        Text {
            id: btnText
            anchors.centerIn: parent
            text: btn.busy ? "..." : btn.label
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(9)
            font.weight: Font.Bold
            color: btn.primary ? Theme.bgBase : Theme.textMain
        }

        MouseArea {
            id: btnMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: btn.clicked()
        }
    }

    component SectionTitle: Text {
        textFormat: Text.PlainText
        font.family: Theme.fontFamily
        font.pixelSize: Theme.px(10)
        font.weight: Font.DemiBold
        font.letterSpacing: 1
        color: Theme.textDim
        Layout.fillWidth: true
    }

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 8

        // Recipe library
        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            SectionTitle { text: qsTr("FUJIFILM RECIPES") }
            PanelButton {
                label: qsTr("IMPORT")
                enabled: root.busyAction === "" && !pickProc.running
                onClicked: pickProc.running = true
            }
            PanelButton {
                label: Theme.iconRefresh
                busy: root.busyAction === "list"
                enabled: root.busyAction === ""
                onClicked: root.refreshLibrary()
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            visible: root.activeIsFuji
            PanelButton {
                Layout.fillWidth: true
                label: qsTr("APPLY PHOTO'S RECIPE")
                busy: root.busyAction === "read"
                enabled: root.busyAction === ""
                onClicked: root.run("read", ["fuji-recipe", root.activePhotoPath])
            }
            PanelButton {
                label: qsTr("SAVE")
                busy: root.busyAction === "save"
                enabled: root.busyAction === ""
                onClicked: root.run("save", ["fuji-recipe", "save", root.activePhotoPath])
            }
        }

        Text {
            font.family: Theme.fontFamily
            visible: root.recipes.length === 0
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: qsTr("No saved recipes. Import an X RAW Studio profile, recipe text or a Fujifilm photo.")
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(10)
            color: Theme.textMuted
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4

            Repeater {
                model: root.recipes
                delegate: Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 34
                    radius: Theme.radiusSm
                    property bool isCur: root.selectedRecipePath === modelData.path
                    color: isCur ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18)
                                 : (rowMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
                    border.color: isCur ? Theme.accent : Theme.border
                    border.width: 1

                    ColumnLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        spacing: 0
                        Text {
                            font.family: Theme.fontFamily
                            Layout.fillWidth: true
                            text: modelData.recipe.label
                            elide: Text.ElideRight
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(11)
                            font.weight: Font.Bold
                            color: parent.parent.isCur ? Theme.accent : Theme.textMain
                        }
                        Text {
                            font.family: Theme.fontFamily
                            Layout.fillWidth: true
                            text: modelData.recipe.recipe.film_simulation_name + (modelData.recipe.device ? "  ·  " + modelData.recipe.device : "")
                            elide: Text.ElideRight
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(9)
                            color: Theme.textMuted
                        }
                    }

                    MouseArea {
                        id: rowMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.selectedRecipePath = modelData.path
                        onDoubleClicked: root.recipeApplied(modelData.recipe.omastudio, modelData.recipe.label)
                    }
                }
            }
        }

        PanelButton {
            Layout.fillWidth: true
            visible: root.selectedRecipe() !== null
            primary: true
            label: qsTr("APPLY SELECTED RECIPE")
            onClicked: {
                var r = root.selectedRecipe();
                if (r) root.recipeApplied(r.recipe.omastudio, r.recipe.label);
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }

        // Camera on USB
        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            SectionTitle { text: qsTr("CAMERA (USB)") }
            PanelButton {
                label: root.camera ? qsTr("RECONNECT") : qsTr("CONNECT")
                busy: root.busyAction === "camera"
                enabled: root.busyAction === ""
                onClicked: root.run("camera", ["fuji-camera", "info"])
            }
        }

        Text {
            font.family: Theme.fontFamily
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(10)
            color: root.camera ? Theme.textMain : Theme.textMuted
            text: root.camera
                ? (root.camera.raw_conversion ? qsTr("%1  ·  in-camera RAW conversion") : "%1").arg(root.camera.device.model || qsTr("Fujifilm camera"))
                : qsTr("Set the camera's USB mode to %1 and connect it.").arg("USB RAW CONV./BACKUP RESTORE")
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 6
            visible: root.camera !== null

            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                PanelButton {
                    Layout.fillWidth: true
                    label: qsTr("READ C1-C7")
                    busy: root.busyAction === "presets"
                    enabled: root.busyAction === ""
                    onClicked: root.run("presets", ["fuji-camera", "presets"])
                }
                PanelButton {
                    Layout.fillWidth: true
                    label: qsTr("CONVERT IN CAMERA")
                    busy: root.busyAction === "convert"
                    enabled: root.busyAction === "" && root.activeIsRaf && root.camera !== null && root.camera.raw_conversion
                    onClicked: {
                        var args = ["fuji-camera", "convert", root.activePhotoPath];
                        if (root.selectedRecipePath !== "") args = args.concat(["--recipe", root.selectedRecipePath]);
                        root.run("convert", args);
                    }
                }
            }

            Repeater {
                model: root.presets
                delegate: Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 26
                    radius: Theme.r(4)
                    color: presetMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                    border.color: Theme.border
                    border.width: 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        spacing: 8
                        Text {
                            font.family: Theme.fontFamily
                            text: "C" + modelData.slot
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            font.weight: Font.Bold
                            color: Theme.accent
                        }
                        Text {
                            font.family: Theme.fontFamily
                            Layout.fillWidth: true
                            text: (modelData.name || qsTr("Unnamed")) + "  ·  " + modelData.recipe.film_simulation_name
                            elide: Text.ElideRight
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(10)
                            color: Theme.textMain
                        }
                    }

                    MouseArea {
                        id: presetMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.recipeApplied(modelData.omastudio, "C" + modelData.slot + " " + modelData.name)
                    }
                }
            }

            // Write the selected library recipe to a custom setting slot
            RowLayout {
                Layout.fillWidth: true
                spacing: 4
                visible: root.selectedRecipe() !== null

                Repeater {
                    model: 7
                    delegate: Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 22
                        radius: Theme.r(4)
                        property bool isCur: root.targetSlot === index + 1
                        color: isCur ? Theme.accent : Theme.bgCard
                        border.color: isCur ? Theme.accent : Theme.border
                        border.width: 1
                        Text {
                            font.family: Theme.fontFamily
                            anchors.centerIn: parent
                            text: "C" + (index + 1)
                            textFormat: Text.PlainText
                            font.pixelSize: Theme.px(9)
                            font.weight: Font.Bold
                            color: parent.isCur ? Theme.bgBase : Theme.textMain
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.targetSlot = index + 1
                        }
                    }
                }
            }

            PanelButton {
                Layout.fillWidth: true
                visible: root.selectedRecipe() !== null
                label: qsTr("WRITE SELECTED RECIPE TO C%1").arg(root.targetSlot)
                busy: root.busyAction === "write"
                enabled: root.busyAction === ""
                onClicked: root.run("write", ["fuji-camera", "write-preset", String(root.targetSlot), root.selectedRecipePath])
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }

        // Pixel Shift Multi-Shot
        SectionTitle { text: qsTr("PIXEL SHIFT MULTI-SHOT") }

        Text {
            font.family: Theme.fontFamily
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(10)
            color: Theme.textMuted
            text: root.shiftFrames.length >= 2
                ? qsTr("%n RAF frames selected. 4 frames give full colour per pixel, 16 frames double the resolution.", "", root.shiftFrames.length)
                : qsTr("Select the RAF frames of one GFX Pixel Shift sequence in the filmstrip.")
        }

        PanelButton {
            Layout.fillWidth: true
            primary: true
            label: root.shiftFrames.length >= 2 ? qsTr("MERGE %n FRAMES TO DNG", "", root.shiftFrames.length) : qsTr("MERGE FRAMES TO DNG")
            busy: root.busyAction === "shift"
            enabled: root.busyAction === "" && root.shiftFrames.length >= 2
            onClicked: root.run("shift", ["fuji-pixel-shift"].concat(root.shiftFrames))
        }
    }
}
