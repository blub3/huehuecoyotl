import QtQuick
import QtTest
import CoyotlUi

// Issue #149: Merge with… with a stub core: likely artists first, the preview proposing the
// matched entry, choosing the other entry, merging, and the refusal for different IDs.
Item {
    id: root
    width: 1000
    height: 800

    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, int kind, string message)
        property int next: 0
        property var merges: []
        readonly property var artists: ({
            1: { id: 1, name: "Beyonce Knowles", albums: 1, songs: 10, matched: false, why: "", blocked: false },
            2: { id: 2, name: "Beyoncé", albums: 3, songs: 40, matched: true, why: "similar name", blocked: false },
            3: { id: 3, name: "Destiny's Child", albums: 2, songs: 20, matched: true, why: "", blocked: true }
        })
        function reply(kind, detail) { const id = ++next; Qt.callLater(() => stub.musicDetailReady(id, kind, detail)); return id }
        function artistMergeCandidates(artist, q) {
            return reply("mergeCandidates", { likely: [artists[2]], others: q === "" ? [] : [artists[3]] })
        }
        function artistMergePreview(keep, drop) {
            const k = artists[keep], d = artists[drop]
            const blocked = keep === 3 || drop === 3
            return reply("mergePreview", { keep: k, drop: d, albums: k.albums + d.albums, songs: k.songs + d.songs,
                                           playlistEntries: 4, plays: 120, refusal: blocked ? "different MusicBrainz IDs" : null })
        }
        function artistMerge(keep, drop) {
            merges = merges.concat([[keep, drop]])
            return reply("artistMerged", { keep: keep, drop: drop, keepName: artists[keep].name, dropName: artists[drop].name })
        }
    }
    property var mergedInfo: null
    ArtistMergeDialog {
        id: dialog
        backend: stub
        onMerged: (info) => root.mergedInfo = info
    }

    TestCase {
        name: "ArtistMergeDialog"
        when: windowShown

        function part(n) { return findChild(dialog.contentItem, n) }
        function init() { stub.merges = []; root.mergedInfo = null }

        function test_mergeTheLikelyOne() {
            dialog.show(1, "Beyonce Knowles", null)
            tryVerify(() => dialog.opened && dialog.likely.length === 1)
            tryVerify(() => part("likely0") !== null)
            part("likely0").clicked()
            // The matched entry with more albums is proposed.
            tryVerify(() => dialog.preview !== null)
            compare(dialog.keep, 2)
            verify(part("mergeSummary").text.indexOf("4 album") > 0, part("mergeSummary").text)
            part("mergeButton").clicked()
            tryVerify(() => root.mergedInfo !== null)
            compare(JSON.stringify(stub.merges), JSON.stringify([[2, 1]]))
            tryVerify(() => !dialog.visible)
        }

        function test_keepTheOtherEntry() {
            dialog.show(1, "Beyonce Knowles", null)
            tryVerify(() => part("likely0") !== null)
            part("likely0").clicked()
            tryVerify(() => dialog.preview !== null && dialog.keep === 2)
            dialog.choose(1)
            tryVerify(() => dialog.preview !== null && dialog.preview.keep.id === 1)
            part("mergeButton").clicked()
            tryVerify(() => root.mergedInfo !== null)
            compare(JSON.stringify(stub.merges), JSON.stringify([[1, 2]]))
        }

        function test_differentIdsAreRefused() {
            dialog.show(2, "Beyoncé", null)
            tryVerify(() => dialog.opened)
            dialog.load("destiny")
            tryVerify(() => dialog.others.length === 1)
            dialog.pick(dialog.others[0])
            tryVerify(() => dialog.preview !== null)
            verify(part("mergeRefusal").visible)
            verify(!part("mergeButton").enabled)
            dialog.close()
        }
    }
}
