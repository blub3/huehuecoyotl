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

    // Issue #83: on Now Playing, F11 enters and leaves its full-screen view.
    Shortcut {
        sequence: "F11"
        onActivated: { if (!(stack.currentItem instanceof Shell && (stack.currentItem as Shell).toggleFullView())) win.toggleFullscreen() }
    }

    // UI preferences (ui.ini in the data folder).
    Settings {
        id: prefs
        location: win.settingsFile
        category: "appearance"
        property string coverWindow: "medium"
        property string coverFullscreen: "large"
        // Issue #117: the Settings tab used last.
        property string settingsTab: "metadata"
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
        // Issue #66: audiobooks.
        property real bookSpeed: 1.0
        property int skipBack: 15
        property int skipForward: 30
    }
    Component.onCompleted: {
        Appearance.coverWindow = prefs.coverWindow
        Appearance.coverFullscreen = prefs.coverFullscreen
        Appearance.settingsTab = prefs.settingsTab
        Appearance.preferredVersion = playbackPrefs.preferredVersion
        Music.replayGain = playbackPrefs.replayGain
        Music.audioDevice = playbackPrefs.audioDevice
        Music.bitPerfect = playbackPrefs.bitPerfect
        Music.volume = playbackPrefs.musicVolume
        Book.defaultSpeed = playbackPrefs.bookSpeed
        Book.skipBack = playbackPrefs.skipBack
        Book.skipForward = playbackPrefs.skipForward
    }
    Connections {
        target: Book
        function onSettingsChanged() {
            playbackPrefs.bookSpeed = Book.defaultSpeed
            playbackPrefs.skipBack = Book.skipBack
            playbackPrefs.skipForward = Book.skipForward
        }
        function onFailed(message) { win.showError(message) }
    }
    Connections {
        target: Appearance
        function onCoverWindowChanged() { prefs.coverWindow = Appearance.coverWindow }
        function onCoverFullscreenChanged() { prefs.coverFullscreen = Appearance.coverFullscreen }
        function onSettingsTabChanged() { prefs.settingsTab = Appearance.settingsTab }
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
    // Issue #63: the preferred version also picks album versions.
    Binding { target: Music; property: "preference"; value: Appearance.preferredVersion }

    Toast { id: toast }
}
