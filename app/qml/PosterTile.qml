import QtQuick
import QtQuick.Controls.Basic

// A "continue watching" card. Artwork arrives with the library in M2; until then the tile
// shows the title on a tinted background.
ItemDelegate {
    id: tile
    property string title
    property real progress: 0
    property string subtitle

    width: 280
    height: 168
    focusPolicy: Qt.StrongFocus
    Accessible.name: title
    Accessible.description: subtitle
    scale: activeFocus ? 1.05 : 1
    z: activeFocus ? 1 : 0
    Behavior on scale { NumberAnimation { duration: 120 } }

    background: Rectangle {
        radius: Theme.radius
        gradient: Gradient {
            GradientStop { position: 0; color: Qt.hsla((tile.title.length * 37 % 360) / 360, 0.35, 0.28, 1) }
            GradientStop { position: 1; color: Theme.surface }
        }
        border.width: tile.activeFocus ? Theme.focusWidth : 0
        border.color: Theme.focus
    }

    contentItem: Item {
        Label {
            anchors { left: parent.left; right: parent.right; bottom: sub.top; bottomMargin: 4 }
            text: tile.title
            color: Theme.text
            font.pixelSize: Theme.fontLarge
            font.bold: true
            elide: Text.ElideRight
            maximumLineCount: 2
            wrapMode: Text.Wrap
        }
        Label {
            id: sub
            anchors { left: parent.left; right: parent.right; bottom: bar.top; bottomMargin: 10 }
            text: tile.subtitle
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
        }
        Rectangle {
            id: bar
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            height: 4
            radius: 2
            color: Qt.rgba(1, 1, 1, 0.15)
            Rectangle { width: parent.width * tile.progress; height: parent.height; radius: 2; color: Theme.accent }
        }
    }
}
