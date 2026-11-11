pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Search TMDB for the right match and pin it (the item becomes "locked": rescans never change it).
Popup {
    id: dialog
    property var itemId
    property var request: 0
    property var candidates: []
    property bool searching: false

    function openFor(id, title, year) {
        itemId = id
        titleField.text = title || ""
        yearField.text = year > 0 ? String(year) : ""
        candidates = []
        open()
        search()
    }
    function search() {
        searching = true
        request = Core.findMatches(itemId, titleField.text, parseInt(yearField.text) || 0)
    }

    modal: true
    focus: true
    width: Math.min(parent ? parent.width - 80 : 800, 820)
    height: Math.min(parent ? parent.height - 80 : 700, 680)
    padding: 24
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    onOpened: titleField.forceActiveFocus()

    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }

    Connections {
        target: Core
        function onCandidatesReady(requestId, item, list) {
            if (requestId !== dialog.request) return
            dialog.searching = false
            dialog.candidates = list
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId !== dialog.request) return
            dialog.searching = false
            status.text = message
        }
    }

    contentItem: ColumnLayout {
        spacing: 14
        Label { text: qsTr("Fix match"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true }
        RowLayout {
            spacing: 10
            TextField {
                id: titleField
                Layout.fillWidth: true
                placeholderText: qsTr("Title")
                font.pixelSize: Theme.fontBody
                Accessible.name: qsTr("Title")
                KeyNavigation.right: yearField
                KeyNavigation.down: results
                onAccepted: dialog.search()
            }
            TextField {
                id: yearField
                Layout.preferredWidth: 90
                placeholderText: qsTr("Year")
                inputMethodHints: Qt.ImhDigitsOnly
                validator: IntValidator { bottom: 0; top: 2100 }
                Accessible.name: qsTr("Year")
                KeyNavigation.right: searchButton
                KeyNavigation.down: results
                onAccepted: dialog.search()
            }
            FocusButton { id: searchButton; text: qsTr("Search"); KeyNavigation.down: results; onClicked: dialog.search() }
        }
        Label { id: status; visible: text !== ""; color: Theme.danger; text: "" }
        BusyIndicator { visible: dialog.searching; running: visible; Layout.alignment: Qt.AlignHCenter }
        Label {
            visible: !dialog.searching && dialog.candidates.length === 0 && status.text === ""
            text: qsTr("No results. Try another title or remove the year.")
            color: Theme.textDim
        }
        ListView {
            id: results
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 8
            model: dialog.candidates
            keyNavigationEnabled: true
            Keys.onUpPressed: (e) => { if (currentIndex === 0) titleField.forceActiveFocus(); else e.accepted = false }
            delegate: ItemDelegate {
                id: cand
                required property var modelData
                required property int index
                width: ListView.view.width
                height: 108
                focus: ListView.isCurrentItem
                Accessible.name: modelData.title + (modelData.year > 0 ? " (" + modelData.year + ")" : "")
                function choose() {
                    Core.chooseMatch(dialog.itemId, cand.modelData.providerId)
                    dialog.close()
                }
                onClicked: choose()
                Keys.onReturnPressed: choose()
                Keys.onEnterPressed: choose()
                background: Rectangle {
                    radius: Theme.radius
                    color: cand.activeFocus || cand.hovered ? Theme.surfaceHi : "transparent"
                    border.width: cand.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                contentItem: RowLayout {
                    spacing: 14
                    Rectangle {
                        Layout.preferredWidth: 62; Layout.preferredHeight: 92; radius: 6; clip: true; color: Theme.surfaceHi
                        Image { anchors.fill: parent; source: cand.modelData.posterUrl; asynchronous: true; fillMode: Image.PreserveAspectCrop }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4
                        Label {
                            text: cand.modelData.title + (cand.modelData.year > 0 ? "  (" + cand.modelData.year + ")" : "")
                            color: Theme.text; font.bold: true; font.pixelSize: Theme.fontBody
                        }
                        Label {
                            Layout.fillWidth: true
                            text: cand.modelData.overview
                            color: Theme.textDim; font.pixelSize: Theme.fontSmall
                            wrapMode: Text.Wrap; maximumLineCount: 3; elide: Text.ElideRight
                        }
                    }
                }
            }
        }
    }
}
