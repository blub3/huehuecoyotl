import QtQuick
import QtQuick.Controls.Basic

// Grid tile: pre-scaled poster from disk (never a full-size image, SPEC 9), title, year,
// progress, watched mark and unwatched count.
ItemDelegate {
    id: card
    property string title
    property int year: 0
    property string poster
    // The 540 px tier, when the core has made it; used when the 360 px one would be upscaled.
    property string posterLarge
    property real progress: 0
    property bool watched: false
    property int unwatched: 0
    // Issue #4: a saved copy is on this computer.
    property bool saved: false
    // Issue #63: an album with more than one version shows how many.
    property int versions: 0
    property string caption: year > 0 ? String(year) : ""
    property real posterWidth: 180
    property real posterRatio: 1.5

    readonly property real textScale: Appearance.textScale(posterWidth)

    width: posterWidth
    height: posterWidth * posterRatio + 54 * textScale
    padding: 0
    focusPolicy: Qt.StrongFocus
    Accessible.name: title + (caption !== "" ? ", " + caption : "") + (watched ? ", " + qsTr("watched") : "") + (saved ? ", " + qsTr("saved on this computer") : "") + (versions > 1 ? ", " + qsTr("%n version(s)", "", versions) : "")

    background: Item {}
    contentItem: Column {
        spacing: Math.round(8 * card.textScale)
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
                font.pixelSize: Math.round(17 * card.textScale)
                font.bold: true
                wrapMode: Text.Wrap
                horizontalAlignment: Text.AlignHCenter
            }
            Image {
                id: img
                anchors.fill: parent
                // The smallest tier that covers the displayed size; the 360 px one until the
                // larger exists.
                source: card.posterLarge !== "" && card.posterWidth * Screen.devicePixelRatio > 360
                        ? card.posterLarge : card.poster
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
            Rectangle {   // saved copy
                visible: card.saved
                anchors { left: parent.left; top: parent.top; margins: 8 }
                width: 26; height: 26; radius: 13
                color: "#c0101418"
                Label {
                    anchors.centerIn: parent
                    text: "⤓"
                    color: Theme.accent
                    font.pixelSize: 14
                    font.bold: true
                }
            }
            Rectangle {   // several versions (issue #63)
                visible: card.versions > 1
                anchors { right: parent.right; bottom: parent.bottom; margins: 8 }
                width: Math.max(26, versionsText.implicitWidth + 12); height: 22; radius: 11
                color: "#c0101418"
                Label {
                    id: versionsText
                    anchors.centerIn: parent
                    text: card.versions + "×"
                    color: Theme.text
                    font.pixelSize: 12
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
            font.pixelSize: Math.round(14 * card.textScale)
            elide: Text.ElideRight
        }
        Label {
            width: card.posterWidth
            visible: card.caption !== ""
            text: card.caption
            color: Theme.textDim
            font.pixelSize: Math.round(12 * card.textScale)
            opacity: 0.8
        }
    }
}
