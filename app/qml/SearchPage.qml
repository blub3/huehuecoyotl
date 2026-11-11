pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

FocusScope {
    id: page
    readonly property bool isPlayer: false

    SearchModel { id: results }
    Timer { id: debounce; interval: 220; onTriggered: results.text = field.text }

    Component.onCompleted: field.forceActiveFocus()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        spacing: 22

        TextField {
            id: field
            Layout.fillWidth: true
            Layout.maximumWidth: 720
            placeholderText: qsTr("Search movies and shows")
            font.pixelSize: Theme.fontLarge
            Accessible.name: qsTr("Search")
            focus: true
            onTextEdited: debounce.restart()
            onAccepted: { results.text = text; if (results.count > 0) grid.forceActiveFocus() }
            Keys.onDownPressed: if (results.count > 0) grid.forceActiveFocus()
            Keys.onLeftPressed: (e) => { if (cursorPosition === 0) Nav.focusRail(); else e.accepted = false }
            background: Rectangle {
                implicitHeight: 52
                radius: Theme.radius
                color: Theme.surface
                border.width: field.activeFocus ? Theme.focusWidth : 1
                border.color: field.activeFocus ? Theme.focus : Theme.surfaceHi
            }
            color: Theme.text
            placeholderTextColor: Theme.textDim
            leftPadding: 18
        }

        Label {
            visible: field.text.length > 0 && !results.loading && results.count === 0
            text: qsTr("Nothing found for “%1”.").arg(field.text)
            color: Theme.textDim
        }

        GridView {
            id: grid
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: results
            cellWidth: 206
            cellHeight: 346
            topMargin: 14
            leftMargin: 10
            keyNavigationEnabled: true
            readonly property int columns: Math.max(1, Math.floor(width / cellWidth))
            Keys.onUpPressed: (e) => { if (currentIndex < columns) field.forceActiveFocus(); else e.accepted = false }
            Keys.onLeftPressed: (e) => { if (currentIndex % columns === 0) Nav.focusRail(); else e.accepted = false }
            delegate: PosterCard {
                id: hit
                required property var model
                required property int index
                title: model.title
                year: model.year
                poster: model.poster
                progress: model.progress
                watched: model.watched
                unwatched: Number(model.unwatched)
                focus: GridView.isCurrentItem
                onClicked: Nav.openItem(hit.model.id)
                Keys.onReturnPressed: Nav.openItem(hit.model.id)
                Keys.onEnterPressed: Nav.openItem(hit.model.id)
            }
        }
    }
}
