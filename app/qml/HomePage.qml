pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Home: continue watching, recently added, and a first-run guide while the library is empty.
FocusScope {
    id: page
    readonly property bool isPlayer: false

    ContinueWatchingModel { id: recent }
    RecentlyAddedModel { id: added }
    LibraryFoldersModel { id: folders }

    readonly property bool firstRun: !folders.loading && folders.count === 0

    // Keyboard and remote users must always have a focused item on this page.
    function ensureFocus() {
        if (!page.activeFocus) return
        for (const v of [recentList, addedList, setupButton, keyButton])
            if (v.activeFocus) return
        if (recent.count > 0) recentList.forceActiveFocus()
        else if (added.count > 0) addedList.forceActiveFocus()
        else if (page.firstRun) setupButton.forceActiveFocus()
        else Nav.focusRail()
    }

    onActiveFocusChanged: if (activeFocus) { recent.refresh(); Qt.callLater(ensureFocus) }
    Connections { target: recent; function onLoadingChanged() { if (!recent.loading) { page.answered |= 1; Qt.callLater(page.ensureFocus) } } }
    Connections { target: added; function onLoadingChanged() { if (!added.loading) { page.answered |= 2; Qt.callLater(page.ensureFocus) } } }
    Connections { target: folders; function onLoadingChanged() { if (!folders.loading) { page.answered |= 4; Qt.callLater(page.ensureFocus) } } }
    // All three rows have their first answer: Home is interactive (startup budget, SPEC 9).
    property int answered: 0
    onAnsweredChanged: if (answered === 7) Bench.homeReady()
    Connections { target: Core; function onItemChanged() { recent.refresh() } }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.implicitHeight + 64
        clip: true

        ColumnLayout {
            id: column
            x: 40
            y: 36
            width: parent.width - 80
            spacing: 16

            // ---- first run --------------------------------------------------------------------
            Rectangle {
                visible: page.firstRun
                Layout.fillWidth: true
                Layout.preferredHeight: welcome.implicitHeight + 56
                radius: Theme.radius
                color: Theme.surface
                ColumnLayout {
                    id: welcome
                    anchors.fill: parent
                    anchors.margins: 28
                    spacing: 12
                    RowLayout {
                        spacing: 16
                        AppLogo { size: 64; Layout.preferredWidth: 64; Layout.preferredHeight: 64 }
                        Label { text: qsTr("Welcome to Huehuecoyotl"); color: Theme.text; font.pixelSize: Theme.fontTitle; font.bold: true }
                    }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        color: Theme.textDim
                        text: qsTr("Add the folders with your movies and shows. They are scanned and sorted into Movies and Shows. With a free TMDB key you also get posters, descriptions and episode titles.")
                    }
                    RowLayout {
                        spacing: 12
                        Layout.topMargin: 8
                        FocusButton {
                            id: setupButton
                            primary: true
                            glyph: "+"
                            text: qsTr("Add a folder")
                            KeyNavigation.right: keyButton
                            Keys.onLeftPressed: Nav.focusRail()
                            onClicked: Nav.go("sources")
                        }
                        FocusButton {
                            id: keyButton
                            // Only once the core looked and found no key (issue #52).
                            visible: Core.tmdbKeyState === "none" || Core.tmdbKeyState === "unavailable"
                            text: qsTr("Set up TMDB")
                            KeyNavigation.left: setupButton
                            onClicked: Nav.go("settings")
                        }
                    }
                }
            }

            // ---- continue watching --------------------------------------------------------
            Label {
                visible: recent.count > 0
                text: qsTr("Continue watching")
                color: Theme.textDim
                font.pixelSize: Theme.fontLarge
            }
            ListView {
                id: recentList
                visible: recent.count > 0
                Layout.fillWidth: true
                Layout.preferredHeight: 190
                orientation: ListView.Horizontal
                spacing: 18
                model: recent
                keyNavigationEnabled: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Continue watching")
                KeyNavigation.down: added.count > 0 ? addedList : null
                Keys.onLeftPressed: (e) => { if (currentIndex === 0) Nav.focusRail(); else e.accepted = false }
                onActiveFocusChanged: if (activeFocus) flick.contentY = 0
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

            // ---- recently added -----------------------------------------------------------
            Label {
                visible: added.count > 0
                text: qsTr("Recently added")
                color: Theme.textDim
                font.pixelSize: Theme.fontLarge
                Layout.topMargin: 8
            }
            ListView {
                id: addedList
                visible: added.count > 0
                Layout.fillWidth: true
                Layout.preferredHeight: Appearance.cardHeight(Appearance.coverWidth) + 16
                orientation: ListView.Horizontal
                spacing: Math.round(22 * Appearance.textScale(Appearance.coverWidth))
                model: added
                // Keep the focused card in view when the cover size changes.
                onSpacingChanged: Qt.callLater(() => { if (currentIndex >= 0) positionViewAtIndex(currentIndex, ListView.Contain) })
                keyNavigationEnabled: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Recently added")
                KeyNavigation.up: recent.count > 0 ? recentList : null
                Keys.onLeftPressed: (e) => { if (currentIndex === 0) Nav.focusRail(); else e.accepted = false }
                onActiveFocusChanged: if (activeFocus) flick.contentY = Math.max(0, y - 60)
                delegate: PosterCard {
                    id: tile
                    required property var model
                    y: 8
                    focus: ListView.isCurrentItem
                    title: model.title
                    year: model.year
                    poster: model.poster
                    posterLarge: model.posterLarge ?? ""
                    posterWidth: Appearance.coverWidth
                    progress: model.progress
                    watched: model.watched
                    unwatched: Number(model.unwatched)
                    onClicked: Nav.openItem(tile.model.id)
                    Keys.onReturnPressed: Nav.openItem(tile.model.id)
                    Keys.onEnterPressed: Nav.openItem(tile.model.id)
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
