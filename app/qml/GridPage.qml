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

    readonly property var sorts: [
        { key: "title", label: qsTr("A–Z") },
        { key: "added", label: qsTr("Recently added") },
        { key: "year", label: qsTr("Year") }
    ]

    LibraryModel {
        id: lib
        kind: page.kind
        // The first page arrives after the grid took focus: select the first card then.
        onTotalChanged: {
            if (grid.currentIndex < 0 && grid.count > 0) grid.currentIndex = 0
            if (Bench.mode === "scroll" && total > 0 && !benchScroll.running) benchStart.start()
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
                Label {
                    text: lib.total > 0 ? String(lib.total) : ""
                    color: Theme.textDim
                    font.pixelSize: Theme.fontLarge
                    Layout.fillWidth: true
                }
                FocusButton {
                    id: sortButton
                    focus: true
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
                    // All → Unwatched → Saved (issue #4) → All
                    text: lib.filter === "unwatched" ? qsTr("Unwatched")
                        : lib.filter === "saved" ? qsTr("Saved") : qsTr("All")
                    tip: qsTr("Filter: all, unwatched or saved")
                    KeyNavigation.left: sortButton
                    KeyNavigation.down: grid
                    onClicked: lib.filter = lib.filter === "" ? "unwatched" : lib.filter === "unwatched" ? "saved" : ""
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
            cellHeight: Appearance.cardHeight(posterWidth) + gap
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
            Keys.onUpPressed: (e) => { if (currentIndex < columns) sortButton.forceActiveFocus(); else e.accepted = false }

            delegate: PosterCard {
                id: tile
                required property int index
                required property var model
                title: model.title
                year: model.year
                poster: model.poster
                posterLarge: model.posterLarge ?? ""
                posterWidth: grid.posterWidth
                progress: model.progress
                watched: model.watched
                unwatched: Number(model.unwatched)
                saved: model.saved === true
                focus: GridView.isCurrentItem
                onClicked: { grid.currentIndex = tile.index; Nav.openItem(tile.model.id) }
                Keys.onReturnPressed: Nav.openItem(tile.model.id)
                Keys.onEnterPressed: Nav.openItem(tile.model.id)
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
