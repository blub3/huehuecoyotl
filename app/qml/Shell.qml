import QtQuick
import QtQuick.Controls.Basic
import Coyotl

// The persistent shell (SPEC 9): navigation rail plus a content StackView per section.
// The player is pushed above the shell by Main.
FocusScope {
    id: shell
    readonly property bool isPlayer: false
    property string section: "home"

    readonly property var pages: ({
        home: homePage,
        movies: moviesPage,
        shows: showsPage,
        music: musicPage,
        audiobooks: audiobooksPage,
        search: searchPage,
        downloads: downloadsPage,
        sources: sourcesPage,
        settings: settingsPage
    })
    Component { id: homePage; HomePage {} }
    Component { id: moviesPage; MoviesPage {} }
    Component { id: showsPage; ShowsPage {} }
    Component { id: musicPage; MusicPage {} }
    Component { id: audiobooksPage; AudiobooksPage {} }
    Component { id: searchPage; SearchPage {} }
    Component { id: downloadsPage; DownloadsPage {} }
    Component { id: sourcesPage; SourcesPage {} }
    Component { id: settingsPage; SettingsPage {} }

    function go(s) {
        section = s
        content.replace(null, pages[s])
        focusContent()
    }
    function openPage(url, props) {
        content.push(url, props)
        focusContent()
    }
    // Back: pop a page, or from a section's first page move to the rail.
    function back() {
        if (content.depth > 1) {
            content.pop()
            focusContent()
        } else {
            focusRail()
        }
    }
    function focusRail() { rail.focusCurrent() }
    function focusContent() { Qt.callLater(() => { if (content.currentItem) content.currentItem.forceActiveFocus() }) }

    Component.onCompleted: {
        Nav.shell = shell
        if (Bench.mode === "scroll") shell.go("movies")
        if (Bench.mode === "episodes") shell.go("shows")
        if (Bench.mode === "music") shell.go("music")
        if (Bench.mode === "books") shell.go("audiobooks")
    }
    // Returning from the player: give focus back to the page.
    StackView.onActivated: focusContent()

    // Issue #8: Now Playing opens above the current section's pages.
    function openNowPlaying() {
        if (content.currentItem && content.currentItem.objectName === "nowPlaying") focusContent()
        else if (Book.front && Book.active) openPage(Qt.resolvedUrl("BookPlayerPage.qml"), {})
        else openPage(Qt.resolvedUrl("NowPlayingPage.qml"), {})
    }
    // Media keys and Space-free shortcuts for music while the shell is in front (the player
    // page handles them itself while a video plays).
    Keys.onPressed: (e) => {
        switch (e.key) {
        // Issue #66: the book when it played last; next and previous are its chapters.
        case Qt.Key_MediaPlay: if (Book.front) Book.play(); else Music.play(); break
        case Qt.Key_MediaPause: if (Book.front) Book.pause(); else Music.pause(); break
        case Qt.Key_MediaTogglePlayPause: if (Book.front) Book.togglePause(); else Music.togglePause(); break
        case Qt.Key_MediaNext: if (Book.front) Book.nextChapter(); else Music.next(); break
        case Qt.Key_MediaPrevious: if (Book.front) Book.previousChapter(); else Music.previous(); break
        default: return
        }
        e.accepted = true
    }

    NavRail {
        id: rail
        anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
        current: shell.section
        onChosen: (s) => { if (s === shell.section) { shell.content_toFirst() } else shell.go(s) }
        onDownFromLast: if (mini.visible) mini.forceActiveFocus()
        KeyNavigation.right: content
    }

    function content_toFirst() {
        if (content.depth > 1) content.pop(null)
        focusContent()
    }

    MiniPlayer {
        id: mini
        anchors { left: rail.right; right: parent.right; bottom: parent.bottom }
        visible: (Music.active || Book.active) && !(content.currentItem && content.currentItem.objectName === "nowPlaying")
        height: visible ? implicitHeight : 0
        onOpenRequested: shell.openNowPlaying()
        onUpRequested: shell.focusContent()
        onLeftRequested: shell.focusRail()
    }

    StackView {
        id: content
        anchors { left: rail.right; right: parent.right; top: parent.top; bottom: mini.top }
        focus: true
        initialItem: homePage
        pushEnter: null; pushExit: null; popEnter: null; popExit: null; replaceEnter: null; replaceExit: null
        // Pages handle Back themselves where they have something to undo (folders, menus);
        // otherwise Back leaves the page.
        Keys.onEscapePressed: shell.back()
        Keys.onBackPressed: shell.back()
    }
}
