pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Matching music again (issues #101 to #105), shared by the album, artist, songs and queue
// menus: `again(id, kind, title, from)` matches an album, song or artist again (asking first
// when the user chose its match), `search(id, kind, title, artist, from)` opens "Find the album",
// "Find the song" or "Find the artist" at MusicBrainz. Results are compared with what you have
// (tracks, length, your albums) before you choose; Enter on a result shows the comparison, Enter
// again chooses. `notice(text)` reports a result, `changed(id)` names the item that now carries
// it. `backend` is the core (Core in the app, a stub in tests).
Item {
    id: matcher
    property var backend: null
    property var itemId: 0
    property string kind: "album"
    property string title: ""
    property string artist: ""
    property Item from: null
    property var request: 0
    property var candidates: []
    property int expanded: -1
    property var preview: null
    property bool searching: false
    property string message: ""
    property string hint: ""
    signal notice(string text)
    signal changed(var newItemId)

    function lockText() {
        if (kind === "track") return qsTr("You chose the match of this song yourself. Match it again anyway? A sure match replaces your choice.")
        if (kind === "artist") return qsTr("You chose the match of this artist yourself. Match it again anyway? A sure match replaces your choice.")
        return qsTr("You chose the match of this album yourself. Match it again anyway? A sure match replaces your choice.")
    }
    // Nothing is sent while music information online is off: say so instead.
    readonly property bool offline: backend !== null && backend.musicOnline === false
    readonly property string offlineText: qsTr("Turn on Music and book information online under Settings → Metadata to look music up at MusicBrainz.")
    function again(id, k, t, item) {
        itemId = id; kind = k; title = t || ""; from = item
        if (offline) { notice(offlineText); return }
        request = backend.matchAgain(id, false)
    }
    function search(id, k, t, a, item) {
        itemId = id; kind = k; title = t || ""; artist = a || ""; from = item
        hint = ""
        openSearch()
    }
    function openSearch() {
        firstField.text = title
        secondField.text = kind === "artist" ? "" : artist
        candidates = []
        expanded = -1
        preview = null
        message = ""
        dialog.open()
        run()
    }
    function run() {
        if (offline) { message = offlineText; candidates = []; return }
        searching = true
        message = ""
        expanded = -1
        preview = null
        const q = { text: firstField.text }
        if (kind !== "artist") q.artist = secondField.text
        request = backend.matchSearch(itemId, q)
    }
    function expand(i) {
        if (expanded === i) { choose(); return }
        expanded = i
        preview = null
        if (kind === "album") request = backend.matchPreview(itemId, { id: candidates[i].id, kind: "release" })
    }
    function choose() {
        if (expanded < 0) return
        const c = candidates[expanded]
        request = backend.matchChoose(itemId, { id: c.id, kind: c.kind })
    }
    function fitText(c) {
        if (kind === "artist")
            return c.albums === undefined || c.albums === null ? ""
                 : qsTr("%1 of your %2 albums are theirs").arg(c.albums).arg(c.ofAlbums)
        if (kind === "track") {
            const f = c.fit || ""
            if (f.startsWith("lengthAgrees:")) return qsTr("Length agrees: %1").arg(f.slice(13))
            if (f.startsWith("lengthDiffers:")) return qsTr("Length differs by %1").arg(f.slice(14))
            if (f === "otherArtist") return qsTr("Other artist")
            return ""
        }
        if (c.tracks && c.localTracks)
            return c.tracks === c.localTracks ? qsTr("%1 of %2 tracks").arg(c.tracks).arg(c.localTracks)
                                              : qsTr("%1 tracks, you have %2").arg(c.tracks).arg(c.localTracks)
        return ""
    }
    function metaText(c) {
        if (kind === "album")
            return [c.type, c.year, c.country, c.label, (c.formats || []).join(" + "),
                    c.tracks ? qsTr("%n track(s)", "", c.tracks) : ""].filter(x => x).join(" · ")
        if (kind === "track")
            return [c.release, c.year, c.lengthMs ? Math.floor(c.lengthMs / 60000) + ":" + String(Math.floor(c.lengthMs / 1000) % 60).padStart(2, "0") : ""]
                   .filter(x => x).join(" · ")
        return [c.type, c.country, c.begin ? (c.begin + (c.end ? " – " + c.end : " – ")) : ""].filter(x => x).join(" · ")
    }
    function effectText() {
        if (kind === "album") return qsTr("Release type, date and cover come from this release. Its artists without a match are matched with it.")
        if (kind === "track") return qsTr("The song's recording, ISRC and credits come from this match; your tags stay. Its artists without a match are matched with it.")
        return qsTr("Image, biography and links are looked up again for this artist, and its albums without a match too.")
    }

    Connections {
        target: matcher.backend
        ignoreUnknownSignals: true
        function onMusicDetailReady(requestId, k, detail) {
            if (requestId !== matcher.request) return
            if (k === "matchAgain") {
                switch (detail.result) {
                case "locked": lockConfirm.open(); return
                case "unchanged": matcher.notice(qsTr("Matched again: no change, “%1” was already right.").arg(matcher.title)); break
                case "newly": case "changed":
                    matcher.notice(qsTr("Matched again: “%1”.").arg(detail.title || matcher.title))
                    matcher.changed(detail.newItemId)
                    break
                case "review":
                    matcher.hint = qsTr("Several matches fit. Choose one.")
                    matcher.openSearch()
                    break
                case "notFound": matcher.notice(qsTr("Nothing sure was found for “%1”; it stays as it is.").arg(matcher.title)); break
                }
            } else if (k === "matchCandidates") {
                matcher.searching = false
                matcher.candidates = detail.candidates || []
            } else if (k === "matchPreview") {
                matcher.preview = detail
            } else if (k === "matchChosen") {
                dialog.close()
                matcher.notice(qsTr("Matched: “%1”.").arg(matcher.candidates[matcher.expanded] ? matcher.candidates[matcher.expanded].title : matcher.title))
                matcher.changed(detail.newItemId)
            }
        }
        function onRequestFailed(requestId, k, message) {
            if (requestId !== matcher.request) return
            matcher.searching = false
            if (dialog.visible) matcher.message = message; else matcher.notice(message)
        }
    }

    // The user chose this match: ask before it is looked up again.
    Popup {
        id: lockConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, (parent ? parent.width : 800) - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: keepButton.forceActiveFocus()
        onClosed: if (matcher.from && !dialog.visible) matcher.from.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                objectName: "lockQuestion"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: matcher.lockText()
            }
            RowLayout {
                spacing: 12
                FocusButton { id: keepButton; objectName: "keepChoice"; text: qsTr("Keep my choice"); KeyNavigation.right: unlockButton; onClicked: lockConfirm.close() }
                FocusButton {
                    id: unlockButton
                    objectName: "matchAnyway"
                    primary: true
                    text: qsTr("Match again")
                    KeyNavigation.left: keepButton
                    onClicked: { lockConfirm.close(); matcher.request = matcher.backend.matchAgain(matcher.itemId, true) }
                }
            }
        }
    }

    Popup {
        id: dialog
        objectName: "musicMatchDialog"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(parent ? parent.width - 80 : 800, 900)
        height: Math.min(parent ? parent.height - 80 : 700, 780)
        modal: true
        focus: true
        padding: 24
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: firstField.forceActiveFocus()
        onClosed: if (matcher.from) matcher.from.forceActiveFocus()

        contentItem: ColumnLayout {
            spacing: 12
            Keys.onPressed: (e) => { if (e.key === Qt.Key_Back) { dialog.close(); e.accepted = true } }
            RowLayout {
                spacing: 12
                Label {
                    objectName: "searchHeading"
                    text: matcher.kind === "track" ? qsTr("Find the song") : matcher.kind === "artist" ? qsTr("Find the artist") : qsTr("Find the album")
                    color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true
                }
                Label { text: qsTr("at MusicBrainz"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            }
            Label { visible: matcher.hint !== ""; text: matcher.hint; color: Theme.accent; Layout.fillWidth: true; wrapMode: Text.Wrap }
            RowLayout {
                spacing: 10
                Layout.fillWidth: true
                TextField {
                    id: firstField
                    color: Theme.text
                    placeholderTextColor: Theme.textDim
                    background: Rectangle {
                        implicitHeight: 44
                        radius: Theme.radius
                        color: Theme.bg
                        border.width: firstField.activeFocus ? Theme.focusWidth : 1
                        border.color: firstField.activeFocus ? Theme.focus : Theme.surfaceHi
                    }
                    objectName: "firstField"
                    Layout.fillWidth: true
                    placeholderText: matcher.kind === "track" ? qsTr("Title") : matcher.kind === "artist" ? qsTr("Name") : qsTr("Album")
                    Accessible.name: placeholderText
                    KeyNavigation.right: secondField.visible ? secondField : searchButton
                    KeyNavigation.down: results
                    onAccepted: matcher.run()
                }
                TextField {
                    id: secondField
                    color: Theme.text
                    placeholderTextColor: Theme.textDim
                    background: Rectangle {
                        implicitHeight: 44
                        radius: Theme.radius
                        color: Theme.bg
                        border.width: secondField.activeFocus ? Theme.focusWidth : 1
                        border.color: secondField.activeFocus ? Theme.focus : Theme.surfaceHi
                    }
                    visible: matcher.kind !== "artist"
                    Layout.preferredWidth: 260
                    placeholderText: qsTr("Artist")
                    Accessible.name: qsTr("Artist")
                    KeyNavigation.left: firstField
                    KeyNavigation.right: searchButton
                    KeyNavigation.down: results
                    onAccepted: matcher.run()
                }
                FocusButton { id: searchButton; text: qsTr("Search"); KeyNavigation.left: secondField.visible ? secondField : firstField; KeyNavigation.down: results; onClicked: matcher.run() }
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: (matcher.kind === "album" ? qsTr("Releases are listed, so you can pick the exact edition: track count, country, label and format tell them apart.")
                       : matcher.kind === "track" ? qsTr("Songs are recordings at MusicBrainz; choosing one also tells which artists it belongs to.")
                       : qsTr("Several artists can share a name; the description, country and your albums tell them apart."))
                      + " " + qsTr("Or paste a MusicBrainz link.")
            }
            Label { visible: matcher.message !== ""; text: matcher.message; color: Theme.danger; Layout.fillWidth: true; wrapMode: Text.Wrap }
            BusyIndicator { visible: matcher.searching; running: visible; Layout.alignment: Qt.AlignHCenter }
            Label {
                visible: !matcher.searching && matcher.candidates.length === 0 && matcher.message === ""
                text: qsTr("No results. Try another spelling, or leave the artist out.")
                color: Theme.textDim
            }
            ListView {
                id: results
                objectName: "results"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 6
                model: matcher.candidates
                keyNavigationEnabled: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Matches")
                Keys.onUpPressed: (e) => { if (currentIndex === 0) firstField.forceActiveFocus(); else e.accepted = false }
                onCurrentIndexChanged: if (matcher.expanded >= 0 && matcher.expanded !== currentIndex) { matcher.expanded = -1; matcher.preview = null }
                delegate: ItemDelegate {
                    id: cand
                    required property var modelData
                    required property int index
                    readonly property bool open: matcher.expanded === index
                    width: ListView.view.width
                    height: Math.max(84, body.implicitHeight + 20)
                    focus: ListView.isCurrentItem
                    Accessible.name: modelData.title + ", " + matcher.metaText(modelData) + ", " + matcher.fitText(modelData)
                    onClicked: { results.currentIndex = index; matcher.expand(index) }
                    Keys.onReturnPressed: matcher.expand(index)
                    Keys.onEnterPressed: matcher.expand(index)
                    background: Rectangle {
                        radius: Theme.radius
                        color: cand.activeFocus || cand.hovered || cand.open ? Theme.surfaceHi : "transparent"
                        border.width: cand.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        id: body
                        spacing: 14
                        Rectangle {
                            Layout.preferredWidth: 60; Layout.preferredHeight: 60
                            Layout.alignment: Qt.AlignTop
                            radius: matcher.kind === "artist" ? 30 : 6
                            clip: true
                            color: Theme.bg
                            Image { anchors.fill: parent; source: cand.modelData.image || ""; asynchronous: true; fillMode: Image.PreserveAspectCrop }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.alignment: Qt.AlignTop
                            spacing: 3
                            RowLayout {
                                spacing: 8
                                Label { text: cand.modelData.title; color: Theme.text; font.bold: true; font.pixelSize: Theme.fontBody }
                                Label { text: cand.modelData.artist || ""; color: Theme.textDim; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; elide: Text.ElideRight }
                                Label { visible: !!cand.modelData.current; text: qsTr("matched now"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                            }
                            Label { text: matcher.metaText(cand.modelData); color: Theme.textDim; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; elide: Text.ElideRight }
                            Label {
                                objectName: "fit" + cand.index
                                visible: text !== ""
                                text: (cand.modelData.good ? "✓ " : "△ ") + matcher.fitText(cand.modelData)
                                color: cand.modelData.good ? Theme.accent : Theme.text
                                font.pixelSize: Theme.fontSmall
                                font.bold: true
                            }
                        }
                        // The comparison, once the result is opened.
                        ColumnLayout {
                            visible: cand.open
                            Layout.preferredWidth: 320
                            Layout.alignment: Qt.AlignTop
                            spacing: 2
                            Label { text: qsTr("Yours → MusicBrainz"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                            BusyIndicator { visible: cand.open && matcher.kind === "album" && !matcher.preview; running: visible; Layout.preferredHeight: 24; Layout.preferredWidth: 24 }
                            Repeater {
                                model: !cand.open ? []
                                       : matcher.kind === "album" ? (matcher.preview ? matcher.preview.tracks || [] : [])
                                       : matcher.kind === "track" ? cand.modelData.compare || [] : []
                                Label {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    elide: Text.ElideRight
                                    font.pixelSize: Theme.fontSmall
                                    color: modelData.ok ? Theme.text : Theme.focus
                                    text: modelData.label !== undefined ? modelData.label
                                          : modelData.position + "  " + modelData.title + (modelData.length ? " " + modelData.length : "") + " → "
                                            + (modelData.theirs !== null && modelData.theirs !== undefined ? modelData.theirs + (modelData.theirLength ? " " + modelData.theirLength : "") : qsTr("missing"))
                                }
                            }
                            Label {
                                visible: cand.open && matcher.kind === "album" && !!matcher.preview && matcher.preview.extra > 0
                                text: matcher.preview ? qsTr("%n track(s) on the release you do not have", "", matcher.preview.extra || 0) : ""
                                color: Theme.textDim; font.pixelSize: Theme.fontSmall
                            }
                            Label {
                                visible: cand.open && matcher.kind === "artist"
                                text: matcher.fitText(cand.modelData)
                                color: Theme.text; font.pixelSize: Theme.fontSmall
                            }
                        }
                    }
                }
            }
            RowLayout {
                visible: matcher.expanded >= 0
                Layout.fillWidth: true
                spacing: 12
                Label { objectName: "effect"; text: matcher.effectText(); color: Theme.text; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; wrapMode: Text.Wrap }
                FocusButton { objectName: "cancelChoice"; text: qsTr("Cancel"); KeyNavigation.right: useButton; onClicked: dialog.close() }
                FocusButton { id: useButton; objectName: "useMatch"; primary: true; text: qsTr("Use this match"); onClicked: matcher.choose() }
            }
            Label {
                text: qsTr("↓ to the results · Enter shows the comparison, Enter again chooses · Back closes")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
            }
        }
    }
}
