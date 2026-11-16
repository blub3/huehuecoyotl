import QtQuick
import QtQuick.Controls.Basic
import QtCore
import Coyotl

ApplicationWindow {
    id: win
    property bool startFullscreen: false
    property url settingsFile

    width: 1280
    height: 760
    minimumWidth: 640
    minimumHeight: 400
    visible: true
    visibility: startFullscreen ? Window.FullScreen : Window.AutomaticVisibility
    title: Playback.title !== "" && stack.currentItem instanceof PlayerPage
           ? Playback.title + " – Huehuecoyotl" : "Huehuecoyotl"
    color: Theme.bg
    font.pixelSize: Theme.fontBody

    function toggleFullscreen() {
        visibility = visibility === Window.FullScreen ? Window.Windowed : Window.FullScreen
    }
    function showError(message) { toast.show(message) }

    StackView {
        id: stack
        anchors.fill: parent
        focus: true
        initialItem: Shell {}
        // Instant transitions: they feel faster on a TV and avoid video re-layout mid-animation.
        pushEnter: null; pushExit: null; popEnter: null; popExit: null
    }

    Component { id: playerPage; PlayerPage {} }

    Connections {
        target: Playback
        function onStarted() {
            if (!(stack.currentItem instanceof PlayerPage)) stack.push(playerPage)
        }
        function onFailed(message) { win.showError(message) }
    }
    Connections {
        target: Music
        function onFailed(message) { win.showError(message) }
    }

    Shortcut { sequence: "F11"; onActivated: win.toggleFullscreen() }

    // UI preferences (ui.ini in the data folder).
    Settings {
        id: prefs
        location: win.settingsFile
        category: "appearance"
        property string coverWindow: "medium"
        property string coverFullscreen: "large"
    }
    Settings {
        id: playbackPrefs
        location: win.settingsFile
        category: "playback"
        property string preferredVersion: "quality"
        // Issue #8: off, track or album.
        property string replayGain: "off"
        // Issue #9: bit-perfect mode and the music output device ("auto": the system's).
        property bool bitPerfect: false
        property string audioDevice: "auto"
        property int musicVolume: 100
    }
    Component.onCompleted: {
        Appearance.coverWindow = prefs.coverWindow
        Appearance.coverFullscreen = prefs.coverFullscreen
        Appearance.preferredVersion = playbackPrefs.preferredVersion
        Music.replayGain = playbackPrefs.replayGain
        Music.audioDevice = playbackPrefs.audioDevice
        Music.bitPerfect = playbackPrefs.bitPerfect
        Music.volume = playbackPrefs.musicVolume
    }
    Connections {
        target: Appearance
        function onCoverWindowChanged() { prefs.coverWindow = Appearance.coverWindow }
        function onCoverFullscreenChanged() { prefs.coverFullscreen = Appearance.coverFullscreen }
        function onPreferredVersionChanged() { playbackPrefs.preferredVersion = Appearance.preferredVersion }
    }
    Connections {
        target: Music
        function onReplayGainChanged() { playbackPrefs.replayGain = Music.replayGain }
        function onOutputChanged() {
            playbackPrefs.bitPerfect = Music.bitPerfect
            playbackPrefs.audioDevice = Music.audioDevice
        }
        function onVolumeChanged() { if (!Music.bitPerfect) playbackPrefs.musicVolume = Music.volume }
    }
    Binding { target: Appearance; property: "fullscreen"; value: win.visibility === Window.FullScreen }

    Toast { id: toast }
}
