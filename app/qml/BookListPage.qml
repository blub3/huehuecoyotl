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
        function onItemChanged(id) { if (page.books.some(b => b.id === id)) page.load() }
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
            Keys.onUpPressed: (e) => { if (currentIndex === 0 && continueButton.visible) continueButton.forceActiveFocus(); else e.accepted = false }
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
