pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Dialogs
import Coyotl
import "MediaInfo.js" as MediaInfo

// A book (issue #66): cover in its own aspect, title, authors and narrators (each opens their
// books), the series with its position (opens the series), duration and time left, the
// description, Play or Resume, Mark as finished, Save and Save as… (all of its files), "Search
// again" (issue #67) and the chapters with the current one marked; Enter on a chapter plays from
// it. Everything comes from the local database.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var itemId
    property var d: ({})
    property var request: 0
    property var finishRequest: 0
    property var exportRequest: 0
    property string exportDest: ""
    readonly property bool loaded: d.id !== undefined
    readonly property var chapters: d.chapters || []
    readonly property bool current: Book.bookId === page.itemId
    // The chapter playing, or the one the saved position is in.
    readonly property real at: current ? Book.position * 1000 : (d.positionMs || 0)
    readonly property int chapterIndex: {
        let i = -1
        for (let k = 0; k < chapters.length; ++k) if (chapters[k].startMs <= at) i = k
        return i
    }
    readonly property bool started: (d.positionMs || 0) > 0 && !d.finished

    // Offline copies (issue #4): the book's save state, polled while copies run.
    property var sv: ({ state: "none", saveable: 0 })
    property var saveRequest: 0
    readonly property bool saveBusy: sv.state === "queued" || sv.state === "running"
    readonly property bool onNetwork: (sv.saveable || 0) > 0

    function load() {
        request = Core.bookDetail(itemId)
        saveRequest = Core.saveState(itemId)
    }
    function play() {
        if (!loaded || d.drm) return
        if (current && Book.state !== 0 && Book.state !== 5) Book.togglePause()
        else Book.playBook(itemId)
    }
    function saveAction(action) {
        if (action === "save" || action === "saveAgain") saveRequest = Core.saveItem(itemId, 0)
        else if (action === "pause") Core.pauseSavedItem(itemId)
        else if (action === "resume" || action === "retry") Core.resumeSavedItem(itemId)
        else if (action === "cancel" || action === "delete") saveRequest = Core.deleteSavedItem(itemId)
        else if (action === "export") { exportDialog.open(); return }
        saveTimer.restart()
    }

    Component.onCompleted: load()
    onActiveFocusChanged: if (activeFocus && loaded && !content.activeFocus) playButton.forceActiveFocus()

    Timer {
        id: saveTimer
        interval: 1000
        repeat: true
        running: page.saveBusy && page.visible
        onTriggered: page.saveRequest = Core.saveState(page.itemId)
    }

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId !== page.request && requestId !== page.finishRequest) return
            const first = !page.loaded
            page.d = detail
            if (first) playButton.forceActiveFocus()
        }
        function onExportConflict(requestId, paths) {
            if (requestId !== page.exportRequest) return
            page.exportRequest = 0;
            (page.Window.window as Main).showError(qsTr("%n file(s) of this book are in that folder already. Choose another folder.", "", paths.length))
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === page.saveRequest || requestId === page.exportRequest || requestId === page.finishRequest)
                (page.Window.window as Main).showError(message)
        }
        function onSaveStateReady(requestId, id, info) { if (id === page.itemId) page.sv = info }
        function onDownloadsListed(requestId) {
            if (requestId === page.saveRequest) page.saveRequest = Core.saveState(page.itemId)
            if (requestId === page.exportRequest) {
                page.exportRequest = 0;
                (page.Window.window as Main).showError(qsTr("Saving a copy to %1. Progress is under Downloads.").arg(page.exportDest))
            }
        }
        function onItemChanged(id) { if (id === page.itemId) page.load() }
    }
    // The position on the page follows the player while this book plays.
    Connections {
        target: Book
        function onStateChanged() { if (page.current && !Book.playing) page.load() }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.implicitHeight + 96
        clip: true
        ScrollBar.vertical: ScrollBar {}
        function reveal(item) {
            const p = item.mapToItem(contentItem, 0, 0)
            if (p.y < contentY + 40) contentY = Math.max(0, p.y - 40)
            else if (p.y + item.height > contentY + height - 40)
                contentY = Math.min(p.y + item.height - height + 40, Math.max(0, contentHeight - height))
        }

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
                spacing: 24

                RowLayout {
                    spacing: 32
                    Layout.fillWidth: true
                    // The cover keeps its own aspect (square or portrait) in a 280 px square.
                    Item {
                        Layout.preferredWidth: 280
                        Layout.preferredHeight: 280
                        Layout.alignment: Qt.AlignTop
                        readonly property real ratio: page.d.coverRatio > 0 ? page.d.coverRatio : 1
                        Rectangle {
                            anchors.top: parent.top
                            anchors.horizontalCenter: parent.horizontalCenter
                            width: Math.min(280, 280 / parent.ratio)
                            height: Math.min(280, 280 * parent.ratio)
                            radius: Theme.radius
                            clip: true
                            color: Qt.hsla(((page.d.title || "").length * 37 % 360) / 360, 0.30, 0.24, 1)
                            Label {
                                anchors.centerIn: parent
                                width: parent.width - 32
                                visible: cover.status !== Image.Ready
                                text: page.d.title || ""
                                color: Theme.text
                                font.pixelSize: 22
                                font.bold: true
                                wrapMode: Text.Wrap
                                horizontalAlignment: Text.AlignHCenter
                            }
                            Image {
                                id: cover
                                anchors.fill: parent
                                source: page.d.posterFull || page.d.poster || ""
                                fillMode: Image.PreserveAspectFit
                                asynchronous: true
                                sourceSize.width: 560
                            }
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: 10
                        // The series and its position; opens the series.
                        FocusButton {
                            id: seriesButton
                            visible: (page.d.series || "") !== ""
                            text: page.d.seriesPosition > 0
                                  ? qsTr("%1, book %2").arg(page.d.series).arg(Number(page.d.seriesPosition).toLocaleString(Qt.locale(), "f", page.d.seriesPosition % 1 ? 1 : 0))
                                  : (page.d.series || "")
                            tip: qsTr("Open the series")
                            font.pixelSize: Theme.fontSmall
                            Keys.onLeftPressed: Nav.focusRail()
                            KeyNavigation.down: people.count > 0 ? people.itemAt(0) : playButton
                            onClicked: Nav.openItem(page.d.seriesId, "book_series")
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
                        // Authors, then narrators ("read by"); each opens their books.
                        Flow {
                            Layout.fillWidth: true
                            spacing: 8
                            Repeater {
                                id: people
                                model: (page.d.authors || []).map(p => ({ id: p.id, name: p.name, narrator: false }))
                                       .concat((page.d.narrators || []).map(p => ({ id: p.id, name: p.name, narrator: true })))
                                FocusButton {
                                    required property var modelData
                                    required property int index
                                    text: modelData.narrator ? qsTr("Read by %1").arg(modelData.name) : modelData.name
                                    font.pixelSize: Theme.fontSmall
                                    tip: modelData.narrator ? qsTr("Books read by %1").arg(modelData.name) : qsTr("Books by %1").arg(modelData.name)
                                    Keys.onLeftPressed: index === 0 ? Nav.focusRail() : people.itemAt(index - 1).forceActiveFocus()
                                    Keys.onRightPressed: if (index + 1 < people.count) people.itemAt(index + 1).forceActiveFocus()
                                    Keys.onUpPressed: if (seriesButton.visible) seriesButton.forceActiveFocus()
                                    Keys.onDownPressed: playButton.forceActiveFocus()
                                    onClicked: Nav.openItem(modelData.id, "person")
                                }
                            }
                        }
                        Label {
                            text: {
                                const parts = []
                                if (page.d.year > 0) parts.push(page.d.year)
                                if (page.d.durationMs > 0) parts.push(MediaInfo.duration(page.d.durationMs))
                                if (page.d.finished) parts.push(qsTr("Finished"))
                                else if (page.at > 0 && page.d.durationMs > 0)
                                    parts.push(qsTr("%1 left").arg(MediaInfo.duration(Math.max(0, page.d.durationMs - page.at))))
                                if (page.chapters.length > 1) parts.push(qsTr("%n chapter(s)", "", page.chapters.length))
                                if (page.d.publisher) parts.push(page.d.publisher)
                                return parts.join("  ·  ")
                            }
                            color: Theme.textDim
                            font.pixelSize: Theme.fontBody
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: page.d.drm === true
                            text: qsTr("This book is copy-protected (Audible). It is listed, but it cannot be played.")
                            color: Theme.danger
                            wrapMode: Text.Wrap
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: page.d.matchState === "needs_review"
                            text: qsTr("Several books online could be this one. Choose the right one with “Search Open Library…”.")
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                            wrapMode: Text.Wrap
                        }
                        // Wraps when the window is narrow (issue #108 added two buttons).
                        Flow {
                            spacing: 12
                            Layout.topMargin: 8
                            Layout.fillWidth: true
                            FocusButton {
                                id: playButton
                                primary: true
                                enabled: !page.d.drm
                                glyph: page.current && Book.playing ? "❚❚" : "▶"
                                text: page.current && Book.playing ? qsTr("Pause")
                                    : page.started || (page.current && Book.state === 4) ? qsTr("Resume") : qsTr("Play")
                                Keys.onLeftPressed: Nav.focusRail()
                                Keys.onUpPressed: people.count > 0 ? people.itemAt(0).forceActiveFocus() : (seriesButton.visible ? seriesButton.forceActiveFocus() : null)
                                KeyNavigation.right: finishButton
                                KeyNavigation.down: chapterList.count > 0 ? chapterList : null
                                onActiveFocusChanged: if (activeFocus) flick.reveal(this)
                                onClicked: page.play()
                            }
                            FocusButton {
                                id: finishButton
                                text: page.d.finished ? qsTr("Mark as unfinished") : qsTr("Mark as finished")
                                KeyNavigation.left: playButton
                                KeyNavigation.right: saveMenu
                                KeyNavigation.down: chapterList.count > 0 ? chapterList : null
                                onClicked: page.finishRequest = Core.bookMarkFinished(page.itemId, !page.d.finished)
                            }
                            SaveMenu {
                                id: saveMenu
                                sv: page.sv
                                networkFile: page.onNetwork
                                KeyNavigation.left: finishButton
                                KeyNavigation.right: matchButton
                                KeyNavigation.down: chapterList.count > 0 ? chapterList : null
                                onTriggered: (action) => page.saveAction(action)
                            }
                            // Issues #108 and #109: replaces "Search again".
                            FocusButton {
                                id: matchButton
                                objectName: "bookMatchAgain"
                                text: qsTr("Match again")
                                KeyNavigation.left: saveMenu
                                KeyNavigation.right: searchButton
                                KeyNavigation.down: chapterList.count > 0 ? chapterList : null
                                onClicked: bookMatch.again(page.itemId, "audiobook", page.d.title, matchButton)
                            }
                            FocusButton {
                                id: searchButton
                                objectName: "bookSearch"
                                text: qsTr("Search Open Library…")
                                KeyNavigation.left: matchButton
                                KeyNavigation.down: chapterList.count > 0 ? chapterList : null
                                onClicked: bookMatch.search(page.itemId, "audiobook", page.d.title,
                                                            (page.d.authors || []).length > 0 ? page.d.authors[0].name : "", searchButton)
                            }
                        }
                    }
                }

                Label {
                    Layout.fillWidth: true
                    visible: (page.d.description || "") !== ""
                    text: page.d.description || ""
                    color: Theme.text
                    font.pixelSize: Theme.fontBody
                    wrapMode: Text.Wrap
                    lineHeight: 1.2
                }

                Label {
                    visible: page.chapters.length > 0
                    text: qsTr("Chapters")
                    color: Theme.text
                    font.pixelSize: Theme.fontLarge
                    font.bold: true
                    Accessible.role: Accessible.Heading
                }
                ListView {
                    id: chapterList
                    Layout.fillWidth: true
                    Layout.preferredHeight: contentHeight
                    interactive: false
                    model: page.chapters
                    currentIndex: 0
                    keyNavigationEnabled: true
                    Accessible.role: Accessible.List
                    Accessible.name: qsTr("Chapters")
                    Keys.onUpPressed: (e) => { if (currentIndex === 0) playButton.forceActiveFocus(); else e.accepted = false }
                    Keys.onLeftPressed: Nav.focusRail()
                    onActiveFocusChanged: if (activeFocus && currentItem) flick.reveal(currentItem)
                    onCurrentIndexChanged: if (activeFocus && currentItem) flick.reveal(currentItem)
                    delegate: ItemDelegate {
                        id: row
                        required property var modelData
                        required property int index
                        width: chapterList.width
                        height: 44
                        readonly property bool playingHere: index === page.chapterIndex
                        Accessible.name: modelData.title + ", " + MediaInfo.duration(modelData.endMs - modelData.startMs) + (playingHere ? ", " + qsTr("current chapter") : "")
                        background: Rectangle {
                            radius: 6
                            color: row.activeFocus || (chapterList.activeFocus && chapterList.currentIndex === row.index) ? Theme.surfaceHi : "transparent"
                            border.width: chapterList.activeFocus && chapterList.currentIndex === row.index ? Theme.focusWidth : 0
                            border.color: Theme.focus
                        }
                        contentItem: RowLayout {
                            spacing: 14
                            Label {
                                Layout.preferredWidth: 28
                                text: row.playingHere ? "▶" : String(row.index + 1)
                                color: row.playingHere ? Theme.accent : Theme.textDim
                                horizontalAlignment: Text.AlignRight
                            }
                            Label {
                                Layout.fillWidth: true
                                text: row.modelData.title
                                color: row.playingHere ? Theme.accent : Theme.text
                                elide: Text.ElideRight
                            }
                            Label {
                                text: MediaInfo.duration(row.modelData.endMs - row.modelData.startMs)
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                            }
                        }
                        onClicked: { chapterList.currentIndex = row.index; chapterList.play(row.index) }
                    }
                    function play(i) {
                        if (page.d.drm) return
                        if (page.current && Book.active) Book.jumpToChapter(i)
                        else Book.playBook(page.itemId, page.chapters[i].startMs)
                    }
                    Keys.onReturnPressed: play(currentIndex)
                    Keys.onEnterPressed: play(currentIndex)
                }
            }
        }
    }

    BookMatch {
        id: bookMatch
        backend: Core
        onNotice: (text) => (page.Window.window as Main).showError(text)
        onSettingsRequested: (tab) => Nav.settings(tab)
        onChanged: page.load()
    }

    // "Save as…": all of the book's files into a folder.
    FolderDialog {
        id: exportDialog
        title: qsTr("Copy the book's files into")
        currentFolder: Core.exportFolder()
        onAccepted: {
            saveMenu.forceActiveFocus()
            page.exportDest = Core.localPath(selectedFolder)
            page.exportRequest = Core.exportItem(page.itemId, 0, page.exportDest, false)
        }
        onRejected: saveMenu.forceActiveFocus()
    }
}
