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
        onTotalChanged: if (grid.currentIndex < 0 && grid.count > 0) grid.currentIndex = 0
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
                    text: lib.unwatchedOnly ? qsTr("Unwatched") : qsTr("All")
                    tip: qsTr("Show only unwatched")
                    KeyNavigation.left: sortButton
                    KeyNavigation.down: grid
                    onClicked: lib.unwatchedOnly = !lib.unwatchedOnly
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
            cellWidth: 206
            cellHeight: 346
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

            readonly property int columns: Math.max(1, Math.floor(width / cellWidth))
            Keys.onLeftPressed: (e) => { if (currentIndex % columns === 0) Nav.focusRail(); else e.accepted = false }
            Keys.onUpPressed: (e) => { if (currentIndex < columns) sortButton.forceActiveFocus(); else e.accepted = false }

            delegate: PosterCard {
                id: tile
                required property int index
                required property var model
                title: model.title
                year: model.year
                poster: model.poster
                progress: model.progress
                watched: model.watched
                unwatched: Number(model.unwatched)
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
            text: lib.unwatchedOnly ? qsTr("Everything here is watched.") : page.emptyText
            color: Theme.textDim
            font.pixelSize: Theme.fontLarge
        }
        FocusButton {
            Layout.alignment: Qt.AlignHCenter
            visible: !lib.unwatchedOnly
            text: qsTr("Add a folder")
            onClicked: Nav.go("sources")
        }
    }
}
