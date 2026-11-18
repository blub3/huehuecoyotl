pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// "Add to playlist" (issue #81), shared by every music menu: the user's playlists and "New
// playlist…" with a name. `show(items, version, from)` opens it for albums, artists or songs
// (an album's `version`, 0 for the preferred one); focus goes back to `from`.
Popup {
    id: chooser
    property var items: []
    property var version: 0
    property Item from: null
    property var mine: []
    property var listRequest: 0
    property var addRequest: 0
    property bool naming: false

    // A popup has no window of its own: the overlay it sits on has.
    function tell(text) {
        const w = chooser.parent ? chooser.parent.Window.window as Main : null
        if (w) w.showError(text)
    }

    function show(list, ver, item) {
        items = list
        version = ver || 0
        from = item
        naming = false
        nameField.text = ""
        listRequest = Core.playlists()
        open()
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(460, (parent ? parent.width : 800) - 80)
    height: Math.min(contentItem.implicitHeight + 48, (parent ? parent.height : 600) - 80)
    modal: true
    padding: 24
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onOpened: newButton.forceActiveFocus()
    onClosed: if (from) from.forceActiveFocus()

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === chooser.listRequest) chooser.mine = detail.mine || []
            if (requestId === chooser.addRequest) {
                chooser.addRequest = 0
                chooser.close()
                chooser.tell(qsTr("Added to “%1”.").arg(detail.card.name))
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId !== chooser.addRequest) return
            chooser.addRequest = 0
            chooser.tell(message)
        }
    }

    contentItem: ColumnLayout {
        spacing: 12
        Label {
            text: qsTr("Add to playlist")
            color: Theme.text
            font.pixelSize: Theme.fontLarge
            font.bold: true
            Accessible.role: Accessible.Heading
        }
        FocusButton {
            id: newButton
            visible: !chooser.naming
            Layout.fillWidth: true
            glyph: "+"
            text: qsTr("New playlist…")
            KeyNavigation.down: list.count > 0 ? list : cancel
            onClicked: { chooser.naming = true; nameField.forceActiveFocus() }
        }
        RowLayout {
            visible: chooser.naming
            Layout.fillWidth: true
            spacing: 8
            TextField {
                id: nameField
                Layout.fillWidth: true
                placeholderText: qsTr("Name of the playlist")
                Accessible.name: qsTr("Name of the playlist")
                KeyNavigation.right: createButton
                onAccepted: createButton.clicked()
            }
            FocusButton {
                id: createButton
                primary: true
                text: qsTr("Create")
                KeyNavigation.left: nameField
                onClicked: chooser.addRequest = Core.playlistCreate(nameField.text, chooser.items, chooser.version)
            }
        }
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight, 300)
            clip: true
            model: chooser.mine
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Your playlists")
            Keys.onUpPressed: (e) => { if (currentIndex === 0) newButton.forceActiveFocus(); else e.accepted = false }
            Keys.onDownPressed: (e) => { if (currentIndex === count - 1) cancel.forceActiveFocus(); else e.accepted = false }
            function choose() {
                const p = chooser.mine[currentIndex]
                if (p) chooser.addRequest = Core.playlistAdd(p.id, chooser.items, chooser.version)
            }
            Keys.onReturnPressed: choose()
            Keys.onEnterPressed: choose()
            delegate: ItemDelegate {
                id: row
                required property var modelData
                required property int index
                width: list.width
                height: 44
                text: modelData.name + "  ·  " + qsTr("%n song(s)", "", modelData.count)
                readonly property bool focused: list.activeFocus && list.currentIndex === index
                background: Rectangle {
                    radius: 6
                    color: row.focused || row.hovered ? Theme.surfaceHi : "transparent"
                    border.width: row.focused ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                contentItem: Label { text: row.text; color: Theme.text; elide: Text.ElideRight; verticalAlignment: Text.AlignVCenter }
                onClicked: { list.currentIndex = index; list.choose() }
            }
        }
        FocusButton {
            id: cancel
            Layout.alignment: Qt.AlignRight
            text: qsTr("Cancel")
            KeyNavigation.up: list.count > 0 ? list : newButton
            onClicked: chooser.close()
        }
    }
}
