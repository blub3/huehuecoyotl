pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Detail page for a movie, show, season or episode. Shows list their seasons; seasons list
// their episodes. Re-queries when the item or the library changes.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var itemId
    property var d: ({})
    property var request: 0
    readonly property string kind: d.kind || ""
    readonly property var childItems: d.children || []
    readonly property bool loaded: d.id !== undefined

    function load() { request = Core.itemDetail(itemId) }
    function childView() {
        if (kind === "show" && seasonRow.count > 0) return seasonRow
        if (kind === "season" && episodeList.count > 0) return episodeList
        return null
    }
    // Keep the focused episode row inside the visible part of the page.
    function reveal(item) {
        const p = item.mapToItem(flick.contentItem, 0, 0)
        if (p.y < flick.contentY + 40) flick.contentY = Math.max(0, p.y - 40)
        else if (p.y + item.height > flick.contentY + flick.height - 40)
            flick.contentY = Math.min(p.y + item.height - flick.height + 40, Math.max(0, flick.contentHeight - flick.height))
    }
    function play() {
        Playback.openItem(d.nextUp ? d.nextUp.id : itemId)
    }
    function metaLine() {
        const parts = []
        if (d.year > 0) parts.push(d.year)
        if (d.runtimeMin > 0) parts.push(qsTr("%1 min").arg(d.runtimeMin))
        if (d.rating > 0) parts.push("★ " + d.rating.toFixed(1))
        if (d.genres && d.genres.length) parts.push(d.genres.slice(0, 3).join(", "))
        return parts.join("  ·  ")
    }
    function playLabel() {
        if (kind === "show" || kind === "season") {
            if (!d.nextUp) return qsTr("Play")
            return qsTr("Play S%1 E%2").arg(d.nextUp.seasonNo).arg(d.nextUp.episodeNo)
        }
        return d.positionMs > 0 && !d.watched ? qsTr("Resume from %1").arg(Theme.time(d.positionMs / 1000)) : qsTr("Play")
    }

    Component.onCompleted: load()
    onActiveFocusChanged: if (activeFocus && loaded && !content.activeFocus) playButton.forceActiveFocus()

    Connections {
        target: Core
        function onDetailReady(requestId, detail) {
            if (requestId !== page.request) return
            const first = !page.loaded
            page.d = detail
            if (first) playButton.forceActiveFocus()
        }
        function onItemChanged(id) { if (id === page.itemId || page.kind === "show" || page.kind === "season") page.load() }
        function onLibraryChanged() { page.load() }
    }

    // Backdrop
    Image {
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: parent.height * 0.7
        source: page.d.backdrop || ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        opacity: 0.35
    }
    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0.0; color: "#600d1015" }
            GradientStop { position: 0.55; color: Theme.bg }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.implicitHeight + 80
        clip: true
        ScrollBar.vertical: ScrollBar {}

        FocusScope {
            id: content
            x: 48
            y: 48
            width: flick.width - 96
            implicitHeight: col.implicitHeight
            focus: true

            ColumnLayout {
                id: col
                width: parent.width
                spacing: 22

                RowLayout {
                    spacing: 32
                    Layout.fillWidth: true

                    Rectangle {
                        Layout.preferredWidth: page.kind === "episode" ? 360 : 240
                        Layout.preferredHeight: page.kind === "episode" ? 203 : 360
                        Layout.alignment: Qt.AlignTop
                        radius: Theme.radius
                        clip: true
                        color: Theme.surfaceHi
                        Label {
                            anchors.centerIn: parent
                            width: parent.width - 32
                            visible: detailPoster.status !== Image.Ready
                            text: page.d.title || ""
                            color: Theme.textDim
                            font.pixelSize: 22
                            font.bold: true
                            wrapMode: Text.Wrap
                            horizontalAlignment: Text.AlignHCenter
                        }
                        Image {
                            id: detailPoster
                            anchors.fill: parent
                            source: page.kind === "episode" ? (page.d.poster || "") : (page.d.posterFull || page.d.poster || "")
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: 10
                        Label {
                            visible: page.d.parentTitle !== undefined && page.d.parentTitle !== ""
                            text: page.kind === "episode" ? qsTr("%1 · Season %2, Episode %3").arg(page.d.parentTitle).arg(page.d.seasonNo).arg(page.d.episodeNo) : (page.d.parentTitle || "")
                            color: Theme.accent
                            font.pixelSize: Theme.fontBody
                        }
                        Label {
                            Layout.fillWidth: true
                            text: page.d.title || ""
                            color: Theme.text
                            font.pixelSize: 36
                            font.bold: true
                            wrapMode: Text.Wrap
                            Accessible.role: Accessible.Heading
                        }
                        Label {
                            visible: text !== ""
                            text: page.loaded ? page.metaLine() : ""
                            color: Theme.textDim
                            font.pixelSize: Theme.fontBody
                        }
                        Label {
                            visible: (page.kind === "movie" || page.kind === "show") && (page.d.matchState === "needs_review" || page.d.matchState === "unmatched")
                            text: page.d.matchState === "needs_review" ? qsTr("Several matches are possible. Choose the right one with “Fix match”.") : qsTr("Not matched yet: titles come from the file names.")
                            color: Theme.accent
                            font.pixelSize: Theme.fontSmall
                        }
                        Label {
                            Layout.fillWidth: true
                            Layout.maximumWidth: 900
                            text: page.d.overview || ""
                            color: Theme.text
                            font.pixelSize: Theme.fontBody
                            wrapMode: Text.Wrap
                            maximumLineCount: 7
                            elide: Text.ElideRight
                            lineHeight: 1.2
                        }
                        Label {
                            visible: page.d.cast !== undefined && page.d.cast.length > 0
                            Layout.fillWidth: true
                            Layout.maximumWidth: 900
                            text: qsTr("With %1").arg((page.d.cast || []).slice(0, 6).join(", "))
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                            wrapMode: Text.Wrap
                        }

                        RowLayout {
                            Layout.topMargin: 10
                            spacing: 12
                            FocusButton {
                                id: playButton
                                primary: true
                                focus: true
                                glyph: "▶"
                                text: page.loaded ? page.playLabel() : qsTr("Play")
                                KeyNavigation.right: watchedButton
                                KeyNavigation.down: page.childView()
                                Keys.onLeftPressed: Nav.focusRail()
                                onClicked: page.play()
                            }
                            FocusButton {
                                id: watchedButton
                                text: page.d.watched || (page.kind !== "movie" && page.kind !== "episode" && page.d.unwatched === 0) ? qsTr("Mark unwatched") : qsTr("Mark watched")
                                KeyNavigation.left: playButton
                                KeyNavigation.right: fixButton.visible ? fixButton : null
                                KeyNavigation.down: page.childView()
                                onClicked: Core.setWatched(page.itemId, !(page.d.watched || (page.kind !== "movie" && page.kind !== "episode" && page.d.unwatched === 0)))
                            }
                            FocusButton {
                                id: fixButton
                                visible: Core.hasTmdbKey && (page.kind === "movie" || page.kind === "show")
                                text: qsTr("Fix match")
                                KeyNavigation.left: watchedButton
                                KeyNavigation.down: page.childView()
                                onClicked: fixDialog.openFor(page.itemId, page.d.title, page.d.year)
                            }
                        }
                    }
                }

                Label {
                    visible: page.childItems.length > 0
                    text: page.kind === "show" ? qsTr("Seasons") : qsTr("Episodes")
                    color: Theme.textDim
                    font.pixelSize: Theme.fontLarge
                    Layout.topMargin: 12
                }

                // Shows: season posters in a row.
                ListView {
                    id: seasonRow
                    visible: page.kind === "show" && count > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: 330
                    orientation: ListView.Horizontal
                    spacing: 22
                    model: page.kind === "show" ? page.childItems : []
                    keyNavigationEnabled: true
                    Keys.onUpPressed: playButton.forceActiveFocus()
                    Keys.onLeftPressed: (e) => { if (currentIndex === 0) Nav.focusRail(); else e.accepted = false }
                    onActiveFocusChanged: if (activeFocus) flick.contentY = Math.min(Math.max(0, y - 40), Math.max(0, flick.contentHeight - flick.height))
                    delegate: PosterCard {
                        id: seasonCard
                        required property var modelData
                        y: 8
                        focus: ListView.isCurrentItem
                        posterWidth: 170
                        title: modelData.title
                        poster: modelData.poster
                        unwatched: Number(modelData.unwatched)
                        watched: Number(modelData.unwatched) === 0
                        caption: ""
                        onClicked: Nav.openItem(seasonCard.modelData.id)
                        Keys.onReturnPressed: Nav.openItem(seasonCard.modelData.id)
                        Keys.onEnterPressed: Nav.openItem(seasonCard.modelData.id)
                    }
                }

                // Seasons: episode rows.
                ListView {
                    id: episodeList
                    visible: page.kind === "season" && count > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: contentHeight
                    interactive: false
                    spacing: 8
                    model: page.kind === "season" ? page.childItems : []
                    keyNavigationEnabled: true
                    Keys.onUpPressed: (e) => { if (currentIndex === 0) playButton.forceActiveFocus(); else e.accepted = false }
                    Keys.onLeftPressed: Nav.focusRail()
                    onCurrentIndexChanged: if (activeFocus && currentItem) page.reveal(currentItem)
                    onActiveFocusChanged: if (activeFocus && currentItem) page.reveal(currentItem)
                    delegate: ItemDelegate {
                        id: ep
                        required property var modelData
                        width: episodeList.width
                        height: 112
                        focus: ListView.isCurrentItem
                        focusPolicy: Qt.StrongFocus
                        Accessible.name: qsTr("Episode %1, %2").arg(modelData.episodeNo).arg(modelData.title)
                        onClicked: Playback.openItem(ep.modelData.id)
                        Keys.onReturnPressed: Playback.openItem(ep.modelData.id)
                        Keys.onEnterPressed: Playback.openItem(ep.modelData.id)
                        Keys.onPressed: (e) => {
                            if (e.key === Qt.Key_W) { Core.setWatched(ep.modelData.id, !ep.modelData.watched); e.accepted = true }
                            else if (e.key === Qt.Key_I) { Nav.openItem(ep.modelData.id); e.accepted = true }
                        }
                        background: Rectangle {
                            radius: Theme.radius
                            color: ep.activeFocus ? Theme.surfaceHi : ep.hovered ? Theme.surface : "transparent"
                            border.width: ep.activeFocus ? Theme.focusWidth : 0
                            border.color: Theme.focus
                        }
                        contentItem: RowLayout {
                            spacing: 18
                            Rectangle {
                                Layout.preferredWidth: 160; Layout.preferredHeight: 90
                                radius: 6; clip: true
                                color: Theme.surfaceHi
                                Label { anchors.centerIn: parent; text: ep.modelData.episodeNo; color: Theme.textDim; font.pixelSize: 26; visible: still.status !== Image.Ready }
                                Image { id: still; anchors.fill: parent; source: ep.modelData.poster; asynchronous: true; fillMode: Image.PreserveAspectCrop; sourceSize.width: 320 }
                                Rectangle {
                                    visible: ep.modelData.progress > 0 && !ep.modelData.watched
                                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                                    height: 4; color: "#80000000"
                                    Rectangle { width: parent.width * ep.modelData.progress; height: 4; color: Theme.accent }
                                }
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 4
                                Label {
                                    Layout.fillWidth: true
                                    text: ep.modelData.episodeNo + ". " + ep.modelData.title
                                    color: ep.modelData.watched ? Theme.textDim : Theme.text
                                    font.pixelSize: Theme.fontBody
                                    font.bold: true
                                    elide: Text.ElideRight
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: ep.modelData.overview
                                    color: Theme.textDim
                                    font.pixelSize: Theme.fontSmall
                                    wrapMode: Text.Wrap
                                    maximumLineCount: 2
                                    elide: Text.ElideRight
                                }
                            }
                            Label { visible: ep.modelData.watched; text: "✓"; color: Theme.accent; font.pixelSize: 20 }
                        }
                    }
                }
            }
        }
    }

    FixMatchDialog {
        id: fixDialog
        anchors.centerIn: parent
    }
}
