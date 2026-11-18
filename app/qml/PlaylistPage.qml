pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl
import "PlaylistInfo.js" as PlaylistInfo

// A playlist (issue #81): its songs with format and length; Play, Shuffle and Save offline.
// An own playlist can be renamed, deleted, reordered (Ctrl+Up / Ctrl+Down) and have songs
// removed (Delete); smart playlists fill themselves. Enter plays from a song; Menu offers the
// song's actions.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // "smart:<rule>" or an own playlist's id.
    property string key: ""
    property var card: ({})
    property var entries: []
    property var request: 0
    property var saveRequest: 0
    readonly property bool own: !(card.smart ?? "") && (card.id ?? 0) > 0

    function load() { request = Core.playlistDetail(key) }
    function move(i, to) { if (own) request = Core.playlistMove(card.id, i, to) }
    Component.onCompleted: load()
    onActiveFocusChanged: if (activeFocus && !content.activeFocus) playButton.forceActiveFocus()

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === page.saveRequest) {
                page.saveRequest = 0;
                (page.Window.window as Main).showError(detail.started > 0
                    ? qsTr("Saving %n song(s). Progress is under Downloads.", "", detail.started)
                    : qsTr("These songs are saved or on this computer already."))
                return
            }
            if (requestId === page.deleteRequest) {
                page.deleteRequest = 0
                Nav.back()
                return
            }
            if (requestId !== page.request) return
            const keep = songs.currentIndex
            page.card = detail.card
            page.entries = detail.entries
            songs.currentIndex = Math.min(Math.max(0, keep), songs.count - 1)
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === page.saveRequest || requestId === page.request)
                (page.Window.window as Main).showError(message)
        }
    }
    property var deleteRequest: 0

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: 48
        spacing: 16

        Label {
            text: page.own ? qsTr("Playlist") : qsTr("Smart playlist")
            color: Theme.accent
            font.pixelSize: Theme.fontBody
        }
        Label {
            Layout.fillWidth: true
            text: PlaylistInfo.title(page.card)
            color: Theme.text
            font.pixelSize: 36
            font.bold: true
            elide: Text.ElideRight
            Accessible.role: Accessible.Heading
        }
        Label {
            text: [page.card.smart ? PlaylistInfo.rule(page.card.smart) : "",
                   qsTr("%n song(s)", "", page.card.count ?? 0),
                   (page.card.durationMs ?? 0) > 0 ? Theme.time(page.card.durationMs / 1000) : ""].filter(x => x).join("  ·  ")
            color: Theme.textDim
        }
        RowLayout {
            spacing: 10
            FocusButton {
                id: playButton
                primary: true
                glyph: "▶"
                text: qsTr("Play")
                enabled: page.entries.length > 0
                Keys.onLeftPressed: Nav.focusRail()
                KeyNavigation.right: shuffleButton
                KeyNavigation.down: songs
                onClicked: Music.playPlaylist(page.key, 0, false)
            }
            FocusButton {
                id: shuffleButton
                text: qsTr("Shuffle")
                enabled: page.entries.length > 0
                KeyNavigation.left: playButton
                KeyNavigation.right: saveButton
                KeyNavigation.down: songs
                onClicked: Music.playPlaylist(page.key, 0, true)
            }
            FocusButton {
                id: saveButton
                text: qsTr("Save offline")
                enabled: page.entries.length > 0
                KeyNavigation.left: shuffleButton
                KeyNavigation.right: page.own ? renameButton : null
                KeyNavigation.down: songs
                onClicked: page.saveRequest = Core.playlistSave(page.key)
            }
            FocusButton {
                id: renameButton
                visible: page.own
                text: qsTr("Rename")
                KeyNavigation.left: saveButton
                KeyNavigation.right: deleteButton
                KeyNavigation.down: songs
                onClicked: { nameField.text = page.card.name; renameDialog.open() }
            }
            FocusButton {
                id: deleteButton
                visible: page.own
                property bool armed: false
                text: armed ? qsTr("Really delete?") : qsTr("Delete playlist")
                KeyNavigation.left: renameButton
                KeyNavigation.down: songs
                onActiveFocusChanged: if (!activeFocus) armed = false
                onClicked: {
                    if (!armed) { armed = true; return }
                    page.deleteRequest = Core.playlistDelete(page.card.id)
                }
            }
        }
        Label {
            visible: page.own && page.entries.length > 0
            text: qsTr("Enter plays · Ctrl+↑/↓ moves · Delete removes · Menu for more")
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
        }
        Label {
            visible: page.entries.length === 0 && page.card.key !== undefined
            text: page.own ? qsTr("This playlist is empty. Use Menu on an album or a song: Add to playlist.")
                           : qsTr("Nothing here yet.")
            color: Theme.textDim
            font.pixelSize: Theme.fontLarge
        }

        ListView {
            id: songs
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: page.entries
            keyNavigationEnabled: true
            reuseItems: true
            Accessible.role: Accessible.List
            Accessible.name: PlaylistInfo.title(page.card)
            ScrollBar.vertical: ScrollBar {}
            Keys.onUpPressed: (e) => {
                if (e.modifiers & Qt.ControlModifier) { if (currentIndex > 0) { page.move(currentIndex, currentIndex - 1); currentIndex-- } }
                else if (currentIndex === 0) playButton.forceActiveFocus()
                else e.accepted = false
            }
            Keys.onDownPressed: (e) => {
                if (e.modifiers & Qt.ControlModifier) { if (currentIndex < count - 1) { page.move(currentIndex, currentIndex + 1); currentIndex++ } }
                else e.accepted = false
            }
            Keys.onDeletePressed: page.move(currentIndex, -1)
            Keys.onLeftPressed: Nav.focusRail()
            delegate: ItemDelegate {
                id: row
                required property var modelData
                required property int index
                width: songs.width
                height: 48
                readonly property bool focused: songs.activeFocus && songs.currentIndex === index
                readonly property bool now: Music.current.id === modelData.itemId
                Accessible.name: [modelData.title, modelData.artist, modelData.album, modelData.format].filter(x => x).join(", ")
                background: Rectangle {
                    radius: 8
                    color: row.focused ? Theme.surfaceHi : row.now ? "#1fe3a33b" : "transparent"
                    border.width: row.focused ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                contentItem: RowLayout {
                    spacing: 16
                    Label { Layout.preferredWidth: 36; text: row.now ? "▶" : String(row.index + 1); color: row.now ? Theme.accent : Theme.textDim }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Label { Layout.fillWidth: true; text: row.modelData.title; color: row.now ? Theme.accent : Theme.text; font.bold: true; elide: Text.ElideRight }
                        Label { Layout.fillWidth: true; text: [row.modelData.artist, row.modelData.album].filter(x => x).join("  ·  "); color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                    }
                    Label { text: row.modelData.durationMs > 0 ? Theme.time(row.modelData.durationMs / 1000) : ""; color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    Rectangle {
                        visible: row.modelData.format !== ""
                        implicitWidth: fmt.implicitWidth + 12
                        implicitHeight: 22
                        radius: 6
                        color: row.modelData.hires ? Theme.accent : Theme.surfaceHi
                        Label { id: fmt; anchors.centerIn: parent; text: row.modelData.format; font.pixelSize: 11; font.bold: true; color: row.modelData.hires ? Theme.bg : Theme.text }
                    }
                }
                onClicked: { songs.currentIndex = index; Music.playPlaylist(page.key, index, false) }
                Keys.onReturnPressed: Music.playPlaylist(page.key, index, false)
                Keys.onEnterPressed: Music.playPlaylist(page.key, index, false)
                Keys.onMenuPressed: {
                    const list = [{ label: qsTr("Play next"), value: "next" }, { label: qsTr("Add to queue"), value: "queue" },
                                  { label: qsTr("Add to playlist…"), value: "playlist" }]
                    if (page.own) list.push({ label: qsTr("Remove from playlist"), value: "remove" })
                    list.push({ label: qsTr("Go to album"), value: "album" })
                    rowMenu.target = modelData
                    rowMenu.index = index
                    rowMenu.show(row, list)
                }
            }
        }
    }

    AddToPlaylist { id: addToPlaylist }
    ActionMenu {
        id: rowMenu
        property var target: ({})
        property int index: 0
        onChosen: (v) => {
            if (v === "next") Music.playNext([target.itemId])
            else if (v === "queue") Music.addToQueue([target.itemId])
            else if (v === "remove") page.move(index, -1)
            else if (v === "playlist") addToPlaylist.show([target.itemId], 0, songs)
            else Nav.openItem(target.albumId, "album")
        }
    }

    Popup {
        id: renameDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(460, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: nameField.forceActiveFocus()
        onClosed: renameButton.forceActiveFocus()
        RowLayout {
            anchors.fill: parent
            spacing: 8
            TextField {
                id: nameField
                Layout.fillWidth: true
                Accessible.name: qsTr("Name of the playlist")
                KeyNavigation.right: okButton
                onAccepted: okButton.clicked()
            }
            FocusButton {
                id: okButton
                primary: true
                text: qsTr("Rename")
                KeyNavigation.left: nameField
                onClicked: {
                    page.request = Core.playlistRename(page.card.id, nameField.text)
                    renameDialog.close()
                }
            }
        }
    }
}
