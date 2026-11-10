import QtQuick
import QtTest
import CoyotlUi

Item {
    id: root
    width: 1280
    height: 720

    QtObject {
        id: stub
        property bool paused: false
        property double position: 100
        property double duration: 1000
        property double cacheSeconds: 20
        property int volume: 100
        property bool muted: false
        property double speed: 1
        property int state: 3
        property var audioTracks: [{ id: 1, label: "English · AC3" }, { id: 2, label: "Deutsch · AAC" }]
        property var subtitleTracks: [{ id: 1, label: "English · SUBRIP" }]
        property int audioTrack: 1
        property int subtitleTrack: 0
        property var chapters: []
        property var seeks: []
        function togglePause() { paused = !paused }
        function seek(s, relative) { seeks.push([s, relative === undefined ? true : relative]) }
        function addChapter(d) {}
    }

    PlayerControls {
        id: controls
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        player: stub
    }

    TestCase {
        name: "PlayerControls"
        when: windowShown

        function init() {
            stub.paused = false
            stub.seeks = []
            stub.subtitleTrack = 0
            controls.focusDefault()
        }

        function test_defaultFocusIsPlay() {
            const play = findChild(controls, "playButton")
            verify(play.activeFocus)
            keyClick(Qt.Key_Return)
            verify(stub.paused, "Enter on play toggles pause")
        }

        function test_arrowNavigationAndSeekBar() {
            keyClick(Qt.Key_Up)
            const bar = findChild(controls, "seekBar")
            verify(bar.activeFocus, "Up from the buttons reaches the seek bar")
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Left)
            compare(stub.seeks.length, 2)
            compare(stub.seeks[0][0], 10)
            compare(stub.seeks[1][0], -10)
            keyClick(Qt.Key_Down)
            verify(findChild(controls, "playButton").activeFocus)
        }

        function test_subtitleMenuByKeyboard() {
            // play -> +10 -> audio -> subtitles
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Right)
            verify(findChild(controls, "subButton").activeFocus)
            keyClick(Qt.Key_Return)
            tryVerify(() => controls.menuOpen)
            keyClick(Qt.Key_Down)      // "Off" -> "English"
            keyClick(Qt.Key_Return)
            tryVerify(() => !controls.menuOpen)
            compare(stub.subtitleTrack, 1)
            verify(findChild(controls, "subButton").activeFocus, "focus returns to the button that opened the menu")
        }
    }
}
