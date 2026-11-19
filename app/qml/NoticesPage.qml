pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// The licences of everything the app ships (SPEC 10). One line per delegate, so the long text
// (several hundred kilobytes) loads instantly and scrolls by keyboard: arrows, Page Up/Down,
// Home/End.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: qsTr("Third-party licences")
    readonly property var lines: Core.thirdPartyNotices().split("\n")

    onActiveFocusChanged: if (activeFocus) Qt.callLater(() => list.forceActiveFocus())

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 32
        spacing: 16
        RowLayout {
            spacing: 14
            Label { text: qsTr("Third-party licences"); color: Theme.text; font.pixelSize: Theme.fontTitle; font.bold: true }
        }
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: Theme.radius
            color: Theme.surface
            border.width: list.activeFocus ? Theme.focusWidth : 0
            border.color: Theme.focus

            ListView {
                id: list
                anchors.fill: parent
                anchors.margins: 16
                clip: true
                model: page.lines.length
                reuseItems: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AlwaysOn }
                Accessible.role: Accessible.Document
                Accessible.name: qsTr("Third-party licences")
                readonly property real lineHeight: 20
                function scrollBy(px) {
                    contentY = Math.max(originY, Math.min(contentY + px, originY + contentHeight - height))
                }
                Keys.onUpPressed: (e) => { if (atYBeginning) e.accepted = false; else scrollBy(-3 * lineHeight) }
                Keys.onDownPressed: scrollBy(3 * lineHeight)
                Keys.onPressed: (e) => {
                    if (e.key === Qt.Key_PageDown || e.key === Qt.Key_Space) { scrollBy(height - lineHeight); e.accepted = true }
                    else if (e.key === Qt.Key_PageUp) { scrollBy(-(height - lineHeight)); e.accepted = true }
                    else if (e.key === Qt.Key_Home) { positionViewAtBeginning(); e.accepted = true }
                    else if (e.key === Qt.Key_End) { positionViewAtEnd(); e.accepted = true }
                }
                Keys.onLeftPressed: Nav.focusRail()
                delegate: Text {
                    required property int index
                    width: ListView.view.width - 14
                    height: Math.max(implicitHeight, 20)
                    text: page.lines[index]
                    textFormat: Text.PlainText
                    wrapMode: Text.WrapAnywhere
                    font.family: "monospace"
                    font.pixelSize: 13
                    color: Theme.text
                }
            }
        }
    }
}
