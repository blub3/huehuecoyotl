pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// One server on the Sources page (issue #119): a network share, or "This computer", with its
// library folders inside. The header shows the protocol, name, address, user, online or offline
// and the number of folders, with Add folders, Edit share (network shares) and Rescan; each
// folder shows what it holds, its path, its count, Rescan and Remove…. Right opens a closed card,
// Left closes an open one (and leaves for the rail from a closed one); Up and Down move through
// the header and the folders and on to the cards around.
FocusScope {
    id: card
    // A share from Core.shares(), or { local: true, folders } for this computer.
    property var info: ({})
    property bool open: true
    property bool canEdit: false
    signal toggled(bool open)
    signal addFoldersRequested()
    signal editRequested()
    signal rescanRequested(var sourceId)
    signal removeRequested(var folder)
    signal upRequested()
    signal downRequested()
    signal leftEdge()

    readonly property bool local: info.local === true
    readonly property var folders: info.folders || []
    readonly property bool online: local || info.online !== false
    function focusFirst() { toggle.forceActiveFocus() }
    // A folder row's first button (each row is a FocusScope whose Rescan button has focus).
    function focusFolder(i) {
        const r = folderRows.itemAt(i)
        if (r) r.forceActiveFocus()
    }
    function focusLast() {
        if (open && folderRows.count > 0) card.focusFolder(folderRows.count - 1)
        else toggle.forceActiveFocus()
    }
    function protocolLabel() {
        if (local) return qsTr("PC")
        return ({ smb: "SMB", nfs: "NFS", webdav: "DAV", ftp: "FTP", sftp: "SFTP" })[info.protocol] ?? (info.protocol || "")
    }
    function contentLabel(c) {
        return c === "music" ? qsTr("Music") : c === "audiobooks" ? qsTr("Audiobooks") : qsTr("Movies & shows")
    }
    function countLabel(f) {
        if (f.scanning) return qsTr("scanning…")
        if (!f.online) return qsTr("offline")
        return f.content === "music" || f.content === "audiobooks" ? qsTr("%n file(s)", "", f.files) : qsTr("%n video(s)", "", f.files)
    }

    implicitHeight: box.implicitHeight
    Rectangle {
        id: box
        width: parent.width
        implicitHeight: content.implicitHeight + 2
        radius: Theme.radius
        color: Theme.surface
        border.width: 1
        border.color: card.online ? Theme.surfaceHi : Theme.focus
        ColumnLayout {
            id: content
            width: parent.width
            spacing: 0
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 12
                spacing: 12
                FocusButton {
                    id: toggle
                    objectName: "toggle"
                    glyph: card.open ? "▾" : "▸"
                    tip: card.open ? qsTr("Close %1").arg(header.text) : qsTr("Open %1").arg(header.text)
                    Accessible.description: header.text
                    KeyNavigation.right: addFolders.visible ? addFolders : rescanAll
                    Keys.onRightPressed: (e) => { if (!card.open) card.toggled(true); else e.accepted = false }
                    Keys.onLeftPressed: { if (card.open) card.toggled(false); else card.leftEdge() }
                    Keys.onUpPressed: card.upRequested()
                    Keys.onDownPressed: card.open && folderRows.count > 0 ? card.focusFolder(0) : card.downRequested()
                    onClicked: card.toggled(!card.open)
                }
                Rectangle {
                    Layout.preferredWidth: Math.max(42, badge.implicitWidth + 12)
                    Layout.preferredHeight: 22
                    radius: 5
                    color: card.local ? Theme.surfaceHi : Theme.accent
                    Text {
                        id: badge
                        anchors.centerIn: parent
                        text: card.protocolLabel()
                        color: card.local ? Theme.text : Theme.bg
                        font.pixelSize: 11
                        font.bold: true
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    RowLayout {
                        spacing: 10
                        Label { id: header; objectName: "shareName"; text: card.local ? qsTr("This computer") : (card.info.name || ""); color: Theme.text; font.pixelSize: Theme.fontBody; font.bold: true }
                        Label { visible: !card.local; text: card.info.url || ""; color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle; Layout.fillWidth: true }
                    }
                    Label {
                        objectName: "shareState"
                        Layout.fillWidth: true
                        color: card.online ? Theme.textDim : Theme.focus
                        font.pixelSize: Theme.fontSmall
                        text: [card.local ? qsTr("always here") : card.online ? qsTr("online") : qsTr("offline · saved copies play"),
                               card.local || !card.info.user ? "" : qsTr("as %1").arg(card.info.user),
                               qsTr("%n folder(s)", "", card.folders.length)].filter(x => x !== "").join("  ·  ")
                    }
                }
                FocusButton {
                    id: addFolders
                    objectName: "addFolders"
                    visible: !card.local
                    glyph: "+"
                    text: qsTr("Add folders")
                    KeyNavigation.left: toggle
                    KeyNavigation.right: edit.visible ? edit : rescanAll
                    Keys.onUpPressed: card.upRequested()
                    Keys.onDownPressed: card.open && folderRows.count > 0 ? card.focusFolder(0) : card.downRequested()
                    onClicked: card.addFoldersRequested()
                }
                FocusButton {
                    id: edit
                    objectName: "editShare"
                    visible: !card.local && card.canEdit
                    text: qsTr("Edit share")
                    KeyNavigation.left: addFolders
                    KeyNavigation.right: rescanAll
                    Keys.onUpPressed: card.upRequested()
                    Keys.onDownPressed: card.open && folderRows.count > 0 ? card.focusFolder(0) : card.downRequested()
                    onClicked: card.editRequested()
                }
                FocusButton {
                    id: rescanAll
                    objectName: "rescanShare"
                    text: qsTr("Rescan")
                    Accessible.description: qsTr("Rescan every folder of %1").arg(header.text)
                    KeyNavigation.left: edit.visible ? edit : addFolders.visible ? addFolders : toggle
                    Keys.onUpPressed: card.upRequested()
                    Keys.onDownPressed: card.open && folderRows.count > 0 ? card.focusFolder(0) : card.downRequested()
                    onClicked: { for (const f of card.folders) card.rescanRequested(f.sourceId) }
                }
            }
            Repeater {
                id: folderRows
                model: card.open ? card.folders : []
                FocusScope {
                    id: row
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    implicitHeight: 56
                    Rectangle { width: parent.width; height: 1; color: Theme.surfaceHi }
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: 2
                        radius: 8
                        color: row.activeFocus ? Theme.surfaceHi : "transparent"
                    }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 34
                        anchors.rightMargin: 12
                        spacing: 12
                        Rectangle {
                            Layout.preferredWidth: 128
                            Layout.preferredHeight: 24
                            radius: 6
                            color: row.modelData.content === "music" ? "#3a2e1a" : row.modelData.content === "audiobooks" ? "#33284a" : "#2b3a4a"
                            Label {
                                anchors.centerIn: parent
                                text: card.contentLabel(row.modelData.content)
                                color: row.modelData.content === "music" ? "#ffd27d" : row.modelData.content === "audiobooks" ? "#d8c6f5" : "#c9e2f2"
                                font.pixelSize: Theme.fontSmall
                                font.bold: true
                            }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1
                            Label { text: row.modelData.name; color: Theme.text; font.pixelSize: Theme.fontBody; elide: Text.ElideRight; Layout.fillWidth: true }
                            Label {
                                objectName: "folderLine" + row.index
                                Layout.fillWidth: true
                                text: (row.modelData.root || "/") + "  ·  " + card.countLabel(row.modelData)
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                                elide: Text.ElideMiddle
                            }
                        }
                        FocusButton {
                            id: rescanOne
                            objectName: "rescanFolder" + row.index
                            focus: true
                            text: qsTr("Rescan")
                            Accessible.description: row.modelData.name
                            KeyNavigation.right: removeOne
                            Keys.onLeftPressed: card.leftEdge()
                            Keys.onUpPressed: row.index === 0 ? toggle.forceActiveFocus() : card.focusFolder(row.index - 1)
                            Keys.onDownPressed: row.index === folderRows.count - 1 ? card.downRequested() : card.focusFolder(row.index + 1)
                            onClicked: card.rescanRequested(row.modelData.sourceId)
                        }
                        FocusButton {
                            id: removeOne
                            objectName: "removeFolder" + row.index
                            text: qsTr("Remove…")
                            Accessible.description: row.modelData.name
                            KeyNavigation.left: rescanOne
                            Keys.onUpPressed: row.index === 0 ? toggle.forceActiveFocus() : card.focusFolder(row.index - 1)
                            Keys.onDownPressed: row.index === folderRows.count - 1 ? card.downRequested() : card.focusFolder(row.index + 1)
                            onClicked: card.removeRequested(row.modelData)
                        }
                    }
                }
            }
        }
    }
}
