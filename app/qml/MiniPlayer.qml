pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// The mini player (issue #8): at the bottom of the shell while there is a queue or a book, on every
// screen but Now Playing. It takes no focus of its own: Down from the last entry of the
// navigation rail reaches it. Left and Right move between its buttons, Up leaves it, Space plays
// or pauses.
FocusScope {
    id: mini
    signal openRequested()
    signal upRequested()
    signal leftRequested()

    // Issue #66: the book when it played last (its title, chapter and author); else the music.
    readonly property bool book: Book.front && Book.active
    readonly property real position: book ? Book.position : Music.position
    readonly property real duration: book ? Book.duration : Music.duration

    implicitHeight: 80
    Accessible.role: Accessible.Grouping
    Accessible.name: book ? qsTr("Audiobook player") : qsTr("Music player")
    Keys.onUpPressed: upRequested()
    Keys.onSpacePressed: book ? Book.togglePause() : Music.togglePause()

    Rectangle {
        anchors.fill: parent
        color: Theme.surface
        Rectangle { anchors { left: parent.left; right: parent.right; top: parent.top } height: 1; color: Theme.surfaceHi }
        // Progress along the top edge.
        Rectangle {
            anchors { left: parent.left; top: parent.top }
            height: 3
            width: mini.duration > 0 ? parent.width * Math.min(1, mini.position / mini.duration) : 0
            color: Theme.accent
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        spacing: 14

        // Cover and title lead to Now Playing (also by click).
        Rectangle {
            Layout.preferredWidth: 56
            Layout.preferredHeight: 56
            radius: 6
            color: Theme.surfaceHi
            clip: true
            Label { anchors.centerIn: parent; visible: cover.status !== Image.Ready; text: mini.book ? "❞" : "♫"; color: Theme.textDim; font.pixelSize: 24 }
            Image {
                id: cover
                anchors.fill: parent
                source: mini.book ? (Book.book.poster || "") : (Music.current.poster || "")
                sourceSize.width: 112
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
            }
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: mini.openRequested() }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Label {
                Layout.fillWidth: true
                text: mini.book ? (Book.book.title || "") : (Music.current.title || "")
                color: Theme.text
                font.pixelSize: Theme.fontBody
                elide: Text.ElideRight
            }
            Label {
                Layout.fillWidth: true
                text: mini.book ? [Book.chapterTitle, Book.book.authors].filter(x => x).join("  ·  ")
                                : [Music.current.artists, Music.current.album].filter(x => x).join("  ·  ")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                elide: Text.ElideRight
            }
            MouseArea {
                Layout.fillWidth: true
                Layout.fillHeight: true
                cursorShape: Qt.PointingHandCursor
                onClicked: mini.openRequested()
            }
        }
        FocusButton {
            id: prev
            glyph: "⏮"
            tip: mini.book ? qsTr("Previous chapter") : qsTr("Previous")
            KeyNavigation.right: play
            Keys.onLeftPressed: mini.leftRequested()
            onClicked: mini.book ? Book.previousChapter() : Music.previous()
        }
        FocusButton {
            id: play
            focus: true
            primary: true
            readonly property bool playing: mini.book ? Book.playing : Music.playing
            glyph: playing ? "❚❚" : "▶"
            tip: playing ? qsTr("Pause") : qsTr("Play")
            KeyNavigation.left: prev
            KeyNavigation.right: next
            onClicked: mini.book ? Book.togglePause() : Music.togglePause()
        }
        FocusButton {
            id: next
            glyph: "⏭"
            tip: mini.book ? qsTr("Next chapter") : qsTr("Next")
            KeyNavigation.left: play
            KeyNavigation.right: open
            onClicked: mini.book ? Book.nextChapter() : Music.next()
        }
        FocusButton {
            id: open
            glyph: "≡"
            text: qsTr("Now Playing")
            font.pixelSize: Theme.fontSmall
            KeyNavigation.left: next
            onClicked: mini.openRequested()
        }
    }
}
