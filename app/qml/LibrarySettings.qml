pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// Issue #150: Settings → Library. Where the library lives (this computer or a share), then per
// media type its import mode and options. `backend` is the core or a test stub:
// libraryCall(action, args), answered with musicDetailReady ("library", "libraryCheck",
// "libraryFolder"), shareBrowse(target, "", path) ("shareFolders"), or requestFailed.
// The owner opens the desktop's folder picker on `pickLocal` and hands the result to
// `checkLocal`. Up from the first control emits `leaveUp`, Left `leaveLeft`.
ColumnLayout {
    id: root
    property var backend: null
    signal pickLocal()
    signal addShare()
    signal leaveUp()
    signal leaveLeft()

    property var status: ({})
    property string kind: "music"
    property string where_: "local"
    property var chosenShare: null
    property string error: ""
    property var check: null
    property var target: null
    property var pending: ({ status: 0, check: 0, apply: 0, set: 0, browse: 0, create: 0 })
    readonly property var type: status.types ? (status.types[kind] || {}) : {}
    // Issue #156: the user's folders that hold this type.
    readonly property var watchable: (status.watchable || []).filter(f =>
        f.content === (kind === "music" ? "music" : kind === "audiobooks" ? "audiobooks" : "video"))
    readonly property var kinds: [
        { key: "movies", label: qsTr("Movies") }, { key: "shows", label: qsTr("TV Shows") },
        { key: "music", label: qsTr("Music") }, { key: "audiobooks", label: qsTr("Audiobooks") }
    ]

    spacing: 14

    function load() { if (backend) pending.status = backend.libraryCall("status", {}) }
    function run(t) {
        target = t
        error = ""
        check = null
        pending.check = backend.libraryCall("check", t)
    }
    function checkLocal(path) { run({ where: "local", path: path }) }
    function apply(mode) {
        const args = Object.assign({}, target)
        args.mode = mode
        pending.apply = backend.libraryCall("apply", args)
    }
    function setType(changes) {
        pending.set = backend.libraryCall("setType", { type: kind, changes: changes })
    }
    // Issue #152: built by the core with the real naming rules.
    function example(k) { return status.examples ? (status.examples[k] || "") : "" }
    function shareState(s) {
        return s.state === "offline" ? qsTr("Offline") : qsTr("Reachable")
    }
    function browse(path) {
        pending.browse = backend.shareBrowse({ share: chosenShare.id }, "", path)
    }

    // Arrow keys walk the controls of this tab in order.
    function step(forward, leave) {
        const now = root.Window.activeFocusItem
        if (!now) return
        const next = now.nextItemInFocusChain(forward)
        let p = next
        while (p && p !== root) p = p.parent
        if (p === root) next.forceActiveFocus()
        else leave()
    }
    Keys.onUpPressed: step(false, root.leaveUp)
    Keys.onDownPressed: step(true, () => {})
    Keys.onLeftPressed: step(false, root.leaveLeft)
    Keys.onRightPressed: step(true, () => {})
    function focusFirst() { hereButton.forceActiveFocus() }

    Component.onCompleted: load()
    Timer { interval: 1500; repeat: true; running: root.visible && root.status.moving === true; onTriggered: root.load() }

    Connections {
        target: root.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (kind === "library" && (requestId === root.pending.status || requestId === root.pending.apply || requestId === root.pending.set)) {
                root.status = detail
                if (requestId === root.pending.status && detail.configured) {
                    root.where_ = detail.where
                    root.chosenShare = (detail.shares || []).find(s => s.id === detail.share) || null
                }
                if (requestId === root.pending.apply) placeDialog.close()
            } else if (requestId === root.pending.check && kind === "libraryCheck") {
                root.check = detail
                placeDialog.open()
            } else if (requestId === root.pending.browse && kind === "shareFolders") {
                folderPicker.at = detail
                if (!folderPicker.opened) folderPicker.open()
            } else if (requestId === root.pending.create && kind === "libraryFolder") {
                newFolder.text = ""
                root.browse(detail.path)
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (Object.values(root.pending).indexOf(requestId) >= 0) {
                root.error = message
                placeDialog.close()
            }
        }
    }

    component Choice: Button {
        id: choice
        property string title: ""
        property string body: ""
        property string note: ""
        property bool selected: false
        Layout.fillWidth: true
        Layout.preferredWidth: 1
        focusPolicy: Qt.StrongFocus
        Accessible.role: Accessible.RadioButton
        Accessible.checked: selected
        Accessible.name: title
        Accessible.description: body
        Keys.onReturnPressed: clicked()
        Keys.onEnterPressed: clicked()
        padding: 14
        background: Rectangle {
            radius: 12
            color: choice.selected ? Theme.surfaceHi : Theme.surface
            border.width: choice.activeFocus ? Theme.focusWidth : choice.selected ? 1 : 0
            border.color: Theme.focus
        }
        contentItem: ColumnLayout {
            spacing: 4
            Label { Layout.fillWidth: true; text: choice.title; color: Theme.text; font.bold: true; wrapMode: Text.Wrap }
            Label { Layout.fillWidth: true; text: choice.body; color: Theme.textDim; font.pixelSize: Theme.fontSmall; wrapMode: Text.Wrap }
            Label { Layout.fillWidth: true; visible: choice.note !== ""; text: choice.note; color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle }
        }
    }
    component Tick: CheckBox {
        id: tick
        Layout.fillWidth: true
        focusPolicy: Qt.StrongFocus
        Keys.onReturnPressed: toggle(), toggled()
        Keys.onEnterPressed: toggle(), toggled()
        contentItem: Label { leftPadding: tick.indicator.width + 8; text: tick.text; color: tick.enabled ? Theme.text : Theme.textDim; wrapMode: Text.Wrap; verticalAlignment: Text.AlignVCenter }
        background: Rectangle { radius: 8; color: "transparent"; border.width: tick.activeFocus ? Theme.focusWidth : 0; border.color: Theme.focus }
    }

    Label { text: qsTr("Where your library lives"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; Accessible.role: Accessible.Heading }
    Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: Theme.textDim
        text: qsTr("Your library holds your own copies, named and tagged. Huehuecoyotl writes only in this folder; your other folders and shares stay as they are.")
    }
    Label {
        objectName: "libraryNow"
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: Theme.text
        text: !root.status.configured ? qsTr("No library folder yet.")
              : [(root.status.shareName ? root.status.shareName + " › " : "") + root.status.path,
                 qsTr("%n copies", "", Number(root.status.copies || 0)) + " (" + MediaInfo.size(Number(root.status.size || 0)) + ")",
                 root.status.free !== null && root.status.free !== undefined ? qsTr("%1 free").arg(MediaInfo.size(Number(root.status.free))) : ""]
                .filter(x => x !== "").join("  ·  ")
    }
    Label {
        objectName: "moveState"
        visible: root.status.moving === true
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: root.status.moveError ? Theme.danger : Theme.accent
        text: root.status.moveError
              ? qsTr("The move is paused and continues when the place is back: %1").arg(root.status.moveError)
              : qsTr("Moving your library: %n copies to go. They keep playing.", "", Number(root.status.moveLeft || 0))
    }
    RowLayout {
        Layout.fillWidth: true
        spacing: 14
        Choice {
            id: hereButton
            objectName: "whereLocal"
            Layout.fillHeight: true
            title: qsTr("This computer")
            body: qsTr("Fast and always there. Other devices see it only if you share the folder yourself.")
            selected: root.where_ === "local"
            onClicked: { root.where_ = "local"; root.pickLocal() }
        }
        Choice {
            objectName: "whereShare"
            Layout.fillHeight: true
            title: qsTr("A network share")
            body: qsTr("One library on your NAS. Needs a share you may write to.")
            selected: root.where_ === "share"
            onClicked: root.where_ = "share"
        }
    }
    ColumnLayout {
        visible: root.where_ === "share"
        Layout.fillWidth: true
        spacing: 8
        Label { text: qsTr("Your shares"); color: Theme.textDim }
        Label {
            visible: (root.status.shares || []).length === 0
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textDim
            text: qsTr("No network share has been added yet.")
        }
        Repeater {
            model: root.status.shares || []
            FocusButton {
                id: shareRow
                required property var modelData
                required property int index
                objectName: "share" + index
                Layout.fillWidth: true
                enabled: modelData.state !== "offline"
                text: modelData.name + "  ·  " + root.shareState(modelData)
                primary: !!root.chosenShare && root.chosenShare.id === modelData.id
                onClicked: { root.chosenShare = modelData; root.browse("/") }
            }
        }
        FocusButton { objectName: "addShare"; text: qsTr("Add network share…"); onClicked: root.addShare() }
    }
    Label {
        objectName: "libraryError"
        visible: root.error !== ""
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: Theme.danger
        text: root.error
    }

    Label { text: qsTr("What goes in"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; Layout.topMargin: 10; Accessible.role: Accessible.Heading }
    RowLayout {
        spacing: 6
        Repeater {
            model: root.kinds
            FocusButton {
                required property var modelData
                required property int index
                objectName: "kind_" + modelData.key
                text: modelData.label
                primary: root.kind === modelData.key
                onClicked: root.kind = modelData.key
            }
        }
    }
    Tick {
        objectName: "typeEnabled"
        text: qsTr("Use the library for this type")
        checked: root.type.enabled !== false
        onToggled: root.setType({ enabled: checked })
    }
    RowLayout {
        Layout.fillWidth: true
        spacing: 14
        enabled: root.type.enabled !== false
        Choice {
            objectName: "importManual"
            Layout.fillHeight: true
            title: qsTr("Manual import")
            body: qsTr("Everything plays from where it is. Add to Library copies what you choose, named and tagged.")
            selected: root.type["import"] !== "auto"
            onClicked: root.setType({ "import": "manual" })
        }
        Choice {
            objectName: "importAuto"
            Layout.fillHeight: true
            title: qsTr("Automatically import")
            body: qsTr("New items in the folders you watch are copied here once they are matched. Unsure ones wait for you.")
            selected: root.type["import"] === "auto"
            onClicked: root.setType({ "import": "auto" })
        }
    }
    // Issue #156: the folders this type watches.
    ColumnLayout {
        objectName: "watchBox"
        visible: root.type["import"] === "auto" && root.type.enabled !== false
        Layout.fillWidth: true
        spacing: 2
        Label { text: qsTr("Watch these folders"); color: Theme.text; font.bold: true }
        Label {
            visible: root.watchable.length === 0
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textDim
            text: qsTr("No folder of this type yet. Add one under Sources.")
        }
        Repeater {
            model: root.watchable
            Tick {
                required property var modelData
                required property int index
                objectName: "watch" + index
                text: modelData.name + "  ·  " + (modelData.root || "/")
                checked: (root.type.watch || []).indexOf(modelData.id) >= 0
                onToggled: {
                    const now = (root.type.watch || []).filter(x => x !== modelData.id)
                    if (checked) now.push(modelData.id)
                    root.setType({ watch: now })
                }
            }
        }
        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
            text: qsTr("What is in a folder when you start watching it stays where it is. Only what is new from then on is imported.")
        }
    }
    Tick {
        objectName: "writeTags"
        enabled: root.type.enabled !== false
        text: qsTr("Write the matched tags and IDs into the copies")
        checked: root.type.writeTags !== false
        onToggled: root.setType({ writeTags: checked })
    }
    Tick {
        objectName: "keepOriginals"
        enabled: false
        checked: true
        text: qsTr("Keep the original files where they are")
    }
    Label { text: qsTr("A copy will be named like"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
    Label {
        objectName: "namePreview"
        Layout.fillWidth: true
        text: root.example(root.kind)
        color: Theme.accent
        font.family: "monospace"
        font.pixelSize: Theme.fontSmall
        elide: Text.ElideMiddle
    }

    // ---- a folder on the share ----------------------------------------------------------------
    Popup {
        id: folderPicker
        objectName: "folderPicker"
        property var at: ({ path: "/", parent: "", folders: [] })
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(620, (parent ? parent.width : 800) - 80)
        height: Math.min(560, (parent ? parent.height : 700) - 80)
        modal: true
        focus: true
        padding: 22
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: useFolder.forceActiveFocus()
        contentItem: ColumnLayout {
            spacing: 10
            Label { Layout.fillWidth: true; text: qsTr("Choose a folder on “%1”").arg(root.chosenShare ? root.chosenShare.name : ""); color: Theme.text; font.bold: true; elide: Text.ElideRight }
            Label { Layout.fillWidth: true; text: folderPicker.at.path; color: Theme.textDim; font.family: "monospace"; elide: Text.ElideMiddle }
            ListView {
                id: pickList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                keyNavigationEnabled: true
                model: folderPicker.at.folders || []
                Keys.onUpPressed: (e) => { if (currentIndex <= 0) useFolder.forceActiveFocus(); else e.accepted = false }
                Keys.onDownPressed: (e) => { if (currentIndex >= count - 1) newFolder.forceActiveFocus(); else e.accepted = false }
                delegate: ItemDelegate {
                    id: pickRow
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    height: 42
                    focus: ListView.isCurrentItem
                    text: modelData.name
                    Keys.onReturnPressed: clicked()
                    Keys.onEnterPressed: clicked()
                    onClicked: root.browse(modelData.path)
                    contentItem: Label { text: pickRow.modelData.name; color: Theme.text; elide: Text.ElideRight; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { radius: 8; color: pickRow.activeFocus || pickRow.hovered ? Theme.surfaceHi : "transparent"; border.width: pickRow.activeFocus ? Theme.focusWidth : 0; border.color: Theme.focus }
                }
            }
            RowLayout {
                spacing: 8
                TextField {
                    id: newFolder
                    objectName: "newFolder"
                    Layout.fillWidth: true
                    placeholderText: qsTr("New folder here")
                    Accessible.name: qsTr("New folder here")
                    Keys.onUpPressed: pickList.count > 0 ? pickList.forceActiveFocus() : useFolder.forceActiveFocus()
                    Keys.onDownPressed: useFolder.forceActiveFocus()
                    onAccepted: createFolder.clicked()
                }
                FocusButton {
                    id: createFolder
                    text: qsTr("Create")
                    enabled: newFolder.text.trim() !== ""
                    onClicked: root.pending.create = root.backend.libraryCall("createFolder", { share: root.chosenShare.id, path: folderPicker.at.path, name: newFolder.text.trim() })
                }
            }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 10
                FocusButton { text: qsTr("Up"); enabled: (folderPicker.at.parent || "") !== ""; onClicked: root.browse(folderPicker.at.parent) }
                FocusButton { text: qsTr("Cancel"); onClicked: folderPicker.close() }
                FocusButton {
                    id: useFolder
                    objectName: "useFolder"
                    primary: true
                    text: qsTr("Use this folder")
                    Keys.onDownPressed: pickList.count > 0 ? pickList.forceActiveFocus() : newFolder.forceActiveFocus()
                    onClicked: { folderPicker.close(); root.run({ where: "share", share: root.chosenShare.id, path: folderPicker.at.path }) }
                }
            }
        }
    }

    // ---- the checks, then move or start empty --------------------------------------------------
    Popup {
        id: placeDialog
        objectName: "placeDialog"
        property string mode: "move"
        readonly property bool hasCopies: !!root.check && Number(root.check.copies || 0) > 0 && !!root.check.current
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(760, (parent ? parent.width : 800) - 60)
        modal: true
        focus: true
        padding: 26
        closePolicy: Popup.CloseOnEscape
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onAboutToShow: mode = "move"
        onOpened: (root.check && root.check.ok ? confirm : cancel).forceActiveFocus()
        contentItem: ColumnLayout {
            spacing: 12
            Label { Layout.fillWidth: true; text: qsTr("Use this folder for your library?"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; wrapMode: Text.Wrap; Accessible.role: Accessible.Heading }
            Label { Layout.fillWidth: true; text: root.check ? root.check.path : ""; color: Theme.textDim; font.family: "monospace"; elide: Text.ElideMiddle }
            Repeater {
                model: root.check ? root.check.checks : []
                RowLayout {
                    id: line
                    required property var modelData
                    required property int index
                    objectName: "check_" + modelData.key
                    Layout.fillWidth: true
                    spacing: 10
                    Label { text: line.modelData.ok ? "✓" : "✕"; color: line.modelData.ok ? Theme.accent : Theme.danger; font.bold: true }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        color: line.modelData.ok ? Theme.text : Theme.danger
                        text: placeDialog.describe(line.modelData)
                    }
                }
            }
            ColumnLayout {
                visible: placeDialog.hasCopies && !!root.check && root.check.ok
                Layout.fillWidth: true
                spacing: 8
                Label { Layout.fillWidth: true; color: Theme.text; font.bold: true; wrapMode: Text.Wrap; text: root.check ? qsTr("Your library has %n copies (%1) in %2", "", Number(root.check.copies || 0)).arg(MediaInfo.size(Number(root.check.needed || 0))).arg(root.check.current || "") : "" }
                Choice {
                    id: moveChoice
                    objectName: "modeMove"
                    Layout.preferredWidth: -1
                    title: qsTr("Move them there (recommended)")
                    body: qsTr("One by one: each copy is written there, checked, and only then removed here. They keep playing during the move. If the place goes away, the move pauses and continues later.")
                    selected: placeDialog.mode === "move"
                    KeyNavigation.down: emptyChoice
                    onClicked: placeDialog.mode = "move"
                }
                Choice {
                    id: emptyChoice
                    objectName: "modeEmpty"
                    Layout.preferredWidth: -1
                    title: qsTr("Start empty there")
                    body: qsTr("New imports go there. The old copies stay where they are and become a normal folder in Sources, so nothing disappears.")
                    selected: placeDialog.mode === "empty"
                    KeyNavigation.up: moveChoice
                    KeyNavigation.down: confirm
                    onClicked: placeDialog.mode = "empty"
                }
            }
            Label { Layout.fillWidth: true; wrapMode: Text.Wrap; color: Theme.textDim; font.pixelSize: Theme.fontSmall; text: qsTr("Nothing is deleted without a finished, checked copy.") }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12
                FocusButton { id: cancel; objectName: "cancelPlace"; text: qsTr("Cancel"); KeyNavigation.right: confirm; onClicked: placeDialog.close() }
                FocusButton {
                    id: confirm
                    objectName: "confirmPlace"
                    primary: true
                    enabled: !!root.check && root.check.ok
                    text: placeDialog.hasCopies && placeDialog.mode === "move" ? qsTr("Move library") : qsTr("Use this folder")
                    KeyNavigation.left: cancel
                    Keys.onUpPressed: placeDialog.hasCopies ? emptyChoice.forceActiveFocus() : cancel.forceActiveFocus()
                    onClicked: root.apply(placeDialog.mode)
                }
            }
        }
        function describe(c) {
            switch (c.key) {
            case "reachable":
                return c.ok ? qsTr("Reachable now")
                     : c.detail === "notFound" ? qsTr("This folder does not exist.")
                     : c.detail === "notFolder" ? qsTr("This is not a folder.")
                     : qsTr("Cannot be reached: %1").arg(c.detail)
            case "writable":
                return c.ok ? qsTr("Folders can be created, files written, renamed and removed")
                     : c.detail === "readOnly" ? qsTr("Read-only: the server does not allow changes here.")
                     : c.detail === "notChecked" ? qsTr("Writing could not be tested.")
                     : qsTr("Writing failed: %1").arg(c.detail)
            case "space":
                return !c.ok ? qsTr("Not enough free space for your copies (%1 needed).").arg(MediaInfo.size(Number(root.check.needed || 0)))
                     : c.detail === "unknown" ? qsTr("This server does not report its free space.")
                     : qsTr("%1 free, enough for your library").arg(MediaInfo.size(Number(root.check.free || 0)))
            default:
                return c.ok ? qsTr("No other library is in this folder")
                     : c.detail === "current" ? qsTr("This already is your library folder.")
                     : qsTr("This folder overlaps “%1”, which is already in your sources.").arg(c.detail)
            }
        }
    }
}
