import QtQuick
import QtTest
import CoyotlUi

// Issues #101 to #105: matching music again with a stub core: a locked album asks first, a
// review opens the search, the album search compares tracks before choosing (Enter, then Enter
// again), song results say whether the length agrees, artist results count your albums.
Item {
    id: root
    width: 1000
    height: 800
    property var notices: []
    property var changedId: 0

    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, string kind, string message)
        property var calls: []
        property int next: 0
        property var answers: ({})
        function reply(kind, arg) {
            next += 1
            const id = next
            calls = calls.concat([[kind, arg]])
            const a = stub.answers[kind]
            const detail = typeof a === "function" ? a(arg) : (a || {})
            Qt.callLater(() => stub.musicDetailReady(id, kind, detail))
            return id
        }
        function matchAgain(item, unlock) { return reply("matchAgain", unlock) }
        function matchSearch(item, q) { return reply("matchCandidates", q) }
        function matchPreview(item, c) { return reply("matchPreview", c) }
        function matchChoose(item, c) { return reply("matchChosen", c) }
    }

    MusicMatch {
        id: matcher
        backend: stub
        onNotice: (t) => root.notices = root.notices.concat([t])
        onChanged: (id) => root.changedId = id
    }

    TestCase {
        name: "MusicMatch"
        when: windowShown

        function dialog() { return findChild(matcher, "musicMatchDialog") }
        function part(name) { return findChild(dialog().contentItem, name) }
        function init() { stub.calls = []; root.notices = []; root.changedId = 0 }
        function cleanup() { if (dialog().visible) { dialog().close(); tryVerify(() => !dialog().visible) } }

        function test_lockedAsksFirst() {
            stub.answers = { matchAgain: (unlock) => unlock ? { result: "changed", newItemId: 9, title: "Winter Rooms" } : { result: "locked" } }
            matcher.again(5, "album", "Winter Rooms", null)
            const confirm = findChild(matcher, "lockQuestion")
            tryVerify(() => confirm.visible)
            verify(confirm.text.indexOf("album") >= 0)
            mouseClick(findChild(matcher, "matchAnyway"))
            tryCompare(root, "changedId", 9)
            compare(stub.calls.map(c => c[1]), [false, true])
            verify(root.notices[0].indexOf("Winter Rooms") >= 0)
        }

        function test_reviewOpensTheSearch() {
            stub.answers = { matchAgain: { result: "review" }, matchCandidates: { candidates: [] } }
            matcher.again(5, "artist", "Halden Quartet", null)
            tryVerify(() => dialog().opened)
            compare(part("searchHeading").text, "Find the artist")
            verify(matcher.hint.indexOf("Several") === 0)
        }

        function test_albumComparedThenChosen() {
            stub.answers = {
                matchCandidates: { candidates: [
                    { id: "r1", kind: "release", title: "Winter Rooms", artist: "Halden Quartet", tracks: 9, localTracks: 9, good: true, country: "DE", formats: ["Digital Media"] },
                    { id: "r2", kind: "release", title: "Winter Rooms", artist: "Halden Quartet", tracks: 8, localTracks: 9, good: false, formats: ["CD"] }] },
                matchPreview: { tracks: [{ position: 1, title: "Northern Lights", length: "6:12", theirs: "Northern Lights", theirLength: "6:12", ok: true }], extra: 0 },
                matchChosen: { newItemId: 5 }
            }
            matcher.search(5, "album", "Winter Rooms", "Halden Quartet", null)
            tryVerify(() => matcher.candidates.length === 2)
            compare(stub.calls[0][1].artist, "Halden Quartet")
            compare(part("searchHeading").text, "Find the album")
            const results = part("results")
            results.forceActiveFocus()
            results.currentIndex = 0
            tryVerify(() => results.currentItem && results.currentItem.activeFocus)
            verify(findChild(results.currentItem, "fit0").text.indexOf("9 of 9 tracks") >= 0)
            keyClick(Qt.Key_Return)
            tryVerify(() => matcher.preview !== null)
            compare(stub.calls[stub.calls.length - 1][0], "matchPreview")
            verify(part("effect").visible)
            keyClick(Qt.Key_Return)
            tryCompare(root, "changedId", 5)
            const choice = stub.calls[stub.calls.length - 1]
            compare(choice[0], "matchChosen")
            compare(choice[1].id, "r1")
        }

        function test_songAndArtistFits() {
            stub.answers = { matchCandidates: { candidates: [
                { id: "c1", kind: "recording", title: "Long Night", fit: "lengthAgrees:6:41 → 6:41", good: true },
                { id: "c2", kind: "recording", title: "Long Night (live)", fit: "lengthDiffers:1:17", good: false }] } }
            matcher.search(7, "track", "Long Night", "Halden Quartet", null)
            tryVerify(() => matcher.candidates.length === 2)
            compare(matcher.fitText(matcher.candidates[0]), "Length agrees: 6:41 → 6:41")
            compare(matcher.fitText(matcher.candidates[1]), "Length differs by 1:17")
            dialog().close()
            tryVerify(() => !dialog().visible)
            stub.answers = { matchCandidates: { candidates: [{ id: "a1", kind: "artist", title: "Halden Quartet", albums: 4, ofAlbums: 4, good: true }] } }
            matcher.search(8, "artist", "Halden Quartet", "", null)
            tryVerify(() => matcher.candidates.length === 1)
            verify(!("artist" in stub.calls[stub.calls.length - 1][1]), "an artist search has no artist field")
            compare(matcher.fitText(matcher.candidates[0]), "4 of your 4 albums are theirs")
        }
    }
}
