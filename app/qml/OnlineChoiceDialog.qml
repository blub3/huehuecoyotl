pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #165: before the first online lookup for music or audiobooks, what would be sent to whom,
// and the choice between that and the files alone. Nothing is sent until it is answered; the
// scan of the files goes on behind it. `backend` is the core or a test stub: onlineChoice()
// answers with musicDetailReady("onlineChoice", {due}), musicOnline stores the choice.
Popup {
    id: dialog
    property var backend: null
    property var pending: 0

    function check() { if (backend && !opened) pending = backend.onlineChoice() }
    function choose(on) {
        backend.musicOnline = on
        close()
    }
    Component.onCompleted: check()

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(680, (parent ? parent.width : 800) - 80)
    modal: true
    focus: true
    padding: 28
    closePolicy: Popup.NoAutoClose
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onOpened: onlineButton.forceActiveFocus()

    Connections {
        target: dialog.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === dialog.pending && kind === "onlineChoice" && detail.due) dialog.open()
        }
        function onLibraryChanged() { dialog.check() }
        function onScanFinished() { dialog.check() }
    }

    contentItem: ColumnLayout {
        spacing: 14
        Label {
            Layout.fillWidth: true
            text: qsTr("Look up music and audiobooks online?")
            color: Theme.text
            font.pixelSize: Theme.fontLarge
            font.bold: true
            wrapMode: Text.Wrap
            Accessible.role: Accessible.Heading
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("Online services can add release dates, covers and descriptions. Nothing has been sent yet.")
            color: Theme.text
            wrapMode: Text.Wrap
        }
        Label {
            objectName: "dataFlows"
            Layout.fillWidth: true
            color: Theme.textDim
            wrapMode: Text.Wrap
            text: [qsTr("MusicBrainz receives album and artist names, track counts and MusicBrainz IDs."),
                   qsTr("The Cover Art Archive receives MusicBrainz IDs of albums."),
                   qsTr("Open Library receives audiobook titles, authors and ISBNs."),
                   qsTr("Like any website, these services also see your IP address.")].join("\n")
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("With files only, information comes from your files, their tags and metadata files, and nothing is sent. You can change this under Settings → Metadata.")
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 12
            FocusButton {
                id: filesButton
                objectName: "filesOnly"
                text: qsTr("Files only")
                KeyNavigation.right: onlineButton
                onClicked: dialog.choose(false)
            }
            FocusButton {
                id: onlineButton
                objectName: "useOnline"
                primary: true
                text: qsTr("Use online services")
                KeyNavigation.left: filesButton
                onClicked: dialog.choose(true)
            }
        }
    }
}
