import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import "theme"
import "components"

Rectangle {
    id: root
    width: 1360
    height: 860
    color: Theme.bgBase

    // State Variables
    property string activePhotoPath: ""
    property string currentRenderPath: "/dev/shm/omastudio_viewport.ppm"
    property var activeMetadata: null
    property var activeHistogram: null
    property var activeScene: null
    property var photoList: []

    property bool isProMode: false
    property bool isSplitView: false
    property real splitRatio: 0.5
    property bool showExportModal: false

    // Lightroom-grade Recipe State
    property real wbTemp: 5500.0
    property real wbTint: 0.0
    property real expValue: 0.0
    property real contrastValue: 0.0
    property real highlightsValue: 0.0
    property real shadowsValue: 0.0
    property real whitesValue: 0.0
    property real blacksValue: 0.0
    property real textureValue: 0.0
    property real clarityValue: 0.0
    property real dehazeValue: 0.0
    property real vibranceValue: 0.0
    property real saturationValue: 0.0

    property real curveHighlights: 0.0
    property real curveLights: 0.0
    property real curveDarks: 0.0
    property real curveShadows: 0.0

    property var hslH: [0, 0, 0, 0, 0, 0, 0, 0]
    property var hslS: [0, 0, 0, 0, 0, 0, 0, 0]
    property var hslL: [0, 0, 0, 0, 0, 0, 0, 0]

    property real sharpnessVal: 25.0
    property real denoiseLumVal: 0.0
    property real denoiseColVal: 10.0
    property real vignetteVal: 0.0

    // DaVinci Resolve 3-Way Color Wheels
    property var liftRgb: [0.0, 0.0, 0.0]
    property real liftLuma: 0.0
    property var gammaRgb: [0.0, 0.0, 0.0]
    property real gammaLuma: 0.0
    property var gainRgb: [0.0, 0.0, 0.0]
    property real gainLuma: 0.0
    property var offsetRgb: [0.0, 0.0, 0.0]
    property real offsetLuma: 0.0

    // Optics & Crop State
    property real defringeVal: 0.0
    property real lensDistortionVal: 0.0
    property real cropX: 0.0
    property real cropY: 0.0
    property real cropW: 1.0
    property real cropH: 1.0
    property string cropAspect: "Original"

    // Clipboard & Toast Notification State
    property var copiedRecipe: null
    property string toastMessage: ""
    property color toastColor: Theme.accent
    property string currentGdrivePath: "Photos"
    property bool isDownloadingRemote: false
    property bool daemonReady: false
    property var daemonQueue: []

    function resolveEnginePath() {
        return Quickshell.env("HOME") + "/.local/bin/omastudio-engine";
    }

    function showToast(msg, col) {
        root.toastMessage = msg;
        root.toastColor = col || Theme.accent;
        toastTimer.restart();
    }

    Timer {
        id: toastTimer
        interval: 2800
        repeat: false
        onTriggered: root.toastMessage = ""
    }

    // Undo / Redo History Stack
    property var undoStack: []
    property var redoStack: []
    property bool isPerformingUndoRedo: false

    function pushUndoState() {
        if (root.isPerformingUndoRedo) return;
        var r = root.buildRecipeObject();
        var stack = root.undoStack.slice();
        if (stack.length >= 50) stack.shift();
        stack.push(r);
        root.undoStack = stack;
        root.redoStack = [];
    }

    function undo() {
        if (root.undoStack.length === 0) {
            root.showToast("[Undo] No previous actions", Theme.textDim);
            return;
        }
        root.isPerformingUndoRedo = true;
        var cur = root.buildRecipeObject();
        var rStack = root.redoStack.slice();
        rStack.push(cur);
        root.redoStack = rStack;

        var uStack = root.undoStack.slice();
        var prev = uStack.pop();
        root.undoStack = uStack;

        root.applyRecipeObject(prev);
        root.isPerformingUndoRedo = false;
        root.showToast("[OK] Undo (Ctrl+Z)", Theme.accentCyan);
    }

    function redo() {
        if (root.redoStack.length === 0) {
            root.showToast("[Redo] No actions to redo", Theme.textDim);
            return;
        }
        root.isPerformingUndoRedo = true;
        var cur = root.buildRecipeObject();
        var uStack = root.undoStack.slice();
        uStack.push(cur);
        root.undoStack = uStack;

        var rStack = root.redoStack.slice();
        var next = rStack.pop();
        root.redoStack = rStack;

        root.applyRecipeObject(next);
        root.isPerformingUndoRedo = false;
        root.showToast("[OK] Redo (Ctrl+Shift+Z)", Theme.accentCyan);
    }

    function copyRecipe() {
        root.copiedRecipe = root.buildRecipeObject();
        root.showToast("[OK] Adjustments Copied (Ctrl+Shift+C)", Theme.accentGreen);
    }

    function pasteRecipe() {
        if (!root.copiedRecipe) {
            root.showToast("[ERR] No adjustments in clipboard", Theme.accentMagenta);
            return;
        }
        root.pushUndoState();
        root.applyRecipeObject(root.copiedRecipe);
        root.showToast("[OK] Adjustments Pasted (Ctrl+Shift+V)", Theme.accentGreen);
    }

    function resetRecipe() {
        root.pushUndoState();
        root.wbTemp = 5500.0;
        root.wbTint = 0.0;
        root.expValue = 0.0;
        root.contrastValue = 0.0;
        root.highlightsValue = 0.0;
        root.shadowsValue = 0.0;
        root.whitesValue = 0.0;
        root.blacksValue = 0.0;
        root.textureValue = 0.0;
        root.clarityValue = 0.0;
        root.dehazeValue = 0.0;
        root.vibranceValue = 0.0;
        root.saturationValue = 0.0;
        root.curveHighlights = 0.0;
        root.curveLights = 0.0;
        root.curveDarks = 0.0;
        root.curveShadows = 0.0;
        root.hslH = [0, 0, 0, 0, 0, 0, 0, 0];
        root.hslS = [0, 0, 0, 0, 0, 0, 0, 0];
        root.hslL = [0, 0, 0, 0, 0, 0, 0, 0];
        root.sharpnessVal = 25.0;
        root.denoiseLumVal = 0.0;
        root.denoiseColVal = 10.0;
        root.vignetteVal = 0.0;
        root.liftRgb = [0.0, 0.0, 0.0];
        root.liftLuma = 0.0;
        root.gammaRgb = [0.0, 0.0, 0.0];
        root.gammaLuma = 0.0;
        root.gainRgb = [0.0, 0.0, 0.0];
        root.gainLuma = 0.0;
        root.offsetRgb = [0.0, 0.0, 0.0];
        root.offsetLuma = 0.0;
        root.defringeVal = 0.0;
        root.lensDistortionVal = 0.0;
        root.cropX = 0.0;
        root.cropY = 0.0;
        root.cropW = 1.0;
        root.cropH = 1.0;
        root.cropAspect = "Original";
        viewport.resetCrop();
        viewport.rotationAngle = 0.0;
        viewport.flipH = false;
        viewport.flipV = false;
        if (typeof presetSel !== "undefined" && presetSel) presetSel.activePreset = "";
        if (typeof socialOpt !== "undefined" && socialOpt) socialOpt.activePlatform = "";
        if (typeof hslMixer !== "undefined" && hslMixer) hslMixer.resetAll();
        if (typeof colorWheels !== "undefined" && colorWheels) colorWheels.resetAllWheels();
        root.showToast("[OK] Reset to defaults (Ctrl+R)", Theme.textDim);
        root.requestRender();
    }

    function resetSocialOptimization() {
        root.pushUndoState();
        root.cropX = 0.0;
        root.cropY = 0.0;
        root.cropW = 1.0;
        root.cropH = 1.0;
        root.cropAspect = "Original";
        viewport.resetCrop();
        root.sharpnessVal = 25.0;
        root.clarityValue = 0.0;
        root.vibranceValue = 0.0;
        root.shadowsValue = 0.0;
        root.whitesValue = 0.0;
        if (typeof socialOpt !== "undefined" && socialOpt) {
            socialOpt.activePlatform = "";
        }
        root.requestRender();
        root.showToast("[OK] AI Social Framing & Adjustments Reset", Theme.accentCyan);
    }

    function buildRecipeObject() {
        return {
            "wb_temperature": root.wbTemp,
            "wb_tint": root.wbTint,
            "exposure": root.expValue,
            "contrast": root.contrastValue,
            "highlights": root.highlightsValue,
            "shadows": root.shadowsValue,
            "whites": root.whitesValue,
            "blacks": root.blacksValue,
            "texture": root.textureValue,
            "clarity": root.clarityValue,
            "dehaze": root.dehazeValue,
            "vibrance": root.vibranceValue,
            "saturation": root.saturationValue,
            "curve_highlights": root.curveHighlights,
            "curve_lights": root.curveLights,
            "curve_darks": root.curveDarks,
            "curve_shadows": root.curveShadows,
            "hsl_hue": root.hslH,
            "hsl_sat": root.hslS,
            "hsl_lum": root.hslL,
            "sharpness": root.sharpnessVal,
            "denoise_lum": root.denoiseLumVal,
            "denoise_col": root.denoiseColVal,
            "vignette": root.vignetteVal,
            "rotation": viewport.rotationAngle,
            "flip_h": viewport.flipH,
            "flip_v": viewport.flipV,
            "lift": root.liftRgb,
            "lift_luma": root.liftLuma,
            "gamma": root.gammaRgb,
            "gamma_luma": root.gammaLuma,
            "gain": root.gainRgb,
            "gain_luma": root.gainLuma,
            "offset": root.offsetRgb,
            "offset_luma": root.offsetLuma,
            "defringe": root.defringeVal,
            "lens_distortion": root.lensDistortionVal,
            "crop_x": root.cropX,
            "crop_y": root.cropY,
            "crop_w": root.cropW,
            "crop_h": root.cropH,
            "crop_aspect": root.cropAspect,
            "preset_name": null
        };
    }

    function applyRecipeObject(r) {
        if (!r) return;
        root.wbTemp = Number(r.wb_temperature) || 5500.0;
        root.wbTint = Number(r.wb_tint) || 0.0;
        root.expValue = Number(r.exposure) || 0.0;
        root.contrastValue = Number(r.contrast) || 0.0;
        root.highlightsValue = Number(r.highlights) || 0.0;
        root.shadowsValue = Number(r.shadows) || 0.0;
        root.whitesValue = Number(r.whites) || 0.0;
        root.blacksValue = Number(r.blacks) || 0.0;
        root.textureValue = Number(r.texture) || 0.0;
        root.clarityValue = Number(r.clarity) || 0.0;
        root.dehazeValue = Number(r.dehaze) || 0.0;
        root.vibranceValue = Number(r.vibrance) || 0.0;
        root.saturationValue = Number(r.saturation) || 0.0;
        root.curveHighlights = Number(r.curve_highlights) || 0.0;
        root.curveLights = Number(r.curve_lights) || 0.0;
        root.curveDarks = Number(r.curve_darks) || 0.0;
        root.curveShadows = Number(r.curve_shadows) || 0.0;
        root.hslH = r.hsl_hue || [0,0,0,0,0,0,0,0];
        root.hslS = r.hsl_sat || [0,0,0,0,0,0,0,0];
        root.hslL = r.hsl_lum || [0,0,0,0,0,0,0,0];
        root.sharpnessVal = Number(r.sharpness) || 25.0;
        root.denoiseLumVal = Number(r.denoise_lum) || 0.0;
        root.denoiseColVal = Number(r.denoise_col) || 10.0;
        root.vignetteVal = Number(r.vignette) || 0.0;
        if (r.rotation !== undefined) viewport.rotationAngle = Number(r.rotation) || 0.0;
        if (r.flip_h !== undefined) viewport.flipH = Boolean(r.flip_h);
        if (r.flip_v !== undefined) viewport.flipV = Boolean(r.flip_v);

        // Color Wheels
        if (r.lift) root.liftRgb = r.lift;
        if (r.lift_luma !== undefined) root.liftLuma = Number(r.lift_luma) || 0.0;
        if (r.gamma) root.gammaRgb = r.gamma;
        if (r.gamma_luma !== undefined) root.gammaLuma = Number(r.gamma_luma) || 0.0;
        if (r.gain) root.gainRgb = r.gain;
        if (r.gain_luma !== undefined) root.gainLuma = Number(r.gain_luma) || 0.0;
        if (r.offset) root.offsetRgb = r.offset;
        if (r.offset_luma !== undefined) root.offsetLuma = Number(r.offset_luma) || 0.0;

        // Optics
        if (r.defringe !== undefined) root.defringeVal = Number(r.defringe) || 0.0;
        if (r.lens_distortion !== undefined) root.lensDistortionVal = Number(r.lens_distortion) || 0.0;

        // Crop
        if (r.crop_x !== undefined) {
            root.cropX = Number(r.crop_x) || 0.0;
            root.cropY = Number(r.crop_y) || 0.0;
            root.cropW = Number(r.crop_w) || 1.0;
            root.cropH = Number(r.crop_h) || 1.0;
            root.cropAspect = r.crop_aspect || "Original";
            viewport.cropX = root.cropX;
            viewport.cropY = root.cropY;
            viewport.cropW = root.cropW;
            viewport.cropH = root.cropH;
            viewport.cropAspect = root.cropAspect;
        }

        requestRender();
    }

    function sendDaemonCommand(obj) {
        if (!daemonProc.running) {
            daemonProc.running = true;
        }
        if (root.daemonReady) {
            daemonProc.write(JSON.stringify(obj) + "\n");
        } else {
            root.daemonQueue.push(obj);
        }
    }

    function handleDaemonMessage(line) {
        var trimmed = String(line || "").trim();
        if (trimmed.length === 0) return;
        try {
            var resp = JSON.parse(trimmed);
            if (!resp.success) {
                console.warn("Daemon returned error for action " + resp.action + ": " + resp.error);
                if (resp.action === "load") {
                    root.showToast("[ERR] Failed to load RAW: " + (resp.error || "Unknown error"), Theme.highlightClip);
                } else if (resp.action === "ai_auto" || resp.action === "ai_social") {
                    root.showToast("[ERR] " + (resp.error || "AI action failed"), Theme.highlightClip);
                }
                return;
            }

            var act = resp.action;
            var data = resp.data;

            if (act === "load") {
                if (data) {
                    root.activeMetadata = data.metadata;
                    root.activeScene = data.scene;
                    root.applyRecipeObject(data.recipe);
                }
            } else if (act === "adjust") {
                if (data) {
                    root.activeHistogram = data.histogram;
                    viewport.imageSource = data.image;
                    navigator.imageSource = data.image;
                }
            } else if (act === "ai_auto") {
                if (data) {
                    root.applyRecipeObject(data.recipe);
                    root.activeScene = data.scene;
                    root.showToast("[OK] AI Auto Tone Applied", Theme.accentCyan);
                }
            } else if (act === "ai_social") {
                if (data) {
                    if (data.recipe) {
                        root.applyRecipeObject(data.recipe);
                    }
                    root.showToast("[OK] " + data.platform + " applied (" + data.target_resolution + ")", Theme.accentCyan);
                }
            } else if (act === "save_recipe") {
                root.showToast("[OK] Recipe sidecar saved", Theme.accentGreen);
            }
        } catch(e) {
            console.error("Error parsing daemon message:", e, line);
        }
    }

    function saveSidecar() {
        root.sendDaemonCommand({
            cmd: "save_recipe",
            recipe: root.buildRecipeObject()
        });
    }

    function triggerAiAuto() {
        root.pushUndoState();
        root.showToast("[AI] Analyzing dynamic range & scene...", Theme.accentCyan);
        root.sendDaemonCommand({ cmd: "ai_auto" });
    }

    function requestRender() {
        if (renderDebounce.running) {
            renderDebounce.restart();
        } else {
            renderDebounce.start();
        }
    }

    Timer {
        id: renderDebounce
        interval: 20
        repeat: false
        onTriggered: {
            if (root.activePhotoPath !== "") {
                var splitVal = root.isSplitView ? root.splitRatio : 0.0;
                root.sendDaemonCommand({
                    cmd: "adjust",
                    recipe: root.buildRecipeObject(),
                    split: splitVal,
                    highlight_mask: viewport.showHighlightMask,
                    shadow_mask: viewport.showShadowMask
                });
            }
        }
    }

    // Persistent High-Speed Rust Engine Daemon
    Process {
        id: daemonProc
        command: [root.resolveEnginePath(), "daemon"]
        running: true
        stdinEnabled: true

        stdout: SplitParser {
            splitMarker: "\n"
            onRead: function(line) {
                root.handleDaemonMessage(line);
            }
        }

        onStarted: {
            root.daemonReady = true;
            while (root.daemonQueue.length > 0) {
                var cmd = root.daemonQueue.shift();
                daemonProc.write(JSON.stringify(cmd) + "\n");
            }
        }

        onExited: function(exitCode, exitStatus) {
            root.daemonReady = false;
        }
    }

    // Process: File picker
    Process {
        id: openFileProc
        command: ["zenity", "--file-selection", "--title=Open RAW Photo", "--file-filter=RAW Photos | *.RAF *.raf *.NEF *.nef *.CR2 *.cr2 *.CR3 *.cr3 *.ARW *.arw *.DNG *.dng"]
        stdout: StdioCollector {
            waitForEnd: true
            onStreamFinished: {
                var p = String(text || "").trim();
                if (p.length > 0) {
                    root.loadPhoto(p);
                }
            }
        }
    }

    // Process: Export
    Process {
        id: exportProc
        stdout: StdioCollector {
            waitForEnd: true
            onStreamFinished: {
                exportDialog.isExporting = false;
                try {
                    var resp = JSON.parse(text);
                    if (resp.success && resp.data) {
                        exportDialog.exportStatusText = "[OK] Saved to: " + resp.data.exported_path;
                    } else {
                        exportDialog.exportStatusText = "[ERR] Export Error: " + (resp.error || "Unknown");
                    }
                } catch(e) {
                    exportDialog.exportStatusText = "[ERR] Export finished";
                }
            }
        }
    }

    // Process: Fetch GDrive Remote RAW
    Process {
        id: fetchGdriveProc
        stdout: StdioCollector {
            waitForEnd: true
            onStreamFinished: {
                root.isDownloadingRemote = false;
                try {
                    var resp = JSON.parse(text);
                    if (resp.success && resp.data) {
                        root.loadPhoto(resp.data);
                        var fName = resp.data.split("/").pop();
                        root.showToast("[OK] Downloaded cloud RAW: " + fName, Theme.accentCyan);
                    } else {
                        root.showToast("[ERR] Cloud fetch failed: " + (resp.error || "Unknown"), Theme.accentMagenta);
                    }
                } catch(e) {
                    root.showToast("[ERR] Cloud fetch parse error", Theme.accentMagenta);
                }
            }
        }
    }

    // Process: Folder scan
    Process {
        id: listProc
        stdout: StdioCollector {
            waitForEnd: true
            onStreamFinished: {
                try {
                    var resp = JSON.parse(text);
                    if (resp.success && resp.data) {
                        root.photoList = resp.data;
                    }
                } catch(e) {}
            }
        }
    }

    // Keyboard Shortcuts (Lightroom & Studio Standards)
    Shortcut {
        sequence: "Ctrl+Z"
        onActivated: root.undo()
    }
    Shortcut {
        sequences: ["Ctrl+Shift+Z", "Ctrl+Y"]
        onActivated: root.redo()
    }
    Shortcut {
        sequence: "Ctrl+S"
        onActivated: root.saveSidecar()
    }
    Shortcut {
        sequence: "Ctrl+Shift+C"
        onActivated: root.copyRecipe()
    }
    Shortcut {
        sequence: "Ctrl+Shift+V"
        onActivated: root.pasteRecipe()
    }
    Shortcut {
        sequence: "Ctrl+R"
        onActivated: root.resetRecipe()
    }
    Shortcut {
        sequence: "C"
        onActivated: {
            viewport.isCropMode = !viewport.isCropMode;
            root.showToast(viewport.isCropMode ? "Crop Mode: ON (C)" : "Crop Mode: OFF", Theme.accentYellow);
        }
    }
    Shortcut {
        sequence: "Y"
        onActivated: {
            root.isSplitView = !root.isSplitView;
            root.requestRender();
        }
    }

    function loadPhoto(path) {
        root.activePhotoPath = path;
        root.sendDaemonCommand({
            cmd: "load",
            path: path
        });
    }

    function scanLocalFolder(dir) {
        listProc.command = [root.resolveEnginePath(), "scan", dir];
        listProc.running = true;
    }

    function applyPresetNamed(name) {
        root.pushUndoState();
        if (name === "Fuji Classic Chrome") {
            root.contrastValue = 15.0;
            root.highlightsValue = -10.0;
            root.shadowsValue = 5.0;
            root.clarityValue = 12.0;
            root.vibranceValue = -15.0;
            root.saturationValue = -8.0;
            root.vignetteVal = -12.0;
        } else if (name === "Fuji Velvia 50") {
            root.contrastValue = 28.0;
            root.highlightsValue = -15.0;
            root.shadowsValue = 10.0;
            root.vibranceValue = 30.0;
            root.saturationValue = 18.0;
            root.clarityValue = 16.0;
        } else if (name === "Kodak Portra 400") {
            root.wbTemp = 5700.0;
            root.wbTint = 4.0;
            root.contrastValue = -5.0;
            root.highlightsValue = -12.0;
            root.shadowsValue = 18.0;
            root.textureValue = -5.0;
            root.clarityValue = 6.0;
            root.vibranceValue = 10.0;
        } else if (name === "Leica Monochrom HC") {
            root.saturationValue = -100.0;
            root.vibranceValue = -100.0;
            root.contrastValue = 35.0;
            root.highlightsValue = -20.0;
            root.shadowsValue = -10.0;
            root.whitesValue = 15.0;
            root.blacksValue = -25.0;
            root.clarityValue = 25.0;
            root.sharpnessVal = 40.0;
            root.vignetteVal = -18.0;
        } else if (name === "Cinematic Teal & Orange") {
            root.contrastValue = 20.0;
            root.shadowsValue = -10.0;
            root.highlightsValue = -15.0;
            root.clarityValue = 15.0;
            root.vibranceValue = 20.0;
            root.vignetteVal = -20.0;
        }
        requestRender();
    }

    function triggerSocial(platformCode) {
        if (!platformCode || platformCode.length === 0) return;
        root.pushUndoState();
        root.showToast("[AI] Optimizing for " + platformCode + "...", Theme.accentCyan);
        root.sendDaemonCommand({
            cmd: "ai_social",
            platform: String(platformCode)
        });
    }

    function toggleCropMode() {
        viewport.isCropMode = !viewport.isCropMode;
        root.showToast(viewport.isCropMode ? "Crop Mode: ON (C)" : "Crop Mode: OFF", Theme.accentYellow);
        return viewport.isCropMode;
    }

    function toggleSplitView() {
        root.isSplitView = !root.isSplitView;
        root.requestRender();
        return root.isSplitView;
    }

    function setExposureEv(ev) {
        root.expValue = Math.max(-5.0, Math.min(5.0, Number(ev) || 0.0));
        root.requestRender();
    }

    function setWarmthKelvin(k) {
        root.wbTemp = Math.max(2000.0, Math.min(12000.0, Number(k) || 5500.0));
        root.requestRender();
    }

    // MAIN LAYOUT
    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // TOP BAR
        TopBar {
            Layout.fillWidth: true
            isProMode: root.isProMode
            isSplitView: root.isSplitView
            isCropMode: viewport.isCropMode
            activePhotoName: {
                var p = root.activePhotoPath.split("/");
                return p[p.length - 1];
            }
            onToggleMode: root.isProMode = !root.isProMode
            onUndoClicked: root.undo()
            onRedoClicked: root.redo()
            onOpenFileClicked: openFileProc.running = true
            onToggleSplit: {
                root.isSplitView = !root.isSplitView;
                requestRender();
            }
            onToggleCrop: {
                viewport.isCropMode = !viewport.isCropMode;
                root.showToast(viewport.isCropMode ? "Crop Mode: ON (C)" : "Crop Mode: OFF", Theme.accentYellow);
            }
            onZoomFit: viewport.resetZoomFit()
            onZoom100: viewport.setZoomAbsolute(1.0)
            onZoom200: viewport.setZoomAbsolute(2.0)
            onExportClicked: root.showExportModal = true
        }

        // WORKSPACE CENTER (LEFT: NAVIGATOR + HISTOGRAM + EXIF, CENTER: CANVAS, RIGHT: ADJUSTMENTS)
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // LEFT PANEL: NAVIGATOR & HISTOGRAM & METADATA (Lightroom Studio Layout)
            Rectangle {
                implicitWidth: 280
                Layout.fillHeight: true
                color: Theme.bgBase
                border.color: Theme.border
                border.width: 1

                ScrollView {
                    anchors.fill: parent
                    anchors.margins: 10
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width
                        spacing: 10

                        // 1. NAVIGATOR (Miniature Viewport with Draggable Crop Box)
                        Navigator {
                            id: navigator
                            Layout.fillWidth: true
                            zoomFactor: viewport.zoomFactor
                            rotationAngle: viewport.rotationAngle
                            normX: viewport.normX
                            normY: viewport.normY
                            normW: viewport.normW
                            normH: viewport.normH
                            onZoomRequested: function(z) {
                                if (z <= 0.05) viewport.resetZoomFit();
                                else viewport.setZoomAbsolute(z);
                            }
                            onPanRequested: function(nx, ny) {
                                viewport.setNormCenter(nx, ny);
                            }
                        }

                        // 2. LIVE RGB & LUMA HISTOGRAM (Positioned right below Navigator)
                        HistogramView {
                            Layout.fillWidth: true
                            histData: root.activeHistogram
                            showClippingHighlights: viewport.showHighlightMask
                            showClippingShadows: viewport.showShadowMask
                            onToggleHighlightMask: {
                                viewport.showHighlightMask = !viewport.showHighlightMask;
                                root.requestRender();
                            }
                            onToggleShadowMask: {
                                viewport.showShadowMask = !viewport.showShadowMask;
                                root.requestRender();
                            }
                        }

                        // 3. EXIF CAMERA METADATA
                        ExifCard {
                            Layout.fillWidth: true
                            metadata: root.activeMetadata
                        }

                        // 4. AI SCENE INSIGHT
                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 70
                            radius: Theme.radiusMd
                            color: Theme.bgDark
                            border.color: Theme.border
                            border.width: 1

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 8
                                spacing: 2

                                RowLayout {
                                    Text {
                                        text: Theme.iconAi
                                        font.family: Theme.iconFont
                                        font.pixelSize: 11
                                        color: Theme.accentPurple
                                    }
                                    Text {
                                        text: "AI SCENE INSIGHT"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 9
                                        font.weight: Font.Bold
                                        color: Theme.accentPurple
                                    }
                                    Item { Layout.fillWidth: true }
                                    Text {
                                        text: root.activeScene ? Math.round(root.activeScene.confidence * 100) + "%" : ""
                                        textFormat: Text.PlainText
                                        font.pixelSize: 9
                                        color: Theme.textDim
                                    }
                                }

                                Text {
                                    text: root.activeScene ? root.activeScene.scene_type : "Analyzing..."
                                    textFormat: Text.PlainText
                                    font.pixelSize: 11
                                    font.weight: Font.Bold
                                    color: Theme.textMain
                                }

                                Text {
                                    text: root.activeScene ? root.activeScene.description : "Extracting dynamic range..."
                                    textFormat: Text.PlainText
                                    font.pixelSize: 9
                                    color: Theme.textMuted
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }
                            }
                        }

                        Item { Layout.fillHeight: true }
                    }
                }
            }

            // CENTER CANVAS (With Mac-like Touchpad Pinch-to-Zoom & Kinetic Pan)
            Viewport {
                id: viewport
                Layout.fillWidth: true
                Layout.fillHeight: true
                isSplitView: root.isSplitView
                splitRatio: root.splitRatio
                onSplitRatioChangedByUser: function(r) {
                    root.splitRatio = r;
                    requestRender();
                }
                onRotationChangedByUser: function(a) {
                    root.requestRender();
                }
                onCropChangedByUser: function(cx, cy, cw, ch, aspect) {
                    root.cropX = cx;
                    root.cropY = cy;
                    root.cropW = cw;
                    root.cropH = ch;
                    root.cropAspect = aspect;
                }
            }

            // RIGHT ADJUSTMENT INSPECTOR (Dedicated Pure Tone & Color Controls)
            Rectangle {
                implicitWidth: 320
                Layout.fillHeight: true
                color: Theme.bgSurface
                border.color: Theme.border
                border.width: 1

                ScrollView {
                    anchors.fill: parent
                    anchors.margins: 12
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width
                        spacing: 12

                        // Film Presets at top of adjustments
                        PresetSelector {
                            id: presetSel
                            Layout.fillWidth: true
                            onApplyPreset: function(n) { root.applyPresetNamed(n) }
                            onTriggerAiAuto: {
                                root.triggerAiAuto();
                            }
                            onResetPreset: {
                                root.resetRecipe();
                            }
                        }

                        // AI Social Media Optimizer (One-click multi-platform framing & grading)
                        SocialOptimizer {
                            id: socialOpt
                            Layout.fillWidth: true
                            onTriggerSocialOptimize: function(platformCode) {
                                root.triggerSocial(platformCode);
                            }
                            onResetSocial: {
                                root.resetSocialOptimization();
                            }
                        }

                        // SIMPLE MODE: Only essential master sliders
                        ColumnLayout {
                            visible: !root.isProMode
                            Layout.fillWidth: true
                            spacing: 12

                            Rectangle {
                                Layout.fillWidth: true
                                height: 1
                                color: Theme.border
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "ESSENTIAL ADJUSTMENTS"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 1
                                    color: Theme.textDim
                                    Layout.fillWidth: true
                                }

                                Rectangle {
                                    implicitWidth: resetEssentialText.implicitWidth + 10
                                    implicitHeight: 18
                                    radius: 4
                                    color: resetEssentialMouse.containsMouse ? Theme.bgCardHover : "transparent"
                                    border.color: Theme.border
                                    border.width: 1

                                    Text {
                                        id: resetEssentialText
                                        anchors.centerIn: parent
                                        text: "RESET"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.weight: Font.Bold
                                        color: Theme.textDim
                                    }

                                    MouseArea {
                                        id: resetEssentialMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: {
                                            root.expValue = 0.0;
                                            root.wbTemp = 5500.0;
                                            root.vibranceValue = 0.0;
                                            root.contrastValue = 0.0;
                                            root.requestRender();
                                        }
                                    }
                                }
                            }

                            SliderGroup {
                                title: "Exposure (Light)"
                                from: -5.0
                                to: 5.0
                                value: root.expValue
                                defaultValue: 0.0
                                stepSize: 0.05
                                decimals: 2
                                suffix: " EV"
                                accentColor: Theme.accentYellow
                                onSliderMoved: function(v) { root.expValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Warmth (Temperature)"
                                from: 2000.0
                                to: 12000.0
                                value: root.wbTemp
                                defaultValue: 5500.0
                                stepSize: 50.0
                                suffix: " K"
                                accentColor: Theme.accentOrange
                                onSliderMoved: function(v) { root.wbTemp = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Vibrance (Color Punch)"
                                from: -100.0
                                to: 100.0
                                value: root.vibranceValue
                                defaultValue: 0.0
                                accentColor: Theme.accentCyan
                                onSliderMoved: function(v) { root.vibranceValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Contrast"
                                from: -100.0
                                to: 100.0
                                value: root.contrastValue
                                defaultValue: 0.0
                                accentColor: Theme.accent
                                onSliderMoved: function(v) { root.contrastValue = v; root.requestRender() }
                            }
                        }

                        // PRO MODE: Complete Lightroom RAW Suite
                        ColumnLayout {
                            visible: root.isProMode
                            Layout.fillWidth: true
                            spacing: 14

                            // White Balance
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "WHITE BALANCE"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 1
                                    color: Theme.textDim
                                    Layout.fillWidth: true
                                }

                                Rectangle {
                                    implicitWidth: resetWbText.implicitWidth + 10
                                    implicitHeight: 18
                                    radius: 4
                                    color: resetWbMouse.containsMouse ? Theme.bgCardHover : "transparent"
                                    border.color: Theme.border
                                    border.width: 1

                                    Text {
                                        id: resetWbText
                                        anchors.centerIn: parent
                                        text: "RESET"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.weight: Font.Bold
                                        color: Theme.textDim
                                    }

                                    MouseArea {
                                        id: resetWbMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: {
                                            root.wbTemp = 5500.0;
                                            root.wbTint = 0.0;
                                            root.requestRender();
                                        }
                                    }
                                }
                            }

                            SliderGroup {
                                title: "Temp (Kelvin)"
                                from: 2000.0
                                to: 12000.0
                                value: root.wbTemp
                                defaultValue: 5500.0
                                stepSize: 25.0
                                suffix: " K"
                                accentColor: Theme.accentOrange
                                onSliderMoved: function(v) { root.wbTemp = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Tint (Green / Magenta)"
                                from: -100.0
                                to: 100.0
                                value: root.wbTint
                                defaultValue: 0.0
                                accentColor: Theme.accentMagenta
                                onSliderMoved: function(v) { root.wbTint = v; root.requestRender() }
                            }

                            // Tone
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "LIGHT & DYNAMIC RANGE"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 1
                                    color: Theme.textDim
                                    Layout.fillWidth: true
                                }

                                Rectangle {
                                    implicitWidth: resetLightText.implicitWidth + 10
                                    implicitHeight: 18
                                    radius: 4
                                    color: resetLightMouse.containsMouse ? Theme.bgCardHover : "transparent"
                                    border.color: Theme.border
                                    border.width: 1

                                    Text {
                                        id: resetLightText
                                        anchors.centerIn: parent
                                        text: "RESET"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.weight: Font.Bold
                                        color: Theme.textDim
                                    }

                                    MouseArea {
                                        id: resetLightMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: {
                                            root.expValue = 0.0;
                                            root.contrastValue = 0.0;
                                            root.highlightsValue = 0.0;
                                            root.shadowsValue = 0.0;
                                            root.whitesValue = 0.0;
                                            root.blacksValue = 0.0;
                                            root.requestRender();
                                        }
                                    }
                                }
                            }

                            SliderGroup {
                                title: "Exposure"
                                from: -5.0
                                to: 5.0
                                value: root.expValue
                                defaultValue: 0.0
                                stepSize: 0.05
                                decimals: 2
                                suffix: " EV"
                                onSliderMoved: function(v) { root.expValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Contrast"
                                from: -100.0
                                to: 100.0
                                value: root.contrastValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.contrastValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Highlights"
                                from: -100.0
                                to: 100.0
                                value: root.highlightsValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.highlightsValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Shadows"
                                from: -100.0
                                to: 100.0
                                value: root.shadowsValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.shadowsValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Whites"
                                from: -100.0
                                to: 100.0
                                value: root.whitesValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.whitesValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Blacks"
                                from: -100.0
                                to: 100.0
                                value: root.blacksValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.blacksValue = v; root.requestRender() }
                            }

                            // Tone Curve (Parametric 4-Zone)
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "TONE CURVE (PARAMETRIC)"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 1
                                    color: Theme.textDim
                                    Layout.fillWidth: true
                                }

                                Rectangle {
                                    implicitWidth: resetCurveText.implicitWidth + 10
                                    implicitHeight: 18
                                    radius: 4
                                    color: resetCurveMouse.containsMouse ? Theme.bgCardHover : "transparent"
                                    border.color: Theme.border
                                    border.width: 1

                                    Text {
                                        id: resetCurveText
                                        anchors.centerIn: parent
                                        text: "RESET"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.weight: Font.Bold
                                        color: Theme.textDim
                                    }

                                    MouseArea {
                                        id: resetCurveMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: {
                                            root.pushUndoState();
                                            root.curveHighlights = 0.0;
                                            root.curveLights = 0.0;
                                            root.curveDarks = 0.0;
                                            root.curveShadows = 0.0;
                                            root.requestRender();
                                        }
                                    }
                                }
                            }

                            SliderGroup {
                                title: "Curve Highlights"
                                from: -100.0
                                to: 100.0
                                value: root.curveHighlights
                                defaultValue: 0.0
                                accentColor: Theme.accent
                                onSliderMoved: function(v) { root.curveHighlights = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Curve Lights (Upper Mids)"
                                from: -100.0
                                to: 100.0
                                value: root.curveLights
                                defaultValue: 0.0
                                accentColor: Theme.accentCyan
                                onSliderMoved: function(v) { root.curveLights = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Curve Darks (Lower Mids)"
                                from: -100.0
                                to: 100.0
                                value: root.curveDarks
                                defaultValue: 0.0
                                accentColor: Theme.accentYellow
                                onSliderMoved: function(v) { root.curveDarks = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Curve Shadows"
                                from: -100.0
                                to: 100.0
                                value: root.curveShadows
                                defaultValue: 0.0
                                accentColor: Theme.accentOrange
                                onSliderMoved: function(v) { root.curveShadows = v; root.requestRender() }
                            }

                            // Presence
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "PRESENCE & TEXTURE"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 1
                                    color: Theme.textDim
                                    Layout.fillWidth: true
                                }

                                Rectangle {
                                    implicitWidth: resetPresenceText.implicitWidth + 10
                                    implicitHeight: 18
                                    radius: 4
                                    color: resetPresenceMouse.containsMouse ? Theme.bgCardHover : "transparent"
                                    border.color: Theme.border
                                    border.width: 1

                                    Text {
                                        id: resetPresenceText
                                        anchors.centerIn: parent
                                        text: "RESET"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.weight: Font.Bold
                                        color: Theme.textDim
                                    }

                                    MouseArea {
                                        id: resetPresenceMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: {
                                            root.textureValue = 0.0;
                                            root.clarityValue = 0.0;
                                            root.dehazeValue = 0.0;
                                            root.vibranceValue = 0.0;
                                            root.saturationValue = 0.0;
                                            root.requestRender();
                                        }
                                    }
                                }
                            }

                            SliderGroup {
                                title: "Texture"
                                from: -100.0
                                to: 100.0
                                value: root.textureValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.textureValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Clarity (Midtone Contrast)"
                                from: -100.0
                                to: 100.0
                                value: root.clarityValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.clarityValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Dehaze"
                                from: -100.0
                                to: 100.0
                                value: root.dehazeValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.dehazeValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Vibrance"
                                from: -100.0
                                to: 100.0
                                value: root.vibranceValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.vibranceValue = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Saturation"
                                from: -100.0
                                to: 100.0
                                value: root.saturationValue
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.saturationValue = v; root.requestRender() }
                            }

                            // HSL Color Mixer
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "COLOR MIXER (8-BAND HSL)"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 1
                                    color: Theme.textDim
                                    Layout.fillWidth: true
                                }

                                Rectangle {
                                    implicitWidth: resetHslText.implicitWidth + 10
                                    implicitHeight: 18
                                    radius: 4
                                    color: resetHslMouse.containsMouse ? Theme.bgCardHover : "transparent"
                                    border.color: Theme.border
                                    border.width: 1

                                    Text {
                                        id: resetHslText
                                        anchors.centerIn: parent
                                        text: "RESET"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.weight: Font.Bold
                                        color: Theme.textDim
                                    }

                                    MouseArea {
                                        id: resetHslMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: {
                                            root.hslH = [0, 0, 0, 0, 0, 0, 0, 0];
                                            root.hslS = [0, 0, 0, 0, 0, 0, 0, 0];
                                            root.hslL = [0, 0, 0, 0, 0, 0, 0, 0];
                                            hslMixer.resetAll();
                                            root.requestRender();
                                        }
                                    }
                                }
                            }

                            HslMixer {
                                id: hslMixer
                                Layout.fillWidth: true
                                hslHue: root.hslH
                                hslSat: root.hslS
                                hslLum: root.hslL
                                onHslValuesChanged: function(h, s, l) {
                                    root.hslH = h;
                                    root.hslS = s;
                                    root.hslL = l;
                                }
                                onColorChanged: root.requestRender()
                            }

                            // DaVinci Resolve 3-Way Color Wheels
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            ColorGradingWheels {
                                id: colorWheels
                                Layout.fillWidth: true
                                liftRgb: root.liftRgb
                                liftLuma: root.liftLuma
                                gammaRgb: root.gammaRgb
                                gammaLuma: root.gammaLuma
                                gainRgb: root.gainRgb
                                gainLuma: root.gainLuma
                                offsetRgb: root.offsetRgb
                                offsetLuma: root.offsetLuma
                                onGradingChanged: {
                                    root.liftRgb = colorWheels.liftRgb;
                                    root.liftLuma = colorWheels.liftLuma;
                                    root.gammaRgb = colorWheels.gammaRgb;
                                    root.gammaLuma = colorWheels.gammaLuma;
                                    root.gainRgb = colorWheels.gainRgb;
                                    root.gainLuma = colorWheels.gainLuma;
                                    root.offsetRgb = colorWheels.offsetRgb;
                                    root.offsetLuma = colorWheels.offsetLuma;
                                    root.requestRender();
                                }
                            }

                            // Detail & Optics
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "DETAIL & OPTICS"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 1
                                    color: Theme.textDim
                                    Layout.fillWidth: true
                                }

                                Rectangle {
                                    implicitWidth: resetDetailText.implicitWidth + 10
                                    implicitHeight: 18
                                    radius: 4
                                    color: resetDetailMouse.containsMouse ? Theme.bgCardHover : "transparent"
                                    border.color: Theme.border
                                    border.width: 1

                                    Text {
                                        id: resetDetailText
                                        anchors.centerIn: parent
                                        text: "RESET"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 8
                                        font.weight: Font.Bold
                                        color: Theme.textDim
                                    }

                                    MouseArea {
                                        id: resetDetailMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: {
                                            root.sharpnessVal = 25.0;
                                            root.denoiseLumVal = 0.0;
                                            root.denoiseColVal = 10.0;
                                            root.vignetteVal = 0.0;
                                            root.defringeVal = 0.0;
                                            root.lensDistortionVal = 0.0;
                                            root.requestRender();
                                        }
                                    }
                                }
                            }

                            SliderGroup {
                                title: "Sharpening"
                                from: 0.0
                                to: 100.0
                                value: root.sharpnessVal
                                defaultValue: 25.0
                                onSliderMoved: function(v) { root.sharpnessVal = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Noise Reduction (Luma)"
                                from: 0.0
                                to: 100.0
                                value: root.denoiseLumVal
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.denoiseLumVal = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Noise Reduction (Color)"
                                from: 0.0
                                to: 100.0
                                value: root.denoiseColVal
                                defaultValue: 10.0
                                accentColor: Theme.accentYellow
                                onSliderMoved: function(v) { root.denoiseColVal = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Vignette"
                                from: -100.0
                                to: 100.0
                                value: root.vignetteVal
                                defaultValue: 0.0
                                onSliderMoved: function(v) { root.vignetteVal = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Defringe (Chromatic Aberration)"
                                from: 0.0
                                to: 100.0
                                value: root.defringeVal
                                defaultValue: 0.0
                                accentColor: Theme.accentMagenta
                                onSliderMoved: function(v) { root.defringeVal = v; root.requestRender() }
                            }

                            SliderGroup {
                                title: "Lens Distortion Correction"
                                from: -50.0
                                to: 50.0
                                value: root.lensDistortionVal
                                defaultValue: 0.0
                                accentColor: Theme.accentCyan
                                onSliderMoved: function(v) { root.lensDistortionVal = v; root.requestRender() }
                            }
                        }

                        // Persistent Lightroom Workflow Action Bar (Copy / Paste / Reset)
                        Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 6

                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: 28
                                radius: Theme.radiusSm
                                color: copyBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                                border.color: Theme.border

                                RowLayout {
                                    anchors.centerIn: parent
                                    spacing: 4
                                    Text {
                                        text: "Copy"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 10
                                        font.weight: Font.Medium
                                        color: Theme.textMain
                                    }
                                }
                                MouseArea {
                                    id: copyBtnMouse
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    hoverEnabled: true
                                    onClicked: root.copyRecipe()
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: 28
                                radius: Theme.radiusSm
                                color: pasteBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                                border.color: Theme.border

                                RowLayout {
                                    anchors.centerIn: parent
                                    spacing: 4
                                    Text {
                                        text: "Paste"
                                        textFormat: Text.PlainText
                                        font.pixelSize: 10
                                        font.weight: Font.Medium
                                        color: Theme.textMain
                                    }
                                }
                                MouseArea {
                                    id: pasteBtnMouse
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    hoverEnabled: true
                                    onClicked: root.pasteRecipe()
                                }
                            }

                            Rectangle {
                                implicitWidth: 70
                                implicitHeight: 28
                                radius: Theme.radiusSm
                                color: resetBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                                border.color: Theme.border

                                Text {
                                    anchors.centerIn: parent
                                    text: "Reset"
                                    textFormat: Text.PlainText
                                    font.pixelSize: 10
                                    font.weight: Font.Bold
                                    color: Theme.highlightClip
                                }
                                MouseArea {
                                    id: resetBtnMouse
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    hoverEnabled: true
                                    onClicked: root.resetRecipe()
                                }
                            }
                        }
                    }
                }
            }
        }

        // BOTTOM FILMSTRIP
        Filmstrip {
            id: filmstrip
            Layout.fillWidth: true
            photoList: root.photoList
            activePhotoPath: root.activePhotoPath
            isDownloadingRemote: root.isDownloadingRemote
            onSelectPhoto: function(path, isRemote) {
                if (isRemote) {
                    root.isDownloadingRemote = true;
                    var fName = path.split("/").pop();
                    root.showToast("[Cloud] Fetching cloud RAW: " + fName + "...", Theme.accentCyan);
                    fetchGdriveProc.command = [root.resolveEnginePath(), "gdrive", "fetch", path];
                    fetchGdriveProc.running = true;
                } else {
                    root.loadPhoto(path);
                }
            }
            onNavigateFolder: function(remotePath) {
                root.currentGdrivePath = remotePath;
                filmstrip.currentFolder = remotePath;
                root.showToast("[Folder] Navigating: " + remotePath, Theme.accent);
                listProc.command = [root.resolveEnginePath(), "gdrive", "list", remotePath];
                listProc.running = true;
            }
            onParentFolderRequested: {
                var parts = root.currentGdrivePath.split("/").filter(function(p) { return p.length > 0; });
                if (parts.length > 1) {
                    parts.pop();
                    root.currentGdrivePath = parts.join("/");
                } else {
                    root.currentGdrivePath = "Photos";
                }
                filmstrip.currentFolder = root.currentGdrivePath;
                root.showToast("[Folder] Navigating: " + root.currentGdrivePath, Theme.accent);
                listProc.command = [root.resolveEnginePath(), "gdrive", "list", root.currentGdrivePath];
                listProc.running = true;
            }
            onSwitchSource: function(isGdrive) {
                if (isGdrive) {
                    filmstrip.currentFolder = root.currentGdrivePath;
                    listProc.command = [root.resolveEnginePath(), "gdrive", "list", root.currentGdrivePath];
                    listProc.running = true;
                } else {
                    filmstrip.currentFolder = "~/Pictures";
                    root.scanLocalFolder("~/Pictures");
                }
            }
            onRefreshRequested: {
                if (filmstrip.isGdriveMode) {
                    listProc.command = [root.resolveEnginePath(), "gdrive", "list", root.currentGdrivePath];
                    listProc.running = true;
                } else {
                    root.scanLocalFolder(filmstrip.currentFolder || "~/Pictures");
                }
            }
        }
    }

    // Toast Notification Banner (Auto-dismissing)
    Rectangle {
        visible: root.toastMessage !== ""
        anchors.top: parent.top
        anchors.topMargin: 52
        anchors.horizontalCenter: parent.horizontalCenter
        implicitWidth: toastContent.implicitWidth + 28
        implicitHeight: 32
        radius: 16
        color: Theme.bgDark
        border.color: root.toastColor
        border.width: 1
        z: 300

        RowLayout {
            id: toastContent
            anchors.centerIn: parent
            spacing: 8

            Text {
                text: root.toastMessage
                textFormat: Text.PlainText
                font.pixelSize: 11
                font.weight: Font.DemiBold
                color: root.toastColor
            }
        }
    }

    // EXPORT MODAL OVERLAY
    Rectangle {
        visible: root.showExportModal
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.65)
        z: 100

        MouseArea {
            anchors.fill: parent
            onClicked: root.showExportModal = false
        }

        ExportDialog {
            id: exportDialog
            anchors.centerIn: parent
            onCloseRequested: root.showExportModal = false
            onDoExport: function(opts) {
                exportDialog.isExporting = true;
                exportDialog.exportStatusText = "Demosaicing full resolution sensor & encoding...";
                var recipeJson = JSON.stringify(root.buildRecipeObject());
                var optsJson = JSON.stringify(opts);
                exportProc.command = [
                    root.resolveEnginePath(),
                    "export",
                    root.activePhotoPath,
                    "--recipe", recipeJson,
                    "--options", optsJson
                ];
                exportProc.running = true;
            }
        }
    }

    Component.onCompleted: {
        scanLocalFolder("~/Pictures");
        if (root.activePhotoPath !== "") {
            loadPhoto(root.activePhotoPath);
        }
    }

    Component.onDestruction: {
        if (daemonProc.running) {
            daemonProc.write(JSON.stringify({ cmd: "exit" }) + "\n");
            daemonProc.running = false;
        }
        if (exportProc.running) exportProc.running = false;
        if (listProc.running) listProc.running = false;
        if (fetchGdriveProc.running) fetchGdriveProc.running = false;
        if (openFileProc.running) openFileProc.running = false;
    }
}
