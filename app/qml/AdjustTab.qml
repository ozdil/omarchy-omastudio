import QtQuick
import QtQuick.Layouts

// Adjust tab of the right inspector: Profile, Fujifilm, White Balance, Tone, Presence,
// Tone Curve, Colour Mixer, Colour Grading, Detail & Optics, Transform, Grain, Colour Management.
ColumnLayout {
    id: tab

    property var host: null
    property var filmstrip: null

    readonly property alias fujiPanel: fujiPanel
    readonly property alias curveSection: curveSection

    visible: tab.host ? tab.host.rightTab === "adjust" : false
    Layout.fillWidth: true
    spacing: 0

    PanelSection {
        key: "profile"
        title: qsTr("Profile")
        host: tab.host
        onReset: filmSimPanel.reset()

        FilmSimulationPanel {
            embedded: true
            id: filmSimPanel
            Layout.fillWidth: true
            activeSimulation: tab.host ? tab.host.activeFilmSimulation : "none"
            intensity: tab.host ? tab.host.activeFilmSimIntensity : 1.0
            onSimulationChanged: function(id, intensity) {
                if (!tab.host) return;
                tab.host.activeFilmSimulation = id;
                tab.host.activeFilmSimIntensity = intensity;
                tab.host.requestRender();
            }
        }
    }

    // Recipes, the camera over USB and Pixel Shift: for Fujifilm photos,
    // and with no photo open so a camera can be connected
    PanelSection {
        key: "fujifilm"
        title: "Fujifilm"
        host: tab.host
        needsPhoto: false
        visible: tab.host ? (tab.host.isFujifilm || tab.host.activePhotoPath === "") : false
        onReset: {
            if (!tab.host) return;
            tab.host.colorChrome = 0.0;
            tab.host.colorChromeBlue = 0.0;
            tab.host.requestRender();
        }

        SliderGroup {
            visible: tab.host ? tab.host.isFujifilm : false
            title: qsTr("Color Chrome Effect")
            from: 0.0
            to: 100.0
            value: tab.host ? tab.host.colorChrome : 0.0
            defaultValue: 0.0
            stepSize: 1.0
            accentColor: Theme.accentOrange
            onSliderMoved: function(v) { if (tab.host) { tab.host.colorChrome = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            visible: tab.host ? tab.host.isFujifilm : false
            title: qsTr("Color Chrome FX Blue")
            from: 0.0
            to: 100.0
            value: tab.host ? tab.host.colorChromeBlue : 0.0
            defaultValue: 0.0
            stepSize: 1.0
            accentColor: Theme.accentCyan
            onSliderMoved: function(v) { if (tab.host) { tab.host.colorChromeBlue = v; tab.host.requestRender(); } }
        }
        FujiPanel {
            id: fujiPanel
            Layout.fillWidth: true
            enginePath: tab.host ? tab.host.resolveEnginePath() : ""
            activePhotoPath: tab.host ? tab.host.activePhotoPath : ""
            selectedPaths: tab.filmstrip ? tab.filmstrip.selectedPaths : []
            onRecipeApplied: function(recipe, label) {
                if (!tab.host) return;
                if (tab.host.activePhotoPath === "") {
                    tab.host.showToast(qsTr("Open a photo first"), Theme.accentMagenta);
                    return;
                }
                tab.host.pushUndoState();
                tab.host.patchRecipe(recipe);
                tab.host.showToast(label, Theme.accentGreen);
            }
            onFileProduced: function(path, openIt) {
                if (openIt && tab.host) tab.host.loadPhoto(path);
            }
            onNotify: function(text, isError) {
                if (tab.host) tab.host.showToast(text, isError ? Theme.accentMagenta : Theme.accentGreen);
            }
        }
    }

    PanelSection {
        key: "wb"
        title: qsTr("White balance")
        host: tab.host
        onReset: {
            if (!tab.host) return;
            tab.host.wbTemp = 5500.0;
            tab.host.wbTint = 0.0;
            tab.host.requestRender();
        }

        SliderGroup {
            title: qsTr("Temperature")
            from: 2000.0
            to: 12000.0
            value: tab.host ? tab.host.wbTemp : 5500.0
            defaultValue: 5500.0
            stepSize: 25.0
            suffix: " K"
            accentColor: Theme.accentOrange
            onSliderMoved: function(v) { if (tab.host) { tab.host.wbTemp = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Tint")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.wbTint : 0.0
            defaultValue: 0.0
            accentColor: Theme.accentMagenta
            onSliderMoved: function(v) { if (tab.host) { tab.host.wbTint = v; tab.host.requestRender(); } }
        }
    }

    PanelSection {
        key: "tone"
        title: qsTr("Tone")
        host: tab.host
        onReset: {
            if (!tab.host) return;
            tab.host.expValue = 0.0;
            tab.host.contrastValue = 0.0;
            tab.host.highlightsValue = 0.0;
            tab.host.shadowsValue = 0.0;
            tab.host.whitesValue = 0.0;
            tab.host.blacksValue = 0.0;
            tab.host.requestRender();
        }

        SliderGroup {
            title: qsTr("Exposure")
            from: -5.0
            to: 5.0
            value: tab.host ? tab.host.expValue : 0.0
            defaultValue: 0.0
            stepSize: 0.05
            decimals: 2
            suffix: " EV"
            onSliderMoved: function(v) { if (tab.host) { tab.host.expValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Contrast")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.contrastValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.contrastValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Highlights")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.highlightsValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.highlightsValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Shadows")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.shadowsValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.shadowsValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Whites")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.whitesValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.whitesValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Blacks")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.blacksValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.blacksValue = v; tab.host.requestRender(); } }
        }
    }

    PanelSection {
        key: "presence"
        title: qsTr("Presence")
        host: tab.host
        onReset: {
            if (!tab.host) return;
            tab.host.textureValue = 0.0;
            tab.host.clarityValue = 0.0;
            tab.host.dehazeValue = 0.0;
            tab.host.vibranceValue = 0.0;
            tab.host.saturationValue = 0.0;
            tab.host.requestRender();
        }

        SliderGroup {
            title: qsTr("Texture")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.textureValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.textureValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Clarity (Midtone Contrast)")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.clarityValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.clarityValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Dehaze")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.dehazeValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.dehazeValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Vibrance")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.vibranceValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.vibranceValue = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Saturation")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.saturationValue : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.saturationValue = v; tab.host.requestRender(); } }
        }
    }

    PanelSection {
        id: curveSection
        key: "curve"
        title: qsTr("Tone curve")
        host: tab.host
        visible: tab.host ? tab.host.isProMode : false
        pro: true
        onReset: {
            if (!tab.host) return;
            tab.host.pushUndoState();
            tab.host.curveHighlights = 0.0;
            tab.host.curveLights = 0.0;
            tab.host.curveDarks = 0.0;
            tab.host.curveShadows = 0.0;
            tab.host.curvePoints = [];
            tab.host.curvePointsRed = [];
            tab.host.curvePointsGreen = [];
            tab.host.curvePointsBlue = [];
            tab.host.requestRender();
        }

        CurveEditor {
            Layout.fillWidth: true
            pointsRgb: tab.host ? tab.host.curvePoints : []
            pointsRed: tab.host ? tab.host.curvePointsRed : []
            pointsGreen: tab.host ? tab.host.curvePointsGreen : []
            pointsBlue: tab.host ? tab.host.curvePointsBlue : []
            histData: tab.host ? tab.host.activeHistogram : null
            onPointsEdited: function(ch, pts) {
                if (!tab.host) return;
                if (ch === "red") tab.host.curvePointsRed = pts;
                else if (ch === "green") tab.host.curvePointsGreen = pts;
                else if (ch === "blue") tab.host.curvePointsBlue = pts;
                else tab.host.curvePoints = pts;
                tab.host.requestRender();
            }
        }

        SliderGroup {
            title: qsTr("Curve Highlights")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.curveHighlights : 0.0
            defaultValue: 0.0
            accentColor: Theme.accent
            onSliderMoved: function(v) { if (tab.host) { tab.host.curveHighlights = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Curve Lights (Upper Mids)")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.curveLights : 0.0
            defaultValue: 0.0
            accentColor: Theme.accentCyan
            onSliderMoved: function(v) { if (tab.host) { tab.host.curveLights = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Curve Darks (Lower Mids)")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.curveDarks : 0.0
            defaultValue: 0.0
            accentColor: Theme.accentYellow
            onSliderMoved: function(v) { if (tab.host) { tab.host.curveDarks = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Curve Shadows")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.curveShadows : 0.0
            defaultValue: 0.0
            accentColor: Theme.accentOrange
            onSliderMoved: function(v) { if (tab.host) { tab.host.curveShadows = v; tab.host.requestRender(); } }
        }
    }

    PanelSection {
        key: "hsl"
        title: qsTr("Colour mixer")
        host: tab.host
        visible: tab.host ? tab.host.isProMode : false
        pro: true
        onReset: hslMixer.resetAll()

        HslMixer {
            id: hslMixer
            Layout.fillWidth: true
            hslHue: tab.host ? tab.host.hslH : [0, 0, 0, 0, 0, 0, 0, 0]
            hslSat: tab.host ? tab.host.hslS : [0, 0, 0, 0, 0, 0, 0, 0]
            hslLum: tab.host ? tab.host.hslL : [0, 0, 0, 0, 0, 0, 0, 0]
            onHslValuesChanged: function(h, s, l) {
                if (!tab.host) return;
                tab.host.hslH = h;
                tab.host.hslS = s;
                tab.host.hslL = l;
            }
            onColorChanged: if (tab.host) tab.host.requestRender()
        }
    }

    PanelSection {
        key: "grading"
        title: qsTr("Colour grading")
        host: tab.host
        visible: tab.host ? tab.host.isProMode : false
        pro: true
        resettable: false

        ColorGradingWheels {
            embedded: true
            id: colorWheels
            Layout.fillWidth: true
            liftRgb: tab.host ? tab.host.liftRgb : [0.0, 0.0, 0.0]
            liftLuma: tab.host ? tab.host.liftLuma : 0.0
            gammaRgb: tab.host ? tab.host.gammaRgb : [0.0, 0.0, 0.0]
            gammaLuma: tab.host ? tab.host.gammaLuma : 0.0
            gainRgb: tab.host ? tab.host.gainRgb : [0.0, 0.0, 0.0]
            gainLuma: tab.host ? tab.host.gainLuma : 0.0
            offsetRgb: tab.host ? tab.host.offsetRgb : [0.0, 0.0, 0.0]
            offsetLuma: tab.host ? tab.host.offsetLuma : 0.0
            colorBoost: tab.host ? tab.host.colorBoost : 0.0
            midtoneDetail: tab.host ? tab.host.midtoneDetail : 0.0
            contrastPivot: tab.host ? tab.host.contrastPivot : 0.435
            onGradingChanged: function(patch) {
                if (tab.host) tab.host.applyGradingPatch(patch);
            }
        }
        RgbMixerPanel {
            embedded: true
            id: rgbMixerPanel
            Layout.fillWidth: true
            mixerEnabled: tab.host ? tab.host.rgbMixerEnabled : false
            monochrome: tab.host ? tab.host.rgbMixerMonochrome : false
            redInR: tab.host ? tab.host.mixerRedInR : 1.0
            greenInR: tab.host ? tab.host.mixerGreenInR : 0.0
            blueInR: tab.host ? tab.host.mixerBlueInR : 0.0
            redInG: tab.host ? tab.host.mixerRedInG : 0.0
            greenInG: tab.host ? tab.host.mixerGreenInG : 1.0
            blueInG: tab.host ? tab.host.mixerBlueInG : 0.0
            redInB: tab.host ? tab.host.mixerRedInB : 0.0
            greenInB: tab.host ? tab.host.mixerGreenInB : 0.0
            blueInB: tab.host ? tab.host.mixerBlueInB : 1.0
            onMixerChanged: function(patch) {
                if (tab.host) tab.host.applyRgbMixerPatch(patch);
            }
        }
        SkinTonePanel {
            embedded: true
            id: skinTonePanel
            Layout.fillWidth: true
            skinToneEnabled: tab.host ? tab.host.skinToneEnabled : false
            skinTargetHue: tab.host ? tab.host.skinTargetHue : 50.0
            skinHueRange: tab.host ? tab.host.skinHueRange : 32.0
            skinUniformityHue: tab.host ? tab.host.skinUniformityHue : 0.0
            skinUniformitySat: tab.host ? tab.host.skinUniformitySat : 0.0
            skinAmountHue: tab.host ? tab.host.skinAmountHue : 0.0
            skinAmountSat: tab.host ? tab.host.skinAmountSat : 0.0
            onSkinToneChanged: function(patch) {
                if (tab.host) tab.host.applySkinTonePatch(patch);
            }
        }
    }

    PanelSection {
        key: "detail"
        title: qsTr("Detail and optics")
        host: tab.host
        visible: tab.host ? tab.host.isProMode : false
        pro: true
        onReset: {
            if (!tab.host) return;
            tab.host.sharpnessVal = 25.0;
            tab.host.denoiseLumVal = 0.0;
            tab.host.denoiseColVal = 10.0;
            tab.host.vignetteVal = 0.0;
            tab.host.defringeVal = 0.0;
            tab.host.lensDistortionVal = 0.0;
            tab.host.requestRender();
        }

        SliderGroup {
            title: qsTr("Sharpening")
            from: 0.0
            to: 100.0
            value: tab.host ? tab.host.sharpnessVal : 25.0
            defaultValue: 25.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.sharpnessVal = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Noise Reduction (Luma)")
            from: 0.0
            to: 100.0
            value: tab.host ? tab.host.denoiseLumVal : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.denoiseLumVal = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Noise Reduction (Color)")
            from: 0.0
            to: 100.0
            value: tab.host ? tab.host.denoiseColVal : 10.0
            defaultValue: 10.0
            accentColor: Theme.accentYellow
            onSliderMoved: function(v) { if (tab.host) { tab.host.denoiseColVal = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Vignette")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.vignetteVal : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.vignetteVal = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Defringe (Chromatic Aberration)")
            from: 0.0
            to: 100.0
            value: tab.host ? tab.host.defringeVal : 0.0
            defaultValue: 0.0
            accentColor: Theme.accentMagenta
            onSliderMoved: function(v) { if (tab.host) { tab.host.defringeVal = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Lens Distortion Correction")
            from: -50.0
            to: 50.0
            value: tab.host ? tab.host.lensDistortionVal : 0.0
            defaultValue: 0.0
            accentColor: Theme.accentCyan
            onSliderMoved: function(v) { if (tab.host) { tab.host.lensDistortionVal = v; tab.host.requestRender(); } }
        }
    }

    PanelSection {
        key: "transform"
        title: qsTr("Transform")
        host: tab.host
        visible: tab.host ? tab.host.isProMode : false
        pro: true
        onReset: {
            if (!tab.host) return;
            tab.host.pushUndoState();
            tab.host.perspectiveVertical = 0.0;
            tab.host.perspectiveHorizontal = 0.0;
            tab.host.perspectiveScale = 100.0;
            tab.host.requestRender();
        }

        SliderGroup {
            title: qsTr("Vertical")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.perspectiveVertical : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.perspectiveVertical = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Horizontal")
            from: -100.0
            to: 100.0
            value: tab.host ? tab.host.perspectiveHorizontal : 0.0
            defaultValue: 0.0
            onSliderMoved: function(v) { if (tab.host) { tab.host.perspectiveHorizontal = v; tab.host.requestRender(); } }
        }
        SliderGroup {
            title: qsTr("Scale")
            from: 50.0
            to: 150.0
            value: tab.host ? tab.host.perspectiveScale : 100.0
            defaultValue: 100.0
            suffix: " %"
            onSliderMoved: function(v) { if (tab.host) { tab.host.perspectiveScale = v; tab.host.requestRender(); } }
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
        host: tab.host
        visible: tab.host ? tab.host.isProMode : false
        pro: true
        onReset: filmGrainPanel.reset()

        FilmGrainPanel {
            embedded: true
            id: filmGrainPanel
            Layout.fillWidth: true
            grainAmount: tab.host ? tab.host.grainAmount : 0.0
            grainSize: tab.host ? tab.host.grainSize : 1.0
            grainRoughness: tab.host ? tab.host.grainRoughness : 50.0
            onGrainChanged: function(amount, size, roughness) {
                if (!tab.host) return;
                tab.host.grainAmount = amount;
                tab.host.grainSize = size;
                tab.host.grainRoughness = roughness;
                tab.host.requestRender();
            }
        }
    }

    PanelSection {
        key: "colour"
        title: qsTr("Colour management")
        host: tab.host
        visible: tab.host ? tab.host.isProMode : false
        pro: true
        resettable: false

        ColorSpaceSelector {
            embedded: true
            id: colorSpaceSelector
            Layout.fillWidth: true
            gamutCompression: tab.host ? tab.host.gamutCompression : false
            acesTonemap: tab.host ? tab.host.activeAcesTonemap : false
            onGamutCompressionToggled: function(enabled) {
                if (!tab.host) return;
                tab.host.gamutCompression = enabled;
                tab.host.requestRender();
            }
            onAcesTonemapToggled: function(enabled) {
                if (!tab.host) return;
                tab.host.activeAcesTonemap = enabled;
                tab.host.requestRender();
            }
        }

        SoftProofPanel {
            Layout.fillWidth: true
            Layout.topMargin: 6
            active: tab.host ? tab.host.softProof : false
            profile: tab.host ? tab.host.proofProfile : "sRGB"
            intent: tab.host ? tab.host.proofIntent : "relative_colorimetric"
            gamutWarning: tab.host ? tab.host.gamutWarning : false
            profiles: tab.host ? tab.host.iccProfiles : []
            onActiveToggled: function(on) { if (tab.host) tab.host.softProof = on }
            onProfilePicked: function(id) { if (tab.host) tab.host.proofProfile = id }
            onIntentPicked: function(id) { if (tab.host) tab.host.proofIntent = id }
            onGamutWarningToggled: function(on) { if (tab.host) tab.host.gamutWarning = on }
        }
    }
}
