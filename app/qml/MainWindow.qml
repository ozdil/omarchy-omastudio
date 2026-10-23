import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
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

    property bool showRightPanel: true
    // Same lists as RAW_EXTENSIONS and RASTER_EXTENSIONS in the engine
    readonly property var rawExtensions: ["3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "cs1", "dc2", "dcr", "dcs", "dng", "drf", "eip", "erf", "fff", "gpr", "iiq", "k25", "kc2", "kdc", "mdc", "mef", "mfw", "mos", "mrw", "nef", "nrw", "orf", "ori", "pef", "ptx", "pxn", "qtk", "raf", "raw", "rdc", "rw2", "rwl", "rwz", "sr2", "srf", "srw", "sti", "x3f"]
    readonly property var imageExtensions: ["jpg", "jpeg", "jpe", "png", "webp", "tif", "tiff", "heic", "heif", "hif", "avif", "jxl", "exr", "hdr", "pnm", "ppm", "pgm", "pam"]
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

    // Undo / Redo History Stack
    property var undoStack: []
    property var redoStack: []
    property bool isPerformingUndoRedo: false
    // Engine-only tone flags with no UI control yet; kept so UI saves don't drop them
    property bool highlightReconstruct: true
    property bool filmicAgx: false
    // Undo baseline: the recipe as of the last committed edit (see commitPendingUndo)
    property string undoBaselineJson: ""

    function pushUndoState() {
        if (root.isPerformingUndoRedo) return;
        root.commitPendingUndo();
        var r = root.buildRecipeObject();
        var json = JSON.stringify(r);
        var stack = root.undoStack.slice();
        if (stack.length === 0 || JSON.stringify(stack[stack.length - 1]) !== json) {
            if (stack.length >= 50) stack.shift();
            stack.push(r);
            root.undoStack = stack;
            root.redoStack = [];
        }
        root.undoBaselineJson = json;
    }

    // Slider, wheel and panel edits never called pushUndoState, so Ctrl+Z skipped
    // them. Every render request (re)starts undoCommitTimer; once edits settle, the
    // pre-edit baseline is pushed as one undo step (a whole drag = one step).
    function commitPendingUndo() {
        undoCommitTimer.stop();
        if (root.isPerformingUndoRedo || root.activePhotoPath === "") return;
        var cur = JSON.stringify(root.buildRecipeObject());
        var base = root.undoBaselineJson;
        if (base === "" || cur === base) {
            root.undoBaselineJson = cur;
            return;
        }
        var stack = root.undoStack.slice();
        var top = stack.length > 0 ? JSON.stringify(stack[stack.length - 1]) : "";
        if (top !== base) {
            if (stack.length >= 50) stack.shift();
            stack.push(JSON.parse(base));
            root.undoStack = stack;
        }
        root.redoStack = [];
        root.undoBaselineJson = cur;
    }

    Timer {
        id: undoCommitTimer
        interval: 600
        repeat: false
        onTriggered: root.commitPendingUndo()
    }

    function undo() {
        root.commitPendingUndo();
        if (root.undoStack.length === 0) {
            root.showToast(qsTr("Nothing to undo"), Theme.textDim);
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

        // Culling (stars / flag) is not part of the edit history
        prev = Object.assign({}, prev, { "rating": root.photoRating, "flag": root.photoFlag });
        root.applyRecipeObject(prev);
        root.isPerformingUndoRedo = false;
        root.showToast(qsTr("Undo"), Theme.accentCyan);
    }

    function redo() {
        root.commitPendingUndo();
        if (root.redoStack.length === 0) {
            root.showToast(qsTr("Nothing to redo"), Theme.textDim);
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

        // Culling (stars / flag) is not part of the edit history
        next = Object.assign({}, next, { "rating": root.photoRating, "flag": root.photoFlag });
        root.applyRecipeObject(next);
        root.isPerformingUndoRedo = false;
        root.showToast(qsTr("Redo"), Theme.accentCyan);
    }

    // History panel: the states in order (undo stack, the current edit, then redo
    // states), each named for what changed from the one before
    property var historyModel: []
    onUndoStackChanged: Qt.callLater(root.refreshHistory)
    onRedoStackChanged: Qt.callLater(root.refreshHistory)

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

    // What changed from a to b, e.g. "Exposure +0.50" or "Tone curve, Grain"
    function describeChange(a, b) {
        var names = [], keys = Object.keys(b);
        var single = "";
        for (var i = 0; i < keys.length; ++i) {
            var k = keys[i];
            if (k === "rating" || k === "flag" || k === "path" || k === "label") continue;
            if (JSON.stringify(a[k]) === JSON.stringify(b[k])) continue;
            var name = root.historyKeyLabel(k);
            if (name === "") name = qsTr("Edit");
            if (names.indexOf(name) < 0) names.push(name);
            single = k;
        }
        if (names.length === 0) return qsTr("Edit");
        if (names.length === 1 && typeof b[single] === "number" && root.historyKeyLabel(single) === names[0]
                && ["exposure", "contrast", "highlights", "shadows", "whites", "blacks", "texture", "clarity",
                    "dehaze", "vibrance", "saturation"].indexOf(single) >= 0) {
            var v = b[single];
            var text = single === "exposure" ? v.toFixed(2) : String(Math.round(v));
            return names[0] + " " + (v > 0 ? "+" : "") + text;
        }
        return names.length > 2 ? names.slice(0, 2).join(", ") + ", …" : names.join(", ");
    }

    function historyStates() {
        var states = root.undoStack.slice();
        states.push(root.buildRecipeObject());
        for (var i = root.redoStack.length - 1; i >= 0; --i) states.push(root.redoStack[i]);
        return states;
    }

    function refreshHistory() {
        if (root.activePhotoPath === "") {
            root.historyModel = [];
            return;
        }
        var states = root.historyStates();
        var model = [];
        for (var i = 0; i < states.length; ++i)
            model.push({ "label": i === 0 ? qsTr("Start") : root.describeChange(states[i - 1], states[i]) });
        root.historyModel = model;
    }

    // Go back or forward to a state in the history; later states stay until the next edit
    function jumpToHistory(index) {
        root.commitPendingUndo();
        var states = root.historyStates();
        if (index < 0 || index >= states.length || index === root.undoStack.length) return;
        root.isPerformingUndoRedo = true;
        root.undoStack = states.slice(0, index);
        root.redoStack = states.slice(index + 1).reverse();
        var target = Object.assign({}, states[index], { "rating": root.photoRating, "flag": root.photoFlag });
        root.applyRecipeObject(target);
        root.undoBaselineJson = JSON.stringify(root.buildRecipeObject());
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

    onCompareModeChanged: {
        if (root.compareMode) root.requestReference();
        else root.referenceImage = "";
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
        viewport.resetCrop();
        viewport.rotationAngle = 0.0;
        viewport.flipH = false;
        viewport.flipV = false;
        if (typeof presetSel !== "undefined" && presetSel) presetSel.activePreset = "";
        exportDialog.socialOptimizer.activePlatform = "";
        root.preSocialRecipe = null;
        if (typeof hslMixer !== "undefined" && hslMixer) hslMixer.resetAll();
        if (typeof colorWheels !== "undefined" && colorWheels) colorWheels.resetAllWheels();
        root.highlightReconstruct = true;
        root.filmicAgx = false;
        try { root.syncPanels(); } catch (e) { console.warn("syncPanels failed: " + e); }
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
        exportDialog.maxDimension = null;
        exportDialog.scalePercent = 100;
        exportDialog.socialOptimizer.activePlatform = "";
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
        return fujiPanel.run("read", ["fuji-recipe", p]) ? "OK" : "Error: busy";
    }

    function fujiRecipeLibrary() {
        return fujiPanel.recipes.map(function(e) {
            return { "path": e.path, "label": e.recipe.label, "film_simulation": e.recipe.recipe.film_simulation_name, "device": e.recipe.device };
        });
    }

    function aiEdit(prompt) {
        if (root.activePhotoPath === "") return "Error: no photo open";
        if (aiAssist.busy) return "Error: busy";
        aiAssist.promptText = String(prompt || "");
        aiAssist.runEdit();
        return "OK";
    }

    function aiMask(kind) {
        if (root.activePhotoPath === "") return "Error: no photo open";
        if (kind !== "subject" && kind !== "sky") return "Error: kind must be subject or sky";
        root.pushUndoState();
        layersPanel.addAiLayer(kind);
        return "OK";
    }

    function applyFujiRecipe(name) {
        var list = fujiPanel.recipes;
        for (var i = 0; i < list.length; i++) {
            if (list[i].path === name || list[i].recipe.label === name) {
                root.pushUndoState();
                root.applyRecipeObject(list[i].recipe.omastudio);
                return "OK";
            }
        }
        return "Error: no recipe named " + name;
    }

    function applyRecipeObject(r) {
        if (!r) return;
        root.wbTemp = Number(r.wb_temperature) || 5500.0;
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
        root.curvePoints = r.curve_points || [];
        root.curvePointsRed = r.curve_points_red || [];
        root.curvePointsGreen = r.curve_points_green || [];
        root.curvePointsBlue = r.curve_points_blue || [];
        root.hslH = r.hsl_hue || [0,0,0,0,0,0,0,0];
        root.hslS = r.hsl_sat || [0,0,0,0,0,0,0,0];
        root.hslL = r.hsl_lum || [0,0,0,0,0,0,0,0];
        root.sharpnessVal = root.num(r.sharpness, 25.0);
        root.denoiseLumVal = root.num(r.denoise_lum, 0.0);
        root.denoiseColVal = root.num(r.denoise_col, 10.0);
        root.vignetteVal = root.num(r.vignette, 0.0);
        if (r.rotation !== undefined) viewport.rotationAngle = root.num(r.rotation, 0.0);
        if (r.flip_h !== undefined) viewport.flipH = Boolean(r.flip_h);
        if (r.flip_v !== undefined) viewport.flipV = Boolean(r.flip_v);

        // Color Wheels
        if (r.lift) root.liftRgb = r.lift;
        if (r.lift_luma !== undefined) root.liftLuma = root.num(r.lift_luma, 0.0);
        if (r.gamma) root.gammaRgb = r.gamma;
        if (r.gamma_luma !== undefined) root.gammaLuma = root.num(r.gamma_luma, 0.0);
        if (r.gain) root.gainRgb = r.gain;
        if (r.gain_luma !== undefined) root.gainLuma = root.num(r.gain_luma, 0.0);
        if (r.offset) root.offsetRgb = r.offset;
        if (r.offset_luma !== undefined) root.offsetLuma = root.num(r.offset_luma, 0.0);

        // DaVinci Primaries & 3D LUT
        if (r.contrast_pivot !== undefined) root.contrastPivot = root.num(r.contrast_pivot, 0.435);
        if (r.color_boost !== undefined) root.colorBoost = root.num(r.color_boost, 0.0);
        if (r.midtone_detail !== undefined) root.midtoneDetail = root.num(r.midtone_detail, 0.0);
        if (r.lut_name !== undefined) root.activeLutName = r.lut_name || "";
        if (r.lut_intensity !== undefined) root.activeLutIntensity = root.num(r.lut_intensity, 1.0);
        if (r.lut_path !== undefined) root.activeLutPath = r.lut_path || "";

        // Gamut compression, ACES tone curve & Film Simulations
        // Recipes from earlier versions stored "ACEScg" for gamut compression
        if (r.gamut_compression !== undefined || r.color_space !== undefined)
            root.gamutCompression = Boolean(r.gamut_compression) || String(r.color_space || "").toLowerCase() === "acescg";
        if (r.aces_tonemap !== undefined) root.activeAcesTonemap = Boolean(r.aces_tonemap);
        if (r.film_simulation !== undefined) root.activeFilmSimulation = r.film_simulation || "none";
        if (r.film_sim_intensity !== undefined) root.activeFilmSimIntensity = root.num(r.film_sim_intensity, 1.0);

        // Capture One Skin Tone Uniformity
        if (r.skin_tone_enabled !== undefined) root.skinToneEnabled = Boolean(r.skin_tone_enabled);
        if (r.skin_target_hue !== undefined) root.skinTargetHue = root.num(r.skin_target_hue, 50.0);
        if (r.skin_hue_range !== undefined) root.skinHueRange = root.num(r.skin_hue_range, 32.0);
        if (r.skin_uniformity_hue !== undefined) root.skinUniformityHue = root.num(r.skin_uniformity_hue, 0.0);
        if (r.skin_uniformity_sat !== undefined) root.skinUniformitySat = root.num(r.skin_uniformity_sat, 0.0);
        if (r.skin_amount_hue !== undefined) root.skinAmountHue = root.num(r.skin_amount_hue, 0.0);
        if (r.skin_amount_sat !== undefined) root.skinAmountSat = root.num(r.skin_amount_sat, 0.0);

        // DaVinci RGB Primary Mixer
        if (r.rgb_mixer_enabled !== undefined) root.rgbMixerEnabled = Boolean(r.rgb_mixer_enabled);
        if (r.rgb_mixer_monochrome !== undefined) root.rgbMixerMonochrome = Boolean(r.rgb_mixer_monochrome);
        if (r.mixer_red_in_r !== undefined) root.mixerRedInR = root.num(r.mixer_red_in_r, 1.0);
        if (r.mixer_green_in_r !== undefined) root.mixerGreenInR = root.num(r.mixer_green_in_r, 0.0);
        if (r.mixer_blue_in_r !== undefined) root.mixerBlueInR = root.num(r.mixer_blue_in_r, 0.0);
        if (r.mixer_red_in_g !== undefined) root.mixerRedInG = root.num(r.mixer_red_in_g, 0.0);
        if (r.mixer_green_in_g !== undefined) root.mixerGreenInG = root.num(r.mixer_green_in_g, 1.0);
        if (r.mixer_blue_in_g !== undefined) root.mixerBlueInG = root.num(r.mixer_blue_in_g, 0.0);
        if (r.mixer_red_in_b !== undefined) root.mixerRedInB = root.num(r.mixer_red_in_b, 0.0);
        if (r.mixer_green_in_b !== undefined) root.mixerGreenInB = root.num(r.mixer_green_in_b, 0.0);
        if (r.mixer_blue_in_b !== undefined) root.mixerBlueInB = root.num(r.mixer_blue_in_b, 1.0);

        // Silver-Halide Film Grain
        if (r.grain_amount !== undefined) root.grainAmount = root.num(r.grain_amount, 0.0);
        if (r.grain_size !== undefined) root.grainSize = Number(r.grain_size) || 1.0;
        if (r.grain_roughness !== undefined) root.grainRoughness = root.num(r.grain_roughness, 50.0);
        root.colorChrome = root.num(r.color_chrome, 0.0);
        root.colorChromeBlue = root.num(r.color_chrome_blue, 0.0);

        // Local Adjustment Layers
        if (r.layers !== undefined && Array.isArray(r.layers)) {
            root.adjustmentLayers = r.layers;
        }
        root.retouchSpots = Array.isArray(r.retouch) ? r.retouch : [];

        // Optics
        if (r.defringe !== undefined) root.defringeVal = Number(r.defringe) || 0.0;
        if (r.lens_distortion !== undefined) root.lensDistortionVal = Number(r.lens_distortion) || 0.0;
        root.perspectiveVertical = root.num(r.perspective_vertical, 0.0);
        root.perspectiveHorizontal = root.num(r.perspective_horizontal, 0.0);
        root.perspectiveScale = root.num(r.perspective_scale, 100.0);

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

        if (r.rating !== undefined) root.photoRating = Number(r.rating) || 0;
        if (r.flag !== undefined) root.photoFlag = r.flag || "none";
        root.highlightReconstruct = r.highlight_reconstruct !== undefined ? Boolean(r.highlight_reconstruct) : true;
        root.filmicAgx = Boolean(r.filmic_agx);

        try { root.syncPanels(); } catch (e) { console.warn("syncPanels failed: " + e); }
        // Programmatic recipe changes (load, undo, paste, AI) are not user edits
        root.undoBaselineJson = JSON.stringify(root.buildRecipeObject());
        requestRender();
    }

    // Panels assign to their own properties while being edited, which replaces the
    // binding to the main state. Re-establish the bindings so undo, paste, AI and
    // photo switches are reflected in every panel (and stale panel values are not
    // written back on the next touch).
    function syncPanels() {
        hslMixer.hslHue = Qt.binding(function() { return root.hslH; });
        hslMixer.hslSat = Qt.binding(function() { return root.hslS; });
        hslMixer.hslLum = Qt.binding(function() { return root.hslL; });

        lutSelector.activeLut = Qt.binding(function() { return root.activeLutName; });
        lutSelector.lutIntensity = Qt.binding(function() { return root.activeLutIntensity; });
        lutSelector.customLutPath = Qt.binding(function() { return root.activeLutPath; });

        colorWheels.liftRgb = Qt.binding(function() { return root.liftRgb; });
        colorWheels.liftLuma = Qt.binding(function() { return root.liftLuma; });
        colorWheels.gammaRgb = Qt.binding(function() { return root.gammaRgb; });
        colorWheels.gammaLuma = Qt.binding(function() { return root.gammaLuma; });
        colorWheels.gainRgb = Qt.binding(function() { return root.gainRgb; });
        colorWheels.gainLuma = Qt.binding(function() { return root.gainLuma; });
        colorWheels.offsetRgb = Qt.binding(function() { return root.offsetRgb; });
        colorWheels.offsetLuma = Qt.binding(function() { return root.offsetLuma; });
        colorWheels.colorBoost = Qt.binding(function() { return root.colorBoost; });
        colorWheels.midtoneDetail = Qt.binding(function() { return root.midtoneDetail; });
        colorWheels.contrastPivot = Qt.binding(function() { return root.contrastPivot; });
        colorWheels.rebindWheels();

        rgbMixerPanel.mixerEnabled = Qt.binding(function() { return root.rgbMixerEnabled; });
        rgbMixerPanel.monochrome = Qt.binding(function() { return root.rgbMixerMonochrome; });
        rgbMixerPanel.redInR = Qt.binding(function() { return root.mixerRedInR; });
        rgbMixerPanel.greenInR = Qt.binding(function() { return root.mixerGreenInR; });
        rgbMixerPanel.blueInR = Qt.binding(function() { return root.mixerBlueInR; });
        rgbMixerPanel.redInG = Qt.binding(function() { return root.mixerRedInG; });
        rgbMixerPanel.greenInG = Qt.binding(function() { return root.mixerGreenInG; });
        rgbMixerPanel.blueInG = Qt.binding(function() { return root.mixerBlueInG; });
        rgbMixerPanel.redInB = Qt.binding(function() { return root.mixerRedInB; });
        rgbMixerPanel.greenInB = Qt.binding(function() { return root.mixerGreenInB; });
        rgbMixerPanel.blueInB = Qt.binding(function() { return root.mixerBlueInB; });

        skinTonePanel.skinToneEnabled = Qt.binding(function() { return root.skinToneEnabled; });
        skinTonePanel.skinTargetHue = Qt.binding(function() { return root.skinTargetHue; });
        skinTonePanel.skinHueRange = Qt.binding(function() { return root.skinHueRange; });
        skinTonePanel.skinUniformityHue = Qt.binding(function() { return root.skinUniformityHue; });
        skinTonePanel.skinUniformitySat = Qt.binding(function() { return root.skinUniformitySat; });
        skinTonePanel.skinAmountHue = Qt.binding(function() { return root.skinAmountHue; });
        skinTonePanel.skinAmountSat = Qt.binding(function() { return root.skinAmountSat; });

        filmSimPanel.activeSimulation = Qt.binding(function() { return root.activeFilmSimulation; });
        filmSimPanel.intensity = Qt.binding(function() { return root.activeFilmSimIntensity; });

        filmGrainPanel.grainAmount = Qt.binding(function() { return root.grainAmount; });
        filmGrainPanel.grainSize = Qt.binding(function() { return root.grainSize; });
        filmGrainPanel.grainRoughness = Qt.binding(function() { return root.grainRoughness; });

        layersPanel.layersList = Qt.binding(function() { return root.adjustmentLayers; });

        colorSpaceSelector.gamutCompression = Qt.binding(function() { return root.gamutCompression; });
        colorSpaceSelector.acesTonemap = Qt.binding(function() { return root.activeAcesTonemap; });
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
                    root.showToast(qsTr("Failed to load RAW: %1").arg(resp.error || qsTr("Unknown error")), Theme.highlightClip);
                } else if (resp.action === "ai_auto" || resp.action === "ai_social") {
                    root.showToast(resp.error || qsTr("AI action failed"), Theme.highlightClip);
                } else if (resp.action === "batch_export") {
                    // Without this the dialog stays stuck on "Exporting Batch..."
                    exportDialog.isExporting = false;
                    exportDialog.exportStatusIsError = true;
                    exportDialog.exportStatusText = resp.error || qsTr("Batch export failed");
                } else if (resp.action === "adjust") {
                    root.renderFinished();
                    root.showToast((resp.error || qsTr("Preview render failed")), Theme.highlightClip);
                } else if (resp.action === "detail") {
                    viewport.detailFailed();
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
                    root.snapshots = data.recipe && Array.isArray(data.recipe.snapshots) ? data.recipe.snapshots : [];
                    root.savedRecipeJson = JSON.stringify(root.buildRecipeObject());
                    root.refreshHistory();
                    if (root.compareRef !== "before") root.compareRef = "before";
                    else root.requestReference();
                }
            } else if (act === "adjust") {
                root.renderFinished();
                if (data) {
                    root.activeHistogram = data.histogram;
                    viewport.imageSource = data.image;
                    navigator.imageSource = data.image;
                    root.reportProofError(data.proof_error);
                }
            } else if (act === "full_ready") {
                if (data && data.path === root.activePhotoPath) {
                    viewport.fullWidth = data.width;
                    viewport.fullHeight = data.height;
                }
            } else if (act === "detail") {
                if (data && data.tag === "loupe") viewport.showLoupe(data.image, data.region);
                else if (data) viewport.showDetail(data.image, data.region);
            } else if (act === "reference") {
                if (data) root.referenceImage = "file://" + data.image + "?v=" + (++root.referenceRevision);
            } else if (act === "icc_profiles") {
                if (Array.isArray(data)) root.iccProfiles = data;
            } else if (act === "ai_auto") {
                if (data) {
                    root.applyRecipeObject(data.recipe);
                    root.activeScene = data.scene;
                    root.showToast(qsTr("Auto applied"), Theme.accentCyan);
                }
            } else if (act === "ai_social") {
                if (data) {
                    if (data.recipe) {
                        root.applyRecipeObject(data.recipe);
                    }
                    // The platform's size and quality go to the export settings
                    if (data.max_dimension) {
                        exportDialog.scalePercent = 100;
                        exportDialog.maxDimension = data.max_dimension;
                    }
                    if (data.quality) exportDialog.quality = data.quality;
                    root.showToast(qsTr("%1 applied: long edge %2 px, quality %3").arg(data.platform).arg(data.max_dimension).arg(data.quality), Theme.accentCyan);
                }
            } else if (act === "save_recipe") {
                if (root.quietSaves > 0) root.quietSaves--;
                else root.showToast(qsTr("Settings saved"), Theme.accentGreen);
            } else if (act === "batch_export") {
                exportDialog.isExporting = false;
                if (data) {
                    exportDialog.exportStatusIsError = data.failed > 0;
                    exportDialog.exportStatusText = qsTr("%1 of %2 photos exported").arg(data.succeeded).arg(data.total);
                    var exported = [];
                    for (var ei = 0; ei < (data.results || []).length; ei++)
                        if (data.results[ei].exported_path) exported.push(data.results[ei].exported_path);
                    root.uploadExported(exported);
                    root.showToast(qsTr("Batch export: %1 succeeded, %2 failed").arg(data.succeeded).arg(data.failed),
                                   data.failed > 0 ? Theme.highlightClip : Theme.accentGreen);
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
        xmpProc.command = [root.resolveEnginePath(), "xmp", "import", root.activePhotoPath];
        xmpProc.running = true;
        return "OK";
    }

    function writeXmp(force) {
        if (root.activePhotoPath === "" || xmpProc.running) return "Error: no photo open or busy";
        xmpProc.action = "export";
        var cmd = [root.resolveEnginePath(), "xmp", "export", root.activePhotoPath, "--recipe", JSON.stringify(root.buildRecipeObject())];
        if (force) cmd.push("--force");
        xmpProc.command = cmd;
        xmpProc.running = true;
        return "OK";
    }

    Process {
        id: xmpProc
        property string action: ""
        onFinished: function(exitCode, output) {
            var resp;
            try { resp = JSON.parse(String(output || "")); } catch (e) { resp = { "success": false, "error": qsTr("No answer from the engine") }; }
            if (!resp.success) {
                root.showToast(String(resp.error || qsTr("XMP failed")).split("\n")[0], Theme.accentMagenta);
            } else if (xmpProc.action === "import") {
                root.pushUndoState();
                root.applyRecipeObject(resp.data.recipe);
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
                renderWatchdog.restart();
                var splitVal = root.isSplitView ? root.splitRatio : 0.0;
                root.sendDaemonCommand({
                    cmd: "adjust",
                    recipe: root.buildRecipeObject(),
                    split: splitVal,
                    highlight_mask: viewport.showHighlightMask,
                    shadow_mask: viewport.showShadowMask,
                    proof: root.proofRequest(),
                    focus_peaking: root.focusPeaking
                });
            }
        }
    }

    // Recovers if a render reply never arrives (engine restarted mid-render)
    Timer {
        id: renderWatchdog
        interval: 5000
        repeat: false
        onTriggered: root.renderFinished()
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
            root.renderInFlight = false;
        }
    }

    // Process: File picker
    Process {
        id: openFileProc
        command: ["zenity", "--file-selection", "--title=" + qsTr("Open photo"),
                  "--file-filter=" + qsTr("Photos") + " | " + root.globs(root.rawExtensions.concat(root.imageExtensions)),
                  "--file-filter=" + qsTr("RAW") + " | " + root.globs(root.rawExtensions),
                  "--file-filter=" + qsTr("Images") + " | " + root.globs(root.imageExtensions)]
        onFinished: function(exitCode, output) {
            var p = String(output || "").trim();
            if (exitCode === 0 && p.length > 0) {
                root.loadPhoto(p);
            }
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
            exportDialog.isExporting = false;
            try {
                var resp = JSON.parse(output);
                if (resp.success && resp.data) {
                    exportDialog.exportStatusIsError = false;
                    exportDialog.exportStatusText = qsTr("Saved to %1").arg(resp.data.exported_path);
                    root.uploadExported([resp.data.exported_path]);
                } else {
                    exportDialog.exportStatusIsError = true;
                    exportDialog.exportStatusText = resp.error || qsTr("Export failed");
                }
            } catch(e) {
                exportDialog.exportStatusIsError = true;
                exportDialog.exportStatusText = qsTr("Export finished without a result");
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
            root.showToast(viewport.isCropMode ? qsTr("Crop mode: on (C)") : qsTr("Crop mode: off"), Theme.accentYellow);
        }
    }
    Shortcut {
        sequence: "S"
        onActivated: {
            root.softProof = !root.softProof;
            root.showToast(root.softProof ? qsTr("Soft proof: %1").arg(root.proofProfileName()) : qsTr("Soft proof off"), Theme.accentYellow);
        }
    }
    Shortcut {
        sequence: "L"
        onActivated: viewport.loupeActive = !viewport.loupeActive
    }
    Shortcut {
        sequence: "Shift+F"
        onActivated: root.focusPeaking = !root.focusPeaking
    }
    Shortcut {
        sequence: "Shift+C"
        onActivated: root.compareMode = !root.compareMode
    }
    Shortcut {
        sequence: "Y"
        onActivated: {
            root.isSplitView = !root.isSplitView;
            root.requestRender();
        }
    }
    Shortcut {
        sequence: "Alt+1"
        onActivated: root.switchGradeVersion("A")
    }
    Shortcut {
        sequence: "Alt+2"
        onActivated: root.switchGradeVersion("B")
    }
    Shortcut {
        sequence: "Alt+3"
        onActivated: root.switchGradeVersion("C")
    }
    Shortcut {
        sequence: "Alt+4"
        onActivated: root.switchGradeVersion("D")
    }
    Shortcut {
        sequence: "Escape"
        onActivated: {
            if (root.showAboutModal) root.showAboutModal = false;
            else if (root.showExportModal) root.showExportModal = false;
            else if (viewport.isCropMode) viewport.isCropMode = false;
        }
    }


    // Lightroom Studio Culling & Ergonomics (1-5 Stars, Flags, Cinematic View)
    Shortcut {
        sequence: "1"
        onActivated: { root.photoRating = 1; root.showToast(qsTr("Rating: ★"), Theme.accentYellow); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "2"
        onActivated: { root.photoRating = 2; root.showToast(qsTr("Rating: ★★"), Theme.accentYellow); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "3"
        onActivated: { root.photoRating = 3; root.showToast(qsTr("Rating: ★★★"), Theme.accentYellow); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "4"
        onActivated: { root.photoRating = 4; root.showToast(qsTr("Rating: ★★★★"), Theme.accentYellow); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "5"
        onActivated: { root.photoRating = 5; root.showToast(qsTr("Rating: ★★★★★"), Theme.accentYellow); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "0"
        onActivated: { root.photoRating = 0; root.showToast(qsTr("Rating cleared"), Theme.textDim); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "X"
        onActivated: { root.photoFlag = (root.photoFlag === "reject" ? "none" : "reject"); root.showToast(root.flagMessage(root.photoFlag), Theme.highlightClip); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "P"
        onActivated: { root.photoFlag = (root.photoFlag === "pick" ? "none" : "pick"); root.showToast(root.flagMessage(root.photoFlag), Theme.accentGreen); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "U"
        onActivated: { root.photoFlag = "none"; root.showToast(qsTr("Flag cleared"), Theme.textDim); root.saveSidecar(true); }
    }
    Shortcut {
        sequence: "E"
        onActivated: {
            if (root.activePhotoPath === "" && !root.showExportModal) {
                root.showToast(qsTr("Open a photo first"), Theme.textDim);
                return;
            }
            root.showExportModal = !root.showExportModal;
        }
    }
    Shortcut {
        sequences: [StandardKey.Open]
        onActivated: openFileProc.running = true
    }
    Shortcut {
        sequence: "Ctrl+0"
        onActivated: viewport.resetZoomFit()
    }
    Shortcut {
        sequence: "Ctrl+1"
        onActivated: viewport.setZoomAbsolute(1.0)
    }
    Shortcut {
        sequence: "Ctrl+Right"
        onActivated: root.stepPhoto(1)
    }
    Shortcut {
        sequence: "Ctrl+Left"
        onActivated: root.stepPhoto(-1)
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
    Shortcut {
        sequence: "Tab"
        onActivated: {
            var anyOpen = root.showLeftPanel || root.showRightPanel || root.showFilmstrip;
            root.showLeftPanel = !anyOpen;
            root.showRightPanel = !anyOpen;
            root.showFilmstrip = !anyOpen;
        }
    }

    function toggleSection(key) {
        var m = Object.assign({}, root.collapsedSections);
        if (m[key] === true) delete m[key]; else m[key] = true;
        root.collapsedSections = m;
    }

    // Collapses every section of the current tab except `key`
    function soloSection(key) {
        var keys = ["profile", "wb", "tone", "presence", "curve", "hsl", "grading", "detail", "grain",
                    "colour", "layers", "looks", "lut", "assistant", "fujifilm"];
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

    function saveUiStateSoon() {
        if (root.uiStateLoaded) uiStateTimer.restart();
    }

    Timer {
        id: uiStateTimer
        interval: 600
        onTriggered: Host.saveUiState(JSON.stringify({
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
        }))
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

    Timer {
        id: proScrollTimer
        interval: 50
        onTriggered: {
            var flick = adjustScroll.contentItem;
            var y = curveSection.mapToItem(flick.contentItem, 0, 0).y;
            flick.contentY = Math.max(0, Math.min(y, flick.contentHeight - flick.height));
        }
    }

    onCollapsedSectionsChanged: saveUiStateSoon()
    onSurroundChanged: saveUiStateSoon()

    // Viewport zoom: "fit", or screen pixels per photo pixel (1 = 100 %)
    function viewState() {
        return { "pixelScale": viewport.pixelScale, "detailShown": viewport.detailShown, "detailWanted": viewport.detailWanted,
                 "fullWidth": viewport.fullWidth, "previewWidth": viewport.previewWidth };
    }

    function setViewZoom(scale) {
        if (scale === "fit") {
            viewport.resetZoomFit();
            return "OK";
        }
        var s = Number(scale);
        if (!(s > 0)) return "Error: zoom must be fit or a positive number";
        viewport.setZoomAtPoint(viewport.width / 2, viewport.height / 2, viewport.zoomForPixelScale(s));
        return "OK";
    }

    function loadPhoto(path) {
        if (path !== root.activePhotoPath) {
            root.autosave();
            root.savedRecipeJson = "";
            root.preSocialRecipe = null;
            // Undo history and grade versions belong to the previous photo
            undoCommitTimer.stop();
            root.undoBaselineJson = "";
            root.undoStack = [];
            root.redoStack = [];
            root.gradeVersions = { "A": null, "B": null, "C": null, "D": null };
            root.currentGradeVersion = "A";
            root.snapshots = [];
        }
        root.activePhotoPath = path;
        // The full-resolution frame is decoded again for this photo (see full_ready)
        viewport.fullWidth = 0;
        viewport.fullHeight = 0;
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
        property string provider: ""
        property bool succeeded: false
        onLineRead: function(line) {
            try {
                var msg = JSON.parse(line);
                if (msg.event === "browser") {
                    cloudDialog.signInUrl = msg.url;
                } else if (msg.event === "device_code") {
                    cloudDialog.deviceCode = msg.code;
                    cloudDialog.signInUrl = msg.url;
                } else if (msg.success === true && msg.data) {
                    cloudConnectProc.succeeded = true;
                    root.showToast(qsTr("%1 connected").arg(msg.data.label), Theme.accentGreen);
                    root.refreshCloud();
                    root.showCloudModal = false;
                    filmstrip.isCatalogMode = false;
                    root.openCloudFolder(msg.data.id, "");
                } else if (msg.success === false) {
                    cloudDialog.errorText = String(msg.error || "").substring(0, 300);
                }
            } catch (e) {
                console.warn("cloud connect:", line);
            }
        }
        onFinished: function(exitCode, output) {
            cloudDialog.connectingLabel = "";
            cloudDialog.signInUrl = "";
            cloudDialog.deviceCode = "";
            if (exitCode !== 0 && !cloudConnectProc.succeeded && cloudDialog.errorText === "" && exitCode !== -1)
                cloudDialog.errorText = qsTr("Sign-in did not finish.");
        }
    }

    Process {
        id: cloudDisconnectProc
        onFinished: function(exitCode, output) {
            try {
                var resp = JSON.parse(output);
                if (resp.success) root.showToast(qsTr("Account disconnected"), Theme.accentCyan);
                else cloudDialog.errorText = root.engineError(output, "");
            } catch (e) {}
            root.refreshCloud();
        }
    }

    function scanLocalFolder(dir) {
        root.isListingFolder = true;
        listProc.command = [root.resolveEnginePath(), "scan", dir];
        listProc.running = true;
    }

    // The looks match the engine's presets (Recipe::fuji_classic_chrome and friends in
    // src/recipe/mod.rs). Applying one replaces the previous look instead of adding to
    // it: tone, presence, curve, colour mixer, grading wheels, sharpening and vignette
    // start from their defaults; white balance, exposure, crop and film simulation stay.
    function applyPresetNamed(name) {
        var looks = {
            "Fuji Classic Chrome": { contrast: 15, highlights: -10, shadows: 5, clarity: 12, vibrance: -15, saturation: -8,
                                     hsl_sat: { 1: 10, 5: -20 }, hsl_hue: { 5: -8 }, vignette: -12 },
            "Fuji Velvia 50": { contrast: 28, highlights: -15, shadows: 10, vibrance: 30, saturation: 18, clarity: 16,
                                hsl_sat: { 3: 25, 5: 25 } },
            "Kodak Portra 400": { wb_temperature: 5700, wb_tint: 4, contrast: -5, highlights: -12, shadows: 18, whites: -8,
                                  texture: -5, clarity: 6, vibrance: 10, saturation: -4, hsl_lum: { 1: 12 }, hsl_sat: { 1: -5 } },
            "Leica Monochrom HC": { saturation: -100, vibrance: -100, contrast: 35, highlights: -20, shadows: -10, whites: 15,
                                    blacks: -25, texture: 15, clarity: 25, sharpness: 40, vignette: -18 },
            "Cinematic Teal & Orange": { contrast: 20, shadows: -10, highlights: -15, clarity: 15, vibrance: 20,
                                         lift: [-0.05, 0.04, 0.12], lift_luma: -0.05, gain: [0.14, 0.05, -0.08], gain_luma: 0.02,
                                         hsl_hue: { 5: -25, 1: -5 }, hsl_sat: { 5: 30, 1: 25 }, vignette: -20 }
        };
        // The names the Looks panel shows, so agents can use what the user sees
        var aliases = { "muted": "Fuji Classic Chrome", "vivid": "Fuji Velvia 50", "warm portrait": "Kodak Portra 400",
                        "black and white": "Leica Monochrom HC", "teal and orange": "Cinematic Teal & Orange" };
        var key = String(name).trim();
        for (var id in looks) if (id.toLowerCase() === key.toLowerCase()) key = id;
        if (aliases[key.toLowerCase()]) key = aliases[key.toLowerCase()];
        var look = looks[key];
        if (!look) return false;
        root.pushUndoState();
        var r = root.buildRecipeObject();
        var zero = ["contrast", "highlights", "shadows", "whites", "blacks", "texture", "clarity", "dehaze", "vibrance",
                    "saturation", "curve_highlights", "curve_lights", "curve_darks", "curve_shadows", "vignette",
                    "lift_luma", "gamma_luma", "gain_luma", "offset_luma"];
        for (var i = 0; i < zero.length; i++) r[zero[i]] = 0.0;
        r.sharpness = 25.0;
        r.lift = [0, 0, 0]; r.gamma = [0, 0, 0]; r.gain = [0, 0, 0]; r.offset = [0, 0, 0];
        r.hsl_hue = [0, 0, 0, 0, 0, 0, 0, 0]; r.hsl_sat = [0, 0, 0, 0, 0, 0, 0, 0]; r.hsl_lum = [0, 0, 0, 0, 0, 0, 0, 0];
        for (var k in look) {
            if (k === "hsl_hue" || k === "hsl_sat" || k === "hsl_lum") {
                for (var band in look[k]) r[k][Number(band)] = look[k][band];
            } else {
                r[k] = look[k];
            }
        }
        root.applyRecipeObject(r);
        requestRender();
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
        viewport.isCropMode = !viewport.isCropMode;
        root.showToast(viewport.isCropMode ? qsTr("Crop mode: on (C)") : qsTr("Crop mode: off"), Theme.accentYellow);
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
            id: topBar
            Layout.fillWidth: true
            isProMode: root.isProMode
            isSplitView: root.isSplitView
            isCropMode: viewport.isCropMode
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
            aiConnected: aiAssist.localConnected
            aiBusy: aiAssist.connecting
            // Behind Omarchy's default agent the live agent, not the one saved at connect time
            aiModel: aiAssist.choice.provider === "omarchy" ? aiAssist.omarchyAgent : (aiAssist.choice.model || "")
            aiProvider: aiAssist.choice.provider || ""
            aiProviderName: aiAssist.providerName(aiAssist.choice.provider || "")
            onConnectLocalAi: aiAssist.connectLocal()
            aiChatOpen: root.aiChatOpen
            onToggleAiChat: root.aiChatOpen = !root.aiChatOpen
            onUndoClicked: root.undo()
            onRedoClicked: root.redo()
            onOpenFileClicked: openFileProc.running = true
            onToggleSplit: {
                root.isSplitView = !root.isSplitView;
                requestRender();
            }
            onToggleCrop: {
                viewport.isCropMode = !viewport.isCropMode;
                root.showToast(viewport.isCropMode ? qsTr("Crop mode: on (C)") : qsTr("Crop mode: off"), Theme.accentYellow);
            }
            onExportClicked: {
                exportDialog.isBatchMode = false;
                exportDialog.batchCount = 0;
                exportDialog.exportStatusText = "";
                exportDialog.exportStatusIsError = false;
                root.showExportModal = true;
            }
            onInfoClicked: {
                root.showAboutModal = true;
            }
        }


        // WORKSPACE CENTER (LEFT: NAVIGATOR + HISTOGRAM + EXIF, CENTER: CANVAS, RIGHT: ADJUSTMENTS)
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // LEFT PANEL: NAVIGATOR & HISTOGRAM & METADATA (Lightroom Studio Layout)
            Rectangle {
                visible: root.showLeftPanel
                implicitWidth: root.showLeftPanel ? Theme.px(280) : 0
                Layout.minimumWidth: implicitWidth
                Layout.preferredWidth: implicitWidth
                Layout.fillHeight: true
                color: Theme.bgBase
                border.color: Theme.border
                border.width: root.showLeftPanel ? 1 : 0

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

                        // 2. DAVINCI RESOLVE VIDEO SCOPES (Waveform, RGB Parade, Vectorscope, Histogram)
                        ScopesView {
                            id: scopesView
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

                        // 4. Scene analysis (after Auto)
                        Rectangle {
                            // A guess from simple image statistics: shown only when it is fairly sure
                            visible: root.activeScene !== null && root.activeScene.confidence >= 0.9
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
                                        font.pixelSize: Theme.px(11)
                                        color: Theme.accentPurple
                                    }
                                    Text {
                                        text: qsTr("Suggested scene")
                                        textFormat: Text.PlainText
                                        font.family: Theme.fontFamily
                                        font.pixelSize: Theme.px(9)
                                        font.weight: Font.Bold
                                        color: Theme.accentPurple
                                        Layout.fillWidth: true
                                        elide: Text.ElideRight
                                    }
                                    Text {
                                        text: root.activeScene ? Math.round(root.activeScene.confidence * 100) + "%" : ""
                                        textFormat: Text.PlainText
                                        font.family: Theme.monoFont
                                        font.pixelSize: Theme.px(9)
                                        color: Theme.textDim
                                    }
                                }

                                Text {
                                    text: root.activeScene ? I18n.sceneType(root.activeScene.scene_type) : qsTr("Analyzing...")
                                    textFormat: Text.PlainText
                                    font.family: Theme.fontFamily
                                    font.pixelSize: Theme.px(11)
                                    font.weight: Font.Bold
                                    color: Theme.textMain
                                }

                                Text {
                                    text: root.activeScene ? I18n.sceneDescription(root.activeScene.scene_type, root.activeScene.description) : qsTr("Extracting dynamic range...")
                                    textFormat: Text.PlainText
                                    font.family: Theme.fontFamily
                                    font.pixelSize: Theme.px(9)
                                    color: Theme.textMuted
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }
                            }
                        }

                        // 5. History and snapshots
                        HistoryPanel {
                            Layout.fillWidth: true
                            visible: root.activePhotoPath !== ""
                            entries: root.historyModel
                            currentIndex: root.undoStack.length
                            snapshots: root.snapshots
                            onJumpRequested: function(index) { root.jumpToHistory(index) }
                            onSnapshotRequested: root.takeSnapshot()
                            onRestoreRequested: function(index) { root.restoreSnapshot(index) }
                            onRenameRequested: function(index, name) { root.renameSnapshot(index, name) }
                            onDeleteRequested: function(index) { root.deleteSnapshot(index) }
                        }

                        Item { Layout.fillHeight: true }
                    }
                }
            }

            // CENTER CANVAS (With DaVinci Grade Versions & Mac-like Touchpad Pinch-to-Zoom & Kinetic Pan)
            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                // DaVinci Resolve Secondary Toolstrip: Grade Versions & Look tools
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 32
                    color: Theme.bgDark
                    border.color: Theme.border
                    border.width: 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        spacing: 8

                        GradeVersionsBar {
                            currentVersion: root.currentGradeVersion
                            activeVersions: ({
                                "A": root.gradeVersions["A"] !== null,
                                "B": root.gradeVersions["B"] !== null,
                                "C": root.gradeVersions["C"] !== null,
                                "D": root.gradeVersions["D"] !== null
                            })
                            onSwitchVersion: function(v) { root.switchGradeVersion(v) }
                            onCopyCurrentToVersion: function(v) { root.copyCurrentToVersion(v) }
                        }

                        Item { Layout.fillWidth: true }

                        // The preview is simulated while soft proofing is on; a click turns it off
                        Rectangle {
                            visible: root.softProof
                            implicitWidth: proofBadge.implicitWidth + 14
                            implicitHeight: 20
                            radius: Theme.r(4)
                            color: Qt.rgba(Theme.accentYellow.r, Theme.accentYellow.g, Theme.accentYellow.b, 0.15)
                            border.color: Theme.accentYellow
                            border.width: 1
                            Text {
                                id: proofBadge
                                anchors.centerIn: parent
                                text: qsTr("Soft proof: %1").arg(root.proofProfileName())
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: Theme.px(9)
                                font.weight: Font.Bold
                                color: Theme.accentYellow
                            }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.softProof = false
                            }
                        }
                    }
                }

                Viewport {
                    id: viewport
                    surround: root.surround
                    onSurroundCycled: function(next) { root.surround = next }
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    isSplitView: root.isSplitView
                    splitRatio: root.splitRatio
                    watermarkEnabled: root.watermarkEnabled
                    watermarkType: root.watermarkType
                    watermarkText: root.watermarkText
                    watermarkLogoPath: root.watermarkLogoPath
                    watermarkPositionIndex: root.watermarkPositionIndex
                    watermarkOpacity: root.watermarkOpacity
                    watermarkSize: root.watermarkSize
                    watermarkMargin: root.watermarkMargin
                    watermarkColorHex: root.watermarkColorHex
                    watermarkDropShadow: root.watermarkDropShadow
                    exifMetadata: root.activeMetadata
                    perspectiveVertical: root.perspectiveVertical
                    perspectiveHorizontal: root.perspectiveHorizontal
                    perspectiveScale: root.perspectiveScale
                    // Mask editing follows the selected layer while the Masks tab is open
                    maskTool: root.activePhotoPath !== "" && root.showRightPanel && root.rightTab === "masks" && !retouchPanel.editing ? layersPanel.overlayTool : ""
                    retouchActive: root.activePhotoPath !== "" && root.showRightPanel && root.rightTab === "masks" && retouchPanel.editing
                    retouchSpots: root.retouchSpots
                    retouchSelected: retouchPanel.selected
                    retouchMode: retouchPanel.mode
                    retouchRadius: retouchPanel.radius
                    retouchFeather: retouchPanel.feather
                    retouchOpacity: retouchPanel.opacity_
                    onRetouchSpotsEdited: function(spots) {
                        root.retouchSpots = spots;
                        root.requestRender();
                    }
                    onRetouchSpotSelected: function(index) { retouchPanel.selected = index }
                    maskShape: layersPanel.editShape
                    maskBrushSize: layersPanel.brushSize
                    maskBrushFeather: layersPanel.brushFeather
                    maskBrushErase: layersPanel.brushErase
                    onMaskShapeEdited: function(shape) { layersPanel.setEditShape(shape) }
                    onMaskStrokePainted: function(points, aspect) { layersPanel.addStroke(points, aspect) }
                    onMaskColourPicked: function(r, g, b) { layersPanel.setPickedColour(r, g, b) }
                    onSplitRatioChangedByUser: function(r) {
                        root.splitRatio = r;
                        requestRender();
                    }
                    onRotationChangedByUser: function(a) {
                        root.requestRender();
                    }
                    onDetailRequested: function(region, outWidth, outHeight) {
                        root.sendDaemonCommand({
                            cmd: "detail",
                            recipe: root.buildRecipeObject(),
                            region: region,
                            out_width: outWidth,
                            out_height: outHeight,
                            proof: root.proofRequest(),
                            focus_peaking: root.focusPeaking
                        });
                    }
                    onLoupeRequested: function(region, outWidth, outHeight) {
                        root.sendDaemonCommand({
                            cmd: "detail",
                            tag: "loupe",
                            recipe: root.buildRecipeObject(),
                            region: region,
                            out_width: outWidth,
                            out_height: outHeight,
                            proof: root.proofRequest(),
                            focus_peaking: root.focusPeaking
                        });
                    }
                    focusPeaking: root.focusPeaking
                    onFocusPeakingToggled: root.focusPeaking = !root.focusPeaking
                    compareActive: root.compareMode
                    compareImage: root.referenceImage
                    compareChoices: root.compareChoices
                    compareChoice: root.compareRef
                    onCompareToggled: root.compareMode = !root.compareMode
                    onCompareChoicePicked: function(id) { root.compareRef = id }
                    onCropChangedByUser: function(cx, cy, cw, ch, aspect) {
                        root.cropX = cx;
                        root.cropY = cy;
                        root.cropW = cw;
                        root.cropH = ch;
                        root.cropAspect = aspect;
                    }
                    onFileDropped: function(filePath) {
                        root.loadPhoto(filePath);
                        root.showToast(qsTr("Opened %1").arg(filePath.split("/").pop()), Theme.accentCyan);
                    }
                    onFolderDropped: function(folderPath) {
                        filmstrip.currentFolder = folderPath;
                        root.scanLocalFolder(folderPath);
                        root.showToast(qsTr("Scanning folder %1").arg(folderPath.split("/").pop()), Theme.accentCyan);
                    }
                }
            }

            // RIGHT ADJUSTMENT INSPECTOR (Dedicated Pure Tone & Color Controls)
            Rectangle {
                visible: root.showRightPanel
                implicitWidth: root.showRightPanel ? Theme.px(320) : 0
                Layout.minimumWidth: implicitWidth
                Layout.preferredWidth: implicitWidth
                Layout.fillHeight: true
                color: Theme.bgSurface
                border.color: Theme.border
                border.width: root.showRightPanel ? 1 : 0

                RowLayout {
                    id: rightTabs
                    anchors.top: parent.top
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: 1
                    height: Theme.px(36)
                    spacing: 0

                    Repeater {
                        model: root.isProMode
                            ? [{ "id": "adjust", "label": qsTr("Adjust") }, { "id": "masks", "label": qsTr("Masks") },
                               { "id": "looks", "label": qsTr("Looks") }, { "id": "ai", "label": qsTr("AI") }]
                            : [{ "id": "adjust", "label": qsTr("Adjust") }, { "id": "looks", "label": qsTr("Looks") },
                               { "id": "ai", "label": qsTr("AI") }]
                        delegate: Item {
                            required property var modelData
                            readonly property bool active: root.rightTab === modelData.id
                            Layout.fillWidth: true
                            Layout.fillHeight: true

                            Text {
                                anchors.centerIn: parent
                                text: modelData.label
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.px(11)
                                font.weight: parent.active ? Font.DemiBold : Font.Medium
                                color: parent.active ? Theme.textMain : (tabMouse.containsMouse ? Theme.textMain : Theme.textMuted)
                            }
                            Rectangle {
                                anchors.bottom: parent.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                height: 2
                                color: parent.active ? Theme.accent : "transparent"
                            }
                            MouseArea {
                                id: tabMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.rightTab = modelData.id
                                Accessible.role: Accessible.PageTab
                                Accessible.name: modelData.label
                            }
                        }
                    }
                }

                Rectangle {
                    id: rightTabsLine
                    anchors.top: rightTabs.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: 1
                    color: Theme.border
                }

                // Copy, paste and reset the edit, and XMP sidecars: the same in every tab
                Rectangle {
                    id: editActionsLine
                    anchors.bottom: editActions.top
                    anchors.bottomMargin: 10
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: 1
                    color: Theme.border
                }
                ColumnLayout {
                    id: editActions
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.margins: 12
                    spacing: 6
                    enabled: root.activePhotoPath !== ""
                    opacity: enabled ? 1.0 : 0.45

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
                                        text: qsTr("Copy")
                                        textFormat: Text.PlainText
                                        font.family: Theme.monoFont
                                        font.pixelSize: Theme.px(10)
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
                                        text: qsTr("Paste")
                                        textFormat: Text.PlainText
                                        font.family: Theme.monoFont
                                        font.pixelSize: Theme.px(10)
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
                                    text: qsTr("Reset")
                                    textFormat: Text.PlainText
                                    font.family: Theme.monoFont
                                    font.pixelSize: Theme.px(10)
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

                        // Edits exchanged with other editors as XMP sidecars
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 6

                            Repeater {
                                model: [
                                    { "label": qsTr("Import XMP"), "tip": qsTr("Read the photo's .xmp sidecar written by another editor") },
                                    { "label": qsTr("Write XMP"), "tip": qsTr("Write the edit, rating and label to an .xmp sidecar for other editors") }
                                ]
                                delegate: Rectangle {
                                    Layout.fillWidth: true
                                    implicitHeight: 28
                                    radius: Theme.radiusSm
                                    opacity: root.activePhotoPath !== "" ? 1.0 : 0.4
                                    color: xmpBtnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard
                                    border.color: Theme.border

                                    Text {
                                        anchors.centerIn: parent
                                        text: modelData.label
                                        textFormat: Text.PlainText
                                        font.family: Theme.monoFont
                                        font.pixelSize: Theme.px(10)
                                        font.weight: Font.Medium
                                        color: Theme.textMain
                                    }
                                    MouseArea {
                                        id: xmpBtnMouse
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        hoverEnabled: true
                                        onClicked: index === 0 ? root.importXmp() : root.writeXmp(false)
                                    }
                                    ToolTip.visible: xmpBtnMouse.containsMouse
                                    ToolTip.delay: 500
                                    ToolTip.text: modelData.tip
                                }
                            }
                        }
                }

                ScrollView {
                    id: adjustScroll
                    anchors.top: rightTabsLine.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: editActionsLine.top
                    anchors.margins: 12
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: parent.width
                        spacing: 12

                        ColumnLayout {
                            visible: root.rightTab === "adjust"
                            Layout.fillWidth: true
                            spacing: 0

                            PanelSection {
                                key: "profile"
                                title: qsTr("Profile")
                                host: root
                                onReset: filmSimPanel.reset()

                                FilmSimulationPanel {
                                    embedded: true
                                    id: filmSimPanel
                                    Layout.fillWidth: true
                                    activeSimulation: root.activeFilmSimulation
                                    intensity: root.activeFilmSimIntensity
                                    onSimulationChanged: function(id, intensity) {
                                        root.activeFilmSimulation = id;
                                        root.activeFilmSimIntensity = intensity;
                                        root.requestRender();
                                    }
                                }
                            }
                            // Recipes, the camera over USB and Pixel Shift: for Fujifilm photos,
                            // and with no photo open so a camera can be connected
                            PanelSection {
                                key: "fujifilm"
                                title: "Fujifilm"
                                host: root
                                needsPhoto: false
                                visible: root.isFujifilm || root.activePhotoPath === ""
                                onReset: {
                                    root.colorChrome = 0.0;
                                    root.colorChromeBlue = 0.0;
                                    root.requestRender();
                                }

                                SliderGroup {
                                    visible: root.isFujifilm
                                    title: qsTr("Color Chrome Effect")
                                    from: 0.0
                                    to: 100.0
                                    value: root.colorChrome
                                    defaultValue: 0.0
                                    stepSize: 1.0
                                    accentColor: Theme.accentOrange
                                    onSliderMoved: function(v) { root.colorChrome = v; root.requestRender() }
                                }
                                SliderGroup {
                                    visible: root.isFujifilm
                                    title: qsTr("Color Chrome FX Blue")
                                    from: 0.0
                                    to: 100.0
                                    value: root.colorChromeBlue
                                    defaultValue: 0.0
                                    stepSize: 1.0
                                    accentColor: Theme.accentCyan
                                    onSliderMoved: function(v) { root.colorChromeBlue = v; root.requestRender() }
                                }
                                FujiPanel {
                                    id: fujiPanel
                                    Layout.fillWidth: true
                                    enginePath: root.resolveEnginePath()
                                    activePhotoPath: root.activePhotoPath
                                    selectedPaths: filmstrip.selectedPaths
                                    onRecipeApplied: function(recipe, label) {
                                        if (root.activePhotoPath === "") {
                                            root.showToast(qsTr("Open a photo first"), Theme.accentMagenta);
                                            return;
                                        }
                                        root.pushUndoState();
                                        root.applyRecipeObject(recipe);
                                        root.showToast(label, Theme.accentGreen);
                                    }
                                    onFileProduced: function(path, openIt) {
                                        if (openIt) root.loadPhoto(path);
                                    }
                                    onNotify: function(text, isError) {
                                        root.showToast(text, isError ? Theme.accentMagenta : Theme.accentGreen);
                                    }
                                }
                            }
                            PanelSection {
                                key: "wb"
                                title: qsTr("White balance")
                                host: root
                                onReset: {
                                    root.wbTemp = 5500.0;
                                    root.wbTint = 0.0;
                                    root.requestRender();
                                }

                                SliderGroup {
                                    title: qsTr("Temperature")
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
                                    title: qsTr("Tint")
                                    from: -100.0
                                    to: 100.0
                                    value: root.wbTint
                                    defaultValue: 0.0
                                    accentColor: Theme.accentMagenta
                                    onSliderMoved: function(v) { root.wbTint = v; root.requestRender() }
                                }
                            }
                            PanelSection {
                                key: "tone"
                                title: qsTr("Tone")
                                host: root
                                onReset: {
                                    root.expValue = 0.0;
                                    root.contrastValue = 0.0;
                                    root.highlightsValue = 0.0;
                                    root.shadowsValue = 0.0;
                                    root.whitesValue = 0.0;
                                    root.blacksValue = 0.0;
                                    root.requestRender();
                                }

                                SliderGroup {
                                    title: qsTr("Exposure")
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
                                    title: qsTr("Contrast")
                                    from: -100.0
                                    to: 100.0
                                    value: root.contrastValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.contrastValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Highlights")
                                    from: -100.0
                                    to: 100.0
                                    value: root.highlightsValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.highlightsValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Shadows")
                                    from: -100.0
                                    to: 100.0
                                    value: root.shadowsValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.shadowsValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Whites")
                                    from: -100.0
                                    to: 100.0
                                    value: root.whitesValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.whitesValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Blacks")
                                    from: -100.0
                                    to: 100.0
                                    value: root.blacksValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.blacksValue = v; root.requestRender() }
                                }
                            }
                            PanelSection {
                                key: "presence"
                                title: qsTr("Presence")
                                host: root
                                onReset: {
                                    root.textureValue = 0.0;
                                    root.clarityValue = 0.0;
                                    root.dehazeValue = 0.0;
                                    root.vibranceValue = 0.0;
                                    root.saturationValue = 0.0;
                                    root.requestRender();
                                }

                                SliderGroup {
                                    title: qsTr("Texture")
                                    from: -100.0
                                    to: 100.0
                                    value: root.textureValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.textureValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Clarity (Midtone Contrast)")
                                    from: -100.0
                                    to: 100.0
                                    value: root.clarityValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.clarityValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Dehaze")
                                    from: -100.0
                                    to: 100.0
                                    value: root.dehazeValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.dehazeValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Vibrance")
                                    from: -100.0
                                    to: 100.0
                                    value: root.vibranceValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.vibranceValue = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Saturation")
                                    from: -100.0
                                    to: 100.0
                                    value: root.saturationValue
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.saturationValue = v; root.requestRender() }
                                }
                            }
                            PanelSection {
                                id: curveSection
                                key: "curve"
                                title: qsTr("Tone curve")
                                host: root
                                visible: root.isProMode
                                pro: true
                                onReset: {
                                    root.pushUndoState();
                                    root.curveHighlights = 0.0;
                                    root.curveLights = 0.0;
                                    root.curveDarks = 0.0;
                                    root.curveShadows = 0.0;
                                    root.curvePoints = [];
                                    root.curvePointsRed = [];
                                    root.curvePointsGreen = [];
                                    root.curvePointsBlue = [];
                                    root.requestRender();
                                }

                                CurveEditor {
                                    Layout.fillWidth: true
                                    pointsRgb: root.curvePoints
                                    pointsRed: root.curvePointsRed
                                    pointsGreen: root.curvePointsGreen
                                    pointsBlue: root.curvePointsBlue
                                    histData: root.activeHistogram
                                    onPointsEdited: function(ch, pts) {
                                        if (ch === "red") root.curvePointsRed = pts;
                                        else if (ch === "green") root.curvePointsGreen = pts;
                                        else if (ch === "blue") root.curvePointsBlue = pts;
                                        else root.curvePoints = pts;
                                        root.requestRender();
                                    }
                                }

                                SliderGroup {
                                    title: qsTr("Curve Highlights")
                                    from: -100.0
                                    to: 100.0
                                    value: root.curveHighlights
                                    defaultValue: 0.0
                                    accentColor: Theme.accent
                                    onSliderMoved: function(v) { root.curveHighlights = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Curve Lights (Upper Mids)")
                                    from: -100.0
                                    to: 100.0
                                    value: root.curveLights
                                    defaultValue: 0.0
                                    accentColor: Theme.accentCyan
                                    onSliderMoved: function(v) { root.curveLights = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Curve Darks (Lower Mids)")
                                    from: -100.0
                                    to: 100.0
                                    value: root.curveDarks
                                    defaultValue: 0.0
                                    accentColor: Theme.accentYellow
                                    onSliderMoved: function(v) { root.curveDarks = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Curve Shadows")
                                    from: -100.0
                                    to: 100.0
                                    value: root.curveShadows
                                    defaultValue: 0.0
                                    accentColor: Theme.accentOrange
                                    onSliderMoved: function(v) { root.curveShadows = v; root.requestRender() }
                                }
                            }
                            PanelSection {
                                key: "hsl"
                                title: qsTr("Colour mixer")
                                host: root
                                visible: root.isProMode
                                pro: true
                                onReset: {
                                    root.hslH = [0, 0, 0, 0, 0, 0, 0, 0];
                                    root.hslS = [0, 0, 0, 0, 0, 0, 0, 0];
                                    root.hslL = [0, 0, 0, 0, 0, 0, 0, 0];
                                    hslMixer.resetAll();
                                    root.requestRender();
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
                            }
                            PanelSection {
                                key: "grading"
                                title: qsTr("Colour grading")
                                host: root
                                visible: root.isProMode
                                pro: true
                                resettable: false

                                ColorGradingWheels {
                                    embedded: true
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
                                    colorBoost: root.colorBoost
                                    midtoneDetail: root.midtoneDetail
                                    contrastPivot: root.contrastPivot
                                    onGradingChanged: {
                                        root.liftRgb = colorWheels.liftRgb;
                                        root.liftLuma = colorWheels.liftLuma;
                                        root.gammaRgb = colorWheels.gammaRgb;
                                        root.gammaLuma = colorWheels.gammaLuma;
                                        root.gainRgb = colorWheels.gainRgb;
                                        root.gainLuma = colorWheels.gainLuma;
                                        root.offsetRgb = colorWheels.offsetRgb;
                                        root.offsetLuma = colorWheels.offsetLuma;
                                        root.colorBoost = colorWheels.colorBoost;
                                        root.midtoneDetail = colorWheels.midtoneDetail;
                                        root.contrastPivot = colorWheels.contrastPivot;
                                        root.requestRender();
                                    }
                                }
                                RgbMixerPanel {
                                    embedded: true
                                    id: rgbMixerPanel
                                    Layout.fillWidth: true
                                    mixerEnabled: root.rgbMixerEnabled
                                    monochrome: root.rgbMixerMonochrome
                                    redInR: root.mixerRedInR
                                    greenInR: root.mixerGreenInR
                                    blueInR: root.mixerBlueInR
                                    redInG: root.mixerRedInG
                                    greenInG: root.mixerGreenInG
                                    blueInG: root.mixerBlueInG
                                    redInB: root.mixerRedInB
                                    greenInB: root.mixerGreenInB
                                    blueInB: root.mixerBlueInB
                                    onMixerChanged: {
                                        root.rgbMixerEnabled = rgbMixerPanel.mixerEnabled;
                                        root.rgbMixerMonochrome = rgbMixerPanel.monochrome;
                                        root.mixerRedInR = rgbMixerPanel.redInR;
                                        root.mixerGreenInR = rgbMixerPanel.greenInR;
                                        root.mixerBlueInR = rgbMixerPanel.blueInR;
                                        root.mixerRedInG = rgbMixerPanel.redInG;
                                        root.mixerGreenInG = rgbMixerPanel.greenInG;
                                        root.mixerBlueInG = rgbMixerPanel.blueInG;
                                        root.mixerRedInB = rgbMixerPanel.redInB;
                                        root.mixerGreenInB = rgbMixerPanel.greenInB;
                                        root.mixerBlueInB = rgbMixerPanel.blueInB;
                                        root.requestRender();
                                    }
                                }
                                SkinTonePanel {
                                    embedded: true
                                    id: skinTonePanel
                                    Layout.fillWidth: true
                                    skinToneEnabled: root.skinToneEnabled
                                    skinTargetHue: root.skinTargetHue
                                    skinHueRange: root.skinHueRange
                                    skinUniformityHue: root.skinUniformityHue
                                    skinUniformitySat: root.skinUniformitySat
                                    skinAmountHue: root.skinAmountHue
                                    skinAmountSat: root.skinAmountSat
                                    onSkinToneChanged: {
                                        root.skinToneEnabled = skinTonePanel.skinToneEnabled;
                                        root.skinTargetHue = skinTonePanel.skinTargetHue;
                                        root.skinHueRange = skinTonePanel.skinHueRange;
                                        root.skinUniformityHue = skinTonePanel.skinUniformityHue;
                                        root.skinUniformitySat = skinTonePanel.skinUniformitySat;
                                        root.skinAmountHue = skinTonePanel.skinAmountHue;
                                        root.skinAmountSat = skinTonePanel.skinAmountSat;
                                        root.requestRender();
                                    }
                                }
                            }
                            PanelSection {
                                key: "detail"
                                title: qsTr("Detail and optics")
                                host: root
                                visible: root.isProMode
                                pro: true
                                onReset: {
                                    root.sharpnessVal = 25.0;
                                    root.denoiseLumVal = 0.0;
                                    root.denoiseColVal = 10.0;
                                    root.vignetteVal = 0.0;
                                    root.defringeVal = 0.0;
                                    root.lensDistortionVal = 0.0;
                                    root.requestRender();
                                }

                                SliderGroup {
                                    title: qsTr("Sharpening")
                                    from: 0.0
                                    to: 100.0
                                    value: root.sharpnessVal
                                    defaultValue: 25.0
                                    onSliderMoved: function(v) { root.sharpnessVal = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Noise Reduction (Luma)")
                                    from: 0.0
                                    to: 100.0
                                    value: root.denoiseLumVal
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.denoiseLumVal = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Noise Reduction (Color)")
                                    from: 0.0
                                    to: 100.0
                                    value: root.denoiseColVal
                                    defaultValue: 10.0
                                    accentColor: Theme.accentYellow
                                    onSliderMoved: function(v) { root.denoiseColVal = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Vignette")
                                    from: -100.0
                                    to: 100.0
                                    value: root.vignetteVal
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.vignetteVal = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Defringe (Chromatic Aberration)")
                                    from: 0.0
                                    to: 100.0
                                    value: root.defringeVal
                                    defaultValue: 0.0
                                    accentColor: Theme.accentMagenta
                                    onSliderMoved: function(v) { root.defringeVal = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Lens Distortion Correction")
                                    from: -50.0
                                    to: 50.0
                                    value: root.lensDistortionVal
                                    defaultValue: 0.0
                                    accentColor: Theme.accentCyan
                                    onSliderMoved: function(v) { root.lensDistortionVal = v; root.requestRender() }
                                }
                            }
                            PanelSection {
                                key: "transform"
                                title: qsTr("Transform")
                                host: root
                                visible: root.isProMode
                                pro: true
                                onReset: {
                                    root.pushUndoState();
                                    root.perspectiveVertical = 0.0;
                                    root.perspectiveHorizontal = 0.0;
                                    root.perspectiveScale = 100.0;
                                    root.requestRender();
                                }

                                SliderGroup {
                                    title: qsTr("Vertical")
                                    from: -100.0
                                    to: 100.0
                                    value: root.perspectiveVertical
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.perspectiveVertical = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Horizontal")
                                    from: -100.0
                                    to: 100.0
                                    value: root.perspectiveHorizontal
                                    defaultValue: 0.0
                                    onSliderMoved: function(v) { root.perspectiveHorizontal = v; root.requestRender() }
                                }
                                SliderGroup {
                                    title: qsTr("Scale")
                                    from: 50.0
                                    to: 150.0
                                    value: root.perspectiveScale
                                    defaultValue: 100.0
                                    suffix: " %"
                                    onSliderMoved: function(v) { root.perspectiveScale = v; root.requestRender() }
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: qsTr("Vertical straightens converging lines, such as buildings photographed from below. Crop afterwards to remove the empty edges.")
                                    textFormat: Text.PlainText
                                    wrapMode: Text.WordWrap
                                    font.family: Theme.fontFamily
                                    font.pixelSize: Theme.px(10)
                                    color: Theme.textDim
                                }
                            }
                            PanelSection {
                                key: "grain"
                                title: qsTr("Grain")
                                host: root
                                visible: root.isProMode
                                pro: true
                                onReset: filmGrainPanel.reset()

                                FilmGrainPanel {
                                    embedded: true
                                    id: filmGrainPanel
                                    Layout.fillWidth: true
                                    grainAmount: root.grainAmount
                                    grainSize: root.grainSize
                                    grainRoughness: root.grainRoughness
                                    onGrainChanged: {
                                        root.grainAmount = filmGrainPanel.grainAmount;
                                        root.grainSize = filmGrainPanel.grainSize;
                                        root.grainRoughness = filmGrainPanel.grainRoughness;
                                        root.requestRender();
                                    }
                                }
                            }
                            PanelSection {
                                key: "colour"
                                title: qsTr("Colour management")
                                host: root
                                visible: root.isProMode
                                pro: true
                                resettable: false

                                ColorSpaceSelector {
                                    embedded: true
                                    id: colorSpaceSelector
                                    Layout.fillWidth: true
                                    gamutCompression: root.gamutCompression
                                    acesTonemap: root.activeAcesTonemap
                                    onGamutCompressionToggled: function(enabled) {
                                        root.gamutCompression = enabled;
                                        root.requestRender();
                                    }
                                    onAcesTonemapToggled: function(enabled) {
                                        root.activeAcesTonemap = enabled;
                                        root.requestRender();
                                    }
                                }

                                SoftProofPanel {
                                    Layout.fillWidth: true
                                    Layout.topMargin: 6
                                    active: root.softProof
                                    profile: root.proofProfile
                                    intent: root.proofIntent
                                    gamutWarning: root.gamutWarning
                                    profiles: root.iccProfiles
                                    onActiveToggled: function(on) { root.softProof = on }
                                    onProfilePicked: function(id) { root.proofProfile = id }
                                    onIntentPicked: function(id) { root.proofIntent = id }
                                    onGamutWarningToggled: function(on) { root.gamutWarning = on }
                                }
                            }
                        }

                        ColumnLayout {
                            visible: root.rightTab === "masks"
                            Layout.fillWidth: true
                            spacing: 0

                            PanelSection {
                                key: "retouch"
                                title: qsTr("Heal and clone")
                                host: root
                                onReset: {
                                    root.pushUndoState();
                                    root.retouchSpots = [];
                                    retouchPanel.selected = -1;
                                    root.requestRender();
                                }

                                RetouchPanel {
                                    id: retouchPanel
                                    Layout.fillWidth: true
                                    spots: root.retouchSpots
                                    enabledForPhoto: root.activePhotoPath !== ""
                                    onSpotsEdited: function(spots) {
                                        root.retouchSpots = spots;
                                        root.requestRender();
                                    }
                                }
                            }

                            PanelSection {
                                key: "layers"
                                title: qsTr("Local adjustments")
                                host: root
                                resettable: false

                                LayersPanel {
                                    embedded: true
                                    id: layersPanel
                                    Layout.fillWidth: true
                                    layersList: root.adjustmentLayers
                                    enginePath: root.resolveEnginePath()
                                    activePhotoPath: root.activePhotoPath
                                    onNotify: function(text, isError) {
                                        root.showToast(text, isError ? Theme.accentMagenta : Theme.accentGreen);
                                    }
                                    onLayersChanged: {
                                        root.adjustmentLayers = layersPanel.layersList;
                                        root.requestRender();
                                    }
                                }
                            }
                        }

                        ColumnLayout {
                            visible: root.rightTab === "looks"
                            Layout.fillWidth: true
                            spacing: 0

                            PanelSection {
                                key: "looks"
                                title: qsTr("Presets")
                                host: root
                                resettable: false

                            PresetSelector {
                                embedded: true
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
                            }
                            PanelSection {
                                key: "lut"
                                title: qsTr("LUT")
                                host: root
                                onReset: lutSelector.reset()

                                LutSelector {
                                    embedded: true
                                    id: lutSelector
                                    Layout.fillWidth: true
                                    activeLut: root.activeLutName
                                    lutIntensity: root.activeLutIntensity
                                    customLutPath: root.activeLutPath
                                    onLutChanged: function(name, intensity, path) {
                                        root.activeLutName = name;
                                        root.activeLutIntensity = intensity;
                                        root.activeLutPath = path;
                                        root.requestRender();
                                    }
                                }
                            }
                        }

                        ColumnLayout {
                            visible: root.rightTab === "ai"
                            Layout.fillWidth: true
                            spacing: 0

                            PanelSection {
                                key: "assistant"
                                title: qsTr("Edit assistant")
                                host: root
                                resettable: false

                            AiAssistPanel {
                                id: aiAssist
                                embedded: true
                                Layout.fillWidth: true
                                enginePath: root.resolveEnginePath()
                                activePhotoPath: root.activePhotoPath
                                recipeProvider: function() { return root.buildRecipeObject(); }
                                onRecipeSuggested: function(recipe, explanation) {
                                    root.pushUndoState();
                                    root.applyRecipeObject(recipe);
                                    root.showToast(qsTr("Assistant edit applied"), Theme.accentGreen);
                                }
                                onNotify: function(text, isError) {
                                    root.showToast(text, isError ? Theme.accentMagenta : Theme.accentGreen);
                                }
                                onLocalConnectedNow: root.aiChatOpen = true
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
                cloudDialog.errorText = "";
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

    function openBatchExport() {
        exportDialog.isBatchMode = true;
        exportDialog.batchCount = filmstrip.selectedPaths.length;
        exportDialog.exportStatusText = "";
        exportDialog.exportStatusIsError = false;
        root.showExportModal = true;
    }

    // Toast Notification Banner (Auto-dismissing)
    Rectangle {
        visible: root.toastMessage !== ""
        anchors.top: parent.top
        anchors.topMargin: 52
        anchors.horizontalCenter: parent.horizontalCenter
        implicitWidth: Math.min(root.width - 64, toastContent.implicitWidth + 28)
        implicitHeight: 32
        radius: Theme.r(16)
        color: Theme.bgDark
        border.color: root.toastColor
        border.width: 1
        clip: true
        z: 300

        RowLayout {
            id: toastContent
            anchors.centerIn: parent
            spacing: 8

            Text {
                text: root.toastMessage
                textFormat: Text.PlainText
                font.pixelSize: Theme.px(11)
                font.family: Theme.monoFont
                font.weight: Font.DemiBold
                color: root.toastColor
                elide: Text.ElideRight
                maximumLineCount: 1
                Layout.maximumWidth: root.width - 96
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
            host: root
            watermarkEnabled: root.watermarkEnabled
            watermarkOptions: root.buildWatermarkOptions()
            cloudAccounts: root.cloudAccounts
            uploadFolder: root.cloudUploadFolder
            uploadAccount: root.cloudUploadAccount
            onUploadAccountPicked: function(account) { root.cloudUploadAccount = account }
            onCloseRequested: root.showExportModal = false
            onDoExport: function(opts) {
                exportDialog.isExporting = true;
                if (exportDialog.isBatchMode && filmstrip.selectedPaths.length > 0) {
                    exportDialog.exportStatusIsError = false;
                    exportDialog.exportStatusText = qsTr("Exporting %n photos…", "", filmstrip.selectedPaths.length);
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
                    exportDialog.exportStatusIsError = false;
                    exportDialog.exportStatusText = qsTr("Developing at full resolution…");
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
        }
    }

    // CLOUD ACCOUNTS MODAL OVERLAY
    Rectangle {
        visible: root.showCloudModal
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.65)
        z: 100

        MouseArea {
            anchors.fill: parent
            onClicked: root.showCloudModal = false
        }

        CloudDialog {
            id: cloudDialog
            anchors.centerIn: parent
            visible: root.showCloudModal
            providers: root.cloudProviders
            accounts: root.cloudAccounts
            uploadFolder: root.cloudUploadFolder
            onCloseRequested: root.showCloudModal = false
            onConnectRequested: function(provider, clientId, clientSecret) {
                if (cloudConnectProc.running) return;
                var label = provider;
                for (var i = 0; i < root.cloudProviders.length; i++)
                    if (root.cloudProviders[i].id === provider) label = root.cloudProviders[i].label;
                cloudDialog.errorText = "";
                cloudDialog.signInUrl = "";
                cloudDialog.deviceCode = "";
                cloudDialog.connectingLabel = label;
                var cmd = [root.resolveEnginePath(), "cloud", "connect", provider];
                if (clientId !== "") cmd = cmd.concat(["--client-id", clientId, "--client-secret", clientSecret]);
                cloudConnectProc.provider = provider;
                cloudConnectProc.succeeded = false;
                cloudConnectProc.command = cmd;
                cloudConnectProc.running = true;
            }
            onCancelRequested: {
                // The engine's helpers die with it, which also closes the sign-in server
                cloudConnectProc.running = false;
                cloudDialog.connectingLabel = "";
                cloudDialog.signInUrl = "";
                cloudDialog.deviceCode = "";
            }
            onBrowseRequested: function(account) {
                root.showCloudModal = false;
                filmstrip.isCatalogMode = false;
                root.openCloudFolder(account, "");
            }
            onDisconnectRequested: function(account) {
                if (root.cloudAccount === account) {
                    root.leaveCloud();
                    filmstrip.currentFolder = "~/Pictures";
                    root.scanLocalFolder("~/Pictures");
                }
                if (root.cloudUploadAccount === account) root.cloudUploadAccount = "";
                cloudDisconnectProc.command = [root.resolveEnginePath(), "cloud", "disconnect", account];
                cloudDisconnectProc.running = true;
            }
        }
    }

    // ABOUT / INFO MODAL OVERLAY
    Rectangle {
        visible: root.showAboutModal
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.65)
        z: 101

        MouseArea {
            anchors.fill: parent
            onClicked: root.showAboutModal = false
        }

        AboutDialog {
            id: aboutDialog
            anchors.centerIn: parent
            onCloseRequested: root.showAboutModal = false
        }
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

    Component.onDestruction: {
        root.autosave();
        if (daemonProc.running) {
            daemonProc.write(JSON.stringify({ cmd: "exit" }) + "\n");
            daemonProc.running = false;
        }
        if (exportProc.running) exportProc.running = false;
        if (listProc.running) listProc.running = false;
        if (cloudConnectProc.running) cloudConnectProc.running = false;
        if (cloudFetchProc.running) cloudFetchProc.running = false;
        if (openFileProc.running) openFileProc.running = false;
    }

    AiChatPanel {
        id: aiChat
        parent: viewport
        visible: root.aiChatOpen && aiAssist.localConnected
        z: 50
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: Theme.px(12)
        width: Math.min(Theme.px(360), parent.width - Theme.px(24))
        height: Math.min(Theme.px(440), parent.height - Theme.px(24))
        assistant: aiAssist
        activePhotoPath: root.activePhotoPath
        label: topBar.aiLabel
        onCloseRequested: root.aiChatOpen = false
        onReconnectRequested: aiAssist.connectLocal()
        onVisibleChanged: if (visible) focusInput()
    }
}
