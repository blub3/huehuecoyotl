pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #122: edit a network share. Connection: server and port, share (or export path, or
// base folder), user, the password ("Stored in the keystore", Change… to enter a new one), the
// key file (SFTP) and HTTPS (WebDAV); Test connection checks that every folder is found at the
// new place, Save connection stores it (a new password only when it works; a share that turns
// out to be the same server is joined). Folders: what each holds (changing it rescans the folder
// as the new type, after a confirmation), Remove… with a confirmation in the row, + Add folders.
// Remove share… names how many folders and files go.
// `backend` is the core or a test stub: shares(), shareEdit(share, spec, password, save),
// shareFolderContent(source, content), removeLibrary(source), shareRemove(share).
FocusScope {
    id: editor
    property var backend: null
    property var shareId: 0
    signal addFoldersRequested(var share, string name)
    signal closed()

    property var info: null
    property bool changePassword: false
    property bool busy: false
    property string status: ""
    property bool statusBad: false
    property int confirmRow: -1         // the folder row asking to confirm
    property string confirmWhat: ""     // "remove" or a new content type
    property bool confirmShare: false
    property var pending: ({ load: 0, edit: 0, action: 0 })

    readonly property var folders: info ? info.folders : []
    readonly property string protocol: info ? info.protocol : ""
    readonly property int totalFiles: folders.reduce((n, f) => n + (f.files || 0), 0)

    function contentLabel(c) {
        return c === "music" ? qsTr("Music") : c === "audiobooks" ? qsTr("Audiobooks") : qsTr("Movies & shows")
    }
    function nextContent(c) { return c === "video" ? "music" : c === "music" ? "audiobooks" : "video" }
    function load() { if (backend) pending.load = backend.shares() }
    function fill() {
        if (!info) return
        hostField.text = info.host || ""
        portField.text = info.port ? String(info.port) : ""
        shareField.text = info.share || ""
        userField.text = info.user || ""
        keyField.text = info.keyFile || ""
        tlsBox.checked = !!info.tls
        passwordField.text = ""
        changePassword = false
    }
    function spec() {
        return {
            protocol: protocol,
            host: hostField.text.trim(),
            port: parseInt(portField.text) || 0,
            share: shareField.text.trim(),
            user: protocol === "nfs" ? "" : userField.text.trim(),
            tls: protocol === "webdav" && tlsBox.checked,
            key_file: protocol === "sftp" && keyField.text.trim() !== "" ? keyField.text.trim() : null
        }
    }
    function run(save) {
        if (hostField.text.trim() === "") { status = qsTr("Please enter a server name or address."); statusBad = true; return }
        busy = true
        statusBad = false
        status = save ? qsTr("Saving…") : qsTr("Testing the connection…")
        pending.edit = backend.shareEdit(shareId, spec(), changePassword ? passwordField.text : "", save)
    }
    function edited(d) {
        busy = false
        if (d.missing && d.missing.length > 0) {
            statusBad = true
            status = qsTr("Not found at the new place: %1. Nothing was saved.").arg(d.missing.join(", "))
            return
        }
        statusBad = false
        const join = d.joins ? qsTr("“%1” is the same server: saving joins its folders into this share.").arg(d.joins.name) : ""
        if (d.saved) {
            status = d.joins ? qsTr("Saved. The folders of “%1” are now in this share.").arg(d.joins.name) : qsTr("Saved.")
            load()
        } else {
            status = [qsTr("The connection works and every folder is there."), join].filter(s => s !== "").join(" ")
        }
    }
    function ask(row, what) { confirmShare = false; confirmRow = row; confirmWhat = what }
    function confirm() {
        const f = folders[confirmRow]
        if (!f) return
        busy = true
        pending.action = confirmWhat === "remove" ? backend.removeLibrary(f.sourceId) : backend.shareFolderContent(f.sourceId, confirmWhat)
        confirmRow = -1
    }

    Component.onCompleted: load()

    Connections {
        target: editor.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === editor.pending.load && kind === "shares") {
                editor.info = (detail.shares || []).find(s => s.id === editor.shareId) || null
                if (editor.info === null && editor.pending.action !== 0) { editor.closed(); return }
                editor.fill()
            } else if (requestId === editor.pending.edit && kind === "shareEdit") {
                editor.edited(detail)
            } else if (requestId === editor.pending.action) {
                editor.busy = false
                editor.load()
            }
        }
        function onLibrariesListed(requestId) {
            if (requestId !== editor.pending.action) return
            editor.busy = false
            editor.load()
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId !== editor.pending.edit && requestId !== editor.pending.action) return
            editor.busy = false
            editor.statusBad = true
            editor.status = requestId === editor.pending.edit && kind === 1
                ? qsTr("The password or user name was not accepted. The stored password stays.") : message
        }
    }

    // The page follows the keyboard focus, so a control reached with the arrows is on screen.
    Connections {
        target: editor.Window.window
        function onActiveFocusItemChanged() {
            const it = editor.Window.activeFocusItem
            let p = it
            while (p && p !== flick.contentItem) p = p.parent
            if (!p) return
            const y = it.mapToItem(flick.contentItem, 0, 0).y
            if (y < flick.contentY) flick.contentY = Math.max(0, y - 16)
            else if (y + it.height > flick.contentY + flick.height)
                flick.contentY = Math.min(flick.contentHeight - flick.height, y + it.height - flick.height + 16)
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.implicitHeight + 32
        clip: true
        ScrollBar.vertical: ScrollBar {}

        ColumnLayout {
            id: col
            width: Math.min(parent.width, 920)
            spacing: 14

            RowLayout {
                spacing: 12
                Label {
                    Layout.fillWidth: true
                    text: editor.info ? qsTr("Edit %1").arg(editor.info.name) : qsTr("Edit share")
                    color: Theme.text
                    font.pixelSize: Theme.fontTitle
                    font.bold: true
                    elide: Text.ElideRight
                }
            }

            // ---- connection
            Label { text: qsTr("Connection"); color: Theme.textDim; font.pixelSize: Theme.fontLarge }
            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 10
                Layout.fillWidth: true

                Label { text: qsTr("Server"); color: Theme.text }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8
                    TextField {
                        id: hostField
                        objectName: "hostField"
                        Layout.fillWidth: true
                        Accessible.name: qsTr("Server")
                        inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhUrlCharactersOnly
                        KeyNavigation.right: portField
                        KeyNavigation.down: shareField
                    }
                    TextField {
                        id: portField
                        Layout.preferredWidth: 110
                        placeholderText: qsTr("Port")
                        Accessible.name: qsTr("Port")
                        inputMethodHints: Qt.ImhDigitsOnly
                        validator: IntValidator { bottom: 1; top: 65535 }
                        KeyNavigation.left: hostField
                        KeyNavigation.down: shareField
                    }
                }
                Label {
                    text: editor.protocol === "smb" ? qsTr("Share") : editor.protocol === "nfs" ? qsTr("Export path") : qsTr("Base folder (optional)")
                    color: Theme.text
                }
                TextField {
                    id: shareField
                    objectName: "shareField"
                    Layout.fillWidth: true
                    Accessible.name: editor.protocol === "smb" ? qsTr("Share") : editor.protocol === "nfs" ? qsTr("Export path") : qsTr("Base folder")
                    inputMethodHints: Qt.ImhNoAutoUppercase
                    KeyNavigation.up: hostField
                    KeyNavigation.down: editor.protocol === "nfs" ? testButton : userField
                }
                Label { visible: editor.protocol !== "nfs"; text: qsTr("User name"); color: Theme.text }
                TextField {
                    id: userField
                    objectName: "userField"
                    visible: editor.protocol !== "nfs"
                    Layout.fillWidth: true
                    Accessible.name: qsTr("User name")
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                    KeyNavigation.up: shareField
                    KeyNavigation.down: editor.changePassword ? passwordField : changeButton
                }
                Label { visible: editor.protocol !== "nfs"; text: editor.protocol === "sftp" && keyField.text.trim() !== "" ? qsTr("Key passphrase") : qsTr("Password"); color: Theme.text }
                RowLayout {
                    visible: editor.protocol !== "nfs"
                    Layout.fillWidth: true
                    spacing: 10
                    Label {
                        visible: !editor.changePassword
                        text: qsTr("Stored in the keystore")
                        color: Theme.textDim
                    }
                    FocusButton {
                        id: changeButton
                        objectName: "changePassword"
                        visible: !editor.changePassword
                        text: qsTr("Change…")
                        KeyNavigation.up: userField
                        KeyNavigation.down: editor.protocol === "sftp" ? keyField : editor.protocol === "webdav" ? tlsBox : testButton
                        onClicked: { editor.changePassword = true; passwordField.forceActiveFocus() }
                    }
                    TextField {
                        id: passwordField
                        objectName: "passwordField"
                        visible: editor.changePassword
                        Layout.fillWidth: true
                        echoMode: TextInput.Password
                        placeholderText: qsTr("New password; kept only when it works")
                        Accessible.name: qsTr("New password")
                        inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText
                        KeyNavigation.up: userField
                        KeyNavigation.down: editor.protocol === "sftp" ? keyField : editor.protocol === "webdav" ? tlsBox : testButton
                        onAccepted: editor.run(false)
                    }
                }
                Label { visible: editor.protocol === "sftp"; text: qsTr("Key file (optional)"); color: Theme.text }
                TextField {
                    id: keyField
                    visible: editor.protocol === "sftp"
                    Layout.fillWidth: true
                    Accessible.name: qsTr("Key file")
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                    KeyNavigation.up: editor.changePassword ? passwordField : changeButton
                    KeyNavigation.down: testButton
                }
                Item { visible: editor.protocol === "webdav"; Layout.preferredWidth: 1 }
                CheckBox {
                    id: tlsBox
                    visible: editor.protocol === "webdav"
                    text: qsTr("Use HTTPS")
                    KeyNavigation.up: editor.changePassword ? passwordField : changeButton
                    KeyNavigation.down: testButton
                    contentItem: Label { text: tlsBox.text; color: Theme.text; leftPadding: tlsBox.indicator.width + 8; verticalAlignment: Text.AlignVCenter }
                }
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: testButton
                    objectName: "test"
                    text: qsTr("Test connection")
                    enabled: !editor.busy
                    KeyNavigation.right: saveButton
                    KeyNavigation.up: editor.protocol === "nfs" ? shareField : editor.changePassword ? passwordField : changeButton
                    Keys.onDownPressed: editor.folders.length > 0 ? folderRows.itemAt(0).forceActiveFocus() : addButton.forceActiveFocus()
                    onClicked: editor.run(false)
                }
                FocusButton {
                    id: saveButton
                    objectName: "save"
                    primary: true
                    text: qsTr("Save connection")
                    enabled: !editor.busy
                    KeyNavigation.left: testButton
                    KeyNavigation.right: cancelButton
                    Keys.onDownPressed: editor.folders.length > 0 ? folderRows.itemAt(0).forceActiveFocus() : addButton.forceActiveFocus()
                    onClicked: editor.run(true)
                }
                FocusButton {
                    id: cancelButton
                    objectName: "cancel"
                    text: qsTr("Cancel")
                    KeyNavigation.left: saveButton
                    Keys.onDownPressed: editor.folders.length > 0 ? folderRows.itemAt(0).forceActiveFocus() : addButton.forceActiveFocus()
                    onClicked: { editor.fill(); editor.status = "" }
                }
            }
            Label {
                objectName: "status"
                visible: editor.status !== ""
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: editor.status
                color: editor.statusBad ? Theme.focus : Theme.textDim
                Accessible.role: Accessible.StaticText
                Accessible.name: text
            }

            // ---- folders
            RowLayout {
                Layout.topMargin: 12
                spacing: 12
                Label { text: qsTr("Folders"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.fillWidth: true }
                FocusButton {
                    id: addButton
                    objectName: "addFolders"
                    glyph: "+"
                    text: qsTr("Add folders")
                    KeyNavigation.up: testButton
                    Keys.onDownPressed: editor.folders.length > 0 ? folderRows.itemAt(0).forceActiveFocus() : removeShareButton.forceActiveFocus()
                    onClicked: editor.addFoldersRequested(editor.shareId, editor.info ? editor.info.name : "")
                }
            }
            Repeater {
                id: folderRows
                model: editor.folders
                FocusScope {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property bool asking: editor.confirmRow === index
                    Layout.fillWidth: true
                    implicitHeight: rowBox.implicitHeight
                    Rectangle {
                        id: rowBox
                        width: parent.width
                        implicitHeight: rowCol.implicitHeight + 16
                        radius: 8
                        color: row.activeFocus ? Theme.surfaceHi : Theme.surface
                        ColumnLayout {
                            id: rowCol
                            x: 12; y: 8
                            width: parent.width - 24
                            spacing: 8
                            RowLayout {
                                spacing: 12
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Label { text: row.modelData.name; color: Theme.text; font.pixelSize: Theme.fontBody; elide: Text.ElideRight; Layout.fillWidth: true }
                                    Label {
                                        text: (row.modelData.root || "/") + "  ·  " + (row.modelData.scanning ? qsTr("scanning…") : qsTr("%n file(s)", "", row.modelData.files || 0))
                                        color: Theme.textDim
                                        font.pixelSize: Theme.fontSmall
                                        elide: Text.ElideMiddle
                                        Layout.fillWidth: true
                                    }
                                }
                                FocusButton {
                                    id: typeButton
                                    objectName: "type" + row.index
                                    focus: true
                                    enabled: !editor.busy
                                    text: editor.contentLabel(row.modelData.content)
                                    Accessible.description: qsTr("What %1 holds; press to change").arg(row.modelData.name)
                                    KeyNavigation.right: removeButton
                                    Keys.onUpPressed: row.index === 0 ? addButton.forceActiveFocus() : folderRows.itemAt(row.index - 1).forceActiveFocus()
                                    Keys.onDownPressed: row.index === folderRows.count - 1 ? removeShareButton.forceActiveFocus() : folderRows.itemAt(row.index + 1).forceActiveFocus()
                                    onClicked: editor.ask(row.index, editor.nextContent(row.asking && editor.confirmWhat !== "remove" ? editor.confirmWhat : row.modelData.content))
                                }
                                FocusButton {
                                    id: removeButton
                                    objectName: "remove" + row.index
                                    enabled: !editor.busy
                                    text: qsTr("Remove…")
                                    Accessible.description: row.modelData.name
                                    KeyNavigation.left: typeButton
                                    Keys.onUpPressed: row.index === 0 ? addButton.forceActiveFocus() : folderRows.itemAt(row.index - 1).forceActiveFocus()
                                    Keys.onDownPressed: row.index === folderRows.count - 1 ? removeShareButton.forceActiveFocus() : folderRows.itemAt(row.index + 1).forceActiveFocus()
                                    onClicked: { editor.ask(row.index, "remove"); keepButton.forceActiveFocus() }
                                }
                            }
                            RowLayout {
                                visible: row.asking
                                spacing: 10
                                Label {
                                    objectName: "question" + row.index
                                    Layout.fillWidth: true
                                    wrapMode: Text.Wrap
                                    color: Theme.text
                                    text: editor.confirmWhat === "remove"
                                          ? qsTr("Remove “%1”? Its titles leave the library and saved copies of its files on this computer are deleted. The files on the server stay.").arg(row.modelData.name)
                                          : qsTr("Treat “%1” as %2? It is scanned again; its titles move there, and their play state and saved copies go.").arg(row.modelData.name).arg(editor.contentLabel(editor.confirmWhat))
                                }
                                FocusButton {
                                    id: keepButton
                                    objectName: "keep" + row.index
                                    text: qsTr("Keep")
                                    KeyNavigation.right: confirmButton
                                    onClicked: { editor.confirmRow = -1; typeButton.forceActiveFocus() }
                                }
                                FocusButton {
                                    id: confirmButton
                                    objectName: "confirm" + row.index
                                    primary: true
                                    text: editor.confirmWhat === "remove" ? qsTr("Remove folder") : qsTr("Change to %1").arg(editor.contentLabel(editor.confirmWhat))
                                    KeyNavigation.left: keepButton
                                    onClicked: editor.confirm()
                                }
                            }
                        }
                    }
                }
            }
            Label {
                visible: editor.folders.length === 0 && editor.info !== null
                text: qsTr("No folders yet.")
                color: Theme.textDim
            }

            // ---- removing the share
            FocusButton {
                id: removeShareButton
                objectName: "removeShare"
                Layout.topMargin: 12
                text: qsTr("Remove share…")
                enabled: !editor.busy && editor.info !== null
                KeyNavigation.up: editor.folders.length > 0 ? folderRows.itemAt(editor.folders.length - 1) : addButton
                onClicked: { editor.confirmRow = -1; editor.confirmShare = true; keepShareButton.forceActiveFocus() }
            }
            RowLayout {
                visible: editor.confirmShare
                spacing: 10
                Label {
                    objectName: "shareQuestion"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.text
                    text: qsTr("Remove “%1”? Its %2 and %3 leave the library, saved copies on this computer are deleted, and its password leaves the keystore. The files on the server stay.")
                          .arg(editor.info ? editor.info.name : "")
                          .arg(qsTr("%n folder(s)", "", editor.folders.length))
                          .arg(qsTr("%n file(s)", "", editor.totalFiles))
                }
                FocusButton {
                    id: keepShareButton
                    objectName: "keepShare"
                    text: qsTr("Keep")
                    KeyNavigation.right: confirmShareButton
                    onClicked: { editor.confirmShare = false; removeShareButton.forceActiveFocus() }
                }
                FocusButton {
                    id: confirmShareButton
                    objectName: "confirmRemoveShare"
                    primary: true
                    text: qsTr("Remove share")
                    KeyNavigation.left: keepShareButton
                    onClicked: { editor.confirmShare = false; editor.busy = true; editor.pending.action = editor.backend.shareRemove(editor.shareId) }
                }
            }
        }
    }
}
