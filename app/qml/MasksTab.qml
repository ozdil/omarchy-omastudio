import QtQuick
import QtQuick.Layouts

// Masks tab of the right inspector: Heal & Clone retouching and Local Adjustment Layers.
ColumnLayout {
    id: tab

    property var host: null

    readonly property alias retouchPanel: retouchPanel
    readonly property alias layersPanel: layersPanel

    visible: tab.host ? tab.host.rightTab === "masks" : false
    Layout.fillWidth: true
    spacing: 0

    PanelSection {
        key: "retouch"
        title: qsTr("Heal and clone")
        host: tab.host
        onReset: {
            if (!tab.host) return;
            tab.host.pushUndoState();
            tab.host.retouchSpots = [];
            retouchPanel.selected = -1;
            tab.host.requestRender();
        }

        RetouchPanel {
            id: retouchPanel
            Layout.fillWidth: true
            spots: tab.host ? tab.host.retouchSpots : []
            enabledForPhoto: tab.host ? tab.host.activePhotoPath !== "" : false
            onSpotsEdited: function(spots) {
                if (!tab.host) return;
                tab.host.retouchSpots = spots;
                tab.host.requestRender();
            }
        }
    }

    PanelSection {
        key: "layers"
        title: qsTr("Local adjustments")
        host: tab.host
        resettable: false

        LayersPanel {
            embedded: true
            id: layersPanel
            Layout.fillWidth: true
            layersList: tab.host ? tab.host.adjustmentLayers : []
            enginePath: tab.host ? tab.host.resolveEnginePath() : ""
            activePhotoPath: tab.host ? tab.host.activePhotoPath : ""
            onNotify: function(text, isError) {
                if (tab.host) tab.host.showToast(text, isError ? Theme.accentMagenta : Theme.accentGreen);
            }
            onLayersChanged: function(nextLayers) {
                if (!tab.host) return;
                tab.host.adjustmentLayers = nextLayers;
                tab.host.requestRender();
            }
        }
    }
}
