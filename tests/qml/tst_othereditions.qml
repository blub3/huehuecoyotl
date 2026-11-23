import QtQuick
import QtTest
import CoyotlUi

// Issue #147: an album's other editions are listed with year and format, and open by click or
// Enter; without any the section is hidden.
Item {
    id: root
    width: 700
    height: 500
    property var openedId: 0
    property int leftUp: 0
    OtherEditions {
        id: list
        width: 600
        onOpened: (id) => root.openedId = id
        onLeaveUp: root.leftUp++
    }
    TestCase {
        name: "OtherEditions"
        when: windowShown
        function init() { root.openedId = 0; root.leftUp = 0; list.editions = [] }
        function rows() {
            return [
                { id: 7, title: "Album", year: 2000, codec: "flac", sampleRate: 44100, bitDepth: 16, tracks: 10, poster: "" },
                { id: 9, title: "Album (Deluxe)", year: 2011, codec: "mp3", sampleRate: 44100, bitDepth: 0, tracks: 14, poster: "" }
            ]
        }
        function test_hiddenWithoutEditions() { compare(list.visible, false) }
        function test_listsTitleYearFormat() {
            list.editions = rows()
            tryCompare(list, "visible", true)
            const row = findChild(list, "edition1")
            verify(row)
            verify(row.Accessible.name.indexOf("Album (Deluxe)") >= 0)
            verify(row.line.indexOf("2011") >= 0)
            verify(row.line.indexOf("MP3") >= 0)
        }
        function test_opensByClickAndEnter() {
            list.editions = rows()
            const row = findChild(list, "edition1")
            mouseClick(row)
            compare(root.openedId, 9)
            root.openedId = 0
            list.focusFirst()
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Return)
            compare(root.openedId, 9)
            keyClick(Qt.Key_Up)
            keyClick(Qt.Key_Up)
            compare(root.leftUp, 1)
        }
    }
}
