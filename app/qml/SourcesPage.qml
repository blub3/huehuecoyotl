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
        for (const c of [addButton, addShare, rescanAll, folderList, placesList]) if (c.activeFocus) return
        (folders.count > 0 ? folderList : addButton).forceActiveFocus()
    }
    onActiveFocusChanged: if (activeFocus) Qt.callLater(focusFirst)
    Connections { target: folders; function onCountChanged() { Qt.callLater(page.focusFirst) } }

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
                    id: addButton
                    glyph: "+"
                    text: qsTr("Add folder")
                    primary: folders.count === 0
                    KeyNavigation.right: addShare
                    KeyNavigation.down: folders.count > 0 ? folderList : placesList
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: page.pickFolder()
                }
                FocusButton {
                    id: addShare
                    glyph: "+"
                    text: qsTr("Add network share")
                    KeyNavigation.left: addButton
                    KeyNavigation.right: rescanAll
                    KeyNavigation.down: folders.count > 0 ? folderList : placesList
                    onClicked: Nav.open(Qt.resolvedUrl("NetworkSharePage.qml"), {})
                }
                FocusButton {
                    id: rescanAll
                    visible: folders.count > 0
                    text: Core.scanning ? qsTr("Scanning…") : qsTr("Rescan all")
                    enabled: !Core.scanning
                    KeyNavigation.left: addShare
                    KeyNavigation.down: folderList
                    onClicked: Core.rescan(0)
                }
            }
            Label {
                visible: folders.count === 0
                Layout.fillWidth: true
                text: qsTr("Add the folders that hold your movies and shows, on this computer or on a network share (NAS). They are scanned, matched with TMDB and shown under Movies and Shows.")
                color: Theme.textDim
                wrapMode: Text.Wrap
            }

            ListView {
                id: folderList
                // Rows arrive after the list took focus: make the first one current then.
                onCountChanged: if (count > 0 && currentIndex < 0) currentIndex = 0
                Layout.fillWidth: true
                Layout.preferredHeight: contentHeight
                interactive: false
                spacing: 6
                model: folders
                keyNavigationEnabled: true
                Keys.onUpPressed: (e) => { if (currentIndex === 0) addButton.forceActiveFocus(); else e.accepted = false }
                Keys.onDownPressed: (e) => { if (currentIndex === count - 1) placesList.forceActiveFocus(); else e.accepted = false }
                Keys.onLeftPressed: Nav.focusRail()
                delegate: FocusScope {
                    id: row
                    required property var model
                    required property int index
                    width: ListView.view.width
                    height: 64
                    focus: ListView.isCurrentItem
                    Rectangle {
                        anchors.fill: parent
                        radius: Theme.radius
                        color: row.activeFocus ? Theme.surfaceHi : Theme.surface
                        border.width: row.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 16; anchors.rightMargin: 10
                        spacing: 12
                        Rectangle {
                            Layout.preferredWidth: Math.max(26, kindBadge.implicitWidth + 8); Layout.preferredHeight: 20; radius: 4
                            color: row.model.online ? Theme.accent : Theme.textDim
                            Text {
                                id: kindBadge
                                anchors.centerIn: parent
                                visible: row.model.kind !== "local"
                                text: ({ smb: "SMB", nfs: "NFS", webdav: "DAV", ftp: "FTP", sftp: "SFTP" })[row.model.kind] ?? ""
                                color: Theme.bg; font.pixelSize: 9; font.bold: true
                            }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Label { text: row.model.name; color: Theme.text; font.pixelSize: Theme.fontBody }
                            Label {
                                Layout.fillWidth: true
                                text: row.model.root + "  ·  " + (row.model.scanning ? qsTr("scanning…")
                                      : !row.model.online ? qsTr("offline")
                                      : row.model.content === "music" ? qsTr("Music · %n file(s)", "", row.model.files)
                                      : row.model.content === "audiobooks" ? qsTr("Audiobooks · %n file(s)", "", row.model.files)
                                      : qsTr("%n video(s)", "", row.model.files))
                                color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle
                            }
                        }
                        FocusButton {
                            id: rescanOne
                            focus: true
                            text: qsTr("Rescan")
                            KeyNavigation.right: removeOne
                            onClicked: Core.rescan(row.model.id)
                        }
                        FocusButton {
                            id: removeOne
                            property bool armed: false
                            text: armed ? qsTr("Really remove?") : qsTr("Remove")
                            KeyNavigation.left: rescanOne
                            onActiveFocusChanged: if (!activeFocus) armed = false
                            onClicked: { if (armed) folders.remove(row.model.id); else armed = true }
                        }
                    }
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
                Keys.onUpPressed: (e) => { if (currentIndex === 0) (folders.count > 0 ? folderList : addButton).forceActiveFocus(); else e.accepted = false }
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
