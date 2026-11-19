import QtQuick
import QtTest
import CoyotlUi

// Issue #120: adding several folders of a share: the quick look and guess per folder, ticking
// and changing the type, folders already in the library, opening a folder and going up, and
// adding the picked folders in one call.
Item {
    id: root
    width: 1200
    height: 700

    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, string kind, string message)
        property var browsed: []
        property var addedWith: null
        property int next: 0
        function shareBrowse(target, password, path) {
            browsed = browsed.concat([path])
            const id = ++next
            const top = { share: 3, path: "/", parent: "", folders: [
                { name: "Filme", path: "/Filme", guess: "video", looked: true, videos: 12, audio: 0, books: 0, folders: 4, inLibrary: [], inside: null },
                { name: "Musik", path: "/Musik", guess: "music", looked: true, videos: 0, audio: 300, books: 0, folders: 20, inLibrary: ["music"], inside: null },
                { name: "Stuff", path: "/Stuff", guess: null, looked: true, videos: 0, audio: 0, books: 0, folders: 0, inLibrary: [], inside: null }] }
            const sub = { share: 3, path: "/Filme", parent: "/", folders: [
                { name: "Kids", path: "/Filme/Kids", guess: "video", looked: true, videos: 3, audio: 0, books: 0, folders: 0, inLibrary: [], inside: null }] }
            Qt.callLater(() => stub.musicDetailReady(id, "shareFolders", path === "/Filme" ? sub : top))
            return id
        }
        function shareAddFolders(share, folders) {
            addedWith = { share: share, folders: folders }
            const id = ++next
            Qt.callLater(() => stub.musicDetailReady(id, "shares", {}))
            return id
        }
    }

    property int addedCount: -1
    ShareFolderBrowser {
        id: browser
        anchors.fill: parent
        backend: stub
        target: ({ share: 3 })
        shareName: "media on nas.local"
        onAdded: (n) => root.addedCount = n
    }

    TestCase {
        name: "ShareFolderBrowser"
        when: windowShown

        function part(name) { return findChild(browser, name) }
        function init() {
            browser.chosen = []
            root.addedCount = -1
            if (browser.path !== "/") { browser.browse("/"); tryCompare(browser, "path", "/") }
        }
        function rowsReady(n) { tryVerify(() => browser.folders.length === n && part("tick0") !== null) }

        function test_pickTwoAndAdd() {
            rowsReady(3)
            verify(part("hint0").text.indexOf("12 video") >= 0, part("hint0").text)
            verify(part("hint1").text.indexOf("in your library") >= 0, part("hint1").text)
            part("tick1").clicked()
            compare(browser.chosen.length, 0, "a folder in the library can't be picked again")
            compare(part("chip2").text, "Movies & shows?", "no guess shows a question mark")

            part("tick0").clicked()
            compare(browser.chosen.length, 1)
            compare(browser.chosen[0].content, "video")
            // The chip of an unpicked folder picks it with the next type.
            part("chip2").clicked()
            compare(browser.chosen[1].content, "music")
            part("chip2").clicked()
            compare(browser.chosen[1].content, "audiobooks")
            verify(part("addChosen").text.indexOf("Add 2 folder") === 0, part("addChosen").text)

            part("addChosen").clicked()
            tryCompare(root, "addedCount", 2)
            compare(stub.addedWith.share, 3)
            compare(JSON.stringify(stub.addedWith.folders.map(f => f.path + ":" + f.content)),
                    JSON.stringify(["/Filme:video", "/Stuff:audiobooks"]))
        }

        function test_openAndUpKeepsPicks() {
            rowsReady(3)
            part("tick0").clicked()
            part("open0").clicked()
            tryCompare(browser, "path", "/Filme")
            rowsReady(1)
            verify(part("up").enabled)
            part("tick0").clicked()
            compare(browser.chosen.length, 2, "picks survive moving through folders")
            part("up").clicked()
            tryCompare(browser, "path", "/")
            browser.unchoose(0)
            compare(browser.chosen.length, 1)
            compare(browser.chosen[0].path, "/Filme/Kids")
        }

        function test_keyboard() {
            rowsReady(3)
            browser.focusFirst()
            tryVerify(() => part("tick0").activeFocus)
            keyClick(Qt.Key_Space)
            compare(browser.chosen.length, 1)
            keyClick(Qt.Key_Right)
            verify(part("chip0").activeFocus)
            keyClick(Qt.Key_Return)
            compare(browser.chosen[0].content, "music")
            keyClick(Qt.Key_Right)
            verify(part("open0").activeFocus)
            keyClick(Qt.Key_Right)
            verify(part("unchoose0").activeFocus, "Right goes on to the list of what will be added")
        }
    }
}
