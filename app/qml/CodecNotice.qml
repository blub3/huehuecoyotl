import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #171: why a track has no picture or sound, and in the Flatpak the command that adds the
// codecs (selectable, to copy it). `message` may be empty when the text is shown elsewhere.
ColumnLayout {
    id: root
    property string message: ""
    property string command: ""
    spacing: 8
    Accessible.role: Accessible.AlertMessage
    Accessible.name: message + (command !== "" ? " " + command : "")

    Label {
        objectName: "codecMessage"
        Layout.fillWidth: true
        visible: root.message !== ""
        text: root.message
        color: Theme.text
        wrapMode: Text.Wrap
    }
    Label {
        Layout.fillWidth: true
        visible: root.command !== ""
        text: qsTr("Install the codec extension, then start the app again:")
        color: Theme.textDim
        font.pixelSize: Theme.fontSmall
        wrapMode: Text.Wrap
    }
    TextEdit {
        objectName: "codecCommand"
        Layout.fillWidth: true
        visible: root.command !== ""
        text: root.command
        readOnly: true
        selectByMouse: true
        activeFocusOnPress: true
        wrapMode: TextEdit.WrapAnywhere
        color: Theme.text
        selectionColor: Theme.accent
        font.family: "monospace"
        font.pixelSize: Theme.fontSmall
    }
}
