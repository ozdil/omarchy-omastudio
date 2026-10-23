import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

// Cloud accounts: the connected ones (browse, disconnect) and the services that can
// be added. Signing in happens in the browser; the main window runs the engine
// (`omastudio-engine cloud ...`) and feeds the sign-in steps back through
// `signInUrl` and `deviceCode`.
Rectangle {
    id: root
    width: Math.min(Theme.px(560), parent ? parent.width - 32 : Theme.px(560))
    height: Math.min(content.implicitHeight + 2 * pad, parent ? parent.height - 32 : content.implicitHeight + 2 * pad)
    radius: Theme.radiusLg
    color: Theme.bgSurface
    border.color: Theme.borderLight
    border.width: 1
    clip: true

    readonly property int pad: Theme.px(20)

    // From `cloud status`: [{ id, label, missing, package }]
    property var providers: []
    // [{ id, provider, label, user }]
    property var accounts: []
    property string uploadFolder: "OmaStudio"
    // Sign-in in progress: the service's label, the page to open again, GitHub's code
    property string connectingLabel: ""
    property string signInUrl: ""
    property string deviceCode: ""
    property string errorText: ""
    property string confirmDisconnect: ""
    property bool showGoogleClient: false

    signal closeRequested()
    signal connectRequested(string provider, string clientId, string clientSecret)
    signal cancelRequested()
    signal browseRequested(string account)
    signal disconnectRequested(string account)

    focus: visible
    onVisibleChanged: if (visible) { root.forceActiveFocus(); root.confirmDisconnect = ""; }
    Keys.onEscapePressed: root.closeRequested()

    // Swallow clicks on the dialog body so they don't reach the overlay's close handler
    MouseArea {
        anchors.fill: parent
        z: -1
    }

    component Caption: Text {
        font.family: Theme.fontFamily
        textFormat: Text.PlainText
        font.pixelSize: Theme.px(11)
        font.weight: Font.DemiBold
        color: Theme.textMuted
    }

    component Hint: Text {
        font.family: Theme.fontFamily
        textFormat: Text.PlainText
        font.pixelSize: Theme.px(10)
        color: Theme.textDim
        wrapMode: Text.WordWrap
        Layout.fillWidth: true
    }

    component SmallButton: Rectangle {
        id: btn
        property string label
        property bool primary: false
        property bool enabledButton: true
        signal clicked()
        implicitWidth: btnText.implicitWidth + Theme.px(20)
        implicitHeight: Theme.px(26)
        radius: Theme.radiusSm
        opacity: enabledButton ? 1 : 0.5
        color: primary ? (btnMouse.containsMouse ? Qt.lighter(Theme.accent, 1.1) : Theme.accent)
                       : (btnMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
        border.color: primary ? Theme.accent : Theme.border
        Text {
            id: btnText
            anchors.centerIn: parent
            text: btn.label
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(11)
            font.weight: btn.primary ? Font.DemiBold : Font.Normal
            color: btn.primary ? Theme.onAccent : Theme.textMain
        }
        MouseArea {
            id: btnMouse
            anchors.fill: parent
            hoverEnabled: true
            enabled: btn.enabledButton
            cursorShape: Qt.PointingHandCursor
            onClicked: btn.clicked()
        }
    }

    component Field: Rectangle {
        property alias text: input.text
        property alias placeholderText: input.placeholderText
        Layout.fillWidth: true
        implicitHeight: Theme.px(28)
        radius: Theme.radiusSm
        color: Theme.bgCard
        border.color: input.activeFocus ? Theme.accent : Theme.border
        BoundedTextInput {
            id: input
            anchors.fill: parent
            anchors.leftMargin: Theme.px(8)
            anchors.rightMargin: Theme.px(8)
            verticalAlignment: TextEdit.AlignVCenter
            font.pixelSize: Theme.px(11)
            color: Theme.textMain
            maxLength: 256
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        anchors.margins: root.pad
        contentHeight: content.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar { policy: flick.contentHeight > flick.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }

        ColumnLayout {
            id: content
            width: flick.width
            spacing: Theme.px(12)

            // Header
            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.px(8)
                Text {
                    text: Theme.iconCloud
                    font.family: Theme.iconFont
                    font.pixelSize: Theme.px(14)
                    color: Theme.accentCyan
                }
                Text {
                    text: qsTr("Cloud accounts")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(15)
                    font.weight: Font.DemiBold
                    color: Theme.textMain
                    Layout.fillWidth: true
                }
                Rectangle {
                    implicitWidth: Theme.px(26)
                    implicitHeight: Theme.px(26)
                    radius: Theme.radiusSm
                    color: closeMouse.containsMouse ? Theme.bgCardHover : "transparent"
                    Text {
                        anchors.centerIn: parent
                        text: Theme.iconTimes
                        font.family: Theme.iconFont
                        color: Theme.textMuted
                        font.pixelSize: Theme.px(11)
                    }
                    MouseArea {
                        id: closeMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.closeRequested()
                        Accessible.role: Accessible.Button
                        Accessible.name: qsTr("Close")
                    }
                }
            }

            Hint {
                text: qsTr("You sign in on the service's own page in your browser. OmaStudio keeps the access keys in the system keyring and creates a folder named %1 in each account for uploads.").arg(root.uploadFolder)
            }

            // Sign-in in progress
            Rectangle {
                visible: root.connectingLabel !== ""
                Layout.fillWidth: true
                implicitHeight: signInCol.implicitHeight + Theme.px(20)
                radius: Theme.radiusSm
                color: Qt.rgba(Theme.accentCyan.r, Theme.accentCyan.g, Theme.accentCyan.b, 0.12)
                border.color: Theme.accentCyan

                ColumnLayout {
                    id: signInCol
                    anchors.fill: parent
                    anchors.margins: Theme.px(10)
                    spacing: Theme.px(8)

                    Text {
                        text: qsTr("Signing in to %1").arg(root.connectingLabel)
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(12)
                        font.weight: Font.DemiBold
                        color: Theme.textMain
                    }
                    Hint {
                        text: root.deviceCode !== ""
                            ? qsTr("Enter this code on the GitHub page that opened in your browser:")
                            : qsTr("Finish signing in in your browser. This window updates when you are done.")
                    }
                    Text {
                        visible: root.deviceCode !== ""
                        text: root.deviceCode
                        textFormat: Text.PlainText
                        font.family: Theme.monoFont
                        font.pixelSize: Theme.px(20)
                        font.weight: Font.Bold
                        color: Theme.accent
                    }
                    RowLayout {
                        spacing: Theme.px(8)
                        SmallButton {
                            visible: root.signInUrl !== ""
                            label: qsTr("Open the page again")
                            onClicked: Qt.openUrlExternally(root.signInUrl)
                        }
                        SmallButton {
                            label: qsTr("Cancel")
                            onClicked: root.cancelRequested()
                        }
                    }
                }
            }

            Text {
                visible: root.errorText !== ""
                text: root.errorText
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(11)
                color: Theme.highlightClip
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }

            // Connected accounts
            Caption {
                text: qsTr("Connected")
            }
            Hint {
                visible: root.accounts.length === 0
                text: qsTr("No account connected yet.")
            }
            Repeater {
                model: root.accounts
                delegate: Rectangle {
                    required property var modelData
                    Layout.fillWidth: true
                    implicitHeight: Theme.px(40)
                    radius: Theme.radiusSm
                    color: Theme.bgCard
                    border.color: Theme.border

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: Theme.px(10)
                        anchors.rightMargin: Theme.px(8)
                        spacing: Theme.px(8)
                        Text {
                            text: Theme.iconCloud
                            font.family: Theme.iconFont
                            font.pixelSize: Theme.px(12)
                            color: Theme.accentCyan
                        }
                        ColumnLayout {
                            spacing: 0
                            Layout.fillWidth: true
                            Text {
                                text: modelData.label
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.px(11)
                                font.weight: Font.DemiBold
                                color: Theme.textMain
                            }
                            Text {
                                visible: modelData.user !== ""
                                text: modelData.user
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: Theme.px(9)
                                color: Theme.textDim
                                elide: Text.ElideMiddle
                                Layout.fillWidth: true
                            }
                        }
                        SmallButton {
                            label: qsTr("Browse")
                            primary: true
                            onClicked: root.browseRequested(modelData.id)
                        }
                        SmallButton {
                            label: root.confirmDisconnect === modelData.id ? qsTr("Confirm") : qsTr("Disconnect")
                            onClicked: {
                                if (root.confirmDisconnect === modelData.id) {
                                    root.confirmDisconnect = "";
                                    root.disconnectRequested(modelData.id);
                                } else {
                                    root.confirmDisconnect = modelData.id;
                                }
                            }
                        }
                    }
                }
            }
            Hint {
                visible: root.confirmDisconnect !== ""
                text: qsTr("Disconnecting removes the account's keys and downloaded copies from this computer. Files in the account stay as they are.")
            }

            // Services that can be added
            Caption {
                text: qsTr("Add an account")
            }
            GridLayout {
                Layout.fillWidth: true
                columns: 3
                columnSpacing: Theme.px(6)
                rowSpacing: Theme.px(6)
                Repeater {
                    model: root.providers
                    delegate: Rectangle {
                        id: provCard
                        required property var modelData
                        readonly property bool available: !modelData.missing
                        readonly property bool usable: available && root.connectingLabel === ""
                        Layout.fillWidth: true
                        implicitHeight: Theme.px(44)
                        radius: Theme.radiusSm
                        opacity: available ? 1 : 0.6
                        color: provMouse.containsMouse && usable ? Theme.bgCardHover : Theme.bgCard
                        border.color: provMouse.containsMouse && usable ? Theme.accent : Theme.border

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: 1
                            Text {
                                text: modelData.label
                                textFormat: Text.PlainText
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.px(11)
                                font.weight: Font.DemiBold
                                color: Theme.textMain
                                Layout.alignment: Qt.AlignHCenter
                            }
                            Text {
                                visible: !provCard.available
                                text: qsTr("Needs %1").arg(modelData.package || modelData.missing || "")
                                textFormat: Text.PlainText
                                font.family: Theme.monoFont
                                font.pixelSize: Theme.px(8)
                                color: Theme.textDim
                                Layout.alignment: Qt.AlignHCenter
                            }
                        }
                        MouseArea {
                            id: provMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            enabled: provCard.usable
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                var google = modelData.id === "google-drive";
                                root.connectRequested(modelData.id,
                                                      google ? googleId.text.trim() : "",
                                                      google ? googleSecret.text.trim() : "");
                            }
                        }
                        ToolTip.visible: provMouse.containsMouse && !available
                        ToolTip.delay: 300
                        ToolTip.text: qsTr("Install it with: sudo pacman -S %1").arg(modelData.package || "")
                    }
                }
            }

            // Google Drive: own OAuth client
            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.px(6)
                Text {
                    text: root.showGoogleClient ? "▾" : "▸"
                    font.pixelSize: Theme.px(10)
                    color: Theme.textMuted
                }
                Text {
                    text: qsTr("Own Google client ID (optional)")
                    textFormat: Text.PlainText
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.px(10)
                    color: Theme.textMuted
                    Layout.fillWidth: true
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.showGoogleClient = !root.showGoogleClient
                    }
                }
            }
            ColumnLayout {
                visible: root.showGoogleClient
                Layout.fillWidth: true
                spacing: Theme.px(6)
                Hint {
                    text: qsTr("The shared client rclone uses for Google Drive is being retired. With a client ID of your own from the Google Cloud Console, Google Drive keeps working. It is used the next time you connect Google Drive.")
                }
                Field {
                    id: googleId
                    placeholderText: qsTr("Client ID")
                }
                Field {
                    id: googleSecret
                    placeholderText: qsTr("Client secret")
                }
            }
        }
    }
}
