pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Effects
import Coyotl
import "PlaylistInfo.js" as PlaylistInfo

// Music (issue #78): a top menu (Albums, Artists, Genres) over one paged grid, "Search music",
// the Quality lens (Hi-Res, Lossless, Several versions, Saved; they combine), Genre and Decade
// filters, four sorts, a format ribbon on every cover and a letter rail for A–Z jumps. Without
// a music folder it explains how to add one. Keys: Enter opens, P plays, Shift+P shuffles, Menu
// offers Play next, Add to queue and Save.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: (views.find(v => v.key === view) || { label: qsTr("Music") }).label

    // albums, artists or genres.
    property string view: "albums"
    readonly property var views: [
        { key: "albums", label: qsTr("Albums") },
        { key: "artists", label: qsTr("Artists") },
        { key: "songs", label: qsTr("Songs") },
        { key: "genres", label: qsTr("Genres") },
        { key: "playlists", label: qsTr("Playlists") }
    ]
    readonly property var lenses: [
        { key: "hires", label: qsTr("Hi-Res") },
        { key: "lossless", label: qsTr("Lossless") },
        { key: "multi", label: qsTr("Several versions") },
        { key: "saved", label: qsTr("Saved") }
    ]
    readonly property var sorts: [
        { key: "added", label: qsTr("Recently added") },
        { key: "title", label: qsTr("A–Z") },
        { key: "year", label: qsTr("Year") },
        { key: "plays", label: qsTr("Most played") }
    ]
    property var lens: ({})
    property string genre: ""
    property int decade: 0
    property string albumSort: "added"
    readonly property bool albums: view === "albums"
    readonly property bool anyFilter: view === "artists" ? artistHires
                                    : view === "songs" ? songLenses.some(l => songLens[l.key])
                                    : (lenses.some(l => lens[l.key]) || genre !== "" || decade > 0)
    // The core's filter: one token per line (crates/core/src/library.rs, parse_filter).
    // Issue #79: artists: album artists (default) or everyone credited, optionally only those with
    // Hi-Res albums.
    property bool credited: false
    property bool artistHires: false
    readonly property bool artistsView: view === "artists"
    // Issue #80: songs: the lenses Hi-Res, Lossless and Never played, and a sortable column.
    readonly property bool songsView: view === "songs"
    property var songLens: ({})
    property string songSort: "title"
    readonly property var songLenses: [
        { key: "hires", label: qsTr("Hi-Res") },
        { key: "lossless", label: qsTr("Lossless") },
        { key: "unplayed", label: qsTr("Never played") }
    ]
    // Issue #81: playlists: the smart ones and the user's, and the one in the preview.
    readonly property bool playlistsView: view === "playlists"
    property var lists: ({ smart: [], mine: [] })
    property var listsRequest: 0
    property string listKey: ""
    property var listDetail: ({})
    property var listDetailRequest: 0
    property var createRequest: 0
    function loadLists() { listsRequest = Core.playlists() }
    function previewList(key) {
        if (key === listKey) return
        listKey = key
        listTimer.restart()
    }
    function openList(key) { Nav.open(Qt.resolvedUrl("PlaylistPage.qml"), { key: key }) }
    Timer {
        id: listTimer
        interval: 150
        onTriggered: page.listDetailRequest = Core.playlistDetail(page.listKey, 7)
    }
    function playSongs(startId, shuffle) { Music.playQuery("track", songSort, filter, startId, shuffle) }
    readonly property string filter: {
        if (artistsView) return [credited ? "credited" : "", artistHires ? "hires" : ""].filter(t => t).join("\n")
        if (songsView) return songLenses.filter(l => songLens[l.key]).map(l => l.key).join("\n")
        if (!albums) return ""
        const t = lenses.filter(l => lens[l.key]).map(l => l.key)
        if (genre !== "") t.push("genre=" + genre)
        if (decade > 0) t.push("decade=" + decade)
        return t.join("\n")
    }
    property var facets: ({})
    property var facetsRequest: 0
    function refreshFacets() { facetsRequest = Core.musicFacets(artistsView ? "artist" : songsView ? "track" : "album", filter) }
    onFilterChanged: Qt.callLater(refreshFacets)
    onViewChanged: {
        Qt.callLater(refreshFacets)
        artist = ({})
        // `view` itself: the bindings on it may not have updated yet.
        if (view === "artists") artistTimer.restart()
        if (view === "playlists") loadLists()
    }
    // Back from a playlist's page: names, counts and covers may have changed.
    onVisibleChanged: if (visible && playlistsView) loadLists()

    function toggleLens(key) {
        const l = Object.assign({}, lens)
        l[key] = !l[key]
        lens = l
    }
    function clearFilters() { lens = ({}); genre = ""; decade = 0 }
    function choose(v) {
        view = v
        Qt.callLater(() => (v === "genres" ? genreGrid : v === "songs" ? songList : v === "playlists" ? smartGrid : grid).forceActiveFocus())
    }
    function lensCount(key) { return page.facets[key] ?? "" }

    // Issue #79: the artist preview panel follows the focused artist.
    property var artist: ({})
    property var artistRequest: 0
    Timer {
        id: artistTimer
        interval: 180
        onTriggered: {
            const m = lib.get(grid.currentIndex)
            if (page.artistsView && m.id !== undefined && m.id !== page.artist.id) page.artistRequest = Core.artistDetail(m.id)
        }
    }

    function open(m) { Nav.openItem(m.id, m.kind, m.parentId) }
    function play(m, shuffle) { Music.playItems([m.id], 0, shuffle) }
    function more(item, m) {
        itemMenu.target = m
        itemMenu.show(item, [
            { label: qsTr("Play next"), value: "next" },
            { label: qsTr("Add to queue"), value: "queue" },
            { label: qsTr("Add to playlist…"), value: "playlist" },
            { label: qsTr("Save on this computer"), value: "save" },
            { label: m.kind === "album" ? qsTr("Open album") : qsTr("Open artist"), value: "open" },
            // Issues #101, #102 and #105.
            { label: qsTr("Match again"), value: "match" },
            { label: qsTr("Search MusicBrainz…"), value: "search" }
        ])
    }

    // Shared by the Genre, Decade and Sort buttons.
    function showChooser(item, mode) {
        chooser.mode = mode
        let list = []
        if (mode === "genre") {
            list = [{ label: qsTr("All genres"), value: "" }]
                .concat((facets.genres || []).map(g => ({ label: g[0] + "  ·  " + g[1], value: g[0] })))
        } else if (mode === "decade") {
            list = [{ label: qsTr("All decades"), value: 0 }]
                .concat((facets.decades || []).map(d => ({ label: qsTr("%1s").arg(d[0]) + "  ·  " + d[1], value: d[0] })))
        } else {
            list = sorts.map(s => ({ label: s.label, value: s.key }))
        }
        chooser.show(item, list)
    }

    LibraryModel {
        id: lib
        kind: page.view === "artists" ? "artist" : page.view === "songs" ? "track" : "album"
        sort: page.view === "artists" ? "title" : page.view === "songs" ? page.songSort : page.albumSort
        filter: page.filter
        onTotalChanged: {
            if (grid.currentIndex < 0 && grid.count > 0) grid.currentIndex = 0
            if (page.artistsView) artistTimer.restart()
            if (((Bench.mode === "music" && page.albums) || (Bench.mode === "songs" && page.songsView)) && total > 0 && !benchScroll.running) benchStart.start()
        }
        // The letter rail asked for a row further down.
        onRowLoaded: (row) => {
            grid.currentIndex = row
            grid.positionViewAtIndex(row, GridView.Beginning)
            grid.forceActiveFocus()
        }
    }

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === page.listsRequest) {
                page.lists = detail
                if (page.listKey === "" && (detail.smart || []).length > 0) page.previewList(detail.smart[0].key)
            }
            if (requestId === page.listDetailRequest) page.listDetail = detail
            if (requestId === page.createRequest) {
                page.createRequest = 0
                page.loadLists()
                page.openList(detail.card.key)
            }
            if (requestId === page.artistRequest) page.artist = detail
            if (requestId !== page.facetsRequest) return
            page.facets = detail
        }
    }

    // --benchmark music: scroll the Albums grid at a steady speed and record frame times.
    Timer { id: benchStart; interval: 1500; onTriggered: { Bench.beginFrames(); benchScroll.start() } }
    // --benchmark songs (issue #80): the same over the Songs list.
    FrameAnimation {
        id: benchScroll
        property real travelled: 0
        readonly property Flickable view: page.songsView ? songList : grid
        onTriggered: {
            const step = Math.min(frameTime, 0.05) * 4000
            const max = view.originY + view.contentHeight - view.height
            view.contentY = Math.min(view.contentY + step, max)
            travelled += step
            if (elapsedTime > 20 || (view.contentY >= max && lib.rowCount() >= lib.total)) {
                stop()
                Bench.endFrames(lib.rowCount(), travelled)
            }
        }
    }

    Component.onCompleted: {
        if (Bench.mode === "songs") view = "songs"
        refreshFacets()
        grid.forceActiveFocus()
    }
    onActiveFocusChanged: if (activeFocus && !header.activeFocus && !lensRow.activeFocus && !artistRow.activeFocus && !songRow.activeFocus && !letterRail.activeFocus && !panel.activeFocus)
                              (view === "genres" ? genreGrid : view === "songs" ? songList : view === "playlists" ? smartGrid : grid).forceActiveFocus()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        anchors.rightMargin: 12
        spacing: 14

        // ---- the top menu and search -----------------------------------------------------
        FocusScope {
            id: header
            Layout.fillWidth: true
            Layout.preferredHeight: 48
            RowLayout {
                anchors.fill: parent
                spacing: 20
                Label {
                    text: qsTr("Music")
                    color: Theme.text
                    font.pixelSize: Theme.fontTitle
                    font.bold: true
                    Accessible.role: Accessible.Heading
                }
                Item { Layout.fillWidth: true }
                Rectangle {
                    Layout.preferredHeight: 48
                    Layout.preferredWidth: menuRow.implicitWidth + 8
                    radius: 12
                    color: Theme.surface
                    Row {
                        id: menuRow
                        anchors.centerIn: parent
                        spacing: 4
                        Repeater {
                            id: menu
                            model: page.views
                            FocusButton {
                                required property var modelData
                                required property int index
                                text: modelData.label
                                primary: page.view === modelData.key
                                font.pixelSize: Theme.fontSmall
                                Accessible.role: Accessible.PageTab
                                Accessible.selected: page.view === modelData.key
                                Keys.onLeftPressed: index === 0 ? Nav.focusRail() : menu.itemAt(index - 1).forceActiveFocus()
                                Keys.onRightPressed: index === page.views.length - 1 ? search.forceActiveFocus() : menu.itemAt(index + 1).forceActiveFocus()
                                Keys.onDownPressed: page.albums ? lensRow.focusFirst() : page.artistsView ? artistRow.focusFirst() : page.songsView ? songRow.focusFirst() : page.playlistsView ? smartGrid.forceActiveFocus() : genreGrid.forceActiveFocus()
                                onClicked: page.choose(modelData.key)
                            }
                        }
                    }
                }
                Item { Layout.fillWidth: true }
                TextField {
                    id: search
                    Layout.preferredWidth: 240
                    placeholderText: qsTr("Search music")
                    Accessible.name: qsTr("Search music")
                    Keys.onLeftPressed: (e) => { if (cursorPosition === 0) menu.itemAt(page.views.length - 1).forceActiveFocus(); else e.accepted = false }
                    Keys.onDownPressed: page.albums ? lensRow.focusFirst() : grid.forceActiveFocus()
                    onAccepted: if (text.trim() !== "") Nav.open(Qt.resolvedUrl("SearchPage.qml"), { initialText: text.trim() })
                }
            }
        }

        // ---- the Quality lens, genre, decade and sort ------------------------------------
        FocusScope {
            id: lensRow
            visible: page.albums
            Layout.fillWidth: true
            Layout.preferredHeight: visible ? 40 : 0
            function focusFirst() { allChip.forceActiveFocus() }
            RowLayout {
                anchors.fill: parent
                spacing: 8
                Label {
                    // Only where the row has room for it.
                    visible: lensRow.width > 1180
                    text: qsTr("Quality lens")
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 1
                    Layout.rightMargin: 4
                }
                FocusButton {
                    id: allChip
                    text: qsTr("All") + (page.facets.all !== undefined ? "  " + page.facets.all : "")
                    primary: !page.lenses.some(l => page.lens[l.key])
                    font.pixelSize: Theme.fontSmall
                    Accessible.checked: primary
                    Keys.onLeftPressed: Nav.focusRail()
                    Keys.onRightPressed: chips.itemAt(0).forceActiveFocus()
                    Keys.onUpPressed: menu.itemAt(0).forceActiveFocus()
                    Keys.onDownPressed: grid.forceActiveFocus()
                    onClicked: page.lens = ({})
                }
                Repeater {
                    id: chips
                    model: page.lenses
                    FocusButton {
                        required property var modelData
                        required property int index
                        text: modelData.label + "  " + page.lensCount(modelData.key)
                        primary: page.lens[modelData.key] === true
                        font.pixelSize: Theme.fontSmall
                        Accessible.role: Accessible.CheckBox
                        Accessible.checked: primary
                        Keys.onLeftPressed: index === 0 ? allChip.forceActiveFocus() : chips.itemAt(index - 1).forceActiveFocus()
                        Keys.onRightPressed: index === page.lenses.length - 1 ? genreButton.forceActiveFocus() : chips.itemAt(index + 1).forceActiveFocus()
                        Keys.onUpPressed: menu.itemAt(0).forceActiveFocus()
                        Keys.onDownPressed: grid.forceActiveFocus()
                        onClicked: page.toggleLens(modelData.key)
                    }
                }
                Item { Layout.fillWidth: true }
                FocusButton {
                    id: genreButton
                    text: (page.genre === "" ? qsTr("Genre") : page.genre) + "  ▾"
                    tip: qsTr("Genre")
                    font.pixelSize: Theme.fontSmall
                    Keys.onLeftPressed: chips.itemAt(page.lenses.length - 1).forceActiveFocus()
                    Keys.onRightPressed: decadeButton.forceActiveFocus()
                    Keys.onUpPressed: search.forceActiveFocus()
                    Keys.onDownPressed: grid.forceActiveFocus()
                    onClicked: page.showChooser(this, "genre")
                }
                FocusButton {
                    id: decadeButton
                    text: (page.decade === 0 ? qsTr("Decade") : qsTr("%1s").arg(page.decade)) + "  ▾"
                    tip: qsTr("Decade")
                    font.pixelSize: Theme.fontSmall
                    Keys.onLeftPressed: genreButton.forceActiveFocus()
                    Keys.onRightPressed: sortButton.forceActiveFocus()
                    Keys.onUpPressed: search.forceActiveFocus()
                    Keys.onDownPressed: grid.forceActiveFocus()
                    onClicked: page.showChooser(this, "decade")
                }
                FocusButton {
                    id: sortButton
                    text: (page.sorts.find(s => s.key === page.albumSort) ?? page.sorts[0]).label + "  ▾"
                    tip: qsTr("Sort")
                    font.pixelSize: Theme.fontSmall
                    Keys.onLeftPressed: decadeButton.forceActiveFocus()
                    Keys.onUpPressed: search.forceActiveFocus()
                    Keys.onDownPressed: grid.forceActiveFocus()
                    onClicked: page.showChooser(this, "sort")
                }
            }
        }
        // Issue #80: songs: lenses and play buttons.
        FocusScope {
            id: songRow
            visible: page.songsView
            Layout.fillWidth: true
            Layout.preferredHeight: visible ? 40 : 0
            function focusFirst() { songChips.itemAt(0).forceActiveFocus() }
            RowLayout {
                anchors.fill: parent
                spacing: 8
                Repeater {
                    id: songChips
                    model: page.songLenses
                    FocusButton {
                        required property var modelData
                        required property int index
                        text: modelData.label + (page.songsView && page.facets[modelData.key] !== undefined ? "  " + page.facets[modelData.key] : "")
                        primary: page.songLens[modelData.key] === true
                        font.pixelSize: Theme.fontSmall
                        Accessible.role: Accessible.CheckBox
                        Accessible.checked: primary
                        Keys.onLeftPressed: index === 0 ? Nav.focusRail() : songChips.itemAt(index - 1).forceActiveFocus()
                        Keys.onRightPressed: index === page.songLenses.length - 1 ? playThese.forceActiveFocus() : songChips.itemAt(index + 1).forceActiveFocus()
                        Keys.onUpPressed: menu.itemAt(2).forceActiveFocus()
                        Keys.onDownPressed: songList.forceActiveFocus()
                        onClicked: {
                            const l = Object.assign({}, page.songLens)
                            l[modelData.key] = !l[modelData.key]
                            page.songLens = l
                        }
                    }
                }
                Item { Layout.fillWidth: true }
                Label {
                    text: qsTr("%n song(s)", "", lib.total)
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
                FocusButton {
                    id: playThese
                    primary: true
                    glyph: "▶"
                    text: qsTr("Play these")
                    enabled: lib.total > 0
                    font.pixelSize: Theme.fontSmall
                    Keys.onLeftPressed: songChips.itemAt(page.songLenses.length - 1).forceActiveFocus()
                    Keys.onRightPressed: shuffleThese.forceActiveFocus()
                    Keys.onUpPressed: search.forceActiveFocus()
                    Keys.onDownPressed: songList.forceActiveFocus()
                    onClicked: page.playSongs(0, false)
                }
                FocusButton {
                    id: shuffleThese
                    text: qsTr("Shuffle")
                    enabled: lib.total > 0
                    font.pixelSize: Theme.fontSmall
                    Keys.onLeftPressed: playThese.forceActiveFocus()
                    Keys.onUpPressed: search.forceActiveFocus()
                    Keys.onDownPressed: songList.forceActiveFocus()
                    onClicked: page.playSongs(0, true)
                }
            }
        }

        // Issue #79: which artists.
        FocusScope {
            id: artistRow
            visible: page.artistsView
            Layout.fillWidth: true
            Layout.preferredHeight: visible ? 40 : 0
            function focusFirst() { albumArtists.forceActiveFocus() }
            RowLayout {
                anchors.fill: parent
                spacing: 8
                FocusButton {
                    id: albumArtists
                    text: qsTr("Album artists")
                    primary: !page.credited
                    font.pixelSize: Theme.fontSmall
                    Accessible.checked: primary
                    Keys.onLeftPressed: Nav.focusRail()
                    Keys.onRightPressed: everyone.forceActiveFocus()
                    Keys.onUpPressed: menu.itemAt(1).forceActiveFocus()
                    Keys.onDownPressed: grid.forceActiveFocus()
                    onClicked: page.credited = false
                }
                FocusButton {
                    id: everyone
                    text: qsTr("Everyone credited")
                    primary: page.credited
                    font.pixelSize: Theme.fontSmall
                    Accessible.checked: primary
                    Keys.onLeftPressed: albumArtists.forceActiveFocus()
                    Keys.onRightPressed: hiresArtists.forceActiveFocus()
                    Keys.onUpPressed: menu.itemAt(1).forceActiveFocus()
                    Keys.onDownPressed: grid.forceActiveFocus()
                    onClicked: page.credited = true
                }
                FocusButton {
                    id: hiresArtists
                    text: qsTr("With Hi-Res albums") + (page.facets.hires !== undefined && page.artistsView ? "  " + page.facets.hires : "")
                    primary: page.artistHires
                    font.pixelSize: Theme.fontSmall
                    Accessible.role: Accessible.CheckBox
                    Accessible.checked: primary
                    Keys.onLeftPressed: everyone.forceActiveFocus()
                    Keys.onUpPressed: menu.itemAt(1).forceActiveFocus()
                    Keys.onDownPressed: grid.forceActiveFocus()
                    onClicked: page.artistHires = !page.artistHires
                }
                Item { Layout.fillWidth: true }
                Label {
                    text: qsTr("Enter opens · P plays all · Shift+P shuffles")
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
            }
        }
        Label {
            visible: page.albums && page.anyFilter && lib.total > 0
            text: qsTr("%1 of %2 albums").arg(lib.total).arg(page.facets.all ?? "…")
                  + (page.anyFilter ? "  ·  " + qsTr("Filters combine; All clears the lenses.") : "")
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
        }

        // ---- the grid and the letter rail ------------------------------------------------
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 8
            visible: page.albums || page.artistsView

            GridView {
                id: grid
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                focus: true
                model: lib
                readonly property real gap: Math.round(26 * Appearance.textScale(Appearance.coverWidth))
                // Artists share the width with their preview panel: smaller tiles.
                readonly property real target: page.artistsView ? Math.round(Appearance.coverWidth * 0.75) : Appearance.coverWidth
                readonly property int columns: Math.max(1, Math.floor((width - leftMargin) / (target + gap)))
                readonly property real posterWidth: Math.floor((width - leftMargin) / columns) - gap
                cellWidth: posterWidth + gap
                cellHeight: Appearance.cardHeight(posterWidth, 1) + gap
                onCellWidthChanged: Qt.callLater(() => { if (currentIndex >= 0) positionViewAtIndex(currentIndex, GridView.Contain) })
                onCurrentIndexChanged: if (page.artistsView) artistTimer.restart()
                topMargin: 14
                leftMargin: 10
                cacheBuffer: 1200
                reuseItems: true
                keyNavigationEnabled: true
                highlightFollowsCurrentItem: true
                highlightMoveDuration: 80
                Accessible.role: Accessible.List
                Accessible.name: page.albums ? qsTr("Albums") : qsTr("Artists")
                ScrollBar.vertical: ScrollBar {}

                Keys.onLeftPressed: (e) => { if (currentIndex % columns === 0) Nav.focusRail(); else e.accepted = false }
                Keys.onRightPressed: (e) => {
                    const last = currentIndex % columns === columns - 1 || currentIndex === count - 1
                    if (last && panel.visible) playAll.forceActiveFocus()
                    else if (last && letterRail.visible) letterRail.focusNear()
                    else e.accepted = false
                }
                Keys.onUpPressed: (e) => {
                    if (currentIndex >= columns) { e.accepted = false; return }
                    if (page.albums) lensRow.focusFirst()
                    else artistRow.focusFirst()
                }

                delegate: PosterCard {
                    id: tile
                    required property int index
                    required property var model
                    title: model.title
                    year: model.year
                    poster: model.poster
                    posterLarge: model.posterLarge ?? ""
                    posterWidth: grid.posterWidth
                    posterRatio: 1
                    caption: page.artistsView
                             ? qsTr("%n album(s)", "", Number(tile.model.albums ?? 0)) + (Number(tile.model.hiresAlbums ?? 0) > 0 ? "  ·  " + qsTr("%n Hi-Res", "", Number(tile.model.hiresAlbums)) : "")
                             : (tile.model.subtitle ?? "") + (page.albums && tile.model.year > 0 ? "  ·  " + tile.model.year : "")
                    round: page.artistsView
                    progress: 0
                    saved: model.saved === true
                    versions: Number(model.versions ?? 0)
                    ribbon: model.format ?? ""
                    ribbonGold: model.hires === true
                    quickActions: true
                    focus: GridView.isCurrentItem
                    onClicked: { grid.currentIndex = tile.index; page.open(tile.model) }
                    onPlayRequested: page.play(tile.model, false)
                    onShuffleRequested: page.play(tile.model, true)
                    onMoreRequested: page.more(tile, tile.model)
                    Keys.onReturnPressed: page.open(tile.model)
                    Keys.onEnterPressed: page.open(tile.model)
                    Keys.onMenuPressed: page.more(tile, tile.model)
                    Keys.onPressed: (e) => {
                        if (e.key === Qt.Key_P || e.key === Qt.Key_MediaPlay) {
                            page.play(tile.model, (e.modifiers & Qt.ShiftModifier) !== 0)
                            e.accepted = true
                        }
                    }
                }
            }

            // Issue #79: the focused artist at a glance.
            Rectangle {
                id: panel
                visible: page.artistsView && lib.total > 0
                Layout.preferredWidth: visible ? 360 : 0
                Layout.fillHeight: true
                Layout.bottomMargin: 8
                radius: 14
                color: Theme.surface
                clip: true
                Accessible.role: Accessible.Pane
                Accessible.name: qsTr("Artist preview")
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 12
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.preferredWidth: 150
                        Layout.preferredHeight: 150
                        radius: 75
                        color: Qt.hsla(((page.artist.title || "").length * 37 % 360) / 360, 0.30, 0.30, 1)
                        Label {
                            anchors.centerIn: parent
                            visible: artistImage.status !== Image.Ready
                            text: (page.artist.title || "").split(" ").filter(w => w).slice(0, 2).map(w => w.charAt(0)).join("").toUpperCase()
                            color: Theme.text
                            font.pixelSize: 44
                            font.bold: true
                        }
                        Image {
                            id: artistImage
                            anchors.fill: parent
                            source: page.artist.image || ""
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                            sourceSize.width: 300
                            layer.enabled: true
                            layer.effect: MultiEffect { maskEnabled: true; maskSource: panelMask }
                        }
                        Rectangle { id: panelMask; anchors.fill: parent; radius: width / 2; visible: false; layer.enabled: true }
                    }
                    Label {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        text: page.artist.title || ""
                        color: Theme.text
                        font.pixelSize: Theme.fontLarge + 4
                        font.bold: true
                        elide: Text.ElideRight
                        Accessible.role: Accessible.Heading
                    }
                    RowLayout {
                        Layout.alignment: Qt.AlignHCenter
                        spacing: 8
                        FocusButton {
                            id: playAll
                            primary: true
                            glyph: "▶"
                            text: qsTr("Play all")
                            font.pixelSize: Theme.fontSmall
                            Keys.onLeftPressed: grid.forceActiveFocus()
                            Keys.onRightPressed: shuffleAll.forceActiveFocus()
                            Keys.onUpPressed: artistRow.focusFirst()
                            onClicked: if (page.artist.id) Music.playItems([page.artist.id], 0, false)
                        }
                        FocusButton {
                            id: shuffleAll
                            text: qsTr("Shuffle")
                            font.pixelSize: Theme.fontSmall
                            Keys.onLeftPressed: playAll.forceActiveFocus()
                            Keys.onRightPressed: openArtist.forceActiveFocus()
                            Keys.onUpPressed: artistRow.focusFirst()
                            onClicked: if (page.artist.id) Music.playItems([page.artist.id], 0, true)
                        }
                        FocusButton {
                            id: openArtist
                            text: qsTr("Open page")
                            font.pixelSize: Theme.fontSmall
                            Keys.onLeftPressed: shuffleAll.forceActiveFocus()
                            Keys.onRightPressed: if (letterRail.visible) letterRail.focusNear()
                            Keys.onUpPressed: artistRow.focusFirst()
                            onClicked: if (page.artist.id) Nav.openItem(page.artist.id, "artist")
                        }
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: (page.artist.biography || "") !== ""
                        text: page.artist.biography || ""
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                        wrapMode: Text.Wrap
                        maximumLineCount: 6
                        elide: Text.ElideRight
                        lineHeight: 1.15
                    }
                    Label {
                        text: qsTr("Albums")
                        visible: (page.artist.albums || []).length > 0
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                        font.capitalization: Font.AllUppercase
                        font.letterSpacing: 1
                    }
                    Repeater {
                        model: (page.artist.albums || []).slice(0, 6)
                        RowLayout {
                            id: rel
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: 10
                            Rectangle {
                                Layout.preferredWidth: 40
                                Layout.preferredHeight: 40
                                radius: 5
                                clip: true
                                color: Theme.surfaceHi
                                Image { anchors.fill: parent; source: rel.modelData.poster || ""; fillMode: Image.PreserveAspectCrop; asynchronous: true; sourceSize.width: 80 }
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label { Layout.fillWidth: true; text: rel.modelData.title; color: Theme.text; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                                Label { text: rel.modelData.year > 0 ? String(rel.modelData.year) : ""; color: Theme.textDim; font.pixelSize: 12 }
                            }
                            Rectangle {
                                visible: rel.modelData.hires === true
                                implicitWidth: hiresLabel.implicitWidth + 10
                                implicitHeight: 20
                                radius: 5
                                color: Theme.accent
                                Label { id: hiresLabel; anchors.centerIn: parent; text: "Hi-Res"; color: Theme.bg; font.pixelSize: 11; font.bold: true }
                            }
                        }
                    }
                    Label {
                        visible: (page.artist.appearsOn || []).length > 0
                        text: qsTr("Appears on %n album(s)", "", (page.artist.appearsOn || []).length)
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                    }
                    Item { Layout.fillHeight: true }
                }
            }

            // A–Z: jump to the first item with that initial (sorted by title only).
            FocusScope {
                id: letterRail
                visible: lib.sort === "title" && (page.facets.letters || []).length > 1
                Layout.preferredWidth: visible ? 26 : 0
                Layout.fillHeight: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Jump to letter")
                readonly property var all: ["#"].concat("ABCDEFGHIJKLMNOPQRSTUVWXYZ".split(""))
                readonly property var offsets: {
                    const m = {}
                    for (const l of (page.facets.letters || [])) m[l[0]] = l[1]
                    return m
                }
                function focusNear() {
                    const item = lib.get(grid.currentIndex)
                    const t = (item.title || "").trim().toUpperCase()
                    const i = Math.max(0, all.indexOf(t.charAt(0)))
                    letters.itemAt(i).forceActiveFocus()
                }
                function jump(l) { if (offsets[l] !== undefined) lib.loadTo(offsets[l]) }
                function has(i) { return offsets[all[i]] !== undefined }
                Column {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 0
                    Repeater {
                        id: letters
                        model: letterRail.all
                        ItemDelegate {
                            id: letter
                            required property string modelData
                            required property int index
                            readonly property bool has: letterRail.offsets[modelData] !== undefined
                            width: 26
                            height: Math.max(18, Math.min(26, (letterRail.height - 4) / 27))
                            padding: 0
                            enabled: has
                            focusPolicy: Qt.StrongFocus
                            Accessible.name: modelData
                            contentItem: Label {
                                text: letter.modelData
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                                font.pixelSize: 12
                                font.bold: letter.activeFocus
                                color: letter.activeFocus ? Theme.bg : letter.has ? Theme.text : "#55606e"
                            }
                            background: Rectangle { radius: 4; color: letter.activeFocus ? Theme.focus : "transparent" }
                            onClicked: letterRail.jump(modelData)
                            Keys.onReturnPressed: letterRail.jump(modelData)
                            Keys.onEnterPressed: letterRail.jump(modelData)
                            Keys.onUpPressed: { for (let i = index - 1; i >= 0; --i) if (letterRail.has(i)) { letters.itemAt(i).forceActiveFocus(); return } }
                            Keys.onDownPressed: { for (let i = index + 1; i < letters.count; ++i) if (letterRail.has(i)) { letters.itemAt(i).forceActiveFocus(); return } }
                            Keys.onLeftPressed: panel.visible ? openArtist.forceActiveFocus() : grid.forceActiveFocus()
                        }
                    }
                }
            }
        }

        // ---- songs (issue #80) -------------------------------------------------------------------
        ColumnLayout {
            visible: page.songsView
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0
            // Column headers sort; Quality sorts best first.
            RowLayout {
                id: songHeader
                Layout.fillWidth: true
                Layout.preferredHeight: 36
                Layout.leftMargin: 14
                Layout.rightMargin: 14
                spacing: 16
                readonly property var cols: [
                    { key: "", label: "#", w: 40 },
                    { key: "title", label: qsTr("Title"), fill: 22 },
                    { key: "artist", label: qsTr("Artist"), fill: 14 },
                    { key: "album", label: qsTr("Album"), fill: 16 },
                    { key: "duration", label: qsTr("Time"), w: 64 },
                    { key: "quality", label: qsTr("Quality"), w: 130 },
                    { key: "plays", label: qsTr("Plays"), w: 56 }
                ]
                Repeater {
                    model: songHeader.cols
                    ItemDelegate {
                        id: col
                        required property var modelData
                        // Stretching columns share the width by their factors only, as in the rows.
                        Layout.preferredWidth: modelData.w ?? 0
                        Layout.minimumWidth: modelData.w ?? 0
                        Layout.fillWidth: modelData.fill !== undefined
                        Layout.horizontalStretchFactor: modelData.fill ?? -1
                        Layout.preferredHeight: 32
                        padding: 0
                        enabled: modelData.key !== ""
                        focusPolicy: Qt.NoFocus
                        Accessible.role: Accessible.ColumnHeader
                        contentItem: Label {
                            text: col.modelData.label + (page.songSort === col.modelData.key ? (col.modelData.key === "title" || col.modelData.key === "artist" || col.modelData.key === "album" ? "  ↑" : "  ↓") : "")
                            color: page.songSort === col.modelData.key ? Theme.focus : Theme.textDim
                            font.pixelSize: 12
                            font.capitalization: Font.AllUppercase
                            font.letterSpacing: 0.8
                            font.bold: page.songSort === col.modelData.key
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Item {}
                        onClicked: page.songSort = modelData.key
                    }
                }
            }
            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.surfaceHi }
            ListView {
                id: songList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: page.songsView ? lib : null
                reuseItems: true
                cacheBuffer: 800
                keyNavigationEnabled: true
                highlightFollowsCurrentItem: true
                highlightMoveDuration: 60
                Accessible.role: Accessible.Table
                Accessible.name: qsTr("Songs")
                ScrollBar.vertical: ScrollBar {}
                Keys.onUpPressed: (e) => { if (currentIndex <= 0) songRow.focusFirst(); else e.accepted = false }
                Keys.onLeftPressed: Nav.focusRail()
                // S cycles the sort through the columns.
                Keys.onPressed: (e) => {
                    if (e.key === Qt.Key_S) {
                        const keys = ["title", "artist", "album", "duration", "quality", "plays"]
                        page.songSort = keys[(keys.indexOf(page.songSort) + 1) % keys.length]
                        e.accepted = true
                    }
                }
                delegate: ItemDelegate {
                    id: song
                    required property int index
                    required property var model
                    width: songList.width
                    height: 44
                    readonly property bool now: Music.current.id === song.model.id
                    readonly property bool focused: songList.activeFocus && songList.currentIndex === index
                    Accessible.role: Accessible.Row
                    Accessible.name: [song.model.title, song.model.artist, song.model.subtitle, song.model.format].filter(x => x).join(", ")
                    background: Rectangle {
                        radius: 8
                        color: song.focused ? Theme.surfaceHi : song.now ? "#1fe3a33b" : "transparent"
                        border.width: song.focused ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        spacing: 16
                        Label {
                            Layout.preferredWidth: 40
                            text: song.now ? "▶" : String(song.index + 1)
                            color: song.now ? Theme.accent : Theme.textDim
                            font.pixelSize: Theme.fontSmall
                        }
                        Label { Layout.fillWidth: true; Layout.preferredWidth: 0; Layout.horizontalStretchFactor: 22; text: song.model.title; color: song.now ? Theme.accent : Theme.text; elide: Text.ElideRight; font.bold: true }
                        Label { Layout.fillWidth: true; Layout.preferredWidth: 0; Layout.horizontalStretchFactor: 14; text: song.model.artist ?? ""; color: Theme.textDim; elide: Text.ElideRight }
                        Label { Layout.fillWidth: true; Layout.preferredWidth: 0; Layout.horizontalStretchFactor: 16; text: song.model.subtitle ?? ""; color: Theme.textDim; elide: Text.ElideRight }
                        Label { Layout.preferredWidth: 64; text: Number(song.model.durationMs ?? 0) > 0 ? Theme.time(Number(song.model.durationMs) / 1000) : ""; color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                        Item {
                            Layout.preferredWidth: 130
                            Layout.preferredHeight: 22
                            Rectangle {
                                visible: (song.model.format ?? "") !== ""
                                width: fmtLabel.implicitWidth + 12
                                height: 22
                                radius: 6
                                anchors.verticalCenter: parent.verticalCenter
                                color: song.model.hires === true ? Theme.accent : Theme.surfaceHi
                                Label { id: fmtLabel; anchors.centerIn: parent; text: song.model.format ?? ""; font.pixelSize: 11; font.bold: true; color: song.model.hires === true ? Theme.bg : Theme.text }
                            }
                        }
                        Label { Layout.preferredWidth: 56; text: Number(song.model.plays ?? 0) > 0 ? String(song.model.plays) : "–"; color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    }
                    onClicked: { songList.currentIndex = index; page.playSongs(song.model.id, false) }
                    Keys.onReturnPressed: page.playSongs(song.model.id, false)
                    Keys.onEnterPressed: page.playSongs(song.model.id, false)
                    Keys.onMenuPressed: {
                        songMenu.target = song.model
                        songMenu.show(song, [
                            { label: qsTr("Play next"), value: "next" },
                            { label: qsTr("Add to queue"), value: "queue" },
                            { label: qsTr("Add to playlist…"), value: "playlist" },
                            { label: qsTr("Go to album"), value: "album" },
                            // Issues #103 and #104.
                            { label: qsTr("Match again"), value: "match" },
                            { label: qsTr("Search MusicBrainz…"), value: "search" }
                        ])
                    }
                }
            }
        }

        // ---- playlists (issue #81) -----------------------------------------------------------
        RowLayout {
            visible: page.playlistsView
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 24
            Flickable {
                id: listsFlick
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentHeight: listsCol.implicitHeight + 24
                ScrollBar.vertical: ScrollBar {}
                function reveal(item) {
                    const p = item.mapToItem(contentItem, 0, 0)
                    if (p.y < contentY) contentY = Math.max(0, p.y - 20)
                    else if (p.y + item.height > contentY + height) contentY = Math.min(p.y + item.height - height + 20, Math.max(0, contentHeight - height))
                }
                ColumnLayout {
                    id: listsCol
                    width: listsFlick.width
                    spacing: 12
                    RowLayout {
                        spacing: 12
                        Label { text: qsTr("Smart playlists"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; Accessible.role: Accessible.Heading }
                        Label { text: qsTr("They fill themselves from your library."); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    }
                    GridView {
                        id: smartGrid
                        Layout.fillWidth: true
                        // All rows, measured from the count (contentHeight stays 0 while the
                        // view has no height of its own).
                        Layout.preferredHeight: Math.ceil(count / columns) * cellHeight
                        interactive: false
                        model: page.lists.smart || []
                        readonly property int columns: Math.max(1, Math.floor(width / 300))
                        cellWidth: Math.floor(width / columns)
                        cellHeight: 100
                        keyNavigationEnabled: true
                        Accessible.role: Accessible.List
                        Accessible.name: qsTr("Smart playlists")
                        onCurrentIndexChanged: if (activeFocus && currentItem) { page.previewList(page.lists.smart[currentIndex].key); listsFlick.reveal(currentItem) }
                        onActiveFocusChanged: if (activeFocus && currentItem) { page.previewList(page.lists.smart[currentIndex].key); listsFlick.reveal(currentItem) }
                        Keys.onLeftPressed: (e) => { if (currentIndex % columns === 0) Nav.focusRail(); else e.accepted = false }
                        Keys.onRightPressed: (e) => { if (currentIndex % columns === columns - 1 || currentIndex === count - 1) listPlay.forceActiveFocus(); else e.accepted = false }
                        Keys.onUpPressed: (e) => { if (currentIndex < columns) menu.itemAt(4).forceActiveFocus(); else e.accepted = false }
                        Keys.onDownPressed: (e) => { if (currentIndex + columns >= count) mineGrid.forceActiveFocus(); else e.accepted = false }
                        delegate: ItemDelegate {
                            id: smartCard
                            required property var modelData
                            required property int index
                            width: smartGrid.cellWidth - 14
                            height: 86
                            focus: GridView.isCurrentItem
                            Accessible.name: PlaylistInfo.name(modelData.smart) + ", " + qsTr("%n song(s)", "", modelData.count)
                            background: Rectangle {
                                radius: Theme.radius
                                color: Theme.surface
                                border.width: smartCard.activeFocus ? Theme.focusWidth + 1 : (page.listKey === smartCard.modelData.key ? 1 : 0)
                                border.color: smartCard.activeFocus ? Theme.focus : Theme.surfaceHi
                            }
                            contentItem: RowLayout {
                                spacing: 14
                                Rectangle {
                                    Layout.preferredWidth: 52
                                    Layout.preferredHeight: 52
                                    radius: 12
                                    color: smartCard.modelData.smart === "hires" ? Theme.accent : Theme.surfaceHi
                                    Label {
                                        anchors.centerIn: parent
                                        text: PlaylistInfo.glyph(smartCard.modelData.smart)
                                        color: smartCard.modelData.smart === "hires" ? Theme.bg : Theme.accent
                                        font.pixelSize: 18
                                        font.bold: true
                                    }
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2
                                    Label { Layout.fillWidth: true; text: PlaylistInfo.name(smartCard.modelData.smart); color: Theme.text; font.bold: true; elide: Text.ElideRight }
                                    Label { Layout.fillWidth: true; text: PlaylistInfo.rule(smartCard.modelData.smart); color: Theme.textDim; font.pixelSize: 12; elide: Text.ElideRight }
                                    Label { text: qsTr("%n song(s)", "", smartCard.modelData.count); color: Theme.textDim; font.pixelSize: 12 }
                                }
                            }
                            onClicked: { smartGrid.currentIndex = index; page.openList(modelData.key) }
                            Keys.onReturnPressed: page.openList(modelData.key)
                            Keys.onEnterPressed: page.openList(modelData.key)
                            Keys.onPressed: (e) => { if (e.key === Qt.Key_P) { Music.playPlaylist(modelData.key, 0, (e.modifiers & Qt.ShiftModifier) !== 0); e.accepted = true } }
                        }
                    }
                    RowLayout {
                        Layout.topMargin: 12
                        spacing: 12
                        Label { text: qsTr("Your playlists"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; Accessible.role: Accessible.Heading }
                        Label { text: qsTr("Menu on any album or song: Add to playlist."); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    }
                    GridView {
                        id: mineGrid
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.ceil(count / columns) * cellHeight
                        interactive: false
                        // The first tile makes a new playlist.
                        model: [{ key: "new" }].concat(page.lists.mine || [])
                        readonly property int columns: Math.max(1, Math.floor(width / 200))
                        cellWidth: Math.floor(width / columns)
                        cellHeight: cellWidth + 50
                        keyNavigationEnabled: true
                        Accessible.role: Accessible.List
                        Accessible.name: qsTr("Your playlists")
                        onCurrentIndexChanged: if (activeFocus && currentItem) { if (currentIndex > 0) page.previewList(page.lists.mine[currentIndex - 1].key); listsFlick.reveal(currentItem) }
                        onActiveFocusChanged: if (activeFocus && currentItem) { if (currentIndex > 0) page.previewList(page.lists.mine[currentIndex - 1].key); listsFlick.reveal(currentItem) }
                        Keys.onLeftPressed: (e) => { if (currentIndex % columns === 0) Nav.focusRail(); else e.accepted = false }
                        Keys.onRightPressed: (e) => { if (currentIndex % columns === columns - 1 || currentIndex === count - 1) listPlay.forceActiveFocus(); else e.accepted = false }
                        Keys.onUpPressed: (e) => { if (currentIndex < columns) smartGrid.forceActiveFocus(); else e.accepted = false }
                        delegate: ItemDelegate {
                            id: mineTile
                            required property var modelData
                            required property int index
                            readonly property bool isNew: modelData.key === "new"
                            width: mineGrid.cellWidth - 16
                            height: mineGrid.cellHeight - 8
                            focus: GridView.isCurrentItem
                            padding: 0
                            Accessible.name: isNew ? qsTr("New playlist") : modelData.name + ", " + qsTr("%n song(s)", "", modelData.count)
                            background: Item {}
                            contentItem: ColumnLayout {
                                spacing: 6
                                Rectangle {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: width
                                    radius: Theme.radius
                                    color: mineTile.isNew ? "transparent" : Theme.surfaceHi
                                    border.width: mineTile.activeFocus ? Theme.focusWidth + 1 : (mineTile.isNew ? 2 : 0)
                                    border.color: mineTile.activeFocus ? Theme.focus : "#3a4252"
                                    clip: true
                                    Label {
                                        anchors.centerIn: parent
                                        visible: mineTile.isNew || (mineTile.modelData.covers || []).length === 0
                                        text: mineTile.isNew ? "+" : "♫"
                                        color: Theme.accent
                                        font.pixelSize: 40
                                    }
                                    Grid {
                                        anchors.fill: parent
                                        anchors.margins: mineTile.activeFocus ? Theme.focusWidth + 1 : 0
                                        columns: 2
                                        visible: !mineTile.isNew
                                        Repeater {
                                            model: (mineTile.modelData.covers || []).length >= 4 ? mineTile.modelData.covers : (mineTile.modelData.covers || []).slice(0, 1)
                                            Image {
                                                required property string modelData
                                                width: (mineTile.modelData.covers || []).length >= 4 ? parent.width / 2 : parent.width
                                                height: width
                                                source: modelData
                                                fillMode: Image.PreserveAspectCrop
                                                asynchronous: true
                                                sourceSize.width: 240
                                            }
                                        }
                                    }
                                }
                                Label { Layout.fillWidth: true; text: mineTile.isNew ? qsTr("New playlist") : mineTile.modelData.name; color: Theme.text; font.bold: true; elide: Text.ElideRight }
                                Label { visible: !mineTile.isNew; text: qsTr("%n song(s)", "", mineTile.modelData.count ?? 0); color: Theme.textDim; font.pixelSize: 12 }
                            }
                            function activate() {
                                if (isNew) { newName.text = ""; newDialog.open() }
                                else page.openList(modelData.key)
                            }
                            onClicked: { mineGrid.currentIndex = index; activate() }
                            Keys.onReturnPressed: activate()
                            Keys.onEnterPressed: activate()
                            Keys.onPressed: (e) => { if (e.key === Qt.Key_P && !isNew) { Music.playPlaylist(modelData.key, 0, (e.modifiers & Qt.ShiftModifier) !== 0); e.accepted = true } }
                        }
                    }
                }
            }
            // The playlist in focus.
            Rectangle {
                Layout.preferredWidth: 360
                Layout.fillHeight: true
                Layout.bottomMargin: 8
                radius: 14
                color: Theme.surface
                Accessible.role: Accessible.Pane
                Accessible.name: qsTr("Playlist preview")
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 10
                    readonly property var card: page.listDetail.card || ({})
                    Label { text: (page.listDetail.card?.smart ?? "") !== "" ? qsTr("Smart playlist") : qsTr("Playlist"); color: Theme.focus; font.pixelSize: 12; font.capitalization: Font.AllUppercase; font.letterSpacing: 1 }
                    Label { Layout.fillWidth: true; text: PlaylistInfo.title(page.listDetail.card || {}); color: Theme.text; font.pixelSize: Theme.fontLarge + 6; font.bold: true; wrapMode: Text.Wrap }
                    Label {
                        text: [qsTr("%n song(s)", "", page.listDetail.card?.count ?? 0),
                               (page.listDetail.card?.durationMs ?? 0) > 0 ? Theme.time(page.listDetail.card.durationMs / 1000) : ""].filter(x => x).join("  ·  ")
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 8
                        FocusButton {
                            id: listPlay
                            primary: true
                            glyph: "▶"
                            text: qsTr("Play")
                            font.pixelSize: Theme.fontSmall
                            enabled: (page.listDetail.entries || []).length > 0
                            Keys.onLeftPressed: smartGrid.forceActiveFocus()
                            KeyNavigation.right: listShuffle
                            onClicked: Music.playPlaylist(page.listKey, 0, false)
                        }
                        FocusButton {
                            id: listShuffle
                            text: qsTr("Shuffle")
                            font.pixelSize: Theme.fontSmall
                            enabled: listPlay.enabled
                            KeyNavigation.left: listPlay
                            KeyNavigation.right: listSave
                            onClicked: Music.playPlaylist(page.listKey, 0, true)
                        }
                        FocusButton {
                            id: listSave
                            text: qsTr("Save offline")
                            font.pixelSize: Theme.fontSmall
                            enabled: listPlay.enabled
                            KeyNavigation.left: listShuffle
                            KeyNavigation.right: listOpen
                            onClicked: listSaveRequest.value = Core.playlistSave(page.listKey)
                        }
                        FocusButton {
                            id: listOpen
                            text: qsTr("Open")
                            font.pixelSize: Theme.fontSmall
                            KeyNavigation.left: listSave
                            onClicked: page.openList(page.listKey)
                        }
                    }
                    Repeater {
                        model: page.listDetail.entries || []
                        RowLayout {
                            id: entryRow
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: 10
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label { Layout.fillWidth: true; text: entryRow.modelData.title; color: Theme.text; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                                Label { Layout.fillWidth: true; text: entryRow.modelData.artist; color: Theme.textDim; font.pixelSize: 12; elide: Text.ElideRight }
                            }
                            Rectangle {
                                visible: entryRow.modelData.format !== ""
                                implicitWidth: entryFmt.implicitWidth + 10
                                implicitHeight: 20
                                radius: 5
                                color: entryRow.modelData.hires ? Theme.accent : Theme.surfaceHi
                                Label { id: entryFmt; anchors.centerIn: parent; text: entryRow.modelData.format; font.pixelSize: 11; font.bold: true; color: entryRow.modelData.hires ? Theme.bg : Theme.text }
                            }
                        }
                    }
                    Item { Layout.fillHeight: true }
                }
            }
        }

        // ---- genres --------------------------------------------------------------------------
        GridView {
            id: genreGrid
            visible: page.view === "genres"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: page.facets.genres || []
            cellWidth: Math.floor(width / Math.max(1, Math.floor(width / 240)))
            cellHeight: 104
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Genres")
            Keys.onLeftPressed: (e) => { if (currentIndex % Math.max(1, Math.floor(width / cellWidth)) === 0) Nav.focusRail(); else e.accepted = false }
            Keys.onUpPressed: (e) => { if (currentIndex < Math.floor(width / cellWidth)) menu.itemAt(3).forceActiveFocus(); else e.accepted = false }
            delegate: ItemDelegate {
                id: genreTile
                required property var modelData
                required property int index
                width: genreGrid.cellWidth - 16
                height: 88
                focus: GridView.isCurrentItem
                Accessible.name: modelData[0] + ", " + qsTr("%n album(s)", "", modelData[1])
                background: Rectangle {
                    radius: Theme.radius
                    color: Qt.hsla((genreTile.modelData[0].length * 53 % 360) / 360, 0.32, 0.26, 1)
                    border.width: genreTile.activeFocus ? Theme.focusWidth + 1 : 0
                    border.color: Theme.focus
                }
                contentItem: ColumnLayout {
                    spacing: 2
                    Label { text: genreTile.modelData[0]; color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; elide: Text.ElideRight; Layout.fillWidth: true }
                    Label { text: qsTr("%n album(s)", "", genreTile.modelData[1]); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                }
                function pick() { page.genre = modelData[0]; page.choose("albums") }
                onClicked: pick()
                Keys.onReturnPressed: pick()
                Keys.onEnterPressed: pick()
            }
        }
    }

    ColumnLayout {
        anchors.centerIn: parent
        visible: !lib.loading && lib.total === 0 && page.view !== "genres" && page.view !== "playlists"
        spacing: 16
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: page.anyFilter ? (page.artistsView ? qsTr("No artists match these filters.") : page.songsView ? qsTr("No songs match these filters.") : qsTr("No albums match these filters."))
                                 : qsTr("No music yet. Add a folder under Sources and choose “Music” for it.")
            color: Theme.textDim
            font.pixelSize: Theme.fontLarge
        }
        FocusButton {
            Layout.alignment: Qt.AlignHCenter
            text: page.anyFilter ? qsTr("Clear filters") : qsTr("Add a folder")
            onClicked: page.anyFilter ? (page.artistsView ? page.artistHires = false : page.songsView ? page.songLens = ({}) : page.clearFilters()) : Nav.go("sources")
        }
    }

    ActionMenu {
        id: chooser
        property string mode: ""
        onChosen: (v) => {
            if (mode === "genre") page.genre = v
            else if (mode === "decade") page.decade = v
            else page.albumSort = v
        }
    }
    QtObject {
        id: listSaveRequest
        property var value: 0
    }
    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId !== listSaveRequest.value) return
            listSaveRequest.value = 0;
            (page.Window.window as Main).showError(detail.started > 0
                ? qsTr("Saving %n song(s). Progress is under Downloads.", "", detail.started)
                : qsTr("These songs are saved or on this computer already."))
        }
    }
    // A new, empty playlist; it opens for adding songs.
    Popup {
        id: newDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(460, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: newName.forceActiveFocus()
        onClosed: mineGrid.forceActiveFocus()
        RowLayout {
            anchors.fill: parent
            spacing: 8
            TextField {
                id: newName
                Layout.fillWidth: true
                placeholderText: qsTr("Name of the playlist")
                Accessible.name: qsTr("Name of the playlist")
                KeyNavigation.right: newCreate
                onAccepted: newCreate.clicked()
            }
            FocusButton {
                id: newCreate
                primary: true
                text: qsTr("Create")
                KeyNavigation.left: newName
                onClicked: {
                    page.createRequest = Core.playlistCreate(newName.text)
                    newDialog.close()
                }
            }
        }
    }
    AddToPlaylist { id: addToPlaylist }
    ActionMenu {
        id: songMenu
        property var target: ({})
        onChosen: (v) => {
            const m = target
            if (v === "next") Music.playNext([m.id])
            else if (v === "queue") Music.addToQueue([m.id])
            else if (v === "playlist") addToPlaylist.show([m.id], 0, songList)
            else if (v === "match") musicMatch.again(m.id, "track", m.title, songList)
            else if (v === "search") musicMatch.search(m.id, "track", m.title, m.artist || "", songList)
            else Nav.openItem(m.id, "track", m.parentId)
        }
    }
    ActionMenu {
        id: itemMenu
        property var target: ({})
        onChosen: (v) => {
            const m = target
            if (v === "next") Music.playNext([m.id])
            else if (v === "queue") Music.addToQueue([m.id])
            else if (v === "save") Core.saveItem(m.id, 0)
            else if (v === "playlist") addToPlaylist.show([m.id], 0, grid)
            else if (v === "match") musicMatch.again(m.id, m.kind, m.title, grid)
            else if (v === "search") musicMatch.search(m.id, m.kind, m.title, m.kind === "album" ? (m.subtitle || "") : "", grid)
            else page.open(m)
        }
    }
    MusicMatch {
        id: musicMatch
        backend: Core
        onNotice: (text) => (page.Window.window as Main).showError(text)
        onSettingsRequested: (tab) => Nav.settings(tab)
    }
}
