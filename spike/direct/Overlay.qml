import QtQuick
import QtQuick.Controls.Basic

// Transparent overlay window: only the control bar is opaque; the rest shows mpv's
// native video window underneath.
Window {
    id: ov
    required property MpvPlayer player
    property var videoWindow: null
    color: "transparent"
    visible: false

    Item {
        anchors.fill: parent
        focus: true
        Keys.onPressed: (e) => {
            switch (e.key) {
            case Qt.Key_Space: ov.player.togglePause(); break
            case Qt.Key_Left: ov.player.seek(-10); break
            case Qt.Key_Right: ov.player.seek(10); break
            case Qt.Key_F:
                if (ov.videoWindow)
                    ov.videoWindow.visibility = ov.videoWindow.visibility === Window.FullScreen
                        ? Window.Windowed : Window.FullScreen
                break
            case Qt.Key_Q: case Qt.Key_Escape: Qt.quit(); break
            default: return
            }
            e.accepted = true
        }
        // Catch clicks on the transparent area so they never reach mpv's window.
        MouseArea { anchors.fill: parent; onDoubleClicked: ov.videoWindow.visibility =
            ov.videoWindow.visibility === Window.FullScreen ? Window.Windowed : Window.FullScreen }
    }

    Rectangle {
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: 96
        color: "#b0101418"
        Column {
            anchors.fill: parent; anchors.margins: 12; spacing: 8
            Row {
                spacing: 12; width: parent.width
                Button { text: ov.player.paused ? "▶" : "❚❚"; onClicked: ov.player.togglePause(); focusPolicy: Qt.NoFocus }
                Slider {
                    width: parent.width - 220
                    from: 0; to: Math.max(1, ov.player.duration)
                    value: ov.player.position
                    focusPolicy: Qt.NoFocus
                    onMoved: ov.player.seek(value, false)
                }
                Label {
                    color: "white"; anchors.verticalCenter: parent.verticalCenter
                    text: "DIRECT  " + ov.player.state + "  " + Math.floor(ov.player.position) + " / " + Math.floor(ov.player.duration) + " s"
                }
            }
            Label { color: "#9fd"; font.family: "monospace"; font.pixelSize: 12; text: ov.player.stats }
        }
    }
}
