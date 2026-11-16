pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// The technical details panel (issues #33 and #48): file, video, every audio and subtitle track,
// from one file's full probe data. Used on the movie and episode pages and under episode tiles.
GridLayout {
    id: panel
    property var file: null
    readonly property var info: file && file.probe && !file.probe.error ? file.probe : null
    columns: Math.max(1, Math.min(3, Math.floor(width / 340)))
    columnSpacing: 32
    rowSpacing: 18

    Repeater {
        model: panel.info ? MediaInfo.groups(panel.file, panel.info) : []
        ColumnLayout {
            id: group
            required property var modelData
            Layout.alignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: 4
            Label { text: group.modelData.title; color: Theme.accent; font.pixelSize: Theme.fontBody; font.bold: true }
            Repeater {
                model: group.modelData.rows
                RowLayout {
                    id: line
                    required property var modelData
                    spacing: 10
                    Layout.fillWidth: true
                    Label { text: line.modelData[0]; color: Theme.textDim; font.pixelSize: Theme.fontSmall; Layout.preferredWidth: 130 }
                    Label { text: line.modelData[1]; color: Theme.text; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; wrapMode: Text.WrapAnywhere }
                }
            }
        }
    }
}
