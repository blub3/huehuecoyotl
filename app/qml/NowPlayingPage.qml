pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl
import "MediaInfo.js" as MediaInfo

// Now Playing (issue #8, redesigned in #82): the whole window (the navigation rail steps aside),
// tinted with the cover's colour; a large cover, the song, its format and the signal path as a
// chain File → Huehuecoyotl → Output with a verdict; a seek bar drawn from the song's waveform;
// the transport; a volume slider (fixed at 100 % in bit-perfect mode); and Up next on the right.
// Space plays or pauses anywhere on the page. In Up next, Enter plays an entry, Menu or Delete
// offers its actions, Ctrl+Up and Ctrl+Down move it.
// Full screen (issue #83): "Full screen" or F11 shows MusicFullScreen over the page and puts
// the window in full screen; Back or F11 leaves both.
FocusScope {
    id: page
    objectName: "nowPlaying"
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: qsTr("Now Playing")
    readonly property var e: Music.current
    readonly property double length: Music.duration > 0 ? Music.duration : (e.durationMs || 0) / 1000
    // Issue #9: what goes out, and what to say about it (SPEC 5.7, "Signal path").
    readonly property var sp: Music.signalPath
    readonly property string signalNote: {
        if (sp.fallback) return qsTr("The chosen audio device could not be opened; playing on the system's default device.")
        if (!(sp.outRate > 0)) return ""
        if (sp.bitPerfect) return qsTr("Every sample reaches the device as it is in the file.")
        if (!sp.exclusive)
            return qsTr("Shared output through %1 at %2. The system mixer may resample.").arg(MediaInfo.outputName(sp.ao)).arg(MediaInfo.rate(sp.outRate))
        if (sp.filters) return qsTr("Audio filters are active.")
        if (sp.sourceRate !== sp.outRate)
            return qsTr("The device does not take %1; converted to %2.").arg(MediaInfo.rate(sp.sourceRate)).arg(MediaInfo.rate(sp.outRate))
        return qsTr("The device does not take this format; converted to %1.").arg(MediaInfo.sampleFormat(sp.outFormat))
    }
    // Issue #82: the tint and the waveform of the song playing.
    property string tint: ""
    // Issue #134: the strip with the Back button above the page takes the page's colour.
    readonly property color pageBackground: tint !== "" ? tint : "#142029"
    property var peaks: []
    property var visualsRequest: 0
    readonly property real peakMax: Math.max(1, ...peaks)
    function loadVisuals() {
        if (e.id === undefined) { tint = ""; peaks = []; return }
        visualsRequest = Core.trackVisuals(e.id, e.fileId || 0)
    }
    onEChanged: loadVisuals()
    Component.onCompleted: { loadVisuals(); Qt.callLater(() => playButton.forceActiveFocus()) }
    onActiveFocusChanged: {
        if (!activeFocus) return
        if (full) { const v = fullView.item as MusicFullScreen; if (v && !v.activeFocus) v.focusPlay() }
        else if (!content.activeFocus) playButton.forceActiveFocus()
    }

    // Issue #83: the full-screen view. The window goes full screen with it unless it already was.
    property bool full: false
    property bool madeFullscreen: false
    function toggleFull() {
        const w = page.Window.window
        if (!full) {
            if (!Music.active) return
            madeFullscreen = w && w.visibility !== Window.FullScreen
            if (madeFullscreen) w.visibility = Window.FullScreen
            full = true
        } else {
            full = false
            if (madeFullscreen && w) w.visibility = Window.Windowed
            madeFullscreen = false
            Qt.callLater(() => fullButton.forceActiveFocus())
        }
    }
    // Leaving the page (Back to Music) leaves the full-screen window too.
    Component.onDestruction: if (madeFullscreen && page.Window.window) page.Window.window.visibility = Window.Windowed
    Connections {
        target: Music
        function onQueueChanged() { if (!Music.active && page.full) page.toggleFull() }
    }
    Keys.onSpacePressed: Music.togglePause()

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId !== page.visualsRequest) return
            page.tint = detail.color || ""
            page.peaks = detail.peaks || []
        }
        // The waveform job finished for this song.
        function onItemChanged(id) { if (id === page.e.id && page.peaks.length === 0) page.loadVisuals() }
    }

    Rectangle {
        anchors.fill: parent
        color: page.pageBackground
        Behavior on color { ColorAnimation { duration: 400 } }
    }

    Label {
        anchors.centerIn: parent
        visible: !Music.active
        text: qsTr("Nothing is playing. Choose Play on an album.")
        color: Theme.textDim
        font.pixelSize: Theme.fontLarge
    }

    RowLayout {
        id: content
        visible: Music.active && !page.full
        anchors.fill: parent
        spacing: 0

        // ---- the song ------------------------------------------------------------------------
        ColumnLayout {
            id: main
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 40
            Layout.topMargin: 20
            spacing: 14

            RowLayout {
                Layout.fillWidth: true
                spacing: 14
                Label {
                    text: qsTr("Playing from")
                    color: "#c9d2dd"
                    font.pixelSize: Theme.fontSmall
                }
                FocusButton {
                    id: fromButton
                    visible: (page.e.album || "") !== ""
                    text: page.e.album || ""
                    font.pixelSize: Theme.fontSmall
                    KeyNavigation.right: fullButton
                    KeyNavigation.down: artistButton.visible ? artistButton : seek
                    onClicked: Nav.openItem(page.e.albumId, "album")
                }
                Label {
                    Layout.fillWidth: true
                    text: Music.queueCount > 0 ? qsTr("%1 of %2 in the queue").arg(Music.queuePosition + 1).arg(Music.queueCount) : ""
                    color: "#c9d2dd"
                    font.pixelSize: Theme.fontSmall
                    elide: Text.ElideRight
                }
                FocusButton {
                    id: fullButton
                    objectName: "fullScreenButton"
                    glyph: "⛶"
                    text: qsTr("Full screen")
                    tip: qsTr("Full screen (F11)")
                    font.pixelSize: Theme.fontSmall
                    KeyNavigation.left: fromButton
                    KeyNavigation.down: queueList
                    onClicked: page.toggleFull()
                }
            }

            RowLayout {
                id: songRow
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 40
                // The cover: as large as the height allows next to the details.
                Rectangle {
                    id: coverBox
                    // From the page, not the layout it sits in, so it does not feed back into the layout.
                    readonly property real side: Math.max(160, Math.min(page.height - 330, (page.width - Math.min(380, page.width * 0.3)) * 0.36))
                    Layout.preferredWidth: side
                    Layout.preferredHeight: side
                    Layout.alignment: Qt.AlignTop
                    radius: 14
                    color: "#26ffffff"
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
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignTop
                    spacing: 12
                    RowLayout {
                        spacing: 8
                        visible: (page.e.codec || "") !== ""
                        Rectangle {
                            visible: page.e.hires === true
                            implicitWidth: hiresText.implicitWidth + 14
                            implicitHeight: 24
                            radius: 6
                            color: Theme.accent
                            Label { id: hiresText; anchors.centerIn: parent; text: qsTr("Hi-Res"); color: Theme.bg; font.pixelSize: 12; font.bold: true }
                        }
                        Rectangle {
                            implicitWidth: fmtText.implicitWidth + 14
                            implicitHeight: 24
                            radius: 6
                            color: "#80000000"
                            Label { id: fmtText; anchors.centerIn: parent; text: MediaInfo.audioLine(page.e.codec || "", page.e.sampleRate || 0, page.e.bitDepth || 0); color: Theme.text; font.pixelSize: 12; font.bold: true }
                        }
                    }
                    Label {
                        Layout.fillWidth: true
                        text: page.e.title || ""
                        color: Theme.text
                        font.pixelSize: 44
                        font.bold: true
                        wrapMode: Text.Wrap
                        maximumLineCount: 2
                        elide: Text.ElideRight
                        lineHeight: 1.05
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
                            Layout.maximumWidth: songRow.width / 3
                            KeyNavigation.right: albumButton
                            KeyNavigation.down: seek
                            onClicked: Nav.openItem(page.e.artistId, "artist")
                        }
                        FocusButton {
                            id: albumButton
                            visible: (page.e.album || "") !== ""
                            text: page.e.album || ""
                            tip: qsTr("Open the album")
                            Layout.maximumWidth: songRow.width / 3
                            KeyNavigation.left: artistButton
                            KeyNavigation.down: seek
                            onClicked: Nav.openItem(page.e.albumId, "album")
                        }
                    }

                    // Issue #9/#82: the signal path as a chain, with a verdict and why.
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        visible: (page.sp.outRate ?? 0) > 0 || page.sp.fallback === true
                        implicitHeight: chain.implicitHeight + 32
                        radius: 14
                        color: "#80000000"
                        Accessible.role: Accessible.Grouping
                        Accessible.name: qsTr("Signal path")
                        ColumnLayout {
                            id: chain
                            anchors.fill: parent
                            anchors.margins: 16
                            spacing: 10
                            RowLayout {
                                spacing: 10
                                Label { text: qsTr("Signal path"); color: Theme.textDim; font.pixelSize: 12; font.capitalization: Font.AllUppercase; font.letterSpacing: 1 }
                                Rectangle {
                                    implicitWidth: verdict.implicitWidth + 14
                                    implicitHeight: 22
                                    radius: 6
                                    color: page.sp.bitPerfect === true ? Theme.accent : Theme.surfaceHi
                                    Label {
                                        id: verdict
                                        anchors.centerIn: parent
                                        text: page.sp.bitPerfect === true ? "✓ " + qsTr("Bit-perfect") : page.sp.exclusive === true ? qsTr("Converted") : qsTr("Shared output")
                                        color: page.sp.bitPerfect === true ? Theme.bg : Theme.focus
                                        font.pixelSize: 12
                                        font.bold: true
                                    }
                                }
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: 6
                                Repeater {
                                    model: [
                                        { role: qsTr("File"), main: MediaInfo.audioLine(page.e.codec || "", page.sp.sourceRate || page.e.sampleRate || 0, page.e.bitDepth || 0),
                                          sub: qsTr("%n channel(s)", "", page.sp.sourceChannels || 2), good: true, arrow: true },
                                        { role: "Huehuecoyotl", main: page.sp.bitPerfect ? qsTr("Unchanged") : qsTr("Volume %1 %").arg(Music.volume),
                                          sub: page.sp.bitPerfect ? qsTr("no resampling, no gain") : (Music.replayGain !== "off" ? qsTr("ReplayGain: %1").arg(Music.replayGain) : qsTr("no ReplayGain")),
                                          good: page.sp.bitPerfect === true, arrow: true },
                                        { role: qsTr("Output"), main: [MediaInfo.rate(page.sp.outRate || 0), MediaInfo.sampleFormat(page.sp.outFormat || "")].filter(x => x).join(" · "),
                                          sub: MediaInfo.outputName(page.sp.ao || "") + (page.sp.exclusive ? "  ·  " + qsTr("exclusive") : "  ·  " + qsTr("shared")),
                                          good: page.sp.bitPerfect === true, arrow: false }
                                    ]
                                    Row {
                                        id: node
                                        required property var modelData
                                        spacing: 6
                                        Rectangle {
                                            width: Math.max(130, nodeCol.implicitWidth + 24)
                                            height: nodeCol.implicitHeight + 18
                                            radius: 10
                                            color: node.modelData.good ? "#1ae3a33b" : Theme.surfaceHi
                                            border.width: 1
                                            border.color: node.modelData.good ? "#73e3a33b" : "#3a4252"
                                            Column {
                                                id: nodeCol
                                                anchors.centerIn: parent
                                                spacing: 1
                                                Label { text: node.modelData.role; color: Theme.textDim; font.pixelSize: 11; font.capitalization: Font.AllUppercase }
                                                Label { text: node.modelData.main; color: Theme.text; font.pixelSize: 13; font.bold: true }
                                                Label { text: node.modelData.sub; color: "#c9d2dd"; font.pixelSize: 11 }
                                            }
                                        }
                                        Label {
                                            visible: node.modelData.arrow
                                            anchors.verticalCenter: parent.verticalCenter
                                            text: "→"
                                            color: Theme.accent
                                            font.pixelSize: 18
                                        }
                                    }
                                }
                            }
                            Label {
                                Layout.fillWidth: true
                                visible: page.signalNote !== ""
                                text: page.signalNote
                                wrapMode: Text.Wrap
                                color: page.sp.fallback ? Theme.danger : "#c9d2dd"
                                font.pixelSize: 12
                            }
                        }
                    }
                }
            }

            // ---- the waveform seek bar --------------------------------------------------------
            Item {
                id: seek
                Layout.fillWidth: true
                Layout.preferredHeight: 64
                focus: false
                activeFocusOnTab: true
                readonly property real progress: page.length > 0 ? Math.min(1, Music.position / page.length) : 0
                Accessible.role: Accessible.Slider
                Accessible.name: qsTr("Position")
                Keys.onLeftPressed: Music.seek(-10, true)
                Keys.onRightPressed: Music.seek(10, true)
                Keys.onUpPressed: (e) => { if (artistButton.visible) artistButton.forceActiveFocus(); else e.accepted = false }
                KeyNavigation.down: playButton
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: -4
                    radius: 8
                    color: "transparent"
                    border.width: seek.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                // Bars from the waveform; a plain bar until it is computed.
                Row {
                    anchors.fill: parent
                    spacing: 2
                    visible: page.peaks.length > 0
                    Repeater {
                        model: page.peaks
                        Rectangle {
                            required property int index
                            required property var modelData
                            width: (seek.width - 2 * (page.peaks.length - 1)) / page.peaks.length
                            height: Math.max(3, seek.height * (0.12 + 0.88 * modelData / page.peakMax))
                            anchors.verticalCenter: parent.verticalCenter
                            radius: 1.5
                            color: index / page.peaks.length < seek.progress ? Theme.accent : "#3deaeef3"
                        }
                    }
                }
                Rectangle {
                    visible: page.peaks.length === 0
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width
                    height: 6
                    radius: 3
                    color: "#3deaeef3"
                    Rectangle { width: parent.width * seek.progress; height: parent.height; radius: 3; color: Theme.accent }
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    function to(x) { if (page.length > 0) Music.seek(Math.max(0, Math.min(1, x / width)) * page.length, false) }
                    onPressed: (m) => to(m.x)
                    onPositionChanged: (m) => { if (pressed) to(m.x) }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Label { text: Theme.time(Music.position); color: "#c9d2dd"; font.pixelSize: Theme.fontSmall }
                Item { Layout.fillWidth: true }
                Label { text: "−" + Theme.time(Math.max(0, page.length - Music.position)); color: "#c9d2dd"; font.pixelSize: Theme.fontSmall }
            }

            // ---- transport and volume ---------------------------------------------------------
            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                FocusButton {
                    id: shuffleButton
                    glyph: "⤮"
                    tip: Music.shuffle ? qsTr("Shuffle: on") : qsTr("Shuffle: off")
                    primary: Music.shuffle
                    KeyNavigation.right: prevButton
                    KeyNavigation.up: seek
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
                RoundButton {
                    id: playButton
                    implicitWidth: 84
                    implicitHeight: 84
                    text: Music.playing ? "❚❚" : "▶"
                    font.pixelSize: 26
                    Accessible.name: Music.playing ? qsTr("Pause") : qsTr("Play")
                    palette.button: Theme.accent
                    palette.buttonText: Theme.bg
                    focusPolicy: Qt.StrongFocus
                    KeyNavigation.left: prevButton
                    KeyNavigation.right: nextButton
                    KeyNavigation.up: seek
                    Keys.onReturnPressed: Music.togglePause()
                    Keys.onEnterPressed: Music.togglePause()
                    onClicked: Music.togglePause()
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: -5
                        radius: width / 2
                        color: "transparent"
                        border.width: playButton.activeFocus ? Theme.focusWidth + 1 : 0
                        border.color: Theme.focus
                    }
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
                    KeyNavigation.right: quieter
                    KeyNavigation.up: seek
                    onClicked: Music.cycleRepeat()
                }
                Item { Layout.fillWidth: true }
                // Volume: fixed at 100 % in bit-perfect mode (issue #9).
                FocusButton {
                    id: quieter
                    glyph: "−"
                    tip: qsTr("Quieter")
                    enabled: !Music.bitPerfect && Music.volume > 0
                    KeyNavigation.left: repeatButton
                    KeyNavigation.right: volumeSlider
                    KeyNavigation.up: seek
                    onClicked: Music.volume = Music.volume - 5
                }
                Slider {
                    id: volumeSlider
                    Layout.preferredWidth: 170
                    from: 0
                    to: 100
                    stepSize: 1
                    value: Music.bitPerfect ? 100 : Music.volume
                    enabled: !Music.bitPerfect
                    Accessible.name: qsTr("Volume")
                    focusPolicy: Qt.StrongFocus
                    onMoved: Music.volume = Math.round(value)
                    KeyNavigation.left: quieter
                    KeyNavigation.right: louder
                    KeyNavigation.up: seek
                    background: Rectangle {
                        x: volumeSlider.leftPadding
                        y: volumeSlider.topPadding + volumeSlider.availableHeight / 2 - height / 2
                        width: volumeSlider.availableWidth
                        height: 4
                        radius: 2
                        color: "#3deaeef3"
                        Rectangle { width: volumeSlider.visualPosition * parent.width; height: parent.height; radius: 2; color: Theme.accent }
                    }
                    handle: Rectangle {
                        x: volumeSlider.leftPadding + volumeSlider.visualPosition * (volumeSlider.availableWidth - width)
                        y: volumeSlider.topPadding + volumeSlider.availableHeight / 2 - height / 2
                        width: volumeSlider.activeFocus ? 20 : 14
                        height: width
                        radius: width / 2
                        color: Theme.text
                        border.width: volumeSlider.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                }
                FocusButton {
                    id: louder
                    glyph: "+"
                    tip: qsTr("Louder")
                    enabled: !Music.bitPerfect && Music.volume < 100
                    KeyNavigation.left: volumeSlider
                    KeyNavigation.right: queueList
                    KeyNavigation.up: seek
                    onClicked: Music.volume = Music.volume + 5
                }
                Label {
                    text: Music.bitPerfect ? qsTr("100 % (fixed)") : Music.volume + " %"
                    color: "#c9d2dd"
                    font.pixelSize: Theme.fontSmall
                    Layout.preferredWidth: 90
                }
            }
        }

        // ---- Up next ---------------------------------------------------------------------------
        Rectangle {
            Layout.preferredWidth: Math.min(380, page.width * 0.3)
            Layout.fillHeight: true
            color: "#cc0d1015"
            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 20
                spacing: 10
                RowLayout {
                    Layout.fillWidth: true
                    Label {
                        text: qsTr("Up next")
                        color: Theme.text
                        font.pixelSize: Theme.fontLarge
                        font.bold: true
                        Accessible.role: Accessible.Heading
                    }
                    Label {
                        Layout.fillWidth: true
                        text: qsTr("%n track(s)", "", Music.queueCount)
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                    }
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
                    Keys.onLeftPressed: louder.enabled ? louder.forceActiveFocus() : repeatButton.forceActiveFocus()
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
                        height: 56
                        focus: ListView.isCurrentItem
                        Accessible.name: modelData.title + (modelData.artists ? ", " + modelData.artists : "")
                        onClicked: Music.jump(modelData.index)
                        Keys.onReturnPressed: Music.jump(modelData.index)
                        Keys.onEnterPressed: Music.jump(modelData.index)
                        Keys.onPressed: (ev) => {
                            if (ev.key === Qt.Key_Menu || ev.key === Qt.Key_Delete) {
                                entryMenu.showFor(row, row.modelData.index, row.modelData.id, row.modelData.title, row.modelData.artists)
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
                            radius: 10
                            color: row.activeFocus ? Theme.surfaceHi : row.now ? "#1fe3a33b" : row.hovered ? Theme.surface : "transparent"
                            border.width: row.activeFocus ? Theme.focusWidth : 0
                            border.color: Theme.focus
                        }
                        contentItem: RowLayout {
                            spacing: 12
                            // Drag handle (mouse): drag up or down to move the entry.
                            Label {
                                text: "⋮⋮"
                                color: "#5b6574"
                                MouseArea {
                                    anchors.fill: parent
                                    anchors.margins: -6
                                    cursorShape: Qt.SizeVerCursor
                                    property real startY: 0
                                    onPressed: (m) => startY = m.y
                                    onReleased: (m) => {
                                        const steps = Math.round((m.y - startY) / row.height)
                                        const to = Math.max(0, Math.min(Music.queueCount - 1, row.modelData.index + steps))
                                        if (to !== row.modelData.index) Music.move(row.modelData.index, to)
                                    }
                                }
                            }
                            Image {
                                Layout.preferredWidth: 42
                                Layout.preferredHeight: 42
                                source: row.modelData.poster || ""
                                sourceSize.width: 84
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
                                    font.bold: true
                                    elide: Text.ElideRight
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: row.modelData.artists || ""
                                    color: Theme.textDim
                                    font.pixelSize: Theme.fontSmall
                                    elide: Text.ElideRight
                                }
                            }
                            // Issue #63: the queued version could not be reached; another one plays.
                            Rectangle {
                                visible: row.modelData.fallback === true
                                implicitWidth: fbText.implicitWidth + 10
                                implicitHeight: 20
                                radius: 5
                                color: Theme.surfaceHi
                                Label { id: fbText; anchors.centerIn: parent; text: qsTr("other version"); color: Theme.focus; font.pixelSize: 10; font.bold: true }
                                ToolTip.visible: fbHover.hovered
                                ToolTip.text: qsTr("The queued file cannot be reached; another version plays.")
                                HoverHandler { id: fbHover }
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
                        onClicked: {
                            if (!armed) { armed = true; return }
                            armed = false
                            Music.clear()
                        }
                    }
                }
                Label {
                    Layout.fillWidth: true
                    text: qsTr("Enter plays · Ctrl+↑/↓ moves · Delete removes · drag to reorder")
                    color: "#7d8794"
                    font.pixelSize: 12
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    Loader {
        id: fullView
        anchors.fill: parent
        active: page.full
        focus: page.full
        sourceComponent: MusicFullScreen {
            player: Music
            tint: page.tint
            onLeaveRequested: page.toggleFull()
        }
        onLoaded: Qt.callLater(() => { const v = item as MusicFullScreen; if (v) v.focusPlay() })
    }

    ActionMenu {
        id: entryMenu
        property int position: -1
        property var itemId: 0
        property Item row: null
        property string title: ""
        property string artist: ""
        function showFor(item, pos, id, title, artist) {
            position = pos
            row = item
            itemId = id
            entryMenu.title = title || ""
            entryMenu.artist = artist || ""
            show(item, [{ value: "play", label: qsTr("Play now") }, { value: "remove", label: qsTr("Remove from queue") },
                        { value: "playlist", label: qsTr("Add to playlist…") },
                        // Issues #103 and #104.
                        { value: "match", label: qsTr("Match again") }, { value: "search", label: qsTr("Search MusicBrainz…") }])
        }
        onChosen: (v) => {
            if (v === "play") Music.jump(position)
            else if (v === "playlist") { if (itemId) addToPlaylist.show([itemId], 0, row) }
            else if (v === "match") { if (itemId) musicMatch.again(itemId, "track", title, row) }
            else if (v === "search") { if (itemId) musicMatch.search(itemId, "track", title, artist, row) }
            else Music.remove(position)
        }
    }
    MusicMatch {
        id: musicMatch
        backend: Core
        onNotice: (text) => (page.Window.window as Main).showError(text)
        onSettingsRequested: (tab) => Nav.settings(tab)
    }
    // Issue #81.
    AddToPlaylist { id: addToPlaylist }
}
