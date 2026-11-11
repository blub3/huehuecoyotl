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
    function openItem(itemId) { open(Qt.resolvedUrl("DetailPage.qml"), { itemId: itemId }) }
}
