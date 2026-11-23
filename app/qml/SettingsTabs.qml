pragma ComponentBehavior: Bound
import QtQuick

// The tab bar of Settings (issue #117): one tab per section, the current one filled. The tab
// changes as focus moves along the bar (Left and Right), as in the Music top menu; Down, Enter or
// Return enter the section; Left on the first tab leaves for the navigation rail.
Flow {
    id: bar
    // [{ key, label }]
    property var tabs: []
    property string current: ""
    signal picked(string key)
    signal downRequested()
    signal leftEdge()
    function focusCurrent() {
        const b = buttons.itemAt(Math.max(0, tabs.findIndex(t => t.key === current)))
        if (b) b.forceActiveFocus()
    }

    spacing: 6
    Accessible.role: Accessible.PageTabList
    Accessible.name: qsTr("Settings sections")
    Repeater {
        id: buttons
        model: bar.tabs
        FocusButton {
            id: tabButton
            required property var modelData
            required property int index
            objectName: "tab_" + modelData.key
            text: modelData.label
            primary: bar.current === modelData.key
            // Nine tabs on one line in an 860 px column.
            horizontalPadding: 9
            font.pixelSize: 14
            Accessible.role: Accessible.PageTab
            Accessible.checkable: true
            Accessible.checked: primary
            onActiveFocusChanged: if (activeFocus) bar.picked(tabButton.modelData.key)
            onClicked: bar.picked(tabButton.modelData.key)
            Keys.onLeftPressed: {
                if (tabButton.index === 0) bar.leftEdge()
                else buttons.itemAt(tabButton.index - 1).forceActiveFocus()
            }
            Keys.onRightPressed: {
                if (tabButton.index < buttons.count - 1) buttons.itemAt(tabButton.index + 1).forceActiveFocus()
            }
            Keys.onDownPressed: bar.downRequested()
            Keys.onReturnPressed: bar.downRequested()
            Keys.onEnterPressed: bar.downRequested()
        }
    }
}
