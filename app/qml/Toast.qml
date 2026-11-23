import QtQuick

// Short message at the bottom of the window.
Rectangle {
    id: toast
    function show(message) {
        label.text = message
        opacity = 1
        // Issue #171: a message with a command to copy stays long enough to do it.
        hideTimer.interval = message.indexOf("\n") >= 0 ? 30000 : 5000
        hideTimer.restart()
    }
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottom: parent.bottom
    anchors.bottomMargin: 48
    width: Math.min(parent.width - 64, label.implicitWidth + 40)
    height: label.implicitHeight + 24
    radius: Theme.radius
    color: Theme.surfaceHi
    border.color: Theme.danger
    opacity: 0
    visible: opacity > 0
    z: 100
    Behavior on opacity { NumberAnimation { duration: 200 } }
    Accessible.role: Accessible.AlertMessage
    Accessible.name: label.text

    TextEdit {
        id: label
        anchors.centerIn: parent
        width: Math.min(implicitWidth, toast.parent.width - 104)
        readOnly: true
        selectByMouse: true
        activeFocusOnPress: false
        wrapMode: TextEdit.Wrap
        color: Theme.text
        selectionColor: Theme.accent
        font.pixelSize: Theme.fontBody
    }
    Timer { id: hideTimer; interval: 5000; onTriggered: toast.opacity = 0 }
}
