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
    property bool showAboutModal: false
    property bool showLeftPanel: true

    property bool showRightPanel: true
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

    // ACES 1.3 & Color Space Management
    property string activeColorSpace: "sRGB"
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

    // Local Layered Adjustments & Masks
    property var adjustmentLayers: []

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
            root.applyRecipeObject(vMap[ver]);
            root.showToast("[Grade] Switched to Version " + ver, Theme.accentCyan);
        } else {
            vMap[ver] = cur;
            root.gradeVersions = vMap;
            root.showToast("[Grade] Version " + ver + " initialized", Theme.accentYellow);
        }
        root.requestRender();
    }

    function copyCurrentToVersion(ver) {
        var cur = root.buildRecipeObject();
        var vMap = Object.assign({}, root.gradeVersions);
        vMap[ver] = cur;
        root.gradeVersions = vMap;
        root.showToast("[Grade] Copied current grade to Version " + ver, Theme.accentGreen);
    }

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
    property var gdriveMemoryCache: ({})
    property bool isDownloadingRemote: false
    property bool isListingFolder: false
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
        root.contrastPivot = 0.435;
        root.colorBoost = 0.0;
        root.midtoneDetail = 0.0;
        root.activeLutName = "";
        root.activeLutIntensity = 1.0;
        root.activeLutPath = "";
        root.activeFilmSimulation = "none";
        root.activeFilmSimIntensity = 1.0;
        root.activeColorSpace = "sRGB";
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
        root.adjustmentLayers = [];
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
            "contrast_pivot": root.contrastPivot,
            "color_boost": root.colorBoost,
            "midtone_detail": root.midtoneDetail,
            "lut_name": root.activeLutName !== "" ? root.activeLutName : null,
            "lut_intensity": root.activeLutIntensity,
            "lut_path": root.activeLutPath !== "" ? root.activeLutPath : null,
            "color_space": root.activeColorSpace,
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
            "layers": root.adjustmentLayers,
            "defringe": root.defringeVal,
            "lens_distortion": root.lensDistortionVal,
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

        // DaVinci Primaries & 3D LUT
        if (r.contrast_pivot !== undefined) root.contrastPivot = Number(r.contrast_pivot) || 0.435;
        if (r.color_boost !== undefined) root.colorBoost = Number(r.color_boost) || 0.0;
        if (r.midtone_detail !== undefined) root.midtoneDetail = Number(r.midtone_detail) || 0.0;
        if (r.lut_name !== undefined) root.activeLutName = r.lut_name || "";
        if (r.lut_intensity !== undefined) root.activeLutIntensity = Number(r.lut_intensity) || 1.0;
        if (r.lut_path !== undefined) root.activeLutPath = r.lut_path || "";

        // ACES 1.3 & Film Simulations
        if (r.color_space !== undefined) root.activeColorSpace = r.color_space || "sRGB";
        if (r.aces_tonemap !== undefined) root.activeAcesTonemap = Boolean(r.aces_tonemap);
        if (r.film_simulation !== undefined) root.activeFilmSimulation = r.film_simulation || "none";
        if (r.film_sim_intensity !== undefined) root.activeFilmSimIntensity = Number(r.film_sim_intensity) || 1.0;

        // Capture One Skin Tone Uniformity
        if (r.skin_tone_enabled !== undefined) root.skinToneEnabled = Boolean(r.skin_tone_enabled);
        if (r.skin_target_hue !== undefined) root.skinTargetHue = Number(r.skin_target_hue) || 50.0;
        if (r.skin_hue_range !== undefined) root.skinHueRange = Number(r.skin_hue_range) || 32.0;
        if (r.skin_uniformity_hue !== undefined) root.skinUniformityHue = Number(r.skin_uniformity_hue) || 0.0;
        if (r.skin_uniformity_sat !== undefined) root.skinUniformitySat = Number(r.skin_uniformity_sat) || 0.0;
        if (r.skin_amount_hue !== undefined) root.skinAmountHue = Number(r.skin_amount_hue) || 0.0;
        if (r.skin_amount_sat !== undefined) root.skinAmountSat = Number(r.skin_amount_sat) || 0.0;

        // DaVinci RGB Primary Mixer
        if (r.rgb_mixer_enabled !== undefined) root.rgbMixerEnabled = Boolean(r.rgb_mixer_enabled);
        if (r.rgb_mixer_monochrome !== undefined) root.rgbMixerMonochrome = Boolean(r.rgb_mixer_monochrome);
        if (r.mixer_red_in_r !== undefined) root.mixerRedInR = Number(r.mixer_red_in_r) || 1.0;
        if (r.mixer_green_in_r !== undefined) root.mixerGreenInR = Number(r.mixer_green_in_r) || 0.0;
        if (r.mixer_blue_in_r !== undefined) root.mixerBlueInR = Number(r.mixer_blue_in_r) || 0.0;
        if (r.mixer_red_in_g !== undefined) root.mixerRedInG = Number(r.mixer_red_in_g) || 0.0;
        if (r.mixer_green_in_g !== undefined) root.mixerGreenInG = Number(r.mixer_green_in_g) || 1.0;
        if (r.mixer_blue_in_g !== undefined) root.mixerBlueInG = Number(r.mixer_blue_in_g) || 0.0;
        if (r.mixer_red_in_b !== undefined) root.mixerRedInB = Number(r.mixer_red_in_b) || 0.0;
        if (r.mixer_green_in_b !== undefined) root.mixerGreenInB = Number(r.mixer_green_in_b) || 0.0;
        if (r.mixer_blue_in_b !== undefined) root.mixerBlueInB = Number(r.mixer_blue_in_b) || 1.0;

        // Silver-Halide Film Grain
        if (r.grain_amount !== undefined) root.grainAmount = Number(r.grain_amount) || 0.0;
        if (r.grain_size !== undefined) root.grainSize = Number(r.grain_size) || 1.0;
        if (r.grain_roughness !== undefined) root.grainRoughness = Number(r.grain_roughness) || 50.0;

        // Local Adjustment Layers
        if (r.layers !== undefined && Array.isArray(r.layers)) {
            root.adjustmentLayers = r.layers;
        }

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

        if (r.rating !== undefined) root.photoRating = Number(r.rating) || 0;
        if (r.flag !== undefined) root.photoFlag = r.flag || "none";

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
                } else if (resp.action === "ai_auto" || resp.action === "ai_social" || resp.action === "ai_jev") {
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
            } else if (act === "ai_jev") {
                if (data) {
                    root.applyRecipeObject(data.recipe);
                    root.activeScene = data.scene;
                    var label = data.jev_decisions ? "TypeSafe Jev System 1" : "Local Heuristic";
                    root.showToast("[OK] " + label + " Applied", Theme.accentCyan);
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
            } else if (act === "batch_export") {
                exportDialog.isExporting = false;
                if (data) {
                    exportDialog.exportStatusText = "[OK] Batch Finished: " + data.succeeded + "/" + data.total + " photos exported";
                    root.showToast("[OK] Batch Export: " + data.succeeded + " succeeded, " + data.failed + " failed", Theme.accentGreen);
                }
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

    function triggerAiJev() {
        root.pushUndoState();
        root.showToast("[JEV] Querying TypeSafe AI Decision Engine...", Theme.accentCyan);
        root.sendDaemonCommand({ cmd: "ai_jev" });
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
                        var localPath = resp.data;
                        var fName = localPath.split("/").pop();
                        var stem = fName.lastIndexOf(".") !== -1 ? fName.substring(0, fName.lastIndexOf(".")) : fName;
                        var expectedThumb = Quickshell.env("HOME") + "/.cache/omastudio/thumbnails/" + stem + ".jpg";

                        // Update photoList model in-place so Filmstrip thumbnail appears immediately
                        var listCopy = root.photoList.slice();
                        for (var i = 0; i < listCopy.length; i++) {
                            if (listCopy[i].name === fName || listCopy[i].path.indexOf(fName) !== -1) {
                                listCopy[i].thumbnail = expectedThumb;
                                break;
                            }
                        }
                        root.photoList = listCopy;

                        root.loadPhoto(localPath);
                        root.showToast("[OK] Loaded cloud RAW: " + fName, Theme.accentCyan);
                    } else {
                        root.showToast("[ERR] Cloud fetch failed: " + (resp.error || "Unknown"), Theme.accentMagenta);
                    }
                } catch(e) {
                    root.showToast("[ERR] Cloud fetch parse error", Theme.accentMagenta);
                }
            }
        }
    }

    function navigateGdriveFolder(path, forceRefresh) {
        root.currentGdrivePath = path;
        filmstrip.currentFolder = path;
        if (!forceRefresh && root.gdriveMemoryCache && root.gdriveMemoryCache[path]) {
            root.photoList = root.gdriveMemoryCache[path];
        } else {
            root.isListingFolder = true;
        }
        var cmd = [root.resolveEnginePath(), "gdrive", "list", path];
        if (forceRefresh) {
            cmd.push("--refresh");
        }
        listProc.command = cmd;
        listProc.running = true;
    }

    // Process: Folder scan
    Process {
        id: listProc
        stdout: StdioCollector {
            waitForEnd: true
            onStreamFinished: {
                root.isListingFolder = false;
                try {
                    var resp = JSON.parse(text);
                    if (resp.success && resp.data) {
                        root.photoList = resp.data;
                        if (filmstrip.isGdriveMode) {
                            var cache = Object.assign({}, root.gdriveMemoryCache);
                            cache[root.currentGdrivePath] = resp.data;
                            root.gdriveMemoryCache = cache;
                        }
                    } else if (resp.error) {
                        var errStr = String(resp.error || "");
                        if (errStr.indexOf("rate limit") !== -1 || errStr.indexOf("403") !== -1 || errStr.indexOf("rateLimitExceeded") !== -1) {
                            root.showToast("[Cloud] Google Drive kota siniri: Onbellekteki dosyalar gosteriliyor.", Theme.accentYellow);
                        } else {
                            var cleanMsg = errStr.split("\n")[0].substring(0, 80);
                            root.showToast("[ERR] " + cleanMsg, Theme.accentMagenta);
                        }
                    }
                } catch(e) {
                    root.showToast("[ERR] Klasor taramasi yanit veremedi.", Theme.accentMagenta);
                }
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
        onActivated: { root.photoRating = 1; root.showToast("Rating: [ * ] 1 Star (1)", Theme.accentYellow); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "2"
        onActivated: { root.photoRating = 2; root.showToast("Rating: [ * * ] 2 Stars (2)", Theme.accentYellow); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "3"
        onActivated: { root.photoRating = 3; root.showToast("Rating: [ * * * ] 3 Stars (3)", Theme.accentYellow); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "4"
        onActivated: { root.photoRating = 4; root.showToast("Rating: [ * * * * ] 4 Stars (4)", Theme.accentYellow); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "5"
        onActivated: { root.photoRating = 5; root.showToast("Rating: [ * * * * * ] 5 Stars (5)", Theme.accentYellow); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "0"
        onActivated: { root.photoRating = 0; root.showToast("Rating Cleared (0)", Theme.textDim); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "X"
        onActivated: { root.photoFlag = (root.photoFlag === "reject" ? "none" : "reject"); root.showToast(root.photoFlag === "reject" ? "Flag: REJECT (X)" : "Flag: NONE", Theme.highlightClip); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "P"
        onActivated: { root.photoFlag = (root.photoFlag === "pick" ? "none" : "pick"); root.showToast(root.photoFlag === "pick" ? "Flag: PICK (P)" : "Flag: NONE", Theme.accentGreen); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "U"
        onActivated: { root.photoFlag = "none"; root.showToast("Flag: UNFLAGGED (U)", Theme.textDim); root.saveSidecar(); }
    }
    Shortcut {
        sequence: "E"
        onActivated: { root.showExportModal = !root.showExportModal; }
    }
    Shortcut {
        sequence: "Tab"
        onActivated: {
            var bothOpen = root.showLeftPanel || root.showRightPanel;
            root.showLeftPanel = !bothOpen;
            root.showRightPanel = !bothOpen;
            root.showToast(bothOpen ? "Cinematic Full Canvas (Tab)" : "Panels Restored (Tab)", Theme.accentCyan);
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
        root.isListingFolder = true;
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
            photoRating: root.photoRating
            photoFlag: root.photoFlag
            activePhotoName: {
                var p = root.activePhotoPath.split("/");
                return p[p.length - 1];
            }
            onRatingChanged: function(r) {
                root.photoRating = r;
                root.showToast("Rating: " + (r > 0 ? (r + " Stars") : "Cleared"), Theme.accentYellow);
                root.saveSidecar();
            }
            onFlagChanged: function(f) {
                root.photoFlag = f;
                root.showToast("Flag: " + f.toUpperCase(), f === "pick" ? Theme.accentGreen : (f === "reject" ? Theme.highlightClip : Theme.textDim));
                root.saveSidecar();
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
            onExportClicked: {
                exportDialog.isBatchMode = false;
                exportDialog.batchCount = 0;
                exportDialog.exportStatusText = "";
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
                implicitWidth: root.showLeftPanel ? 280 : 0
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
                                        font.family: Theme.fontFamily
                                        font.pixelSize: 9
                                        font.weight: Font.Bold
                                        color: Theme.accentPurple
                                    }
                                    Item { Layout.fillWidth: true }
                                    Text {
                                        text: root.activeScene ? Math.round(root.activeScene.confidence * 100) + "%" : ""
                                        textFormat: Text.PlainText
                                        font.family: Theme.monoFont
                                        font.pixelSize: 9
                                        color: Theme.textDim
                                    }
                                }

                                Text {
                                    text: root.activeScene ? root.activeScene.scene_type : "Analyzing..."
                                    textFormat: Text.PlainText
                                    font.family: Theme.fontFamily
                                    font.pixelSize: 11
                                    font.weight: Font.Bold
                                    color: Theme.textMain
                                }

                                Text {
                                    text: root.activeScene ? root.activeScene.description : "Extracting dynamic range..."
                                    textFormat: Text.PlainText
                                    font.family: Theme.fontFamily
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

                        Text {
                            text: "DaVinci Color Engine • [Alt+1..4] Switch Version"
                            textFormat: Text.PlainText
                            font.pixelSize: 8
                            font.family: Theme.monoFont
                            color: Theme.textDim
                        }
                    }
                }

                Viewport {
                    id: viewport
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
                    onFileDropped: function(filePath) {
                        root.loadPhoto(filePath);
                        root.showToast("[OK] Opened dropped RAW: " + filePath.split("/").pop(), Theme.accentCyan);
                    }
                    onFolderDropped: function(folderPath) {
                        filmstrip.currentFolder = folderPath;
                        root.scanLocalFolder(folderPath);
                        root.showToast("[OK] Scanning dropped folder: " + folderPath.split("/").pop(), Theme.accentCyan);
                    }
                }
            }

            // RIGHT ADJUSTMENT INSPECTOR (Dedicated Pure Tone & Color Controls)
            Rectangle {
                visible: root.showRightPanel
                implicitWidth: root.showRightPanel ? 320 : 0
                Layout.fillHeight: true
                color: Theme.bgSurface
                border.color: Theme.border
                border.width: root.showRightPanel ? 1 : 0

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
                                    font.family: Theme.fontFamily
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
                                        font.family: Theme.monoFont
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
                                    font.family: Theme.fontFamily
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
                                        font.family: Theme.monoFont
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
                                    font.family: Theme.fontFamily
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
                                        font.family: Theme.monoFont
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
                                    font.family: Theme.fontFamily
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
                                        font.family: Theme.monoFont
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
                                    font.family: Theme.fontFamily
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
                                        font.family: Theme.monoFont
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
                                    font.family: Theme.fontFamily
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
                                        font.family: Theme.monoFont
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

                            // DaVinci 3D LUT Engine (.cube)
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            LutSelector {
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

                            // DaVinci Resolve Primaries (Color Wheels + MD + Color Boost + Pivot)
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

                            // Detail & Optics
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Text {
                                    text: "DETAIL & OPTICS"
                                    textFormat: Text.PlainText
                                    font.family: Theme.fontFamily
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
                                        font.family: Theme.monoFont
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

                            // DaVinci Resolve RGB Primary Matrix Mixer
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            RgbMixerPanel {
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

                            // Capture One Skin Tone Uniformity Engine
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            SkinTonePanel {
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

                            // Film Simulation Panel (Fujifilm & Hasselblad)
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            FilmSimulationPanel {
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

                            // Photochemical Silver-Halide Film Grain
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            FilmGrainPanel {
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

                            // Local Layered Adjustments (Linear / Radial / Luma Range Masks)
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            LayersPanel {
                                id: layersPanel
                                Layout.fillWidth: true
                                layersList: root.adjustmentLayers
                                onLayersChanged: {
                                    root.adjustmentLayers = layersPanel.layersList;
                                    root.requestRender();
                                }
                            }

                            // ACES 1.3 & Color Management Panel
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            ColorSpaceSelector {
                                id: colorSpaceSelector
                                Layout.fillWidth: true
                                activeColorSpace: root.activeColorSpace
                                acesTonemap: root.activeAcesTonemap
                                onColorSpaceSelected: function(space) {
                                    root.activeColorSpace = space;
                                    root.requestRender();
                                }
                                onAcesTonemapToggled: function(enabled) {
                                    root.activeAcesTonemap = enabled;
                                    root.requestRender();
                                }
                            }

                            // Watermark & Branding Config
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }
                            WatermarkConfig {
                                id: watermarkConfig
                                Layout.fillWidth: true
                                watermarkEnabled: root.watermarkEnabled
                                watermarkType: root.watermarkType
                                text: root.watermarkText
                                logoPath: root.watermarkLogoPath
                                positionIndex: root.watermarkPositionIndex
                                watermarkOpacity: root.watermarkOpacity
                                size: root.watermarkSize
                                margin: root.watermarkMargin
                                colorHex: root.watermarkColorHex
                                dropShadow: root.watermarkDropShadow
                                onWatermarkChanged: function(opts) {
                                    root.watermarkEnabled = opts.enabled;
                                    root.watermarkType = opts.watermark_type;
                                    root.watermarkText = opts.text;
                                    root.watermarkLogoPath = opts.logo_path || "";
                                    root.watermarkPositionIndex = opts.position_index;
                                    root.watermarkOpacity = opts.opacity;
                                    root.watermarkSize = opts.size;
                                    root.watermarkMargin = opts.margin;
                                    root.watermarkColorHex = opts.color;
                                    root.watermarkDropShadow = opts.drop_shadow;
                                }
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
                                        font.family: Theme.monoFont
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
                                        font.family: Theme.monoFont
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
                                    font.family: Theme.monoFont
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
            activeRating: root.photoRating
            activeFlag: root.photoFlag
            isDownloadingRemote: root.isDownloadingRemote
            isListingFolder: root.isListingFolder
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
                root.showToast("[Folder] Navigating: " + remotePath, Theme.accent);
                root.navigateGdriveFolder(remotePath, false);
            }
            onParentFolderRequested: {
                var parts = root.currentGdrivePath.split("/").filter(function(p) { return p.length > 0; });
                var parentP = "Photos";
                if (parts.length > 1) {
                    parts.pop();
                    parentP = parts.join("/");
                }
                root.showToast("[Folder] Navigating: " + parentP, Theme.accent);
                root.navigateGdriveFolder(parentP, false);
            }
            onSwitchSource: function(isGdrive) {
                if (isGdrive) {
                    root.navigateGdriveFolder(root.currentGdrivePath, false);
                } else {
                    filmstrip.currentFolder = "~/Pictures";
                    root.scanLocalFolder("~/Pictures");
                }
            }
            onRefreshRequested: {
                if (filmstrip.isGdriveMode) {
                    root.showToast("[Cloud] Refreshing Google Drive cache...", Theme.accentCyan);
                    root.navigateGdriveFolder(root.currentGdrivePath, true);
                } else {
                    root.scanLocalFolder(filmstrip.currentFolder || "~/Pictures");
                }
            }
            onBatchExportRequested: {
                exportDialog.isBatchMode = true;
                exportDialog.batchCount = filmstrip.selectedPaths.length;
                exportDialog.exportStatusText = "";
                root.showExportModal = true;
            }
        }
    }

    // Toast Notification Banner (Auto-dismissing)
    Rectangle {
        visible: root.toastMessage !== ""
        anchors.top: parent.top
        anchors.topMargin: 52
        anchors.horizontalCenter: parent.horizontalCenter
        implicitWidth: Math.min(root.width - 64, toastContent.implicitWidth + 28)
        implicitHeight: 32
        radius: 16
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
                font.pixelSize: 11
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
            watermarkEnabled: root.watermarkEnabled
            watermarkOptions: root.buildWatermarkOptions()
            onCloseRequested: root.showExportModal = false
            onDoExport: function(opts) {
                exportDialog.isExporting = true;
                if (exportDialog.isBatchMode && filmstrip.selectedPaths.length > 0) {
                    exportDialog.exportStatusText = "Dispatching " + filmstrip.selectedPaths.length + " photos to multi-core Rayon pool...";
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
