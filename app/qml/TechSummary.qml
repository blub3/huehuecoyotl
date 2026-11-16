pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// One line of technical details (issues #33 and #48): badges for 4K, HDR and Atmos or DTS:X, then
// "1080p · H.264 · AAC 2.0 · 2 subtitles". Until the file is analysed it says so. Used on the
// movie and episode pages and on episode tiles.
RowLayout {
    id: line
    // The file's probe data (null until it is analysed).
    property var info: null
    // Say "Analysing…" (and that a file could not be analysed) instead of showing nothing.
    property bool showPending: true
    readonly property bool usable: info !== null && info !== undefined && !info.error
    spacing: 8
    visible: usable || showPending

    Repeater {
        model: line.usable ? MediaInfo.badges(line.info) : []
        Rectangle {
            required property string modelData
            implicitWidth: badgeText.implicitWidth + 12
            implicitHeight: 22
            radius: 4
            color: "transparent"
            border.width: 1
            border.color: Theme.textDim
            Label {
                id: badgeText
                anchors.centerIn: parent
                text: parent.modelData
                color: Theme.text
                font.pixelSize: 12
                font.bold: true
            }
        }
    }
    Label {
        Layout.fillWidth: true
        text: line.usable ? MediaInfo.summary(line.info)
            : line.info ? qsTr("This file could not be analysed.") : qsTr("Analysing…")
        color: Theme.textDim
        font.pixelSize: Theme.fontSmall
        elide: Text.ElideRight
    }
}
