import QtQuick
import QtTest
import CoyotlUi

// Issue #137: the mini player with stub players: Play and Pause the same size and nothing moving
// when playback starts, Shuffle and Repeat showing and changing their state, the controls centred
// whatever the title, Left and Right through every control, the times and Volume giving way in
// narrow windows.
Item {
    id: root
    width: 1280
    height: 300

    QtObject {
        id: music
        property bool playing: false
        property bool active: true
        property real position: 124
        property real duration: 332
        property var current: ({ title: "Mother", artists: "Pink Floyd", album: "The Wall" })
        property bool shuffle: false
        property string repeat: "off"
        property bool bitPerfect: false
        property int volume: 80
        property var calls: []
        function togglePause() { playing = !playing; calls = calls.concat(["toggle"]) }
        function next() { calls = calls.concat(["next"]) }
        function previous() { calls = calls.concat(["previous"]) }
        function cycleRepeat() { repeat = repeat === "off" ? "all" : repeat === "all" ? "one" : "off" }
    }
    QtObject {
        id: book
        property bool front: false
        property bool active: false
        property bool playing: true
        property real position: 1300
        property real duration: 9000
        property var book: ({ title: "A Test Book", authors: "Test Author" })
        property string chapterTitle: "Chapter 7"
        property var chapters: [{ startMs: 0, endMs: 1200000 }, { startMs: 1200000, endMs: 2645000 }]
        property int chapter: 1
        property int skipBack: 15
        property int skipForward: 30
        property real speed: 1.0
        property var calls: []
        function skip(d) { calls = calls.concat(["skip " + d]) }
        function setSpeed(s) { speed = s }
        function previousChapter() { calls = calls.concat(["previous chapter"]) }
        function nextChapter() { calls = calls.concat(["next chapter"]) }
        function togglePause() { playing = !playing }
    }

    property var events: []
    MiniPlayer {
        id: mini
        y: 100
        width: root.width
        music: music
        bookPlayer: book
        onOpenRequested: root.events = root.events.concat(["open"])
        onLeftRequested: root.events = root.events.concat(["left"])
    }

    TestCase {
        name: "MiniPlayer"
        when: windowShown

        function part(name) { return findChild(mini, name) }
        function xOf(name) { const p = part(name); return p.mapToItem(root, 0, 0).x }
        function init() {
            mini.width = root.width
            music.playing = false
            music.shuffle = false
            music.repeat = "off"
            music.calls = []
            music.current = { title: "Mother", artists: "Pink Floyd", album: "The Wall" }
            book.front = false
            book.active = false
            book.calls = []
            book.speed = 1.0
            root.events = []
        }

        function test_nothingMovesWhenPlaying() {
            const play = part("play")
            const w = play.width, prevX = xOf("previous"), nextX = xOf("next")
            compare(play.Accessible.name, "Play")
            play.clicked()
            compare(music.playing, true)
            compare(play.Accessible.name, "Pause")
            compare(play.width, w)
            compare(xOf("previous"), prevX)
            compare(xOf("next"), nextX)
            compare(play.width, 64)
        }

        function test_centredWhateverTheTitle() {
            const c = part("controls")
            const centre = c.mapToItem(root, 0, 0).x + c.width / 2
            compare(Math.round(centre), root.width / 2)
            music.current = { title: "A very long song title that goes on and on and on and on and on", artists: "x" }
            compare(Math.round(c.mapToItem(root, 0, 0).x + c.width / 2), root.width / 2)
            verify(part("title").truncated)
        }

        function test_shuffleAndRepeat() {
            const s = part("shuffle"), r = part("repeat")
            compare(s.Accessible.name, "Shuffle: off")
            s.clicked()
            compare(music.shuffle, true)
            compare(s.Accessible.name, "Shuffle: on")
            verify(s.on)
            r.clicked()
            compare(r.Accessible.name, "Repeat: all")
            r.clicked()
            compare(r.Accessible.name, "Repeat: this song")
            r.clicked()
            verify(!r.on)
        }

        function test_keyboardOrder() {
            part("play").forceActiveFocus()
            const order = ["next", "repeat", "volume", "open"]
            for (const n of order) { keyClick(Qt.Key_Right); verify(part(n).activeFocus, n) }
            for (const n of ["volume", "repeat", "next", "play", "previous", "shuffle"]) { keyClick(Qt.Key_Left); verify(part(n).activeFocus, n) }
            keyClick(Qt.Key_Left)
            compare(JSON.stringify(root.events), JSON.stringify(["left"]))
            part("play").forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(music.calls[music.calls.length - 1], "toggle")
        }

        function test_narrowWindow() {
            verify(part("volume").visible)
            mini.width = 820
            verify(!part("volume").visible, "Volume goes in narrow windows")
            compare(Math.round(part("controls").mapToItem(mini, 0, 0).x + part("controls").width / 2), 410)
        }

        function test_book() {
            const prevX = xOf("previous"), playX = xOf("play")
            book.front = true
            book.active = true
            compare(xOf("previous"), prevX, "the buttons keep their places")
            compare(xOf("play"), playX)
            compare(part("title").text, "A Test Book")
            compare(part("shuffle").Accessible.name, "15 second(s) back")
            compare(part("repeat").Accessible.name, "30 second(s) ahead")
            verify(!part("shuffle").on)
            part("shuffle").clicked()
            part("repeat").clicked()
            part("previous").clicked()
            part("next").clicked()
            compare(JSON.stringify(book.calls), JSON.stringify(["skip -1", "skip 1", "previous chapter", "next chapter"]))
            compare(music.shuffle, false, "Shuffle is not touched for a book")
            // The times are the chapter's: 100 s into a 24 min 05 s chapter.
            verify(findChild(mini, "controls").children[0].text === "1:40", findChild(mini, "controls").children[0].text)
            verify(!part("volume").visible)
            verify(part("speed").visible)
            compare(part("open").text, "Player")
            part("speed").clicked()
            const popup = findChild(mini, "speedPopup")
            tryVerify(() => popup.opened)
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Return)
            tryVerify(() => !popup.visible)
            compare(book.speed, 1.2)
            verify(/^1[.,]2×$/.test(part("speed").text), part("speed").text)
        }

        function test_volume() {
            part("volume").clicked()
            const popup = findChild(mini, "volumePopup")
            tryVerify(() => popup.opened)
            const slider = findChild(mini, "volumeSlider")
            compare(slider.value, 80)
            keyClick(Qt.Key_Up)
            compare(music.volume, 85)
            keyClick(Qt.Key_Escape)
            tryVerify(() => !popup.visible)
        }
    }
}
