pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// On-screen controls. Kept free of C++ types so Qt Quick Test can drive it with a stub player.
// `player` needs: paused, position, duration, cacheSeconds, volume, muted, speed, state,
// audioTracks, subtitleTracks, audioTrack, subtitleTrack, chapters and the matching methods.
FocusScope {
    id: controls
    property var player
    property bool fullscreen: false
    readonly property bool menuOpen: trackMenu.opened
    signal toggleFullscreen()
    signal interacted()

    implicitHeight: column.implicitHeight + 40

    function focusDefault() { playButton.forceActiveFocus() }

    function openMenu(kind, opener) {
        interacted()
        trackMenu.opener = opener
        if (kind === "audio") {
            trackMenu.heading = qsTr("Audio")
            trackMenu.options = player.audioTracks.map(t => ({ value: t.id, label: t.label }))
            trackMenu.current = player.audioTrack
        } else if (kind === "sub") {
            trackMenu.heading = qsTr("Subtitles")
            trackMenu.options = [{ value: 0, label: qsTr("Off") }].concat(
                player.subtitleTracks.map(t => ({ value: t.id, label: t.label })))
            trackMenu.current = player.subtitleTrack
        } else {
            trackMenu.heading = qsTr("Speed")
            trackMenu.options = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0].map(s => ({ value: s, label: s === 1 ? qsTr("Normal") : s + "×" }))
            trackMenu.current = player.speed
        }
        trackMenu.kind = kind
        trackMenu.open()
    }

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0; color: "transparent" }
            GradientStop { position: 0.35; color: "#b0000000" }
            GradientStop { position: 1; color: "#e0000000" }
        }
    }

    ColumnLayout {
        id: column
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom; margins: 28 }
        spacing: 10

        RowLayout {
            spacing: 14
            Label { text: Theme.time(controls.player.position); color: Theme.text; font.pixelSize: Theme.fontSmall; font.family: "monospace" }
            Slider {
                id: seekBar
                objectName: "seekBar"
                Layout.fillWidth: true
                from: 0
                to: Math.max(1, controls.player.duration)
                value: pressed ? value : controls.player.position
                focusPolicy: Qt.StrongFocus
                Accessible.name: qsTr("Position")
                KeyNavigation.down: playButton
                Keys.onLeftPressed: { controls.player.seek(-10); controls.interacted() }
                Keys.onRightPressed: { controls.player.seek(10); controls.interacted() }
                onMoved: { controls.player.seek(value, false); controls.interacted() }
                background: Rectangle {
                    x: seekBar.leftPadding
                    y: seekBar.topPadding + seekBar.availableHeight / 2 - height / 2
                    width: seekBar.availableWidth
                    height: seekBar.activeFocus ? 8 : 5
                    radius: 3
                    color: Qt.rgba(1, 1, 1, 0.18)
                    Rectangle {   // buffered
                        width: Math.min(parent.width, parent.width * (controls.player.position + controls.player.cacheSeconds) / seekBar.to)
                        height: parent.height; radius: 3; color: Qt.rgba(1, 1, 1, 0.22)
                    }
                    Rectangle { width: seekBar.visualPosition * parent.width; height: parent.height; radius: 3; color: Theme.accent }
                    Repeater {   // chapter marks
                        model: controls.player.chapters
                        Rectangle {
                            required property var modelData
                            x: parent.width * modelData.time / seekBar.to - 1
                            width: 2; height: parent.height; color: "#80000000"
                            visible: modelData.time > 0
                        }
                    }
                }
                handle: Rectangle {
                    x: seekBar.leftPadding + seekBar.visualPosition * (seekBar.availableWidth - width)
                    y: seekBar.topPadding + seekBar.availableHeight / 2 - height / 2
                    width: seekBar.activeFocus ? 22 : 16; height: width; radius: width / 2
                    color: Theme.text
                    border.width: seekBar.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
            }
            Label { text: Theme.time(controls.player.duration); color: Theme.textDim; font.pixelSize: Theme.fontSmall; font.family: "monospace" }
        }

        RowLayout {
            spacing: 10
            FocusButton {
                id: back10
                glyph: "↺"; text: "10"; tip: qsTr("Back 10 seconds")
                KeyNavigation.right: playButton; KeyNavigation.up: seekBar
                onClicked: { controls.player.seek(-10); controls.interacted() }
            }
            FocusButton {
                id: playButton
                objectName: "playButton"
                primary: true
                glyph: controls.player.paused || controls.player.state === 5 ? "▶" : "❚❚"
                tip: controls.player.paused ? qsTr("Play") : qsTr("Pause")
                KeyNavigation.left: back10; KeyNavigation.right: fwd10; KeyNavigation.up: seekBar
                onClicked: { controls.player.togglePause(); controls.interacted() }
            }
            FocusButton {
                id: fwd10
                glyph: "↻"; text: "10"; tip: qsTr("Forward 10 seconds")
                KeyNavigation.left: playButton; KeyNavigation.right: chapterButton.visible ? chapterButton : audioButton; KeyNavigation.up: seekBar
                onClicked: { controls.player.seek(10); controls.interacted() }
            }
            FocusButton {
                id: chapterButton
                visible: controls.player.chapters.length > 1
                glyph: "⏭"; tip: qsTr("Next chapter")
                KeyNavigation.left: fwd10; KeyNavigation.right: audioButton; KeyNavigation.up: seekBar
                onClicked: { controls.player.addChapter(1); controls.interacted() }
            }
            Item { Layout.fillWidth: true }
            FocusButton {
                id: audioButton
                objectName: "audioButton"
                text: qsTr("Audio")
                enabled: controls.player.audioTracks.length > 0
                KeyNavigation.left: chapterButton.visible ? chapterButton : fwd10; KeyNavigation.right: subButton; KeyNavigation.up: seekBar
                onClicked: controls.openMenu("audio", audioButton)
            }
            FocusButton {
                id: subButton
                objectName: "subButton"
                text: qsTr("Subtitles")
                KeyNavigation.left: audioButton; KeyNavigation.right: speedButton; KeyNavigation.up: seekBar
                onClicked: controls.openMenu("sub", subButton)
            }
            FocusButton {
                id: speedButton
                text: controls.player.speed === 1 ? qsTr("Speed") : controls.player.speed + "×"
                KeyNavigation.left: subButton; KeyNavigation.right: fsButton; KeyNavigation.up: seekBar
                onClicked: controls.openMenu("speed", speedButton)
            }
            FocusButton {
                id: fsButton
                glyph: controls.fullscreen ? "⤡" : "⤢"
                tip: qsTr("Fullscreen")
                KeyNavigation.left: speedButton; KeyNavigation.up: seekBar
                onClicked: { controls.toggleFullscreen(); controls.interacted() }
            }
        }
    }

    TrackMenu {
        id: trackMenu
        property string kind
        property Item opener
        parent: controls.parent
        onChosen: (value) => {
            if (kind === "audio") controls.player.audioTrack = value
            else if (kind === "sub") controls.player.subtitleTrack = value
            else controls.player.speed = value
            controls.interacted()
        }
        onClosed: if (opener) opener.forceActiveFocus(); else controls.focusDefault()
    }
}
