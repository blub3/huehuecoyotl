pragma Singleton
import QtQuick

// Navigation entry points for every page: open a page in the content area, go back, or move
// keyboard focus to the navigation rail. The shell registers itself on start.
QtObject {
    property var shell: null

    function open(url, props) { if (shell) shell.openPage(url, props || {}) }
    function back() { if (shell) shell.back() }
    function go(section) { if (shell) shell.go(section) }
    function focusRail() { if (shell) shell.focusRail() }
    // Music (2.0) has pages of its own: an album, an artist; a track opens its album.
    function openItem(itemId, kind, parentId) {
        if (kind === "album") open(Qt.resolvedUrl("AlbumPage.qml"), { itemId: itemId })
        else if (kind === "artist") open(Qt.resolvedUrl("ArtistPage.qml"), { itemId: itemId })
        else if (kind === "track") open(Qt.resolvedUrl("AlbumPage.qml"), { itemId: parentId, focusTrack: itemId })
        else open(Qt.resolvedUrl("DetailPage.qml"), { itemId: itemId })
    }
}
