pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #120: add several folders of a network share at once. The folders of the current path,
// each with a quick look ("12 videos · 3 folders") and a guess of what it holds; a tick picks a
// folder, its type chip cycles Movies & shows → Music → Audiobooks with one press, and › opens
// it. Folders already in the library are marked and cannot be picked again; a folder inside one
// already added says so. The panel on the right lists what will be added; "Add N folders" adds
// them all (backend.shareAddFolders) and emits `added`.
// `backend` is the core (or a test stub): shareBrowse(target, password, path),
// shareAddFolders(share, folders) and, for a new share (issue #121), shareCreate(target,
// password, folders), answering with musicDetailReady or requestFailed.
FocusScope {
    id: browser
    property var backend: null
    // {share: id} for a share in the library, or a new share's settings (issue #121): its
    // folders are added together with the share.
    property var target: ({})
    property bool showTitle: true
    // Browse the root when created; off where the owner starts it (AddShareFlow).
    property bool autoBrowse: true
    property string password: ""
    property string shareName: ""
    signal added(int count)
    signal cancelled()

    property string path: "/"
    property string parentPath: ""
    property var folders: []
    property bool busy: false
    property string status: ""
    // The picked folders, in the order they were picked: [{path, name, content}].
    property var chosen: []
    property var pending: ({ browse: 0, add: 0 })

    function contentLabel(c) {
        return c === "music" ? qsTr("Music") : c === "audiobooks" ? qsTr("Audiobooks") : qsTr("Movies & shows")
    }
    function nextContent(c) { return c === "video" ? "music" : c === "music" ? "audiobooks" : "video" }
    function hint(f) {
        if (f.inLibrary && f.inLibrary.length > 0)
            return qsTr("in your library as %1").arg(f.inLibrary.map(c => browser.contentLabel(c)).join(", "))
        const parts = []
        if (f.videos > 0) parts.push(qsTr("%n video(s)", "", f.videos))
        if (f.audio > 0) parts.push(qsTr("%n audio file(s)", "", f.audio))
        if (f.books > 0) parts.push(qsTr("%n audiobook(s)", "", f.books))
        if (f.folders > 0) parts.push(qsTr("%n folder(s)", "", f.folders))
        if (parts.length === 0) parts.push(f.looked ? qsTr("no media found") : qsTr("not looked into"))
        if (f.inside) parts.push(qsTr("inside “%1”, already in your library").arg(f.inside))
        return parts.join("  ·  ")
    }
    function indexOfChosen(p) {
        for (let i = 0; i < chosen.length; ++i) if (chosen[i].path === p) return i
        return -1
    }
    function contentOf(f) {
        const i = indexOfChosen(f.path)
        return i >= 0 ? chosen[i].content : (f.guess || "video")
    }
    function toggle(f) {
        if (f.inLibrary && f.inLibrary.length > 0) return
        const i = indexOfChosen(f.path)
        const next = chosen.slice()
        if (i >= 0) next.splice(i, 1)
        else next.push({ path: f.path, name: f.name, content: f.guess || "video" })
        chosen = next
    }
    function cycle(f) {
        const i = indexOfChosen(f.path)
        const next = chosen.slice()
        if (i >= 0) next[i] = { path: next[i].path, name: next[i].name, content: nextContent(next[i].content) }
        else if (!(f.inLibrary && f.inLibrary.length > 0)) next.push({ path: f.path, name: f.name, content: nextContent(f.guess || "video") })
        chosen = next
    }
    function unchoose(i) {
        const next = chosen.slice()
        next.splice(i, 1)
        chosen = next
    }

    function browse(p) {
        if (!backend) return
        busy = true
        status = qsTr("Looking into the folders…")
        pending.browse = backend.shareBrowse(target, password, p)
    }
    function addAll() {
        if (!backend || chosen.length === 0) return
        busy = true
        status = qsTr("Adding…")
        const list = chosen.map(c => ({ path: c.path, content: c.content, name: c.name }))
        // A new share's settings name a protocol; a share in the library is just {share: id}.
        pending.add = target.protocol === undefined ? backend.shareAddFolders(target.share, list)
                                                    : backend.shareCreate(target, password, list)
    }
    function focusFirst() {
        if (folderList.count > 0) folderList.forceActiveFocus()
        else upButton.forceActiveFocus()
    }

    Component.onCompleted: if (autoBrowse) browse("/")

    Connections {
        target: browser.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === browser.pending.browse && kind === "shareFolders") {
                browser.busy = false
                browser.path = detail.path
                browser.parentPath = detail.parent
                browser.folders = detail.folders
                browser.status = detail.folders.length === 0 ? qsTr("No folders here.") : ""
                folderList.currentIndex = 0
                Qt.callLater(browser.focusFirst)
            } else if (requestId === browser.pending.add) {
                browser.busy = false
                browser.added(browser.chosen.length)
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId !== browser.pending.browse && requestId !== browser.pending.add) return
            browser.busy = false
            browser.status = message
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 16

        Label {
            visible: browser.showTitle
            text: browser.shareName !== "" ? qsTr("Add folders from %1").arg(browser.shareName) : qsTr("Add folders")
            color: Theme.text
            font.pixelSize: Theme.fontTitle
            font.bold: true
        }

        RowLayout {
            spacing: 12
            FocusButton {
                id: upButton
                objectName: "up"
                glyph: "↑"
                tip: qsTr("Up one folder")
                enabled: browser.parentPath !== "" && !browser.busy
                KeyNavigation.down: folderList
                KeyNavigation.right: cancelButton
                onClicked: browser.browse(browser.parentPath)
            }
            Label {
                objectName: "pathBar"
                Layout.fillWidth: true
                text: browser.path
                color: Theme.text
                font.pixelSize: Theme.fontBody
                elide: Text.ElideMiddle
            }
            Label {
                objectName: "status"
                text: browser.status
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 16

            ListView {
                id: folderList
                objectName: "folderList"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                focus: true
                model: browser.folders
                keyNavigationEnabled: true
                ScrollBar.vertical: ScrollBar {}
                Keys.onUpPressed: (e) => { if (currentIndex === 0) upButton.forceActiveFocus(); else e.accepted = false }

                delegate: FocusScope {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property bool inLibrary: !!(modelData.inLibrary && modelData.inLibrary.length > 0)
                    readonly property bool picked: browser.indexOfChosen(modelData.path) >= 0
                    readonly property string content: browser.contentOf(modelData)
                    width: folderList.width
                    height: 60
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: 2
                        radius: 8
                        color: row.activeFocus ? Theme.surfaceHi : row.picked ? Theme.surface : "transparent"
                    }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        spacing: 12
                        FocusButton {
                            id: tick
                            objectName: "tick" + row.index
                            focus: true
                            // Stays focusable (the row's way in from the keyboard); picks nothing.
                            opacity: row.inLibrary ? 0.5 : 1
                            glyph: row.inLibrary ? "✓" : row.picked ? "☑" : "☐"
                            tip: row.inLibrary ? qsTr("%1 is in your library").arg(row.modelData.name) : row.picked ? qsTr("Don't add %1").arg(row.modelData.name) : qsTr("Add %1").arg(row.modelData.name)
                            KeyNavigation.right: chip
                            onClicked: browser.toggle(row.modelData)
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1
                            Label { text: row.modelData.name; color: row.inLibrary ? Theme.textDim : Theme.text; font.pixelSize: Theme.fontBody; elide: Text.ElideRight; Layout.fillWidth: true }
                            Label {
                                objectName: "hint" + row.index
                                Layout.fillWidth: true
                                text: browser.hint(row.modelData)
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                                elide: Text.ElideRight
                            }
                        }
                        FocusButton {
                            id: chip
                            objectName: "chip" + row.index
                            visible: !row.inLibrary
                            text: browser.contentLabel(row.content) + (row.picked || row.modelData.guess ? "" : "?")
                            Accessible.description: qsTr("What %1 holds; press to change").arg(row.modelData.name)
                            KeyNavigation.left: tick
                            KeyNavigation.right: openButton
                            onClicked: browser.cycle(row.modelData)
                        }
                        FocusButton {
                            id: openButton
                            objectName: "open" + row.index
                            glyph: "›"
                            tip: qsTr("Open %1").arg(row.modelData.name)
                            KeyNavigation.left: chip.visible ? chip : tick
                            Keys.onRightPressed: (chosenList.count > 0 ? chosenList : addButton).forceActiveFocus()
                            onClicked: browser.browse(row.modelData.path)
                        }
                    }
                }
            }

            Rectangle {
                Layout.preferredWidth: 320
                Layout.fillHeight: true
                radius: Theme.radius
                color: Theme.surface
                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 16
                    spacing: 10
                    Label {
                        text: qsTr("To add")
                        color: Theme.text
                        font.pixelSize: Theme.fontLarge
                        font.bold: true
                    }
                    Label {
                        visible: browser.chosen.length === 0
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: qsTr("Tick the folders to add. Each becomes a library folder of this share.")
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                    }
                    ListView {
                        id: chosenList
                        objectName: "chosenList"
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: browser.chosen
                        keyNavigationEnabled: true
                        Keys.onLeftPressed: folderList.forceActiveFocus()
                        Keys.onDownPressed: (e) => { if (currentIndex === count - 1) addButton.forceActiveFocus(); else e.accepted = false }
                        delegate: FocusScope {
                            id: chosenRow
                            required property var modelData
                            required property int index
                            width: chosenList.width
                            height: 48
                            RowLayout {
                                anchors.fill: parent
                                spacing: 8
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 0
                                    Label { text: chosenRow.modelData.name; color: Theme.text; font.pixelSize: Theme.fontBody; elide: Text.ElideRight; Layout.fillWidth: true }
                                    Label { text: browser.contentLabel(chosenRow.modelData.content); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                                }
                                FocusButton {
                                    objectName: "unchoose" + chosenRow.index
                                    focus: true
                                    glyph: "×"
                                    tip: qsTr("Don't add %1").arg(chosenRow.modelData.name)
                                    onClicked: browser.unchoose(chosenRow.index)
                                }
                            }
                        }
                    }
                    RowLayout {
                        spacing: 10
                        FocusButton {
                            id: addButton
                            objectName: "addChosen"
                            primary: true
                            enabled: browser.chosen.length > 0 && !browser.busy
                            text: browser.chosen.length === 0 ? qsTr("Add folders") : qsTr("Add %n folder(s)", "", browser.chosen.length)
                            KeyNavigation.right: cancelButton
                            Keys.onUpPressed: (chosenList.count > 0 ? chosenList : upButton).forceActiveFocus()
                            Keys.onLeftPressed: folderList.forceActiveFocus()
                            onClicked: browser.addAll()
                        }
                        FocusButton {
                            id: cancelButton
                            objectName: "cancel"
                            text: qsTr("Cancel")
                            KeyNavigation.left: addButton
                            Keys.onUpPressed: upButton.forceActiveFocus()
                            onClicked: browser.cancelled()
                        }
                    }
                }
            }
        }
    }
}
