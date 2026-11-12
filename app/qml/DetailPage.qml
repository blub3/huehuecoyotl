pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Detail page for a movie, show, season or episode. A show (issue #32) has a season switcher
// (tabs, or a list for many seasons) above the selected season's episodes, including episodes
// only TMDB knows (dimmed). Seasons list their episodes. Re-queries when the library changes.
// Keys on a show: left/right switch seasons, up/down move through episodes, W marks the focused
// episode or (on the season tabs) the whole season watched or unwatched.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var itemId
    property var d: ({})
    property var request: 0
    readonly property string kind: d.kind || ""
    readonly property var childItems: d.children || []
    readonly property bool loaded: d.id !== undefined

    // Show page: the seasons (specials last), the selected one and its episodes.
    readonly property var seasons: kind === "show" ? childItems : []
    property int seasonIndex: -1
    property var seasonD: ({})
    property var seasonRequest: 0
    readonly property var season: seasonIndex >= 0 && seasonIndex < seasons.length ? seasons[seasonIndex] : null
    readonly property var episodes: kind === "show" ? (seasonD.children || []) : (kind === "season" ? childItems : [])
    readonly property bool manySeasons: seasons.length > 8

    function load() { request = Core.itemDetail(itemId) }
    function selectSeason(i) {
        if (seasons.length === 0) return
        seasonIndex = Math.max(0, Math.min(i, seasons.length - 1))
        seasonRequest = Core.itemDetail(seasons[seasonIndex].id)
    }
    // Opens on the season that holds the next episode.
    function initialSeason() {
        const n = d.nextUp
        let i = n ? seasons.findIndex(s => s.seasonNo === n.seasonNo) : -1
        if (i < 0) i = seasons.findIndex(s => s.onDisk)
        return Math.max(0, i)
    }
    function seasonBar() { return manySeasons ? seasonPicker : seasonTabs }
    function childView() {
        if (kind === "show" && seasons.length > 0) return seasonBar()
        if (kind === "season" && episodeList.count > 0) return episodeList
        return null
    }
    // Keep the focused episode row inside the visible part of the page.
    function reveal(item) {
        const p = item.mapToItem(flick.contentItem, 0, 0)
        if (p.y < flick.contentY + 40) flick.contentY = Math.max(0, p.y - 40)
        else if (p.y + item.height > flick.contentY + flick.height - 40)
            flick.contentY = Math.min(p.y + item.height - flick.height + 40, Math.max(0, flick.contentHeight - flick.height))
    }
    function play() {
        Playback.openItem(d.nextUp ? d.nextUp.id : itemId)
    }
    function metaLine() {
        const parts = []
        if (d.year > 0) parts.push(d.year)
        if (d.runtimeMin > 0) parts.push(qsTr("%1 min").arg(d.runtimeMin))
        if (d.rating > 0) parts.push("★ " + d.rating.toFixed(1))
        if (d.genres && d.genres.length) parts.push(d.genres.slice(0, 3).join(", "))
        return parts.join("  ·  ")
    }
    function playLabel() {
        if (kind === "show" || kind === "season") {
            const n = d.nextUp
            if (!n) return qsTr("Play")
            if (n.watched) return qsTr("Play again")
            if (n.progress > 0) return qsTr("Resume S%1 E%2").arg(n.seasonNo).arg(n.episodeNo)
            return qsTr("Play S%1 E%2").arg(n.seasonNo).arg(n.episodeNo)
        }
        return d.positionMs > 0 && !d.watched ? qsTr("Resume from %1").arg(Theme.time(d.positionMs / 1000)) : qsTr("Play")
    }

    Component.onCompleted: load()
    onActiveFocusChanged: if (activeFocus && loaded && !content.activeFocus) playButton.forceActiveFocus()

    Connections {
        target: Core
        function onDetailReady(requestId, detail) {
            // A reload of the same list keeps the focused episode (marking one watched reloads it).
            const keep = episodeList.currentIndex
            if (requestId === page.seasonRequest) {
                const same = page.seasonD.id === detail.id
                page.seasonD = detail
                if (same) episodeList.currentIndex = Math.min(keep, episodeList.count - 1)
                return
            }
            if (requestId !== page.request) return
            const first = !page.loaded
            page.d = detail
            if (page.kind === "season" && !first) episodeList.currentIndex = Math.min(keep, episodeList.count - 1)
            if (page.kind === "show")
                page.selectSeason(first || page.seasonIndex < 0 ? page.initialSeason() : page.seasonIndex)
            if (first) playButton.forceActiveFocus()
        }
        function onItemChanged(id) { if (id === page.itemId || page.kind === "show" || page.kind === "season") page.load() }
        function onLibraryChanged() { page.load() }
    }

    // Backdrop
    Image {
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: parent.height * 0.7
        source: page.d.backdrop || ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        opacity: 0.35
    }
    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0.0; color: "#600d1015" }
            GradientStop { position: 0.55; color: Theme.bg }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.implicitHeight + 80
        clip: true
        ScrollBar.vertical: ScrollBar {}

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
                spacing: 22

                RowLayout {
                    spacing: 32
                    Layout.fillWidth: true

                    Rectangle {
                        Layout.preferredWidth: page.kind === "episode" ? 360 : 240
                        Layout.preferredHeight: page.kind === "episode" ? 203 : 360
                        Layout.alignment: Qt.AlignTop
                        radius: Theme.radius
                        clip: true
                        color: Theme.surfaceHi
                        Label {
                            anchors.centerIn: parent
                            width: parent.width - 32
                            visible: detailPoster.status !== Image.Ready
                            text: page.d.title || ""
                            color: Theme.textDim
                            font.pixelSize: 22
                            font.bold: true
                            wrapMode: Text.Wrap
                            horizontalAlignment: Text.AlignHCenter
                        }
                        Image {
                            id: detailPoster
                            anchors.fill: parent
                            source: page.kind === "episode" ? (page.d.poster || "") : (page.d.posterFull || page.d.poster || "")
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: 10
                        Label {
                            visible: page.d.parentTitle !== undefined && page.d.parentTitle !== ""
                            text: page.kind === "episode" ? qsTr("%1 · Season %2, Episode %3").arg(page.d.parentTitle).arg(page.d.seasonNo).arg(page.d.episodeNo) : (page.d.parentTitle || "")
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
                        Label {
                            visible: text !== ""
                            text: page.loaded ? page.metaLine() : ""
                            color: Theme.textDim
                            font.pixelSize: Theme.fontBody
                        }
                        Label {
                            visible: (page.kind === "movie" || page.kind === "show") && (page.d.matchState === "needs_review" || page.d.matchState === "unmatched")
                            text: page.d.matchState === "needs_review" ? qsTr("Several matches are possible. Choose the right one with “Fix match”.") : qsTr("Not matched yet: titles come from the file names.")
                            color: Theme.accent
                            font.pixelSize: Theme.fontSmall
                        }
                        Label {
                            Layout.fillWidth: true
                            Layout.maximumWidth: 900
                            text: page.d.overview || ""
                            color: Theme.text
                            font.pixelSize: Theme.fontBody
                            wrapMode: Text.Wrap
                            maximumLineCount: 7
                            elide: Text.ElideRight
                            lineHeight: 1.2
                        }
                        Label {
                            visible: page.d.cast !== undefined && page.d.cast.length > 0
                            Layout.fillWidth: true
                            Layout.maximumWidth: 900
                            text: qsTr("With %1").arg((page.d.cast || []).slice(0, 6).join(", "))
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                            wrapMode: Text.Wrap
                        }

                        RowLayout {
                            Layout.topMargin: 10
                            spacing: 12
                            FocusButton {
                                id: playButton
                                primary: true
                                focus: true
                                glyph: "▶"
                                text: page.loaded ? page.playLabel() : qsTr("Play")
                                KeyNavigation.right: watchedButton
                                KeyNavigation.down: page.childView()
                                Keys.onLeftPressed: Nav.focusRail()
                                onClicked: page.play()
                            }
                            FocusButton {
                                id: watchedButton
                                text: page.d.watched || (page.kind !== "movie" && page.kind !== "episode" && page.d.unwatched === 0) ? qsTr("Mark unwatched") : qsTr("Mark watched")
                                KeyNavigation.left: playButton
                                KeyNavigation.right: fixButton.visible ? fixButton : null
                                KeyNavigation.down: page.childView()
                                onClicked: Core.setWatched(page.itemId, !(page.d.watched || (page.kind !== "movie" && page.kind !== "episode" && page.d.unwatched === 0)))
                            }
                            FocusButton {
                                id: fixButton
                                visible: Core.hasTmdbKey && (page.kind === "movie" || page.kind === "show")
                                text: qsTr("Fix match")
                                KeyNavigation.left: watchedButton
                                KeyNavigation.down: page.childView()
                                onClicked: fixDialog.openFor(page.itemId, page.d.title, page.d.year)
                            }
                        }
                    }
                }

                Label {
                    visible: page.childItems.length > 0
                    text: page.kind === "show" ? qsTr("Seasons") : qsTr("Episodes")
                    color: Theme.textDim
                    font.pixelSize: Theme.fontLarge
                    Layout.topMargin: 12
                }

                // Show: the season switcher. Tabs, or one button with a list for many seasons.
                RowLayout {
                    visible: page.kind === "show" && page.seasons.length > 0
                    Layout.fillWidth: true
                    spacing: 12
                    ListView {
                        id: seasonTabs
                        visible: !page.manySeasons
                        Layout.preferredWidth: Math.min(contentWidth, col.width - seasonWatched.width - 24)
                        Layout.preferredHeight: 48
                        orientation: ListView.Horizontal
                        spacing: 8
                        clip: true
                        interactive: false
                        model: page.manySeasons ? [] : page.seasons
                        currentIndex: page.seasonIndex
                        Accessible.role: Accessible.PageTabList
                        Accessible.name: qsTr("Seasons")
                        Keys.onLeftPressed: page.seasonIndex === 0 ? Nav.focusRail() : page.selectSeason(page.seasonIndex - 1)
                        Keys.onRightPressed: {
                            if (page.seasonIndex < page.seasons.length - 1) page.selectSeason(page.seasonIndex + 1)
                            else seasonWatched.forceActiveFocus()
                        }
                        Keys.onPressed: (e) => { if (e.key === Qt.Key_W) { seasonWatched.toggle(); e.accepted = true } }
                        Keys.onUpPressed: playButton.forceActiveFocus()
                        Keys.onDownPressed: if (episodeList.count > 0) episodeList.forceActiveFocus()
                        onActiveFocusChanged: if (activeFocus) page.reveal(this)
                        delegate: FocusButton {
                            id: tab
                            required property var modelData
                            required property int index
                            text: modelData.title
                            primary: index === page.seasonIndex
                            focus: ListView.isCurrentItem
                            focusPolicy: Qt.NoFocus
                            opacity: modelData.onDisk ? 1 : 0.5
                            Accessible.role: Accessible.PageTab
                            Accessible.name: modelData.title + (modelData.onDisk ? "" : ", " + qsTr("not in your library"))
                            Accessible.selected: index === page.seasonIndex
                            onClicked: page.selectSeason(index)
                        }
                    }
                    FocusButton {
                        id: seasonPicker
                        visible: page.manySeasons
                        text: (page.season ? page.season.title : "") + "  ▾"
                        tip: qsTr("Choose a season")
                        Keys.onLeftPressed: page.seasonIndex === 0 ? Nav.focusRail() : page.selectSeason(page.seasonIndex - 1)
                        Keys.onRightPressed: {
                            if (page.seasonIndex < page.seasons.length - 1) page.selectSeason(page.seasonIndex + 1)
                            else seasonWatched.forceActiveFocus()
                        }
                        Keys.onPressed: (e) => { if (e.key === Qt.Key_W) { seasonWatched.toggle(); e.accepted = true } }
                        Keys.onUpPressed: playButton.forceActiveFocus()
                        Keys.onDownPressed: if (episodeList.count > 0) episodeList.forceActiveFocus()
                        onActiveFocusChanged: if (activeFocus) page.reveal(this)
                        onClicked: seasonMenu.open()
                    }
                    Item { Layout.fillWidth: true }
                    FocusButton {
                        id: seasonWatched
                        readonly property bool done: page.season !== null && page.season.onDisk && Number(page.season.unwatched) === 0
                        visible: page.season !== null && page.season.onDisk
                        text: done ? qsTr("Mark season unwatched") : qsTr("Mark season watched")
                        Keys.onLeftPressed: page.seasonBar().forceActiveFocus()
                        Keys.onUpPressed: playButton.forceActiveFocus()
                        Keys.onDownPressed: if (episodeList.count > 0) episodeList.forceActiveFocus()
                        // W on the season tabs does the same.
                        function toggle() { if (visible) Core.setWatched(page.season.id, !done) }
                        onClicked: toggle()
                    }
                }

                // The episodes of the selected season (show) or of this season (season page).
                ListView {
                    id: episodeList
                    visible: (page.kind === "season" || page.kind === "show") && count > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: contentHeight
                    interactive: false
                    spacing: 8
                    model: page.episodes
                    keyNavigationEnabled: true
                    Keys.onUpPressed: (e) => {
                        if (currentIndex > 0) { e.accepted = false; return }
                        if (page.kind === "show") page.seasonBar().forceActiveFocus()
                        else playButton.forceActiveFocus()
                    }
                    Keys.onLeftPressed: {
                        if (page.kind === "show" && page.seasonIndex > 0) { page.selectSeason(page.seasonIndex - 1); currentIndex = 0 }
                        else Nav.focusRail()
                    }
                    Keys.onRightPressed: {
                        if (page.kind === "show" && page.seasonIndex < page.seasons.length - 1) { page.selectSeason(page.seasonIndex + 1); currentIndex = 0 }
                    }
                    onCurrentIndexChanged: if (activeFocus && currentItem) page.reveal(currentItem)
                    onActiveFocusChanged: if (activeFocus && currentItem) page.reveal(currentItem)
                    delegate: EpisodeRow {
                        required property var modelData
                        width: episodeList.width
                        ep: modelData
                        focus: ListView.isCurrentItem
                        onPlay: Playback.openItem(modelData.id)
                        onInfo: Nav.openItem(modelData.id)
                        onToggleWatched: Core.setWatched(modelData.id, !modelData.watched)
                    }
                }
            }
        }
    }

    // Many seasons: the list behind the season button.
    Popup {
        id: seasonMenu
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: 360
        height: Math.min(480, seasonMenuList.contentHeight + 24)
        modal: true
        padding: 12
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: { seasonMenuList.currentIndex = page.seasonIndex; seasonMenuList.forceActiveFocus() }
        onClosed: seasonPicker.forceActiveFocus()
        ListView {
            id: seasonMenuList
            anchors.fill: parent
            clip: true
            model: page.seasons
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Seasons")
            delegate: ItemDelegate {
                id: menuRow
                required property var modelData
                required property int index
                width: seasonMenuList.width
                focus: ListView.isCurrentItem
                opacity: modelData.onDisk ? 1 : 0.5
                text: modelData.title
                highlighted: index === page.seasonIndex
                contentItem: Label { text: menuRow.text; color: Theme.text; font.bold: menuRow.highlighted }
                background: Rectangle {
                    color: menuRow.activeFocus ? Theme.surfaceHi : "transparent"
                    radius: 6
                    border.width: menuRow.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                onClicked: { page.selectSeason(index); seasonMenu.close() }
                Keys.onReturnPressed: clicked()
                Keys.onEnterPressed: clicked()
            }
        }
    }

    FixMatchDialog {
        id: fixDialog
        anchors.centerIn: parent
    }
}
