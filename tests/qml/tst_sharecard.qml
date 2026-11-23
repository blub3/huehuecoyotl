import QtQuick
import QtTest
import CoyotlUi

// Issue #119: a share's card on the Sources page: its folders inside, opening and closing with
// the keyboard, moving through the folders, the signals for rescan and remove, the offline state.
Item {
    id: root
    width: 1000
    height: 600
    property var events: []
    property bool open: true

    ShareCard {
        id: card
        width: 900
        open: root.open
        info: ({ id: 3, name: "media on nas.local", protocol: "smb", url: "smb://nas.local/media", user: "me", online: true,
                 folders: [{ sourceId: 7, name: "Movies", root: "/Movies", content: "video", files: 398, scanning: false, online: true },
                           { sourceId: 8, name: "Music", root: "/Music", content: "music", files: 426, scanning: true, online: true }] })
        onToggled: (on) => { root.open = on; root.events = root.events.concat([on ? "open" : "close"]) }
        onRescanRequested: (id) => root.events = root.events.concat(["rescan " + id])
        onRemoveRequested: (f) => root.events = root.events.concat(["remove " + f.sourceId])
        onAddFoldersRequested: root.events = root.events.concat(["add"])
        onUpRequested: root.events = root.events.concat(["up"])
        onDownRequested: root.events = root.events.concat(["down"])
        onLeftEdge: root.events = root.events.concat(["rail"])
    }

    TestCase {
        name: "ShareCard"
        when: windowShown

        function part(name) { return findChild(card, name) }
        function init() { root.events = []; root.open = true }

        function test_headerAndFolders() {
            compare(part("shareName").text, "media on nas.local")
            const state = part("shareState").text
            verify(state.indexOf("online") === 0, state)
            verify(state.indexOf("as me") > 0, state)
            verify(part("folderLine0").text.indexOf("/Movies") === 0)
            verify(part("folderLine1").text.indexOf("scanning") > 0)
            verify(!part("editShare").visible, "editing comes with #122")
        }

        function test_keyboard() {
            card.focusFirst()
            verify(part("toggle").activeFocus)
            keyClick(Qt.Key_Down)
            verify(part("rescanFolder0").activeFocus)
            keyClick(Qt.Key_Down)
            verify(part("rescanFolder1").activeFocus)
            keyClick(Qt.Key_Right)
            verify(part("removeFolder1").activeFocus)
            keyClick(Qt.Key_Return)
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Up)
            keyClick(Qt.Key_Up)
            verify(part("toggle").activeFocus)
            keyClick(Qt.Key_Up)
            compare(root.events, ["remove 8", "down", "up"])
        }

        function test_openClose() {
            card.focusFirst()
            keyClick(Qt.Key_Left)
            compare(root.events, ["close"])
            verify(!part("folderLine0"), "closed: no folders")
            keyClick(Qt.Key_Down)
            compare(root.events, ["close", "down"], "down from a closed card goes on")
            card.focusFirst()
            keyClick(Qt.Key_Left)
            compare(root.events[2], "rail")
            keyClick(Qt.Key_Right)
            compare(root.events[3], "open")
        }

        function test_rescanAll() {
            mouseClick(part("rescanShare"))
            compare(root.events, ["rescan 7", "rescan 8"])
            mouseClick(part("addFolders"))
            compare(root.events[2], "add")
        }

        function test_offline() {
            const i = Object.assign({}, card.info)
            i.online = false
            card.info = i
            verify(part("shareState").text.indexOf("offline") === 0)
        }
    }
}
