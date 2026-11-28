import QtQuick
import QtQuick.Layouts
import QtQuick.Window
import QtQuick.Controls
import QtQuick.Dialogs
import OmaStudio.Host

Rectangle {
    id: root
    width: 1360
    height: 860
    color: Theme.bgBase

    // State Variables
    property string activePhotoPath: ""
    property string currentRenderPath: "/dev/shm/omastudio_viewport.ppm"
    property var activeMetadata: null
    readonly property bool isFujifilm: activeMetadata !== null && activeMetadata !== undefined
        && /fujifilm/i.test(activeMetadata.make || "")
    property var activeHistogram: null
    property var activeScene: null
    property var photoList: []

    property bool isProMode: false
    // The edit as last written to (or read from) the photo's sidecar; autosave compares against it
    property string savedRecipeJson: ""
    property int quietSaves: 0
    // The edit before a social platform was applied; reset brings it back
    property var preSocialRecipe: null
    // The AI chat over the viewport; opens on connect, toggled from the top bar
    property bool aiChatOpen: false
    property bool isSplitView: false
    property real splitRatio: 0.5
    property bool showExportModal: false
    property bool showAboutModal: false
    property bool showCloudModal: false
    readonly property bool anyModalOpen: root.showExportModal || root.showAboutModal || root.showCloudModal
    readonly property bool textInputFocused: {
        var win = root.Window.window;
        var item = win ? win.activeFocusItem : null;
        return item !== null && item !== undefined
            && (item instanceof TextInput || item instanceof TextEdit);
    }
    property bool _shuttingDown: false
    // Monotonic daemon request IDs for discarding stale out-of-order responses across photo switches
    property int nextDaemonRequestId: 0
    property int latestLoadReqId: 0
    property int latestAdjustReqId: 0
    property int latestDetailReqId: 0
    property int latestLoupeReqId: 0
    property int latestRefReqId: 0
    property int latestAiAutoReqId: 0
    property int latestAiSocialReqId: 0
    // Cloud accounts (`omastudio-engine cloud status`), the one open in the filmstrip
    // and the folder inside it
    property var cloudAccounts: []
    property var cloudProviders: []
    property string cloudUploadFolder: "OmaStudio"
    property string cloudAccount: ""
    property string cloudPath: ""
    // Photos waiting to be downloaded ([{ remote, local }]) and what runs once they are
    property var cloudFetchQueue: []
    property var cloudAfterFetch: null
    property string cloudFetching: ""
    // Exported files waiting to be uploaded, and the account they go to
    property var cloudUploadQueue: []
    property string cloudUploadAccount: ""
    property int cloudUploaded: 0
    property bool showLeftPanel: true
    // A narrow window (tiled beside another, a scrolling-layout column) keeps the
    // photo large: the left panel steps aside and the inspector narrows
    readonly property bool leftPanelFits: root.width >= Theme.px(780)
    readonly property bool leftPanelShown: root.showLeftPanel && root.leftPanelFits
    readonly property real leftPanelWidth: Math.round(Math.min(Theme.px(280), root.width * 0.24))
    readonly property real rightPanelWidth: Math.round(Math.max(Math.min(Theme.px(320), root.width * 0.32), Math.min(Theme.px(260), root.width * 0.45)))

    property bool showRightPanel: true
    // Single source of truth from Rust (`src/raw/supported_extensions.inc` via Host)
    readonly property var rawExtensions: JSON.parse(Host.rawExtensions)
    readonly property var imageExtensions: JSON.parse(Host.imageExtensions)
    function globs(exts) {
        return exts.map(function(e) { return "*." + e + " *." + e.toUpperCase(); }).join(" ");
    }
    property bool showFilmstrip: true
    // Right panel tab: adjust, masks, looks, ai, output
    property string rightTab: "adjust"
    // Section key -> true when the section is collapsed
    property var collapsedSections: ({})
    // Photo surround: "dark", "grey" (middle grey) or "theme"
    property string surround: "dark"
    property bool uiStateLoaded: false
    property int photoRating: 0
    property string photoFlag: "none"

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
    // Point curves: [[x, y], ...] on 0..1; empty = straight line
    property var curvePoints: []
    property var curvePointsRed: []
    property var curvePointsGreen: []
    property var curvePointsBlue: []

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

    // DaVinci Resolve Primaries: Contrast Pivot, Color Boost, Midtone Detail
    property real contrastPivot: 0.435
    property real colorBoost: 0.0
    property real midtoneDetail: 0.0

    // DaVinci 3D LUT State
    property string activeLutName: ""
    property real activeLutIntensity: 1.0
    property string activeLutPath: ""

    // DaVinci Resolve Local Grade Versions (A, B, C, D)
    property string currentGradeVersion: "A"
    property var gradeVersions: ({ "A": null, "B": null, "C": null, "D": null })

    // Film Simulation & Medium Format Sensor Profiles
    property string activeFilmSimulation: "none"
    property real activeFilmSimIntensity: 1.0

    // Gamut compression and ACES tone curve (output space is chosen at export)
    property bool gamutCompression: false
    property bool activeAcesTonemap: false

    // Capture One Pro Skin Tone Uniformity Engine
    property bool skinToneEnabled: false
    property real skinTargetHue: 50.0
    property real skinHueRange: 32.0
    property real skinUniformityHue: 0.0
    property real skinUniformitySat: 0.0
    property real skinAmountHue: 0.0
    property real skinAmountSat: 0.0

    // DaVinci Resolve RGB Primary Matrix Mixer
    property bool rgbMixerEnabled: false
    property bool rgbMixerMonochrome: false
    property real mixerRedInR: 1.0
    property real mixerGreenInR: 0.0
    property real mixerBlueInR: 0.0
    property real mixerRedInG: 0.0
    property real mixerGreenInG: 1.0
    property real mixerBlueInG: 0.0
    property real mixerRedInB: 0.0
    property real mixerGreenInB: 0.0
    property real mixerBlueInB: 1.0

    // Photochemical Silver-Halide Film Grain
    property real grainAmount: 0.0
    property real grainSize: 1.0
    property real grainRoughness: 50.0

    // Fujifilm Color Chrome Effect / FX Blue
    property real colorChrome: 0.0
    property real colorChromeBlue: 0.0

    // Local Layered Adjustments & Masks
    property var adjustmentLayers: []
    // Heal and clone spots (recipe "retouch")
    property var retouchSpots: []

    // Watermark & Branding State
    property bool watermarkEnabled: false
    property string watermarkType: "text"
    property string watermarkText: "OmaStudio Photography"
    property string watermarkLogoPath: ""
    property int watermarkPositionIndex: 8
    property real watermarkOpacity: 0.85
    property int watermarkSize: 14
    property int watermarkMargin: 24
    property string watermarkColorHex: "#ffffff"
    property bool watermarkDropShadow: true

    function buildWatermarkOptions() {
        return {
            "enabled": root.watermarkEnabled,
            "watermark_type": root.watermarkType,
            "text": root.watermarkText,
            "logo_path": root.watermarkLogoPath ? root.watermarkLogoPath : null,
            "position_index": root.watermarkPositionIndex,
            "opacity": root.watermarkOpacity,
            "size": root.watermarkSize,
            "margin": root.watermarkMargin,
            "color": root.watermarkColorHex,
            "drop_shadow": root.watermarkDropShadow
        };
    }

    function switchGradeVersion(ver) {
        if (ver === root.currentGradeVersion) return;
        var cur = root.buildRecipeObject();
        var vMap = Object.assign({}, root.gradeVersions);
        vMap[root.currentGradeVersion] = cur;

        root.currentGradeVersion = ver;
        if (vMap[ver]) {
            root.gradeVersions = vMap;
            // Grade versions hold the edit, not the photo's culling state
            root.applyRecipeObject(Object.assign({}, vMap[ver], { "rating": root.photoRating, "flag": root.photoFlag }));
            root.showToast(qsTr("Version %1").arg(ver), Theme.accentCyan);
        } else {
            vMap[ver] = cur;
            root.gradeVersions = vMap;
            root.showToast(qsTr("Version %1 started").arg(ver), Theme.accentYellow);
        }
        root.requestRender();
    }

    function copyCurrentToVersion(ver) {
        var cur = root.buildRecipeObject();
        var vMap = Object.assign({}, root.gradeVersions);
        vMap[ver] = cur;
        root.gradeVersions = vMap;
        root.showToast(qsTr("Copied to version %1").arg(ver), Theme.accentGreen);
    }

    // Optics & Crop State
    property real defringeVal: 0.0
    property real lensDistortionVal: 0.0
    property real perspectiveVertical: 0.0
    property real perspectiveHorizontal: 0.0
    property real perspectiveScale: 100.0
    property real cropX: 0.0
    property real cropY: 0.0
    property real cropW: 1.0
    property real cropH: 1.0
    property string cropAspect: "Original"

    // Clipboard & Toast Notification State
    property var copiedRecipe: null
    property string toastMessage: ""
    property color toastColor: Theme.accent
    property string currentCatalogFilter: ""
    property bool isListingFolder: false
    property bool daemonReady: false
    // One preview render at a time: slider moves arriving while the engine renders
    // collapse into a single follow-up render with the latest values
    property bool renderInFlight: false
    property bool renderPending: false
    // Adjust commands sent and not yet answered. The daemon answers in order, so a
    // render is finished only when the answer to the last one arrives: a late answer
    // to an earlier one no longer starts another render.
    property int adjustOutstanding: 0
    // How long the watchdog has waited on a running engine (first decode of a 100 MP file)
    property int renderWaitedMs: 0
    // The engine stopped (crash, out of memory) with a photo open: its replacement loads
    // the photo again before anything else, keeping the edits made in the window
    property bool engineReloading: false
    property var daemonQueue: []

    function resolveEnginePath() {
        // OMASTUDIO_ENGINE, the engine installed next to the application, or ~/.local/bin
        return Host.enginePath;
    }

    function showToast(msg, col) {
        // Older call sites tag messages ("[OK] ...", "[Catalog] ..."); the colour carries that
        root.toastMessage = String(msg).replace(/^\[[^\]]*\]\s*/, "");
        root.toastColor = col || Theme.accent;
        toastTimer.restart();
    }

    function ratingMessage(stars) {
        return stars > 0 ? qsTr("Rating: %n star(s)", "", stars) : qsTr("Rating cleared");
    }

    function flagMessage(flag) {
        if (flag === "pick") return qsTr("Flag: pick (P)");
        if (flag === "reject") return qsTr("Flag: reject (X)");
        return qsTr("Flag: none (U)");
    }

    Timer {
        id: toastTimer
        interval: 2800
        repeat: false
        onTriggered: root.toastMessage = ""
    }

    // Rust-backed Undo / Redo History Controller (100-entry bounded stack)
    HistoryController {
        id: historyCtrl
        onRevisionChanged: Qt.callLater(root.refreshHistory)
    }
    readonly property alias historyCtrl: historyCtrl
    property bool isPerformingUndoRedo: false
    // Engine-only tone flags with no UI control yet; kept so UI saves don't drop them
    property bool highlightReconstruct: true
    property bool filmicAgx: false

    function pushUndoState() {
        if (root.isPerformingUndoRedo) return;
        undoCommitTimer.stop();
        historyCtrl.pushState(JSON.stringify(root.buildRecipeObject()));
    }

    // Every render request (re)starts undoCommitTimer; once edits settle, the
    // pre-edit baseline is pushed as one undo step (a whole drag = one step).
    function commitPendingUndo() {
        undoCommitTimer.stop();
        if (root.isPerformingUndoRedo || root.activePhotoPath === "") return;
        historyCtrl.commitPending(JSON.stringify(root.buildRecipeObject()));
    }

    Timer {
        id: undoCommitTimer
        interval: 600
        repeat: false
        onTriggered: root.commitPendingUndo()
    }

    function undo() {
        undoCommitTimer.stop();
        var prevJson = historyCtrl.undo(JSON.stringify(root.buildRecipeObject()));
        if (prevJson === "") {
            root.showToast(qsTr("Nothing to undo"), Theme.textDim);
            return;
        }
        root.isPerformingUndoRedo = true;
        // Culling (stars / flag) is not part of the edit history
        var prev = Object.assign({}, JSON.parse(prevJson), { "rating": root.photoRating, "flag": root.photoFlag });
        root.applyRecipeObject(prev);
        root.isPerformingUndoRedo = false;
        root.showToast(qsTr("Undo"), Theme.accentCyan);
    }

    function redo() {
        undoCommitTimer.stop();
        var nextJson = historyCtrl.redo(JSON.stringify(root.buildRecipeObject()));
        if (nextJson === "") {
            root.showToast(qsTr("Nothing to redo"), Theme.textDim);
            return;
        }
        root.isPerformingUndoRedo = true;
        // Culling (stars / flag) is not part of the edit history
        var next = Object.assign({}, JSON.parse(nextJson), { "rating": root.photoRating, "flag": root.photoFlag });
        root.applyRecipeObject(next);
        root.isPerformingUndoRedo = false;
        root.showToast(qsTr("Redo"), Theme.accentCyan);
    }

    // History panel: the states in order (undo stack, the current edit, then redo
    // states), each named for what changed from the one before
    property var historyModel: []

    // Sections the recipe keys belong to, by key prefix; single sliders are named on their own
    function historyKeyLabel(key) {
        var own = {
            "exposure": qsTr("Exposure"), "contrast": qsTr("Contrast"), "highlights": qsTr("Highlights"),
            "shadows": qsTr("Shadows"), "whites": qsTr("Whites"), "blacks": qsTr("Blacks"),
            "texture": qsTr("Texture"), "clarity": qsTr("Clarity (Midtone Contrast)"), "dehaze": qsTr("Dehaze"),
            "vibrance": qsTr("Vibrance"), "saturation": qsTr("Saturation"), "sharpness": qsTr("Sharpening"),
            "vignette": qsTr("Vignette"), "gamut_compression": qsTr("Gamut compression"),
            "aces_tonemap": qsTr("ACES tone curve"), "layers": qsTr("Local adjustments"),
            "retouch": qsTr("Heal and clone"), "preset_name": qsTr("Presets")
        };
        if (own[key] !== undefined) return own[key];
        var prefixes = [
            ["wb_", qsTr("White balance")], ["curve_", qsTr("Tone curve")], ["hsl_", qsTr("Colour mixer")],
            ["rgb_mixer", qsTr("Colour mixer")], ["mixer_", qsTr("Colour mixer")], ["skin_", qsTr("Skin tone")],
            ["lut_", qsTr("LUT")], ["grain_", qsTr("Grain")], ["perspective_", qsTr("Transform")],
            ["crop_", qsTr("Crop")], ["rotation", qsTr("Crop")], ["flip_", qsTr("Crop")],
            ["film_sim", qsTr("Profile")], ["color_chrome", qsTr("Profile")],
            ["denoise_", qsTr("Detail and optics")], ["defringe", qsTr("Detail and optics")],
            ["lens_", qsTr("Detail and optics")],
            ["lift", qsTr("Colour grading")], ["gamma", qsTr("Colour grading")], ["gain", qsTr("Colour grading")],
            ["offset", qsTr("Colour grading")], ["contrast_pivot", qsTr("Colour grading")],
            ["color_boost", qsTr("Colour grading")], ["midtone_detail", qsTr("Colour grading")]
        ];
        for (var i = 0; i < prefixes.length; ++i)
            if (key.indexOf(prefixes[i][0]) === 0) return prefixes[i][1];
        return "";
    }

    // Localizes a structured diff descriptor produced by Rust HistoryController
    function formatHistoryDescriptor(d) {
        if (!d || d.kind === "start") return qsTr("Start");
        var keys = d.keys || [];
        var names = [];
        for (var i = 0; i < keys.length; ++i) {
            var name = root.historyKeyLabel(keys[i]);
            if (name === "") name = qsTr("Edit");
            if (names.indexOf(name) < 0) names.push(name);
        }
        if (names.length === 0) return qsTr("Edit");
        if (names.length === 1 && d.single_key && typeof d.single_value === "number"
                && root.historyKeyLabel(d.single_key) === names[0]) {
            var v = d.single_value;
            var text = d.single_key === "exposure" ? v.toFixed(2) : String(Math.round(v));
            return names[0] + " " + (v > 0 ? "+" : "") + text;
        }
        return names.length > 2 ? names.slice(0, 2).join(", ") + ", …" : names.join(", ");
    }

    function refreshHistory() {
        if (root.activePhotoPath === "") {
            root.historyModel = [];
            return;
        }
        var descriptors = JSON.parse(historyCtrl.diffDescriptors(JSON.stringify(root.buildRecipeObject())));
        var model = [];
        for (var i = 0; i < descriptors.length; ++i)
            model.push({ "label": root.formatHistoryDescriptor(descriptors[i]) });
        root.historyModel = model;
    }

    // Go back or forward to a state in the history; later states stay until the next edit
    function jumpToHistory(index) {
        undoCommitTimer.stop();
        var targetJson = historyCtrl.jumpTo(index, JSON.stringify(root.buildRecipeObject()));
        if (targetJson === "") return;
        root.isPerformingUndoRedo = true;
        var target = Object.assign({}, JSON.parse(targetJson), { "rating": root.photoRating, "flag": root.photoFlag });
        root.applyRecipeObject(target);
        root.isPerformingUndoRedo = false;
    }

    // Snapshots: named states of the edit, kept in the sidecar
    property var snapshots: []

    // Soft proofing: a view setting sent with every render, never part of the edit
    property bool softProof: false
    property string proofProfile: "sRGB"
    property string proofIntent: "relative_colorimetric"
    property bool gamutWarning: false
    property var iccProfiles: []
    property string lastProofError: ""

    function proofRequest() {
        if (!root.softProof) return null;
        return { "profile": root.proofProfile, "intent": root.proofIntent, "gamut_warning": root.gamutWarning };
    }

    function proofProfileName() {
        for (var i = 0; i < root.iccProfiles.length; ++i)
            if (root.iccProfiles[i].id === root.proofProfile) return root.iccProfiles[i].name;
        return root.proofProfile;
    }

    function reportProofError(error) {
        if (!error) {
            root.lastProofError = "";
            return;
        }
        // Once per problem, not on every render
        if (error === root.lastProofError) return;
        root.lastProofError = error;
        root.showToast(qsTr("Soft proof not applied: %1").arg(error), Theme.highlightClip);
    }

    onSoftProofChanged: { root.requestRender(); root.requestReference(); root.saveUiStateSoon(); }
    onProofProfileChanged: { if (root.softProof) { root.requestRender(); root.requestReference(); } root.saveUiStateSoon(); }
    onProofIntentChanged: { if (root.softProof) { root.requestRender(); root.requestReference(); } root.saveUiStateSoon(); }
    onGamutWarningChanged: { if (root.softProof) { root.requestRender(); root.requestReference(); } root.saveUiStateSoon(); }

    // Focus peaking: a view setting like soft proofing
    property bool focusPeaking: false
    onFocusPeakingChanged: root.requestRender()

    // Compare view: the current edit beside the photo as first opened ("before"),
    // a grade version ("ver:A") or a snapshot ("snap:0")
    property bool compareMode: false
    property string compareRef: "before"
    property string referenceImage: ""
    property int referenceRevision: 0

    readonly property var compareChoices: {
        var list = [{ "id": "before", "label": qsTr("Before") }];
        var versions = ["A", "B", "C", "D"];
        for (var i = 0; i < versions.length; ++i) {
            var v = versions[i];
            if (v !== root.currentGradeVersion && root.gradeVersions[v])
                list.push({ "id": "ver:" + v, "label": qsTr("Version %1").arg(v) });
        }
        for (var k = 0; k < root.snapshots.length; ++k)
            list.push({ "id": "snap:" + k, "label": root.snapshots[k].name });
        return list;
    }

    function compareRecipe(ref) {
        if (ref.indexOf("ver:") === 0) return root.gradeVersions[ref.slice(4)] || null;
        if (ref.indexOf("snap:") === 0) {
            var snap = root.snapshots[Number(ref.slice(5))];
            return snap ? snap.recipe : null;
        }
        // Before: the engine's defaults, i.e. no edits
        return {};
    }

    function requestReference() {
        if (!root.compareMode || root.activePhotoPath === "") return;
        var recipe = root.compareRecipe(root.compareRef);
        if (recipe === null) {
            root.compareRef = "before";
            return;   // the change of compareRef asks again
        }
        root.sendDaemonCommand({ cmd: "reference", recipe: recipe, proof: root.proofRequest() });
    }

    // Vulkan RHI DMA-BUF preview fallback state (latches to memfd if import ever fails)
    property bool dmaBufDisabled: false
    function colorSpaceCode(name) {
        if (name === "display_p3") return 1;
        if (name === "bt2020") return 2;
        if (name === "bt2100_pq") return 3;
        if (name === "bt2100_hlg") return 4;
        return 0;
    }
    function onDmaBufFailed() {
        if (root.dmaBufDisabled) return;
        root.dmaBufDisabled = true;
        if (root.activePhotoPath !== "") root.requestRender();
    }

    onCompareModeChanged: {
        if (root.compareMode) {
            root.requestReference();
            if (root.viewport && root.viewport.imageSource.indexOf("dmabuf://") === 0)
                root.requestRender();
        } else {
            root.referenceImage = "";
        }
    }
    onCompareRefChanged: root.requestReference()
    onSnapshotsChanged: if (root.compareRef.indexOf("snap:") === 0) root.requestReference()
    onGradeVersionsChanged: if (root.compareRef.indexOf("ver:") === 0) root.requestReference()

    function takeSnapshot() {
        root.commitPendingUndo();
        var r = root.buildRecipeObject();
        delete r.rating;
        delete r.flag;
        var list = root.snapshots.slice();
        list.push({ "name": qsTr("Snapshot %1").arg(list.length + 1), "created": new Date().toISOString(), "recipe": r });
        root.snapshots = list;
        root.saveSidecar(true);
        root.showToast(qsTr("Snapshot saved"), Theme.accentGreen);
    }

    function restoreSnapshot(index) {
        var snap = root.snapshots[index];
        if (!snap || !snap.recipe) return;
        root.pushUndoState();
        root.applyRecipeObject(Object.assign({}, snap.recipe, { "rating": root.photoRating, "flag": root.photoFlag }));
        root.showToast(qsTr("Snapshot restored"), Theme.accentCyan);
    }

    function renameSnapshot(index, name) {
        var trimmed = String(name).trim();
        if (index < 0 || index >= root.snapshots.length || trimmed === "" || trimmed === root.snapshots[index].name) return;
        var list = JSON.parse(JSON.stringify(root.snapshots));
        list[index].name = trimmed.slice(0, 80);
        root.snapshots = list;
        root.saveSidecar(true);
    }

    function deleteSnapshot(index) {
        if (index < 0 || index >= root.snapshots.length) return;
        var list = root.snapshots.slice();
        list.splice(index, 1);
        root.snapshots = list;
        root.saveSidecar(true);
    }

    function copyRecipe() {
        root.copiedRecipe = root.buildRecipeObject();
        root.showToast(qsTr("Settings copied"), Theme.accentGreen);
    }

    function pasteRecipe() {
        if (!root.copiedRecipe) {
            root.showToast(qsTr("Nothing copied yet"), Theme.accentMagenta);
            return;
        }
        root.pushUndoState();
        root.applyRecipeObject(root.copiedRecipe);
        root.showToast(qsTr("Settings pasted"), Theme.accentGreen);
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
        root.curvePoints = [];
        root.curvePointsRed = [];
        root.curvePointsGreen = [];
        root.curvePointsBlue = [];
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
        root.contrastPivot = 0.435;
        root.colorBoost = 0.0;
        root.midtoneDetail = 0.0;
        root.activeLutName = "";
        root.activeLutIntensity = 1.0;
        root.activeLutPath = "";
        root.activeFilmSimulation = "none";
        root.activeFilmSimIntensity = 1.0;
        root.gamutCompression = false;
        root.activeAcesTonemap = false;
        root.skinToneEnabled = false;
        root.skinTargetHue = 50.0;
        root.skinHueRange = 32.0;
        root.skinUniformityHue = 0.0;
        root.skinUniformitySat = 0.0;
        root.skinAmountHue = 0.0;
        root.skinAmountSat = 0.0;
        root.rgbMixerEnabled = false;
        root.rgbMixerMonochrome = false;
        root.mixerRedInR = 1.0;
        root.mixerGreenInR = 0.0;
        root.mixerBlueInR = 0.0;
        root.mixerRedInG = 0.0;
        root.mixerGreenInG = 1.0;
        root.mixerBlueInG = 0.0;
        root.mixerRedInB = 0.0;
        root.mixerGreenInB = 0.0;
        root.mixerBlueInB = 1.0;
        root.grainAmount = 0.0;
        root.grainSize = 1.0;
        root.grainRoughness = 50.0;
        root.colorChrome = 0.0;
        root.colorChromeBlue = 0.0;
        root.adjustmentLayers = [];
        root.retouchSpots = [];
        root.defringeVal = 0.0;
        root.lensDistortionVal = 0.0;
        root.perspectiveVertical = 0.0;
        root.perspectiveHorizontal = 0.0;
        root.perspectiveScale = 100.0;
        root.cropX = 0.0;
        root.cropY = 0.0;
        root.cropW = 1.0;
        root.cropH = 1.0;
        root.cropAspect = "Original";
        root.viewport.resetCrop();
        root.viewport.rotationAngle = 0.0;
        root.viewport.flipH = false;
        root.viewport.flipV = false;
        if (root.presetSel) root.presetSel.activePreset = "";
        root.exportDialog.socialOptimizer.activePlatform = "";
        root.preSocialRecipe = null;
        root.highlightReconstruct = true;
        root.filmicAgx = false;
        historyCtrl.setBaseline(JSON.stringify(root.buildRecipeObject()));
        root.showToast(qsTr("Reset to defaults"), Theme.textDim);
        root.requestRender();
    }

    // Undoes the social crop and tuning: the values from before the first platform was
    // applied come back (not the factory defaults), and the export size returns to full
    function resetSocialOptimization() {
        var pre = root.preSocialRecipe;
        root.pushUndoState();
        if (pre) {
            var r = root.buildRecipeObject();
            var keys = ["crop_x", "crop_y", "crop_w", "crop_h", "crop_aspect", "sharpness", "clarity", "vibrance", "shadows", "whites"];
            for (var i = 0; i < keys.length; i++) r[keys[i]] = pre[keys[i]];
            root.applyRecipeObject(r);
        }
        root.preSocialRecipe = null;
        root.exportDialog.maxDimension = null;
        root.exportDialog.scalePercent = 100;
        root.exportDialog.socialOptimizer.activePlatform = "";
        root.showToast(qsTr("Platform crop reset"), Theme.textDim);
        root.requestRender();
    }

    function buildRecipeObject() {
        return {
            "wb_temperature": root.wbTemp,
            "wb_tint": root.wbTint,
            "exposure": root.expValue,
            "contrast": root.contrastValue,
            "highlight_reconstruct": root.highlightReconstruct,
            "filmic_agx": root.filmicAgx,
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
            "curve_points": root.curvePoints,
            "curve_points_red": root.curvePointsRed,
            "curve_points_green": root.curvePointsGreen,
            "curve_points_blue": root.curvePointsBlue,
            "hsl_hue": root.hslH,
            "hsl_sat": root.hslS,
            "hsl_lum": root.hslL,
            "sharpness": root.sharpnessVal,
            "denoise_lum": root.denoiseLumVal,
            "denoise_col": root.denoiseColVal,
            "vignette": root.vignetteVal,
            "rotation": root.viewport.rotationAngle,
            "flip_h": root.viewport.flipH,
            "flip_v": root.viewport.flipV,
            "lift": root.liftRgb,
            "lift_luma": root.liftLuma,
            "gamma": root.gammaRgb,
            "gamma_luma": root.gammaLuma,
            "gain": root.gainRgb,
            "gain_luma": root.gainLuma,
            "offset": root.offsetRgb,
            "offset_luma": root.offsetLuma,
            "contrast_pivot": root.contrastPivot,
            "color_boost": root.colorBoost,
            "midtone_detail": root.midtoneDetail,
            "lut_name": root.activeLutName !== "" ? root.activeLutName : null,
            "lut_intensity": root.activeLutIntensity,
            "lut_path": root.activeLutPath !== "" ? root.activeLutPath : null,
            "gamut_compression": root.gamutCompression,
            "aces_tonemap": root.activeAcesTonemap,
            "film_simulation": root.activeFilmSimulation,
            "film_sim_intensity": root.activeFilmSimIntensity,
            "skin_tone_enabled": root.skinToneEnabled,
            "skin_target_hue": root.skinTargetHue,
            "skin_hue_range": root.skinHueRange,
            "skin_uniformity_hue": root.skinUniformityHue,
            "skin_uniformity_sat": root.skinUniformitySat,
            "skin_amount_hue": root.skinAmountHue,
            "skin_amount_sat": root.skinAmountSat,
            "rgb_mixer_enabled": root.rgbMixerEnabled,
            "rgb_mixer_monochrome": root.rgbMixerMonochrome,
            "mixer_red_in_r": root.mixerRedInR,
            "mixer_green_in_r": root.mixerGreenInR,
            "mixer_blue_in_r": root.mixerBlueInR,
            "mixer_red_in_g": root.mixerRedInG,
            "mixer_green_in_g": root.mixerGreenInG,
            "mixer_blue_in_g": root.mixerBlueInG,
            "mixer_red_in_b": root.mixerRedInB,
            "mixer_green_in_b": root.mixerGreenInB,
            "mixer_blue_in_b": root.mixerBlueInB,
            "grain_amount": root.grainAmount,
            "grain_size": root.grainSize,
            "grain_roughness": root.grainRoughness,
            "color_chrome": root.colorChrome,
            "color_chrome_blue": root.colorChromeBlue,
            "layers": root.adjustmentLayers,
            "retouch": root.retouchSpots,
            "defringe": root.defringeVal,
            "lens_distortion": root.lensDistortionVal,
            "perspective_vertical": root.perspectiveVertical,
            "perspective_horizontal": root.perspectiveHorizontal,
            "perspective_scale": root.perspectiveScale,
            "crop_x": root.cropX,
            "crop_y": root.cropY,
            "crop_w": root.cropW,
            "crop_h": root.cropH,
            "crop_aspect": root.cropAspect,
            "rating": root.photoRating,
            "flag": root.photoFlag,
            "preset_name": null
        };
    }

    // Number(v) || d would turn a legitimate 0 (e.g. sharpening 0) into the default
    function num(v, d) {
        var n = Number(v);
        return (v === undefined || v === null || v === "" || isNaN(n)) ? d : n;
    }

    // Applies the recipe a Fujifilm photo was shot with (the open photo by default)
    function applyCameraRecipe(path) {
        var p = path || root.activePhotoPath;
        if (p === "") return "Error: no photo open";
        return root.fujiPanel.run("read", ["fuji-recipe", p]) ? "OK" : "Error: busy";
    }

    function fujiRecipeLibrary() {
        return root.fujiPanel.recipes.map(function(e) {
            return { "path": e.path, "label": e.recipe.label, "film_simulation": e.recipe.recipe.film_simulation_name, "device": e.recipe.device };
        });
    }

    function aiEdit(prompt) {
        if (root.activePhotoPath === "") return "Error: no photo open";
        if (root.aiAssist.busy) return "Error: busy";
        root.aiAssist.promptText = String(prompt || "");
        root.aiAssist.runEdit();
        return "OK";
    }

    function aiMask(kind) {
        if (root.activePhotoPath === "") return "Error: no photo open";
        if (kind !== "subject" && kind !== "sky") return "Error: kind must be subject or sky";
        root.pushUndoState();
        root.layersPanel.addAiLayer(kind);
        return "OK";
    }

    function applyFujiRecipe(name) {
        var list = root.fujiPanel.recipes;
        for (var i = 0; i < list.length; i++) {
            if (list[i].path === name || list[i].recipe.label === name) {
                root.pushUndoState();
                root.patchRecipe(list[i].recipe.omastudio);
                return "OK";
            }
        }
        return "Error: no recipe named " + name;
    }

    // Backwards-compatible alias for full-recipe replacement callers
    function applyRecipeObject(r) {
        root.replaceRecipe(r);
    }

    // Replaces the entire recipe state cleanly from a full recipe object (load, undo/redo,
    // snapshots, grade versions, paste, XMP import, presets). Every omitted key resets to its
    // engine default so switching photos never leaks LUTs, grain, layers, crops, or flips.
    function replaceRecipe(r) {
        if (!r) return;
        root.wbTemp = root.num(r.wb_temperature, 5500.0);
        root.wbTint = root.num(r.wb_tint, 0.0);
        root.expValue = root.num(r.exposure, 0.0);
        root.contrastValue = root.num(r.contrast, 0.0);
        root.highlightsValue = root.num(r.highlights, 0.0);
        root.shadowsValue = root.num(r.shadows, 0.0);
        root.whitesValue = root.num(r.whites, 0.0);
        root.blacksValue = root.num(r.blacks, 0.0);
        root.textureValue = root.num(r.texture, 0.0);
        root.clarityValue = root.num(r.clarity, 0.0);
        root.dehazeValue = root.num(r.dehaze, 0.0);
        root.vibranceValue = root.num(r.vibrance, 0.0);
        root.saturationValue = root.num(r.saturation, 0.0);
        root.curveHighlights = root.num(r.curve_highlights, 0.0);
        root.curveLights = root.num(r.curve_lights, 0.0);
        root.curveDarks = root.num(r.curve_darks, 0.0);
        root.curveShadows = root.num(r.curve_shadows, 0.0);
        root.curvePoints = Array.isArray(r.curve_points) ? r.curve_points : [];
        root.curvePointsRed = Array.isArray(r.curve_points_red) ? r.curve_points_red : [];
        root.curvePointsGreen = Array.isArray(r.curve_points_green) ? r.curve_points_green : [];
        root.curvePointsBlue = Array.isArray(r.curve_points_blue) ? r.curve_points_blue : [];
        root.hslH = Array.isArray(r.hsl_hue) ? r.hsl_hue : [0, 0, 0, 0, 0, 0, 0, 0];
        root.hslS = Array.isArray(r.hsl_sat) ? r.hsl_sat : [0, 0, 0, 0, 0, 0, 0, 0];
        root.hslL = Array.isArray(r.hsl_lum) ? r.hsl_lum : [0, 0, 0, 0, 0, 0, 0, 0];
        root.sharpnessVal = root.num(r.sharpness, 25.0);
        root.denoiseLumVal = root.num(r.denoise_lum, 0.0);
        root.denoiseColVal = root.num(r.denoise_col, 10.0);
        root.vignetteVal = root.num(r.vignette, 0.0);
        root.viewport.rotationAngle = root.num(r.rotation, 0.0);
        root.viewport.flipH = Boolean(r.flip_h);
        root.viewport.flipV = Boolean(r.flip_v);

        // Color Wheels
        root.liftRgb = Array.isArray(r.lift) ? r.lift : [0.0, 0.0, 0.0];
        root.liftLuma = root.num(r.lift_luma, 0.0);
        root.gammaRgb = Array.isArray(r.gamma) ? r.gamma : [0.0, 0.0, 0.0];
        root.gammaLuma = root.num(r.gamma_luma, 0.0);
        root.gainRgb = Array.isArray(r.gain) ? r.gain : [0.0, 0.0, 0.0];
        root.gainLuma = root.num(r.gain_luma, 0.0);
        root.offsetRgb = Array.isArray(r.offset) ? r.offset : [0.0, 0.0, 0.0];
        root.offsetLuma = root.num(r.offset_luma, 0.0);

        // DaVinci Primaries & 3D LUT
        root.contrastPivot = root.num(r.contrast_pivot, 0.435);
        root.colorBoost = root.num(r.color_boost, 0.0);
        root.midtoneDetail = root.num(r.midtone_detail, 0.0);
        root.activeLutName = r.lut_name || "";
        root.activeLutIntensity = root.num(r.lut_intensity, 1.0);
        root.activeLutPath = r.lut_path || "";

        // Gamut compression, ACES tone curve & Film Simulations
        // Recipes from earlier versions stored "ACEScg" for gamut compression
        root.gamutCompression = Boolean(r.gamut_compression) || String(r.color_space || "").toLowerCase() === "acescg";
        root.activeAcesTonemap = Boolean(r.aces_tonemap);
        root.activeFilmSimulation = r.film_simulation || "none";
        root.activeFilmSimIntensity = root.num(r.film_sim_intensity, 1.0);

        // Capture One Skin Tone Uniformity
        root.skinToneEnabled = Boolean(r.skin_tone_enabled);
        root.skinTargetHue = root.num(r.skin_target_hue, 50.0);
        root.skinHueRange = root.num(r.skin_hue_range, 32.0);
        root.skinUniformityHue = root.num(r.skin_uniformity_hue, 0.0);
        root.skinUniformitySat = root.num(r.skin_uniformity_sat, 0.0);
        root.skinAmountHue = root.num(r.skin_amount_hue, 0.0);
        root.skinAmountSat = root.num(r.skin_amount_sat, 0.0);

        // DaVinci RGB Primary Mixer
        root.rgbMixerEnabled = Boolean(r.rgb_mixer_enabled);
        root.rgbMixerMonochrome = Boolean(r.rgb_mixer_monochrome);
        root.mixerRedInR = root.num(r.mixer_red_in_r, 1.0);
        root.mixerGreenInR = root.num(r.mixer_green_in_r, 0.0);
        root.mixerBlueInR = root.num(r.mixer_blue_in_r, 0.0);
        root.mixerRedInG = root.num(r.mixer_red_in_g, 0.0);
        root.mixerGreenInG = root.num(r.mixer_green_in_g, 1.0);
        root.mixerBlueInG = root.num(r.mixer_blue_in_g, 0.0);
        root.mixerRedInB = root.num(r.mixer_red_in_b, 0.0);
        root.mixerGreenInB = root.num(r.mixer_green_in_b, 0.0);
        root.mixerBlueInB = root.num(r.mixer_blue_in_b, 1.0);

        // Silver-Halide Film Grain
        root.grainAmount = root.num(r.grain_amount, 0.0);
        root.grainSize = root.num(r.grain_size, 1.0);
        root.grainRoughness = root.num(r.grain_roughness, 50.0);
        root.colorChrome = root.num(r.color_chrome, 0.0);
        root.colorChromeBlue = root.num(r.color_chrome_blue, 0.0);

        // Local Adjustment Layers & Retouch
        root.adjustmentLayers = Array.isArray(r.layers) ? r.layers : [];
        root.retouchSpots = Array.isArray(r.retouch) ? r.retouch : [];

        // Optics
        root.defringeVal = root.num(r.defringe, 0.0);
        root.lensDistortionVal = root.num(r.lens_distortion, 0.0);
        root.perspectiveVertical = root.num(r.perspective_vertical, 0.0);
        root.perspectiveHorizontal = root.num(r.perspective_horizontal, 0.0);
        root.perspectiveScale = root.num(r.perspective_scale, 100.0);

        // Crop
        root.cropX = root.num(r.crop_x, 0.0);
        root.cropY = root.num(r.crop_y, 0.0);
        root.cropW = root.num(r.crop_w, 1.0);
        root.cropH = root.num(r.crop_h, 1.0);
        root.cropAspect = r.crop_aspect || "Original";
        root.viewport.cropX = root.cropX;
        root.viewport.cropY = root.cropY;
        root.viewport.cropW = root.cropW;
        root.viewport.cropH = root.cropH;
        root.viewport.cropAspect = root.cropAspect;

        root.photoRating = root.num(r.rating, 0);
        root.photoFlag = r.flag || "none";
        root.highlightReconstruct = r.highlight_reconstruct !== undefined ? Boolean(r.highlight_reconstruct) : true;
        root.filmicAgx = Boolean(r.filmic_agx);

        // Programmatic recipe changes (load, undo, paste, AI) are not user edits
        historyCtrl.setBaseline(JSON.stringify(root.buildRecipeObject()));
        requestRender();
    }

    // Patches only keys explicitly present in a partial recipe delta (IPC applyRecipe,
    // Fujifilm looks, AI deltas) without resetting unspecified settings.
    function patchRecipe(r) {
        if (!r || typeof r !== "object") return;
        if (r.wb_temperature !== undefined) root.wbTemp = root.num(r.wb_temperature, root.wbTemp);
        if (r.wb_tint !== undefined) root.wbTint = root.num(r.wb_tint, root.wbTint);
        if (r.exposure !== undefined) root.expValue = root.num(r.exposure, root.expValue);
        if (r.contrast !== undefined) root.contrastValue = root.num(r.contrast, root.contrastValue);
        if (r.highlights !== undefined) root.highlightsValue = root.num(r.highlights, root.highlightsValue);
        if (r.shadows !== undefined) root.shadowsValue = root.num(r.shadows, root.shadowsValue);
        if (r.whites !== undefined) root.whitesValue = root.num(r.whites, root.whitesValue);
        if (r.blacks !== undefined) root.blacksValue = root.num(r.blacks, root.blacksValue);
        if (r.texture !== undefined) root.textureValue = root.num(r.texture, root.textureValue);
        if (r.clarity !== undefined) root.clarityValue = root.num(r.clarity, root.clarityValue);
        if (r.dehaze !== undefined) root.dehazeValue = root.num(r.dehaze, root.dehazeValue);
        if (r.vibrance !== undefined) root.vibranceValue = root.num(r.vibrance, root.vibranceValue);
        if (r.saturation !== undefined) root.saturationValue = root.num(r.saturation, root.saturationValue);
        if (r.curve_highlights !== undefined) root.curveHighlights = root.num(r.curve_highlights, root.curveHighlights);
        if (r.curve_lights !== undefined) root.curveLights = root.num(r.curve_lights, root.curveLights);
        if (r.curve_darks !== undefined) root.curveDarks = root.num(r.curve_darks, root.curveDarks);
        if (r.curve_shadows !== undefined) root.curveShadows = root.num(r.curve_shadows, root.curveShadows);
        if (Array.isArray(r.curve_points)) root.curvePoints = r.curve_points;
        if (Array.isArray(r.curve_points_red)) root.curvePointsRed = r.curve_points_red;
        if (Array.isArray(r.curve_points_green)) root.curvePointsGreen = r.curve_points_green;
        if (Array.isArray(r.curve_points_blue)) root.curvePointsBlue = r.curve_points_blue;
        if (Array.isArray(r.hsl_hue)) root.hslH = r.hsl_hue;
        if (Array.isArray(r.hsl_sat)) root.hslS = r.hsl_sat;
        if (Array.isArray(r.hsl_lum)) root.hslL = r.hsl_lum;
        if (r.sharpness !== undefined) root.sharpnessVal = root.num(r.sharpness, root.sharpnessVal);
        if (r.denoise_lum !== undefined) root.denoiseLumVal = root.num(r.denoise_lum, root.denoiseLumVal);
        if (r.denoise_col !== undefined) root.denoiseColVal = root.num(r.denoise_col, root.denoiseColVal);
        if (r.vignette !== undefined) root.vignetteVal = root.num(r.vignette, root.vignetteVal);
        if (r.rotation !== undefined) root.viewport.rotationAngle = root.num(r.rotation, root.viewport.rotationAngle);
        if (r.flip_h !== undefined) root.viewport.flipH = Boolean(r.flip_h);
        if (r.flip_v !== undefined) root.viewport.flipV = Boolean(r.flip_v);

        if (Array.isArray(r.lift)) root.liftRgb = r.lift;
        if (r.lift_luma !== undefined) root.liftLuma = root.num(r.lift_luma, root.liftLuma);
        if (Array.isArray(r.gamma)) root.gammaRgb = r.gamma;
        if (r.gamma_luma !== undefined) root.gammaLuma = root.num(r.gamma_luma, root.gammaLuma);
        if (Array.isArray(r.gain)) root.gainRgb = r.gain;
        if (r.gain_luma !== undefined) root.gainLuma = root.num(r.gain_luma, root.gainLuma);
        if (Array.isArray(r.offset)) root.offsetRgb = r.offset;
        if (r.offset_luma !== undefined) root.offsetLuma = root.num(r.offset_luma, root.offsetLuma);

        if (r.contrast_pivot !== undefined) root.contrastPivot = root.num(r.contrast_pivot, root.contrastPivot);
        if (r.color_boost !== undefined) root.colorBoost = root.num(r.color_boost, root.colorBoost);
        if (r.midtone_detail !== undefined) root.midtoneDetail = root.num(r.midtone_detail, root.midtoneDetail);
        if (r.lut_name !== undefined) root.activeLutName = r.lut_name || "";
        if (r.lut_intensity !== undefined) root.activeLutIntensity = root.num(r.lut_intensity, root.activeLutIntensity);
        if (r.lut_path !== undefined) root.activeLutPath = r.lut_path || "";

        if (r.gamut_compression !== undefined || r.color_space !== undefined)
            root.gamutCompression = Boolean(r.gamut_compression) || String(r.color_space || "").toLowerCase() === "acescg";
        if (r.aces_tonemap !== undefined) root.activeAcesTonemap = Boolean(r.aces_tonemap);
        if (r.film_simulation !== undefined) root.activeFilmSimulation = r.film_simulation || "none";
        if (r.film_sim_intensity !== undefined) root.activeFilmSimIntensity = root.num(r.film_sim_intensity, root.activeFilmSimIntensity);

        if (r.skin_tone_enabled !== undefined) root.skinToneEnabled = Boolean(r.skin_tone_enabled);
        if (r.skin_target_hue !== undefined) root.skinTargetHue = root.num(r.skin_target_hue, root.skinTargetHue);
        if (r.skin_hue_range !== undefined) root.skinHueRange = root.num(r.skin_hue_range, root.skinHueRange);
        if (r.skin_uniformity_hue !== undefined) root.skinUniformityHue = root.num(r.skin_uniformity_hue, root.skinUniformityHue);
        if (r.skin_uniformity_sat !== undefined) root.skinUniformitySat = root.num(r.skin_uniformity_sat, root.skinUniformitySat);
        if (r.skin_amount_hue !== undefined) root.skinAmountHue = root.num(r.skin_amount_hue, root.skinAmountHue);
        if (r.skin_amount_sat !== undefined) root.skinAmountSat = root.num(r.skin_amount_sat, root.skinAmountSat);

        if (r.rgb_mixer_enabled !== undefined) root.rgbMixerEnabled = Boolean(r.rgb_mixer_enabled);
        if (r.rgb_mixer_monochrome !== undefined) root.rgbMixerMonochrome = Boolean(r.rgb_mixer_monochrome);
        if (r.mixer_red_in_r !== undefined) root.mixerRedInR = root.num(r.mixer_red_in_r, root.mixerRedInR);
        if (r.mixer_green_in_r !== undefined) root.mixerGreenInR = root.num(r.mixer_green_in_r, root.mixerGreenInR);
        if (r.mixer_blue_in_r !== undefined) root.mixerBlueInR = root.num(r.mixer_blue_in_r, root.mixerBlueInR);
        if (r.mixer_red_in_g !== undefined) root.mixerRedInG = root.num(r.mixer_red_in_g, root.mixerRedInG);
        if (r.mixer_green_in_g !== undefined) root.mixerGreenInG = root.num(r.mixer_green_in_g, root.mixerGreenInG);
        if (r.mixer_blue_in_g !== undefined) root.mixerBlueInG = root.num(r.mixer_blue_in_g, root.mixerBlueInG);
        if (r.mixer_red_in_b !== undefined) root.mixerRedInB = root.num(r.mixer_red_in_b, root.mixerRedInB);
        if (r.mixer_green_in_b !== undefined) root.mixerGreenInB = root.num(r.mixer_green_in_b, root.mixerGreenInB);
        if (r.mixer_blue_in_b !== undefined) root.mixerBlueInB = root.num(r.mixer_blue_in_b, root.mixerBlueInB);

        if (r.grain_amount !== undefined) root.grainAmount = root.num(r.grain_amount, root.grainAmount);
        if (r.grain_size !== undefined) root.grainSize = root.num(r.grain_size, root.grainSize);
        if (r.grain_roughness !== undefined) root.grainRoughness = root.num(r.grain_roughness, root.grainRoughness);
        if (r.color_chrome !== undefined) root.colorChrome = root.num(r.color_chrome, root.colorChrome);
        if (r.color_chrome_blue !== undefined) root.colorChromeBlue = root.num(r.color_chrome_blue, root.colorChromeBlue);

        if (Array.isArray(r.layers)) root.adjustmentLayers = r.layers;
        if (Array.isArray(r.retouch)) root.retouchSpots = r.retouch;

        if (r.defringe !== undefined) root.defringeVal = root.num(r.defringe, root.defringeVal);
        if (r.lens_distortion !== undefined) root.lensDistortionVal = root.num(r.lens_distortion, root.lensDistortionVal);
        if (r.perspective_vertical !== undefined) root.perspectiveVertical = root.num(r.perspective_vertical, root.perspectiveVertical);
        if (r.perspective_horizontal !== undefined) root.perspectiveHorizontal = root.num(r.perspective_horizontal, root.perspectiveHorizontal);
        if (r.perspective_scale !== undefined) root.perspectiveScale = root.num(r.perspective_scale, root.perspectiveScale);

        if (r.crop_x !== undefined || r.crop_y !== undefined || r.crop_w !== undefined || r.crop_h !== undefined || r.crop_aspect !== undefined) {
            if (r.crop_x !== undefined) root.cropX = root.num(r.crop_x, root.cropX);
            if (r.crop_y !== undefined) root.cropY = root.num(r.crop_y, root.cropY);
            if (r.crop_w !== undefined) root.cropW = root.num(r.crop_w, root.cropW);
            if (r.crop_h !== undefined) root.cropH = root.num(r.crop_h, root.cropH);
            if (r.crop_aspect !== undefined) root.cropAspect = r.crop_aspect || "Original";
            root.viewport.cropX = root.cropX;
            root.viewport.cropY = root.cropY;
            root.viewport.cropW = root.cropW;
            root.viewport.cropH = root.cropH;
            root.viewport.cropAspect = root.cropAspect;
        }

        if (r.rating !== undefined) root.photoRating = root.num(r.rating, root.photoRating);
        if (r.flag !== undefined) root.photoFlag = r.flag || "none";
        if (r.highlight_reconstruct !== undefined) root.highlightReconstruct = Boolean(r.highlight_reconstruct);
        if (r.filmic_agx !== undefined) root.filmicAgx = Boolean(r.filmic_agx);

        historyCtrl.setBaseline(JSON.stringify(root.buildRecipeObject()));
        requestRender();
    }

    // Controlled-panel patch handlers (unidirectional data flow — panels never mutate input props)
    function applyGradingPatch(patch) {
        if (!patch) return;
        if (patch.liftRgb !== undefined) root.liftRgb = patch.liftRgb;
        if (patch.liftLuma !== undefined) root.liftLuma = patch.liftLuma;
        if (patch.gammaRgb !== undefined) root.gammaRgb = patch.gammaRgb;
        if (patch.gammaLuma !== undefined) root.gammaLuma = patch.gammaLuma;
        if (patch.gainRgb !== undefined) root.gainRgb = patch.gainRgb;
        if (patch.gainLuma !== undefined) root.gainLuma = patch.gainLuma;
        if (patch.offsetRgb !== undefined) root.offsetRgb = patch.offsetRgb;
        if (patch.offsetLuma !== undefined) root.offsetLuma = patch.offsetLuma;
        if (patch.colorBoost !== undefined) root.colorBoost = patch.colorBoost;
        if (patch.midtoneDetail !== undefined) root.midtoneDetail = patch.midtoneDetail;
        if (patch.contrastPivot !== undefined) root.contrastPivot = patch.contrastPivot;
        root.requestRender();
    }

    function applyRgbMixerPatch(patch) {
        if (!patch) return;
        if (patch.mixerEnabled !== undefined) root.rgbMixerEnabled = patch.mixerEnabled;
        if (patch.monochrome !== undefined) root.rgbMixerMonochrome = patch.monochrome;
        if (patch.redInR !== undefined) root.mixerRedInR = patch.redInR;
        if (patch.greenInR !== undefined) root.mixerGreenInR = patch.greenInR;
        if (patch.blueInR !== undefined) root.mixerBlueInR = patch.blueInR;
        if (patch.redInG !== undefined) root.mixerRedInG = patch.redInG;
        if (patch.greenInG !== undefined) root.mixerGreenInG = patch.greenInG;
        if (patch.blueInG !== undefined) root.mixerBlueInG = patch.blueInG;
        if (patch.redInB !== undefined) root.mixerRedInB = patch.redInB;
        if (patch.greenInB !== undefined) root.mixerGreenInB = patch.greenInB;
        if (patch.blueInB !== undefined) root.mixerBlueInB = patch.blueInB;
        root.requestRender();
    }

    function applySkinTonePatch(patch) {
        if (!patch) return;
        if (patch.skinToneEnabled !== undefined) root.skinToneEnabled = patch.skinToneEnabled;
        if (patch.skinTargetHue !== undefined) root.skinTargetHue = patch.skinTargetHue;
        if (patch.skinHueRange !== undefined) root.skinHueRange = patch.skinHueRange;
        if (patch.skinUniformityHue !== undefined) root.skinUniformityHue = patch.skinUniformityHue;
        if (patch.skinUniformitySat !== undefined) root.skinUniformitySat = patch.skinUniformitySat;
        if (patch.skinAmountHue !== undefined) root.skinAmountHue = patch.skinAmountHue;
        if (patch.skinAmountSat !== undefined) root.skinAmountSat = patch.skinAmountSat;
        root.requestRender();
    }

    function onCropEdited(cx, cy, cw, ch, aspect) {
        root.cropX = cx;
        root.cropY = cy;
        root.cropW = cw;
        root.cropH = ch;
        root.cropAspect = aspect;
        if (!root.isPerformingUndoRedo) undoCommitTimer.restart();
        if (root.savedRecipeJson !== "") autosaveTimer.restart();
    }

    function sendDaemonCommand(obj) {
        var reqId = ++root.nextDaemonRequestId;
        obj.request_id = reqId;
        if (obj.cmd === "load") root.latestLoadReqId = reqId;
        else if (obj.cmd === "adjust") root.latestAdjustReqId = reqId;
        else if (obj.cmd === "detail") {
            if (obj.tag === "loupe") root.latestLoupeReqId = reqId;
            else root.latestDetailReqId = reqId;
        } else if (obj.cmd === "reference") root.latestRefReqId = reqId;
        else if (obj.cmd === "ai_auto") root.latestAiAutoReqId = reqId;
        else if (obj.cmd === "ai_social") root.latestAiSocialReqId = reqId;

        if (!daemonProc.running) {
            daemonProc.running = true;
        }
        if (root.daemonReady) {
            daemonProc.write(JSON.stringify(obj) + "\n");
        } else {
            root.daemonQueue.push(obj);
        }
        return reqId;
    }

    function isStalePhotoResponse(respReqId, respPhoto, expectedReqId) {
        if (respPhoto !== "" && respPhoto !== root.activePhotoPath) return true;
        if (respReqId > 0 && expectedReqId > 0 && respReqId < expectedReqId) return true;
        return false;
    }

    function handleDaemonMessage(line) {
        var trimmed = String(line || "").trim();
        if (trimmed.length === 0) return;
        try {
            var resp = JSON.parse(trimmed);
            var act = resp.action;
            var data = resp.data;
            var respReqId = (typeof resp.request_id === "number") ? resp.request_id : 0;
            var respPhoto = (typeof resp.photo_path === "string") ? resp.photo_path : "";

            if (!resp.success) {
                console.warn("Daemon returned error for action " + act + ": " + resp.error);
                if (act === "adjust") {
                    // Always account for the outstanding render slot so the queue never stalls
                    root.adjustAnswered();
                    if (!root.isStalePhotoResponse(respReqId, respPhoto, root.latestAdjustReqId)) {
                        root.showToast((resp.error || qsTr("Preview render failed")), Theme.highlightClip);
                    }
                } else if (act === "load") {
                    if (!root.isStalePhotoResponse(respReqId, respPhoto, root.latestLoadReqId)) {
                        root.engineReloading = false;
                        root.showToast(qsTr("Failed to load RAW: %1").arg(resp.error || qsTr("Unknown error")), Theme.highlightClip);
                    }
                } else if (act === "ai_auto") {
                    if (!root.isStalePhotoResponse(respReqId, respPhoto, root.latestAiAutoReqId)) {
                        root.showToast(resp.error || qsTr("AI action failed"), Theme.highlightClip);
                    }
                } else if (act === "ai_social") {
                    if (!root.isStalePhotoResponse(respReqId, respPhoto, root.latestAiSocialReqId)) {
                        root.showToast(resp.error || qsTr("AI action failed"), Theme.highlightClip);
                    }
                } else if (act === "batch_export") {
                    // Without this the dialog stays stuck on "Exporting Batch..."
                    root.exportDialog.isExporting = false;
                    root.exportDialog.exportStatusIsError = true;
                    root.exportDialog.exportStatusText = resp.error || qsTr("Batch export failed");
                } else if (act === "detail") {
                    var isLatestDetail = (respReqId === 0 || respReqId === root.latestDetailReqId || respReqId === root.latestLoupeReqId);
                    if (isLatestDetail && (respPhoto === "" || respPhoto === root.activePhotoPath)) {
                        root.viewport.detailFailed();
                    }
                }
                return;
            }

            if (act === "load" && root.engineReloading) {
                if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestLoadReqId)) return;
                // Reloaded after an engine restart: the window's recipe stays as it is
                root.engineReloading = false;
                renderDebounce.restart();
            } else if (act === "load") {
                if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestLoadReqId)) return;
                if (data) {
                    root.activeMetadata = data.metadata;
                    root.activeScene = data.scene;
                    root.replaceRecipe(data.recipe);
                    root.snapshots = data.recipe && Array.isArray(data.recipe.snapshots) ? data.recipe.snapshots : [];
                    root.savedRecipeJson = JSON.stringify(root.buildRecipeObject());
                    root.refreshHistory();
                    if (root.compareRef !== "before") root.compareRef = "before";
                    else root.requestReference();
                }
            } else if (act === "adjust") {
                root.adjustAnswered();
                if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestAdjustReqId)) return;
                if (data) {
                    root.activeHistogram = data.histogram;
                    if (data.transport === "dma-buf" && !root.dmaBufDisabled && root.viewport.rhiAvailable) {
                        root.viewport.showDmaBufPreview(
                            data.image,
                            Number(data.generation || 0),
                            Number(data.width || 0),
                            Number(data.height || 0),
                            root.colorSpaceCode(data.color_space)
                        );
                        root.navigator.imageSource = data.image;
                    } else {
                        root.viewport.imageSource = data.image;
                        root.navigator.imageSource = data.image;
                    }
                    root.reportProofError(data.proof_error);
                }
            } else if (act === "full_ready") {
                var readyPath = respPhoto !== "" ? respPhoto : (data && data.path ? data.path : "");
                if (data && readyPath === root.activePhotoPath) {
                    root.viewport.fullWidth = data.width;
                    root.viewport.fullHeight = data.height;
                }
            } else if (act === "detail") {
                if (data && data.tag === "loupe") {
                    if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestLoupeReqId)) return;
                    root.viewport.showLoupe(data.image, data.region);
                } else if (data) {
                    if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestDetailReqId)) return;
                    root.viewport.showDetail(data.image, data.region);
                }
            } else if (act === "reference") {
                if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestRefReqId)) return;
                if (data && data.image) {
                    var refImg = String(data.image);
                    root.referenceImage = (refImg.indexOf("image://") === 0 || refImg.indexOf("file://") === 0)
                        ? refImg
                        : ("file://" + refImg + "?v=" + (++root.referenceRevision));
                }
            } else if (act === "icc_profiles") {
                if (Array.isArray(data)) root.iccProfiles = data;
            } else if (act === "ai_auto") {
                if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestAiAutoReqId)) return;
                if (data) {
                    root.replaceRecipe(data.recipe);
                    root.activeScene = data.scene;
                    root.showToast(qsTr("Auto applied"), Theme.accentCyan);
                }
            } else if (act === "ai_social") {
                if (root.isStalePhotoResponse(respReqId, respPhoto, root.latestAiSocialReqId)) return;
                if (data) {
                    if (data.recipe) {
                        root.replaceRecipe(data.recipe);
                    }
                    // The platform's size and quality go to the export settings
                    if (data.max_dimension) {
                        root.exportDialog.scalePercent = 100;
                        root.exportDialog.maxDimension = data.max_dimension;
                    }
                    if (data.quality) root.exportDialog.quality = data.quality;
                    root.showToast(qsTr("%1 applied: long edge %2 px, quality %3").arg(data.platform).arg(data.max_dimension).arg(data.quality), Theme.accentCyan);
                }
            } else if (act === "save_recipe") {
                if (root.quietSaves > 0) root.quietSaves--;
                else root.showToast(qsTr("Settings saved"), Theme.accentGreen);
            } else if (act === "batch_export") {
                root.exportDialog.isExporting = false;
                if (data) {
                    root.exportDialog.exportStatusIsError = data.failed > 0;
                    root.exportDialog.exportStatusText = qsTr("%1 of %2 photos exported").arg(data.succeeded).arg(data.total);
                    var exported = [];
                    for (var ei = 0; ei < (data.results || []).length; ei++)
                        if (data.results[ei].exported_path) exported.push(data.results[ei].exported_path);
                    root.uploadExported(exported);
                    root.showToast(qsTr("Batch export: %1 succeeded, %2 failed").arg(data.succeeded).arg(data.failed),
                                   data.failed > 0 ? Theme.highlightClip : Theme.accentGreen);
                    root.notifyDesktop(qsTr("Export finished"),
                                       qsTr("Batch export: %1 succeeded, %2 failed").arg(data.succeeded).arg(data.failed),
                                       exported.length > 0 ? exported[0] : "");
                }
            }
        } catch(e) {
            console.error("Error parsing daemon message:", e, line);
        }
    }

    // XMP sidecars for Lightroom, Capture One and darktable
    function importXmp() {
        if (root.activePhotoPath === "" || xmpProc.running) return "Error: no photo open or busy";
        xmpProc.action = "import";
        xmpProc.targetPhotoPath = root.activePhotoPath;
        xmpProc.command = [root.resolveEnginePath(), "xmp", "import", root.activePhotoPath];
        xmpProc.running = true;
        return "OK";
    }

    function writeXmp(force) {
        if (root.activePhotoPath === "" || xmpProc.running) return "Error: no photo open or busy";
        xmpProc.action = "export";
        xmpProc.targetPhotoPath = root.activePhotoPath;
        xmpProc.recipeJson = JSON.stringify(root.buildRecipeObject());
        var cmd = [root.resolveEnginePath(), "xmp", "export", root.activePhotoPath, "--recipe", "-"];
        if (force) cmd.push("--force");
        xmpProc.command = cmd;
        xmpProc.running = true;
        return "OK";
    }

    Process {
        id: xmpProc
        stdinEnabled: true
        property string action: ""
        property string recipeJson: ""
        property string targetPhotoPath: ""
        onStarted: {
            if (xmpProc.action === "export") {
                xmpProc.write(xmpProc.recipeJson + "\n");
                xmpProc.recipeJson = "";
            }
        }
        onFinished: function(exitCode, output) {
            if (xmpProc.action === "import" && xmpProc.targetPhotoPath !== "" && xmpProc.targetPhotoPath !== root.activePhotoPath) {
                return;
            }
            var resp;
            try { resp = JSON.parse(String(output || "")); } catch (e) { resp = { "success": false, "error": qsTr("No answer from the engine") }; }
            if (!resp.success) {
                root.showToast(String(resp.error || qsTr("XMP failed")).split("\n")[0], Theme.accentMagenta);
            } else if (xmpProc.action === "import") {
                root.pushUndoState();
                root.replaceRecipe(resp.data.recipe);
                root.showToast(qsTr("Edit read from %1").arg(String(resp.data.source).split("/").pop()), Theme.accentGreen);
            } else {
                root.showToast(qsTr("Wrote %1").arg(String(resp.data.path).split("/").pop()), Theme.accentGreen);
            }
        }
    }

    // Writes the edit next to the photo. `quiet` skips the "Settings saved" toast, for
    // autosave and for rating/flag keys that show their own message.
    function saveSidecar(quiet) {
        if (root.activePhotoPath === "") return;
        var recipe = root.buildRecipeObject();
        root.savedRecipeJson = JSON.stringify(recipe);
        // Snapshots live in the sidecar but not in the edit (undo, copy and the engine never see them)
        if (root.snapshots.length > 0) recipe.snapshots = root.snapshots;
        if (quiet === true) root.quietSaves++;
        root.sendDaemonCommand({
            cmd: "save_recipe",
            path: root.activePhotoPath,
            recipe: recipe
        });
    }

    // Saves the edit when it differs from what the sidecar holds, so switching photos
    // or closing the window never drops work
    function autosave() {
        autosaveTimer.stop();
        if (root.activePhotoPath === "" || root.savedRecipeJson === "") return;
        if (JSON.stringify(root.buildRecipeObject()) === root.savedRecipeJson) return;
        root.saveSidecar(true);
    }

    Timer {
        id: autosaveTimer
        interval: 3000
        onTriggered: root.autosave()
    }

    function triggerAiAuto() {
        root.pushUndoState();
        root.showToast(qsTr("Auto: analysing the photo…"), Theme.accentCyan);
        root.sendDaemonCommand({ cmd: "ai_auto" });
    }

    function requestRender() {
        if (!root.isPerformingUndoRedo) undoCommitTimer.restart();
        if (root.savedRecipeJson !== "") autosaveTimer.restart();
        if (renderDebounce.running) {
            renderDebounce.restart();
        } else {
            renderDebounce.start();
        }
    }

    Connections {
        target: root.viewport
        function onMaskToolChanged() {
            if (root.viewport && root.viewport.maskTool === "pick"
                    && root.viewport.imageSource.indexOf("dmabuf://") === 0) {
                root.requestRender();
            }
        }
    }

    Timer {
        id: renderDebounce
        interval: 20
        repeat: false
        onTriggered: {
            if (root.activePhotoPath !== "") {
                if (root.renderInFlight) {
                    root.renderPending = true;
                    return;
                }
                root.renderInFlight = true;
                root.adjustOutstanding += 1;
                root.renderWaitedMs = 0;
                renderWatchdog.restart();
                var splitVal = root.isSplitView ? root.splitRatio : 0.0;
                var preferMemfd = root.dmaBufDisabled || Host.softwareRenderer
                    || !root.viewport.rhiAvailable || root.compareMode
                    || (root.viewport && root.viewport.maskTool === "pick");
                root.sendDaemonCommand({
                    cmd: "adjust",
                    recipe: root.buildRecipeObject(),
                    split: splitVal,
                    highlight_mask: root.viewport.showHighlightMask,
                    shadow_mask: root.viewport.showShadowMask,
                    proof: root.proofRequest(),
                    focus_peaking: root.focusPeaking,
                    prefer_memfd: preferMemfd
                });
            }
        }
    }

    // Recovers if a render reply never arrives (engine restarted mid-render). A running
    // engine that is only slow gets up to a minute before another render is sent, so
    // slow renders don't pile up in its queue.
    Timer {
        id: renderWatchdog
        interval: 5000
        repeat: false
        onTriggered: {
            root.renderWaitedMs += interval;
            if (daemonProc.running && root.adjustOutstanding > 0 && root.renderWaitedMs < 60000) {
                renderWatchdog.restart();
                return;
            }
            root.adjustOutstanding = 0;
            root.renderFinished();
        }
    }

    function adjustAnswered() {
        root.adjustOutstanding = Math.max(0, root.adjustOutstanding - 1);
        if (root.adjustOutstanding === 0) root.renderFinished();
    }

    function renderFinished() {
        renderWatchdog.stop();
        root.renderInFlight = false;
        if (root.renderPending) {
            root.renderPending = false;
            renderDebounce.restart();
        }
    }

    // Persistent High-Speed Rust Engine Daemon
    Process {
        id: daemonProc
        command: [root.resolveEnginePath(), "daemon"]
        running: true
        stdinEnabled: true
        splitLines: true
        previewChannel: true

        onLineRead: function(line) {
            root.handleDaemonMessage(line);
        }

        onStarted: {
            root.daemonReady = true;
            while (root.daemonQueue.length > 0) {
                var cmd = root.daemonQueue.shift();
                daemonProc.write(JSON.stringify(cmd) + "\n");
            }
        }

        onFinished: function(exitCode, output) {
            root.daemonReady = false;
            // Answers to commands sent to the old engine never come
            root.adjustOutstanding = 0;
            if (root.activePhotoPath !== "" && !root.engineReloading) {
                root.engineReloading = true;
                var reloadReqId = ++root.nextDaemonRequestId;
                root.latestLoadReqId = reloadReqId;
                // Queued, not sent: the next command starts the engine, this goes first
                root.daemonQueue.unshift({ cmd: "load", path: root.activePhotoPath, request_id: reloadReqId });
            }
            root.renderFinished();
        }
    }

    // Process: File picker
    // File chooser: the desktop's own (xdg-desktop-portal on Omarchy), opening in the
    // folder the filmstrip shows
    FileDialog {
        id: openFileProc
        title: qsTr("Open photo")
        currentFolder: Host.folderUrl(filmstrip.currentFolder)
        nameFilters: [qsTr("Photos") + " (" + root.globs(root.rawExtensions.concat(root.imageExtensions)) + ")",
                      qsTr("RAW") + " (" + root.globs(root.rawExtensions) + ")",
                      qsTr("Images") + " (" + root.globs(root.imageExtensions) + ")"]
        onAccepted: {
            var p = Host.localPath(selectedFile);
            if (p.length > 0) root.loadPhoto(p);
        }
    }

    // Process: Export
    Process {
        id: exportProc
        // The recipe goes in on stdin (`--recipe -`): with brush masks it can be larger
        // than one command line argument may be
        stdinEnabled: true
        property string recipeJson: ""
        onStarted: exportProc.write(exportProc.recipeJson + "\n")
        onFinished: function(exitCode, output) {
            root.exportDialog.isExporting = false;
            try {
                var resp = JSON.parse(output);
                if (resp.success && resp.data) {
                    root.exportDialog.exportStatusIsError = false;
                    root.exportDialog.exportStatusText = qsTr("Saved to %1").arg(resp.data.exported_path);
                    root.uploadExported([resp.data.exported_path]);
                    root.notifyDesktop(qsTr("Export finished"), String(resp.data.exported_path).split("/").pop(), resp.data.exported_path);
                } else {
                    root.exportDialog.exportStatusIsError = true;
                    root.exportDialog.exportStatusText = resp.error || qsTr("Export failed");
                }
            } catch(e) {
                root.exportDialog.exportStatusIsError = true;
                root.exportDialog.exportStatusText = qsTr("Export finished without a result");
            }
        }
    }

    // A desktop notification for work that finishes while OmaStudio is in the
    // background (another workspace, another window focused): Omarchy's own
    // notification card with a button that opens the folder, else notify-send
    function notifyDesktop(title, body, path) {
        if (Window.window && Window.window.active) return;
        var dir = String(path || "");
        dir = dir.lastIndexOf("/") > 0 ? dir.substring(0, dir.lastIndexOf("/")) : "";
        var cmd = ["omarchy-notification-send", "--app-name", "OmaStudio", "-g", "󰄄", String(title), String(body || "")];
        if (dir !== "") cmd = cmd.concat(["--action", qsTr("Open folder"), "xdg-open", dir]);
        desktopNotifyProc.fallback = ["notify-send", "--app-name=OmaStudio", "--icon=omastudio", String(title), String(body || "")];
        desktopNotifyProc.command = cmd;
        desktopNotifyProc.running = true;
    }
    Process {
        id: desktopNotifyProc
        property var fallback: []
        onFinished: function(exitCode, output) {
            // Not on Omarchy (no omarchy-notification-send): the freedesktop one
            if (exitCode !== 0 && desktopNotifyProc.fallback.length > 0) {
                var next = desktopNotifyProc.fallback;
                desktopNotifyProc.fallback = [];
                desktopNotifyProc.command = next;
                desktopNotifyProc.running = true;
            }
        }
    }

    // Load Studio Catalog
    function loadStudioCatalog(filterQuery) {
        root.isListingFolder = true;
        filmstrip.currentFolder = "Studio Catalog";
        var cmd = [root.resolveEnginePath(), "catalog", "list"];
        listProc.command = cmd;
        listProc.running = true;
    }

    // Process: Folder scan / Catalog list
    Process {
        id: listProc
        onFinished: function(exitCode, output) {
            root.isListingFolder = false;
            try {
                var resp = JSON.parse(output);
                if (resp.success && resp.data) {
                    if (resp.data.items && typeof resp.data.items === "object") {
                        // Converted catalog map into array
                        var arr = [];
                        var keys = Object.keys(resp.data.items);
                        for (var i = 0; i < keys.length; i++) {
                            var it = resp.data.items[keys[i]];
                            arr.push({
                                name: it.path.split("/").pop(),
                                path: it.path,
                                // Never the original file: only engine-encoded thumbnails reach Qt
                                thumbnail: it.thumbnail_path || "",
                                is_remote: false
                            });
                        }
                        root.photoList = arr;
                    } else if (Array.isArray(resp.data)) {
                        root.photoList = resp.data;
                    }
                } else if (resp.error) {
                    var cleanMsg = String(resp.error || "").split("\n")[0].substring(0, 80);
                    root.showToast(cleanMsg, Theme.accentMagenta);
                }
            } catch(e) {
                root.showToast(qsTr("The scan did not respond."), Theme.accentMagenta);
            }
        }
    }

    // Keyboard Shortcuts (Lightroom & Studio Standards)
    ShortcutRouter {
        host: root
        viewport: root.viewport
        openFileDialog: openFileProc
    }

    // Opens the next / previous photo of the current folder (skips folders)
    function stepPhoto(delta) {
        var list = root.photoList || [];
        if (list.length === 0) return;
        var cur = -1;
        for (var i = 0; i < list.length; i++) {
            if (list[i].path === root.activePhotoPath) { cur = i; break; }
        }
        var j = cur;
        for (var n = 0; n < list.length; n++) {
            j = (j + delta + list.length) % list.length;
            if (!list[j].is_dir) {
                if (list[j].path !== root.activePhotoPath) root.loadPhoto(list[j].path);
                return;
            }
        }
    }

    function moveControlFocus(forward) {
        var win = root.Window.window;
        if (!win) return;
        var from = win.activeFocusItem || win.contentItem;
        if (!from) return;
        var next = from.nextItemInFocusChain(forward);
        if (next) next.forceActiveFocus(forward ? Qt.TabFocusReason : Qt.BacktabFocusReason);
    }

    function toggleSection(key) {
        var m = Object.assign({}, root.collapsedSections);
        if (m[key] === true) delete m[key]; else m[key] = true;
        root.collapsedSections = m;
    }

    // Collapses every section of the current tab except `key`
    function soloSection(key) {
        var keys = ["profile", "wb", "tone", "presence", "curve", "hsl", "grading", "detail", "transform", "grain",
                    "colour", "retouch", "layers", "looks", "lut", "assistant", "fujifilm"];
        var m = {};
        for (var i = 0; i < keys.length; i++) if (keys[i] !== key) m[keys[i]] = true;
        root.collapsedSections = m;
    }

    function loadUiState() {
        var st = {};
        try { st = JSON.parse(Host.uiState() || "{}"); } catch (e) { st = {}; }
        if (typeof st.showLeftPanel === "boolean") root.showLeftPanel = st.showLeftPanel;
        if (typeof st.showRightPanel === "boolean") root.showRightPanel = st.showRightPanel;
        if (typeof st.showFilmstrip === "boolean") root.showFilmstrip = st.showFilmstrip;
        if (["adjust", "masks", "looks", "ai"].indexOf(st.rightTab) >= 0) root.rightTab = st.rightTab;
        if (st.collapsedSections && typeof st.collapsedSections === "object") root.collapsedSections = st.collapsedSections;
        if (["dark", "grey", "theme"].indexOf(st.surround) >= 0) root.surround = st.surround;
        if (typeof st.proMode === "boolean") root.isProMode = st.proMode;
        if (typeof st.proofProfile === "string" && st.proofProfile !== "") root.proofProfile = st.proofProfile;
        if (["perceptual", "relative_colorimetric", "saturation", "absolute_colorimetric"].indexOf(st.proofIntent) >= 0)
            root.proofIntent = st.proofIntent;
        if (typeof st.gamutWarning === "boolean") root.gamutWarning = st.gamutWarning;
        root.uiStateLoaded = true;
    }

    function saveUiStateNow() {
        if (!root.uiStateLoaded) return;
        uiStateTimer.stop();
        Host.saveUiState(JSON.stringify({
            "showLeftPanel": root.showLeftPanel,
            "showRightPanel": root.showRightPanel,
            "showFilmstrip": root.showFilmstrip,
            "rightTab": root.rightTab,
            "collapsedSections": root.collapsedSections,
            "surround": root.surround,
            "proMode": root.isProMode,
            "proofProfile": root.proofProfile,
            "proofIntent": root.proofIntent,
            "gamutWarning": root.gamutWarning
        }));
    }

    function saveUiStateSoon() {
        if (root.uiStateLoaded) uiStateTimer.restart();
    }

    Timer {
        id: uiStateTimer
        interval: 600
        onTriggered: root.saveUiStateNow()
    }

    onShowLeftPanelChanged: saveUiStateSoon()
    onShowRightPanelChanged: saveUiStateSoon()
    onShowFilmstripChanged: saveUiStateSoon()
    onRightTabChanged: saveUiStateSoon()
    onIsProModeChanged: {
        if (!root.isProMode && root.rightTab === "masks") root.rightTab = "adjust";
        saveUiStateSoon();
    }

    // Switching from the top bar: say what changed and bring the first Pro section into view,
    // since the Pro sections sit below the Essentials ones and are easy to miss
    function setProMode(on) {
        if (root.isProMode === on) return;
        root.isProMode = on;
        if (on) {
            root.rightTab = "adjust";
            if (root.collapsedSections["curve"] === true) root.toggleSection("curve");
            proScrollTimer.restart();
            root.showToast(qsTr("Pro: tone curve, colour mixer, colour grading, detail, grain, colour management and the Masks tab are shown"), Theme.accent);
        } else {
            root.showToast(qsTr("Essentials: Pro sections are hidden. Settings made in them stay applied."), Theme.accent);
        }
    }

    // Opens one section on its tab, folds the others and scrolls it into view
    // (control socket `showSection`)
    function showSection(key) {
        var tabs = { "retouch": "masks", "layers": "masks", "looks": "looks", "lut": "looks", "assistant": "ai" };
        root.rightTab = tabs[key] || "adjust";
        root.soloSection(key);
        sectionScrollTimer.key = key;
        sectionScrollTimer.restart();
    }

    function findSection(item, key) {
        if (!item) return null;
        if (item.key === key && item.expanded !== undefined) return item;
        for (var i = 0; i < item.children.length; i++) {
            var found = root.findSection(item.children[i], key);
            if (found) return found;
        }
        return null;
    }

    Timer {
        id: sectionScrollTimer
        property string key: ""
        interval: 50
        onTriggered: {
            var flick = root.adjustScroll.contentItem;
            var section = root.findSection(flick.contentItem, key);
            if (!section) return;
            var y = section.mapToItem(flick.contentItem, 0, 0).y;
            flick.contentY = Math.max(0, Math.min(y, flick.contentHeight - flick.height));
        }
    }

    Timer {
        id: proScrollTimer
        interval: 50
        onTriggered: {
            var flick = root.adjustScroll.contentItem;
            var y = root.curveSection.mapToItem(flick.contentItem, 0, 0).y;
            flick.contentY = Math.max(0, Math.min(y, flick.contentHeight - flick.height));
        }
    }

    onCollapsedSectionsChanged: saveUiStateSoon()
    onSurroundChanged: saveUiStateSoon()

    // Viewport zoom: "fit", or screen pixels per photo pixel (1 = 100 %)
    function viewState() {
        return { "pixelScale": root.viewport.pixelScale, "detailShown": root.viewport.detailShown, "detailWanted": root.viewport.detailWanted,
                 "fullWidth": root.viewport.fullWidth, "previewWidth": root.viewport.previewWidth };
    }

    function setViewZoom(scale) {
        if (scale === "fit") {
            root.viewport.resetZoomFit();
            return "OK";
        }
        var s = Number(scale);
        if (!(s > 0)) return "Error: zoom must be fit or a positive number";
        root.viewport.setZoomAtPoint(root.viewport.width / 2, root.viewport.height / 2, root.viewport.zoomForPixelScale(s));
        return "OK";
    }

    function loadPhoto(path) {
        if (path !== root.activePhotoPath) {
            root.autosave();
            root.savedRecipeJson = "";
            root.preSocialRecipe = null;
            // Undo history and grade versions belong to the previous photo
            undoCommitTimer.stop();
            historyCtrl.reset();
            root.gradeVersions = { "A": null, "B": null, "C": null, "D": null };
            root.currentGradeVersion = "A";
            root.snapshots = [];
        }
        root.activePhotoPath = path;
        // The full-resolution frame is decoded again for this photo (see full_ready);
        // discard any pending detail/loupe/reference/AI replies belonging to the previous photo.
        root.viewport.fullWidth = 0;
        root.viewport.fullHeight = 0;
        root.viewport.detailFailed();
        root.viewport.loupeShown = null;
        root.sendDaemonCommand({
            cmd: "load",
            path: path
        });
    }

    // ---- Cloud accounts (src/cloud in the engine) ----

    function refreshCloud() {
        if (!cloudStatusProc.running) {
            cloudStatusProc.command = [root.resolveEnginePath(), "cloud", "status"];
            cloudStatusProc.running = true;
        }
    }

    function openCloudFolder(account, path) {
        root.cloudAccount = account;
        root.cloudPath = path;
        root.isListingFolder = true;
        root.photoList = [];
        cloudListProc.running = false;
        cloudListProc.command = [root.resolveEnginePath(), "cloud", "list", account, path];
        cloudListProc.running = true;
    }

    function leaveCloud() {
        root.cloudAccount = "";
        root.cloudPath = "";
        cloudListProc.running = false;
    }

    // Downloads the given photos ([{ remote, local }]) one after another, then runs `then`
    function fetchCloudPhotos(items, then) {
        var pending = [];
        for (var i = 0; i < items.length; i++) pending.push(items[i]);
        cloudFetchProc.running = false;
        root.cloudFetchQueue = pending;
        root.cloudAfterFetch = then;
        root.fetchNextCloudPhoto();
    }

    function fetchNextCloudPhoto() {
        if (root.cloudFetchQueue.length === 0) {
            root.cloudFetching = "";
            var then = root.cloudAfterFetch;
            root.cloudAfterFetch = null;
            if (then) then();
            return;
        }
        var next = root.cloudFetchQueue[0];
        root.cloudFetching = next.local;
        cloudFetchProc.command = [root.resolveEnginePath(), "cloud", "fetch", next.account, next.remote];
        cloudFetchProc.running = true;
    }

    function cloudItem(localPath) {
        for (var i = 0; i < root.photoList.length; i++) {
            var it = root.photoList[i];
            if (it.path === localPath && it.is_remote) return { account: root.cloudAccount, remote: it.remote_path, local: it.path };
        }
        return null;
    }

    function uploadExported(paths) {
        if (root.cloudUploadAccount === "" || paths.length === 0) return;
        root.cloudUploadQueue = root.cloudUploadQueue.concat(paths);
        if (!cloudUploadProc.running) root.uploadNext();
    }

    function uploadNext() {
        if (root.cloudUploadQueue.length === 0) {
            if (root.cloudUploaded > 0)
                root.showToast(qsTr("Uploaded %n file(s) to %1", "", root.cloudUploaded).arg(root.cloudUploadFolder), Theme.accentGreen);
            root.cloudUploaded = 0;
            return;
        }
        var file = root.cloudUploadQueue[0];
        root.cloudUploadQueue = root.cloudUploadQueue.slice(1);
        cloudUploadProc.command = [root.resolveEnginePath(), "cloud", "upload", root.cloudUploadAccount, file];
        cloudUploadProc.running = true;
    }

    function engineError(output, fallback) {
        try {
            var resp = JSON.parse(String(output).trim().split("\n").pop());
            if (resp.error) return String(resp.error).split("\n")[0].substring(0, 200);
        } catch (e) {}
        return fallback;
    }

    Process {
        id: cloudStatusProc
        onFinished: function(exitCode, output) {
            try {
                var resp = JSON.parse(output);
                if (resp.success && resp.data) {
                    root.cloudProviders = resp.data.providers || [];
                    root.cloudAccounts = resp.data.accounts || [];
                    root.cloudUploadFolder = resp.data.upload_folder || "OmaStudio";
                    // An account removed elsewhere is not left open in the filmstrip
                    var known = false;
                    for (var i = 0; i < root.cloudAccounts.length; i++)
                        if (root.cloudAccounts[i].id === root.cloudAccount) known = true;
                    if (root.cloudAccount !== "" && !known) {
                        root.leaveCloud();
                        filmstrip.currentFolder = "~/Pictures";
                        root.scanLocalFolder("~/Pictures");
                    }
                }
            } catch (e) {
                console.warn("cloud status:", e);
            }
        }
    }

    Process {
        id: cloudListProc
        onFinished: function(exitCode, output) {
            root.isListingFolder = false;
            try {
                var resp = JSON.parse(output);
                if (resp.success && resp.data && resp.data.account === root.cloudAccount) {
                    root.photoList = resp.data.items || [];
                } else if (!resp.success) {
                    root.showToast(root.engineError(output, qsTr("The folder could not be listed.")), Theme.accentMagenta);
                }
            } catch (e) {
                if (exitCode !== -1) root.showToast(qsTr("The folder could not be listed."), Theme.accentMagenta);
            }
        }
    }

    Process {
        id: cloudFetchProc
        onFinished: function(exitCode, output) {
            var done = root.cloudFetchQueue.length > 0 ? root.cloudFetchQueue[0] : null;
            var ok = false;
            var thumbnail = "";
            try {
                var resp = JSON.parse(output);
                ok = resp.success === true;
                if (ok && resp.data) thumbnail = resp.data.thumbnail || "";
                if (!ok) root.showToast(root.engineError(output, qsTr("Download failed")), Theme.accentMagenta);
            } catch (e) {
                // Cancelled (another photo was picked) or no answer
                if (exitCode !== -1) root.showToast(qsTr("Download failed"), Theme.accentMagenta);
            }
            if (exitCode === -1) return;
            if (!ok) {
                root.cloudFetchQueue = [];
                root.cloudAfterFetch = null;
                root.cloudFetching = "";
                return;
            }
            // The downloaded copy has a thumbnail now
            if (done && thumbnail !== "" && root.cloudAccount !== "") {
                var list = root.photoList.slice();
                for (var i = 0; i < list.length; i++) {
                    if (list[i].path === done.local && !list[i].thumbnail)
                        list[i] = Object.assign({}, list[i], { "thumbnail": thumbnail });
                }
                root.photoList = list;
            }
            root.cloudFetchQueue = root.cloudFetchQueue.slice(1);
            root.fetchNextCloudPhoto();
        }
    }

    Process {
        id: cloudUploadProc
        onFinished: function(exitCode, output) {
            try {
                var resp = JSON.parse(output);
                if (resp.success) root.cloudUploaded++;
                else root.showToast(qsTr("Upload failed: %1").arg(root.engineError(output, qsTr("no answer from the engine"))), Theme.accentMagenta);
            } catch (e) {
                root.showToast(qsTr("Upload failed: %1").arg(qsTr("no answer from the engine")), Theme.accentMagenta);
            }
            root.uploadNext();
        }
    }

    // Sign-in: the engine prints the steps as JSON lines, then the result
    Process {
        id: cloudConnectProc
        splitLines: true
        stdinEnabled: true
        property string provider: ""
        property bool succeeded: false
        property string pendingSecret: ""
        onStarted: {
            if (cloudConnectProc.pendingSecret !== "") {
                cloudConnectProc.write(cloudConnectProc.pendingSecret + "\n");
                cloudConnectProc.pendingSecret = "";
            }
        }
        onLineRead: function(line) {
            try {
                var msg = JSON.parse(line);
                if (msg.event === "browser") {
                    root.cloudDialog.signInUrl = msg.url;
                } else if (msg.event === "device_code") {
                    root.cloudDialog.deviceCode = msg.code;
                    root.cloudDialog.signInUrl = msg.url;
                } else if (msg.success === true && msg.data) {
                    cloudConnectProc.succeeded = true;
                    root.showToast(qsTr("%1 connected").arg(msg.data.label), Theme.accentGreen);
                    root.refreshCloud();
                    root.showCloudModal = false;
                    filmstrip.isCatalogMode = false;
                    root.openCloudFolder(msg.data.id, "");
                } else if (msg.success === false) {
                    root.cloudDialog.errorText = String(msg.error || "").substring(0, 300);
                }
            } catch (e) {
                console.warn("cloud connect:", line);
            }
        }
        onFinished: function(exitCode, output) {
            root.cloudDialog.connectingLabel = "";
            root.cloudDialog.signInUrl = "";
            root.cloudDialog.deviceCode = "";
            if (exitCode !== 0 && !cloudConnectProc.succeeded && root.cloudDialog.errorText === "" && exitCode !== -1)
                root.cloudDialog.errorText = qsTr("Sign-in did not finish.");
        }
    }

    Process {
        id: cloudDisconnectProc
        onFinished: function(exitCode, output) {
            try {
                var resp = JSON.parse(output);
                if (resp.success) root.showToast(qsTr("Account disconnected"), Theme.accentCyan);
                else root.cloudDialog.errorText = root.engineError(output, "");
            } catch (e) {}
            root.refreshCloud();
        }
    }

    // A folder on this computer in the filmstrip (control socket `openFolder`)
    function openLocalFolder(dir) {
        root.leaveCloud();
        filmstrip.isCatalogMode = false;
        filmstrip.currentFolder = dir;
        root.scanLocalFolder(dir);
    }

    function scanLocalFolder(dir) {
        root.isListingFolder = true;
        listProc.command = [root.resolveEnginePath(), "scan", dir];
        listProc.running = true;
    }

    // Applies one of the 5 shared look presets from Rust (`src/recipe/look_presets.inc`).
    function applyPresetNamed(name) {
        var updatedJson = Host.applyLookPreset(JSON.stringify(root.buildRecipeObject()), String(name || ""));
        if (updatedJson === "") return false;
        root.pushUndoState();
        root.applyRecipeObject(JSON.parse(updatedJson));
        return true;
    }

    function triggerSocial(platformCode) {
        if (!platformCode || platformCode.length === 0) return;
        root.pushUndoState();
        // Switching platforms starts again from the edit before the first one
        if (!root.preSocialRecipe) root.preSocialRecipe = root.buildRecipeObject();
        root.showToast(qsTr("Preparing for %1…").arg(platformCode), Theme.accentCyan);
        root.sendDaemonCommand({
            cmd: "ai_social",
            platform: String(platformCode),
            recipe: root.preSocialRecipe
        });
    }

    function toggleCropMode() {
        root.viewport.isCropMode = !root.viewport.isCropMode;
        root.showToast(root.viewport.isCropMode ? qsTr("Crop mode: on (C)") : qsTr("Crop mode: off"), Theme.accentYellow);
        return root.viewport.isCropMode;
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
            id: topBar
            Layout.fillWidth: true
            isProMode: root.isProMode
            isSplitView: root.isSplitView
            isCropMode: root.viewport.isCropMode
            photoRating: root.photoRating
            photoFlag: root.photoFlag
            activePhotoName: {
                var p = root.activePhotoPath.split("/");
                return p[p.length - 1];
            }
            onRatingChanged: function(r) {
                root.photoRating = r;
                root.showToast(r > 0 ? qsTr("Rating: %1").arg("★".repeat(r)) : qsTr("Rating cleared"), Theme.accentYellow);
                root.saveSidecar(true);
            }
            onFlagChanged: function(f) {
                root.photoFlag = f;
                root.showToast(f === "pick" ? qsTr("Picked") : (f === "reject" ? qsTr("Rejected") : qsTr("Flag cleared")), f === "pick" ? Theme.accentGreen : (f === "reject" ? Theme.highlightClip : Theme.textDim));
                root.saveSidecar(true);
            }
            onToggleMode: root.setProMode(!root.isProMode)
            aiConnected: root.aiAssist.localConnected
            aiBusy: root.aiAssist.connecting
            // Behind Omarchy's default agent the live agent, not the one saved at connect time
            aiModel: root.aiAssist.choice.provider === "omarchy" ? root.aiAssist.omarchyAgent : (root.aiAssist.choice.model || "")
            aiProvider: root.aiAssist.choice.provider || ""
            aiProviderName: root.aiAssist.providerName(root.aiAssist.choice.provider || "")
            onConnectLocalAi: root.aiAssist.connectLocal()
            aiChatOpen: root.aiChatOpen
            onToggleAiChat: root.aiChatOpen = !root.aiChatOpen
            onUndoClicked: root.undo()
            onRedoClicked: root.redo()
            onOpenFileClicked: openFileProc.open()
            onToggleSplit: {
                root.isSplitView = !root.isSplitView;
                requestRender();
            }
            onToggleCrop: {
                root.viewport.isCropMode = !root.viewport.isCropMode;
                root.showToast(root.viewport.isCropMode ? qsTr("Crop mode: on (C)") : qsTr("Crop mode: off"), Theme.accentYellow);
            }
            onExportClicked: {
                root.exportDialog.isBatchMode = false;
                root.exportDialog.batchCount = 0;
                root.exportDialog.exportStatusText = "";
                root.exportDialog.exportStatusIsError = false;
                root.showExportModal = true;
            }
            onInfoClicked: {
                root.showAboutModal = true;
            }
        }

        // WORKSPACE CENTER (LEFT: NAVIGATOR + SCOPES + EXIF + HISTORY, CENTER: CANVAS, RIGHT: ADJUSTMENTS)
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            LeftInspector {
                id: leftInspector
                host: root
                viewport: centerWorkspace.viewport
            }

            CenterWorkspace {
                id: centerWorkspace
                host: root
                layersPanel: rightInspector.layersPanel
                retouchPanel: rightInspector.retouchPanel
                filmstrip: filmstrip
            }

            RightInspector {
                id: rightInspector
                host: root
                filmstrip: filmstrip
            }
        }

        // BOTTOM FILMSTRIP
        Filmstrip {
            id: filmstrip
            visible: root.showFilmstrip
            Layout.fillWidth: true
            photoList: root.photoList
            activePhotoPath: root.activePhotoPath
            activeRating: root.photoRating
            activeFlag: root.photoFlag
            isListingFolder: root.isListingFolder
            cloudAccounts: root.cloudAccounts
            cloudAccount: root.cloudAccount
            cloudPath: root.cloudPath
            isDownloadingRemote: root.cloudFetching !== ""
            downloadingPath: root.cloudFetching
            onSelectPhoto: function(path, isRemote) {
                var item = isRemote ? root.cloudItem(path) : null;
                if (item) {
                    root.fetchCloudPhotos([item], function() { root.loadPhoto(item.local); });
                } else {
                    root.loadPhoto(path);
                }
            }
            onNavigateFolder: function(remotePath) {
                if (root.cloudAccount !== "") {
                    root.openCloudFolder(root.cloudAccount, remotePath);
                    return;
                }
                root.showToast(qsTr("Opening %1").arg(remotePath), Theme.accent);
                filmstrip.currentFolder = remotePath;
                root.scanLocalFolder(remotePath);
            }
            onConnectRequested: {
                root.cloudDialog.errorText = "";
                root.refreshCloud();
                root.showCloudModal = true;
            }
            onOpenCloudAccount: function(account) {
                root.openCloudFolder(account, "");
            }
            onParentFolderRequested: {
                if (root.cloudAccount !== "") {
                    var segs = root.cloudPath.split("/").filter(function(p) { return p.length > 0; });
                    segs.pop();
                    root.openCloudFolder(root.cloudAccount, segs.join("/"));
                    return;
                }
                var cur = filmstrip.currentFolder || "~/Pictures";
                if (cur.indexOf("/") !== -1) {
                    var parts = cur.split("/").filter(function(p) { return p.length > 0; });
                    if (parts.length > 1) {
                        parts.pop();
                        var parentP = (cur.startsWith("/") ? "/" : "") + parts.join("/");
                        if (parentP === "~") parentP = Host.homeDir;
                        filmstrip.currentFolder = parentP;
                        root.scanLocalFolder(parentP);
                    }
                }
            }
            onSwitchSource: function(isCatalog) {
                root.leaveCloud();
                if (isCatalog) {
                    root.showToast(qsTr("Loading the catalog…"), Theme.accentCyan);
                    root.loadStudioCatalog("");
                } else {
                    filmstrip.currentFolder = "~/Pictures";
                    root.scanLocalFolder("~/Pictures");
                }
            }
            onRefreshRequested: {
                if (root.cloudAccount !== "" && !filmstrip.isCatalogMode) {
                    root.openCloudFolder(root.cloudAccount, root.cloudPath);
                } else if (filmstrip.isCatalogMode) {
                    root.showToast(qsTr("Refreshing the catalog…"), Theme.accentCyan);
                    root.loadStudioCatalog("");
                } else {
                    root.scanLocalFolder(filmstrip.currentFolder || "~/Pictures");
                }
            }
            onBatchExportRequested: {
                // Photos in a cloud account are downloaded before they can be exported
                var missing = [];
                for (var i = 0; i < filmstrip.selectedPaths.length; i++) {
                    var item = root.cloudItem(filmstrip.selectedPaths[i]);
                    if (item) missing.push(item);
                }
                if (missing.length > 0) {
                    root.showToast(qsTr("Downloading %n photo(s)…", "", missing.length), Theme.accentCyan);
                    root.fetchCloudPhotos(missing, root.openBatchExport);
                    return;
                }
                root.openBatchExport();
            }
        }
    }

    // Component aliases so MainWindow methods and Main.qml IPC work cleanly across modules
    readonly property alias viewport: centerWorkspace.viewport
    readonly property alias navigator: leftInspector.navigator
    readonly property alias adjustScroll: rightInspector.adjustScroll
    readonly property alias curveSection: rightInspector.curveSection
    readonly property alias fujiPanel: rightInspector.fujiPanel
    readonly property alias retouchPanel: rightInspector.retouchPanel
    readonly property alias layersPanel: rightInspector.layersPanel
    readonly property alias presetSel: rightInspector.presetSel
    readonly property alias aiAssist: rightInspector.aiAssist
    readonly property alias exportDialog: modalHost.exportDialog
    readonly property alias cloudDialog: modalHost.cloudDialog

    function openBatchExport() {
        root.exportDialog.isBatchMode = true;
        root.exportDialog.batchCount = filmstrip.selectedPaths.length;
        root.exportDialog.exportStatusText = "";
        root.exportDialog.exportStatusIsError = false;
        root.showExportModal = true;
    }

    function startExport(opts) {
        root.exportDialog.isExporting = true;
        if (root.exportDialog.isBatchMode && filmstrip.selectedPaths.length > 0) {
            root.exportDialog.exportStatusIsError = false;
            root.exportDialog.exportStatusText = qsTr("Exporting %n photos…", "", filmstrip.selectedPaths.length);
            var batchItems = [];
            for (var i = 0; i < filmstrip.selectedPaths.length; i++) {
                var p = filmstrip.selectedPaths[i];
                batchItems.push({
                    "path": p,
                    "recipe": (p === root.activePhotoPath ? root.buildRecipeObject() : null)
                });
            }
            root.sendDaemonCommand({
                cmd: "batch_export",
                items: batchItems,
                options: opts
            });
        } else {
            root.exportDialog.exportStatusIsError = false;
            root.exportDialog.exportStatusText = qsTr("Developing at full resolution…");
            exportProc.recipeJson = JSON.stringify(root.buildRecipeObject());
            var optsJson = JSON.stringify(opts);
            exportProc.command = [
                root.resolveEnginePath(),
                "export",
                root.activePhotoPath,
                "--recipe", "-",
                "--options", optsJson
            ];
            exportProc.running = true;
        }
    }

    function connectCloudAccount(provider, clientId, clientSecret) {
        if (cloudConnectProc.running) return;
        var label = provider;
        for (var i = 0; i < root.cloudProviders.length; i++)
            if (root.cloudProviders[i].id === provider) label = root.cloudProviders[i].label;
        root.cloudDialog.errorText = "";
        root.cloudDialog.signInUrl = "";
        root.cloudDialog.deviceCode = "";
        root.cloudDialog.connectingLabel = label;
        var cmd = [root.resolveEnginePath(), "cloud", "connect", provider];
        cloudConnectProc.pendingSecret = "";
        if (clientId !== "") {
            cmd = cmd.concat(["--client-id", clientId]);
            if (clientSecret !== "") {
                cmd.push("--client-secret-stdin");
                cloudConnectProc.pendingSecret = clientSecret;
            }
        }
        cloudConnectProc.provider = provider;
        cloudConnectProc.succeeded = false;
        cloudConnectProc.command = cmd;
        cloudConnectProc.running = true;
    }

    function cancelCloudConnect() {
        // The engine's helpers die with it, which also closes the sign-in server
        cloudConnectProc.running = false;
        root.cloudDialog.connectingLabel = "";
        root.cloudDialog.signInUrl = "";
        root.cloudDialog.deviceCode = "";
    }

    function disconnectCloudAccount(account) {
        if (root.cloudAccount === account) {
            root.leaveCloud();
            filmstrip.currentFolder = "~/Pictures";
            root.scanLocalFolder("~/Pictures");
        }
        if (root.cloudUploadAccount === account) root.cloudUploadAccount = "";
        cloudDisconnectProc.command = [root.resolveEnginePath(), "cloud", "disconnect", account];
        cloudDisconnectProc.running = true;
    }

    ToastBanner {
        host: root
        anchorColumn: centerWorkspace
        anchorStrip: centerWorkspace.gradeStrip
    }

    ModalHost {
        id: modalHost
        host: root
        filmstrip: filmstrip
    }

    Component.onCompleted: {
        loadUiState();
        root.sendDaemonCommand({ cmd: "icc_profiles" });
        scanLocalFolder("~/Pictures");
        refreshCloud();
        // Photo passed on the command line (`omastudio photo.NEF`, desktop "Open with")
        var openPath = Host.openPath;
        if (openPath && root.activePhotoPath === "") root.activePhotoPath = String(openPath);
        if (root.activePhotoPath !== "") {
            loadPhoto(root.activePhotoPath);
        }
    }

    function shutdown() {
        if (root._shuttingDown) return;
        root._shuttingDown = true;
        if (uiStateTimer.running) root.saveUiStateNow();
        root.autosave();
        if (daemonProc.running) {
            daemonProc.write(JSON.stringify({ cmd: "exit" }) + "\n");
            daemonProc.gracefulStop(2000);
        }
        if (exportProc.running) exportProc.running = false;
        if (listProc.running) listProc.running = false;
        if (cloudConnectProc.running) cloudConnectProc.running = false;
        if (cloudFetchProc.running) cloudFetchProc.running = false;
    }

    Component.onDestruction: root.shutdown()

    AiChatPanel {
        id: aiChat
        parent: root.viewport
        visible: root.aiChatOpen && root.aiAssist.localConnected
        z: 50
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: Theme.px(12)
        width: Math.min(Theme.px(360), parent.width - Theme.px(24))
        height: Math.min(Theme.px(440), parent.height - Theme.px(24))
        assistant: root.aiAssist
        activePhotoPath: root.activePhotoPath
        label: topBar.aiLabel
        onCloseRequested: root.aiChatOpen = false
        onReconnectRequested: root.aiAssist.connectLocal()
        onVisibleChanged: if (visible) focusInput()
    }
}
