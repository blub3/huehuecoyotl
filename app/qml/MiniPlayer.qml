pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// The mini player (issue #8): at the bottom of the shell while there is a queue, on every
// screen but Now Playing. It takes no focus of its own: Down from the last entry of the
// navigation rail reaches it. Left and Right move between its buttons, Up leaves it, Space plays
// or pauses.
FocusScope {
    id: mini
    signal openRequested()
    signal upRequested()
    signal leftRequested()

    implicitHeight: 80
    Accessible.role: Accessible.Grouping
    Accessible.name: qsTr("Music player")
    Keys.onUpPressed: upRequested()
    Keys.onSpacePressed: Music.togglePause()

    Rectangle {
        anchors.fill: parent
        color: Theme.surface
        Rectangle { anchors { left: parent.left; right: parent.right; top: parent.top } height: 1; color: Theme.surfaceHi }
        // Progress along the top edge.
        Rectangle {
            anchors { left: parent.left; top: parent.top }
            height: 3
            width: Music.duration > 0 ? parent.width * Math.min(1, Music.position / Music.duration) : 0
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
            Label { anchors.centerIn: parent; visible: cover.status !== Image.Ready; text: "♫"; color: Theme.textDim; font.pixelSize: 24 }
            Image {
                id: cover
                anchors.fill: parent
                source: Music.current.poster || ""
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
                text: Music.current.title || ""
                color: Theme.text
                font.pixelSize: Theme.fontBody
                elide: Text.ElideRight
            }
            Label {
                Layout.fillWidth: true
                text: [Music.current.artists, Music.current.album].filter(x => x).join("  ·  ")
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
            tip: qsTr("Previous")
            KeyNavigation.right: play
            Keys.onLeftPressed: mini.leftRequested()
            onClicked: Music.previous()
        }
        FocusButton {
            id: play
            focus: true
            primary: true
            glyph: Music.playing ? "❚❚" : "▶"
            tip: Music.playing ? qsTr("Pause") : qsTr("Play")
            KeyNavigation.left: prev
            KeyNavigation.right: next
            onClicked: Music.togglePause()
        }
        FocusButton {
            id: next
            glyph: "⏭"
            tip: qsTr("Next")
            KeyNavigation.left: play
            KeyNavigation.right: open
            onClicked: Music.next()
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
