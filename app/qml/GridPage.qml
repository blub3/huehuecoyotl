pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Poster grid for movies or shows, paged from the core as it scrolls. Smooth with thousands of
// items: delegates are reused and posters are small pre-scaled files (SPEC 9).
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property string kind: "movie"
    property string heading
    property string emptyText
    // Music (2.0): square covers, tabs instead of one kind, no "unwatched" filter.
    property real posterRatio: 1.5
    readonly property bool music: kind === "album" || kind === "artist"
    // [{ key, label, kind, sort }]: tabs above the grid (the Music page).
    property var tabs: []
    property string tab: tabs.length > 0 ? tabs[0].key : ""
    function chooseTab(key) {
        const t = tabs.find(x => x.key === key)
        if (!t) return
        tab = key
        kind = t.kind
        lib.sort = t.sort
    }
    function open(m) { Nav.openItem(m.id, m.kind, m.parentId) }

    readonly property var sorts: [
        { key: "title", label: qsTr("A–Z") },
        { key: "added", label: qsTr("Recently added") },
        { key: "year", label: qsTr("Year") }
    ]

    property bool benchOpened: false
    LibraryModel {
        id: lib
        kind: page.kind
        // The first page arrives after the grid took focus: select the first card then.
        onTotalChanged: {
            if (grid.currentIndex < 0 && grid.count > 0) grid.currentIndex = 0
            if ((Bench.mode === "scroll" || (Bench.mode === "music" && page.music)) && total > 0 && !benchScroll.running) benchStart.start()
            // --benchmark episodes: open the first show.
            if (Bench.mode === "episodes" && total > 0 && page.kind === "show" && grid.count > 0 && !page.benchOpened) {
                page.benchOpened = true
                Qt.callLater(() => Nav.openItem(lib.get(0).id))
            }
        }
    }

    // --benchmark scroll: scroll the grid on its own at a steady speed and record frame times.
    // 20 seconds at 4000 px/s covers about 1,400 posters, with pages fetched along the way.
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

    Component.onCompleted: grid.forceActiveFocus()
    onActiveFocusChanged: if (activeFocus && !grid.activeFocus && !header.activeFocus) grid.forceActiveFocus()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        anchors.rightMargin: 16
        spacing: 18

        FocusScope {
            id: header
            Layout.fillWidth: true
            Layout.preferredHeight: 46
            RowLayout {
                anchors.fill: parent
                spacing: 12
                Label {
                    text: page.heading
                    color: Theme.text
                    font.pixelSize: Theme.fontTitle
                    font.bold: true
                    Accessible.role: Accessible.Heading
                }
                Repeater {
                    id: tabButtons
                    model: page.tabs
                    FocusButton {
                        required property var modelData
                        required property int index
                        text: modelData.label
                        primary: page.tab === modelData.key
                        font.pixelSize: Theme.fontSmall
                        Accessible.role: Accessible.PageTab
                        Accessible.selected: page.tab === modelData.key
                        KeyNavigation.down: grid
                        Keys.onLeftPressed: index === 0 ? Nav.focusRail() : tabButtons.itemAt(index - 1).forceActiveFocus()
                        Keys.onRightPressed: index === page.tabs.length - 1 ? sortButton.forceActiveFocus() : tabButtons.itemAt(index + 1).forceActiveFocus()
                        onClicked: page.chooseTab(modelData.key)
                    }
                }
                Label {
                    text: lib.total > 0 ? String(lib.total) : ""
                    color: Theme.textDim
                    font.pixelSize: Theme.fontLarge
                    Layout.fillWidth: true
                }
                FocusButton {
                    id: sortButton
                    focus: true
                    visible: page.tabs.length === 0
                    Keys.onLeftPressed: (e) => { if (tabButtons.count > 0) tabButtons.itemAt(tabButtons.count - 1).forceActiveFocus(); else e.accepted = false }
                    text: page.sorts.find(s => s.key === lib.sort).label
                    tip: qsTr("Sort")
                    KeyNavigation.right: unwatchedButton
                    KeyNavigation.down: grid
                    onClicked: {
                        const i = page.sorts.findIndex(s => s.key === lib.sort)
                        lib.sort = page.sorts[(i + 1) % page.sorts.length].key
                    }
                }
                FocusButton {
                    id: unwatchedButton
                    // All → Unwatched → Saved (issue #4) → All; music: All → Saved → All.
                    text: lib.filter === "unwatched" ? qsTr("Unwatched")
                        : lib.filter === "saved" ? qsTr("Saved") : qsTr("All")
                    tip: page.music ? qsTr("Filter: all or saved") : qsTr("Filter: all, unwatched or saved")
                    visible: page.kind !== "artist"
                    KeyNavigation.left: sortButton.visible ? sortButton : (tabButtons.count > 0 ? tabButtons.itemAt(tabButtons.count - 1) : null)
                    KeyNavigation.down: grid
                    onClicked: lib.filter = lib.filter === "" ? (page.music ? "saved" : "unwatched") : lib.filter === "unwatched" ? "saved" : ""
                }
            }
        }

        GridView {
            id: grid
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            focus: true
            model: lib
            // Columns follow from the width and the cover size (issue #2); the cells share the
            // width exactly, so no column is cut off at the edge.
            readonly property real gap: Math.round(26 * Appearance.textScale(Appearance.coverWidth))
            readonly property int columns: Math.max(1, Math.floor((width - leftMargin) / (Appearance.coverWidth + gap)))
            readonly property real posterWidth: Math.floor((width - leftMargin) / columns) - gap
            cellWidth: posterWidth + gap
            cellHeight: Appearance.cardHeight(posterWidth, page.posterRatio) + gap
            // Keep the focused card in view when the size changes (window and fullscreen).
            onCellWidthChanged: Qt.callLater(() => { if (currentIndex >= 0) positionViewAtIndex(currentIndex, GridView.Contain) })
            // Room for the focused card's scale-up and ring inside the clip.
            topMargin: 14
            leftMargin: 10
            cacheBuffer: 1200
            reuseItems: true
            keyNavigationEnabled: true
            highlightFollowsCurrentItem: true
            highlightMoveDuration: 80
            Accessible.role: Accessible.List
            Accessible.name: page.heading
            ScrollBar.vertical: ScrollBar {}

            Keys.onLeftPressed: (e) => { if (currentIndex % columns === 0) Nav.focusRail(); else e.accepted = false }
            Keys.onUpPressed: (e) => {
                if (currentIndex >= columns) { e.accepted = false; return }
                if (tabButtons.count > 0) tabButtons.itemAt(Math.max(0, page.tabs.findIndex(t => t.key === page.tab))).forceActiveFocus()
                else sortButton.forceActiveFocus()
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
                posterRatio: page.posterRatio
                caption: page.music ? (tile.model.subtitle ?? "") : (tile.model.year > 0 ? String(tile.model.year) : "")
                progress: model.progress
                watched: model.watched
                unwatched: Number(model.unwatched)
                saved: model.saved === true
                focus: GridView.isCurrentItem
                onClicked: { grid.currentIndex = tile.index; page.open(tile.model) }
                Keys.onReturnPressed: page.open(tile.model)
                Keys.onEnterPressed: page.open(tile.model)
                Keys.onPressed: (e) => {
                    if (e.key === Qt.Key_P || e.key === Qt.Key_MediaPlay) { Playback.openItem(tile.model.id); e.accepted = true }
                    else if (e.key === Qt.Key_W) { Core.setWatched(tile.model.id, !tile.model.watched); e.accepted = true }
                }
            }
        }
    }

    ColumnLayout {
        anchors.centerIn: parent
        visible: !lib.loading && lib.total === 0
        spacing: 16
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: lib.filter === "unwatched" ? qsTr("Everything here is watched.")
                : lib.filter === "saved" ? qsTr("Nothing here is saved on this computer.") : page.emptyText
            color: Theme.textDim
            font.pixelSize: Theme.fontLarge
        }
        FocusButton {
            Layout.alignment: Qt.AlignHCenter
            visible: lib.filter === ""
            text: qsTr("Add a folder")
            onClicked: Nav.go("sources")
        }
    }
}
