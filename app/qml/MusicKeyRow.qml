import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// One optional music provider's key in Settings → Metadata (issue #12): its state, a field to
// enter a key (checked with the service before it is kept), and Remove. The key itself is never
// shown or sent back. Up and down leave the row; left and right move inside it.
ColumnLayout {
    id: row
    property string name: ""
    // none | keystore | session | rejected
    property string keyState: "none"
    property string message: ""
    property bool busy: false
    property string hint: ""
    property string link: ""
    signal saveRequested(string key)
    signal removeRequested()
    signal upRequested()
    signal downRequested()
    readonly property bool hasKey: keyState === "keystore" || keyState === "session"
    readonly property Item firstItem: field
    function clear() { field.text = "" }

    spacing: 6
    RowLayout {
        spacing: 10
        Layout.fillWidth: true
        Label { text: row.name; color: Theme.text; font.pixelSize: Theme.fontBody; Layout.preferredWidth: 130 }
        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: row.keyState === "rejected" ? Theme.danger : Theme.textDim
            font.pixelSize: Theme.fontSmall
            text: row.keyState === "keystore" ? qsTr("Key stored. Artist images and more are looked up.")
                : row.keyState === "session" ? qsTr("Key in use until the app closes (no keystore to keep it).")
                : row.keyState === "rejected" ? qsTr("The service no longer accepts the stored key. Enter a new one.")
                : qsTr("Off: no key. Nothing is sent to %1.").arg(row.name)
        }
    }
    RowLayout {
        spacing: 10
        Layout.fillWidth: true
        TextField {
            id: field
            Layout.fillWidth: true
            placeholderText: row.hasKey ? qsTr("Enter a new key to replace the stored one") : qsTr("Your %1 API key").arg(row.name)
            echoMode: TextInput.Password
            color: Theme.text
            placeholderTextColor: Theme.textDim
            Accessible.name: qsTr("%1 API key").arg(row.name)
            Accessible.description: row.hint
            KeyNavigation.right: save
            Keys.onUpPressed: row.upRequested()
            Keys.onDownPressed: row.downRequested()
            Keys.onLeftPressed: (e) => { if (cursorPosition === 0) Nav.focusRail(); else e.accepted = false }
            onAccepted: if (save.enabled) save.clicked()
            background: Rectangle {
                implicitHeight: 44
                radius: Theme.radius
                color: Theme.surface
                border.width: field.activeFocus ? Theme.focusWidth : 1
                border.color: field.activeFocus ? Theme.focus : Theme.surfaceHi
            }
        }
        FocusButton {
            id: save
            text: row.busy ? qsTr("Checking…") : qsTr("Check and save")
            enabled: !row.busy && field.text.trim().length >= 3
            primary: true
            KeyNavigation.left: field
            KeyNavigation.right: remove.visible ? remove : null
            Keys.onUpPressed: row.upRequested()
            Keys.onDownPressed: row.downRequested()
            onClicked: row.saveRequested(field.text)
        }
        FocusButton {
            id: remove
            property bool armed: false
            visible: row.hasKey || row.keyState === "rejected"
            text: armed ? qsTr("Really remove?") : qsTr("Remove key")
            enabled: !row.busy
            KeyNavigation.left: save
            Keys.onUpPressed: row.upRequested()
            Keys.onDownPressed: row.downRequested()
            onActiveFocusChanged: if (!activeFocus) armed = false
            onClicked: { if (armed) { armed = false; row.removeRequested() } else armed = true }
        }
    }
    Label {
        Layout.fillWidth: true
        visible: row.hint !== "" && !row.hasKey
        wrapMode: Text.Wrap
        color: Theme.textDim
        font.pixelSize: Theme.fontSmall
        text: row.hint
    }
    Label {
        visible: row.link !== "" && !row.hasKey
        text: row.link
        color: Theme.accent
        font.underline: true
        font.pixelSize: Theme.fontSmall
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Qt.openUrlExternally(parent.text) }
    }
    Label {
        Layout.fillWidth: true
        visible: row.message !== ""
        text: row.message
        color: Theme.danger
        wrapMode: Text.Wrap
    }
}
