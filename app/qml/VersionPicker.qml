pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import "MediaInfo.js" as MediaInfo

// The version to play (issues #34 and #48): a button with the selected version's label that opens
// the list of versions. Versions that cannot be played (their source is offline and no saved copy
// exists) are listed but cannot be chosen. Used on the movie and episode pages and on episode tiles.
FocusButton {
    id: picker
    property var files: []
    property int index: 0
    // Issue #63: other labels (album versions): the button's and the list's, by index.
    property var labels: []
    property var choices: []
    signal chosen(int index)

    readonly property var file: files.length > 0 ? files[Math.min(index, files.length - 1)] : null
    text: (labels.length > 0 ? (labels[Math.min(index, labels.length - 1)] || "")
                              : file ? MediaInfo.versionLabel(file) : "") + "  ▾"
    tip: qsTr("Choose a version")
    font.pixelSize: Theme.fontSmall
    Accessible.name: qsTr("Version: %1").arg(choices.length > 0 ? (choices[Math.min(index, choices.length - 1)] || "")
                                             : file ? MediaInfo.versionChoice(file) : "")
    onClicked: menu.open()
    function close() { menu.close() }

    Popup {
        id: menu
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(720, parent ? parent.width - 80 : 720)
        height: Math.min(480, list.contentHeight + 24)
        modal: true
        padding: 12
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        // After opening, so the list has its rows: start on the selected version.
        onOpened: Qt.callLater(() => { list.currentIndex = picker.index; list.forceActiveFocus() })
        onClosed: picker.forceActiveFocus()
        ListView {
            id: list
            objectName: "versionList"
            anchors.fill: parent
            clip: true
            spacing: 4
            model: picker.files
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Versions")
            delegate: ItemDelegate {
                id: versionRow
                required property var modelData
                required property int index
                width: list.width
                focus: ListView.isCurrentItem
                enabled: MediaInfo.playable(modelData)
                highlighted: index === picker.index
                text: picker.choices.length > index ? picker.choices[index] : MediaInfo.versionChoice(modelData)
                Accessible.name: text
                contentItem: Label {
                    text: versionRow.text
                    color: versionRow.enabled ? Theme.text : Theme.textDim
                    font.bold: versionRow.highlighted
                    elide: Text.ElideRight
                }
                background: Rectangle {
                    color: versionRow.activeFocus ? Theme.surfaceHi : "transparent"
                    radius: 6
                    border.width: versionRow.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                onClicked: { picker.chosen(index); menu.close() }
                Keys.onReturnPressed: clicked()
                Keys.onEnterPressed: clicked()
            }
        }
    }
}
