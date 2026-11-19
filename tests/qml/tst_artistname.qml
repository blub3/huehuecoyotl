import QtQuick
import QtTest
import CoyotlUi

// Issue #148: the artist's name dialog with a stub core: the current choice preselected, the
// three choices reaching the core, an empty own name refused, and saving closing it.
Item {
    id: root
    width: 900
    height: 700

    QtObject {
        id: stub
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, int kind, string message)
        property int next: 0
        property var last: null
        function setArtistName(artist, choice, own) {
            const id = ++next
            last = { artist: artist, choice: choice, own: own }
            Qt.callLater(() => stub.musicDetailReady(id, "artist", { id: artist, title: own || "Rosalía" }))
            return id
        }
    }
    property var savedDetail: null
    ArtistNameDialog {
        id: dialog
        backend: stub
        onSaved: (d) => root.savedDetail = d
    }

    TestCase {
        name: "ArtistNameDialog"
        when: windowShown

        function part(n) { return findChild(dialog.contentItem, n) }
        function init() { stub.last = null; root.savedDetail = null }

        function test_defaultsAndSave() {
            dialog.show(7, { shown: "Rosalía", source: "musicbrainz", files: "Rosalia", musicbrainz: "Rosalía", choice: "auto", own: "" }, null)
            tryVerify(() => dialog.opened)
            compare(dialog.choice, "auto")
            verify(part("mbChoice").visible)
            part("filesChoice").toggled()
            part("filesChoice").checked = true
            dialog.choice = "files"
            part("saveName").clicked()
            tryVerify(() => root.savedDetail !== null)
            compare(stub.last.choice, "files")
            compare(stub.last.artist, 7)
            tryVerify(() => !dialog.visible)
        }

        function test_ownName() {
            dialog.show(7, { shown: "Rosalía", files: "Rosalia", musicbrainz: "Rosalía", choice: "auto" }, null)
            tryVerify(() => dialog.opened)
            dialog.choice = "own"
            part("ownField").text = ""
            part("saveName").clicked()
            verify(part("nameError").visible, "an empty name is refused")
            compare(stub.last, null)
            part("ownField").text = "ROSALÍA"
            part("saveName").clicked()
            tryVerify(() => root.savedDetail !== null)
            compare(stub.last.own, "ROSALÍA")
        }

        function test_unmatchedHasNoMusicBrainzChoice() {
            dialog.show(8, { shown: "Rosana", files: "Rosana", musicbrainz: "", choice: "auto" }, null)
            tryVerify(() => dialog.opened)
            verify(!part("mbChoice").visible)
            compare(dialog.choice, "files")
            dialog.close()
        }
    }
}
