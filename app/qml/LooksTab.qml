import QtQuick
import QtQuick.Layouts

// Looks tab of the right inspector: built-in presets and 3D LUT loader.
ColumnLayout {
    id: tab

    property var host: null

    readonly property alias presetSel: presetSel

    visible: tab.host ? tab.host.rightTab === "looks" : false
    Layout.fillWidth: true
    spacing: 0

    PanelSection {
        key: "looks"
        title: qsTr("Presets")
        host: tab.host
        resettable: false

        PresetSelector {
            embedded: true
            id: presetSel
            Layout.fillWidth: true
            onApplyPreset: function(n) { if (tab.host) tab.host.applyPresetNamed(n) }
            onTriggerAiAuto: if (tab.host) tab.host.triggerAiAuto()
            onResetPreset: if (tab.host) tab.host.resetRecipe()
        }
    }

    PanelSection {
        key: "lut"
        title: qsTr("LUT")
        host: tab.host
        onReset: lutSelector.reset()

        LutSelector {
            embedded: true
            id: lutSelector
            Layout.fillWidth: true
            activeLut: tab.host ? tab.host.activeLutName : ""
            lutIntensity: tab.host ? tab.host.activeLutIntensity : 1.0
            customLutPath: tab.host ? tab.host.activeLutPath : ""
            onLutChanged: function(name, intensity, path) {
                if (!tab.host) return;
                tab.host.activeLutName = name;
                tab.host.activeLutIntensity = intensity;
                tab.host.activeLutPath = path;
                tab.host.requestRender();
            }
        }
    }
}
