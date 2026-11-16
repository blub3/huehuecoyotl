pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// One episode on the show and season pages (issues #32, #48): still, number, title, runtime,
// overview, progress and watched state, and for an episode on disk its version, technical summary,
// details panel and Save menu. An episode only TMDB knows is dimmed, cannot be played and has no
// controls.
//
// Keys on the tile: Enter plays the selected version, Right moves into the controls, Up and Down
// move to the next tile, W toggles watched, I opens the episode's page, Menu opens the Save menu.
// In the controls: Left and Right move between them, Up and Down leave them for the next tile,
// Back returns to the tile. The tile does nothing itself: the page acts on its signals.
FocusScope {
    id: tile
    // The episode as EpisodeModel delivers it: summary, files, lastFileId, positionMs, save.
    property var ep: ({})
    // The version chosen on this tile (0: preselect: last played, else `preference`).
    property var chosenFileId: 0
    property string preference: "quality"
    // The details panel is open, with this file's full probe data (null while it loads).
    property bool expanded: false
    property var fullFile: null

    signal play(var fileId)
    signal info()
    signal toggleWatched()
    signal chooseVersion(var fileId)
    signal toggleDetails()
    signal analyse(var fileId)
    signal saveAction(string action, var fileId)
    signal moveUp()
    signal moveDown()
    signal leave()

    readonly property bool onDisk: ep.onDisk !== false
    readonly property var files: ep.files || []
    readonly property int fileIndex: {
        const keep = files.findIndex(f => f.id === chosenFileId)
        return keep >= 0 ? keep : MediaInfo.preselect(files, ep.lastFileId || 0, preference)
    }
    readonly property var file: files.length > 0 ? files[fileIndex] : null
    readonly property var fileInfo: file && file.probe ? file.probe : null
    readonly property bool playable: MediaInfo.playable(file)
    readonly property var sv: ep.save || ({ state: "none", saveable: 0 })
    readonly property bool hasControls: onDisk && file !== null
    readonly property int savePercent: sv.size > 0 ? Math.floor(100 * sv.bytesDone / sv.size) : 0

    function focusBody() { body.forceActiveFocus() }
    function focusControls() { (versions.visible ? versions : detailsButton).forceActiveFocus() }
    function openSaveMenu(from) { if (hasControls && saveMenu.visible) saveMenu.openMenu(from || saveMenu) }
    function activate() {
        if (onDisk && playable) play(file ? file.id : 0)
        else info()
    }
    // Keys every part of the tile shares.
    function commonKey(e) {
        if (e.key === Qt.Key_W && onDisk) { toggleWatched(); e.accepted = true }
        else if (e.key === Qt.Key_I) { info(); e.accepted = true }
        else if (e.key === Qt.Key_Menu && hasControls) { openSaveMenu(tile.Window.activeFocusItem); e.accepted = true }
    }
    // In the controls: Back returns to the tile, Up and Down leave for the next tile.
    function controlKey(e) {
        if (e.key === Qt.Key_Escape || e.key === Qt.Key_Back) { focusBody(); e.accepted = true }
        else if (e.key === Qt.Key_Up) { moveUp(); e.accepted = true }
        else if (e.key === Qt.Key_Down) { moveDown(); e.accepted = true }
        else commonKey(e)
    }

    implicitHeight: col.implicitHeight + 24
    // Coming back to a tile lands on the tile itself, not on the control used last.
    onActiveFocusChanged: if (!activeFocus) body.focus = true
    Accessible.role: Accessible.ListItem
    Accessible.name: qsTr("Episode %1, %2").arg(ep.episodeNo).arg(ep.title)
        + (onDisk ? "" : ", " + qsTr("not in your library"))
        + (ep.watched ? ", " + qsTr("watched") : "")
        + (sv.state === "done" ? ", " + qsTr("saved on this computer") : "")
        + (sv.state === "outdated" ? ", " + qsTr("saved copy outdated") : "")

    Rectangle {
        anchors.fill: parent
        radius: Theme.radius
        color: tile.activeFocus ? Theme.surfaceHi : hover.hovered ? Theme.surface : "transparent"
        border.width: body.activeFocus ? Theme.focusWidth : 0
        border.color: Theme.focus
        HoverHandler { id: hover }
    }

    // The part of the tile that has focus when it is "selected": the still and the text.
    Item {
        id: body
        objectName: "tileBody"
        anchors.fill: parent
        focus: true
        activeFocusOnTab: true
        Keys.onReturnPressed: tile.activate()
        Keys.onEnterPressed: tile.activate()
        Keys.onUpPressed: tile.moveUp()
        Keys.onDownPressed: tile.moveDown()
        Keys.onLeftPressed: tile.leave()
        Keys.onRightPressed: if (tile.hasControls) tile.focusControls()
        Keys.onPressed: (e) => tile.commonKey(e)
        TapHandler { onTapped: { tile.focusBody(); tile.activate() } }
    }

    ColumnLayout {
        id: col
        x: 12
        y: 12
        width: parent.width - 24
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            spacing: 18
            opacity: tile.onDisk ? 1 : 0.45
            Rectangle {
                Layout.preferredWidth: 160; Layout.preferredHeight: 90
                Layout.alignment: Qt.AlignTop
                radius: 6; clip: true
                color: Theme.surfaceHi
                Label {
                    anchors.centerIn: parent
                    text: tile.ep.episodeNo ?? ""
                    color: Theme.textDim
                    font.pixelSize: 26
                    visible: still.status !== Image.Ready
                }
                Image {
                    id: still
                    anchors.fill: parent
                    source: tile.ep.poster || ""
                    asynchronous: true
                    fillMode: Image.PreserveAspectCrop
                    sourceSize.width: 320
                }
                Rectangle {
                    visible: tile.ep.progress > 0 && !tile.ep.watched
                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                    height: 4; color: "#80000000"
                    Rectangle { width: parent.width * tile.ep.progress; height: 4; color: Theme.accent }
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop
                spacing: 4
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Label {
                        Layout.fillWidth: true
                        text: tile.ep.episodeNo + ". " + tile.ep.title
                        color: tile.ep.watched ? Theme.textDim : Theme.text
                        font.pixelSize: Theme.fontBody
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    Label {
                        objectName: "savedMark"
                        visible: tile.sv.state === "done" || tile.sv.state === "outdated"
                        text: tile.sv.state === "done" ? "⤓ " + qsTr("Saved") : "⚠ " + qsTr("Saved copy outdated")
                        color: tile.sv.state === "done" ? Theme.accent : Theme.danger
                        font.pixelSize: Theme.fontSmall
                    }
                    Label {
                        visible: tile.ep.runtimeMin > 0
                        text: qsTr("%1 min").arg(tile.ep.runtimeMin)
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                    }
                    // Watched toggle for the mouse (W on the keyboard).
                    ToolButton {
                        visible: tile.onDisk
                        focusPolicy: Qt.NoFocus
                        Accessible.name: tile.ep.watched ? qsTr("Mark unwatched") : qsTr("Mark watched")
                        ToolTip.visible: hovered
                        ToolTip.text: Accessible.name
                        contentItem: Text {
                            text: "✓"
                            font.pixelSize: 20
                            color: tile.ep.watched ? Theme.accent : Theme.surfaceHi
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Item {}
                        onClicked: tile.toggleWatched()
                    }
                }
                Label {
                    Layout.fillWidth: true
                    visible: !tile.onDisk
                    text: qsTr("Not in your library")
                    color: Theme.accent
                    font.pixelSize: Theme.fontSmall
                }
                TechSummary {
                    objectName: "tileSummary"
                    Layout.fillWidth: true
                    visible: tile.hasControls
                    info: tile.fileInfo
                }
                Label {
                    Layout.fillWidth: true
                    text: tile.ep.overview || ""
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                }
            }
        }

        // The controls: version, details and the Save menu, for the selected version.
        RowLayout {
            visible: tile.hasControls
            Layout.fillWidth: true
            Layout.leftMargin: 178
            spacing: 10
            VersionPicker {
                id: versions
                objectName: "tileVersions"
                visible: tile.files.length > 1
                files: tile.files
                index: tile.fileIndex
                KeyNavigation.right: detailsButton
                Keys.onLeftPressed: tile.focusBody()
                Keys.onPressed: (e) => tile.controlKey(e)
                onChosen: (i) => tile.chooseVersion(tile.files[i].id)
            }
            FocusButton {
                id: detailsButton
                objectName: "tileDetails"
                glyph: tile.expanded ? "▾" : "▸"
                text: qsTr("Details")
                font.pixelSize: Theme.fontSmall
                Accessible.name: qsTr("Technical details")
                KeyNavigation.right: saveMenu
                Keys.onLeftPressed: (versions.visible ? versions : body).forceActiveFocus()
                Keys.onPressed: (e) => {
                    if (e.key === Qt.Key_Down && tile.expanded && analyseButton.visible) { analyseButton.forceActiveFocus(); e.accepted = true }
                    else tile.controlKey(e)
                }
                onClicked: tile.toggleDetails()
            }
            SaveMenu {
                id: saveMenu
                objectName: "tileSave"
                sv: tile.sv
                networkFile: tile.file !== null && tile.file.sourceKind !== "local"
                Keys.onLeftPressed: detailsButton.forceActiveFocus()
                Keys.onRightPressed: {}
                Keys.onPressed: (e) => tile.controlKey(e)
                onTriggered: (action) => tile.saveAction(action, tile.file ? tile.file.id : 0)
            }
            Label {
                Layout.fillWidth: true
                visible: tile.file !== null && !tile.playable
                text: qsTr("Source offline")
                color: Theme.danger
                font.pixelSize: Theme.fontSmall
                elide: Text.ElideRight
            }
            Item { Layout.fillWidth: true; visible: tile.playable }
        }
        Rectangle {
            objectName: "tileProgress"
            visible: tile.hasControls && (saveMenu.busy || tile.sv.state === "paused")
            Layout.leftMargin: 178
            Layout.preferredWidth: 320
            Layout.preferredHeight: 4
            radius: 2
            color: Theme.surfaceHi
            Rectangle { width: parent.width * tile.savePercent / 100; height: parent.height; radius: 2; color: Theme.accent }
        }

        // The details panel, under the tile.
        ColumnLayout {
            visible: tile.expanded && tile.hasControls
            Layout.fillWidth: true
            Layout.leftMargin: 178
            Layout.topMargin: 6
            spacing: 10
            RowLayout {
                spacing: 12
                visible: tile.fullFile === null || !tile.fullFile.probe || tile.fullFile.probe.error !== undefined
                Label {
                    text: tile.fullFile === null ? qsTr("Loading…")
                        : !tile.fullFile.probe ? qsTr("Analysing…") : qsTr("This file could not be analysed.")
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
                FocusButton {
                    id: analyseButton
                    objectName: "tileAnalyse"
                    visible: tile.fullFile !== null && (!tile.fullFile.probe || tile.fullFile.probe.error !== undefined)
                    text: qsTr("Analyse now")
                    font.pixelSize: Theme.fontSmall
                    Keys.onPressed: (e) => {
                        if (e.key === Qt.Key_Up) { detailsButton.forceActiveFocus(); e.accepted = true }
                        else tile.controlKey(e)
                    }
                    Keys.onLeftPressed: detailsButton.forceActiveFocus()
                    onClicked: tile.analyse(tile.file.id)
                }
            }
            TechDetails {
                objectName: "tileDetailsPanel"
                Layout.fillWidth: true
                file: tile.fullFile
            }
        }
    }
}
