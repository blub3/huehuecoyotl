import QtQuick
import QtQuick.Controls.Basic

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

    visible: below !== null
    focusPolicy: Qt.StrongFocus
    implicitHeight: 48
    implicitWidth: Math.min(300, row.implicitWidth + leftPadding + rightPadding)
    leftPadding: 12
    rightPadding: 20
    Accessible.name: qsTr("Back to %1").arg(target)
    Keys.onReturnPressed: clicked()
    Keys.onEnterPressed: clicked()
    Keys.onEscapePressed: backRequested()
    onClicked: backRequested()

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
