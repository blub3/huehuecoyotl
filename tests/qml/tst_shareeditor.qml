import QtQuick
import QtTest
import CoyotlUi

// Issue #122: editing a share with a stub core: the fields filled from the share, a test that
// names missing folders, saving with and without a new password, changing a folder's type and
// removing a folder after a confirmation, and removing the whole share.
Item {
    id: root
    width: 1100
    height: 900

    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal librariesListed(var requestId)
        signal requestFailed(var requestId, int kind, string message)
        property int next: 0
        property var calls: []
        property bool gone: false
        property var missing: []
        function later(f) { Qt.callLater(f) }
        function overview() {
            return { shares: gone ? [] : [{ id: 3, name: "media on nas.local", protocol: "smb", host: "nas.local", port: null, share: "media", user: "me", tls: false, keyFile: null,
                folders: [{ sourceId: 7, name: "Movies", root: "/Movies", content: "video", files: 12, scanning: false },
                          { sourceId: 8, name: "Music", root: "/Music", content: "music", files: 30, scanning: false }] }], local: [] }
        }
        function shares() { const id = ++next; later(() => stub.musicDetailReady(id, "shares", stub.overview())); return id }
        function shareEdit(share, spec, password, save) {
            const id = ++next
            calls = calls.concat([{ what: "edit", share: share, spec: spec, password: password, save: save }])
            const d = { ok: missing.length === 0, missing: missing, joins: null, saved: save && missing.length === 0, hostKey: "" }
            later(() => stub.musicDetailReady(id, "shareEdit", d))
            return id
        }
        function shareFolderContent(source, content) {
            const id = ++next
            calls = calls.concat([{ what: "content", source: source, content: content }])
            later(() => stub.musicDetailReady(id, "shares", stub.overview()))
            return id
        }
        function removeLibrary(source) {
            const id = ++next
            calls = calls.concat([{ what: "remove", source: source }])
            later(() => stub.librariesListed(id))
            return id
        }
        function shareRemove(share) {
            const id = ++next
            calls = calls.concat([{ what: "removeShare", share: share }])
            gone = true
            later(() => stub.musicDetailReady(id, "shares", stub.overview()))
            return id
        }
    }

    property var events: []
    Loader {
        id: loader
        anchors.fill: parent
        sourceComponent: ShareEditor {
            backend: stub
            shareId: 3
            onClosed: root.events = root.events.concat(["closed"])
            onAddFoldersRequested: (share, name) => root.events = root.events.concat(["add " + share])
        }
    }

    TestCase {
        name: "ShareEditor"
        when: windowShown
        property var ed: null

        function part(name) { return findChild(ed, name) }
        function init() {
            stub.calls = []
            stub.gone = false
            stub.missing = []
            root.events = []
            loader.active = false
            loader.active = true
            ed = loader.item
            tryVerify(() => ed.info !== null && part("type1") !== null)
        }

        function test_fillTestAndSave() {
            compare(part("hostField").text, "nas.local")
            compare(part("shareField").text, "media")
            compare(part("userField").text, "me")
            verify(!part("passwordField").visible, "the password is not shown")

            stub.missing = ["Movies"]
            part("hostField").text = "192.168.1.20"
            part("test").clicked()
            tryVerify(() => part("status").text.indexOf("Movies") >= 0)
            compare(stub.calls[0].spec.host, "192.168.1.20")
            compare(stub.calls[0].password, "", "no new password: the stored one is used")
            compare(stub.calls[0].save, false)

            stub.missing = []
            part("changePassword").clicked()
            verify(part("passwordField").visible)
            part("passwordField").text = "new-secret"
            part("save").clicked()
            tryVerify(() => stub.calls.length === 2 && !ed.busy)
            compare(stub.calls[1].password, "new-secret")
            compare(stub.calls[1].save, true)
            tryVerify(() => !part("passwordField").visible, 5000, "the form is filled again after saving")
        }

        function test_changeTypeAndRemoveFolder() {
            part("type0").clicked()
            verify(part("question0").visible)
            verify(part("question0").text.indexOf("Music") >= 0, part("question0").text)
            part("type0").clicked()
            verify(part("question0").text.indexOf("Audiobooks") >= 0, "pressing again picks the next type")
            part("confirm0").clicked()
            tryVerify(() => stub.calls.length === 1)
            compare(stub.calls[0].what, "content")
            compare(stub.calls[0].source, 7)
            compare(stub.calls[0].content, "audiobooks")

            tryVerify(() => !ed.busy)
            part("remove1").clicked()
            verify(part("question1").visible)
            part("keep1").clicked()
            verify(!part("question1").visible)
            compare(stub.calls.length, 1, "Keep removes nothing")
            part("remove1").clicked()
            part("confirm1").clicked()
            tryVerify(() => stub.calls.length === 2)
            compare(stub.calls[1].what, "remove")
            compare(stub.calls[1].source, 8)
        }

        function test_removeShare() {
            part("addFolders").clicked()
            compare(JSON.stringify(root.events), JSON.stringify(["add 3"]))
            part("removeShare").clicked()
            const q = part("shareQuestion").text
            verify(q.indexOf("2 folder") >= 0 && q.indexOf("42 file") >= 0, q)
            part("confirmRemoveShare").clicked()
            tryVerify(() => root.events.indexOf("closed") >= 0)
            compare(stub.calls[0].what, "removeShare")
            compare(stub.calls[0].share, 3)
        }
    }
}
