pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Effects

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
    // Issue #78: a format ribbon along the cover's bottom edge ("FLAC 24/96"), gold for Hi-Res;
    // and Play, Shuffle and More on the cover while it has focus or the mouse.
    property string ribbon: ""
    // Issue #79: a round image (artists), with the initials while there is none.
    property bool round: false
    readonly property string initials: title.split(" ").filter(w => w.length > 0).slice(0, 2).map(w => w.charAt(0)).join("").toUpperCase()
    property bool ribbonGold: false
    property bool quickActions: false
    signal playRequested()
    signal shuffleRequested()
    signal moreRequested()
    // Issue #66: the cover's own height / width (books): it keeps its aspect inside the cell,
    // standing on the cell's bottom, so the titles of square and portrait covers align. 0: fill.
    property real coverRatio: 0
    readonly property real cellHeight: posterWidth * posterRatio
    readonly property real frameWidth: coverRatio > 0 ? Math.min(posterWidth, cellHeight / coverRatio) : posterWidth
    readonly property real frameHeight: coverRatio > 0 ? Math.min(cellHeight, posterWidth * coverRatio) : cellHeight

    readonly property real textScale: Appearance.textScale(posterWidth)

    width: posterWidth
    height: posterWidth * posterRatio + 54 * textScale
    padding: 0
    focusPolicy: Qt.StrongFocus
    Accessible.name: title + (caption !== "" ? ", " + caption : "") + (watched ? ", " + qsTr("watched") : "") + (saved ? ", " + qsTr("saved on this computer") : "") + (versions > 1 ? ", " + qsTr("%n version(s)", "", versions) : "") + (ribbon !== "" ? ", " + ribbon : "")

    background: Item {}
    contentItem: Column {
        spacing: Math.round(8 * card.textScale)
        Item {
          width: card.posterWidth
          height: card.cellHeight
          Rectangle {
            id: frame
            anchors.bottom: parent.bottom
            anchors.horizontalCenter: parent.horizontalCenter
            width: card.frameWidth
            height: card.frameHeight
            radius: card.round ? width / 2 : Theme.radius
            clip: true
            color: Qt.hsla((card.title.length * 37 % 360) / 360, 0.30, 0.24, 1)
            scale: card.activeFocus ? 1.05 : 1
            Behavior on scale { NumberAnimation { duration: 110 } }

            Label {
                anchors.centerIn: parent
                width: parent.width - 24
                visible: img.status !== Image.Ready
                text: card.round ? card.initials : card.title
                color: Theme.text
                font.pixelSize: Math.round((card.round ? 40 : 17) * card.textScale)
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
                // Round tiles (issue #79) are masked to a circle; square ones need no layer.
                layer.enabled: card.round
                layer.effect: MultiEffect {
                    maskEnabled: true
                    maskSource: roundMask
                }
            }
            Rectangle {
                id: roundMask
                visible: false
                anchors.fill: parent
                radius: width / 2
                layer.enabled: card.round
            }
            Rectangle {   // format ribbon (issue #78)
                visible: card.ribbon !== ""
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: Math.round(24 * card.textScale)
                color: card.ribbonGold ? Theme.accent : "#c80d1015"
                Label {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 8
                    verticalAlignment: Text.AlignVCenter
                    text: card.ribbon
                    color: card.ribbonGold ? Theme.bg : Theme.text
                    font.pixelSize: Math.max(10, Math.round(11 * card.textScale))
                    font.bold: true
                    elide: Text.ElideRight
                }
            }
            Row {         // quick actions (issue #78)
                visible: card.quickActions && (card.activeFocus || card.hovered)
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Math.round(34 * card.textScale)
                spacing: 8
                Repeater {
                    model: [{ glyph: "▶", tip: qsTr("Play"), act: 0 }, { glyph: "⤮", tip: qsTr("Shuffle"), act: 1 }, { glyph: "⋯", tip: qsTr("More"), act: 2 }]
                    RoundButton {
                        required property var modelData
                        width: 40; height: 40
                        focusPolicy: Qt.NoFocus
                        text: modelData.glyph
                        Accessible.name: modelData.tip
                        ToolTip.visible: hovered
                        ToolTip.text: modelData.tip
                        palette.button: modelData.act === 0 ? Theme.accent : "#d90d1015"
                        palette.buttonText: modelData.act === 0 ? Theme.bg : Theme.text
                        onClicked: modelData.act === 0 ? card.playRequested() : modelData.act === 1 ? card.shuffleRequested() : card.moreRequested()
                    }
                }
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
                anchors { right: parent.right; bottom: parent.bottom; margins: 8; bottomMargin: card.ribbon !== "" ? Math.round(24 * card.textScale) + 6 : 8 }
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
                radius: card.round ? width / 2 : Theme.radius
                color: "transparent"
                border.width: card.activeFocus ? Theme.focusWidth + 1 : 0
                border.color: Theme.focus
            }
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
