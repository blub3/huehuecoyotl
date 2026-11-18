import QtQuick
import QtTest
import CoyotlUi

// Issues #108 to #110: matching audiobooks with a stub core: the book search sends title,
// author and ISBN, Enter shows the field table, Enter again chooses; authors show how many of
// your books are theirs; with online information off nothing is sent.
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
        property bool musicOnline: true
        property var calls: []
        property int next: 0
        property var answers: ({})
        function reply(kind, arg) {
            next += 1
            const id = next
            calls = calls.concat([[kind, arg]])
            Qt.callLater(() => stub.musicDetailReady(id, kind, stub.answers[kind] || {}))
            return id
        }
        function matchAgain(item, unlock) { return reply("matchAgain", unlock) }
        function matchSearch(item, q) { return reply("matchCandidates", q) }
        function matchPreview(item, c) { return reply("matchPreview", c) }
        function matchChoose(item, c) { return reply("matchChosen", c) }
    }

    BookMatch {
        id: matcher
        backend: stub
        onNotice: (t) => root.notices = root.notices.concat([t])
        onChanged: (id) => root.changedId = id
    }

    TestCase {
        name: "BookMatch"
        when: windowShown

        function dialog() { return findChild(matcher, "bookMatchDialog") }
        function part(name) { return findChild(dialog().contentItem, name) }
        function init() { stub.calls = []; root.notices = []; root.changedId = 0; stub.musicOnline = true }
        function cleanup() { if (dialog().visible) { dialog().close(); tryVerify(() => !dialog().visible) } }

        function test_bookFieldsThenChoose() {
            stub.answers = {
                matchCandidates: { candidates: [{ id: "OL9W", kind: "work", title: "The Salt Archive", artist: "Liv Ostrander", year: 2018, good: true }] },
                matchPreview: { fields: [{ field: "description", yours: "Mine", theirs: "Theirs", shown: "yours" },
                                         { field: "year", yours: "", theirs: "2018", shown: "openLibrary" }] },
                matchChosen: { newItemId: 3 }
            }
            matcher.search(3, "audiobook", "The Salt Archive", "Liv Ostrander", null)
            tryVerify(() => matcher.candidates.length === 1)
            compare(part("searchHeading").text, "Find the book")
            compare(stub.calls[0][1].author, "Liv Ostrander")
            compare(stub.calls[0][1].isbn, "")
            const results = part("results")
            results.forceActiveFocus()
            tryVerify(() => results.currentItem && results.currentItem.activeFocus)
            verify(findChild(results.currentItem, "fit0").text.indexOf("Title and author agree") >= 0)
            keyClick(Qt.Key_Return)
            tryVerify(() => matcher.preview !== null)
            verify(findChild(results.currentItem, "fieldTable").visible)
            keyClick(Qt.Key_Return)
            tryCompare(root, "changedId", 3)
            compare(stub.calls[stub.calls.length - 1][1].id, "OL9W")
        }

        function test_authorFit() {
            stub.answers = { matchCandidates: { candidates: [{ id: "OL1A", kind: "author", title: "Liv Ostrander", birth: "1974", books: 3, ofBooks: 3, good: true }] } }
            matcher.search(4, "person", "Liv Ostrander", "", null)
            tryVerify(() => matcher.candidates.length === 1)
            compare(part("searchHeading").text, "Find the author")
            verify(!part("isbnField").visible)
            compare(matcher.fitText(matcher.candidates[0]), "3 of your 3 books are theirs")
            verify(!("isbn" in stub.calls[0][1]))
        }

        function test_offlineSendsNothing() {
            stub.musicOnline = false
            matcher.again(3, "audiobook", "The Salt Archive", null)
            compare(stub.calls.length, 0)
            verify(root.notices[0].indexOf("Open Library") >= 0)
            matcher.search(3, "audiobook", "The Salt Archive", "", null)
            tryVerify(() => dialog().opened)
            compare(stub.calls.length, 0)
            verify(matcher.message.indexOf("Turn on") === 0)
        }
    }
}
