pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl
import "MediaInfo.js" as MediaInfo

// Downloads (issues #4 and #5): saved copies and "Save as…" exports, in progress, failed and
// done, with the space saved copies use. Progress comes from the core's events; a change of
// state re-reads the list.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var items: []
    property real used: 0
    property string storageDir: ""
    property var request: 0
    property var pending: null

    function group(d) {
        if (d.state === "queued" || d.state === "running" || d.state === "paused") return 0
        if (d.state === "failed" || d.state === "outdated") return 1
        return 2
    }
    function sorted(list) {
        return list.slice().sort((a, b) => group(a) - group(b) || (group(a) === 2 ? b.completedAt - a.completedAt : a.id - b.id))
    }
    function reload() { request = Core.downloads() }
    function percent(d) { return d.size > 0 ? Math.floor(100 * d.bytesDone / d.size) : 0 }
    function stateText(d) {
        switch (d.state) {
        case "queued": return qsTr("Waiting")
        case "running": return qsTr("%1 %").arg(percent(d)) + "  ·  " + MediaInfo.size(d.bytesDone) + " / " + MediaInfo.size(d.size)
        case "paused": return qsTr("Paused at %1 %").arg(percent(d))
        case "failed": return Core.downloadErrorText(d.errorKind, d.error)
        case "outdated": return d.fileId > 0 ? qsTr("Outdated: the file on the server changed")
                                             : qsTr("Outdated: the file is no longer on the server")
        }
        return d.kind === "export" ? qsTr("Exported · %1").arg(MediaInfo.size(d.size)) : qsTr("Saved · %1").arg(MediaInfo.size(d.size))
    }

    Component.onCompleted: reload()
    onActiveFocusChanged: if (activeFocus) list.forceActiveFocus()

    Timer { id: settle; interval: 300; onTriggered: page.reload() }

    Connections {
        target: Core
        function onDownloadsListed(requestId, items, used, dir) {
            const keep = list.currentIndex
            page.items = page.sorted(items)
            page.used = used
            page.storageDir = dir
            list.currentIndex = Math.min(Math.max(keep, 0), list.count - 1)
        }
        function onDownloadProgress(e) {
            const i = page.items.findIndex(d => d.id === e.mainId)
            if (i < 0 || e.state !== "running" || e.id !== e.mainId || page.items[i].state !== "running") {
                settle.restart()
                return
            }
            // A running main file: update its row in place.
            const row = Object.assign({}, page.items[i], { bytesDone: e.bytesDone, size: e.size })
            const copy = page.items.slice()
            copy[i] = row
            page.items = copy
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === page.request) (page.Window.window as Main).showError(message)
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        spacing: 14

        Label {
            text: qsTr("Downloads")
            color: Theme.text
            font.pixelSize: Theme.fontTitle
            font.bold: true
            Accessible.role: Accessible.Heading
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("Saved copies use %1 in %2").arg(MediaInfo.size(page.used)).arg(page.storageDir)
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.WrapAnywhere
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            focus: true
            spacing: 6
            model: page.items
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Downloads")
            ScrollBar.vertical: ScrollBar {}
            Keys.onLeftPressed: Nav.focusRail()
            section.property: "state"
            section.criteria: ViewSection.FullString
            section.delegate: Item {
                id: head
                required property string section
                readonly property string label: head.section === "running" || head.section === "queued" || head.section === "paused" ? qsTr("In progress")
                    : head.section === "failed" ? qsTr("Failed")
                    : head.section === "outdated" ? qsTr("Outdated") : qsTr("Done")
                width: ListView.view.width
                height: 40
                Label {
                    anchors.bottom: parent.bottom
                    text: head.label
                    color: Theme.textDim
                    font.pixelSize: Theme.fontLarge
                }
            }

            delegate: FocusScope {
                id: row
                required property var modelData
                required property int index
                readonly property var dl: modelData
                readonly property bool active: dl.state === "queued" || dl.state === "running" || dl.state === "paused"
                width: ListView.view.width
                height: 96
                focus: ListView.isCurrentItem
                Accessible.role: Accessible.ListItem
                Accessible.name: dl.title + ", " + page.stateText(dl)

                Rectangle {
                    anchors.fill: parent
                    radius: Theme.radius
                    color: row.activeFocus ? Theme.surfaceHi : Theme.surface
                    border.width: row.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 14
                    spacing: 14
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4
                        Label {
                            Layout.fillWidth: true
                            text: (row.dl.kind === "export" ? "⇩ " : "⤓ ") + row.dl.title
                            color: Theme.text
                            font.pixelSize: Theme.fontBody
                            font.bold: true
                            elide: Text.ElideRight
                        }
                        Label {
                            Layout.fillWidth: true
                            text: page.stateText(row.dl)
                            color: row.dl.state === "failed" || row.dl.state === "outdated" ? Theme.danger : Theme.textDim
                            font.pixelSize: Theme.fontSmall
                            elide: Text.ElideRight
                        }
                        Rectangle {
                            visible: row.active
                            Layout.fillWidth: true
                            Layout.preferredHeight: 4
                            radius: 2
                            color: Qt.rgba(1, 1, 1, 0.08)
                            Rectangle { width: parent.width * page.percent(row.dl) / 100; height: 4; radius: 2; color: Theme.accent }
                        }
                        Label {
                            Layout.fillWidth: true
                            text: row.dl.path
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                            elide: Text.ElideMiddle
                            opacity: 0.8
                        }
                    }
                    FocusButton {
                        id: first
                        focus: true
                        visible: text !== ""
                        text: row.dl.state === "running" || row.dl.state === "queued" ? qsTr("Pause")
                            : row.dl.state === "paused" ? qsTr("Resume")
                            : row.dl.state === "failed" ? qsTr("Try again")
                            // Only while the file is still in the library (else it is gone from the server).
                            : row.dl.state === "outdated" ? (row.dl.fileId > 0 ? qsTr("Save again") : "")
                            : row.dl.kind === "export" ? qsTr("Show in file manager") : ""
                        KeyNavigation.right: second
                        Keys.onLeftPressed: Nav.focusRail()
                        onClicked: {
                            const d = row.dl
                            if (d.state === "running" || d.state === "queued") Core.pauseDownload(d.id)
                            else if (d.state === "paused" || d.state === "failed") Core.resumeDownload(d.id)
                            else if (d.state === "outdated") page.request = Core.saveItem(d.itemId, d.fileId)
                            else Qt.openUrlExternally("file://" + d.path.substring(0, d.path.lastIndexOf("/")))
                            settle.restart()
                        }
                    }
                    FocusButton {
                        id: second
                        focus: !first.visible
                        text: row.active ? qsTr("Cancel")
                            : row.dl.kind === "export" ? (row.dl.state === "done" ? qsTr("Dismiss") : qsTr("Remove"))
                            : qsTr("Delete saved copy")
                        KeyNavigation.left: first.visible ? first : null
                        Keys.onLeftPressed: (e) => { if (first.visible) first.forceActiveFocus(); else Nav.focusRail() }
                        onClicked: {
                            const d = row.dl
                            // A finished export belongs to the user: dismissing only forgets it.
                            if (d.kind === "export" || row.active) page.request = Core.deleteDownload(d.id)
                            else { page.pending = d; confirm.open() }
                        }
                    }
                }
            }
        }
    }

    Label {
        anchors.centerIn: parent
        visible: page.items.length === 0
        text: qsTr("Nothing saved yet. Use “Save” on a movie or episode to keep a copy on this computer.")
        color: Theme.textDim
        font.pixelSize: Theme.fontLarge
        wrapMode: Text.Wrap
        width: Math.min(560, parent.width - 64)
        horizontalAlignment: Text.AlignHCenter
    }

    Popup {
        id: confirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: keep.forceActiveFocus()
        onClosed: list.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("Delete the saved copy of “%1” from this computer? Only the local copy is removed; the file on the server stays.").arg(page.pending ? page.pending.title : "")
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: keep
                    text: qsTr("Keep")
                    KeyNavigation.right: ok
                    onClicked: confirm.close()
                }
                FocusButton {
                    id: ok
                    primary: true
                    text: qsTr("Delete saved copy")
                    KeyNavigation.left: keep
                    onClicked: {
                        if (page.pending) page.request = Core.deleteDownload(page.pending.id)
                        confirm.close()
                    }
                }
            }
        }
    }
}
