pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic

// Focusable choice list for audio tracks, subtitles and speed.
Popup {
    id: menu
    property string heading
    property var options: []        // [{ value, label }]
    property var current
    signal chosen(var value)

    modal: true
    focus: true
    padding: 12
    width: 420
    height: Math.min(parent.height - 120, list.contentHeight + headingLabel.height + 36)
    x: parent.width - width - 32
    y: parent.height - height - 150
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    onOpened: {
        let i = 0
        for (let k = 0; k < options.length; ++k) if (options[k].value === current) i = k
        list.currentIndex = i
        list.forceActiveFocus()
    }

    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }

    contentItem: Column {
        spacing: 8
        Label { id: headingLabel; text: menu.heading; color: Theme.textDim; font.pixelSize: Theme.fontSmall; leftPadding: 6 }
        ListView {
            id: list
            width: parent.width
            height: menu.height - headingLabel.height - 36
            clip: true
            model: menu.options
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: menu.heading
            delegate: ItemDelegate {
                id: opt
                required property int index
                required property var modelData
                width: ListView.view.width
                height: 44
                focus: ListView.isCurrentItem
                Accessible.name: opt.modelData.label
                onClicked: { menu.chosen(opt.modelData.value); menu.close() }
                Keys.onReturnPressed: clicked()
                Keys.onEnterPressed: clicked()
                background: Rectangle {
                    radius: 8
                    color: opt.activeFocus || opt.hovered ? Theme.surfaceHi : "transparent"
                    border.width: opt.activeFocus ? 2 : 0
                    border.color: Theme.focus
                }
                contentItem: Row {
                    spacing: 10
                    Label { width: 18; text: opt.modelData.value === menu.current ? "●" : ""; color: Theme.accent }
                    Label { text: opt.modelData.label; color: Theme.text; elide: Text.ElideRight; width: opt.width - 60 }
                }
            }
        }
    }
}
