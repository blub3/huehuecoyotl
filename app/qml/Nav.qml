pragma Singleton
import QtQuick

// Navigation entry points for every page: open a page in the content area, go back, or move
// keyboard focus to the navigation rail. The shell registers itself on start.
QtObject {
    property var shell: null

    function open(url, props) { if (shell) shell.openPage(url, props || {}) }
    function back() { if (shell) shell.back() }
    // Issue #121: swap the current page for another (Back then skips the one replaced).
    function replace(url, props) { if (shell) shell.replacePage(url, props || {}) }
    function go(section) { if (shell) shell.go(section) }
    // Issue #117: Settings on one of its tabs (metadata, matching, appearance, playback, audio,
    // storage, troubleshooting, about).
    function settings(tab) {
        if (tab) Appearance.settingsTab = tab
        go("settings")
    }
    function focusRail() { if (shell) shell.focusRail() }
    // Issue #133: from a page's first row, Up goes to the Back button; false when there is none.
    function focusBack() { return shell ? shell.focusBack() : false }
    function nowPlaying() { if (shell) shell.openNowPlaying() }
    // Music (2.0) has pages of its own: an album, an artist; a track opens its album.
    // Audiobooks (2.5): a book, a series, a person.
    function openItem(itemId, kind, parentId) {
        if (kind === "album") open(Qt.resolvedUrl("AlbumPage.qml"), { itemId: itemId })
        else if (kind === "artist") open(Qt.resolvedUrl("ArtistPage.qml"), { itemId: itemId })
        else if (kind === "track") open(Qt.resolvedUrl("AlbumPage.qml"), { itemId: parentId, focusTrack: itemId })
        // Issue #66: a book, and the books of a series or a person.
        else if (kind === "audiobook") open(Qt.resolvedUrl("BookPage.qml"), { itemId: itemId })
        else if (kind === "book_series" || kind === "person") open(Qt.resolvedUrl("BookListPage.qml"), { itemId: itemId })
        else open(Qt.resolvedUrl("DetailPage.qml"), { itemId: itemId })
    }
}
