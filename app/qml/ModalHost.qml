import QtQuick

// Scrim overlays hosting Export, Cloud Accounts, and About modals.
Item {
    id: modals
    anchors.fill: parent
    z: 100

    property var host: null
    property var filmstrip: null

    readonly property alias exportDialog: exportDialog
    readonly property alias cloudDialog: cloudDialog

    // EXPORT MODAL OVERLAY
    Rectangle {
        visible: modals.host ? modals.host.showExportModal : false
        anchors.fill: parent
        color: Theme.scrim
        z: 100

        MouseArea {
            anchors.fill: parent
            onClicked: if (modals.host) modals.host.showExportModal = false
        }

        ExportDialog {
            id: exportDialog
            anchors.centerIn: parent
            host: modals.host
            iccProfiles: modals.host ? modals.host.iccProfiles : []
            watermarkEnabled: modals.host ? modals.host.watermarkEnabled : false
            watermarkOptions: modals.host ? modals.host.buildWatermarkOptions() : ({})
            cloudAccounts: modals.host ? modals.host.cloudAccounts : []
            uploadFolder: modals.host ? modals.host.cloudUploadFolder : "OmaStudio"
            uploadAccount: modals.host ? modals.host.cloudUploadAccount : ""
            onUploadAccountPicked: function(account) { if (modals.host) modals.host.cloudUploadAccount = account }
            onCloseRequested: if (modals.host) modals.host.showExportModal = false
            onDoExport: function(opts) { if (modals.host) modals.host.startExport(opts) }
        }
    }

    // CLOUD ACCOUNTS MODAL OVERLAY
    Rectangle {
        visible: modals.host ? modals.host.showCloudModal : false
        anchors.fill: parent
        color: Theme.scrim
        z: 100

        MouseArea {
            anchors.fill: parent
            onClicked: if (modals.host) modals.host.showCloudModal = false
        }

        CloudDialog {
            id: cloudDialog
            anchors.centerIn: parent
            visible: modals.host ? modals.host.showCloudModal : false
            providers: modals.host ? modals.host.cloudProviders : []
            accounts: modals.host ? modals.host.cloudAccounts : []
            uploadFolder: modals.host ? modals.host.cloudUploadFolder : "OmaStudio"
            onCloseRequested: if (modals.host) modals.host.showCloudModal = false
            onConnectRequested: function(provider, clientId, clientSecret) {
                if (modals.host) modals.host.connectCloudAccount(provider, clientId, clientSecret);
            }
            onCancelRequested: if (modals.host) modals.host.cancelCloudConnect()
            onBrowseRequested: function(account) {
                if (!modals.host) return;
                modals.host.showCloudModal = false;
                if (modals.filmstrip) modals.filmstrip.isCatalogMode = false;
                modals.host.openCloudFolder(account, "");
            }
            onDisconnectRequested: function(account) {
                if (modals.host) modals.host.disconnectCloudAccount(account);
            }
        }
    }

    // ABOUT / INFO MODAL OVERLAY
    Rectangle {
        visible: modals.host ? modals.host.showAboutModal : false
        anchors.fill: parent
        color: Theme.scrim
        z: 101

        MouseArea {
            anchors.fill: parent
            onClicked: if (modals.host) modals.host.showAboutModal = false
        }

        AboutDialog {
            id: aboutDialog
            anchors.centerIn: parent
            onCloseRequested: if (modals.host) modals.host.showAboutModal = false
        }
    }
}
