import QtQuick
import QtQuick.Controls.Basic
import Spike

ApplicationWindow {
    id: win
    required property MpvPlayer player
    property bool startFullscreen: false
    width: 1280; height: 720
    visible: true
    visibility: startFullscreen ? Window.FullScreen : Window.Windowed
    color: "black"
    title: "coyotl spike — embedded"

    MpvItem {
        id: video
        anchors.fill: parent
        player: win.player
        focus: true
        Keys.onPressed: (e) => {
            switch (e.key) {
            case Qt.Key_Space: win.player.togglePause(); break
            case Qt.Key_Left: win.player.seek(-10); break
            case Qt.Key_Right: win.player.seek(10); break
            case Qt.Key_F: win.visibility = win.visibility === Window.FullScreen ? Window.Windowed : Window.FullScreen; break
            case Qt.Key_Q: case Qt.Key_Escape: Qt.quit(); break
            default: return
            }
            e.accepted = true
        }
    }

    // QML composited on top of the video: proves the overlay path in embedded mode.
    Rectangle {
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: 96
        color: "#b0101418"
        Column {
            anchors.fill: parent; anchors.margins: 12; spacing: 8
            Row {
                spacing: 12; width: parent.width
                Button {
                    text: win.player.paused ? "▶" : "❚❚"
                    onClicked: win.player.togglePause()
                    focusPolicy: Qt.NoFocus
                }
                Slider {
                    width: parent.width - 220
                    from: 0; to: Math.max(1, win.player.duration)
                    value: win.player.position
                    focusPolicy: Qt.NoFocus
                    onMoved: win.player.seek(value, false)
                }
                Label {
                    color: "white"; anchors.verticalCenter: parent.verticalCenter
                    text: win.player.state + "  " + Math.floor(win.player.position) + " / " + Math.floor(win.player.duration) + " s"
                }
            }
            Label { color: "#9fd"; font.family: "monospace"; font.pixelSize: 12; text: win.player.stats }
        }
    }
}
