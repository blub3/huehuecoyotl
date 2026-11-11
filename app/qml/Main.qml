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

    Shortcut { sequence: "F11"; onActivated: win.toggleFullscreen() }

    // UI preferences (ui.ini in the data folder).
    Settings {
        id: prefs
        location: win.settingsFile
        category: "appearance"
        property string coverWindow: "medium"
        property string coverFullscreen: "large"
    }
    Component.onCompleted: {
        Appearance.coverWindow = prefs.coverWindow
        Appearance.coverFullscreen = prefs.coverFullscreen
    }
    Connections {
        target: Appearance
        function onCoverWindowChanged() { prefs.coverWindow = Appearance.coverWindow }
        function onCoverFullscreenChanged() { prefs.coverFullscreen = Appearance.coverFullscreen }
    }
    Binding { target: Appearance; property: "fullscreen"; value: win.visibility === Window.FullScreen }

    Toast { id: toast }
}
