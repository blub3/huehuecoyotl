import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Player. Every control derives from Playback.player.state (SPEC 5.5).
FocusScope {
    id: page
    readonly property bool isPlayer: true
    readonly property MpvPlayer player: Playback.player
    readonly property bool active: player.state === MpvPlayer.Playing
    property bool osdVisible: true
    onOsdVisibleChanged: player.subtitleLift = osdVisible ? 120 : 0

    function showOsd() {
        osdVisible = true
        hideTimer.restart()
    }
    function hideOsd() {
        if (controls.menuOpen) return
        osdVisible = false
        page.forceActiveFocus()
    }
    function leave() {
        Playback.stop()
        StackView.view.pop()
    }

    focus: true
    Component.onCompleted: { showOsd(); page.forceActiveFocus() }

    Timer {
        id: hideTimer
        interval: 3500
        onTriggered: if (page.active && !controls.menuOpen) page.hideOsd()
    }
    Connections {
        target: Playback
        function onLeaveRequested() { page.leave() }
    }
    Connections {
        target: page.player
        function onStateChanged() {
            if (page.player.state !== MpvPlayer.Playing) page.showOsd()
            else hideTimer.restart()
        }
    }

    Rectangle { anchors.fill: parent; color: "black" }

    MpvVideoItem {
        id: video
        anchors.fill: parent
        player: page.player
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: page.osdVisible ? Qt.ArrowCursor : Qt.BlankCursor
        onPositionChanged: page.showOsd()
        onClicked: { page.player.togglePause(); page.showOsd() }
        onDoubleClicked: (Window.window as Main).toggleFullscreen()
    }

    // ---- keyboard / remote --------------------------------------------------------------------
    Keys.onPressed: (e) => {
        const osdHasFocus = controls.activeFocus
        switch (e.key) {
        case Qt.Key_Escape: case Qt.Key_Back: case Qt.Key_Backspace:
            if (osdHasFocus && page.osdVisible && page.active) page.hideOsd()
            else page.leave()
            break
        case Qt.Key_Space: case Qt.Key_K: case Qt.Key_MediaTogglePlayPause: case Qt.Key_MediaPlay: case Qt.Key_MediaPause:
            page.player.togglePause(); page.showOsd(); break
        case Qt.Key_Return: case Qt.Key_Enter:
            if (osdHasFocus) return
            page.player.togglePause(); page.showOsd(); break
        case Qt.Key_Left: case Qt.Key_J:
            if (osdHasFocus && e.key === Qt.Key_Left) return
            page.player.seek(-10); page.showOsd(); break
        case Qt.Key_Right: case Qt.Key_L:
            if (osdHasFocus && e.key === Qt.Key_Right) return
            page.player.seek(10); page.showOsd(); break
        case Qt.Key_Up: case Qt.Key_Down:
            if (osdHasFocus) return
            page.showOsd(); controls.focusDefault(); break
        case Qt.Key_F: (Window.window as Main).toggleFullscreen(); break
        case Qt.Key_M: page.player.muted = !page.player.muted; page.showOsd(); break
        case Qt.Key_Plus: case Qt.Key_Equal: case Qt.Key_VolumeUp: page.player.volume += 5; volumeHint.show(); break
        case Qt.Key_Minus: case Qt.Key_VolumeDown: page.player.volume -= 5; volumeHint.show(); break
        case Qt.Key_A: page.player.cycleAudio(); page.showOsd(); break
        case Qt.Key_S: case Qt.Key_V: page.player.cycleSubtitle(); page.showOsd(); break
        case Qt.Key_BracketLeft: page.player.speed = page.player.speed - 0.25; page.showOsd(); break
        case Qt.Key_BracketRight: page.player.speed = page.player.speed + 0.25; page.showOsd(); break
        case Qt.Key_PageUp: case Qt.Key_MediaPrevious: page.player.addChapter(-1); page.showOsd(); break
        case Qt.Key_PageDown: case Qt.Key_MediaNext: page.player.addChapter(1); page.showOsd(); break
        case Qt.Key_Home: Playback.startOver(); page.showOsd(); break
        case Qt.Key_Period: page.player.frameStep(false); break
        case Qt.Key_Comma: page.player.frameStep(true); break
        default: return
        }
        e.accepted = true
    }

    // ---- top bar ------------------------------------------------------------------------------
    Rectangle {
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: 88
        opacity: page.osdVisible ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 180 } }
        gradient: Gradient {
            GradientStop { position: 0; color: "#c0000000" }
            GradientStop { position: 1; color: "transparent" }
        }
        RowLayout {
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: 24 }
            spacing: 16
            FocusButton { glyph: "‹"; tip: qsTr("Back"); focusPolicy: Qt.NoFocus; onClicked: page.leave() }
            Label {
                Layout.fillWidth: true
                text: Playback.title
                color: Theme.text
                font.pixelSize: Theme.fontLarge
                elide: Text.ElideRight
            }
            Label { text: page.player.videoInfo; color: Theme.textDim; font.pixelSize: Theme.fontSmall }
        }
    }

    // ---- bottom controls ----------------------------------------------------------------------
    PlayerControls {
        id: controls
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        player: page.player
        fullscreen: Window.window ? Window.window.visibility === Window.FullScreen : false
        opacity: page.osdVisible ? 1 : 0
        visible: opacity > 0
        enabled: page.osdVisible
        Behavior on opacity { NumberAnimation { duration: 180 } }
        onToggleFullscreen: (Window.window as Main).toggleFullscreen()
        onInteracted: page.showOsd()
        onActiveFocusChanged: if (activeFocus) page.showOsd()
    }

    // ---- states -------------------------------------------------------------------------------
    BusyIndicator {
        anchors.centerIn: parent
        running: page.player.state === MpvPlayer.Opening || page.player.state === MpvPlayer.Buffering
        implicitWidth: 72; implicitHeight: 72
        Accessible.name: qsTr("Loading")
    }

    Rectangle {
        id: volumeHint
        function show() { opacity = 1; volTimer.restart() }
        anchors { right: parent.right; top: parent.top; margins: 32; topMargin: 100 }
        width: 180; height: 44; radius: Theme.radius
        color: Theme.scrim
        opacity: 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 150 } }
        Label {
            anchors.centerIn: parent
            text: page.player.muted ? qsTr("Muted") : qsTr("Volume %1%").arg(page.player.volume)
            color: Theme.text
        }
        Timer { id: volTimer; interval: 1200; onTriggered: volumeHint.opacity = 0 }
    }

    // Resume hint with a "start over" action, for the first seconds of a resumed file.
    Rectangle {
        id: resumeHint
        property bool shown: Playback.resumedFrom > 0 && page.player.position < Playback.resumedFrom + 8
        anchors { left: parent.left; top: parent.top; margins: 32; topMargin: 100 }
        width: resumeRow.implicitWidth + 28; height: 56; radius: Theme.radius
        color: Theme.scrim
        visible: shown && page.player.state !== MpvPlayer.Error
        RowLayout {
            id: resumeRow
            anchors.centerIn: parent
            spacing: 14
            Label { text: qsTr("Resumed at %1").arg(Theme.time(Playback.resumedFrom)); color: Theme.text }
            FocusButton { text: qsTr("Start over"); focusPolicy: Qt.NoFocus; onClicked: Playback.startOver() }
        }
    }

    Rectangle {
        anchors.centerIn: parent
        visible: page.player.state === MpvPlayer.Error
        width: Math.min(parent.width - 80, 560); height: errCol.implicitHeight + 48
        radius: Theme.radius
        color: Theme.surface
        ColumnLayout {
            id: errCol
            anchors.centerIn: parent
            width: parent.width - 48
            spacing: 16
            Label { Layout.fillWidth: true; text: page.player.errorString; color: Theme.text; wrapMode: Text.Wrap; horizontalAlignment: Text.AlignHCenter }
            FocusButton { Layout.alignment: Qt.AlignHCenter; text: qsTr("Back"); onClicked: page.leave() }
        }
    }
}
