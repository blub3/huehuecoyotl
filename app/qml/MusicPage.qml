pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Music (issue #78): a top menu (Albums, Artists, Genres) over one paged grid, "Search music",
// the Quality lens (Hi-Res, Lossless, Several versions, Saved; they combine), Genre and Decade
// filters, four sorts, a format ribbon on every cover and a letter rail for A–Z jumps. Without
// a music folder it explains how to add one. Keys: Enter opens, P plays, Shift+P shuffles, Menu
// offers Play next, Add to queue and Save.
FocusScope {
    id: page
    readonly property bool isPlayer: false

    // albums, artists or genres.
    property string view: "albums"
    readonly property var views: [
        { key: "albums", label: qsTr("Albums") },
        { key: "artists", label: qsTr("Artists") },
        { key: "genres", label: qsTr("Genres") }
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
    readonly property bool anyFilter: lenses.some(l => lens[l.key]) || genre !== "" || decade > 0
    // The core's filter: one token per line (crates/core/src/library.rs, parse_filter).
    readonly property string filter: {
        if (!albums) return ""
        const t = lenses.filter(l => lens[l.key]).map(l => l.key)
        if (genre !== "") t.push("genre=" + genre)
        if (decade > 0) t.push("decade=" + decade)
        return t.join("\n")
    }
    property var facets: ({})
    property var facetsRequest: 0
    function refreshFacets() { facetsRequest = Core.musicFacets(albums || view === "genres" ? "album" : "artist", filter) }
    onFilterChanged: Qt.callLater(refreshFacets)
    onViewChanged: Qt.callLater(refreshFacets)

    function toggleLens(key) {
        const l = Object.assign({}, lens)
        l[key] = !l[key]
        lens = l
    }
    function clearFilters() { lens = ({}); genre = ""; decade = 0 }
    function choose(v) {
        view = v
        Qt.callLater(() => (v === "genres" ? genreGrid : grid).forceActiveFocus())
    }
    function lensCount(key) { return page.facets[key] ?? "" }

    function open(m) { Nav.openItem(m.id, m.kind, m.parentId) }
    function play(m, shuffle) { Music.playItems([m.id], 0, shuffle) }
    function more(item, m) {
        itemMenu.target = m
        itemMenu.show(item, [
            { label: qsTr("Play next"), value: "next" },
            { label: qsTr("Add to queue"), value: "queue" },
            { label: qsTr("Save on this computer"), value: "save" },
            { label: m.kind === "album" ? qsTr("Open album") : qsTr("Open artist"), value: "open" }
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
        kind: page.view === "artists" ? "artist" : "album"
        sort: page.view === "artists" ? "title" : page.albumSort
        filter: page.filter
        onTotalChanged: {
            if (grid.currentIndex < 0 && grid.count > 0) grid.currentIndex = 0
            if (Bench.mode === "music" && page.albums && total > 0 && !benchScroll.running) benchStart.start()
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
            if (requestId !== page.facetsRequest) return
            page.facets = detail
        }
    }

    // --benchmark music: scroll the Albums grid at a steady speed and record frame times.
    Timer { id: benchStart; interval: 1500; onTriggered: { Bench.beginFrames(); benchScroll.start() } }
    FrameAnimation {
        id: benchScroll
        property real travelled: 0
        onTriggered: {
            const step = Math.min(frameTime, 0.05) * 4000
            const max = grid.originY + grid.contentHeight - grid.height
            grid.contentY = Math.min(grid.contentY + step, max)
            travelled += step
            if (elapsedTime > 20 || (grid.contentY >= max && grid.count >= lib.total)) {
                stop()
                Bench.endFrames(grid.count, travelled)
            }
        }
    }

    Component.onCompleted: { refreshFacets(); grid.forceActiveFocus() }
    onActiveFocusChanged: if (activeFocus && !header.activeFocus && !lensRow.activeFocus && !letterRail.activeFocus)
                              (view === "genres" ? genreGrid : grid).forceActiveFocus()

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
                                Keys.onDownPressed: page.albums ? lensRow.focusFirst() : (page.view === "genres" ? genreGrid : grid).forceActiveFocus()
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
            visible: page.view !== "genres"

            GridView {
                id: grid
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                focus: true
                model: lib
                readonly property real gap: Math.round(26 * Appearance.textScale(Appearance.coverWidth))
                readonly property int columns: Math.max(1, Math.floor((width - leftMargin) / (Appearance.coverWidth + gap)))
                readonly property real posterWidth: Math.floor((width - leftMargin) / columns) - gap
                cellWidth: posterWidth + gap
                cellHeight: Appearance.cardHeight(posterWidth, 1) + gap
                onCellWidthChanged: Qt.callLater(() => { if (currentIndex >= 0) positionViewAtIndex(currentIndex, GridView.Contain) })
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
                    if ((currentIndex % columns === columns - 1 || currentIndex === count - 1) && letterRail.visible) letterRail.focusNear()
                    else e.accepted = false
                }
                Keys.onUpPressed: (e) => {
                    if (currentIndex >= columns) { e.accepted = false; return }
                    if (page.albums) lensRow.focusFirst()
                    else menu.itemAt(1).forceActiveFocus()
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
                    caption: (tile.model.subtitle ?? "") + (page.albums && tile.model.year > 0 ? "  ·  " + tile.model.year : "")
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
                            Keys.onLeftPressed: grid.forceActiveFocus()
                        }
                    }
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
            Keys.onUpPressed: (e) => { if (currentIndex < Math.floor(width / cellWidth)) menu.itemAt(2).forceActiveFocus(); else e.accepted = false }
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
        visible: !lib.loading && lib.total === 0 && page.view !== "genres"
        spacing: 16
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: page.anyFilter ? qsTr("No albums match these filters.")
                                 : qsTr("No music yet. Add a folder under Sources and choose “Music” for it.")
            color: Theme.textDim
            font.pixelSize: Theme.fontLarge
        }
        FocusButton {
            Layout.alignment: Qt.AlignHCenter
            text: page.anyFilter ? qsTr("Clear filters") : qsTr("Add a folder")
            onClicked: page.anyFilter ? page.clearFilters() : Nav.go("sources")
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
    ActionMenu {
        id: itemMenu
        property var target: ({})
        onChosen: (v) => {
            const m = target
            if (v === "next") Music.playNext([m.id])
            else if (v === "queue") Music.addToQueue([m.id])
            else if (v === "save") Core.saveItem(m.id, 0)
            else page.open(m)
        }
    }
}
