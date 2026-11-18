pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Items whose match was not certain (SPEC 7, scan step 4): pick the right candidate.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var request: 0
    property var items: []
    property bool loaded: false

    function load() { request = Core.review() }
    Component.onCompleted: load()
    function focusFirst() {
        if (!page.activeFocus || back.activeFocus || reviewList.activeFocus) return
        (items.length > 0 ? reviewList : back).forceActiveFocus()
    }
    onActiveFocusChanged: if (activeFocus) Qt.callLater(focusFirst)

    Connections {
        target: Core
        function onReviewReady(requestId, list) {
            if (requestId !== page.request) return
            page.items = list
            page.loaded = true
            list.length > 0 ? reviewList.forceActiveFocus() : back.forceActiveFocus()
        }
        function onLibraryChanged() { page.load() }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        spacing: 16
        RowLayout {
            spacing: 14
            FocusButton { id: back; glyph: "‹"; tip: qsTr("Back"); onClicked: Nav.back(); KeyNavigation.down: reviewList; Keys.onLeftPressed: Nav.focusRail() }
            Label { text: qsTr("Review matches"); color: Theme.text; font.pixelSize: Theme.fontTitle; font.bold: true }
        }
        Label {
            visible: page.loaded && page.items.length === 0
            text: qsTr("Nothing to review. Every movie and show has a confident match.")
            color: Theme.textDim
        }
        ListView {
            id: reviewList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 18
            model: page.items
            keyNavigationEnabled: true
            Keys.onUpPressed: (e) => { if (currentIndex === 0) back.forceActiveFocus(); else e.accepted = false }
            Keys.onLeftPressed: Nav.focusRail()
            delegate: FocusScope {
                id: entry
                required property var modelData
                required property int index
                width: ListView.view.width
                height: 300
                focus: ListView.isCurrentItem
                ColumnLayout {
                    anchors.fill: parent
                    spacing: 8
                    Label {
                        text: qsTr("“%1” could be:").arg(entry.modelData.title)
                        color: Theme.text; font.pixelSize: Theme.fontLarge
                    }
                    ListView {
                        id: cands
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        orientation: ListView.Horizontal
                        spacing: 18
                        focus: true
                        keyNavigationEnabled: true
                        model: entry.modelData.candidates
                        delegate: PosterCard {
                            id: c
                            required property var modelData
                            posterWidth: 140
                            title: modelData.title
                            year: modelData.year
                            poster: modelData.posterUrl
                            focus: ListView.isCurrentItem
                            onClicked: Core.chooseMatch(entry.modelData.id, c.modelData.providerId)
                            Keys.onReturnPressed: Core.chooseMatch(entry.modelData.id, c.modelData.providerId)
                            Keys.onEnterPressed: Core.chooseMatch(entry.modelData.id, c.modelData.providerId)
                        }
                        footer: Item {
                            width: 180; height: 210
                            FocusButton {
                                anchors.centerIn: parent
                                text: qsTr("Search…")
                                onClicked: fixDialog.openFor(entry.modelData.id, entry.modelData.title, entry.modelData.year, entry.modelData.kind)
                            }
                        }
                    }
                }
            }
        }
    }

    FixMatchDialog { id: fixDialog; backend: Core; onChosen: page.load() }
}
