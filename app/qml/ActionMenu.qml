pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic

// A small menu of actions for a row (issue #8: a track's Play next and Add to queue, a queue
// entry's Remove), opened with Menu or a click; styled like the Save menu. `chosen` names the
// action; focus goes back to the row.
Popup {
    id: menu
    property var actions: []
    property Item from: null
    signal chosen(string value)

    function show(item, list) {
        from = item
        actions = list
        parent = item
        x = Math.max(0, item.width - width - 12)
        y = item.height
        open()
    }

    width: 240
    height: list.contentHeight + 16
    margins: 12
    modal: true
    padding: 8
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onOpened: { list.currentIndex = 0; list.forceActiveFocus() }
    onClosed: if (from) from.forceActiveFocus()

    ListView {
        id: list
        anchors.fill: parent
        interactive: false
        model: menu.visible ? menu.actions : []
        keyNavigationEnabled: true
        Accessible.role: Accessible.List
        delegate: ItemDelegate {
            id: opt
            required property var modelData
            width: list.width
            height: 40
            focus: ListView.isCurrentItem
            text: modelData.label
            Accessible.name: text
            contentItem: Label { text: opt.text; color: Theme.text; elide: Text.ElideRight }
            background: Rectangle {
                radius: 6
                color: opt.activeFocus || opt.hovered ? Theme.surfaceHi : "transparent"
                border.width: opt.activeFocus ? 2 : 0
                border.color: Theme.focus
            }
            onClicked: {
                const v = modelData.value
                menu.close()
                menu.chosen(v)
            }
            Keys.onReturnPressed: clicked()
            Keys.onEnterPressed: clicked()
        }
    }
}
