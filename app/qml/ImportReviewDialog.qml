pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// Issue #155: an import that waits for the user. *Which one is it?* shows the user's files and
// opens the matcher (`findMatch`), or imports with the file's own names. *Already there* shows
// the library's copy and the one to import side by side: another version, replace, or keep; a
// folder holding files the library did not put there can only get another name or be skipped.
// `backend`: libraryCall("importReview", {job}) and ("importResolve", {job, choice, label, all}),
// ("importSkip", {job}), answered with musicDetailReady or requestFailed.
Popup {
    id: dialog
    property var backend: null
    property var info: null
    property string choice: "version"
    property string error: ""
    property Item from: null
    property var pending: ({ info: 0, act: 0 })
    signal findMatch(var info)
    signal resolved()

    readonly property bool isClash: !!info && info.review === "clash"
    readonly property bool foreign: isClash && !info.library

    function show(job, item) {
        from = item || null
        info = null
        error = ""
        pending.info = backend.libraryCall("importReview", { job: job.id })
        open()
    }
    function act(name, args) {
        error = ""
        pending.act = backend.libraryCall(name, Object.assign({ job: info.job }, args))
    }
    function confirm() {
        act("importResolve", { choice: choice, label: choice === "rename" ? nameField.text.trim() : "", all: sameForNext.checked })
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(860, (parent ? parent.width : 900) - 60)
    modal: true
    focus: true
    padding: 26
    closePolicy: Popup.CloseOnEscape
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onClosed: if (from) from.forceActiveFocus()

    Connections {
        target: dialog.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === dialog.pending.info && kind === "importReview") {
                dialog.info = detail
                dialog.choice = !detail.library && detail.review === "clash" ? "rename" : "version"
                if (dialog.choice === "rename") nameField.text = detail.label || ""
                Qt.callLater(() => (detail.review === "clash" ? primary : findButton).forceActiveFocus())
            } else if (requestId === dialog.pending.act) {
                dialog.resolved()
                dialog.close()
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === dialog.pending.act || requestId === dialog.pending.info) dialog.error = message
        }
    }

    component Option: Button {
        id: opt
        property string title: ""
        property string body: ""
        property string value: ""
        Layout.fillWidth: true
        focusPolicy: Qt.StrongFocus
        visible: value !== "replace" && value !== "version" && value !== "keep" || !dialog.foreign
        Accessible.role: Accessible.RadioButton
        Accessible.checked: dialog.choice === value
        Accessible.name: title
        Accessible.description: body
        padding: 12
        onClicked: dialog.choice = value
        Keys.onReturnPressed: clicked()
        Keys.onEnterPressed: clicked()
        background: Rectangle {
            radius: 12
            color: dialog.choice === opt.value ? Theme.surfaceHi : Theme.bg
            border.width: opt.activeFocus ? Theme.focusWidth : dialog.choice === opt.value ? 1 : 0
            border.color: Theme.focus
        }
        contentItem: ColumnLayout {
            spacing: 3
            Label { Layout.fillWidth: true; text: opt.title; color: Theme.text; font.bold: true; wrapMode: Text.Wrap }
            Label { Layout.fillWidth: true; text: opt.body; color: Theme.textDim; font.pixelSize: Theme.fontSmall; wrapMode: Text.Wrap }
        }
    }
    component Side: Rectangle {
        id: side
        property string label: ""
        property string what: ""
        property string detail: ""
        property string path: ""
        Layout.fillWidth: true
        Layout.preferredWidth: 1
        implicitHeight: sideCol.implicitHeight + 24
        radius: 12
        color: Theme.bg
        ColumnLayout {
            id: sideCol
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: 12 }
            spacing: 3
            Label { text: side.label; color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            Label { Layout.fillWidth: true; text: side.what; color: Theme.text; font.bold: true; elide: Text.ElideRight }
            Label { Layout.fillWidth: true; text: side.detail; color: Theme.text; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
            Label { Layout.fillWidth: true; text: side.path; color: Theme.textDim; font.family: "monospace"; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle }
        }
    }

    contentItem: ColumnLayout {
        spacing: 12
        Label {
            objectName: "reviewTitle"
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontLarge
            font.bold: true
            Accessible.role: Accessible.Heading
            text: !dialog.info ? qsTr("Loading…")
                  : dialog.foreign ? qsTr("The folder for “%1” holds other files").arg(dialog.info.title)
                  : dialog.isClash ? qsTr("“%1” is already in your library").arg(dialog.info.title)
                  : qsTr("Which one is “%1”?").arg(dialog.info.title)
        }

        // ---- which release ------------------------------------------------------------------
        ColumnLayout {
            visible: !!dialog.info && !dialog.isClash
            Layout.fillWidth: true
            spacing: 10
            Side {
                objectName: "yourFiles"
                Layout.preferredWidth: -1
                label: qsTr("Your files")
                what: dialog.info ? [dialog.info.artist, dialog.info.title].filter(x => x).join("  ·  ") : ""
                detail: dialog.info ? [qsTr("%n file(s)", "", Number(dialog.info.files || 0)), MediaInfo.size(Number(dialog.info.size || 0)), dialog.info.format].filter(x => x).join("  ·  ") : ""
                path: dialog.info ? dialog.info.from : ""
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("This is not surely matched. The match you choose decides the name, the year and the IDs written into the copies.")
            }
        }

        // ---- already there -------------------------------------------------------------------
        ColumnLayout {
            visible: dialog.isClash
            Layout.fillWidth: true
            spacing: 10
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("Nothing is copied or replaced until you choose.")
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                Side {
                    objectName: "libraryCopy"
                    label: dialog.foreign ? qsTr("In that folder") : qsTr("In your library")
                    what: dialog.info ? dialog.info.title : ""
                    detail: dialog.info && dialog.info.library ? [qsTr("%n file(s)", "", Number(dialog.info.library.files)), dialog.info.library.format].filter(x => x).join("  ·  ") : qsTr("Files the library did not put there")
                    path: dialog.info ? (dialog.info.library ? dialog.info.library.folder : dialog.info.folder) : ""
                }
                Side {
                    objectName: "importCopy"
                    label: qsTr("To import")
                    what: dialog.info ? dialog.info.title : ""
                    detail: dialog.info ? [qsTr("%n file(s)", "", Number(dialog.info.files || 0)), dialog.info.format].filter(x => x).join("  ·  ") : ""
                    path: dialog.info ? dialog.info.from : ""
                }
            }
            Option {
                id: optVersion
                objectName: "optVersion"
                value: "version"
                title: qsTr("Add as another version (recommended)")
                body: dialog.info ? qsTr("Goes into “%1 [%2]”. The item shows both versions, and you pick which plays.").arg(dialog.info.folder).arg(dialog.info.label || "2") : ""
                KeyNavigation.down: optReplace
            }
            Option {
                id: optReplace
                objectName: "optReplace"
                value: "replace"
                title: qsTr("Replace the copy in the library")
                body: qsTr("The old files move to the .trash folder inside your library. Play counts and playlists stay.")
                KeyNavigation.up: optVersion
                KeyNavigation.down: optKeep
            }
            Option {
                id: optKeep
                objectName: "optKeep"
                value: "keep"
                title: qsTr("Keep the library's copy")
                body: qsTr("Nothing is copied. This import is marked as done.")
                KeyNavigation.up: optReplace
                KeyNavigation.down: primary
            }
            Option {
                id: optRename
                objectName: "optRename"
                visible: dialog.foreign
                value: "rename"
                title: qsTr("Use another folder name")
                body: qsTr("The files there stay as they are.")
                KeyNavigation.down: nameField
            }
            TextField {
                id: nameField
                objectName: "reviewName"
                visible: dialog.foreign
                Layout.fillWidth: true
                placeholderText: qsTr("Added to the folder name")
                Accessible.name: qsTr("Added to the folder name")
                KeyNavigation.up: optRename
                KeyNavigation.down: primary
            }
            CheckBox {
                id: sameForNext
                objectName: "sameForNext"
                Layout.fillWidth: true
                focusPolicy: Qt.StrongFocus
                text: qsTr("Do the same for the next clashes of this import")
                contentItem: Label { leftPadding: sameForNext.indicator.width + 8; text: sameForNext.text; color: Theme.text; verticalAlignment: Text.AlignVCenter; wrapMode: Text.Wrap }
            }
        }

        Label {
            objectName: "reviewError"
            visible: dialog.error !== ""
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.danger
            text: dialog.error
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            FocusButton {
                id: findButton
                objectName: "reviewFind"
                visible: !!dialog.info && !dialog.isClash
                primary: true
                text: dialog.info && dialog.info.type === "music" ? qsTr("Search MusicBrainz…") : qsTr("Find the match…")
                KeyNavigation.right: asIs
                onClicked: { const i = dialog.info; dialog.close(); dialog.findMatch(i) }
            }
            FocusButton {
                id: asIs
                objectName: "reviewAsIs"
                visible: !!dialog.info && !dialog.isClash
                text: qsTr("Import with its own names")
                KeyNavigation.left: findButton
                KeyNavigation.right: later
                onClicked: dialog.act("importResolve", { choice: "asIs" })
            }
            Item { Layout.fillWidth: true }
            FocusButton {
                id: skip
                objectName: "reviewSkip"
                text: qsTr("Skip")
                enabled: !!dialog.info
                KeyNavigation.right: later
                onClicked: dialog.act("importSkip", {})
            }
            FocusButton {
                id: later
                objectName: "reviewLater"
                text: qsTr("Decide later")
                KeyNavigation.left: skip
                KeyNavigation.right: primary.visible ? primary : null
                onClicked: dialog.close()
            }
            FocusButton {
                id: primary
                objectName: "reviewConfirm"
                visible: dialog.isClash
                primary: true
                enabled: dialog.choice !== "rename" || nameField.text.trim() !== ""
                KeyNavigation.left: later
                text: dialog.choice === "replace" ? qsTr("Replace") : dialog.choice === "keep" ? qsTr("Keep the copy")
                      : dialog.choice === "rename" ? qsTr("Use this name") : qsTr("Add as another version")
                onClicked: dialog.confirm()
            }
        }
    }
}
