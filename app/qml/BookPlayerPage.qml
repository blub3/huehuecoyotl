pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// A book's Now Playing (issue #66): cover, title, author, the chapter and the time left in it
// and in the book, a seek bar over the whole book with chapter marks, previous and next
// chapter, skip back and forward, speed, the sleep timer and the chapter list. Space plays or
// pauses, Left and Right skip (on the seek bar), Page Up and Page Down change chapter.
FocusScope {
    id: page
    objectName: "nowPlaying"
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: qsTr("Now Playing")
    readonly property var b: Book.book
    readonly property var speeds: {
        const out = []
        for (let s = 5; s <= 30; ++s) out.push(s / 10)
        return out
    }
    readonly property var sleepChoices: [0, 5, 10, 15, 30, 45, 60, 90, 120, -1]

    function speedLabel(s) { return Number(s).toLocaleString(Qt.locale(), "f", 1) + "×" }

    Component.onCompleted: Qt.callLater(() => playButton.forceActiveFocus())
    onActiveFocusChanged: if (activeFocus && !content.activeFocus) playButton.forceActiveFocus()
    Keys.onSpacePressed: Book.togglePause()
    Keys.onPressed: (e) => {
        if (e.key === Qt.Key_PageUp) { Book.previousChapter(); e.accepted = true }
        else if (e.key === Qt.Key_PageDown) { Book.nextChapter(); e.accepted = true }
    }

    Label {
        anchors.centerIn: parent
        visible: !Book.active
        text: qsTr("No book is playing. Choose Play on a book.")
        color: Theme.textDim
        font.pixelSize: Theme.fontLarge
    }

    RowLayout {
        id: content
        visible: Book.active
        anchors.fill: parent
        anchors.margins: 48
        spacing: 48

        ColumnLayout {
            id: main
            readonly property real side: Math.max(120, Math.min(width, page.height - 96 - 430))
            Layout.preferredWidth: Math.min(560, (page.width - 96) * 0.52)
            Layout.maximumWidth: Layout.preferredWidth
            Layout.fillHeight: true
            spacing: 12

            Item {
                Layout.preferredWidth: main.side
                Layout.preferredHeight: main.side
                Image {
                    id: cover
                    anchors.fill: parent
                    source: page.b.posterFull || page.b.poster || ""
                    fillMode: Image.PreserveAspectFit
                    horizontalAlignment: Image.AlignLeft
                    asynchronous: true
                    sourceSize.width: 1000
                }
                Rectangle {
                    anchors.fill: parent
                    visible: cover.status !== Image.Ready
                    radius: Theme.radius
                    color: Theme.surface
                    Label { anchors.centerIn: parent; text: "❞"; color: Theme.textDim; font.pixelSize: 96 }
                }
            }
            Label {
                Layout.fillWidth: true
                text: page.b.title || ""
                color: Theme.text
                font.pixelSize: 28
                font.bold: true
                wrapMode: Text.Wrap
                maximumLineCount: 2
                elide: Text.ElideRight
                Accessible.role: Accessible.Heading
            }
            Label {
                Layout.fillWidth: true
                text: page.b.authors || ""
                color: Theme.textDim
                elide: Text.ElideRight
            }
            Label {
                Layout.fillWidth: true
                text: Book.chapterTitle !== ""
                      ? qsTr("%1  ·  %2 left in the chapter").arg(Book.chapterTitle).arg(Theme.time(Book.chapterLeft / Book.speed))
                      : ""
                color: Theme.accent
                elide: Text.ElideRight
            }

            // The whole book, with chapter marks.
            Slider {
                id: seek
                Layout.fillWidth: true
                Layout.topMargin: 6
                from: 0
                to: Math.max(1, Book.duration)
                value: pressed ? value : Book.position
                focusPolicy: Qt.StrongFocus
                Accessible.name: qsTr("Position in the book")
                onMoved: Book.seekTo(value)
                Keys.onLeftPressed: Book.skip(-1)
                Keys.onRightPressed: Book.skip(1)
                KeyNavigation.down: playButton
                background: Rectangle {
                    x: seek.leftPadding
                    y: seek.topPadding + seek.availableHeight / 2 - height / 2
                    width: seek.availableWidth
                    height: 4
                    radius: 2
                    color: Theme.surfaceHi
                    Rectangle { width: seek.visualPosition * parent.width; height: parent.height; radius: 2; color: Theme.accent }
                    Repeater {
                        model: Book.duration > 0 && Book.chapters.length < 200 ? Book.chapters : []
                        Rectangle {
                            required property var modelData
                            visible: modelData.startMs > 0
                            x: parent.width * modelData.startMs / 1000 / Book.duration - 1
                            y: -3
                            width: 2
                            height: 10
                            color: Theme.textDim
                        }
                    }
                }
                handle: Rectangle {
                    x: seek.leftPadding + seek.visualPosition * (seek.availableWidth - width)
                    y: seek.topPadding + seek.availableHeight / 2 - height / 2
                    width: seek.activeFocus ? 20 : 14
                    height: width
                    radius: width / 2
                    color: Theme.text
                    border.width: seek.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Label { text: Theme.time(Book.position); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                Item { Layout.fillWidth: true }
                Label {
                    text: qsTr("%1 left").arg(Theme.time(Math.max(0, Book.duration - Book.position) / Book.speed))
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
            }

            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                spacing: 10
                FocusButton {
                    id: prevChapter
                    glyph: "⏮"
                    tip: qsTr("Previous chapter")
                    KeyNavigation.right: back
                    KeyNavigation.up: seek
                    KeyNavigation.down: faster
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Book.previousChapter()
                }
                FocusButton {
                    id: back
                    text: qsTr("−%1 s").arg(Book.skipBack)
                    tip: qsTr("Back %n second(s)", "", Book.skipBack)
                    KeyNavigation.left: prevChapter
                    KeyNavigation.right: playButton
                    KeyNavigation.up: seek
                    KeyNavigation.down: faster
                    onClicked: Book.skip(-1)
                }
                FocusButton {
                    id: playButton
                    primary: true
                    glyph: Book.playing ? "❚❚" : "▶"
                    tip: Book.playing ? qsTr("Pause") : qsTr("Play")
                    KeyNavigation.left: back
                    KeyNavigation.right: forward
                    KeyNavigation.up: seek
                    KeyNavigation.down: faster
                    onClicked: Book.togglePause()
                }
                FocusButton {
                    id: forward
                    text: qsTr("+%1 s").arg(Book.skipForward)
                    tip: qsTr("Forward %n second(s)", "", Book.skipForward)
                    KeyNavigation.left: playButton
                    KeyNavigation.right: nextChapter
                    KeyNavigation.up: seek
                    KeyNavigation.down: faster
                    onClicked: Book.skip(1)
                }
                FocusButton {
                    id: nextChapter
                    glyph: "⏭"
                    tip: qsTr("Next chapter")
                    KeyNavigation.left: forward
                    KeyNavigation.right: chapterList
                    KeyNavigation.up: seek
                    KeyNavigation.down: faster
                    onClicked: Book.nextChapter()
                }
            }
            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                spacing: 10
                // Speed: slower and faster in steps of 0.1 (0.5× to 3.0×).
                FocusButton {
                    id: slower
                    text: "−"
                    tip: qsTr("Slower")
                    enabled: Book.speed > 0.5
                    KeyNavigation.right: faster
                    KeyNavigation.up: playButton
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Book.setSpeed(Book.speed - 0.1)
                }
                Label {
                    text: page.speedLabel(Book.speed)
                    color: Theme.text
                    Layout.minimumWidth: 48
                    horizontalAlignment: Text.AlignHCenter
                    Accessible.name: qsTr("Speed %1").arg(page.speedLabel(Book.speed))
                }
                FocusButton {
                    id: faster
                    text: "+"
                    tip: qsTr("Faster")
                    enabled: Book.speed < 3.0
                    KeyNavigation.left: slower
                    KeyNavigation.right: sleepButton
                    KeyNavigation.up: playButton
                    onClicked: Book.setSpeed(Book.speed + 0.1)
                }
                // Sleep timer: off, 5 to 120 minutes, end of chapter; Enter steps through them.
                FocusButton {
                    id: sleepButton
                    text: Book.sleep === "chapter" ? qsTr("Sleep: end of chapter")
                        : Book.sleep === "time" ? qsTr("Sleep in %1").arg(Theme.time(Book.sleepLeft))
                        : qsTr("Sleep timer")
                    tip: qsTr("Stop after a while or at the end of the chapter")
                    KeyNavigation.left: faster
                    KeyNavigation.up: playButton
                    property int step: 0
                    onClicked: {
                        step = (step + 1) % page.sleepChoices.length
                        const m = page.sleepChoices[step]
                        if (m === 0) Book.cancelSleep()
                        else if (m < 0) Book.sleepAtChapterEnd()
                        else Book.sleepAfter(m)
                    }
                    Connections {
                        target: Book
                        function onSleepChanged() { if (Book.sleep === "off") sleepButton.step = 0 }
                    }
                }
            }
            Item { Layout.fillHeight: true }
        }

        // ---- chapters -------------------------------------------------------------------------
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 12
            Label {
                text: qsTr("Chapters")
                color: Theme.text
                font.pixelSize: Theme.fontLarge
                font.bold: true
                Accessible.role: Accessible.Heading
            }
            ListView {
                id: chapterList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: Book.chapters
                currentIndex: Math.max(0, Book.chapter)
                keyNavigationEnabled: true
                highlightFollowsCurrentItem: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Chapters")
                ScrollBar.vertical: ScrollBar {}
                Keys.onLeftPressed: nextChapter.forceActiveFocus()
                Keys.onReturnPressed: Book.jumpToChapter(currentIndex)
                Keys.onEnterPressed: Book.jumpToChapter(currentIndex)
                onActiveFocusChanged: if (activeFocus) positionViewAtIndex(currentIndex, ListView.Contain)
                delegate: ItemDelegate {
                    id: row
                    required property var modelData
                    required property int index
                    width: chapterList.width
                    height: 44
                    readonly property bool playingHere: index === Book.chapter
                    readonly property bool focused: chapterList.activeFocus && chapterList.currentIndex === index
                    Accessible.name: modelData.title + (playingHere ? ", " + qsTr("current chapter") : "")
                    background: Rectangle {
                        radius: 6
                        color: row.focused ? Theme.surfaceHi : "transparent"
                        border.width: row.focused ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    onClicked: { chapterList.currentIndex = index; Book.jumpToChapter(index) }
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
                            text: Theme.time((row.modelData.endMs - row.modelData.startMs) / 1000)
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                        }
                    }
                }
            }
        }
    }
}
