import QtQuick
import QtQuick.Controls.Basic

// Grid tile: pre-scaled poster from disk (never a full-size image, SPEC 9), title, year,
// progress, watched mark and unwatched count.
ItemDelegate {
    id: card
    property string title
    property int year: 0
    property string poster
    property real progress: 0
    property bool watched: false
    property int unwatched: 0
    property string caption: year > 0 ? String(year) : ""
    property real posterWidth: 180
    property real posterRatio: 1.5

    width: posterWidth
    height: posterWidth * posterRatio + 54
    padding: 0
    focusPolicy: Qt.StrongFocus
    Accessible.name: title + (caption !== "" ? ", " + caption : "") + (watched ? ", " + qsTr("watched") : "")

    background: Item {}
    contentItem: Column {
        spacing: 8
        Rectangle {
            id: frame
            width: card.posterWidth
            height: card.posterWidth * card.posterRatio
            radius: Theme.radius
            clip: true
            color: Qt.hsla((card.title.length * 37 % 360) / 360, 0.30, 0.24, 1)
            scale: card.activeFocus ? 1.05 : 1
            Behavior on scale { NumberAnimation { duration: 110 } }

            Label {
                anchors.centerIn: parent
                width: parent.width - 24
                visible: img.status !== Image.Ready
                text: card.title
                color: Theme.text
                font.pixelSize: 17
                font.bold: true
                wrapMode: Text.Wrap
                horizontalAlignment: Text.AlignHCenter
            }
            Image {
                id: img
                anchors.fill: parent
                source: card.poster
                asynchronous: true
                cache: true
                fillMode: Image.PreserveAspectCrop
                sourceSize.width: Math.ceil(card.posterWidth * Screen.devicePixelRatio)
            }
            Rectangle {   // progress
                visible: card.progress > 0 && !card.watched
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: 5
                color: "#80000000"
                Rectangle { width: parent.width * card.progress; height: parent.height; color: Theme.accent }
            }
            Rectangle {   // watched or unwatched count
                visible: card.watched || card.unwatched > 0
                anchors { right: parent.right; top: parent.top; margins: 8 }
                width: Math.max(26, badge.implicitWidth + 12); height: 26; radius: 13
                color: card.watched ? "#c0101418" : Theme.accent
                Label {
                    id: badge
                    anchors.centerIn: parent
                    text: card.watched ? "✓" : String(card.unwatched)
                    color: card.watched ? Theme.accent : Theme.bg
                    font.pixelSize: 13
                    font.bold: true
                }
            }
            Rectangle {   // focus ring drawn above the image
                anchors.fill: parent
                radius: Theme.radius
                color: "transparent"
                border.width: card.activeFocus ? Theme.focusWidth + 1 : 0
                border.color: Theme.focus
            }
        }
        Label {
            width: card.posterWidth
            text: card.title
            color: card.activeFocus ? Theme.text : Theme.textDim
            font.pixelSize: 14
            elide: Text.ElideRight
        }
        Label {
            width: card.posterWidth
            visible: card.caption !== ""
            text: card.caption
            color: Theme.textDim
            font.pixelSize: 12
            opacity: 0.8
        }
    }
}
