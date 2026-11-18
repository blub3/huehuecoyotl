pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl
import "MediaInfo.js" as MediaInfo

// A series or a person (issue #66): the books in order (a series by position, a person's by
// series and year) with their progress. A series has "Continue series": the first unfinished
// book after the last one listened to. Enter opens a book, P plays it.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var itemId
    property var d: ({})
    property string kind: ""
    property var request: 0
    property var continueRequest: 0
    readonly property var books: d.books || []
    readonly property bool series: kind === "series"

    function load() { request = Core.bookDetail(itemId) }
    BookMatch {
        id: bookMatch
        backend: Core
        onNotice: (text) => (page.Window.window as Main).showError(text)
        onSettingsRequested: (tab) => Nav.settings(tab)
        onChanged: page.load()
    }
    Component.onCompleted: load()
    onActiveFocusChanged: if (activeFocus && !list.activeFocus && !continueButton.activeFocus) (continueButton.visible ? continueButton : list).forceActiveFocus()

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === page.continueRequest) {
                page.continueRequest = 0
                if (detail.bookId > 0) {
                    Book.playBook(detail.bookId)
                    Nav.nowPlaying()
                }
                return
            }
            if (requestId !== page.request) return
            const first = page.d.id === undefined
            page.kind = kind
            page.d = detail
            if (first) (continueButton.visible ? continueButton : list).forceActiveFocus()
        }
        function onItemChanged(id) { if (id === page.itemId || page.books.some(b => b.id === id)) page.load() }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 48
        spacing: 20

        Label {
            text: page.series ? qsTr("Series") : qsTr("Author or narrator")
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
        RowLayout {
            spacing: 12
            Label {
                text: qsTr("%n book(s)", "", page.books.length)
                color: Theme.textDim
            }
            FocusButton {
                id: continueButton
                visible: page.series && page.books.some(b => !b.finished)
                primary: true
                glyph: "▶"
                text: page.books.some(b => b.positionMs > 0 || b.finished) ? qsTr("Continue series") : qsTr("Play the series")
                Keys.onLeftPressed: Nav.focusRail()
                KeyNavigation.down: list
                onClicked: page.continueRequest = Core.continueSeries(page.itemId)
            }
        }

        // Issue #110: an author's photo, biography and match, and matching it again.
        RowLayout {
            visible: !page.series && page.d.id !== undefined
            spacing: 24
            Layout.fillWidth: true
            Rectangle {
                Layout.preferredWidth: 120
                Layout.preferredHeight: 120
                Layout.alignment: Qt.AlignTop
                radius: 60
                clip: true
                color: Theme.surfaceHi
                Label {
                    anchors.centerIn: parent
                    visible: authorPhoto.status !== Image.Ready
                    text: (page.d.title || "").split(/\s+/).filter(w => w.length > 0).slice(0, 2).map(w => w[0].toUpperCase()).join("")
                    color: Theme.text
                    font.pixelSize: 40
                    font.bold: true
                }
                Image {
                    id: authorPhoto
                    anchors.fill: parent
                    source: page.d.image || ""
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                    sourceSize.width: 240
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 8
                Label {
                    objectName: "authorState"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    font.pixelSize: Theme.fontSmall
                    color: page.d.matched ? Theme.textDim : Theme.focus
                    text: !page.d.matched ? qsTr("Not matched yet: no photo or biography.")
                          : page.d.matchedVia ? qsTr("Matched through the book “%1”.").arg(page.d.matchedVia.title)
                          : page.d.locked ? qsTr("Matched as you chose.") : ""
                }
                Label {
                    Layout.fillWidth: true
                    Layout.maximumWidth: 900
                    visible: (page.d.biography || "") !== ""
                    text: page.d.biography || ""
                    color: Theme.text
                    wrapMode: Text.Wrap
                    maximumLineCount: 6
                    elide: Text.ElideRight
                }
                RowLayout {
                    spacing: 10
                    FocusButton {
                        id: authorMatch
                        objectName: "authorMatchAgain"
                        text: qsTr("Match again")
                        KeyNavigation.right: authorSearch
                        KeyNavigation.down: list
                        Keys.onLeftPressed: Nav.focusRail()
                        onClicked: bookMatch.again(page.itemId, "person", page.d.title, authorMatch)
                    }
                    FocusButton {
                        id: authorSearch
                        objectName: "authorSearch"
                        text: page.d.matchedVia ? qsTr("Not this author?") : qsTr("Search Open Library…")
                        KeyNavigation.left: authorMatch
                        KeyNavigation.down: list
                        onClicked: bookMatch.search(page.itemId, "person", page.d.title, "", authorSearch)
                    }
                }
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 6
            model: page.books
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: page.d.title || ""
            ScrollBar.vertical: ScrollBar {}
            Keys.onLeftPressed: Nav.focusRail()
            Keys.onUpPressed: (e) => {
                if (currentIndex === 0 && continueButton.visible) continueButton.forceActiveFocus()
                else if (currentIndex === 0 && !page.series) authorMatch.forceActiveFocus()
                else e.accepted = false
            }
            Keys.onReturnPressed: if (currentItem) Nav.openItem(page.books[currentIndex].id, "audiobook")
            Keys.onEnterPressed: if (currentItem) Nav.openItem(page.books[currentIndex].id, "audiobook")
            Keys.onPressed: (e) => {
                if ((e.key === Qt.Key_P || e.key === Qt.Key_MediaPlay) && currentItem) { Book.playBook(page.books[currentIndex].id); e.accepted = true }
            }
            delegate: ItemDelegate {
                id: row
                required property var modelData
                required property int index
                width: list.width
                height: 84
                readonly property bool focused: list.activeFocus && list.currentIndex === index
                Accessible.name: modelData.title + (modelData.finished ? ", " + qsTr("finished") : modelData.positionMs > 0 ? ", " + qsTr("in progress") : "")
                background: Rectangle {
                    radius: 8
                    color: row.focused ? Theme.surfaceHi : Theme.surface
                    border.width: row.focused ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                onClicked: { list.currentIndex = index; Nav.openItem(modelData.id, "audiobook") }
                contentItem: RowLayout {
                    spacing: 16
                    Rectangle {
                        Layout.preferredWidth: 64
                        Layout.preferredHeight: 64
                        radius: 6
                        clip: true
                        color: Theme.surfaceHi
                        Image {
                            anchors.fill: parent
                            source: row.modelData.poster || ""
                            fillMode: Image.PreserveAspectFit
                            asynchronous: true
                            sourceSize.width: 128
                        }
                    }
                    Label {
                        visible: page.series && row.modelData.seriesPosition > 0
                        text: String(row.modelData.seriesPosition)
                        color: Theme.accent
                        font.pixelSize: Theme.fontLarge
                        font.bold: true
                        Layout.preferredWidth: 36
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4
                        Label {
                            Layout.fillWidth: true
                            text: row.modelData.title
                            color: Theme.text
                            elide: Text.ElideRight
                        }
                        Label {
                            text: {
                                const parts = []
                                if (row.modelData.year > 0) parts.push(row.modelData.year)
                                if (row.modelData.durationMs > 0) parts.push(MediaInfo.duration(row.modelData.durationMs))
                                if (row.modelData.finished) parts.push(qsTr("Finished"))
                                else if (row.modelData.positionMs > 0)
                                    parts.push(qsTr("%1 left").arg(MediaInfo.duration(Math.max(0, row.modelData.durationMs - row.modelData.positionMs))))
                                return parts.join("  ·  ")
                            }
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                        }
                        Rectangle {
                            visible: row.modelData.positionMs > 0 && !row.modelData.finished && row.modelData.durationMs > 0
                            Layout.preferredWidth: 220
                            Layout.preferredHeight: 4
                            radius: 2
                            color: Theme.surfaceHi
                            Rectangle {
                                width: parent.width * Math.min(1, row.modelData.positionMs / Math.max(1, row.modelData.durationMs))
                                height: parent.height
                                radius: 2
                                color: Theme.accent
                            }
                        }
                    }
                }
            }
        }
    }
}
