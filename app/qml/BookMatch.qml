pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Matching audiobooks again (issues #108 to #110), on the book page and the author page:
// `again(id, kind, title, from)` matches a book or an author again (asking first when the user
// chose its match); `search(id, kind, title, author, from)` opens "Find the book" (title, author,
// ISBN) or "Find the author" at Open Library. Enter on a book shows what it would add, field by
// field (yours, Open Library's, which is shown); Enter again chooses. Authors show their dates,
// works and how many of your books are theirs. `notice(text)` reports a result, `changed(id)`
// says the item changed. `backend` is the core (Core in the app, a stub in tests).
Item {
    id: matcher
    property var backend: null
    property var itemId: 0
    // audiobook | person
    property string kind: "audiobook"
    property string title: ""
    property string author: ""
    property Item from: null
    property var request: 0
    property var candidates: []
    property int expanded: -1
    property var preview: null
    property bool searching: false
    property string message: ""
    property string hint: ""
    signal notice(string text)
    signal changed(var newItemId)

    readonly property bool offline: backend !== null && backend.musicOnline === false
    readonly property string offlineText: qsTr("Turn on Music and book information online under Settings → Metadata to look books up at Open Library.")
    readonly property bool isBook: kind === "audiobook"

    function again(id, k, t, item) {
        itemId = id; kind = k; title = t || ""; from = item
        if (offline) { notice(offlineText); return }
        request = backend.matchAgain(id, false)
    }
    function search(id, k, t, a, item) {
        itemId = id; kind = k; title = t || ""; author = a || ""; from = item
        hint = ""
        openSearch()
    }
    function openSearch() {
        firstField.text = title
        authorField.text = author
        isbnField.text = ""
        candidates = []
        expanded = -1
        preview = null
        message = ""
        dialog.open()
        run()
    }
    function run() {
        if (offline) { message = offlineText; candidates = []; return }
        searching = true
        message = ""
        expanded = -1
        preview = null
        const q = { text: firstField.text }
        if (isBook) { q.author = authorField.text; q.isbn = isbnField.text }
        request = backend.matchSearch(itemId, q)
    }
    function expand(i) {
        if (expanded === i) { choose(); return }
        expanded = i
        preview = null
        if (isBook) request = backend.matchPreview(itemId, { id: candidates[i].id, kind: "work" })
    }
    function choose() {
        if (expanded < 0) return
        const c = candidates[expanded]
        request = backend.matchChoose(itemId, { id: c.id, kind: c.kind })
    }
    function lockText() {
        return isBook ? qsTr("You chose the match of this book yourself. Match it again anyway? A sure match replaces your choice.")
                      : qsTr("You chose the match of this author yourself. Match it again anyway? A sure match replaces your choice.")
    }
    function fieldName(f) {
        switch (f) {
        case "description": return qsTr("Description")
        case "year": return qsTr("Year")
        case "subjects": return qsTr("Subjects")
        case "cover": return qsTr("Cover")
        }
        return f
    }
    function fitText(c) {
        if (isBook) return c.good ? qsTr("Title and author agree") : ""
        return c.books === undefined || c.books === null ? "" : qsTr("%1 of your %2 books are theirs").arg(c.books).arg(c.ofBooks)
    }
    function metaText(c) {
        if (isBook) return [c.artist, c.year].filter(x => x).join(" · ")
        const life = c.birth ? (c.birth + (c.death ? " – " + c.death : "")) : ""
        return [life, c.workCount ? qsTr("%n work(s)", "", c.workCount) : "", c.topWork].filter(x => x).join(" · ")
    }

    Connections {
        target: matcher.backend
        ignoreUnknownSignals: true
        function onMusicDetailReady(requestId, k, detail) {
            if (requestId !== matcher.request) return
            if (k === "matchAgain") {
                switch (detail.result) {
                case "locked": lockConfirm.open(); return
                case "unchanged": matcher.notice(qsTr("Matched again: no change, “%1” was already right.").arg(matcher.title)); break
                case "newly": case "changed":
                    matcher.notice(qsTr("Matched again: “%1”.").arg(detail.title || matcher.title))
                    matcher.changed(detail.newItemId)
                    break
                case "review":
                    matcher.hint = qsTr("Several matches fit. Choose one.")
                    matcher.openSearch()
                    break
                case "notFound": matcher.notice(qsTr("Nothing sure was found for “%1”; it stays as it is.").arg(matcher.title)); break
                }
            } else if (k === "matchCandidates") {
                matcher.searching = false
                matcher.candidates = detail.candidates || []
            } else if (k === "matchPreview") {
                matcher.preview = detail
            } else if (k === "matchChosen") {
                dialog.close()
                matcher.notice(qsTr("Matched: “%1”.").arg(matcher.candidates[matcher.expanded] ? matcher.candidates[matcher.expanded].title : matcher.title))
                matcher.changed(detail.newItemId)
            }
        }
        function onRequestFailed(requestId, k, message) {
            if (requestId !== matcher.request) return
            matcher.searching = false
            if (dialog.visible) matcher.message = message; else matcher.notice(message)
        }
    }

    component Field: TextField {
        id: field
        color: Theme.text
        placeholderTextColor: Theme.textDim
        Accessible.name: placeholderText
        background: Rectangle {
            implicitHeight: 44
            radius: Theme.radius
            color: Theme.bg
            border.width: field.activeFocus ? Theme.focusWidth : 1
            border.color: field.activeFocus ? Theme.focus : Theme.surfaceHi
        }
    }

    Popup {
        id: lockConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, (parent ? parent.width : 800) - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: keepButton.forceActiveFocus()
        onClosed: if (matcher.from && !dialog.visible) matcher.from.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label { objectName: "lockQuestion"; Layout.fillWidth: true; wrapMode: Text.Wrap; color: Theme.text; text: matcher.lockText() }
            RowLayout {
                spacing: 12
                FocusButton { id: keepButton; text: qsTr("Keep my choice"); KeyNavigation.right: unlockButton; onClicked: lockConfirm.close() }
                FocusButton {
                    id: unlockButton
                    objectName: "matchAnyway"
                    primary: true
                    text: qsTr("Match again")
                    KeyNavigation.left: keepButton
                    onClicked: { lockConfirm.close(); matcher.request = matcher.backend.matchAgain(matcher.itemId, true) }
                }
            }
        }
    }

    Popup {
        id: dialog
        objectName: "bookMatchDialog"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(parent ? parent.width - 80 : 800, 900)
        height: Math.min(parent ? parent.height - 80 : 700, 780)
        modal: true
        focus: true
        padding: 24
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: firstField.forceActiveFocus()
        onClosed: if (matcher.from) matcher.from.forceActiveFocus()

        contentItem: ColumnLayout {
            spacing: 12
            Keys.onPressed: (e) => { if (e.key === Qt.Key_Back) { dialog.close(); e.accepted = true } }
            RowLayout {
                spacing: 12
                Label {
                    objectName: "searchHeading"
                    text: matcher.isBook ? qsTr("Find the book") : qsTr("Find the author")
                    color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true
                }
                Label { text: qsTr("at Open Library"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            }
            Label { visible: matcher.hint !== ""; text: matcher.hint; color: Theme.accent; Layout.fillWidth: true; wrapMode: Text.Wrap }
            RowLayout {
                spacing: 10
                Layout.fillWidth: true
                Field {
                    id: firstField
                    onAccepted: matcher.run()
                    objectName: "firstField"
                    Layout.fillWidth: true
                    placeholderText: matcher.isBook ? qsTr("Title") : qsTr("Name")
                    KeyNavigation.right: matcher.isBook ? authorField : searchButton
                    KeyNavigation.down: results
                }
                Field {
                    id: authorField
                    onAccepted: matcher.run()
                    visible: matcher.isBook
                    Layout.preferredWidth: 220
                    placeholderText: qsTr("Author")
                    KeyNavigation.left: firstField
                    KeyNavigation.right: isbnField
                    KeyNavigation.down: results
                }
                Field {
                    id: isbnField
                    onAccepted: matcher.run()
                    objectName: "isbnField"
                    visible: matcher.isBook
                    Layout.preferredWidth: 160
                    placeholderText: qsTr("ISBN")
                    KeyNavigation.left: authorField
                    KeyNavigation.right: searchButton
                    KeyNavigation.down: results
                }
                FocusButton {
                    id: searchButton
                    text: qsTr("Search")
                    KeyNavigation.left: matcher.isBook ? isbnField : firstField
                    KeyNavigation.down: results
                    onClicked: matcher.run()
                }
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: matcher.isBook ? qsTr("Only fields your files leave empty are filled; your own metadata stays. Or paste an Open Library link or an ISBN.")
                                     : qsTr("Several authors can share a name; their dates, works and your books tell them apart. Or paste an Open Library link.")
            }
            Label { visible: matcher.message !== ""; text: matcher.message; color: Theme.danger; Layout.fillWidth: true; wrapMode: Text.Wrap }
            BusyIndicator { visible: matcher.searching; running: visible; Layout.alignment: Qt.AlignHCenter }
            Label {
                visible: !matcher.searching && matcher.candidates.length === 0 && matcher.message === ""
                text: qsTr("No results. Try another spelling, or leave the author out.")
                color: Theme.textDim
            }
            ListView {
                id: results
                objectName: "results"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 6
                model: matcher.candidates
                keyNavigationEnabled: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Matches")
                Keys.onUpPressed: (e) => { if (currentIndex === 0) firstField.forceActiveFocus(); else e.accepted = false }
                onCurrentIndexChanged: if (matcher.expanded >= 0 && matcher.expanded !== currentIndex) { matcher.expanded = -1; matcher.preview = null }
                delegate: ItemDelegate {
                    id: cand
                    required property var modelData
                    required property int index
                    readonly property bool open: matcher.expanded === index
                    width: ListView.view.width
                    height: Math.max(92, body.implicitHeight + 20)
                    focus: ListView.isCurrentItem
                    Accessible.name: modelData.title + ", " + matcher.metaText(modelData) + ", " + matcher.fitText(modelData)
                    onClicked: { results.currentIndex = index; matcher.expand(index) }
                    Keys.onReturnPressed: matcher.expand(index)
                    Keys.onEnterPressed: matcher.expand(index)
                    background: Rectangle {
                        radius: Theme.radius
                        color: cand.activeFocus || cand.hovered || cand.open ? Theme.surfaceHi : "transparent"
                        border.width: cand.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        id: body
                        spacing: 14
                        Rectangle {
                            Layout.preferredWidth: matcher.isBook ? 50 : 60
                            Layout.preferredHeight: matcher.isBook ? 72 : 60
                            Layout.alignment: Qt.AlignTop
                            radius: matcher.isBook ? 4 : 30
                            clip: true
                            color: Theme.bg
                            Image { anchors.fill: parent; source: cand.modelData.image || ""; asynchronous: true; fillMode: Image.PreserveAspectCrop }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.alignment: Qt.AlignTop
                            spacing: 3
                            RowLayout {
                                spacing: 8
                                Label { text: cand.modelData.title; color: Theme.text; font.bold: true; font.pixelSize: Theme.fontBody; Layout.fillWidth: true; elide: Text.ElideRight }
                                Label { visible: !!cand.modelData.current; text: qsTr("matched now"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                            }
                            Label { text: matcher.metaText(cand.modelData); color: Theme.textDim; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; elide: Text.ElideRight }
                            Label {
                                objectName: "fit" + cand.index
                                visible: text !== ""
                                text: (cand.modelData.good ? "✓ " : "△ ") + matcher.fitText(cand.modelData)
                                color: cand.modelData.good ? Theme.accent : Theme.text
                                font.pixelSize: Theme.fontSmall
                                font.bold: true
                            }
                            // Issue #109: what the work would add, field by field.
                            GridLayout {
                                objectName: "fieldTable"
                                visible: cand.open && matcher.isBook && !!matcher.preview
                                Layout.fillWidth: true
                                Layout.topMargin: 6
                                columns: 4
                                columnSpacing: 14
                                rowSpacing: 3
                                Label { text: qsTr("Field"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                                Label { text: qsTr("Yours"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                                Label { text: qsTr("Open Library"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                                Label { text: qsTr("Shown"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                                Repeater {
                                    model: cand.open && matcher.preview ? (matcher.preview.fields || []).length * 4 : 0
                                    Label {
                                        required property int index
                                        readonly property var f: matcher.preview ? (matcher.preview.fields || [])[Math.floor(index / 4)] || ({}) : ({})
                                        readonly property int col: index % 4
                                        Layout.fillWidth: col === 1 || col === 2
                                        Layout.preferredWidth: col === 1 || col === 2 ? 200 : -1
                                        elide: Text.ElideRight
                                        font.pixelSize: Theme.fontSmall
                                        font.bold: col === 3
                                        color: col === 3 && f.shown === "openLibrary" ? Theme.accent : Theme.text
                                        text: col === 0 ? matcher.fieldName(f.field || "")
                                              : col === 1 ? (f.yours || "—")
                                              : col === 2 ? (f.theirs || "—")
                                              : f.shown === "openLibrary" ? qsTr("Open Library") : qsTr("yours")
                                    }
                                }
                            }
                        }
                    }
                }
            }
            RowLayout {
                visible: matcher.expanded >= 0
                Layout.fillWidth: true
                spacing: 12
                Label {
                    text: matcher.isBook ? qsTr("The match is locked. The book's authors without a match are matched with it.")
                                         : qsTr("The photo and biography come from Open Library; a biography of your own stays.")
                    color: Theme.text; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; wrapMode: Text.Wrap
                }
                FocusButton { text: qsTr("Cancel"); KeyNavigation.right: useButton; onClicked: dialog.close() }
                FocusButton { id: useButton; objectName: "useMatch"; primary: true; text: qsTr("Use this match"); onClicked: matcher.choose() }
            }
            Label {
                text: qsTr("↓ to the results · Enter shows the comparison, Enter again chooses · Back closes")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
            }
        }
    }
}
