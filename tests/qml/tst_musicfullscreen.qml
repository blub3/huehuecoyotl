import QtQuick
import QtTest
import CoyotlUi

// Issue #83: the full-screen Now Playing view with a stub music player.
Item {
    id: root
    width: 1920
    height: 1080

    QtObject {
        id: stub
        property bool playing: true
        property double position: 30
        property double duration: 240
        property bool shuffle: false
        property string repeat: "off"
        property var current: ({ id: 1, title: "First song", artists: "Artist", album: "Album", codec: "flac", sampleRate: 96000, bitDepth: 24, hires: true })
        property var queue: [{ index: 0, id: 1, title: "First song", artists: "Artist" },
                             { index: 1, id: 2, title: "Second song", artists: "Artist" },
                             { index: 2, id: 3, title: "Third song", artists: "Artist" }]
        property int queueOffset: 0
        property int queueCount: 3
        property int queuePosition: 0
        property var seeks: []
        property int toggles: 0
        property var jumps: []
        property int nexts: 0
        function togglePause() { toggles++; playing = !playing }
        function seek(s, relative) { seeks.push([s, relative]) }
        function next() { nexts++ }
        function previous() {}
        function cycleRepeat() {}
        function jump(i) { jumps.push(i) }
    }

    property int leaves: 0
    Loader {
        id: loader
        anchors.fill: parent
        active: false
        focus: true
        sourceComponent: MusicFullScreen {
            player: stub
            onLeaveRequested: root.leaves++
        }
    }

    TestCase {
        name: "MusicFullScreen"
        when: windowShown

        function enter(idleMs) {
            loader.active = true
            const v = loader.item
            if (idleMs) v.idleMs = idleMs
            v.wake()
            loader.forceActiveFocus()
            tryVerify(() => findChild(v, "fullPlay").activeFocus, 2000, "Play / Pause has the focus on entry")
            return v
        }

        function init() {
            stub.playing = true
            stub.seeks = []
            stub.toggles = 0
            stub.jumps = []
            stub.nexts = 0
            stub.queuePosition = 0
            stub.current = { id: 1, title: "First song", artists: "Artist", album: "Album", codec: "flac", sampleRate: 96000, bitDepth: 24, hires: true }
            root.leaves = 0
        }
        function cleanup() { loader.active = false }

        function test_enterAndLeave() {
            const v = enter()
            compare(v.idleMs, 5000, "controls fade after 5 s by default")
            verify(v.awake)
            compare(findChild(v, "fullTitle").text, "First song")
            keyClick(Qt.Key_Escape)
            compare(root.leaves, 1, "Back leaves the view")
        }

        function test_keysOnPlay() {
            enter()
            keyClick(Qt.Key_Return)
            compare(stub.toggles, 1)
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Left)
            compare(stub.seeks.length, 2)
            compare(stub.seeks[0][0], 10)
            compare(stub.seeks[1][0], -10)
        }

        function test_otherControls() {
            const v = enter()
            keyClick(Qt.Key_Up)
            verify(findChild(v, "fullPrevious").activeFocus, "↑ reaches the other controls")
            keyClick(Qt.Key_Right)
            verify(findChild(v, "fullNext").activeFocus)
            keyClick(Qt.Key_Return)
            compare(stub.nexts, 1)
            keyClick(Qt.Key_Down)
            verify(findChild(v, "fullPlay").activeFocus, "↓ goes back to Play / Pause")
        }

        function test_fadeAndWakeWithoutActing() {
            const v = enter(300)
            tryVerify(() => !v.awake, 3000, "controls fade without input")
            tryCompare(findChild(v, "fullControls"), "opacity", 0, 3000)
            verify(findChild(v, "fullTitle").visible, "the title stays")
            keyClick(Qt.Key_Return)
            verify(v.awake, "a key wakes the view")
            compare(stub.toggles, 0, "the waking key does not act")
            keyClick(Qt.Key_Return)
            compare(stub.toggles, 1, "the next key acts")

            tryVerify(() => !v.awake, 3000)
            keyClick(Qt.Key_Escape)
            verify(v.awake)
            compare(root.leaves, 0, "Back while faded only wakes")

            tryVerify(() => !v.awake, 3000)
            mouseClick(findChild(v, "fullPlay"))
            verify(v.awake, "a click wakes the view")
            compare(stub.toggles, 1, "the waking click does not act")
        }

        function test_queueOverlay() {
            const v = enter()
            keyClick(Qt.Key_Down)
            verify(v.queueOpen, "↓ opens the queue")
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Return)
            compare(stub.jumps, [1])
            verify(!v.queueOpen, "playing an entry closes the queue")
            verify(findChild(v, "fullPlay").activeFocus)
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Escape)
            verify(!v.queueOpen, "Back closes the queue")
            compare(root.leaves, 0)
        }

        function test_followsTrackChanges() {
            const v = enter()
            verify(findChild(v, "upNextCard").visible)
            stub.current = { id: 2, title: "Second song", artists: "Artist" }
            stub.queuePosition = 1
            compare(findChild(v, "fullTitle").text, "Second song")
            stub.queuePosition = 2
            verify(!findChild(v, "upNextCard").visible, "no card after the last entry")
        }
    }
}
