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
        search: searchPage,
        sources: sourcesPage,
        settings: settingsPage
    })
    Component { id: homePage; HomePage {} }
    Component { id: moviesPage; MoviesPage {} }
    Component { id: showsPage; ShowsPage {} }
    Component { id: searchPage; SearchPage {} }
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

    Component.onCompleted: Nav.shell = shell
    // Returning from the player: give focus back to the page.
    StackView.onActivated: focusContent()

    NavRail {
        id: rail
        anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
        current: shell.section
        onChosen: (s) => { if (s === shell.section) { shell.content_toFirst() } else shell.go(s) }
        KeyNavigation.right: content
    }

    function content_toFirst() {
        if (content.depth > 1) content.pop(null)
        focusContent()
    }

    StackView {
        id: content
        anchors { left: rail.right; right: parent.right; top: parent.top; bottom: parent.bottom }
        focus: true
        initialItem: homePage
        pushEnter: null; pushExit: null; popEnter: null; popExit: null; replaceEnter: null; replaceExit: null
        // Pages handle Back themselves where they have something to undo (folders, menus);
        // otherwise Back leaves the page.
        Keys.onEscapePressed: shell.back()
        Keys.onBackPressed: shell.back()
    }
}
