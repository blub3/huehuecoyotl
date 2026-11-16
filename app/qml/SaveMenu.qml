pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic

// Save, Save as… and Delete saved copy of one movie or episode in one menu (issues #4, #5, #48).
// The button says where the saved copy stands; the menu offers what can be done now. Used on the
// movie and episode pages and on episode tiles. The page carries out the action (and asks first
// before deleting); `triggered` names it: save, pause, resume, retry, cancel, saveAgain, delete
// or export.
FocusButton {
    id: control
    // The item's SaveInfo: state none, queued, running, paused, done, failed or outdated.
    property var sv: ({ state: "none", saveable: 0 })
    // The selected file is on a network source (only those can be saved), the item has one, and
    // there is a file at all (Save as…).
    property bool networkFile: true
    readonly property bool canSave: networkFile && (sv.saveable || 0) > 0 || saveState !== "none"
    property bool canExport: true
    // The library still has the file (else an outdated copy cannot be saved again).
    property bool fileExists: true
    // Where focus goes when the menu closes (the button, or the tile that opened it with Menu).
    property Item returnFocus: control
    signal triggered(string action)

    readonly property string saveState: sv.state || "none"
    readonly property bool busy: saveState === "queued" || saveState === "running"
    readonly property int percent: sv.size > 0 ? Math.floor(100 * sv.bytesDone / sv.size) : 0

    function actions() {
        const out = []
        const add = (value, label) => out.push({ value: value, label: label })
        switch (saveState) {
        case "none": if (canSave) add("save", qsTr("Save")); break
        case "queued": case "running": add("pause", qsTr("Pause")); add("cancel", qsTr("Cancel")); break
        case "paused": add("resume", qsTr("Resume")); add("cancel", qsTr("Cancel")); break
        case "failed": add("retry", qsTr("Try again")); add("delete", qsTr("Delete saved copy")); break
        case "outdated":
            if (fileExists) add("saveAgain", qsTr("Save again"))
            add("delete", qsTr("Delete saved copy"))
            break
        case "done": add("delete", qsTr("Delete saved copy")); break
        }
        if (canExport) add("export", qsTr("Save as…"))
        return out
    }
    function label() {
        switch (saveState) {
        case "queued": return qsTr("Waiting to save")
        case "running": return qsTr("Saving %1 %").arg(percent)
        case "paused": return qsTr("Paused at %1 %").arg(percent)
        case "done": return qsTr("Saved")
        case "failed": return qsTr("Saving failed")
        case "outdated": return qsTr("Saved copy outdated")
        }
        return canSave ? qsTr("Save") : qsTr("Save as…")
    }
    // Opens the menu; with only Save as… on offer, the file dialog instead.
    function openMenu(from) {
        returnFocus = from || control
        const list = actions()
        if (list.length === 1 && list[0].value === "export" && saveState === "none") triggered("export")
        else if (list.length > 0) menu.open()
    }

    visible: canSave || canExport
    glyph: busy ? "❚❚" : saveState === "done" ? "✓" : "⤓"
    text: label() + (actions().length > 1 || saveState !== "none" ? "  ▾" : "")
    font.pixelSize: Theme.fontSmall
    Accessible.name: label()
    Accessible.description: qsTr("Save on this computer, or as a copy in a place you choose")
    onClicked: openMenu(control)

    Popup {
        id: menu
        objectName: "saveMenuPopup"
        y: control.height + 4
        width: 260
        height: list.contentHeight + 16
        margins: 12
        modal: true
        padding: 8
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: { list.currentIndex = 0; list.forceActiveFocus() }
        // Focus goes back where it came from, also when the chosen action changes the button.
        onClosed: (control.returnFocus || control).forceActiveFocus()
        ListView {
            id: list
            objectName: "saveMenuList"
            anchors.fill: parent
            interactive: false
            model: menu.visible ? control.actions() : []
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: control.label()
            delegate: ItemDelegate {
                id: opt
                required property var modelData
                width: list.width
                height: 40
                focus: ListView.isCurrentItem
                objectName: "saveAction_" + modelData.value
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
                    control.triggered(v)
                }
                Keys.onReturnPressed: clicked()
                Keys.onEnterPressed: clicked()
            }
        }
    }
}
