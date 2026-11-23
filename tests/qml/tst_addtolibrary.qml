import QtQuick
import QtTest
import CoyotlUi

// Issue #154: the sheet and Imports with a stub core.
Item {
    id: root
    width: 1000
    height: 800
    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, int kind, string message)
        property int next: 0
        property var calls: []
        property var answer: ({})
        property var list: ({ waiting: 1, active: 1, jobs: [
            { id: 1, title: "El mal querer", state: "review", review: "match", note: "", files: 11, filesDone: 0, size: 100, sizeDone: 0 },
            { id: 2, title: "MOTOMAMI", state: "queued", review: null, note: "", files: 16, filesDone: 8, size: 100, sizeDone: 50 },
            { id: 3, title: "Los Ángeles", state: "done", review: null, note: "", files: 12, filesDone: 12, size: 100, sizeDone: 100 },
            { id: 4, title: "Again", state: "done", review: null, note: "already", files: 0, filesDone: 0, size: 0, sizeDone: 0 },
            { id: 5, title: "Alien", state: "failed", review: null, note: "the NAS went offline", files: 1, filesDone: 0, size: 100, sizeDone: 0 }] })
        function libraryCall(action, args) {
            const id = ++next
            calls = calls.concat([{ action: action, args: args }])
            const kind = action === "importPlan" ? "importSheet" : "imports"
            const d = action === "importPlan" ? answer : list
            Qt.callLater(() => stub.musicDetailReady(id, kind, d))
            return id
        }
    }
    AddToLibrarySheet { id: sheet; backend: stub }
    ImportsPanel { id: panel; width: 800; height: 600; backend: stub; visible: false }
    property var reviewed: null
    Connections { target: panel; function onReviewRequested(j) { root.reviewed = j } }
    function plan(extra) {
        return Object.assign({ configured: true, files: 2, size: 2000000, already: 0, review: 0, from: "smb://nas.local/music/x",
            tags: [{ field: "artist", now: "Rosalia", then: "Rosalía", changed: true }],
            plans: [{ item: 5, kind: "music", title: "MOTOMAMI", subtitle: "Rosalía", to: "/lib/Music/Rosalía/MOTOMAMI (2022)", names: ["01 - SAOKO.flac", "02 - CANDY.flac"], files: 2 }] }, extra || {})
    }
    TestCase {
        name: "AddToLibrary"
        when: windowShown
        function cleanup() { if (sheet.opened) { sheet.close(); tryCompare(sheet, "visible", false) } stub.calls = [] }
        function c(n) { return findChild(sheet.contentItem, n) }
        function test_showsWhatWillHappenThenStarts() {
            stub.answer = root.plan()
            sheet.show(5, null)
            tryVerify(() => sheet.plan !== null)
            verify(c("sheetTitle").text.indexOf("MOTOMAMI") >= 0)
            verify(c("sheetFrom").text.indexOf("nas.local") >= 0)
            verify(c("sheetTo").text.indexOf("/lib/Music") === 0)
            compare(stub.calls.filter(x => x.action === "importStart").length, 0, "nothing starts by itself")
            c("sheetAdd").clicked()
            tryVerify(() => stub.calls.some(x => x.action === "importStart" && x.args.item === 5))
            tryCompare(sheet, "visible", false)
        }
        function test_alreadyThere() {
            stub.answer = root.plan({ files: 0, already: 1, tags: [], plans: [{ item: 5, kind: "music", title: "X", subtitle: "", to: "", names: [], files: 0 }] })
            sheet.show(5, null)
            tryVerify(() => sheet.plan !== null)
            compare(c("sheetAlready").visible, true)
            compare(c("sheetAdd").visible, false)
        }
        function test_noLibraryYet() {
            stub.answer = root.plan({ configured: false })
            sheet.show(5, null)
            tryVerify(() => sheet.plan !== null)
            compare(c("sheetNotConfigured").visible, true)
            compare(c("sheetAdd").visible, false)
        }
        function test_importsGroupsAndActions() {
            panel.visible = true
            panel.load()
            tryCompare(panel.review, "length", 1)
            compare(panel.now.length, 1)
            compare(panel.done.length, 3)
            verify(panel.whyText(panel.review[0]).length > 0)
            verify(panel.resultText(panel.done[1]).indexOf("Already") === 0)
            verify(panel.resultText(panel.done[2]).indexOf("offline") > 0)
            panel.reviewRequested(panel.review[0])
            compare(root.reviewed.id, 1)
            panel.act("importPause", panel.now[0])
            tryVerify(() => stub.calls.some(x => x.action === "importPause" && x.args.job === 2))
            panel.visible = false
        }
    }
}
