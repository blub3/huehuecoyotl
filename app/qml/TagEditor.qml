pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #157: Edit tags… on a song, an album or all of an artist. Three steps: the fields (only
// ticked ones change), a preview of every change, and the result with what failed or was
// skipped and why. It writes into the files and never renames or moves them.
// `backend`: libraryCall("tagPlan", {item}), ("tagValues", {scope, id}), ("tagPreview", edit),
// ("tagWrite", edit), answered with musicDetailReady or requestFailed. `grantRequested(source)`
// asks the owner for write access to a local folder (the portal, under Flatpak).
Popup {
    id: editor
    property var backend: null
    property Item from: null
    property string step: "edit"
    property var scopes: ({})
    property string scope: "album"
    property var values: null
    property var ticked: ({})
    property var texts: ({})
    property var previewData: null
    property var result: null
    property string error: ""
    property bool busy: false
    property var pending: ({ plan: 0, values: 0, preview: 0, write: 0, grant: 0 })
    // The source and its folder, so the picker opens at the right place.
    signal grantRequested(var source, string root)

    readonly property var scopeId: scope === "track" ? scopes.track : scope === "album" ? scopes.album : scopes.artist
    readonly property var fieldKeys: ["albumArtist", "artist", "album", "year", "genre", "disc", "title"]
    readonly property bool anyTicked: fieldKeys.some(k => ticked[k] === true && isAllowed(k))

    function fieldLabel(k) {
        return ({ albumArtist: qsTr("Album artist"), artist: qsTr("Artist"), album: qsTr("Album"), year: qsTr("Year"),
                  genre: qsTr("Genre"), disc: qsTr("Disc"), title: qsTr("Title") })[k]
    }
    function isAllowed(k) { return !!values && !!values.fields[k] && values.fields[k].allowed === true }
    function show(item, source) {
        from = source || null
        step = "edit"
        values = null
        previewData = null
        result = null
        error = ""
        ticked = ({})
        texts = ({})
        writeIds.checked = false
        open()
        pending.plan = backend.libraryCall("tagPlan", { item: Number(item) })
    }
    function pick(s) {
        scope = s
        values = null
        ticked = ({})
        texts = ({})
        pending.values = backend.libraryCall("tagValues", { scope: s, id: Number(scopeId) })
    }
    function setText(k, v) {
        const t = Object.assign({}, texts); t[k] = v; texts = t
        if (!ticked[k]) setTick(k, true)
    }
    function setTick(k, on) { const t = Object.assign({}, ticked); t[k] = on; ticked = t }
    function fill() {
        const c = values ? values.canonical : null
        if (!c) return
        const t = Object.assign({}, texts), on = Object.assign({}, ticked)
        for (const k of fieldKeys)
            if (isAllowed(k) && c[k]) { t[k] = c[k]; on[k] = true }
        texts = t
        ticked = on
        writeIds.checked = true
    }
    function edit() {
        const changes = {}
        for (const k of fieldKeys) if (ticked[k] && isAllowed(k)) changes[k] = texts[k] !== undefined ? texts[k] : values.fields[k].value
        return { scope: scope, id: Number(scopeId), changes: changes, writeIds: writeIds.checked }
    }
    function toPreview() {
        error = ""
        busy = true
        pending.preview = backend.libraryCall("tagPreview", edit())
    }
    function write(only) {
        error = ""
        busy = true
        const e = edit()
        if (only) e.only = only
        pending.write = backend.libraryCall("tagWrite", e)
    }
    // The folder the portal gave: once the core has checked and stored it, write again.
    function grant(source, path) {
        error = ""
        busy = true
        pending.grant = backend.libraryCall("tagGrant", { source: Number(source), path: path })
    }
    function reasonText(r) {
        return r === "changed" ? qsTr("the file changed while it was written, so it was left as it was")
             : r === "gone" ? qsTr("the file is gone") : r
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(900, (parent ? parent.width : 920) - 60)
    height: Math.min(730, (parent ? parent.height : 760) - 30)
    modal: true
    focus: true
    padding: 24
    closePolicy: Popup.CloseOnEscape
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onClosed: if (from) from.forceActiveFocus()

    Connections {
        target: editor.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === editor.pending.plan && kind === "tagPlan") {
                editor.scopes = detail.scopes
                editor.pick(detail.scopes.track ? "track" : detail.scopes.album ? "album" : "artist")
            } else if (requestId === editor.pending.values && kind === "tagValues") {
                editor.values = detail
                Qt.callLater(() => toPreviewButton.forceActiveFocus())
            } else if (requestId === editor.pending.preview && kind === "tagPreview") {
                editor.busy = false
                editor.previewData = detail
                editor.step = "preview"
                Qt.callLater(() => (detail.willChange > 0 ? writeButton : backButton).forceActiveFocus())
            } else if (requestId === editor.pending.grant && kind === "tagGranted") {
                editor.write(null)
            } else if (requestId === editor.pending.write && kind === "tagResult") {
                editor.busy = false
                editor.result = detail
                editor.step = "result"
                Qt.callLater(() => doneButton.forceActiveFocus())
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (Object.values(editor.pending).indexOf(requestId) >= 0) {
                editor.busy = false
                editor.error = message === "wrongFolder" && requestId === editor.pending.grant
                               ? qsTr("That is not the folder of these files. Choose the folder shown in the picker.") : message
            }
        }
    }

    component Group: Rectangle {
        id: grp
        property string title: ""
        property string body: ""
        property color tone: Theme.text
        Layout.fillWidth: true
        implicitHeight: grpCol.implicitHeight + 20
        radius: 10
        color: Theme.bg
        ColumnLayout {
            id: grpCol
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: 10 }
            spacing: 3
            Label { Layout.fillWidth: true; text: grp.title; color: grp.tone; font.bold: true; wrapMode: Text.Wrap }
            Label { Layout.fillWidth: true; visible: grp.body !== ""; text: grp.body; color: Theme.textDim; font.pixelSize: Theme.fontSmall; wrapMode: Text.Wrap; maximumLineCount: 4; elide: Text.ElideRight }
        }
    }

    contentItem: ColumnLayout {
        spacing: 10
        Label {
            objectName: "tagHeading"
            text: editor.step === "preview" ? qsTr("Preview") : editor.step === "result"
                  ? qsTr("%1 of %2 files updated").arg(editor.result ? editor.result.updated : 0).arg(editor.result ? editor.result.attempted + editor.result.readOnly.length + editor.result.offline.length : 0)
                  : qsTr("Edit tags")
            color: Theme.text
            font.pixelSize: Theme.fontLarge
            font.bold: true
            Accessible.role: Accessible.Heading
        }

        // ---- the fields ----------------------------------------------------------------------
        ColumnLayout {
            visible: editor.step === "edit"
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 8
            RowLayout {
                spacing: 6
                FocusButton {
                    objectName: "scopeTrack"
                    visible: !!editor.scopes.track
                    primary: editor.scope === "track"
                    text: qsTr("This song: %1").arg(editor.scopes.trackTitle || "")
                    onClicked: editor.pick("track")
                }
                FocusButton {
                    objectName: "scopeAlbum"
                    visible: !!editor.scopes.album
                    primary: editor.scope === "album"
                    text: qsTr("This album: %1 (%2)").arg(editor.scopes.albumTitle || "").arg(editor.scopes.albumFiles || 0)
                    onClicked: editor.pick("album")
                }
                FocusButton {
                    objectName: "scopeArtist"
                    visible: !!editor.scopes.artist
                    primary: editor.scope === "artist"
                    text: qsTr("All of %1 (%n album(s))", "", Number(editor.scopes.artistAlbums || 0)).arg(editor.scopes.artistName || "")
                    onClicked: editor.pick("artist")
                }
            }
            Label {
                objectName: "tagCount"
                Layout.fillWidth: true
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: !editor.values ? qsTr("Reading the tags…")
                      : qsTr("%n file(s)", "", Number(editor.values.files)) + (editor.values.unreadable > 0 ? "  ·  " + qsTr("%n not reachable", "", Number(editor.values.unreadable)) : "")
            }
            Repeater {
                model: editor.fieldKeys
                RowLayout {
                    id: fieldRow
                    required property string modelData
                    readonly property var info: editor.values ? editor.values.fields[modelData] : null
                    visible: !!info && info.allowed === true
                    Layout.fillWidth: true
                    spacing: 10
                    CheckBox {
                        id: tick
                        objectName: "tick_" + fieldRow.modelData
                        focusPolicy: Qt.StrongFocus
                        checked: editor.ticked[fieldRow.modelData] === true
                        Accessible.name: qsTr("Change %1").arg(editor.fieldLabel(fieldRow.modelData))
                        onToggled: editor.setTick(fieldRow.modelData, checked)
                        KeyNavigation.right: input
                    }
                    Label { Layout.preferredWidth: 110; text: editor.fieldLabel(fieldRow.modelData); color: Theme.textDim }
                    TextField {
                        id: input
                        objectName: "field_" + fieldRow.modelData
                        Layout.fillWidth: true
                        Accessible.name: editor.fieldLabel(fieldRow.modelData)
                        text: editor.texts[fieldRow.modelData] !== undefined ? editor.texts[fieldRow.modelData] : (fieldRow.info ? fieldRow.info.value : "")
                        placeholderText: fieldRow.info && fieldRow.info.several ? qsTr("Several values: %1").arg(fieldRow.info.sample.join(", ")) : ""
                        KeyNavigation.left: tick
                        onTextEdited: editor.setText(fieldRow.modelData, text)
                    }
                }
            }
            CheckBox {
                id: writeIds
                objectName: "writeIds"
                Layout.fillWidth: true
                focusPolicy: Qt.StrongFocus
                text: qsTr("Also write the MusicBrainz IDs, ISRCs and barcode")
                contentItem: Label { leftPadding: writeIds.indicator.width + 8; text: writeIds.text; color: Theme.text; verticalAlignment: Text.AlignVCenter; wrapMode: Text.Wrap }
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: editor.scope === "album" ? qsTr("Only ticked fields change; the rest stay as each file has them. Files are never renamed or moved.")
                      : qsTr("Only ticked fields change. Album fields are edited for the whole album. Files are never renamed or moved.")
            }
            Item { Layout.fillHeight: true }
        }

        // ---- the preview ---------------------------------------------------------------------
        ColumnLayout {
            visible: editor.step === "preview"
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 8
            Label {
                objectName: "previewSummary"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                font.bold: true
                text: editor.previewData ? qsTr("%1 files: %2 will change").arg(editor.previewData.files).arg(editor.previewData.willChange) : ""
            }
            Label {
                objectName: "previewSkips"
                visible: !!editor.previewData && (editor.previewData.readOnly > 0 || editor.previewData.offline > 0)
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.accent
                font.pixelSize: Theme.fontSmall
                text: editor.previewData ? [
                    editor.previewData.readOnly > 0 ? qsTr("%1 read-only: %2").arg(editor.previewData.readOnly).arg(editor.previewData.readOnlyNames.join(", ")) : "",
                    editor.previewData.offline > 0 ? qsTr("%1 not reachable: %2").arg(editor.previewData.offline).arg(editor.previewData.offlineNames.join(", ")) : ""
                ].filter(x => x).join("  ·  ") : ""
            }
            RowLayout {
                Layout.fillWidth: true
                Label { Layout.fillWidth: true; Layout.preferredWidth: 3; text: qsTr("File"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                Label { Layout.preferredWidth: 100; text: qsTr("Field"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                Label { Layout.fillWidth: true; Layout.preferredWidth: 2; text: qsTr("Before"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                Label { Layout.fillWidth: true; Layout.preferredWidth: 2; text: qsTr("After"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            }
            ListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: editor.previewData ? editor.previewData.rows : []
                ScrollBar.vertical: ScrollBar {}
                delegate: RowLayout {
                    id: change
                    required property var modelData
                    width: ListView.view.width - 12
                    height: 26
                    Label { Layout.fillWidth: true; Layout.preferredWidth: 3; text: change.modelData.file; color: Theme.text; font.family: "monospace"; font.pixelSize: Theme.fontSmall; elide: Text.ElideLeft }
                    Label { Layout.preferredWidth: 100; text: editor.fieldLabel(change.modelData.field); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    Label { Layout.fillWidth: true; Layout.preferredWidth: 2; text: change.modelData.before || "—"; color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                    Label { Layout.fillWidth: true; Layout.preferredWidth: 2; text: change.modelData.after; color: Theme.accent; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                }
            }
        }

        // ---- the result ----------------------------------------------------------------------
        ColumnLayout {
            visible: editor.step === "result"
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 8
            Group {
                objectName: "resultUpdated"
                title: editor.result ? qsTr("✓  %n updated", "", Number(editor.result.updated)) : ""
                tone: Theme.accent
                body: qsTr("Their new tags are in the library now; nothing was renamed or moved.")
            }
            Group {
                objectName: "resultFailed"
                visible: !!editor.result && editor.result.failed.length > 0
                title: editor.result ? qsTr("!  %n failed", "", editor.result.failed.length) : ""
                tone: Theme.danger
                body: editor.result ? editor.result.failed.slice(0, 3).map(f => f.file + ": " + editor.reasonText(f.reason)).join("\n") : ""
            }
            Group {
                objectName: "resultReadOnly"
                visible: !!editor.result && editor.result.readOnly.length > 0
                title: editor.result ? qsTr("–  %n skipped: read-only", "", editor.result.readOnly.length) : ""
                tone: Theme.accent
                body: editor.result ? qsTr("%1 does not allow changes. Change that on the server, or use Add to Library for a copy you can tag.").arg([...new Set(editor.result.readOnly.map(f => f.source))].join(", ")) : ""
            }
            FocusButton {
                id: grant
                objectName: "grantWrite"
                visible: !!editor.result && editor.result.flatpak === true && editor.result.readOnly.some(f => f.local)
                text: qsTr("Allow writing to this folder…")
                KeyNavigation.right: retry
                onClicked: {
                    const f = editor.result.readOnly.find(x => x.local)
                    editor.grantRequested(f.sourceId, f.root)
                }
            }
            Group {
                objectName: "resultOffline"
                visible: !!editor.result && editor.result.offline.length > 0
                title: editor.result ? qsTr("–  %n skipped: not reachable", "", editor.result.offline.length) : ""
                tone: Theme.accent
                body: editor.result ? qsTr("%1 is not connected. Its files keep their tags until you run this again.").arg([...new Set(editor.result.offline.map(f => f.source))].join(", ")) : ""
            }
            Item { Layout.fillHeight: true }
        }

        Label {
            objectName: "tagError"
            visible: editor.error !== ""
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.danger
            text: editor.error
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            FocusButton {
                objectName: "fillMb"
                visible: editor.step === "edit"
                enabled: !!editor.values && !!editor.values.canonical
                text: qsTr("Fill from MusicBrainz")
                KeyNavigation.right: cancelButton
                onClicked: editor.fill()
            }
            Item { Layout.fillWidth: true }
            FocusButton {
                id: cancelButton
                objectName: "tagCancel"
                visible: editor.step !== "result"
                text: qsTr("Cancel")
                KeyNavigation.right: editor.step === "edit" ? toPreviewButton : backButton
                onClicked: editor.close()
            }
            FocusButton {
                id: toPreviewButton
                objectName: "tagToPreview"
                visible: editor.step === "edit"
                primary: true
                enabled: editor.anyTicked && !editor.busy
                text: editor.busy ? qsTr("Reading…") : qsTr("Preview changes")
                KeyNavigation.left: cancelButton
                onClicked: editor.toPreview()
            }
            FocusButton {
                id: backButton
                objectName: "tagBack"
                visible: editor.step === "preview"
                text: qsTr("Back")
                KeyNavigation.left: cancelButton
                KeyNavigation.right: writeButton
                onClicked: editor.step = "edit"
            }
            FocusButton {
                id: writeButton
                objectName: "tagWrite"
                visible: editor.step === "preview"
                primary: true
                enabled: !!editor.previewData && editor.previewData.willChange > 0 && !editor.busy
                text: editor.busy ? qsTr("Writing…") : qsTr("Write %n file(s)", "", editor.previewData ? Number(editor.previewData.willChange) : 0)
                KeyNavigation.left: backButton
                onClicked: editor.write()
            }
            FocusButton {
                id: retry
                objectName: "tagRetry"
                visible: editor.step === "result" && !!editor.result && editor.result.failed.length > 0
                text: qsTr("Retry the failed ones")
                KeyNavigation.right: doneButton
                onClicked: editor.write(editor.result.failed.map(f => f.fileId))
            }
            FocusButton {
                id: doneButton
                objectName: "tagDone"
                visible: editor.step === "result"
                primary: true
                text: qsTr("Done")
                KeyNavigation.left: retry
                onClicked: editor.close()
            }
        }
    }
}
