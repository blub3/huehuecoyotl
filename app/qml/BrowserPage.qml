pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Folder browser over the core's local source. Fully usable with arrows, Enter and Back.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property string startPath
    property string returnTo: ""   // path of the folder we just left, to restore focus

    FolderModel {
        id: folder
        path: page.startPath
        onLoadingChanged: if (!loading) page.restoreFocus()
    }

    function restoreFocus() {
        if (StackView.status !== StackView.Active) return
        const i = returnTo !== "" ? folder.indexOfPath(returnTo) : Math.min(list.currentIndex, folder.count - 1)
        list.currentIndex = Math.max(0, i)
        list.positionViewAtIndex(list.currentIndex, ListView.Center)
        returnTo = ""
        list.forceActiveFocus()
    }
    function goUp() {
        // Back from the place you entered returns to Home, like a remote expects.
        if (folder.parentPath === "" || folder.path === page.startPath) {
            StackView.view.pop()
        } else {
            returnTo = folder.path
            folder.path = folder.parentPath
        }
    }
    function activate(entry) {
        if (entry.isDir) { list.currentIndex = 0; folder.path = entry.path }
        else Playback.openPath(entry.path)
    }

    StackView.onActivated: { folder.refresh(); list.forceActiveFocus() }
    Keys.onEscapePressed: goUp()
    Keys.onBackPressed: goUp()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        spacing: 16

        RowLayout {
            spacing: 14
            FocusButton {
                glyph: "‹"
                tip: qsTr("Back")
                focusPolicy: Qt.NoFocus
                onClicked: page.goUp()
            }
            Label {
                Layout.fillWidth: true
                text: folder.path
                color: Theme.text
                font.pixelSize: Theme.fontLarge
                elide: Text.ElideMiddle
                Accessible.role: Accessible.Heading
            }
            BusyIndicator { running: folder.loading; implicitWidth: 32; implicitHeight: 32 }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 4
            model: folder
            focus: true
            keyNavigationEnabled: true
            highlightMoveDuration: 0
            Accessible.role: Accessible.List
            Accessible.name: folder.path
            Keys.onLeftPressed: page.goUp()
            Keys.onPressed: (e) => { if (e.key === Qt.Key_Backspace) { page.goUp(); e.accepted = true } }
            ScrollBar.vertical: ScrollBar {}

            delegate: ItemDelegate {
                id: row
                required property int index
                required property string name
                required property string path
                required property bool isDir
                required property real progress
                required property bool watched
                width: ListView.view.width - 12
                height: 52
                focus: ListView.isCurrentItem
                focusPolicy: Qt.StrongFocus
                Accessible.name: row.name + (row.watched ? ", " + qsTr("watched") : "")
                onClicked: { list.currentIndex = row.index; page.activate(folder.get(row.index)) }
                Keys.onReturnPressed: page.activate(folder.get(row.index))
                Keys.onEnterPressed: page.activate(folder.get(row.index))
                Keys.onRightPressed: if (row.isDir) page.activate(folder.get(row.index))
                background: Rectangle {
                    radius: Theme.radius
                    color: row.activeFocus ? Theme.surfaceHi : row.hovered ? Theme.surface : "transparent"
                    border.width: row.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                contentItem: RowLayout {
                    spacing: 14
                    Item {
                        Layout.preferredWidth: 26; Layout.preferredHeight: 22
                        Rectangle { visible: row.isDir; anchors.fill: parent; radius: 4; color: Theme.accent; opacity: 0.8 }
                        Label { visible: !row.isDir; anchors.centerIn: parent; text: "▶"; color: Theme.textDim; font.pixelSize: 16 }
                    }
                    Label {
                        Layout.fillWidth: true
                        text: row.name
                        color: row.watched ? Theme.textDim : Theme.text
                        font.pixelSize: Theme.fontBody
                        elide: Text.ElideRight
                    }
                    Rectangle {
                        visible: !row.isDir && row.progress > 0 && !row.watched
                        Layout.preferredWidth: 90; Layout.preferredHeight: 4; radius: 2
                        color: Qt.rgba(1, 1, 1, 0.15)
                        Rectangle { width: parent.width * row.progress; height: 4; radius: 2; color: Theme.accent }
                    }
                    Label { visible: row.watched; text: "✓"; color: Theme.accent; font.pixelSize: 18 }
                    Label { visible: row.isDir; text: "›"; color: Theme.textDim; font.pixelSize: 22 }
                }
            }
        }

        Label {
            visible: !folder.loading && folder.count === 0
            Layout.alignment: Qt.AlignHCenter
            text: folder.errorString !== "" ? folder.errorString : qsTr("No videos or folders here.")
            color: folder.errorString !== "" ? Theme.danger : Theme.textDim
        }
    }
}
