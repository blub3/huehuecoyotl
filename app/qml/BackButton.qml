pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #133: the one Back button of every stacked page. The shell places it at the top left of
// the content; it names the page it goes back to (that page's `pageTitle`, else its `pageKind`)
// and is hidden on a section's first page. Dark glass, so it reads over pictures and plain
// ground alike; the app's amber focus ring; amber while pressed.
Button {
    id: button
    // The StackView whose pages it walks back through.
    property StackView stack: null
    // var: pages add `pageTitle` and `pageKind` of their own.
    readonly property var below: stack && stack.depth > 1 ? stack.get(stack.depth - 2, StackView.DontLoad) : null
    readonly property string target: {
        if (!below) return ""
        const t = below.pageTitle
        if (t) return String(t)
        const k = below.pageKind
        return k ? String(k) : qsTr("Back")
    }
    signal backRequested()
    // Issue #135: back to the page at this stack index in one step (the pages above it close).
    signal backToRequested(int index)

    function titleOf(p) {
        if (!p) return ""
        if (p.pageTitle) return String(p.pageTitle)
        return p.pageKind ? String(p.pageKind) : ""
    }
    // Issue #136: the other keys that mean Back: Backspace (never while typing in a text field)
    // and Alt+Left. Esc and the remote's Back key are handled with their hold (the trail).
    function isBackKey(key, modifiers, focusItem) {
        if (key === Qt.Key_Left && (modifiers & Qt.AltModifier)) return true
        if (key !== Qt.Key_Backspace || modifiers !== Qt.NoModifier) return false
        return !(focusItem && focusItem.cursorPosition !== undefined)
    }
    function kindOf(p) { return p && p.pageKind ? String(p.pageKind) : "" }
    // The way here, bottom first: "Albums › Test Artist › First Album".
    readonly property string path: {
        if (!stack || stack.depth < 2) return ""
        const parts = []
        for (let i = 0; i < stack.depth; ++i) {
            const t = titleOf(stack.get(i, StackView.DontLoad))
            if (t !== "") parts.push(t)
        }
        return parts.join("  ›  ")
    }
    // The trail: this page (dimmed), then the pages below it, nearest first.
    property var trail: []
    readonly property bool trailOpen: trailPopup.opened
    // Opened by holding Esc or Back: that key's release (and its repeats) must not close it.
    property bool keyHeld: false
    function openTrail(byHold) {
        keyHeld = byHold === true
        if (!stack || stack.depth < 2) return
        const rows = []
        for (let i = stack.depth - 1; i >= 0; --i) {
            const p = stack.get(i, StackView.DontLoad)
            rows.push({ index: i, title: titleOf(p) || qsTr("Back"), kind: kindOf(p), here: i === stack.depth - 1 })
        }
        trail = rows
        trailPopup.open()
        trailList.currentIndex = rows.length > 1 ? 1 : 0
        trailList.forceActiveFocus()
    }
    function choose(row) {
        trailPopup.close()
        if (row && !row.here) backToRequested(row.index)
    }

    visible: below !== null
    focusPolicy: Qt.StrongFocus
    implicitHeight: 48
    implicitWidth: Math.min(300, row.implicitWidth + leftPadding + rightPadding)
    leftPadding: 12
    rightPadding: 20
    Accessible.name: qsTr("Back to %1").arg(target)
    Keys.onReturnPressed: clicked()
    Keys.onEnterPressed: clicked()
    Keys.onEscapePressed: (e) => { if (!e.isAutoRepeat) backRequested() }
    Keys.onDownPressed: openTrail()
    onClicked: backRequested()
    onPressAndHold: openTrail()
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: button.openTrail()
    }

    Popup {
        id: trailPopup
        objectName: "trail"
        y: button.height + 8
        width: 340
        padding: 8
        modal: false
        focus: true
        // Esc closes it on release (see the list), so a held Esc that opened it does not.
        closePolicy: Popup.CloseOnPressOutside
        onClosed: if (button.visible) button.forceActiveFocus()
        background: Rectangle {
            radius: 14
            color: "#1b2029"
            border.width: 1
            border.color: "#2c3442"
        }
        contentItem: ColumnLayout {
            spacing: 2
            Label {
                text: qsTr("Go back to")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                font.capitalization: Font.AllUppercase
                leftPadding: 12
                topPadding: 6
                bottomPadding: 4
            }
            ListView {
                id: trailList
                objectName: "trailList"
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(contentHeight, 6 * 46)
                clip: true
                model: button.trail
                keyNavigationEnabled: true
                Keys.onReturnPressed: button.choose(button.trail[currentIndex])
                Keys.onEnterPressed: button.choose(button.trail[currentIndex])
                Keys.onLeftPressed: trailPopup.close()
                Keys.onPressed: (e) => { if (e.key === Qt.Key_Escape || e.key === Qt.Key_Back) e.accepted = true }
                Keys.onReleased: (e) => {
                    if (e.key !== Qt.Key_Escape && e.key !== Qt.Key_Back) return
                    e.accepted = true
                    if (e.isAutoRepeat) return
                    if (button.keyHeld) button.keyHeld = false
                    else trailPopup.close()
                }
                delegate: ItemDelegate {
                    id: trailRow
                    required property var modelData
                    required property int index
                    objectName: "trailRow" + index
                    width: ListView.view.width
                    height: 44
                    enabled: !modelData.here
                    Accessible.name: modelData.here ? qsTr("%1 (here)").arg(modelData.title) : modelData.title
                    onClicked: button.choose(trailRow.modelData)
                    background: Rectangle {
                        radius: 9
                        color: trailRow.ListView.isCurrentItem && !trailRow.modelData.here ? "#2a3140" : "transparent"
                        border.width: trailRow.ListView.isCurrentItem && trailList.activeFocus && !trailRow.modelData.here ? 2 : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        spacing: 10
                        Label {
                            Layout.fillWidth: true
                            text: trailRow.modelData.title
                            elide: Text.ElideRight
                            color: trailRow.modelData.here ? Theme.textDim : Theme.text
                            font.pixelSize: Theme.fontBody
                        }
                        Label {
                            text: trailRow.modelData.here ? qsTr("here") : trailRow.modelData.kind
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                        }
                    }
                }
            }
        }
    }

    contentItem: Row {
        id: row
        spacing: 8
        Icon {
            id: chevron
            anchors.verticalCenter: parent.verticalCenter
            name: "back"
            size: 24
            lineWidth: 2.6
            color: button.down ? Theme.bg : button.activeFocus ? Theme.focus : Theme.text
        }
        Text {
            id: label
            objectName: "backLabel"
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, 300 - button.leftPadding - button.rightPadding - chevron.width - row.spacing)
            text: button.target
            elide: Text.ElideRight
            color: button.down ? Theme.bg : Theme.text
            font.pixelSize: Theme.fontBody
            font.bold: true
        }
    }
    background: Rectangle {
        radius: height / 2
        color: button.down ? Theme.accent : button.hovered ? Qt.rgba(0.17, 0.2, 0.26, 0.92) : Qt.rgba(0.086, 0.1, 0.13, 0.82)
        border.width: button.activeFocus ? Theme.focusWidth : 1
        border.color: button.activeFocus ? Theme.focus : Qt.rgba(1, 1, 1, 0.16)
    }
}
