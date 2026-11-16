pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl
import "MediaInfo.js" as MediaInfo

// An album (issue #10): cover, artist, year, length, the format line and the Hi-Res badge,
// Play, Shuffle and Save (offline copies of all its tracks), and the tracks grouped by disc.
// Everything comes from the local database (SPEC 2, principle 5).
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var itemId
    // A track to put focus on (opened from search).
    property var focusTrack: 0
    property var d: ({})
    property var request: 0
    readonly property bool loaded: d.id !== undefined
    readonly property var tracks: d.tracks || []

    // Offline copies (issue #4): the album's save state, polled while copies run.
    property var sv: ({ state: "none", saveable: 0 })
    property var saveRequest: 0
    readonly property bool saveBusy: sv.state === "queued" || sv.state === "running"

    function load() {
        request = Core.albumDetail(itemId)
        saveRequest = Core.saveState(itemId)
    }
    // Until the play queue (issue #8), Play starts the first track and Shuffle a random one.
    function play(shuffle) {
        if (tracks.length === 0) return
        const t = shuffle ? tracks[Math.floor(Math.random() * tracks.length)] : tracks[0]
        Playback.openItem(t.id)
    }
    function saveAction(action) {
        if (action === "save" || action === "saveAgain") saveRequest = Core.saveItem(itemId, 0)
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
                            KeyNavigation.down: playButton
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
                        RowLayout {
                            Layout.topMargin: 12
                            spacing: 12
                            FocusButton {
                                id: playButton
                                primary: true
                                focus: true
                                glyph: "▶"
                                text: qsTr("Play")
                                enabled: page.tracks.length > 0
                                KeyNavigation.up: artistButton.visible ? artistButton : null
                                KeyNavigation.right: shuffleButton
                                KeyNavigation.down: trackList
                                Keys.onLeftPressed: Nav.focusRail()
                                onClicked: page.play(false)
                            }
                            FocusButton {
                                id: shuffleButton
                                glyph: "⤮"
                                text: qsTr("Shuffle")
                                enabled: page.tracks.length > 1
                                KeyNavigation.up: artistButton.visible ? artistButton : null
                                KeyNavigation.left: playButton
                                KeyNavigation.right: saveMenu.visible ? saveMenu : null
                                KeyNavigation.down: trackList
                                onClicked: page.play(true)
                            }
                            SaveMenu {
                                id: saveMenu
                                sv: page.sv
                                canExport: false
                                KeyNavigation.up: artistButton.visible ? artistButton : null
                                KeyNavigation.left: shuffleButton
                                KeyNavigation.down: trackList
                                onTriggered: (action) => page.saveAction(action)
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
                        onClicked: Playback.openItem(modelData.id)
                        Keys.onReturnPressed: Playback.openItem(modelData.id)
                        Keys.onEnterPressed: Playback.openItem(modelData.id)
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
                                text: row.modelData.number > 0 ? row.modelData.number : ""
                                color: Theme.textDim
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
