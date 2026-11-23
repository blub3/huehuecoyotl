pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// Full-screen Now Playing for the living room (issue #83): a large cover, the title in large
// type, the format, a progress bar, large controls, an Up next card and a line of key hints,
// all scaled to the screen (designed at 1920 × 1080). After `idleMs` without input the
// controls, the card and the hints fade out; cover, title and progress stay. The first key or
// click after that only brings them back.
//
// Keys on Play / Pause: Enter plays or pauses, ← → seek 10 s, ↑ reaches the other controls,
// ↓ opens the queue, Back leaves. `player` is the music player (a stub in the tests).
FocusScope {
    id: view
    objectName: "musicFullScreen"
    required property var player
    property string tint: ""
    property int idleMs: 5000
    property bool awake: true
    // Issue #191: the shell's Back bar is in use (pointer on it, focus, its trail): stay awake.
    property bool held: false
    onHeldChanged: idle.restart()
    // The view's ground; the shell's Back bar takes it too, so it fades without an edge.
    readonly property color ground: tint !== "" ? Qt.darker(tint, 1.25) : "#0b1218"
    readonly property bool queueOpen: queuePanel.visible
    signal leaveRequested()

    readonly property real s: Math.max(0.4, Math.min(width / 1920, height / 1080))
    readonly property var e: player.current || ({})
    readonly property double length: player.duration > 0 ? player.duration : (e.durationMs || 0) / 1000
    readonly property var nextEntry: {
        const q = player.queue || []
        const i = player.queuePosition - player.queueOffset + 1
        return i >= 0 && i < q.length ? q[i] : null
    }
    readonly property int following: Math.max(0, player.queueCount - player.queuePosition - 1)

    // While faded, `catcher` holds the focus so the first key only wakes the view; waking gives
    // the focus back to where it was.
    property Item resumeFocus: null
    function wake() {
        idle.restart()
        if (awake) return
        awake = true
        if (resumeFocus) resumeFocus.forceActiveFocus()
        else focusPlay()
        resumeFocus = null
    }
    function sleep() {
        if (queueOpen || !awake || held) return
        const w = view.Window.window
        resumeFocus = w && w.activeFocusItem && view.activeFocus ? w.activeFocusItem : null
        awake = false
        if (view.activeFocus) catcher.forceActiveFocus()
    }
    function focusPlay() { playButton.forceActiveFocus() }
    function openQueue() {
        queuePanel.visible = true
        queueList.currentIndex = Math.max(0, player.queuePosition - player.queueOffset)
        queueList.positionViewAtIndex(queueList.currentIndex, ListView.Center)
        queueList.forceActiveFocus()
    }
    function closeQueue() {
        queuePanel.visible = false
        playButton.forceActiveFocus()
    }

    Component.onCompleted: { wake(); Qt.callLater(focusPlay) }

    Timer {
        id: idle
        interval: view.idleMs
        onTriggered: view.sleep()
    }
    Item {
        id: catcher
        objectName: "wakeCatcher"
        Keys.onPressed: (ev) => { view.wake(); ev.accepted = true }
    }

    // Moving the focus or acting keeps the view awake (keys the controls take never reach this
    // scope's own handler).
    Connections {
        target: view.Window.window
        function onActiveFocusItemChanged() { if (view.awake) idle.restart() }
    }

    // Other keys keep the view awake too; Back closes the queue or leaves.
    Keys.onPressed: (ev) => {
        idle.restart()
        if (ev.key === Qt.Key_Escape || ev.key === Qt.Key_Back) {
            if (view.queueOpen) view.closeQueue()
            else view.leaveRequested()
            ev.accepted = true
            return
        }
        ev.accepted = false
    }

    Rectangle {
        anchors.fill: parent
        color: view.ground
        Behavior on color { ColorAnimation { duration: 400 } }
    }

    // ---- what stays ----------------------------------------------------------------------
    Rectangle {
        id: coverBox
        width: 760 * view.s
        height: width
        x: 120 * view.s
        anchors.verticalCenter: parent.verticalCenter
        radius: 18 * view.s
        color: "#26ffffff"
        clip: true
        Label {
            anchors.centerIn: parent
            visible: cover.status !== Image.Ready
            text: "♫"
            color: Theme.textDim
            font.pixelSize: 160 * view.s
        }
        Image {
            id: cover
            anchors.fill: parent
            source: view.e.posterFull || view.e.poster || ""
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            sourceSize.width: 1000
        }
    }

    ColumnLayout {
        id: side
        anchors.left: coverBox.right
        anchors.leftMargin: 90 * view.s
        anchors.right: parent.right
        anchors.rightMargin: 120 * view.s
        anchors.verticalCenter: parent.verticalCenter
        spacing: 18 * view.s

        RowLayout {
            spacing: 10 * view.s
            visible: (view.e.codec || "") !== ""
            Rectangle {
                visible: view.e.hires === true
                implicitWidth: hiresText.implicitWidth + 22 * view.s
                implicitHeight: 38 * view.s
                radius: 8 * view.s
                color: Theme.accent
                Label { id: hiresText; anchors.centerIn: parent; text: qsTr("Hi-Res"); color: Theme.bg; font.pixelSize: 20 * view.s; font.bold: true }
            }
            Rectangle {
                implicitWidth: fmtText.implicitWidth + 22 * view.s
                implicitHeight: 38 * view.s
                radius: 8 * view.s
                color: "#80000000"
                Label {
                    id: fmtText
                    anchors.centerIn: parent
                    text: MediaInfo.audioLine(view.e.codec || "", view.e.sampleRate || 0, view.e.bitDepth || 0)
                    color: Theme.text
                    font.pixelSize: 20 * view.s
                    font.bold: true
                }
            }
        }
        Label {
            objectName: "fullTitle"
            Layout.fillWidth: true
            text: view.e.title || ""
            color: Theme.text
            font.pixelSize: 76 * view.s
            font.bold: true
            wrapMode: Text.Wrap
            maximumLineCount: 2
            elide: Text.ElideRight
            lineHeight: 1.02
            Accessible.role: Accessible.Heading
        }
        Label {
            Layout.fillWidth: true
            text: [view.e.artists || "", view.e.album || ""].filter(x => x).join("  ·  ")
            color: "#d5dce5"
            font.pixelSize: 34 * view.s
            elide: Text.ElideRight
        }

        // Progress: elapsed and remaining.
        Item {
            id: progressBar
            Layout.fillWidth: true
            Layout.topMargin: 24 * view.s
            Layout.preferredHeight: 12 * view.s
            readonly property real progress: view.length > 0 ? Math.min(1, view.player.position / view.length) : 0
            Accessible.role: Accessible.ProgressBar
            Accessible.name: qsTr("Position")
            Rectangle {
                anchors.fill: parent
                radius: height / 2
                color: "#3deaeef3"
                Rectangle { width: parent.width * progressBar.progress; height: parent.height; radius: parent.radius; color: Theme.accent }
            }
        }
        RowLayout {
            Layout.fillWidth: true
            Label { text: Theme.time(view.player.position); color: "#d5dce5"; font.pixelSize: 26 * view.s }
            Item { Layout.fillWidth: true }
            Label { text: "−" + Theme.time(Math.max(0, view.length - view.player.position)); color: "#d5dce5"; font.pixelSize: 26 * view.s }
        }

        // ---- what fades ------------------------------------------------------------------
        RowLayout {
            id: controls
            objectName: "fullControls"
            Layout.topMargin: 20 * view.s
            spacing: 26 * view.s
            opacity: view.awake ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 600 } }

            component BigButton: FocusButton {
                id: big
                font.pixelSize: 34 * view.s
                implicitHeight: 92 * view.s
                implicitWidth: 92 * view.s
                horizontalPadding: 0
                contentItem: Text {
                    text: big.glyph
                    color: Theme.text
                    font.pixelSize: 36 * view.s
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                // ↑ from Play / Pause lands here; ↓ goes back.
                Keys.onDownPressed: view.focusPlay()
            }

            BigButton {
                id: shuffleButton
                objectName: "fullShuffle"
                glyph: "⤮"
                tip: view.player.shuffle ? qsTr("Shuffle: on") : qsTr("Shuffle: off")
                primary: view.player.shuffle
                KeyNavigation.right: prevButton
                onClicked: { view.wake(); view.player.shuffle = !view.player.shuffle }
            }
            BigButton {
                id: prevButton
                objectName: "fullPrevious"
                glyph: "⏮"
                tip: qsTr("Previous")
                KeyNavigation.left: shuffleButton
                KeyNavigation.right: nextButton
                onClicked: { view.wake(); view.player.previous() }
            }
            RoundButton {
                id: playButton
                objectName: "fullPlay"
                implicitWidth: 132 * view.s
                implicitHeight: 132 * view.s
                text: view.player.playing ? "❚❚" : "▶"
                font.pixelSize: 44 * view.s
                Accessible.name: view.player.playing ? qsTr("Pause") : qsTr("Play")
                palette.button: Theme.accent
                palette.buttonText: Theme.bg
                focusPolicy: Qt.StrongFocus
                Keys.onReturnPressed: { view.wake(); view.player.togglePause() }
                Keys.onEnterPressed: { view.wake(); view.player.togglePause() }
                Keys.onLeftPressed: { view.wake(); view.player.seek(-10, true) }
                Keys.onRightPressed: { view.wake(); view.player.seek(10, true) }
                Keys.onUpPressed: prevButton.forceActiveFocus()
                Keys.onDownPressed: view.openQueue()
                onClicked: { view.wake(); view.player.togglePause() }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: -8 * view.s
                    radius: width / 2
                    color: "transparent"
                    border.width: playButton.activeFocus ? Theme.focusWidth + 2 : 0
                    border.color: Theme.focus
                }
            }
            BigButton {
                id: nextButton
                objectName: "fullNext"
                glyph: "⏭"
                tip: qsTr("Next")
                KeyNavigation.left: prevButton
                KeyNavigation.right: repeatButton
                onClicked: { view.wake(); view.player.next() }
            }
            BigButton {
                id: repeatButton
                objectName: "fullRepeat"
                glyph: view.player.repeat === "one" ? "↻1" : "↻"
                tip: view.player.repeat === "one" ? qsTr("Repeat: this track")
                   : view.player.repeat === "all" ? qsTr("Repeat: all") : qsTr("Repeat: off")
                primary: view.player.repeat !== "off"
                KeyNavigation.left: nextButton
                onClicked: { view.wake(); view.player.cycleRepeat() }
            }
        }

        // Up next: the next entry and how many follow.
        Rectangle {
            objectName: "upNextCard"
            Layout.fillWidth: true
            Layout.maximumWidth: 720 * view.s
            Layout.topMargin: 26 * view.s
            visible: view.nextEntry !== null
            opacity: view.awake ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 600 } }
            implicitHeight: 100 * view.s
            radius: 14 * view.s
            color: "#99000000"
            RowLayout {
                anchors.fill: parent
                anchors.margins: 14 * view.s
                spacing: 18 * view.s
                Image {
                    Layout.preferredWidth: 72 * view.s
                    Layout.preferredHeight: 72 * view.s
                    source: view.nextEntry ? view.nextEntry.poster || "" : ""
                    sourceSize.width: 144
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2 * view.s
                    Label {
                        text: qsTr("Up next")
                        color: Theme.accent
                        font.pixelSize: 18 * view.s
                        font.capitalization: Font.AllUppercase
                        font.letterSpacing: 1
                    }
                    Label {
                        Layout.fillWidth: true
                        text: view.nextEntry ? [view.nextEntry.title, view.nextEntry.artists || ""].filter(x => x).join("  ·  ") : ""
                        color: Theme.text
                        font.pixelSize: 26 * view.s
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    Label {
                        visible: view.following > 1
                        text: qsTr("and %n more", "", view.following - 1)
                        color: "#c9d2dd"
                        font.pixelSize: 20 * view.s
                    }
                }
            }
        }
    }

    Label {
        objectName: "keyHints"
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 40 * view.s
        opacity: view.awake ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 600 } }
        text: playButton.activeFocus
              ? qsTr("Enter play / pause  ·  ← → −10 / +10 s  ·  ↑ more controls  ·  ↓ queue  ·  Back leave")
              : qsTr("← → choose  ·  Enter select  ·  ↓ play / pause  ·  Back leave")
        color: "#aab3bf"
        font.pixelSize: 22 * view.s
    }

    // ---- the queue overlay (↓) --------------------------------------------------------------
    Rectangle {
        id: queuePanel
        objectName: "fullQueue"
        visible: false
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: Math.max(420, 640 * view.s)
        color: "#ee0d1015"
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 36 * view.s
            spacing: 14 * view.s
            Label {
                text: qsTr("Queue")
                color: Theme.text
                font.pixelSize: 40 * view.s
                font.bold: true
                Accessible.role: Accessible.Heading
            }
            ListView {
                id: queueList
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: view.player.queue
                spacing: 4
                keyNavigationEnabled: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Queue")
                Keys.onLeftPressed: view.closeQueue()
                delegate: ItemDelegate {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property bool now: modelData.index === view.player.queuePosition
                    width: queueList.width
                    height: 76 * view.s
                    focus: ListView.isCurrentItem
                    Accessible.name: modelData.title + (modelData.artists ? ", " + modelData.artists : "")
                    function play() { view.player.jump(modelData.index); view.closeQueue() }
                    onClicked: play()
                    Keys.onReturnPressed: play()
                    Keys.onEnterPressed: play()
                    background: Rectangle {
                        radius: 10
                        color: row.activeFocus ? Theme.surfaceHi : row.now ? "#1fe3a33b" : "transparent"
                        border.width: row.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: ColumnLayout {
                        spacing: 0
                        Label {
                            Layout.fillWidth: true
                            text: row.modelData.title
                            color: row.now ? Theme.accent : Theme.text
                            font.pixelSize: 26 * view.s
                            font.bold: true
                            elide: Text.ElideRight
                        }
                        Label {
                            Layout.fillWidth: true
                            text: row.modelData.artists || ""
                            color: Theme.textDim
                            font.pixelSize: 20 * view.s
                            elide: Text.ElideRight
                        }
                    }
                }
            }
            Label {
                text: qsTr("Enter plays  ·  Back or ← closes")
                color: "#7d8794"
                font.pixelSize: 18 * view.s
            }
        }
    }

    // The mouse: moving wakes the view (a passive handler, so the buttons keep their hover); while
    // faded, a click only wakes it.
    // Qt also sends hover events when the scene repaints under a still pointer; only a real move
    // counts.
    HoverHandler {
        property point last: Qt.point(-1, -1)
        onPointChanged: {
            const p = point.scenePosition
            if (Math.abs(p.x - last.x) < 2 && Math.abs(p.y - last.y) < 2) return
            last = p
            view.wake()
        }
    }
    MouseArea {
        objectName: "wakeArea"
        anchors.fill: parent
        z: 10
        enabled: !view.awake
        acceptedButtons: Qt.AllButtons
        onPressed: view.wake()
        onWheel: view.wake()
    }
}
