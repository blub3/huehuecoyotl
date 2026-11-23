pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #148: how an artist is named. MusicBrainz's spelling (the default for a matched artist),
// the spelling of the files, kept through every scan and match, or the user's own name. Only
// the name in the app changes, never the files. `backend` is the core (or a test stub):
// setArtistName(artist, choice, own), answering with musicDetailReady ("artist") or
// requestFailed. Focus goes back to `from` on close.
Popup {
    id: dialog
    property var backend: null
    property var artistId: 0
    // The artist's `names` from its page: shown, source, files, musicbrainz, choice, own.
    property var names: ({})
    property Item from: null
    property string choice: "auto"
    property string error: ""
    property var pending: 0
    signal saved(var detail)

    readonly property bool matched: (names.musicbrainz || "") !== ""

    function show(artist, artistNames, item) {
        artistId = artist
        names = artistNames || {}
        from = item
        choice = names.choice === "files" || names.choice === "own" ? names.choice : (matched ? "auto" : "files")
        ownField.text = names.own || names.shown || ""
        error = ""
        open()
    }
    function save() {
        if (!backend) return
        if (choice === "own" && ownField.text.trim() === "") { error = qsTr("Please enter a name."); return }
        error = ""
        pending = backend.setArtistName(artistId, choice, choice === "own" ? ownField.text.trim() : "")
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(560, (parent ? parent.width : 800) - 80)
    modal: true
    focus: true
    padding: 24
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onOpened: (choice === "own" ? ownChoice : choice === "files" ? filesChoice : mbChoice).forceActiveFocus()
    onClosed: if (from) from.forceActiveFocus()

    Connections {
        target: dialog.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId !== dialog.pending) return
            dialog.pending = 0
            dialog.saved(detail)
            dialog.close()
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId !== dialog.pending) return
            dialog.pending = 0
            dialog.error = message
        }
    }

    component Choice: RadioButton {
        id: option
        property string hint: ""
        padding: 8
        Keys.onReturnPressed: dialog.save()
        Keys.onEnterPressed: dialog.save()
        background: Rectangle {
            radius: 10
            color: option.checked ? Theme.surfaceHi : "transparent"
            border.width: option.activeFocus ? Theme.focusWidth : 0
            border.color: Theme.focus
        }
        contentItem: ColumnLayout {
            spacing: 2
            Label { leftPadding: option.indicator.width + 10; text: option.text; color: Theme.text; font.bold: option.checked; wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { leftPadding: option.indicator.width + 10; visible: option.hint !== ""; text: option.hint; color: Theme.textDim; font.pixelSize: Theme.fontSmall; wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
    }

    contentItem: ColumnLayout {
        spacing: 10
        Keys.onPressed: (e) => { if (e.key === Qt.Key_Back) { dialog.close(); e.accepted = true } }
        Label {
            text: qsTr("Name of this artist")
            color: Theme.text
            font.pixelSize: Theme.fontLarge
            font.bold: true
            Accessible.role: Accessible.Heading
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("Only the name in Huehuecoyotl changes; your files stay as they are.")
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
        }
        Choice {
            id: mbChoice
            objectName: "mbChoice"
            Layout.fillWidth: true
            visible: dialog.matched
            checked: dialog.choice === "auto"
            text: qsTr("MusicBrainz's spelling: %1").arg(dialog.names.musicbrainz || "")
            hint: qsTr("Recommended. Follows MusicBrainz if it ever corrects the name.")
            KeyNavigation.down: filesChoice
            onToggled: if (checked) dialog.choice = "auto"
        }
        Choice {
            id: filesChoice
            objectName: "filesChoice"
            Layout.fillWidth: true
            checked: dialog.choice === "files"
            text: qsTr("As in my files: %1").arg(dialog.names.files || "")
            hint: qsTr("Kept through every scan and match.")
            KeyNavigation.up: mbChoice.visible ? mbChoice : null
            KeyNavigation.down: ownChoice
            onToggled: if (checked) dialog.choice = "files"
        }
        Choice {
            id: ownChoice
            objectName: "ownChoice"
            Layout.fillWidth: true
            checked: dialog.choice === "own"
            text: qsTr("My own name")
            KeyNavigation.up: filesChoice
            KeyNavigation.down: ownField
            onToggled: if (checked) { dialog.choice = "own"; ownField.forceActiveFocus(); ownField.selectAll() }
        }
        TextField {
            id: ownField
            objectName: "ownField"
            Layout.fillWidth: true
            Layout.leftMargin: 36
            enabled: dialog.choice === "own"
            Accessible.name: qsTr("My own name")
            KeyNavigation.up: ownChoice
            KeyNavigation.down: saveButton
            onAccepted: dialog.save()
        }
        Label {
            objectName: "nameError"
            visible: dialog.error !== ""
            text: dialog.error
            color: Theme.focus
            font.pixelSize: Theme.fontSmall
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 12
            FocusButton {
                id: cancelButton
                text: qsTr("Cancel")
                KeyNavigation.right: saveButton
                onClicked: dialog.close()
            }
            FocusButton {
                id: saveButton
                objectName: "saveName"
                primary: true
                text: qsTr("Save")
                enabled: dialog.pending === 0
                KeyNavigation.left: cancelButton
                KeyNavigation.up: ownField
                onClicked: dialog.save()
            }
        }
    }
}
