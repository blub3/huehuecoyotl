pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl
import "MediaInfo.js" as MediaInfo

// An album (issue #10): cover, artist, year, length, the format line and the Hi-Res badge,
// Play, Shuffle, Play next, Add to queue (issue #8) and Save (offline copies of all its
// tracks), and the tracks grouped by disc. Enter on a track plays the album from it; Menu on a
// track offers Play next and Add to queue.
// Everything comes from the local database (SPEC 2, principle 5).
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: d.title || ""
    readonly property string pageKind: qsTr("Album")
    property var itemId
    // A track to put focus on (opened from search).
    property var focusTrack: 0
    property var d: ({})
    property var request: 0
    readonly property bool loaded: d.id !== undefined
    readonly property var tracks: d.tracks || []
    // Issue #63: the album's versions (best first) and the one shown (0: the core picks the one
    // played last, else by the preferred version).
    readonly property var versions: d.versions || []
    property var version: 0
    readonly property int versionIndex: Math.max(0, versions.findIndex(v => v.id === d.versionId))
    property var splitRequest: 0

    // Offline copies (issue #4): the album's save state, polled while copies run.
    property var sv: ({ state: "none", saveable: 0 })
    property var saveRequest: 0
    readonly property bool saveBusy: sv.state === "queued" || sv.state === "running"

    function load() {
        request = Core.albumDetail(itemId, version, Appearance.preferredVersion)
        saveRequest = Core.saveState(itemId)
    }
    function play(shuffle) {
        if (tracks.length > 0 && d.playable) Music.playItems([itemId], 0, shuffle, d.versionId || 0)
    }
    function playFrom(trackId) { if (d.playable) Music.playItems([itemId], trackId, false, d.versionId || 0) }
    function chooseVersion(i) {
        if (i < 0 || i >= versions.length) return
        version = versions[i].id
        load()
    }
    function saveAction(action) {
        // Issue #63: "Save" keeps the version shown (any of its files names it).
        if (action === "save" || action === "saveAgain") saveRequest = Core.saveItem(itemId, tracks.length > 0 ? tracks[0].fileId : 0)
        else if (action === "pause") Core.pauseSavedItem(itemId)
        else if (action === "resume" || action === "retry") Core.resumeSavedItem(itemId)
        else if (action === "cancel" || action === "delete") deleteConfirm.open()
        saveTimer.restart()
    }

    Component.onCompleted: load()
    onActiveFocusChanged: if (activeFocus && loaded && !content.activeFocus) playButton.forceActiveFocus()

    Timer {
        id: saveTimer
        interval: 1000
        repeat: true
        running: page.saveBusy && page.visible
        onTriggered: page.saveRequest = Core.saveState(page.itemId)
    }

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            // "This is a different album": the new album opens.
            if (requestId === page.splitRequest) {
                page.splitRequest = 0
                page.version = 0
                page.load()
                Nav.openItem(detail.id, "album")
                return
            }
            if (requestId !== page.request) return
            const first = !page.loaded
            const keep = trackList.currentIndex
            page.d = detail
            if (first) {
                const i = page.tracks.findIndex(t => t.id === page.focusTrack)
                if (i >= 0) { trackList.currentIndex = i; trackList.forceActiveFocus() }
                else playButton.forceActiveFocus()
            } else {
                trackList.currentIndex = Math.min(keep, trackList.count - 1)
            }
        }
        function onSaveStateReady(requestId, id, info) { if (id === page.itemId) page.sv = info }
        function onDownloadsListed(requestId) { if (requestId === page.saveRequest) page.saveRequest = Core.saveState(page.itemId) }
        function onItemChanged(id) { if (id === page.itemId || page.tracks.some(t => t.id === id)) page.load() }
        function onLibraryChanged() { page.load() }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.implicitHeight + 96
        clip: true
        ScrollBar.vertical: ScrollBar {}
        // Keep the focused element visible.
        function reveal(item) {
            const p = item.mapToItem(contentItem, 0, 0)
            if (p.y < contentY + 40) contentY = Math.max(0, p.y - 40)
            else if (p.y + item.height > contentY + height - 40)
                contentY = Math.min(p.y + item.height - height + 40, Math.max(0, contentHeight - height))
        }

        FocusScope {
            id: content
            x: 48
            y: 48
            width: flick.width - 96
            implicitHeight: col.implicitHeight
            focus: true

            ColumnLayout {
                id: col
                width: parent.width
                spacing: 24

                RowLayout {
                    spacing: 32
                    Layout.fillWidth: true
                    Rectangle {
                        Layout.preferredWidth: 280
                        Layout.preferredHeight: 280
                        Layout.alignment: Qt.AlignTop
                        radius: Theme.radius
                        clip: true
                        color: Qt.hsla(((page.d.title || "").length * 37 % 360) / 360, 0.30, 0.24, 1)
                        Label {
                            anchors.centerIn: parent
                            width: parent.width - 32
                            visible: cover.status !== Image.Ready
                            text: page.d.title || ""
                            color: Theme.text
                            font.pixelSize: 22
                            font.bold: true
                            wrapMode: Text.Wrap
                            horizontalAlignment: Text.AlignHCenter
                        }
                        Image {
                            id: cover
                            anchors.fill: parent
                            source: page.d.posterFull || page.d.poster || ""
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                            sourceSize.width: 560
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: 10
                        Label {
                            text: MediaInfo.releaseType(page.d.releaseType || "", page.d.compilation === true)
                            color: Theme.accent
                            font.pixelSize: Theme.fontBody
                        }
                        Label {
                            Layout.fillWidth: true
                            text: page.d.title || ""
                            color: Theme.text
                            font.pixelSize: 36
                            font.bold: true
                            wrapMode: Text.Wrap
                            Accessible.role: Accessible.Heading
                        }
                        FocusButton {
                            id: artistButton
                            visible: (page.d.artist || "") !== ""
                            text: page.d.artist || ""
                            tip: qsTr("Open the artist")
                            KeyNavigation.down: versionPicker.visible ? versionPicker : playButton
                            Keys.onLeftPressed: Nav.focusRail()
                            onClicked: Nav.openItem(page.d.artistId, "artist")
                        }
                        Label {
                            text: {
                                const parts = []
                                if (page.d.year > 0) parts.push(page.d.year)
                                if (page.tracks.length > 0) parts.push(qsTr("%n track(s)", "", page.tracks.length))
                                if (page.d.discs > 1) parts.push(qsTr("%n disc(s)", "", page.d.discs))
                                if (page.d.durationMs > 0) parts.push(MediaInfo.duration(page.d.durationMs))
                                if (page.d.genre) parts.push(page.d.genre)
                                return parts.join("  ·  ")
                            }
                            color: Theme.textDim
                            font.pixelSize: Theme.fontBody
                        }
                        RowLayout {
                            spacing: 8
                            visible: (page.d.codec || "") !== ""
                            Rectangle {
                                visible: page.d.hires === true
                                implicitWidth: hiresText.implicitWidth + 12
                                implicitHeight: 22
                                radius: 4
                                color: "transparent"
                                border.width: 1
                                border.color: Theme.accent
                                Label { id: hiresText; anchors.centerIn: parent; text: qsTr("Hi-Res"); color: Theme.accent; font.pixelSize: 12; font.bold: true }
                            }
                            Label {
                                text: MediaInfo.audioLine(page.d.codec || "", page.d.sampleRate || 0, page.d.bitDepth || 0)
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                            }
                        }
                        // Issue #63: the version shown, when there are several.
                        RowLayout {
                            spacing: 10
                            visible: page.versions.length > 1
                            Layout.topMargin: 4
                            VersionPicker {
                                id: versionPicker
                                files: page.versions
                                index: page.versionIndex
                                labels: page.versions.map(v => MediaInfo.musicVersionLabel(v))
                                choices: page.versions.map(v => MediaInfo.musicVersionChoice(v))
                                Keys.onUpPressed: (e) => { const t = artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.down: playButton
                                KeyNavigation.right: splitButton
                                Keys.onLeftPressed: Nav.focusRail()
                                onChosen: (i) => page.chooseVersion(i)
                            }
                            FocusButton {
                                id: splitButton
                                text: qsTr("This is a different album")
                                font.pixelSize: Theme.fontSmall
                                KeyNavigation.left: versionPicker
                                KeyNavigation.down: playButton
                                Keys.onUpPressed: (e) => { const t = artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                onClicked: splitConfirm.open()
                            }
                        }
                        Label {
                            visible: page.loaded && page.d.allOffline === true
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            text: qsTr("Every version of this album is on a source that cannot be reached, and none is saved on this computer.")
                            color: Theme.danger
                            font.pixelSize: Theme.fontSmall
                        }
                        Label {
                            visible: page.loaded && !page.d.allOffline && page.d.playable === false
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            text: qsTr("This version is offline. Choose another one to play.")
                            color: Theme.danger
                            font.pixelSize: Theme.fontSmall
                        }
                        RowLayout {
                            Layout.topMargin: 12
                            spacing: 12
                            FocusButton {
                                id: playButton
                                primary: true
                                focus: true
                                glyph: "▶"
                                text: qsTr("Play")
                                enabled: page.tracks.length > 0 && page.d.playable === true
                                Keys.onUpPressed: (e) => { const t = versionPicker.visible ? versionPicker : artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.right: shuffleButton
                                KeyNavigation.down: trackList
                                Keys.onLeftPressed: Nav.focusRail()
                                onClicked: page.play(false)
                            }
                            FocusButton {
                                id: shuffleButton
                                glyph: "⤮"
                                text: qsTr("Shuffle")
                                enabled: page.tracks.length > 1 && page.d.playable === true
                                Keys.onUpPressed: (e) => { const t = versionPicker.visible ? versionPicker : artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.left: playButton
                                KeyNavigation.right: playNextButton
                                KeyNavigation.down: trackList
                                onClicked: page.play(true)
                            }
                            FocusButton {
                                id: playNextButton
                                glyph: "⤵"
                                tip: qsTr("Play next")
                                enabled: page.tracks.length > 0 && page.d.playable === true
                                Keys.onUpPressed: (e) => { const t = versionPicker.visible ? versionPicker : artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.left: shuffleButton
                                KeyNavigation.right: queueButton
                                KeyNavigation.down: trackList
                                onClicked: Music.playNext([page.itemId], page.d.versionId || 0)
                            }
                            FocusButton {
                                id: queueButton
                                glyph: "+"
                                tip: qsTr("Add to queue")
                                enabled: page.tracks.length > 0 && page.d.playable === true
                                Keys.onUpPressed: (e) => { const t = versionPicker.visible ? versionPicker : artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.left: playNextButton
                                KeyNavigation.right: playlistButton
                                KeyNavigation.down: trackList
                                onClicked: Music.addToQueue([page.itemId], page.d.versionId || 0)
                            }
                            // Issue #81: this version's songs to a playlist.
                            FocusButton {
                                id: playlistButton
                                glyph: "≡+"
                                tip: qsTr("Add to playlist")
                                enabled: page.tracks.length > 0
                                Keys.onUpPressed: (e) => { const t = versionPicker.visible ? versionPicker : artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.left: queueButton
                                KeyNavigation.right: saveMenu.visible ? saveMenu : moreButton
                                KeyNavigation.down: trackList
                                onClicked: addToPlaylist.show([page.itemId], page.d.versionId || 0, playlistButton)
                            }
                            SaveMenu {
                                id: saveMenu
                                sv: page.sv
                                canExport: false
                                Keys.onUpPressed: (e) => { const t = versionPicker.visible ? versionPicker : artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.left: playlistButton
                                KeyNavigation.right: moreButton
                                KeyNavigation.down: trackList
                                onTriggered: (action) => page.saveAction(action)
                            }
                            // Issues #101 and #102: match the album again, or search MusicBrainz.
                            FocusButton {
                                id: moreButton
                                objectName: "albumMore"
                                glyph: "⋯"
                                tip: qsTr("More")
                                Keys.onUpPressed: (e) => { const t = versionPicker.visible ? versionPicker : artistButton.visible ? artistButton : null; if (t) t.forceActiveFocus(); else e.accepted = Nav.focusBack() }
                                KeyNavigation.left: saveMenu.visible ? saveMenu : playlistButton
                                KeyNavigation.down: trackList
                                onClicked: albumMenu.show(moreButton, [{ value: "match", label: qsTr("Match again") },
                                                                       { value: "search", label: qsTr("Search MusicBrainz…") }])
                            }
                        }
                    }
                }

                // The tracks, with a heading per disc when there are several.
                ListView {
                    id: trackList
                    Layout.fillWidth: true
                    Layout.preferredHeight: contentHeight
                    interactive: false
                    model: page.tracks
                    keyNavigationEnabled: true
                    Accessible.role: Accessible.List
                    Accessible.name: qsTr("Tracks")
                    section.property: page.d.discs > 1 ? "disc" : ""
                    section.delegate: Label {
                        required property string section
                        topPadding: 16
                        bottomPadding: 6
                        text: qsTr("Disc %1").arg(section)
                        color: Theme.textDim
                        font.pixelSize: Theme.fontLarge
                    }
                    Keys.onUpPressed: (e) => { if (currentIndex > 0) e.accepted = false; else playButton.forceActiveFocus() }
                    Keys.onLeftPressed: Nav.focusRail()
                    onCurrentIndexChanged: if (activeFocus && currentItem) flick.reveal(currentItem)
                    onActiveFocusChanged: if (activeFocus && currentItem) flick.reveal(currentItem)
                    delegate: ItemDelegate {
                        id: row
                        required property var modelData
                        required property int index
                        width: trackList.width
                        height: 52
                        focus: ListView.isCurrentItem
                        Accessible.name: qsTr("Track %1, %2").arg(modelData.number).arg(modelData.title)
                            + (modelData.artists !== page.d.artist && modelData.artists !== "" ? ", " + modelData.artists : "")
                        readonly property bool nowPlaying: Music.current.id === modelData.id
                        onClicked: page.playFrom(modelData.id)
                        Keys.onReturnPressed: page.playFrom(modelData.id)
                        Keys.onEnterPressed: page.playFrom(modelData.id)
                        Keys.onPressed: (e) => {
                            if (e.key === Qt.Key_Menu) { trackMenu.showFor(row, row.modelData); e.accepted = true }
                        }
                        background: Rectangle {
                            radius: 8
                            color: row.activeFocus ? Theme.surfaceHi : row.hovered ? Theme.surface : "transparent"
                            border.width: row.activeFocus ? Theme.focusWidth : 0
                            border.color: Theme.focus
                        }
                        contentItem: RowLayout {
                            spacing: 16
                            Label {
                                Layout.preferredWidth: 36
                                text: row.nowPlaying ? "♪" : row.modelData.number > 0 ? row.modelData.number : ""
                                color: row.nowPlaying ? Theme.accent : Theme.textDim
                                horizontalAlignment: Text.AlignRight
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label {
                                    Layout.fillWidth: true
                                    text: row.modelData.title
                                    color: Theme.text
                                    font.pixelSize: Theme.fontBody
                                    elide: Text.ElideRight
                                }
                                Label {
                                    Layout.fillWidth: true
                                    // Only when the track's artists differ from the album's.
                                    visible: row.modelData.artists !== "" && row.modelData.artists !== page.d.artist
                                    text: row.modelData.artists
                                    color: Theme.textDim
                                    font.pixelSize: Theme.fontSmall
                                    elide: Text.ElideRight
                                }
                            }
                            Label {
                                visible: row.modelData.saved === true
                                text: "⤓"
                                color: Theme.accent
                            }
                            // Issue #63: two files of this track in this version (Menu chooses).
                            Label {
                                visible: (row.modelData.files || []).length > 1
                                text: qsTr("%n version(s)", "", (row.modelData.files || []).length)
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                            }
                            Label {
                                visible: row.modelData.hires === true && page.d.hires !== true
                                text: qsTr("Hi-Res")
                                color: Theme.accent
                                font.pixelSize: Theme.fontSmall
                            }
                            Label {
                                text: row.modelData.durationMs > 0 ? Theme.time(row.modelData.durationMs / 1000) : ""
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                            }
                        }
                    }
                }
            }
        }
    }

    AddToPlaylist { id: addToPlaylist }

    // Menu on a track (issue #8).
    ActionMenu {
        id: trackMenu
        property var trackId: 0
        property string trackTitle: ""
        property string trackArtist: ""
        menuWidth: Math.min(640, page.width - 120)
        // Issue #63: a track with two files in this version offers each ("Version: …").
        function showFor(item, t) {
            trackId = t.id
            trackTitle = t.title || ""
            trackArtist = t.artists || page.d.artist || ""
            const list = [{ value: "next", label: qsTr("Play next") }, { value: "queue", label: qsTr("Add to queue") },
                          { value: "playlist", label: qsTr("Add to playlist…") }]
            for (const f of (t.files || []))
                list.push({ value: "file:" + f.id,
                            label: (f.id === t.fileId ? "✓ " : "") + qsTr("Version: %1").arg(MediaInfo.trackFileLabel(f)) })
            // Issues #103 and #104.
            list.push({ value: "match", label: qsTr("Match again") }, { value: "search", label: qsTr("Search MusicBrainz…") })
            show(item, list)
        }
        onChosen: (v) => {
            if (v === "next") Music.playNext([trackId], page.d.versionId || 0)
            else if (v === "queue") Music.addToQueue([trackId], page.d.versionId || 0)
            else if (v === "playlist") addToPlaylist.show([trackId], page.d.versionId || 0, trackList)
            else if (v.startsWith("file:")) Core.setTrackFile(trackId, Number(v.slice(5)))
            else if (v === "match") musicMatch.again(trackId, "track", trackTitle, trackList)
            else if (v === "search") musicMatch.search(trackId, "track", trackTitle, trackArtist, trackList)
        }
    }

    ActionMenu {
        id: albumMenu
        onChosen: (v) => {
            if (v === "match") musicMatch.again(page.itemId, "album", page.d.title, moreButton)
            else musicMatch.search(page.itemId, "album", page.d.title, page.d.artist, moreButton)
        }
    }
    MusicMatch {
        id: musicMatch
        backend: Core
        onNotice: (text) => (page.Window.window as Main).showError(text)
        onSettingsRequested: (tab) => Nav.settings(tab)
        onChanged: (id) => {
            // A merge can keep another album: the page follows it.
            if (id !== undefined && id !== page.itemId && musicMatch.kind === "album") page.itemId = id
            page.load()
        }
    }

    // Issue #63: "This is a different album".
    Popup {
        id: splitConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(600, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: splitKeep.forceActiveFocus()
        onClosed: if (versionPicker.visible) splitButton.forceActiveFocus(); else playButton.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("Make “%1” an album of its own? It stays apart from this one, also after the next scan.")
                      .arg(page.versions.length > 0 ? MediaInfo.musicVersionLabel(page.versions[page.versionIndex]) : "")
            }
            RowLayout {
                spacing: 12
                FocusButton { id: splitKeep; text: qsTr("Cancel"); KeyNavigation.right: splitOk; onClicked: splitConfirm.close() }
                FocusButton {
                    id: splitOk
                    primary: true
                    text: qsTr("Make it an album")
                    KeyNavigation.left: splitKeep
                    onClicked: { page.splitRequest = Core.splitAlbumVersion(page.d.versionId); splitConfirm.close() }
                }
            }
        }
    }

    Popup {
        id: deleteConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: deleteKeep.forceActiveFocus()
        onClosed: saveMenu.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: page.saveBusy || page.sv.state === "paused"
                      ? qsTr("Stop saving and remove what was copied so far? The files on the server stay.")
                      : qsTr("Delete the saved copies of this album from this computer? The files on the server stay.")
            }
            RowLayout {
                spacing: 12
                FocusButton { id: deleteKeep; text: qsTr("Keep"); KeyNavigation.right: deleteOk; onClicked: deleteConfirm.close() }
                FocusButton {
                    id: deleteOk
                    primary: true
                    text: qsTr("Delete saved copies")
                    KeyNavigation.left: deleteKeep
                    onClicked: { page.saveRequest = Core.deleteSavedItem(page.itemId); deleteConfirm.close() }
                }
            }
        }
    }
}
