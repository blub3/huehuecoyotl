import QtQuick
import QtTest
import CoyotlUi

// Issue #157: the editor, the preview and the result with a stub core.
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
        property var resultData: ({})
        function field(v, several, allowed) { return { value: v, several: several, sample: several ? ["Pop", "Flamenco"] : [], allowed: allowed } }
        function libraryCall(action, args) {
            const id = ++next
            calls = calls.concat([{ action: action, args: args }])
            let kind = action, d = {}
            if (action === "tagPlan") d = { item: 2, scopes: { track: null, album: 2, albumTitle: "Lux", albumFiles: 4, artist: 1, artistName: "Rosalía", artistAlbums: 3 } }
            if (action === "tagValues") {
                kind = "tagValues"
                d = { scope: args.scope, id: args.id, files: 4, unreadable: 0, canonical: { album: "LUX", albumArtist: "Rosalía" },
                      fields: { albumArtist: field("Rosalia", false, true), artist: field("Rosalia", false, true), album: field(args.scope === "album" ? "Lux" : "", false, args.scope === "album"),
                                year: field("2025", false, args.scope === "album"), genre: field("", true, true), disc: field("1", false, args.scope === "album"), title: field("", true, false) } }
            }
            if (action === "tagPreview") d = { files: 4, willChange: 3, same: 0, readOnly: 1, readOnlyNames: ["archive"], offline: 0, offlineNames: [],
                rows: [{ file: "a/01.flac", field: "album", before: "Lux", after: "LUX" }] }
            if (action === "tagWrite") { kind = "tagResult"; d = stub.resultData }
            const k = kind
            Qt.callLater(() => stub.musicDetailReady(id, k, d))
            return id
        }
    }
    TagEditor { id: editor; backend: stub }
    TestCase {
        name: "TagEditor"
        when: windowShown
        function c(n) { return findChild(editor.contentItem, n) }
        function begin() {
            stub.calls = []
            editor.show(2, null)
            tryVerify(() => editor.opened && editor.values !== null)
        }
        function cleanup() { if (editor.visible) { editor.close(); tryCompare(editor, "visible", false) } }
        function last() { return stub.calls[stub.calls.length - 1] }
        function test_onlyTickedFieldsChange() {
            begin()
            compare(editor.scope, "album")
            compare(c("tagToPreview").enabled, false, "nothing ticked")
            compare(c("field_title").visible, false, "an album has no one title")
            verify(c("field_genre").placeholderText.indexOf("Several values") === 0)
            editor.setText("album", "LUX")
            compare(editor.ticked.album, true)
            c("tagToPreview").clicked()
            tryCompare(editor, "step", "preview")
            const e = stub.calls.filter(x => x.action === "tagPreview")[0].args
            compare(Object.keys(e.changes).length, 1)
            compare(e.changes.album, "LUX")
            verify(c("previewSkips").text.indexOf("archive") > 0)
            compare(stub.calls.filter(x => x.action === "tagWrite").length, 0, "the preview writes nothing")
        }
        function test_fillFromMusicBrainz() {
            begin()
            c("fillMb").clicked()
            compare(editor.texts.album, "LUX")
            compare(editor.ticked.albumArtist, true)
            compare(c("writeIds").checked, true)
        }
        function test_resultGroupsAndRetry() {
            begin()
            stub.resultData = { updated: 2, same: 0, attempted: 3, flatpak: false, offline: [],
                failed: [{ fileId: 9, file: "a/03.flac", reason: "changed" }], readOnly: [{ file: "b/1.flac", source: "archive", sourceId: 4, local: false }] }
            editor.setText("album", "LUX")
            c("tagToPreview").clicked()
            tryCompare(editor, "step", "preview")
            c("tagWrite").clicked()
            tryCompare(editor, "step", "result")
            verify(c("tagHeading").text.indexOf("2 of 4") === 0)
            compare(c("resultFailed").visible, true)
            compare(c("resultReadOnly").visible, true)
            compare(c("resultOffline").visible, false)
            compare(c("grantWrite").visible, false)
            c("tagRetry").clicked()
            tryVerify(() => last().action === "tagWrite" && last().args.only !== undefined && last().args.only[0] === 9)
        }
        function test_artistScope() {
            begin()
            c("scopeArtist").clicked()
            tryVerify(() => editor.values !== null && editor.values.scope === "artist")
            compare(c("field_album").visible, false)
            compare(c("field_artist").visible, true)
        }
    }
}
