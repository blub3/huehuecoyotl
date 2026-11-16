pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The TMDB key's state in Settings (issue #52): whether a key is stored and where it comes from,
// when TMDB last accepted it, and Replace, Test and Remove. The key itself is never shown.
// States (Core.tmdbKeyState): keystore, environment, session (in use, but the keystore could not
// keep it), none, unavailable (no keystore), "" (still looking).
ColumnLayout {
    id: status
    property string keyState: ""
    property var validatedAt: null
    property bool busy: false
    signal replaceRequested()
    signal testRequested()
    signal removeRequested()
    // Left on the first button: the page moves focus to the navigation rail.
    signal leftEdge()

    readonly property bool hasKey: keyState === "keystore" || keyState === "environment" || keyState === "session"
    readonly property bool validated: validatedAt instanceof Date && !isNaN(validatedAt.getTime())
    // The first button, for the page's keyboard navigation (null without a key).
    readonly property Item firstButton: hasKey ? (replaceButton.visible ? replaceButton : testButton) : null
    property Item downTarget: null

    function headline() {
        switch (keyState) {
        case "keystore": return qsTr("Key stored")
        case "environment": return qsTr("Key set for development")
        case "session": return qsTr("Key in use until the app closes")
        case "unavailable": return qsTr("No system keystore available")
        case "": return qsTr("Looking for the stored key…")
        }
        return ""
    }
    function detail() {
        switch (keyState) {
        case "keystore": return qsTr("Kept in the system keystore. Posters and descriptions are downloaded automatically.")
        case "environment": return qsTr("Comes from the COYOTL_TMDB_KEY environment variable and is not stored. This is meant for development.")
        case "session": return qsTr("The system keystore could not keep the key, so it has to be entered again after a restart. Install or unlock a keyring (for example GNOME Keyring or KWallet), then enter the key again.")
        case "unavailable": return qsTr("Huehuecoyotl keeps the TMDB key only in the system keystore, and none can be reached. Install or unlock a keyring (for example GNOME Keyring or KWallet) and restart the app. Until then the app plays everything and shows titles from the file names; a key entered now works until the app closes.")
        }
        return ""
    }
    function checkedText() {
        if (!hasKey) return ""
        return validated ? qsTr("Last accepted by TMDB: %1").arg(validatedAt.toLocaleString(Qt.locale(), Locale.ShortFormat))
                         : qsTr("Not checked with TMDB yet.")
    }

    visible: keyState !== "none"
    spacing: 8

    Label {
        objectName: "keyHeadline"
        text: status.headline()
        color: status.keyState === "unavailable" || status.keyState === "session" ? Theme.danger : Theme.text
        font.pixelSize: Theme.fontBody
        font.bold: true
    }
    Label {
        objectName: "keyDetail"
        Layout.fillWidth: true
        visible: text !== ""
        text: status.detail()
        color: Theme.textDim
        font.pixelSize: Theme.fontSmall
        wrapMode: Text.Wrap
    }
    Label {
        objectName: "keyChecked"
        visible: text !== ""
        text: status.checkedText()
        color: Theme.textDim
        font.pixelSize: Theme.fontSmall
    }
    RowLayout {
        visible: status.hasKey
        spacing: 10
        FocusButton {
            id: replaceButton
            objectName: "replaceKey"
            visible: status.keyState !== "environment"
            text: qsTr("Replace key")
            KeyNavigation.right: testButton
            KeyNavigation.down: status.downTarget
            Keys.onLeftPressed: status.leftEdge()
            onClicked: status.replaceRequested()
        }
        FocusButton {
            id: testButton
            objectName: "testKey"
            // Stays enabled while checking: a disabled button would drop keyboard focus.
            text: status.busy ? qsTr("Checking…") : qsTr("Test key")
            KeyNavigation.left: replaceButton.visible ? replaceButton : null
            KeyNavigation.right: removeButton.visible ? removeButton : null
            KeyNavigation.down: status.downTarget
            Keys.onLeftPressed: (e) => { if (replaceButton.visible) replaceButton.forceActiveFocus(); else status.leftEdge() }
            onClicked: if (!status.busy) status.testRequested()
        }
        FocusButton {
            id: removeButton
            objectName: "removeKey"
            visible: status.keyState !== "environment"
            text: qsTr("Remove key")
            KeyNavigation.left: testButton
            KeyNavigation.down: status.downTarget
            onClicked: status.removeRequested()
        }
    }
}
