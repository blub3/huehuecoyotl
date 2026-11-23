import QtQuick
import QtTest
import CoyotlUi

// Issue #155: both review screens with a stub core.
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
        property var answer: ({})
        function libraryCall(action, args) {
            const id = ++next
            calls = calls.concat([{ action: action, args: args }])
            const kind = action === "importReview" ? "importReview" : "imports"
            const d = action === "importReview" ? answer : ({ jobs: [] })
            Qt.callLater(() => stub.musicDetailReady(id, kind, d))
            return id
        }
    }
    property var found: null
    ImportReviewDialog { id: dialog; backend: stub; onFindMatch: (i) => root.found = i }
    function base(extra) {
        return Object.assign({ job: 7, item: 5, type: "music", title: "LUX", artist: "Rosalía", files: 3, size: 1000, format: "FLAC 24/96",
            from: "NAS › Music/LUX", folder: "Rosalía/LUX (2025)", label: "24-96", review: "clash",
            library: { files: 3, format: "FLAC 16/44.1", folder: "Rosalía/LUX (2025)" } }, extra || {})
    }
    TestCase {
        name: "ImportReview"
        when: windowShown
        function c(n) { return findChild(dialog.contentItem, n) }
        function open(info) {
            stub.answer = info
            stub.calls = []
            dialog.show({ id: 7 }, null)
            tryVerify(() => dialog.opened && dialog.info !== null)
        }
        function cleanup() { if (dialog.visible) { dialog.close(); tryCompare(dialog, "visible", false) } }
        function last() { return stub.calls[stub.calls.length - 1] }
        function test_anotherVersionIsTheDefault() {
            open(root.base())
            verify(c("reviewTitle").text.indexOf("already") > 0)
            compare(c("optVersion").visible, true)
            c("reviewConfirm").clicked()
            tryVerify(() => last().action === "importResolve")
            compare(last().args.choice, "version")
            compare(last().args.all, false)
            tryCompare(dialog, "visible", false)
        }
        function test_replaceForTheNextToo() {
            open(root.base())
            c("optReplace").clicked()
            c("sameForNext").toggle()
            c("reviewConfirm").clicked()
            tryVerify(() => last().action === "importResolve")
            compare(last().args.choice, "replace")
            compare(last().args.all, true)
            c("sameForNext").checked = false
        }
        function test_foreignFolderOnlyGetsAnotherName() {
            open(root.base({ library: null }))
            compare(c("optVersion").visible, false)
            compare(c("optReplace").visible, false)
            compare(c("optKeep").visible, false)
            compare(c("optRename").visible, true)
            c("reviewName").text = ""
            compare(c("reviewConfirm").enabled, false)
            c("reviewName").text = "Second"
            c("reviewConfirm").clicked()
            tryVerify(() => last().action === "importResolve")
            compare(last().args.choice, "rename")
            compare(last().args.label, "Second")
        }
        function test_whichRelease() {
            open(root.base({ review: "match", library: null }))
            verify(c("reviewTitle").text.indexOf("Which") === 0)
            compare(c("reviewConfirm").visible, false)
            c("reviewFind").clicked()
            tryCompare(dialog, "visible", false)
            compare(root.found.job, 7)
        }
        function test_asIsAndSkip() {
            open(root.base({ review: "match", library: null }))
            c("reviewAsIs").clicked()
            tryVerify(() => last().action === "importResolve" && last().args.choice === "asIs")
            tryCompare(dialog, "visible", false)
            open(root.base({ review: "match", library: null }))
            c("reviewSkip").clicked()
            tryVerify(() => last().action === "importSkip")
        }
    }
}
