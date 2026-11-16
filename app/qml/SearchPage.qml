pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Search (issue #10: results grouped by kind): one row each for movies, shows, artists, albums
// and tracks, shown only when it has results. Down from the field reaches the first row; up and
// down move between rows, left and right along a row.
FocusScope {
    id: page
    readonly property bool isPlayer: false

    readonly property var kinds: [
        { kind: "movie", label: qsTr("Movies"), ratio: 1.5 },
        { kind: "show", label: qsTr("Shows"), ratio: 1.5 },
        { kind: "artist", label: qsTr("Artists"), ratio: 1 },
        { kind: "album", label: qsTr("Albums"), ratio: 1 },
        { kind: "track", label: qsTr("Tracks"), ratio: 1 }
    ]
    property string query: ""
    Timer { id: debounce; interval: 220; onTriggered: page.query = field.text }

    // Rows with results, in order (indexes into the repeater).
    function visibleRows() {
        const out = []
        for (let i = 0; i < rows.count; ++i) if (rows.itemAt(i) && rows.itemAt(i).visible) out.push(i)
        return out
    }
    function focusNext(from, step) {
        const v = visibleRows()
        const at = v.indexOf(from)
        const next = at < 0 ? (step > 0 ? v[0] : undefined) : v[at + step]
        if (next !== undefined) rows.itemAt(next).forceActiveFocus()
        else if (step < 0) field.forceActiveFocus()
    }
    // Set by the rows when their results change.
    property bool anyResults: false
    // Enter in the field: move to the first row once results are there.
    property bool focusWhenFound: false
    function recount() {
        anyResults = visibleRows().length > 0
        if (focusWhenFound && anyResults && field.activeFocus) {
            focusWhenFound = false
            focusNext(-1, 1)
        }
    }

    Component.onCompleted: field.forceActiveFocus()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        spacing: 18

        TextField {
            id: field
            Layout.fillWidth: true
            Layout.maximumWidth: 720
            placeholderText: qsTr("Search movies, shows and music")
            font.pixelSize: Theme.fontLarge
            Accessible.name: qsTr("Search")
            focus: true
            onTextEdited: { page.focusWhenFound = false; debounce.restart() }
            onAccepted: {
                page.query = text
                if (page.anyResults) page.focusNext(-1, 1)
                else page.focusWhenFound = true
            }
            Keys.onDownPressed: page.focusNext(-1, 1)
            Keys.onLeftPressed: (e) => { if (cursorPosition === 0) Nav.focusRail(); else e.accepted = false }
            background: Rectangle {
                implicitHeight: 52
                radius: Theme.radius
                color: Theme.surface
                border.width: field.activeFocus ? Theme.focusWidth : 1
                border.color: field.activeFocus ? Theme.focus : Theme.surfaceHi
            }
            color: Theme.text
            placeholderTextColor: Theme.textDim
            leftPadding: 18
        }

        Label {
            visible: page.query.length > 0 && !page.anyResults
            text: qsTr("Nothing found for “%1”.").arg(page.query)
            color: Theme.textDim
        }

        Flickable {
            id: flick
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentHeight: col.implicitHeight
            clip: true
            ScrollBar.vertical: ScrollBar {}
            function reveal(item) {
                const p = item.mapToItem(contentItem, 0, 0)
                if (p.y < contentY) contentY = Math.max(0, p.y)
                else if (p.y + item.height > contentY + height)
                    contentY = Math.min(p.y + item.height - height, Math.max(0, contentHeight - height))
            }

            ColumnLayout {
                id: col
                width: flick.width
                spacing: 14

                Repeater {
                    id: rows
                    model: page.kinds
                    FocusScope {
                        id: row
                        required property var modelData
                        required property int index
                        onVisibleChanged: page.recount()
                        Layout.fillWidth: true
                        implicitHeight: rowCol.implicitHeight
                        visible: results.count > 0
                        SearchModel {
                            id: results
                            kind: row.modelData.kind
                            text: page.query
                        }
                        ColumnLayout {
                            id: rowCol
                            width: parent.width
                            spacing: 8
                            Label {
                                text: row.modelData.label
                                color: Theme.textDim
                                font.pixelSize: Theme.fontLarge
                                Accessible.role: Accessible.Heading
                            }
                            ListView {
                                id: list
                                focus: true
                                Layout.fillWidth: true
                                Layout.preferredHeight: Appearance.cardHeight(Appearance.coverWidth, row.modelData.ratio) + 24
                                orientation: ListView.Horizontal
                                spacing: Math.round(26 * Appearance.textScale(Appearance.coverWidth))
                                topMargin: 10
                                leftMargin: 10
                                clip: true
                                model: results
                                keyNavigationEnabled: true
                                Accessible.role: Accessible.List
                                Accessible.name: row.modelData.label
                                Keys.onUpPressed: page.focusNext(row.index, -1)
                                Keys.onDownPressed: page.focusNext(row.index, 1)
                                Keys.onLeftPressed: (e) => { if (currentIndex === 0) Nav.focusRail(); else e.accepted = false }
                                onActiveFocusChanged: if (activeFocus) flick.reveal(row)
                                delegate: PosterCard {
                                    id: hit
                                    required property var model
                                    title: model.title
                                    year: model.year
                                    // Music: an album's artist, a track's album.
                                    caption: (model.subtitle ?? "") !== "" ? model.subtitle : (model.year > 0 ? String(model.year) : "")
                                    poster: model.poster
                                    posterLarge: model.posterLarge ?? ""
                                    posterWidth: Appearance.coverWidth
                                    posterRatio: row.modelData.ratio
                                    progress: model.progress
                                    watched: model.watched
                                    unwatched: Number(model.unwatched)
                                    focus: ListView.isCurrentItem
                                    onClicked: Nav.openItem(hit.model.id, hit.model.kind, hit.model.parentId)
                                    Keys.onReturnPressed: Nav.openItem(hit.model.id, hit.model.kind, hit.model.parentId)
                                    Keys.onEnterPressed: Nav.openItem(hit.model.id, hit.model.kind, hit.model.parentId)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
