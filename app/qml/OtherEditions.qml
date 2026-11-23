pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// Issue #147: the other releases of an album's release group, each an album of its own. One row
// per edition with its title, year, format and track count; Enter or a click opens it. `editions`
// comes from the album's detail. Up from the first row emits `leaveUp`.
ColumnLayout {
    id: root
    property var editions: []
    signal opened(var id)
    signal leaveUp()

    readonly property bool hasFocusTarget: repeater.count > 0
    function focusFirst() { if (repeater.count > 0) repeater.itemAt(0).forceActiveFocus() }

    visible: editions.length > 0
    spacing: 6

    Label {
        text: qsTr("Other editions")
        color: Theme.text
        font.pixelSize: Theme.fontLarge
        font.bold: true
        Accessible.role: Accessible.Heading
    }
    Repeater {
        id: repeater
        model: root.editions
        Button {
            id: row
            required property var modelData
            required property int index
            objectName: "edition" + index
            Layout.fillWidth: true
            implicitHeight: 60
            focusPolicy: Qt.StrongFocus
            readonly property string line: [row.modelData.year ? String(row.modelData.year) : "",
                                            MediaInfo.audioLine(row.modelData.codec || "", Number(row.modelData.sampleRate || 0), Number(row.modelData.bitDepth || 0)),
                                            qsTr("%n track(s)", "", Number(row.modelData.tracks || 0))]
                                           .filter(x => x !== "").join("  ·  ")
            Accessible.name: row.modelData.title + ", " + line
            onClicked: root.opened(row.modelData.id)
            Keys.onReturnPressed: root.opened(row.modelData.id)
            Keys.onEnterPressed: root.opened(row.modelData.id)
            Keys.onUpPressed: {
                if (index > 0) repeater.itemAt(index - 1).forceActiveFocus()
                else root.leaveUp()
            }
            Keys.onDownPressed: if (index < repeater.count - 1) repeater.itemAt(index + 1).forceActiveFocus()
            background: Rectangle {
                radius: 10
                color: row.activeFocus || row.hovered ? Theme.surfaceHi : "transparent"
                border.width: row.activeFocus ? Theme.focusWidth : 0
                border.color: Theme.focus
            }
            contentItem: RowLayout {
                spacing: 12
                Rectangle {
                    Layout.preferredWidth: 44
                    Layout.preferredHeight: 44
                    radius: 4
                    color: Theme.surface
                    clip: true
                    Image {
                        anchors.fill: parent
                        visible: (row.modelData.poster || "") !== ""
                        source: row.modelData.poster || ""
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        sourceSize: Qt.size(88, 88)
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Label { Layout.fillWidth: true; text: row.modelData.title; color: Theme.text; font.bold: true; elide: Text.ElideRight }
                    Label { Layout.fillWidth: true; text: row.line; color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                }
                Label { text: "›"; color: Theme.textDim; font.pixelSize: Theme.fontLarge }
            }
        }
    }
}
