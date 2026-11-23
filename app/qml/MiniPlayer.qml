pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The mini player (issue #8; laid out anew in issue #137): at the bottom of the shell while there
// is a queue or a book, on every screen but Now Playing. A 96 px bar in three parts: the cover
// and title on the left, the controls of Now Playing in the middle (Shuffle, Previous, a round
// Play/Pause, Next, Repeat, with the times), Volume and Now Playing on the right. Every control
// has a fixed size and the middle group is centred in the bar, so nothing moves when Play turns
// into Pause. Narrow windows lose the times first, then Volume; the title shortens.
// It takes no focus of its own: Down from the last entry of the navigation rail reaches it.
// Left and Right move between its controls, Up leaves it, Space plays or pauses.
// `music` and `bookPlayer` are the players (the Music and Book singletons, or test stubs).
FocusScope {
    id: mini
    property var music: null
    property var bookPlayer: null
    signal openRequested()
    signal upRequested()
    signal leftRequested()

    // Issue #66: the book when it played last (its title, chapter and author); else the music.
    readonly property bool book: !!bookPlayer && bookPlayer.front && bookPlayer.active
    readonly property var player: book ? bookPlayer : music
    readonly property real position: player ? player.position : 0
    readonly property real duration: player ? player.duration : 0
    // Issue #138: for a book, the times are the chapter's.
    readonly property var chapterInfo: book && bookPlayer.chapters ? (bookPlayer.chapters[bookPlayer.chapter] || null) : null
    readonly property real shownAt: chapterInfo ? Math.max(0, position - chapterInfo.startMs / 1000) : position
    readonly property real shownLength: chapterInfo ? Math.max(0, (chapterInfo.endMs - chapterInfo.startMs) / 1000) : duration
    function speedLabel(s) { return Number(s).toLocaleString(Qt.locale(), "f", 2).replace(/0$/, "") + "×" }
    readonly property bool playing: !!player && player.playing
    readonly property var current: music && music.current ? music.current : ({})
    readonly property bool showTimes: width >= 1000
    readonly property bool showVolume: !book && width >= 860

    function clock(s) {
        if (!(s >= 0)) s = 0
        const t = Math.floor(s)
        const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), sec = t % 60
        const ss = (sec < 10 ? "0" : "") + sec
        return h > 0 ? h + ":" + (m < 10 ? "0" : "") + m + ":" + ss : m + ":" + ss
    }
    function togglePause() { if (player) player.togglePause() }

    implicitHeight: 96
    Accessible.role: Accessible.Grouping
    Accessible.name: book ? qsTr("Audiobook player") : qsTr("Music player")
    Keys.onUpPressed: upRequested()
    Keys.onSpacePressed: togglePause()

    // A control of the bar: a fixed square (or, for Play, a round) button with a drawn icon.
    component Control: Button {
        id: control
        property string iconName: ""
        property int box: 44
        property bool on: false
        property bool round: false
        property string tip: ""
        // Issue #138: the seconds a skip button skips, inside its arrow.
        property int seconds: 0
        focusPolicy: Qt.StrongFocus
        implicitWidth: box
        implicitHeight: box
        padding: 0
        Accessible.name: tip
        Keys.onReturnPressed: clicked()
        Keys.onEnterPressed: clicked()
        ToolTip.visible: hovered && tip !== ""
        ToolTip.text: tip
        ToolTip.delay: 600
        contentItem: Item {
            Icon {
                anchors.centerIn: parent
                name: control.iconName
                size: control.seconds > 0 ? 30 : control.round ? 26 : control.box >= 48 ? 22 : 20
                lineWidth: control.seconds > 0 ? 1.6 : 2.2
                color: control.round ? Theme.bg : control.on ? Theme.focus : (control.box >= 48 ? Theme.text : "#c5ccd6")
            }
            Text {
                visible: control.seconds > 0
                anchors.centerIn: parent
                anchors.verticalCenterOffset: 1
                text: control.seconds
                color: "#c5ccd6"
                font.pixelSize: 10
                font.bold: true
            }
        }
        background: Rectangle {
            radius: control.round ? width / 2 : 12
            color: control.round ? (control.down ? Qt.darker(Theme.accent, 1.15) : Theme.accent)
                                 : control.on ? "#3a2e1a" : control.down ? Theme.surface : control.hovered ? "#2c3442" : Theme.surfaceHi
            border.width: control.activeFocus ? Theme.focusWidth : 0
            border.color: Theme.focus
        }
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.surface
        Rectangle { anchors { left: parent.left; right: parent.right; top: parent.top } height: 1; color: Theme.surfaceHi }
        // Progress along the top edge.
        Rectangle {
            objectName: "progress"
            anchors { left: parent.left; top: parent.top }
            height: 3
            width: mini.duration > 0 ? parent.width * Math.min(1, mini.position / mini.duration) : 0
            color: Theme.accent
        }
    }

    // ---- left: cover and title (both open Now Playing)
    RowLayout {
        id: info
        anchors { left: parent.left; leftMargin: 20; verticalCenter: parent.verticalCenter }
        width: Math.max(0, controls.x - 24 - anchors.leftMargin)
        spacing: 14
        Rectangle {
            Layout.preferredWidth: 60
            Layout.preferredHeight: 60
            radius: 8
            color: Theme.surfaceHi
            clip: true
            Label { anchors.centerIn: parent; visible: cover.status !== Image.Ready; text: mini.book ? "❞" : "♫"; color: Theme.textDim; font.pixelSize: 24 }
            Image {
                id: cover
                anchors.fill: parent
                source: mini.book ? (mini.bookPlayer.book.poster || "") : (mini.current.poster || "")
                sourceSize.width: 120
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
            }
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: mini.openRequested() }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 3
            Label {
                objectName: "title"
                Layout.fillWidth: true
                text: mini.book ? (mini.bookPlayer.book.title || "") : (mini.current.title || "")
                color: Theme.text
                font.pixelSize: Theme.fontBody
                font.bold: true
                elide: Text.ElideRight
            }
            Label {
                Layout.fillWidth: true
                text: mini.book ? [mini.bookPlayer.chapterTitle, mini.bookPlayer.book.authors].filter(x => x).join("  ·  ")
                                : [mini.current.artists, mini.current.album].filter(x => x).join("  ·  ")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                elide: Text.ElideRight
            }
        }
        TapHandler { onTapped: mini.openRequested() }
    }

    // ---- middle: the controls of Now Playing, centred in the bar
    RowLayout {
        id: controls
        objectName: "controls"
        anchors.centerIn: parent
        spacing: 14
        Label {
            visible: mini.showTimes
            Layout.preferredWidth: 48
            horizontalAlignment: Text.AlignRight
            text: mini.clock(mini.shownAt)
            color: Theme.textDim
            font.pixelSize: 12
            font.features: { "tnum": 1 }
        }
        Control {
            id: shuffle
            objectName: "shuffle"
            box: 44
            // Issue #138: for a book, back by the skip time of Settings → Playback.
            iconName: mini.book ? "skipBack" : "shuffle"
            seconds: mini.book ? mini.bookPlayer.skipBack : 0
            on: !mini.book && !!mini.music && mini.music.shuffle
            tip: mini.book ? qsTr("%n second(s) back", "", seconds) : on ? qsTr("Shuffle: on") : qsTr("Shuffle: off")
            KeyNavigation.right: prev
            Keys.onLeftPressed: mini.leftRequested()
            onClicked: mini.book ? mini.bookPlayer.skip(-1) : mini.music.shuffle = !mini.music.shuffle
        }
        Control {
            id: prev
            objectName: "previous"
            box: 48
            iconName: "previous"
            tip: mini.book ? qsTr("Previous chapter") : qsTr("Previous")
            KeyNavigation.right: play
            KeyNavigation.left: shuffle
            onClicked: mini.book ? mini.bookPlayer.previousChapter() : mini.music.previous()
        }
        Control {
            id: play
            objectName: "play"
            focus: true
            box: 64
            round: true
            iconName: mini.playing ? "pause" : "play"
            tip: mini.playing ? qsTr("Pause") : qsTr("Play")
            KeyNavigation.left: prev
            KeyNavigation.right: next
            onClicked: mini.togglePause()
        }
        Control {
            id: next
            objectName: "next"
            box: 48
            iconName: "next"
            tip: mini.book ? qsTr("Next chapter") : qsTr("Next")
            KeyNavigation.left: play
            KeyNavigation.right: repeat
            onClicked: mini.book ? mini.bookPlayer.nextChapter() : mini.music.next()
        }
        Control {
            id: repeat
            objectName: "repeat"
            box: 44
            // Issue #138: for a book, ahead by the skip time of Settings → Playback.
            iconName: mini.book ? "skipAhead" : "repeat"
            seconds: mini.book ? mini.bookPlayer.skipForward : 0
            readonly property string mode: mini.music ? mini.music.repeat : "off"
            on: !mini.book && mode !== "off"
            tip: mini.book ? qsTr("%n second(s) ahead", "", seconds)
                           : mode === "one" ? qsTr("Repeat: this song") : mode === "all" ? qsTr("Repeat: all") : qsTr("Repeat: off")
            KeyNavigation.left: next
            Keys.onRightPressed: (speed.visible ? speed : volume.visible ? volume : open).forceActiveFocus()
            onClicked: mini.book ? mini.bookPlayer.skip(1) : mini.music.cycleRepeat()
            // "This song" carries a small 1.
            Rectangle {
                visible: !mini.book && repeat.mode === "one"
                anchors { right: parent.right; top: parent.top; rightMargin: 4; topMargin: 4 }
                width: 15; height: 15; radius: 8
                color: Theme.focus
                Text { anchors.centerIn: parent; text: "1"; color: Theme.bg; font.pixelSize: 10; font.bold: true }
            }
        }
        Label {
            visible: mini.showTimes
            Layout.preferredWidth: 48
            text: mini.clock(mini.shownLength)
            color: Theme.textDim
            font.pixelSize: 12
            font.features: { "tnum": 1 }
        }
    }

    // ---- right: Volume and Now Playing
    RowLayout {
        anchors { right: parent.right; rightMargin: 20; verticalCenter: parent.verticalCenter }
        spacing: 12
        // Issue #138: the book's speed; its choices open above the button.
        Button {
            id: speed
            objectName: "speed"
            visible: mini.book
            focusPolicy: Qt.StrongFocus
            implicitHeight: 44
            implicitWidth: Math.max(64, speedText.implicitWidth + 28)
            text: mini.book ? mini.speedLabel(mini.bookPlayer.speed) : ""
            Accessible.name: qsTr("Speed %1").arg(text)
            Keys.onLeftPressed: repeat.forceActiveFocus()
            KeyNavigation.right: open
            Keys.onReturnPressed: clicked()
            Keys.onEnterPressed: clicked()
            onClicked: speedPopup.open()
            contentItem: Text { id: speedText; text: speed.text; color: Theme.text; font.pixelSize: Theme.fontSmall + 1; font.bold: true; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
            background: Rectangle {
                radius: 12
                color: speed.down ? Theme.surface : speed.hovered ? "#2c3442" : Theme.surfaceHi
                border.width: speed.activeFocus ? Theme.focusWidth : 0
                border.color: Theme.focus
            }
            Popup {
                id: speedPopup
                objectName: "speedPopup"
                x: (speed.width - width) / 2
                y: -height - 10
                width: 96
                height: 260
                padding: 6
                focus: true
                readonly property var speeds: {
                    const out = []
                    for (let s = 5; s <= 30; ++s) out.push(s / 10)
                    return out
                }
                onOpened: {
                    const i = speeds.findIndex(s => Math.abs(s - mini.bookPlayer.speed) < 0.05)
                    speedList.currentIndex = Math.max(0, i)
                    speedList.positionViewAtIndex(speedList.currentIndex, ListView.Center)
                    speedList.forceActiveFocus()
                }
                onClosed: speed.forceActiveFocus()
                background: Rectangle { radius: 14; color: "#1b2029"; border.width: 1; border.color: "#2c3442" }
                contentItem: ListView {
                    id: speedList
                    objectName: "speedList"
                    clip: true
                    model: speedPopup.speeds
                    keyNavigationEnabled: true
                    Keys.onReturnPressed: { mini.bookPlayer.setSpeed(speedPopup.speeds[currentIndex]); speedPopup.close() }
                    Keys.onEnterPressed: { mini.bookPlayer.setSpeed(speedPopup.speeds[currentIndex]); speedPopup.close() }
                    delegate: ItemDelegate {
                        id: speedRow
                        required property var modelData
                        required property int index
                        width: ListView.view.width
                        height: 36
                        text: mini.speedLabel(modelData)
                        onClicked: { mini.bookPlayer.setSpeed(modelData); speedPopup.close() }
                        contentItem: Text {
                            text: speedRow.text
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                            color: Math.abs(speedRow.modelData - mini.bookPlayer.speed) < 0.05 ? Theme.focus : Theme.text
                            font.pixelSize: Theme.fontSmall + 1
                        }
                        background: Rectangle {
                            radius: 8
                            color: speedRow.ListView.isCurrentItem ? "#2a3140" : "transparent"
                            border.width: speedRow.ListView.isCurrentItem && speedList.activeFocus ? 2 : 0
                            border.color: Theme.focus
                        }
                    }
                }
            }
        }
        Control {
            id: volume
            objectName: "volume"
            visible: mini.showVolume
            box: 44
            iconName: "volume"
            tip: qsTr("Volume")
            KeyNavigation.left: repeat
            KeyNavigation.right: open
            onClicked: volumePopup.open()
            Popup {
                id: volumePopup
                objectName: "volumePopup"
                x: (volume.width - width) / 2
                y: -height - 10
                width: 64
                height: 200
                padding: 10
                focus: true
                onOpened: volumeSlider.forceActiveFocus()
                onClosed: volume.forceActiveFocus()
                background: Rectangle { radius: 14; color: "#1b2029"; border.width: 1; border.color: "#2c3442" }
                contentItem: ColumnLayout {
                    spacing: 6
                    Slider {
                        id: volumeSlider
                        objectName: "volumeSlider"
                        Layout.fillHeight: true
                        Layout.alignment: Qt.AlignHCenter
                        orientation: Qt.Vertical
                        from: 0; to: 100; stepSize: 5
                        enabled: !(mini.music && mini.music.bitPerfect)
                        value: mini.music ? (mini.music.bitPerfect ? 100 : mini.music.volume) : 0
                        Accessible.name: qsTr("Volume")
                        onMoved: mini.music.volume = Math.round(value)
                    }
                    Label {
                        Layout.alignment: Qt.AlignHCenter
                        text: Math.round(volumeSlider.value) + " %"
                        color: Theme.textDim
                        font.pixelSize: 12
                    }
                }
            }
        }
        Button {
            id: open
            objectName: "open"
            focusPolicy: Qt.StrongFocus
            implicitHeight: 44
            leftPadding: 16
            rightPadding: 18
            Accessible.name: text
            // Issue #138: a book opens its player.
            text: mini.book ? qsTr("Player") : qsTr("Now Playing")
            Keys.onLeftPressed: (speed.visible ? speed : volume.visible ? volume : repeat).forceActiveFocus()
            Keys.onReturnPressed: clicked()
            Keys.onEnterPressed: clicked()
            onClicked: mini.openRequested()
            contentItem: Row {
                spacing: 10
                Icon { anchors.verticalCenter: parent.verticalCenter; name: "list"; size: 18; color: Theme.text }
                Text { anchors.verticalCenter: parent.verticalCenter; text: open.text; color: Theme.text; font.pixelSize: Theme.fontSmall + 1 }
            }
            background: Rectangle {
                radius: 12
                color: open.down ? Theme.surface : open.hovered ? "#2c3442" : Theme.surfaceHi
                border.width: open.activeFocus ? Theme.focusWidth : 0
                border.color: Theme.focus
            }
        }
    }
}
