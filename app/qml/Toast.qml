import QtQuick
import QtQuick.Controls.Basic

// Short message at the bottom of the window.
Rectangle {
    id: toast
    function show(message) {
        label.text = message
        opacity = 1
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

    Label {
        id: label
        anchors.centerIn: parent
        width: Math.min(implicitWidth, toast.parent.width - 104)
        wrapMode: Text.Wrap
        color: Theme.text
        font.pixelSize: Theme.fontBody
    }
    Timer { id: hideTimer; interval: 5000; onTriggered: toast.opacity = 0 }
}
