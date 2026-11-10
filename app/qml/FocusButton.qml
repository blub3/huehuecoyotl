import QtQuick
import QtQuick.Controls.Basic

// Button with a clear focus ring, usable from keyboard and remote (SPEC 9: directional navigation).
Button {
    id: control
    property string glyph: ""
    property bool primary: false
    property string tip: ""

    focusPolicy: Qt.StrongFocus
    padding: 10
    horizontalPadding: glyph !== "" && text === "" ? 12 : 18
    font.pixelSize: Theme.fontBody
    Accessible.name: text !== "" ? text : tip

    Keys.onReturnPressed: clicked()
    Keys.onEnterPressed: clicked()

    contentItem: Row {
        spacing: 8
        Text {
            visible: control.glyph !== ""
            text: control.glyph
            color: Theme.text
            font.pixelSize: control.font.pixelSize + 4
            anchors.verticalCenter: parent.verticalCenter
        }
        Text {
            visible: control.text !== ""
            text: control.text
            color: Theme.text
            font: control.font
            anchors.verticalCenter: parent.verticalCenter
        }
    }

    background: Rectangle {
        implicitWidth: 44
        implicitHeight: 44
        radius: Theme.radius
        color: control.down ? Qt.darker(Theme.surfaceHi, 1.3)
             : control.primary ? Qt.rgba(0.89, 0.64, 0.23, control.activeFocus ? 0.45 : 0.28)
             : control.activeFocus || control.hovered ? Theme.surfaceHi : Qt.rgba(1, 1, 1, 0.06)
        border.width: control.activeFocus ? Theme.focusWidth : 0
        border.color: Theme.focus
    }

    ToolTip.visible: tip !== "" && hovered
    ToolTip.text: tip
    ToolTip.delay: 600
}
