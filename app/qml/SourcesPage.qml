pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Library folders on this computer and on network shares (scanned into Movies and Shows, or
// Music: each folder says which, 2.0), and plain browsing of this computer.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: qsTr("Sources")

    LibraryFoldersModel { id: folders }
    // A folder just picked, waiting for its content type.
    property string picked: ""
    FolderModel { id: places; path: "" }

    function pickFolder() {
        Nav.open(Qt.resolvedUrl("BrowserPage.qml"), { startPath: "", pickMode: true, onPicked: (p) => {
            page.picked = p
            Qt.callLater(() => contentQuestion.open())
        } })
    }

    // When the page receives focus but none of its controls has it, focus the first control.
    function focusFirst() {
        if (!page.activeFocus) return
        if (page.Window.activeFocusItem && page.Window.activeFocusItem !== page) return
        if (cardRows.count > 0) (cardRows.itemAt(0) as ShareCard).focusFirst(); else addShare.forceActiveFocus()
    }
    onActiveFocusChanged: if (activeFocus) Qt.callLater(focusFirst)

    // Issue #119: the folders grouped by server, from Core.shares(); "This computer" last.
    property var overview: ({ shares: [], local: [] })
    property var request: 0
    readonly property var cards: (overview.shares || []).concat((overview.local || []).length > 0 ? [{ local: true, folders: overview.local }] : [])
    function load() { request = Core.shares() }
    function cardKey(c) { return c.local ? "local" : "share:" + c.id }
    function isOpen(c) { return Appearance.closedSources.split(",").indexOf(cardKey(c)) < 0 }
    function setOpen(c, on) {
        const keys = Appearance.closedSources.split(",").filter(k => k !== "" && k !== cardKey(c))
        if (!on) keys.push(cardKey(c))
        Appearance.closedSources = keys.join(",")
    }
    // Issue #150: the library's row comes first when there is one.
    function focusBelowButtons() {
        if (librarySettings.visible) librarySettings.forceActiveFocus()
        else if (cardRows.count > 0) focusCard(0, false)
        else placesList.forceActiveFocus()
    }
    function focusCard(i, last) {
        const c = cardRows.itemAt(i) as ShareCard
        if (!c) return
        if (last) c.focusLast(); else c.focusFirst()
    }
    Component.onCompleted: load()
    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) { if (kind === "shares") page.overview = detail }
        function onLibraryChanged() { page.load() }
        function onScanFinished() { page.load() }
    }
    // Counts and "scanning…" follow a running scan.
    Timer { interval: 2000; running: page.visible && Core.scanning; repeat: true; triggeredOnStart: true; onTriggered: page.load() }
    Connections { target: folders; function onCountChanged() { page.load() } }

    Flickable {
        anchors.fill: parent
        contentHeight: col.implicitHeight + 64
        clip: true
        ScrollBar.vertical: ScrollBar {}

        ColumnLayout {
            id: col
            x: 32; y: 32
            width: parent.width - 64
            spacing: 16

            Label { text: qsTr("Sources"); color: Theme.text; font.pixelSize: Theme.fontTitle; font.bold: true }

            RowLayout {
                spacing: 12
                Layout.topMargin: 8
                Label { text: qsTr("Library folders"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.fillWidth: true }
                FocusButton {
                    id: addShare
                    objectName: "addShare"
                    glyph: "+"
                    text: qsTr("Add network share")
                    primary: true
                    KeyNavigation.right: addButton
                    Keys.onDownPressed: page.focusBelowButtons()
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Nav.open(Qt.resolvedUrl("NetworkSharePage.qml"), {})
                }
                FocusButton {
                    id: addButton
                    glyph: "+"
                    text: qsTr("Folder on this computer")
                    KeyNavigation.left: addShare
                    KeyNavigation.right: rescanAll
                    Keys.onDownPressed: page.focusBelowButtons()
                    onClicked: page.pickFolder()
                }
                FocusButton {
                    id: rescanAll
                    visible: page.cards.length > 0
                    text: Core.scanning ? qsTr("Scanning…") : qsTr("Rescan all")
                    enabled: !Core.scanning
                    KeyNavigation.left: addButton
                    Keys.onDownPressed: page.focusBelowButtons()
                    onClicked: Core.rescan(0)
                }
            }
            // Issue #150: the managed library, above the folders it was copied from.
            Rectangle {
                visible: !!page.overview.library
                Layout.fillWidth: true
                implicitHeight: libraryRow.implicitHeight + 24
                radius: Theme.radius
                color: Theme.surface
                RowLayout {
                    id: libraryRow
                    anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: 14 }
                    spacing: 14
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Label { text: qsTr("Your library"); color: Theme.text; font.bold: true }
                        Label {
                            objectName: "libraryLine"
                            Layout.fillWidth: true
                            elide: Text.ElideMiddle
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                            text: page.overview.library
                                  ? (page.overview.library.shareName ? page.overview.library.shareName + " › " : "") + page.overview.library.path
                                    + "  ·  " + qsTr("%n copies", "", Number(page.overview.library.copies || 0))
                                  : ""
                        }
                    }
                    FocusButton {
                        id: librarySettings
                        text: qsTr("Settings…")
                        Keys.onUpPressed: addShare.forceActiveFocus()
                        Keys.onDownPressed: cardRows.count > 0 ? page.focusCard(0, false) : placesList.forceActiveFocus()
                        Keys.onLeftPressed: Nav.focusRail()
                        onClicked: Nav.settings("library")
                    }
                }
            }
            Label {
                visible: page.cards.length === 0
                Layout.fillWidth: true
                text: qsTr("Add the folders that hold your movies, shows, music and audiobooks, on this computer or on a network share (NAS). Each server is added once; its folders sit inside it.")
                color: Theme.textDim
                wrapMode: Text.Wrap
            }
            Repeater {
                id: cardRows
                model: page.cards
                ShareCard {
                    id: shareCard
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    info: modelData
                    open: page.isOpen(modelData)
                    onToggled: (on) => { page.setOpen(shareCard.modelData, on); Qt.callLater(() => shareCard.focusFirst()) }
                    canEdit: !shareCard.modelData.local
                    onEditRequested: Nav.open(Qt.resolvedUrl("ShareEditPage.qml"), { share: shareCard.modelData.id })
                    onAddFoldersRequested: Nav.open(Qt.resolvedUrl("ShareFoldersPage.qml"), { share: shareCard.modelData.id, shareName: shareCard.modelData.name })
                    onRescanRequested: (id) => Core.rescan(id)
                    onRemoveRequested: (f) => removeConfirm.ask(f, shareCard)
                    onGoneRequested: (f) => goneList.ask(f, shareCard)
                    onUpRequested: shareCard.index === 0 ? (librarySettings.visible ? librarySettings.forceActiveFocus() : addShare.forceActiveFocus()) : page.focusCard(shareCard.index - 1, true)
                    onDownRequested: shareCard.index === cardRows.count - 1 ? placesList.forceActiveFocus() : page.focusCard(shareCard.index + 1, false)
                    onLeftEdge: Nav.focusRail()
                }
            }

            Label { text: qsTr("Browse this computer"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 18 }
            ListView {
                id: placesList
                Layout.fillWidth: true
                Layout.preferredHeight: contentHeight
                interactive: false
                spacing: 6
                model: places
                keyNavigationEnabled: true
                Keys.onUpPressed: (e) => { if (currentIndex === 0) (cardRows.count > 0 ? page.focusCard(cardRows.count - 1, true) : librarySettings.visible ? librarySettings.forceActiveFocus() : addShare.forceActiveFocus()); else e.accepted = false }
                Keys.onLeftPressed: Nav.focusRail()
                delegate: ItemDelegate {
                    id: place
                    required property string name
                    required property string path
                    width: ListView.view.width
                    height: 52
                    focus: ListView.isCurrentItem
                    focusPolicy: Qt.StrongFocus
                    Accessible.name: name
                    function openIt() { Nav.open(Qt.resolvedUrl("BrowserPage.qml"), { startPath: place.path }) }
                    onClicked: openIt()
                    Keys.onReturnPressed: openIt()
                    Keys.onEnterPressed: openIt()
                    Keys.onRightPressed: openIt()
                    background: Rectangle {
                        radius: Theme.radius
                        color: place.activeFocus || place.hovered ? Theme.surfaceHi : Theme.surface
                        border.width: place.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        spacing: 14
                        Rectangle { Layout.preferredWidth: 26; Layout.preferredHeight: 20; radius: 4; color: Theme.textDim; opacity: 0.6 }
                        Label { text: place.name; color: Theme.text; Layout.fillWidth: true; elide: Text.ElideRight }
                        Label { text: place.path; color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle; Layout.maximumWidth: 420 }
                    }
                }
            }
        }
    }

    // Issue #119: removing a folder asks first and says what goes and what stays.
    Popup {
        id: removeConfirm
        objectName: "removeConfirm"
        property var folder: ({})
        property Item from: null
        // Issue #150: how many items stay because the library has them.
        property var preview: null
        property var previewRequest: 0
        function ask(f, item) {
            folder = f
            from = item
            preview = null
            previewRequest = Core.libraryCall("removePreview", { source: Number(f.sourceId) })
            open()
        }
        Connections {
            target: Core
            function onMusicDetailReady(requestId, kind, detail) {
                if (requestId === removeConfirm.previewRequest && kind === "libraryRemovePreview") removeConfirm.preview = detail
            }
        }
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(580, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: keepFolder.forceActiveFocus()
        onClosed: Qt.callLater(() => { if (cardRows.count > 0) page.focusCard(0, false) })
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                font.bold: true
                text: qsTr("Remove “%1” from the library?").arg(removeConfirm.folder.name || "")
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: qsTr("Its titles leave the library, and saved copies of its files on this computer are deleted. The files on the server or disk stay. If you add the folder again, it is scanned and matched again.")
            }
            Label {
                objectName: "removePreview"
                visible: !!removeConfirm.preview && Number(removeConfirm.preview.stay) > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: removeConfirm.preview
                      ? qsTr("%n item(s) are in your library and stay, with their play counts and playlists.", "", Number(removeConfirm.preview.stay)) + " "
                        + qsTr("%n item(s) are only here and leave the app.", "", Number(removeConfirm.preview.leave))
                      : ""
            }
            RowLayout {
                spacing: 12
                FocusButton { id: keepFolder; text: qsTr("Keep it"); KeyNavigation.right: addThoseFirst.visible ? addThoseFirst : removeFolder; onClicked: removeConfirm.close() }
                // Issue #154: what only this folder has can go into the library first.
                FocusButton {
                    id: addThoseFirst
                    objectName: "addThoseFirst"
                    visible: !!page.overview.library && !!removeConfirm.preview && Number(removeConfirm.preview.leave) > 0
                    text: qsTr("Add those to Library first…")
                    KeyNavigation.left: keepFolder
                    KeyNavigation.right: removeFolder
                    onClicked: {
                        Core.libraryCall("importSource", { source: Number(removeConfirm.folder.sourceId) })
                        removeConfirm.close()
                        Nav.go("downloads")
                    }
                }
                FocusButton {
                    id: removeFolder
                    objectName: "confirmRemove"
                    primary: true
                    text: qsTr("Remove folder")
                    KeyNavigation.left: keepFolder
                    onClicked: { folders.remove(removeConfirm.folder.sourceId); removeConfirm.close() }
                }
            }
        }
    }

    // Issue #150: what left a folder.
    Popup {
        id: goneList
        objectName: "goneList"
        property var items: []
        property string name: ""
        property var request: 0
        property Item from: null
        function ask(f, item) {
            name = f.name
            from = item
            items = []
            request = Core.libraryCall("gone", { source: Number(f.sourceId) })
            open()
        }
        Connections {
            target: Core
            function onMusicDetailReady(requestId, kind, detail) {
                if (requestId === goneList.request && kind === "libraryGone") goneList.items = detail.items
            }
        }
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(620, parent.width - 80)
        height: Math.min(520, parent.height - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: goneClose.forceActiveFocus()
        onClosed: if (from) from.focusFirst()
        ColumnLayout {
            anchors.fill: parent
            spacing: 10
            Label { Layout.fillWidth: true; text: qsTr("No longer in “%1”").arg(goneList.name); color: Theme.text; font.bold: true; elide: Text.ElideRight }
            ListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: goneList.items
                delegate: RowLayout {
                    id: goneRow
                    required property var modelData
                    width: ListView.view.width
                    height: 34
                    Label { Layout.fillWidth: true; text: goneRow.modelData.title; color: Theme.text; elide: Text.ElideRight }
                    Label { text: goneRow.modelData.kept ? qsTr("In your library") : qsTr("Left the app"); color: goneRow.modelData.kept ? Theme.accent : Theme.textDim; font.pixelSize: Theme.fontSmall }
                }
            }
            FocusButton { id: goneClose; Layout.alignment: Qt.AlignRight; text: qsTr("Close"); onClicked: goneList.close() }
        }
    }

    // 2.0: what a new folder holds decides how it is scanned.
    Popup {
        id: contentQuestion
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: videoChoice.forceActiveFocus()
        onClosed: addButton.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("What is in “%1”?").arg(page.picked)
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: qsTr("Video folders become Movies and Shows, music folders Music, audiobook folders Audiobooks. A folder with more than one is added once per subfolder.")
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: videoChoice
                    primary: true
                    text: qsTr("Movies and shows")
                    KeyNavigation.right: musicChoice
                    onClicked: { folders.add(page.picked, "video"); contentQuestion.close() }
                }
                FocusButton {
                    id: musicChoice
                    text: qsTr("Music")
                    KeyNavigation.left: videoChoice
                    KeyNavigation.right: bookChoice
                    onClicked: { folders.add(page.picked, "music"); contentQuestion.close() }
                }
                FocusButton {
                    id: bookChoice
                    text: qsTr("Audiobooks")
                    KeyNavigation.left: musicChoice
                    KeyNavigation.right: cancelChoice
                    onClicked: { folders.add(page.picked, "audiobooks"); contentQuestion.close() }
                }
                FocusButton {
                    id: cancelChoice
                    text: qsTr("Cancel")
                    KeyNavigation.left: bookChoice
                    onClicked: contentQuestion.close()
                }
            }
        }
    }
}
