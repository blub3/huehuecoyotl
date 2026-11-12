import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// One episode in a list (show page and season page): still, number, title, runtime, overview,
// progress and watched state. An episode only TMDB knows (not on disk) is dimmed and cannot be
// played. Enter plays, I opens the episode's page, W toggles watched.
ItemDelegate {
    id: row
    property var ep: ({})
    readonly property bool onDisk: ep.onDisk !== false
    signal play()
    signal info()
    signal toggleWatched()

    height: 112
    focusPolicy: Qt.StrongFocus
    Accessible.name: qsTr("Episode %1, %2").arg(ep.episodeNo).arg(ep.title)
        + (onDisk ? "" : ", " + qsTr("not in your library"))
        + (ep.watched ? ", " + qsTr("watched") : "")
    onClicked: onDisk ? row.play() : row.info()
    Keys.onReturnPressed: onDisk ? row.play() : row.info()
    Keys.onEnterPressed: onDisk ? row.play() : row.info()
    Keys.onPressed: (e) => {
        if (e.key === Qt.Key_W && row.onDisk) { row.toggleWatched(); e.accepted = true }
        else if (e.key === Qt.Key_I || e.key === Qt.Key_Menu) { row.info(); e.accepted = true }
    }

    background: Rectangle {
        radius: Theme.radius
        color: row.activeFocus ? Theme.surfaceHi : row.hovered ? Theme.surface : "transparent"
        border.width: row.activeFocus ? Theme.focusWidth : 0
        border.color: Theme.focus
    }
    contentItem: RowLayout {
        spacing: 18
        opacity: row.onDisk ? 1 : 0.45
        Rectangle {
            Layout.preferredWidth: 160; Layout.preferredHeight: 90
            radius: 6; clip: true
            color: Theme.surfaceHi
            Label {
                anchors.centerIn: parent
                text: row.ep.episodeNo ?? ""
                color: Theme.textDim
                font.pixelSize: 26
                visible: still.status !== Image.Ready
            }
            Image {
                id: still
                anchors.fill: parent
                source: row.ep.poster || ""
                asynchronous: true
                fillMode: Image.PreserveAspectCrop
                sourceSize.width: 320
            }
            Rectangle {
                visible: row.ep.progress > 0 && !row.ep.watched
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: 4; color: "#80000000"
                Rectangle { width: parent.width * row.ep.progress; height: 4; color: Theme.accent }
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Label {
                    Layout.fillWidth: true
                    text: row.ep.episodeNo + ". " + row.ep.title
                    color: row.ep.watched ? Theme.textDim : Theme.text
                    font.pixelSize: Theme.fontBody
                    font.bold: true
                    elide: Text.ElideRight
                }
                Label {
                    visible: row.ep.runtimeMin > 0
                    text: qsTr("%1 min").arg(row.ep.runtimeMin)
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
            }
            Label {
                Layout.fillWidth: true
                visible: !row.onDisk
                text: qsTr("Not in your library")
                color: Theme.accent
                font.pixelSize: Theme.fontSmall
            }
            Label {
                Layout.fillWidth: true
                text: row.ep.overview || ""
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.Wrap
                maximumLineCount: 2
                elide: Text.ElideRight
            }
        }
        // Watched toggle for the mouse (W on the keyboard).
        ToolButton {
            visible: row.onDisk
            focusPolicy: Qt.NoFocus
            text: "✓"
            font.pixelSize: 20
            Accessible.name: row.ep.watched ? qsTr("Mark unwatched") : qsTr("Mark watched")
            ToolTip.visible: hovered
            ToolTip.text: Accessible.name
            contentItem: Text {
                text: "✓"
                font.pixelSize: 20
                color: row.ep.watched ? Theme.accent : Theme.surfaceHi
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }
            background: Item {}
            onClicked: row.toggleWatched()
        }
    }
}
