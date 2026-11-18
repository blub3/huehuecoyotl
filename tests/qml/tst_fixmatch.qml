import QtQuick
import QtTest
import CoyotlUi

// Issues #97 to #100: Fix match with a stub core: the current match, the Movie / TV show switch
// searching the other kind, the confirm step for a plain change, a movie that becomes a show
// (files with numbers, a file without numbers blocking the change) and Match automatically again.
Item {
    id: root
    width: 1000
    height: 800

    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, string kind, string message)
        property var calls: []
        property int next: 0
        property var answers: ({})
        function reply(kind, detail) {
            next += 1
            const id = next
            calls = calls.concat([[kind, detail]])
            Qt.callLater(() => stub.musicDetailReady(id, kind === "search" ? "matchCandidates" : kind === "preview" ? "matchPreview"
                                                          : kind === "choose" ? "matchChosen" : "matchAgain", stub.answers[kind] || {}))
            return id
        }
        function matchSearch(item, q) { return reply("search", q) }
        function matchPreview(item, c) { return reply("preview", c) }
        function matchChoose(item, c) { return reply("choose", c) }
        function matchAgain(item, unlock) { return reply("again", { unlock: unlock }) }
    }

    FixMatchDialog {
        id: dialog
        backend: stub
        onChosen: (id) => root.chosenId = id
    }
    property var chosenId: 0

    function candidates(kind) {
        return {
            current: { kind: "movie", title: "The Long Harbour", year: 1987, state: "matched", locked: false },
            candidates: kind === "show"
                ? [{ provider: "tmdb", id: "77", kind: "show", title: "The Long Harbour", year: 2019, score: 0.96, overview: "A coast-guard station." }]
                : [{ provider: "tmdb", id: "5", kind: "movie", title: "The Long Harbour", year: 1987, score: 0.61, current: true },
                   { provider: "tmdb", id: "6", kind: "movie", title: "Long Harbour", year: 2019, score: 0.58 }]
        }
    }

    TestCase {
        name: "FixMatch"
        when: windowShown

        function part(name) { return findChild(dialog.contentItem, name) }
        function init() {
            stub.calls = []
            root.chosenId = 0
            stub.answers = { search: root.candidates("movie") }
        }
        function cleanup() { dialog.close(); tryVerify(() => !dialog.visible) }

        function test_searchAndSwitchKind() {
            dialog.openFor(10, "The Long Harbour", 2019, "movie")
            tryVerify(() => dialog.candidates.length === 2)
            compare(stub.calls[0][1].kind, "movie")
            verify(part("currentLine").text.indexOf("The Long Harbour (1987)") >= 0)
            verify(part("currentLine").text.indexOf("chosen automatically") >= 0)
            verify(part("kindHint").text.indexOf("Is it a series?") >= 0)
            // The current match cannot be chosen again.
            dialog.pick(dialog.candidates[0])
            compare(dialog.state_, "search")
            // TV show searches shows.
            stub.answers = { search: root.candidates("show") }
            mouseClick(part("showKind"))
            tryVerify(() => dialog.candidates.length === 1)
            compare(stub.calls[stub.calls.length - 1][1].kind, "show")
            verify(part("kindHint").text.indexOf("single film") >= 0)
        }

        function test_plainChange() {
            stub.answers = { search: root.candidates("movie"), preview: { change: "none", merge: "Long Harbour", missing: [], files: [] },
                             choose: { itemId: 10, newItemId: 12 } }
            dialog.openFor(10, "The Long Harbour", 2019, "movie")
            tryVerify(() => dialog.candidates.length === 2)
            dialog.pick(dialog.candidates[1])
            tryVerify(() => dialog.preview !== null)
            compare(part("confirmHeading").text, "Change the match?")
            verify(part("becomesTitle").text.indexOf("Long Harbour (2019)") === 0)
            verify(part("mergeNote").visible)
            verify(!part("fileList").visible)
            tryVerify(() => part("confirmButton").activeFocus)
            keyClick(Qt.Key_Return)
            tryCompare(root, "chosenId", 12)
            const choose = stub.calls[stub.calls.length - 1]
            compare(choose[0], "choose")
            compare(choose[1].id, "6")
            compare(choose[1].kind, "movie")
        }

        function test_movieBecomesAShow() {
            stub.answers = { search: root.candidates("show"), choose: { newItemId: 30 },
                             preview: { change: "toShow", merge: null, files: [
                                 { fileId: 1, name: "The.Long.Harbour.S01E01.mkv", fixed: true, on: true, season: 1, episode: 1 },
                                 { fileId: 2, name: "The.Long.Harbour.S01E02.mkv", fixed: false, on: true, season: 1, episode: 2 },
                                 { fileId: 3, name: "Bonus.mkv", fixed: false, on: false, season: null, episode: null, needsNumbers: true }] } }
            dialog.openFor(10, "The Long Harbour", 0, "movie")
            dialog.setKind("show")
            tryVerify(() => dialog.candidates.length === 1)
            dialog.pick(dialog.candidates[0])
            tryVerify(() => dialog.files.length === 3)
            compare(part("confirmHeading").text, "Make this a TV show?")
            verify(part("fileList").visible)
            verify(part("confirmButton").enabled)
            // Ticking the file without numbers blocks the change until it has them.
            dialog.files = dialog.files.map((f, i) => i === 2 ? Object.assign({}, f, { on: true }) : f)
            verify(dialog.numbersMissing)
            verify(!part("confirmButton").enabled)
            verify(part("numbersNote").visible)
            dialog.files = dialog.files.map((f, i) => i === 2 ? Object.assign({}, f, { on: false }) : f)
            verify(part("confirmButton").enabled)
            compare(part("confirmButton").text, "Make it a TV show")
            mouseClick(part("confirmButton"))
            tryCompare(root, "chosenId", 30)
            const choose = stub.calls[stub.calls.length - 1][1]
            compare(choose.kind, "show")
            compare(choose.files.length, 2)
            compare(choose.files[1].episode, 2)
        }

        function test_episodeBecomesAMovie() {
            stub.answers = { search: root.candidates("movie"),
                             preview: { change: "toMovie", files: [{ fileId: 7, name: "Night.Shift.2019.mkv", fixed: true, on: true }] } }
            dialog.openFor(20, "Night Shift", 0, "episode")
            tryVerify(() => dialog.candidates.length === 2)
            verify(!part("showKind").visible, "an episode is not matched to a show here")
            verify(!part("matchAgainButton").visible)
            dialog.pick(dialog.candidates[1])
            tryVerify(() => dialog.files.length === 1)
            compare(part("confirmHeading").text, "Make this a movie?")
            compare(part("confirmButton").text, "Make it a movie")
            // Back returns to the results.
            mouseClick(part("backButton"))
            compare(dialog.state_, "search")
        }

        function test_matchAutomaticallyAgain() {
            stub.answers = { search: root.candidates("movie"), again: { result: "changed", newItemId: 44, title: "Long Harbour" } }
            dialog.openFor(10, "The Long Harbour", 2019, "movie")
            tryVerify(() => dialog.candidates.length === 2)
            mouseClick(part("matchAgainButton"))
            tryCompare(root, "chosenId", 44)
            verify(part("noticeLine").text.indexOf("Now: Long Harbour") >= 0)
            compare(stub.calls.find(c => c[0] === "again")[1].unlock, true)
        }
    }
}
