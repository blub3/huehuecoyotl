pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #149: Merge with… on an artist page. Step one picks the other artist: likely ones first
// (the same MusicBrainz ID, or a name that contains the other), then a search of all artists.
// Step two shows which entry stays (the matched one, or the one with more albums, is
// preselected) and what the merged artist holds; Merge joins them. Artists with different
// MusicBrainz IDs cannot be merged; the preview says so. `backend` is the core or a test stub:
// artistMergeCandidates(artist, query), artistMergePreview(keep, drop), artistMerge(keep, drop),
// answering with musicDetailReady or requestFailed. Emits `merged` with {keep, drop, keepName,
// dropName}.
Popup {
    id: dialog
    property var backend: null
    property var artistId: 0
    property string artistName: ""
    property Item from: null
    signal merged(var info)

    property string step: "pick"
    property var likely: []
    property var others: []
    property var other: null      // the candidate picked
    property var keep: 0
    property var preview: null
    property string error: ""
    property var pending: ({ list: 0, preview: 0, merge: 0 })

    function show(artist, name, item) {
        artistId = artist
        artistName = name
        from = item
        step = "pick"
        likely = []
        others = []
        other = null
        preview = null
        error = ""
        search.text = ""
        open()
        load("")
    }
    function load(q) { if (backend) pending.list = backend.artistMergeCandidates(artistId, q) }
    // The first preview decides which entry is proposed: the matched one, else the one with
    // more albums, else this one.
    property bool proposed: false
    function pick(c) {
        other = c
        keep = artistId
        proposed = false
        step = "preview"
        loadPreview()
    }
    function loadPreview() {
        const drop = keep === artistId ? other.id : artistId
        error = ""
        preview = null
        pending.preview = backend.artistMergePreview(keep, drop)
    }
    function choose(id) {
        if (keep === id) return
        keep = id
        loadPreview()
    }
    function merge() {
        if (!preview || preview.refusal) return
        error = ""
        pending.merge = backend.artistMerge(preview.keep.id, preview.drop.id)
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(640, (parent ? parent.width : 800) - 80)
    height: Math.min(620, (parent ? parent.height : 700) - 80)
    modal: true
    focus: true
    padding: 24
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onOpened: search.forceActiveFocus()
    onClosed: if (from) from.forceActiveFocus()

    Connections {
        target: dialog.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === dialog.pending.list && kind === "mergeCandidates") {
                dialog.likely = detail.likely || []
                dialog.others = detail.others || []
            } else if (requestId === dialog.pending.preview && kind === "mergePreview") {
                const k = detail.keep, d = detail.drop
                if (!dialog.proposed) {
                    dialog.proposed = true
                    if ((d.matched && !k.matched) || (d.matched === k.matched && d.albums > k.albums)) {
                        dialog.choose(d.id)
                        return
                    }
                }
                dialog.preview = detail
                Qt.callLater(() => (detail.refusal ? backButton : mergeButton).forceActiveFocus())
            } else if (requestId === dialog.pending.merge && kind === "artistMerged") {
                dialog.merged(detail)
                dialog.close()
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === dialog.pending.merge || requestId === dialog.pending.preview) dialog.error = message
        }
    }
    Timer { id: debounce; interval: 250; onTriggered: dialog.load(search.text) }

    component CandidateRow: Button {
        id: row
        required property var modelData
        required property int index
        property string list: ""
        objectName: list + index
        width: ListView.view ? ListView.view.width : 0
        height: 52
        focusPolicy: Qt.StrongFocus
        Accessible.name: modelData.name
        Keys.onReturnPressed: clicked()
        Keys.onEnterPressed: clicked()
        onClicked: dialog.pick(modelData)
        background: Rectangle {
            radius: 10
            color: row.activeFocus || row.hovered ? Theme.surfaceHi : "transparent"
            border.width: row.activeFocus ? Theme.focusWidth : 0
            border.color: Theme.focus
        }
        contentItem: ColumnLayout {
            spacing: 2
            Label { text: row.modelData.name; color: row.modelData.blocked ? Theme.textDim : Theme.text; font.bold: true; elide: Text.ElideRight; Layout.fillWidth: true }
            Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: [row.modelData.why === "same MusicBrainz ID" ? qsTr("Same MusicBrainz ID")
                       : row.modelData.why === "similar name" ? qsTr("Similar name") : "",
                       qsTr("%n album(s)", "", Number(row.modelData.albums || 0)),
                       row.modelData.blocked ? qsTr("another MusicBrainz ID") : row.modelData.matched ? qsTr("matched") : qsTr("not matched")]
                      .filter(x => x !== "").join("  ·  ")
            }
        }
    }

    // Two steps shown by visibility (a StackLayout crashed while the preview's choices were
    // being replaced).
    contentItem: Item {
        Keys.onPressed: (e) => { if (e.key === Qt.Key_Back) { dialog.close(); e.accepted = true } }

        // ---- step one: which artist
        ColumnLayout {
            anchors.fill: parent
            visible: dialog.step === "pick"
            spacing: 10
            Label {
                text: qsTr("Merge %1 with…").arg(dialog.artistName)
                color: Theme.text
                font.pixelSize: Theme.fontLarge
                font.bold: true
                Accessible.role: Accessible.Heading
            }
            TextField {
                id: search
                objectName: "mergeSearch"
                Layout.fillWidth: true
                placeholderText: qsTr("Search your artists")
                Accessible.name: qsTr("Search your artists")
                onTextEdited: debounce.restart()
                Keys.onDownPressed: (likelyList.count > 0 ? likelyList : othersList).forceActiveFocus()
            }
            Label { visible: dialog.likely.length > 0; text: qsTr("Likely the same"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            ListView {
                id: likelyList
                objectName: "likelyList"
                visible: count > 0
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(contentHeight, 3 * 52)
                clip: true
                model: dialog.likely
                keyNavigationEnabled: true
                Keys.onUpPressed: (e) => { if (currentIndex <= 0) search.forceActiveFocus(); else e.accepted = false }
                Keys.onDownPressed: (e) => { if (currentIndex >= count - 1 && othersList.count > 0) othersList.forceActiveFocus(); else e.accepted = false }
                delegate: CandidateRow { list: "likely"; focus: ListView.isCurrentItem }
            }
            Label { visible: dialog.others.length > 0; text: qsTr("Other artists"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            ListView {
                id: othersList
                objectName: "othersList"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: dialog.others
                keyNavigationEnabled: true
                Keys.onUpPressed: (e) => { if (currentIndex <= 0) (likelyList.count > 0 ? likelyList : search).forceActiveFocus(); else e.accepted = false }
                delegate: CandidateRow { list: "other"; focus: ListView.isCurrentItem }
            }
            Label {
                visible: dialog.likely.length === 0 && dialog.others.length === 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: search.text === "" ? qsTr("Type part of the other artist's name.") : qsTr("No artist with that name.")
                color: Theme.textDim
            }
            FocusButton {
                Layout.alignment: Qt.AlignRight
                text: qsTr("Cancel")
                onClicked: dialog.close()
            }
        }

        // ---- step two: the preview
        ColumnLayout {
            anchors.fill: parent
            visible: dialog.step === "preview"
            spacing: 10
            Label {
                text: qsTr("Keep which entry?")
                color: Theme.text
                font.pixelSize: Theme.fontLarge
                font.bold: true
                Accessible.role: Accessible.Heading
            }
            Repeater {
                model: dialog.preview ? [dialog.preview.keep, dialog.preview.drop].sort((a, b) => a.id - b.id) : []
                RadioButton {
                    id: keepChoice
                    required property var modelData
                    required property int index
                    objectName: "keep" + index
                    Layout.fillWidth: true
                    checked: dialog.keep === modelData.id
                    padding: 8
                    text: modelData.name
                    Accessible.description: qsTr("%n album(s)", "", Number(modelData.albums || 0))
                    onToggled: if (checked) dialog.choose(modelData.id)
                    background: Rectangle { radius: 10; color: keepChoice.checked ? Theme.surfaceHi : "transparent"; border.width: keepChoice.activeFocus ? Theme.focusWidth : 0; border.color: Theme.focus }
                    contentItem: ColumnLayout {
                        spacing: 2
                        Label { leftPadding: keepChoice.indicator.width + 10; text: keepChoice.text; color: Theme.text; font.bold: keepChoice.checked }
                        Label {
                            leftPadding: keepChoice.indicator.width + 10
                            text: [qsTr("%n album(s)", "", Number(keepChoice.modelData.albums || 0)), keepChoice.modelData.matched ? qsTr("matched") : qsTr("not matched")].join("  ·  ")
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                        }
                    }
                }
            }
            Label {
                visible: !!dialog.preview && !dialog.preview.refusal
                text: qsTr("After merging")
                color: Theme.text
                font.bold: true
            }
            Label {
                objectName: "mergeSummary"
                visible: !!dialog.preview && !dialog.preview.refusal
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: dialog.preview ? [
                    qsTr("One artist “%1” with %2").arg(dialog.preview.keep.name).arg(qsTr("%n album(s)", "", Number(dialog.preview.albums || 0))),
                    qsTr("Every song stays on its album (%n song(s))", "", Number(dialog.preview.songs || 0)),
                    qsTr("Playlists keep their entries (%n)", "", Number(dialog.preview.playlistEntries || 0)),
                    qsTr("Play counts add up (%n play(s))", "", Number(dialog.preview.plays || 0)),
                    qsTr("Your files are not changed. The merge is remembered, so a rescan does not split them again.")
                ].join("\n") : ""
            }
            Label {
                objectName: "mergeRefusal"
                visible: !!dialog.preview && !!dialog.preview.refusal
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.focus
                text: qsTr("These are two different artists at MusicBrainz, so they cannot be merged. If one of them is matched wrongly, use Match again on it first.")
            }
            Label {
                visible: dialog.error !== ""
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.focus
                text: dialog.error
            }
            Item { Layout.fillHeight: true }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12
                FocusButton {
                    id: backButton
                    text: qsTr("Back")
                    KeyNavigation.right: mergeButton
                    onClicked: { dialog.step = "pick"; search.forceActiveFocus() }
                }
                FocusButton {
                    id: mergeButton
                    objectName: "mergeButton"
                    primary: true
                    text: qsTr("Merge")
                    enabled: !!dialog.preview && !dialog.preview.refusal && dialog.pending.merge === 0
                    KeyNavigation.left: backButton
                    onClicked: dialog.merge()
                }
            }
        }
    }
}
