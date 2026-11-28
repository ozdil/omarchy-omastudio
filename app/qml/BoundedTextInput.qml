import QtQuick
import QtQuick.Controls
import OmaStudio.Host

TextEdit {
    font.family: Theme.fontFamily
    id: control

    property string placeholderText: ""
    property color placeholderTextColor: Theme.textDim
    property int maxLength: 300
    property int maxPasteBytes: 4096
    property string pasteNotice: ""
    property bool multiline: false

    signal accepted()

    wrapMode: multiline ? TextEdit.Wrap : TextEdit.NoWrap
    textFormat: TextEdit.PlainText
    selectByMouse: true
    clip: true

    function insertCleanText(raw) {
        if (readOnly) return;
        var clean = String(raw || "").replace(/\u0000/g, "");
        if (!multiline) {
            clean = clean.replace(/[\r\n\t\v\f]/g, " ");
        }
        var start = Math.min(selectionStart, selectionEnd);
        var end = Math.max(selectionStart, selectionEnd);
        var room = maxLength > 0 ? Math.max(0, maxLength - (length - (end - start))) : clean.length;
        if (clean.length > room) {
            clean = clean.substring(0, room);
        }
        if (clean.length === 0 && start === end) return;
        if (end > start) {
            remove(start, end);
        }
        if (clean.length > 0) {
            insert(start, clean);
            cursorPosition = start + clean.length;
        }
        // TextEdit only has textEdited() from Qt 6.9 on; on older Qt the call would
        // throw after the paste was inserted, so emit it only where it exists.
        if (typeof control.textEdited === "function")
            control.textEdited();
    }

    function startBoundedPaste() {
        if (readOnly) return;
        control.pasteNotice = "";
        var rawJson = Host.clipboardTextBounded(Math.max(1, control.maxPasteBytes));
        try {
            var res = JSON.parse(rawJson);
            if (res.exceeded) {
                control.pasteNotice = qsTr("Clipboard content exceeded byte limit or timed out.");
                return;
            }
            if (!res.text || res.text.length === 0) return;
            control.insertCleanText(res.text);
        } catch (e) {
            return;
        }
    }

    Keys.onShortcutOverride: (event) => {
        if (control.readOnly || event.key === Qt.Key_Escape) return;
        var ctrl = (event.modifiers & Qt.ControlModifier) !== 0;
        var alt = (event.modifiers & Qt.AltModifier) !== 0;
        var meta = (event.modifiers & Qt.MetaModifier) !== 0;
        if (!ctrl && !alt && !meta && (event.text.length > 0
            || event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete
            || event.key === Qt.Key_Left || event.key === Qt.Key_Right
            || event.key === Qt.Key_Up || event.key === Qt.Key_Down
            || event.key === Qt.Key_Home || event.key === Qt.Key_End)) {
            event.accepted = true;
            return;
        }
        if (event.matches(StandardKey.Undo) || event.matches(StandardKey.Redo)
            || event.matches(StandardKey.Copy) || event.matches(StandardKey.Cut)
            || event.matches(StandardKey.Paste) || event.matches(StandardKey.SelectAll)) {
            event.accepted = true;
        }
    }

    Keys.onPressed: (event) => {
        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            if (!control.multiline) {
                event.accepted = true;
                control.accepted();
                return;
            }
        }
        if (event.matches(StandardKey.Paste) ||
            (event.modifiers & Qt.ControlModifier && (event.key === Qt.Key_V || event.key === Qt.Key_Insert)) ||
            (event.modifiers & Qt.ShiftModifier && event.key === Qt.Key_Insert)) {
            event.accepted = true;
            control.startBoundedPaste();
            return;
        }
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.MiddleButton
        onPressed: (mouse) => {
            mouse.accepted = true;
        }
    }

    onTextChanged: {
        control.pasteNotice = "";
        if (maxLength > 0 && text.length > maxLength) {
            var pos = cursorPosition;
            text = text.slice(0, maxLength);
            cursorPosition = Math.min(pos, maxLength);
        }
    }

    Text {
        anchors.fill: parent
        anchors.leftMargin: control.leftPadding
        anchors.rightMargin: control.rightPadding
        anchors.topMargin: control.topPadding
        anchors.bottomMargin: control.bottomPadding
        text: control.placeholderText
        font: control.font
        color: control.placeholderTextColor
        visible: control.text.length === 0 && !control.activeFocus
        verticalAlignment: control.multiline ? Text.AlignTop : control.verticalAlignment
        elide: Text.ElideRight
        textFormat: Text.PlainText
    }
}
