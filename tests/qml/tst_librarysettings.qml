import QtQuick
import QtTest
import CoyotlUi

// Issue #150: Settings → Library with a stub core: this computer or a share, the checks and
// refusals, move or start empty, the type's import mode and the name preview; the origin note.
Item {
    id: root
    width: 1000
    height: 900
    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, int kind, string message)
        property int next: 0
        property var calls: []
        property var status: ({ examples: { movies: "Movies/A (2000)/A (2000).mkv", music: "Music/B/C (2001)/01 - D.flac" }, configured: true, where: "local", path: "/lib", copies: 3, size: 1000, free: 5000, moving: false,
            shares: [{ id: 1, name: "media on nas", state: "online" }, { id: 2, name: "films", state: "offline" }],
            types: { movies: { enabled: true, "import": "manual", writeTags: true }, shows: { enabled: true, "import": "manual", writeTags: true },
                     music: { enabled: true, "import": "manual", writeTags: true }, audiobooks: { enabled: true, "import": "manual", writeTags: true } } })
        property var checkAnswer: ({})
        function libraryCall(action, args) {
            const id = ++next
            calls = calls.concat([{ action: action, args: args }])
            let kind = "library", d = status
            if (action === "check") { kind = "libraryCheck"; d = checkAnswer }
            if (action === "itemOrigin") { kind = "libraryOrigin"; d = { libraryOnly: true, from: "NAS", goneAt: 1000 } }
            Qt.callLater(() => stub.musicDetailReady(id, kind, d))
            return id
        }
        function shareBrowse(target, password, path) {
            const id = ++next
            Qt.callLater(() => stub.musicDetailReady(id, "shareFolders", { path: "/", parent: "", folders: [] }))
            return id
        }
    }
    LibrarySettings { id: page; width: 900; backend: stub }
    LibraryOnlyNote { id: note; y: 860; width: 400; backend: stub; itemId: 5 }
    function ok(extra) {
        return Object.assign({ ok: true, path: "/new", copies: 3, needed: 1000, free: 9000, current: "/lib", checks: [
            { key: "reachable", ok: true, detail: "" }, { key: "writable", ok: true, detail: "" },
            { key: "space", ok: true, detail: "" }, { key: "alone", ok: true, detail: "" }] }, extra || {})
    }
    TestCase {
        name: "LibrarySettings"
        when: windowShown
        function dialog() { return findChild(page, "placeDialog") }
        function init() {
            stub.calls = []
            tryVerify(() => page.status.configured === true)
        }
        function cleanup() { if (dialog().opened) { dialog().close(); tryCompare(dialog(), "visible", false) } }
        function lastCall() { return stub.calls[stub.calls.length - 1] }
        function test_showsTheLibrary() {
            verify(findChild(page, "libraryNow").text.indexOf("/lib") >= 0)
        }
        function test_moveByDefault() {
            stub.checkAnswer = root.ok()
            page.checkLocal("/new")
            tryCompare(dialog(), "opened", true)
            compare(lastCall().action, "check")
            findChild(dialog().contentItem, "confirmPlace").clicked()
            tryVerify(() => lastCall().action === "apply")
            compare(lastCall().args.mode, "move")
            compare(lastCall().args.path, "/new")
            tryCompare(dialog(), "visible", false)
        }
        function test_startEmpty() {
            stub.checkAnswer = root.ok()
            page.checkLocal("/new")
            tryCompare(dialog(), "opened", true)
            findChild(dialog().contentItem, "modeEmpty").clicked()
            findChild(dialog().contentItem, "confirmPlace").clicked()
            tryVerify(() => lastCall().action === "apply")
            compare(lastCall().args.mode, "empty")
        }
        function test_refusedPlaceCannotBeConfirmed() {
            stub.checkAnswer = root.ok({ ok: false, checks: [{ key: "reachable", ok: true, detail: "" }, { key: "writable", ok: false, detail: "readOnly" }] })
            page.run({ where: "share", share: 1, path: "/x" })
            tryCompare(dialog(), "opened", true)
            compare(findChild(dialog().contentItem, "confirmPlace").enabled, false)
        }
        function test_typeModeAndPreview() {
            findChild(page, "kind_movies").clicked()
            verify(findChild(page, "namePreview").text.indexOf("Movies/") === 0)
            findChild(page, "importAuto").clicked()
            compare(lastCall().action, "setType")
            compare(lastCall().args.type, "movies")
            compare(lastCall().args.changes["import"], "auto")
            findChild(page, "kind_music").clicked()
            verify(findChild(page, "namePreview").text.indexOf("Music/") === 0)
        }
        function test_offlineShareCannotBeChosen() {
            findChild(page, "whereShare").clicked()
            tryVerify(() => findChild(page, "share1") !== null)
            compare(findChild(page, "share0").enabled, true)
            compare(findChild(page, "share1").enabled, false)
            findChild(page, "whereLocal").clicked()
        }
        function test_originNote() {
            tryCompare(note, "visible", true)
            verify(findChild(note, "originNote").text.indexOf("NAS") >= 0)
        }
    }
}
