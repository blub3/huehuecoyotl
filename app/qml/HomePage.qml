pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

FocusScope {
    id: page
    readonly property bool isPlayer: false

    ContinueWatchingModel { id: recent }
    FolderModel { id: places; path: "" }

    function openFolder(path) {
        StackView.view.push(Qt.resolvedUrl("BrowserPage.qml"), { startPath: path })
    }

    // Keyboard and remote users must always have a focused item on this page.
    function ensureFocus() {
        if (StackView.status !== StackView.Active) return
        if (recentList.activeFocus && recent.count > 0) return
        if (placesList.activeFocus && places.count > 0) return
        if (recent.count > 0) recentList.forceActiveFocus()
        else placesList.forceActiveFocus()
    }

    StackView.onActivated: { recent.refresh(); ensureFocus() }
    Connections { target: recent; function onLoadingChanged() { if (!recent.loading) page.ensureFocus() } }
    Connections { target: places; function onLoadingChanged() { if (!places.loading) page.ensureFocus() } }

    Flickable {
        anchors.fill: parent
        contentHeight: column.implicitHeight + 64
        clip: true

        ColumnLayout {
            id: column
            x: 48
            y: 40
            width: parent.width - 96
            spacing: 18

            RowLayout {
                spacing: 14
                Rectangle {
                    Layout.preferredWidth: 44; Layout.preferredHeight: 44; radius: 22
                    color: Theme.accent
                    Label { anchors.centerIn: parent; text: "H"; color: Theme.bg; font.pixelSize: 24; font.bold: true }
                }
                Label {
                    text: "Huehuecoyotl"
                    color: Theme.text
                    font.pixelSize: Theme.fontTitle
                    font.bold: true
                    Accessible.role: Accessible.Heading
                }
            }

            Label {
                visible: recent.count > 0
                text: qsTr("Continue watching")
                color: Theme.textDim
                font.pixelSize: Theme.fontLarge
                Layout.topMargin: 12
            }
            ListView {
                id: recentList
                visible: recent.count > 0
                Layout.fillWidth: true
                Layout.preferredHeight: 190
                orientation: ListView.Horizontal
                spacing: 18
                clip: false
                model: recent
                focus: recent.count > 0
                keyNavigationEnabled: true
                highlightFollowsCurrentItem: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Continue watching")
                KeyNavigation.down: placesList
                delegate: PosterTile {
                    id: card
                    required property var model
                    focus: ListView.isCurrentItem
                    y: 10
                    title: model.title
                    progress: model.durationMs > 0 ? model.positionMs / model.durationMs : 0
                    subtitle: qsTr("%1 left").arg(Theme.time((model.durationMs - model.positionMs) / 1000))
                    onClicked: Playback.openItem(card.model.itemId)
                    Keys.onReturnPressed: Playback.openItem(card.model.itemId)
                    Keys.onEnterPressed: Playback.openItem(card.model.itemId)
                    Keys.onDeletePressed: Core.setWatched(card.model.itemId, true)
                }
            }

            Label {
                text: qsTr("Browse")
                color: Theme.textDim
                font.pixelSize: Theme.fontLarge
                Layout.topMargin: 12
            }
            ListView {
                id: placesList
                Layout.fillWidth: true
                Layout.preferredHeight: contentHeight
                interactive: false
                spacing: 6
                model: places
                focus: recent.count === 0
                keyNavigationEnabled: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Places")
                KeyNavigation.up: recent.count > 0 ? recentList : null
                delegate: ItemDelegate {
                    id: place
                    required property int index
                    required property string name
                    required property string path
                    width: ListView.view.width
                    height: 56
                    focus: ListView.isCurrentItem
                    focusPolicy: Qt.StrongFocus
                    Accessible.name: place.name
                    onClicked: page.openFolder(place.path)
                    Keys.onReturnPressed: page.openFolder(place.path)
                    Keys.onEnterPressed: page.openFolder(place.path)
                    Keys.onRightPressed: page.openFolder(place.path)
                    background: Rectangle {
                        radius: Theme.radius
                        color: place.activeFocus || place.hovered ? Theme.surfaceHi : Theme.surface
                        border.width: place.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        spacing: 14
                        Rectangle { Layout.preferredWidth: 26; Layout.preferredHeight: 20; radius: 4; color: Theme.accent; opacity: 0.85 }
                        Label { text: place.name; color: Theme.text; font.pixelSize: Theme.fontBody; Layout.fillWidth: true; elide: Text.ElideRight }
                        Label { text: place.path; color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle; Layout.maximumWidth: 420 }
                    }
                }
            }

            Label {
                Layout.topMargin: 24
                text: qsTr("Arrow keys to move · Enter to open · Esc to go back · F11 fullscreen")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
            }
        }
    }
}
