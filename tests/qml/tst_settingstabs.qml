import QtQuick
import QtTest
import CoyotlUi

// Issue #117: the Settings tab bar: the tab follows focus along the bar, Down enters the section,
// Left on the first tab leaves for the rail, the current tab is filled.
Item {
    id: root
    width: 1000
    height: 200
    property string current: "matching"
    property var events: []

    SettingsTabs {
        id: bar
        width: 900
        tabs: [{ key: "metadata", label: "Metadata" }, { key: "matching", label: "Matching" }, { key: "about", label: "About" }]
        current: root.current
        onPicked: (key) => root.current = key
        onDownRequested: root.events = root.events.concat(["down"])
        onLeftEdge: root.events = root.events.concat(["rail"])
    }

    TestCase {
        name: "SettingsTabs"
        when: windowShown

        function tab(key) { return findChild(bar, "tab_" + key) }
        function init() { root.events = []; root.current = "matching" }

        function test_focusFollowsTheCurrentTab() {
            bar.focusCurrent()
            verify(tab("matching").activeFocus)
            verify(tab("matching").primary)
            verify(!tab("metadata").primary)
            keyClick(Qt.Key_Right)
            verify(tab("about").activeFocus)
            compare(root.current, "about")
            keyClick(Qt.Key_Right)
            compare(root.current, "about", "the last tab stays")
            keyClick(Qt.Key_Left)
            keyClick(Qt.Key_Left)
            compare(root.current, "metadata")
            keyClick(Qt.Key_Left)
            compare(root.events, ["rail"])
        }

        function test_downEntersTheSection() {
            bar.focusCurrent()
            keyClick(Qt.Key_Down)
            keyClick(Qt.Key_Return)
            compare(root.events, ["down", "down"])
        }

        function test_clickPicks() {
            mouseClick(tab("about"))
            compare(root.current, "about")
        }
    }
}
