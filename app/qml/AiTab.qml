import QtQuick
import QtQuick.Layouts

// AI tab of the right inspector: local & remote edit assistant.
ColumnLayout {
    id: tab

    property var host: null

    readonly property alias aiAssist: aiAssist

    visible: tab.host ? tab.host.rightTab === "ai" : false
    Layout.fillWidth: true
    spacing: 0

    PanelSection {
        key: "assistant"
        title: qsTr("Edit assistant")
        host: tab.host
        resettable: false

        AiAssistPanel {
            id: aiAssist
            embedded: true
            Layout.fillWidth: true
            enginePath: tab.host ? tab.host.resolveEnginePath() : ""
            activePhotoPath: tab.host ? tab.host.activePhotoPath : ""
            recipeProvider: function() { return tab.host ? tab.host.buildRecipeObject() : ({}); }
            onRecipeSuggested: function(recipe, explanation) {
                if (!tab.host) return;
                tab.host.pushUndoState();
                tab.host.patchRecipe(recipe);
                tab.host.showToast(qsTr("Assistant edit applied"), Theme.accentGreen);
            }
            onNotify: function(text, isError) {
                if (tab.host) tab.host.showToast(text, isError ? Theme.accentMagenta : Theme.accentGreen);
            }
            onLocalConnectedNow: if (tab.host) tab.host.aiChatOpen = true
        }
    }
}
