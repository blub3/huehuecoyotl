pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl
import "MediaInfo.js" as MediaInfo

// Now Playing (issue #8): cover, title, artist, album, the format line, the seek bar, the
// transport (shuffle, previous, play or pause, next, repeat) and the queue. Space plays or
// pauses anywhere on the page. In the queue, Enter plays an entry, Menu or Delete offers to
// remove it, Ctrl+Up and Ctrl+Down move it.
FocusScope {
    id: page
    objectName: "nowPlaying"
    readonly property bool isPlayer: false
    readonly property var e: Music.current
    readonly property double length: Music.duration > 0 ? Music.duration : (e.durationMs || 0) / 1000

    Component.onCompleted: Qt.callLater(() => playButton.forceActiveFocus())
    onActiveFocusChanged: if (activeFocus && !content.activeFocus) playButton.forceActiveFocus()
    Keys.onSpacePressed: Music.togglePause()

    Label {
        anchors.centerIn: parent
        visible: !Music.active
        text: qsTr("Nothing is playing. Choose Play on an album.")
        color: Theme.textDim
        font.pixelSize: Theme.fontLarge
    }

    RowLayout {
        id: content
        visible: Music.active
        anchors.fill: parent
        anchors.margins: 48
        spacing: 48

        // ---- the track ---------------------------------------------------------------------
        ColumnLayout {
            id: trackColumn
            // The cover is square: as wide as the column, at most 45 % of the page's height.
            readonly property real side: Math.min(width, page.height * 0.45)
            Layout.preferredWidth: Math.min(520, (page.width - 96) * 0.48)
            Layout.maximumWidth: Layout.preferredWidth
            Layout.fillHeight: true
            spacing: 14

            Rectangle {
                Layout.preferredWidth: trackColumn.side
                Layout.preferredHeight: trackColumn.side
                radius: Theme.radius
                color: Theme.surface
                clip: true
                Label {
                    anchors.centerIn: parent
                    visible: cover.status !== Image.Ready
                    text: "♫"
                    color: Theme.textDim
                    font.pixelSize: 96
                }
                Image {
                    id: cover
                    anchors.fill: parent
                    source: page.e.posterFull || page.e.poster || ""
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                    sourceSize.width: 1000
                }
            }
            Label {
                Layout.fillWidth: true
                text: page.e.title || ""
                color: Theme.text
                font.pixelSize: 30
                font.bold: true
                wrapMode: Text.Wrap
                maximumLineCount: 2
                elide: Text.ElideRight
                Accessible.role: Accessible.Heading
            }
            RowLayout {
                spacing: 8
                Layout.fillWidth: true
                FocusButton {
                    id: artistButton
                    visible: (page.e.artists || "") !== ""
                    text: page.e.artists || ""
                    tip: qsTr("Open the artist")
                    enabled: (page.e.artistId || 0) > 0
                    Layout.maximumWidth: trackColumn.width / 2 - 4
                    KeyNavigation.right: albumButton
                    KeyNavigation.down: seek
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Nav.openItem(page.e.artistId, "artist")
                }
                FocusButton {
                    id: albumButton
                    visible: (page.e.album || "") !== ""
                    text: page.e.album || ""
                    tip: qsTr("Open the album")
                    Layout.maximumWidth: trackColumn.width / 2 - 4
                    KeyNavigation.left: artistButton
                    KeyNavigation.down: seek
                    onClicked: Nav.openItem(page.e.albumId, "album")
                }
            }
            RowLayout {
                spacing: 8
                visible: (page.e.codec || "") !== ""
                Rectangle {
                    visible: page.e.hires === true
                    implicitWidth: hiresText.implicitWidth + 12
                    implicitHeight: 22
                    radius: 4
                    color: "transparent"
                    border.width: 1
                    border.color: Theme.accent
                    Label { id: hiresText; anchors.centerIn: parent; text: qsTr("Hi-Res"); color: Theme.accent; font.pixelSize: 12; font.bold: true }
                }
                Label {
                    text: MediaInfo.audioLine(page.e.codec || "", page.e.sampleRate || 0, page.e.bitDepth || 0)
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
            }

            // Seek bar: Left and Right jump 10 s.
            Slider {
                id: seek
                Layout.fillWidth: true
                Layout.topMargin: 6
                from: 0
                to: Math.max(1, page.length)
                value: pressed ? value : Music.position
                focusPolicy: Qt.StrongFocus
                Accessible.name: qsTr("Position")
                onMoved: Music.seek(value, false)
                Keys.onLeftPressed: Music.seek(-10, true)
                Keys.onRightPressed: Music.seek(10, true)
                KeyNavigation.up: artistButton.visible ? artistButton : albumButton
                KeyNavigation.down: playButton
                background: Rectangle {
                    x: seek.leftPadding
                    y: seek.topPadding + seek.availableHeight / 2 - height / 2
                    width: seek.availableWidth
                    height: 4
                    radius: 2
                    color: Theme.surfaceHi
                    Rectangle { width: seek.visualPosition * parent.width; height: parent.height; radius: 2; color: Theme.accent }
                }
                handle: Rectangle {
                    x: seek.leftPadding + seek.visualPosition * (seek.availableWidth - width)
                    y: seek.topPadding + seek.availableHeight / 2 - height / 2
                    width: seek.activeFocus ? 20 : 14
                    height: width
                    radius: width / 2
                    color: Theme.text
                    border.width: seek.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Label { text: Theme.time(Music.position); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                Item { Layout.fillWidth: true }
                Label { text: Theme.time(page.length); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            }

            RowLayout {
                Layout.alignment: Qt.AlignHCenter
                spacing: 12
                FocusButton {
                    id: shuffleButton
                    glyph: "⤮"
                    tip: Music.shuffle ? qsTr("Shuffle: on") : qsTr("Shuffle: off")
                    primary: Music.shuffle
                    KeyNavigation.right: prevButton
                    KeyNavigation.up: seek
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Music.shuffle = !Music.shuffle
                }
                FocusButton {
                    id: prevButton
                    glyph: "⏮"
                    tip: qsTr("Previous")
                    KeyNavigation.left: shuffleButton
                    KeyNavigation.right: playButton
                    KeyNavigation.up: seek
                    onClicked: Music.previous()
                }
                FocusButton {
                    id: playButton
                    primary: true
                    glyph: Music.playing ? "❚❚" : "▶"
                    tip: Music.playing ? qsTr("Pause") : qsTr("Play")
                    KeyNavigation.left: prevButton
                    KeyNavigation.right: nextButton
                    KeyNavigation.up: seek
                    onClicked: Music.togglePause()
                }
                FocusButton {
                    id: nextButton
                    glyph: "⏭"
                    tip: qsTr("Next")
                    KeyNavigation.left: playButton
                    KeyNavigation.right: repeatButton
                    KeyNavigation.up: seek
                    onClicked: Music.next()
                }
                FocusButton {
                    id: repeatButton
                    glyph: Music.repeat === "one" ? "↻1" : "↻"
                    tip: Music.repeat === "one" ? qsTr("Repeat: this track")
                       : Music.repeat === "all" ? qsTr("Repeat: all") : qsTr("Repeat: off")
                    primary: Music.repeat !== "off"
                    KeyNavigation.left: nextButton
                    KeyNavigation.right: queueList
                    KeyNavigation.up: seek
                    onClicked: Music.cycleRepeat()
                }
            }
            Item { Layout.fillHeight: true }
        }

        // ---- the queue ---------------------------------------------------------------------
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 10
            Label {
                text: qsTr("Queue") + "  ·  " + qsTr("%n track(s)", "", Music.queueCount)
                color: Theme.textDim
                font.pixelSize: Theme.fontLarge
            }
            ListView {
                id: queueList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: Music.queue
                spacing: 2
                keyNavigationEnabled: true
                currentIndex: Math.max(0, Music.queuePosition - Music.queueOffset)
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Queue")
                ScrollBar.vertical: ScrollBar {}
                Keys.onLeftPressed: repeatButton.forceActiveFocus()
                Keys.onDownPressed: (ev) => { if (currentIndex >= count - 1) clearButton.forceActiveFocus(); else ev.accepted = false }
                // Keep the playing entry in view when the queue moves on.
                Connections {
                    target: Music
                    function onQueueChanged() {
                        if (!queueList.activeFocus)
                            queueList.positionViewAtIndex(Math.max(0, Music.queuePosition - Music.queueOffset), ListView.Contain)
                    }
                }
                delegate: ItemDelegate {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property bool now: modelData.index === Music.queuePosition
                    width: queueList.width
                    height: 52
                    focus: ListView.isCurrentItem
                    Accessible.name: modelData.title + (modelData.artists ? ", " + modelData.artists : "")
                    onClicked: Music.jump(modelData.index)
                    Keys.onReturnPressed: Music.jump(modelData.index)
                    Keys.onEnterPressed: Music.jump(modelData.index)
                    Keys.onPressed: (ev) => {
                        if (ev.key === Qt.Key_Menu || ev.key === Qt.Key_Delete) {
                            entryMenu.showFor(row, row.modelData.index)
                            ev.accepted = true
                        } else if ((ev.modifiers & Qt.ControlModifier) && ev.key === Qt.Key_Up && row.modelData.index > 0) {
                            Music.move(row.modelData.index, row.modelData.index - 1)
                            queueList.currentIndex = Math.max(0, row.index - 1)
                            ev.accepted = true
                        } else if ((ev.modifiers & Qt.ControlModifier) && ev.key === Qt.Key_Down && row.modelData.index < Music.queueCount - 1) {
                            Music.move(row.modelData.index, row.modelData.index + 1)
                            queueList.currentIndex = row.index + 1
                            ev.accepted = true
                        }
                    }
                    background: Rectangle {
                        radius: 8
                        color: row.activeFocus ? Theme.surfaceHi : row.hovered || row.now ? Theme.surface : "transparent"
                        border.width: row.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        spacing: 14
                        Label {
                            Layout.preferredWidth: 30
                            text: row.now ? "♪" : String(row.modelData.index + 1)
                            color: row.now ? Theme.accent : Theme.textDim
                            horizontalAlignment: Text.AlignRight
                        }
                        Image {
                            Layout.preferredWidth: 40
                            Layout.preferredHeight: 40
                            source: row.modelData.poster || ""
                            sourceSize.width: 80
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Label {
                                Layout.fillWidth: true
                                text: row.modelData.title
                                color: row.now ? Theme.accent : Theme.text
                                elide: Text.ElideRight
                            }
                            Label {
                                Layout.fillWidth: true
                                text: [row.modelData.artists, row.modelData.album].filter(x => x).join("  ·  ")
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                                elide: Text.ElideRight
                            }
                        }
                        Label {
                            text: row.modelData.durationMs > 0 ? Theme.time(row.modelData.durationMs / 1000) : ""
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                        }
                    }
                }
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: clearButton
                    property bool armed: false
                    text: armed ? qsTr("Really clear the queue?") : qsTr("Clear queue")
                    font.pixelSize: Theme.fontSmall
                    KeyNavigation.up: queueList
                    onActiveFocusChanged: if (!activeFocus) armed = false
                    onClicked: { if (armed) Music.clear(); else armed = true }
                }
            }
        }
    }

    ActionMenu {
        id: entryMenu
        property int position: -1
        function showFor(item, pos) {
            position = pos
            show(item, [{ value: "play", label: qsTr("Play now") }, { value: "remove", label: qsTr("Remove from queue") }])
        }
        onChosen: (v) => { if (v === "play") Music.jump(position); else Music.remove(position) }
    }
}
