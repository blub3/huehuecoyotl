import QtQuick
import QtQuick.Controls.Basic
import QtTest
import CoyotlUi

// Issues #93 to #96: a library's row in Settings → Matching (counts, progress and Stop, the
// result, why it cannot run) and the "Match again…" dialog (its choices reach the request).
Item {
    id: root
    width: 1000
    height: 800
    property var calls: []

    Column {
        width: 900
        MatchingLibraryRow {
            id: row
            width: 900
            onStartRequested: root.calls = root.calls.concat(["start"])
            onStopRequested: root.calls = root.calls.concat(["stop"])
        }
    }
    RematchDialog {
        id: dialog
        onStartRequested: (library, all, includeLocked) => root.calls = root.calls.concat([[library, all, includeLocked]])
    }

    function movies(run) {
        return { library: "movies", total: 1284, matched: 1263, review: 6, none: 15, locked: 22, episodes: 0,
                 blocked: "", run: run || null }
    }

    TestCase {
        name: "Matching"
        when: windowShown

        function part(item, name) { return findChild(item, name) }
        function init() { root.calls = []; dialog.close() }

        function test_idleRow() {
            row.info = root.movies()
            compare(part(row, "libraryName").text, "Movies")
            const counts = part(row, "countsLine").text
            // Plurals stay untranslated here ("movie(s)"): only the numbers and words are checked.
            for (const t of ["1284 movie", "1263 matched", "6 need", "15 not found", "22 chosen by you"])
                verify(counts.indexOf(t) >= 0, t + " in " + counts)
            verify(part(row, "startButton").visible)
            verify(part(row, "startButton").enabled)
            verify(!part(row, "stopButton").visible)
            row.firstButton.forceActiveFocus()
            keyClick(Qt.Key_Return)
            compare(root.calls, ["start"])
        }

        function test_otherLibrariesCountTheirParts() {
            row.info = { library: "music", total: 2512, matched: 1, review: 0, none: 0, locked: 0, songs: 31402, artists: 1140, blocked: "" }
            verify(/^2512 album.*31402 song.*1140 artist/.test(part(row, "countsLine").text))
            row.info = { library: "shows", total: 96, matched: 1, review: 0, none: 0, locked: 0, episodes: 2861, blocked: "" }
            verify(/^96 show.*2861 episode/.test(part(row, "countsLine").text))
            row.info = { library: "books", total: 312, matched: 1, review: 0, none: 0, locked: 0, authors: 184, blocked: "" }
            verify(/^312 book.*184 author/.test(part(row, "countsLine").text))
        }

        function test_runningAndDone() {
            row.info = root.movies({ state: "running", total: 21, done: 7 })
            verify(!part(row, "countsLine").visible)
            verify(part(row, "progressLine").text.indexOf("Matching… 7 of 21") === 0)
            verify(part(row, "stopButton").visible)
            verify(!part(row, "startButton").visible)
            row.firstButton.forceActiveFocus()
            keyClick(Qt.Key_Return)
            compare(root.calls, ["stop"])
            row.info = root.movies({ state: "done", total: 21, done: 21, newly: 14, changed: 3, review: 2 })
            verify(/^Done: 14 newly matched, 3 changed, 2 need/.test(part(row, "resultLine").text))
            row.info = root.movies({ state: "stopped", total: 21, done: 4, newly: 1, changed: 0, review: 0 })
            verify(part(row, "resultLine").text.indexOf("Stopped:") === 0)
            row.runningName = "music"
            row.info = root.movies({ state: "waiting" })
            compare(part(row, "progressLine").text, "Waiting for music to finish.")
        }

        function test_blocked() {
            const m = root.movies()
            m.blocked = "noKey"
            row.info = m
            verify(!part(row, "startButton").enabled)
            verify(part(row, "blockedLine").text.indexOf("TMDB key") >= 0)
            row.info = { library: "books", total: 3, blocked: "offline" }
            verify(part(row, "blockedLine").text.indexOf("nothing is sent") >= 0)
        }

        function test_dialogChoices() {
            dialog.show(root.movies(), null)
            tryVerify(() => dialog.opened)
            compare(part(dialog.contentItem, "dialogTitle").text, "Match movies again")
            tryVerify(() => part(dialog.contentItem, "openOnly").activeFocus)
            verify(part(dialog.contentItem, "estimateLine").text.indexOf("21 item") === 0)
            // Enter on the default starts with "unmatched and uncertain only", keeping choices.
            keyClick(Qt.Key_Return)
            compare(root.calls, [["movies", false, false]])
            tryVerify(() => !dialog.visible)

            root.calls = []
            dialog.show(root.movies(), null)
            tryVerify(() => part(dialog.contentItem, "openOnly").activeFocus)
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Space)
            verify(dialog.all)
            // Everything but the 22 choices.
            verify(part(dialog.contentItem, "estimateLine").text.indexOf("1262 item") === 0)
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Space)
            verify(!dialog.keepLocked)
            verify(part(dialog.contentItem, "estimateLine").text.indexOf("1284 item") === 0)
            keyClick(Qt.Key_Return)
            compare(root.calls, [["movies", true, true]])
        }

        function test_musicCountsArtists() {
            const m = { library: "music", total: 10, matched: 8, review: 1, none: 1, locked: 2, artists: 6, artistsOpen: 3, artistsLocked: 1 }
            row.info = m
            verify(part(row, "countsLine").text.indexOf("3 artist") >= 0)
            dialog.show(m, null)
            tryVerify(() => dialog.opened)
            verify(part(dialog.contentItem, "musicNote").visible)
            compare(dialog.count, 5, "2 albums and 3 artists without a match")
            dialog.all = true
            compare(dialog.count, 8 + 5, "all but the chosen: 8 albums and 5 artists")
            dialog.close()
        }

        function test_nothingToDo() {
            dialog.show({ library: "books", total: 5, review: 0, none: 0, locked: 0 }, null)
            tryVerify(() => dialog.opened)
            verify(!part(dialog.contentItem, "startButton").enabled)
            verify(part(dialog.contentItem, "estimateLine").text.indexOf("Nothing to do") === 0)
            keyClick(Qt.Key_Escape)
            tryVerify(() => !dialog.visible)
            compare(root.calls, [])
        }
    }
}
