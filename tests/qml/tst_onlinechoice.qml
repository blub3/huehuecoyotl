import QtQuick
import QtTest
import CoyotlUi

// Issue #165: the choice appears only when due, names the services, and stores the answer.
Item {
    id: root
    width: 900
    height: 700
    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal libraryChanged()
        signal scanFinished()
        property bool due: false
        property var musicOnline: undefined
        property int next: 0
        function onlineChoice() {
            const id = ++next
            const d = due
            Qt.callLater(() => stub.musicDetailReady(id, "onlineChoice", { due: d }))
            return id
        }
    }
    OnlineChoiceDialog { id: dialog; backend: stub }
    TestCase {
        name: "OnlineChoice"
        when: windowShown
        function init() { stub.due = false; stub.musicOnline = undefined; dialog.close(); tryCompare(dialog, "visible", false) }
        function test_notDue() {
            stub.libraryChanged()
            wait(50)
            compare(dialog.opened, false)
        }
        function test_namesServices() {
            stub.due = true
            stub.libraryChanged()
            tryCompare(dialog, "opened", true)
            const text = findChild(dialog.contentItem, "dataFlows").text
            for (const s of ["MusicBrainz", "Cover Art Archive", "Open Library"]) verify(text.indexOf(s) >= 0, s)
            compare(stub.musicOnline, undefined)
        }
        function test_filesOnly() {
            stub.due = true
            stub.scanFinished()
            tryCompare(dialog, "opened", true)
            findChild(dialog.contentItem, "filesOnly").clicked()
            compare(stub.musicOnline, false)
            tryCompare(dialog, "visible", false)
        }
        function test_online() {
            stub.due = true
            stub.libraryChanged()
            tryCompare(dialog, "opened", true)
            findChild(dialog.contentItem, "useOnline").clicked()
            compare(stub.musicOnline, true)
        }
    }
}
