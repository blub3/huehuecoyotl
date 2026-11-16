pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Dialogs
import Coyotl
import "MediaInfo.js" as MediaInfo

// Detail page for a movie, show, season or episode. A show (issue #32) has a season switcher
// (tabs, or a list for many seasons) above the selected season's episodes, including episodes
// only TMDB knows (dimmed). Seasons list their episodes. Re-queries when the library changes.
// Keys on a show: left/right switch seasons, up/down move through episodes, W marks the focused
// episode or (on the season tabs) the whole season watched or unwatched.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var itemId
    property var d: ({})
    property var request: 0
    readonly property string kind: d.kind || ""
    readonly property var childItems: d.children || []
    readonly property bool loaded: d.id !== undefined

    // Show page: the seasons (specials last), the selected one and its episodes.
    readonly property var seasons: kind === "show" ? childItems : []
    property int seasonIndex: -1
    property var seasonD: ({})
    property var seasonRequest: 0
    readonly property var season: seasonIndex >= 0 && seasonIndex < seasons.length ? seasons[seasonIndex] : null
    readonly property var episodes: kind === "show" ? (seasonD.children || []) : (kind === "season" ? childItems : [])
    readonly property bool manySeasons: seasons.length > 8

    function load() { request = Core.itemDetail(itemId) }
    function selectSeason(i) {
        if (seasons.length === 0) return
        seasonIndex = Math.max(0, Math.min(i, seasons.length - 1))
        seasonRequest = Core.itemDetail(seasons[seasonIndex].id)
    }
    // Opens on the season that holds the next episode.
    function initialSeason() {
        const n = d.nextUp
        let i = n ? seasons.findIndex(s => s.seasonNo === n.seasonNo) : -1
        if (i < 0) i = seasons.findIndex(s => s.onDisk)
        return Math.max(0, i)
    }
    function seasonBar() { return manySeasons ? seasonPicker : seasonTabs }
    function childView() {
        if (kind === "show" && seasons.length > 0) return seasonBar()
        if (kind === "season" && episodeList.count > 0) return episodeList
        if (hasFiles) return detailsToggle
        return null
    }

    // Movies and episodes (issue #33): the file the page describes and its probe data. With
    // several versions (issue #34) the selector sets it; the choice survives reloads.
    property int fileIndex: 0
    property var selectedFileId: 0
    property var splitRequest: 0
    readonly property bool manyVersions: hasFiles && files.length > 1
    // Resume belongs to the movie, unless this version is another cut (durations > 2 % apart).
    readonly property bool sharedResume: !hasFiles || MediaInfo.sameCut(d.durationMs || 0, file)
    readonly property real resumeMs: hasFiles && !sharedResume ? (file.positionMs || 0) : (d.positionMs || 0)
    function pickVersion() {
        const keep = files.findIndex(f => f.id === selectedFileId)
        fileIndex = keep >= 0 ? keep : MediaInfo.preselect(files, d.lastFileId || 0, Appearance.preferredVersion)
        selectedFileId = files.length ? files[fileIndex].id : 0
    }
    function chooseVersion(i) {
        fileIndex = i
        selectedFileId = files[i].id
    }
    readonly property var files: d.files || []
    readonly property bool hasFiles: (kind === "movie" || kind === "episode") && files.length > 0
    readonly property var file: hasFiles ? files[Math.min(fileIndex, files.length - 1)] : null
    readonly property var info: file && file.probe ? file.probe : null
    property bool detailsOpen: false

    // Issue #4: offline copies. The detail carries the state; while copies run, it is polled.
    property var liveSave: null
    property var saveRequest: 0
    readonly property var sv: liveSave || d.save || ({ state: "none", saved: 0, saveable: 0 })
    readonly property bool saveBusy: sv.state === "queued" || sv.state === "running"
    readonly property bool canSave: loaded && kind !== "video" && (sv.saveable > 0 || sv.state !== "none")
    readonly property int savePercent: sv.size > 0 ? Math.floor(100 * sv.bytesDone / sv.size) : 0
    function saveLabel() {
        if (sv.state === "failed") return qsTr("Try again")
        if (sv.state === "outdated") return qsTr("Save again")
        if (sv.state === "paused") return qsTr("Resume")
        if (saveBusy) return qsTr("Pause")
        if (sv.state === "done") return deleteLabel()
        if (kind === "season") return qsTr("Save season")
        if (kind === "show") return qsTr("Save all")
        return qsTr("Save")
    }
    function deleteLabel() {
        return kind === "season" || kind === "show" ? qsTr("Delete saved copies") : qsTr("Delete saved copy")
    }
    function saveAction() {
        if (sv.state === "done") { deleteSavedConfirm.open(); return }
        if (sv.state === "paused" || sv.state === "failed") Core.resumeSavedItem(itemId)
        else if (saveBusy) Core.pauseSavedItem(itemId)
        else saveRequest = Core.saveItem(itemId, hasFiles && file ? file.id : 0)
        saveTimer.restart()
    }
    function saveStatus() {
        const many = kind === "season" || kind === "show"
        switch (sv.state) {
        case "queued": return qsTr("Waiting to save…")
        case "running": return qsTr("Saving… %1 %").arg(savePercent) + "  ·  " + MediaInfo.size(sv.bytesDone) + " / " + MediaInfo.size(sv.size)
        case "paused": return qsTr("Paused at %1 %").arg(savePercent)
        case "done": return many ? qsTr("All episodes are saved on this computer") : qsTr("Saved on this computer")
        case "outdated": return qsTr("The saved copy is outdated: the file on the server changed.")
        case "failed": return qsTr("Saving failed: %1").arg(Core.downloadErrorText(sv.errorKind, sv.error))
        }
        return many && sv.saved > 0 ? qsTr("%1 of %2 episodes saved").arg(sv.saved).arg(sv.saveable) : ""
    }
    readonly property bool hasCopy: sv.state !== "none" || sv.saved > 0
    // Issue #5: Save as… (a file for a movie or episode, a folder for a season or show).
    readonly property bool canExport: loaded && (hasFiles || kind === "season" || kind === "show")
    property var exportRequest: 0
    property string exportDest: ""
    property var conflicts: []
    function exportTo(dest, overwrite) {
        exportDest = dest
        exportRequest = Core.exportItem(itemId, hasFiles && file ? file.id : 0, dest, overwrite)
    }
    function belowActions() { return canSave ? saveButton : canExport ? exportButton : childView() }
    Timer {
        id: saveTimer
        interval: 1000
        repeat: true
        running: page.saveBusy && page.visible
        onTriggered: page.saveRequest = Core.saveState(page.itemId)
    }
    // Keep the focused episode row inside the visible part of the page.
    function reveal(item) {
        const p = item.mapToItem(flick.contentItem, 0, 0)
        if (p.y < flick.contentY + 40) flick.contentY = Math.max(0, p.y - 40)
        else if (p.y + item.height > flick.contentY + flick.height - 40)
            flick.contentY = Math.min(p.y + item.height - flick.height + 40, Math.max(0, flick.contentHeight - flick.height))
    }
    function play() {
        if (hasFiles && file) Playback.openItemFile(itemId, file.id)
        else Playback.openItem(d.nextUp ? d.nextUp.id : itemId)
    }
    function metaLine() {
        const parts = []
        if (d.year > 0) parts.push(d.year)
        if (d.runtimeMin > 0) parts.push(qsTr("%1 min").arg(d.runtimeMin))
        if (d.rating > 0) parts.push("★ " + d.rating.toFixed(1))
        if (d.genres && d.genres.length) parts.push(d.genres.slice(0, 3).join(", "))
        return parts.join("  ·  ")
    }
    function playLabel() {
        if (kind === "show" || kind === "season") {
            const n = d.nextUp
            if (!n) return qsTr("Play")
            if (n.watched) return qsTr("Play again")
            if (n.progress > 0) return qsTr("Resume S%1 E%2").arg(n.seasonNo).arg(n.episodeNo)
            return qsTr("Play S%1 E%2").arg(n.seasonNo).arg(n.episodeNo)
        }
        const pos = d.watched && sharedResume ? 0 : resumeMs
        return pos > 0 ? qsTr("Resume from %1").arg(Theme.time(pos / 1000)) : qsTr("Play")
    }

    Component.onCompleted: load()
    onActiveFocusChanged: if (activeFocus && loaded && !content.activeFocus) playButton.forceActiveFocus()

    Connections {
        target: Core
        function onDetailReady(requestId, detail) {
            // A reload of the same list keeps the focused episode (marking one watched reloads it).
            const keep = episodeList.currentIndex
            if (requestId === page.seasonRequest) {
                const same = page.seasonD.id === detail.id
                page.seasonD = detail
                if (same) episodeList.currentIndex = Math.min(keep, episodeList.count - 1)
                return
            }
            if (requestId === page.splitRequest) {
                // The version is now its own entry; this page shows the remaining versions.
                (page.Window.window as Main).showError(qsTr("“%1” is now a separate entry. Choose its match under Settings → Review matches.").arg(detail.title))
                page.selectedFileId = 0
                page.load()
                return
            }
            if (requestId !== page.request) return
            const first = !page.loaded
            page.liveSave = null
            page.d = detail
            if (page.hasFiles) page.pickVersion()
            if (page.kind === "season" && !first) episodeList.currentIndex = Math.min(keep, episodeList.count - 1)
            if (page.kind === "show")
                page.selectSeason(first || page.seasonIndex < 0 ? page.initialSeason() : page.seasonIndex)
            if (first) playButton.forceActiveFocus()
        }
        function onSaveStateReady(requestId, id, info) {
            if (id !== page.itemId) return
            const was = page.sv.state
            page.liveSave = info
            if (was !== info.state && info.state === "done") page.load()
        }
        function onDownloadsListed(requestId, items, used, dir) {
            if (requestId === page.saveRequest) page.saveRequest = Core.saveState(page.itemId)
            if (requestId === page.exportRequest) {
                page.exportRequest = 0;
                (page.Window.window as Main).showError((Core.isPortalPath(page.exportDest)
                    ? qsTr("Saving a copy to %1. Progress is under Downloads. Subtitle files are not copied along: the system's file dialog grants this one file only. Choose a folder (Save as… on a season) to include them.")
                    : qsTr("Saving a copy to %1. Progress is under Downloads.")).arg(page.exportDest))
            }
        }
        function onExportConflict(requestId, paths) {
            if (requestId !== page.exportRequest) return
            page.conflicts = paths
            overwriteConfirm.open()
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === page.saveRequest || requestId === page.exportRequest) (page.Window.window as Main).showError(message)
        }
        function onItemChanged(id) { if (id === page.itemId || page.kind === "show" || page.kind === "season") page.load() }
        function onLibraryChanged() { page.load() }
    }

    // Backdrop
    Image {
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: parent.height * 0.7
        source: page.d.backdrop || ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        opacity: 0.35
    }
    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0.0; color: "#600d1015" }
            GradientStop { position: 0.55; color: Theme.bg }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.implicitHeight + 80
        clip: true
        ScrollBar.vertical: ScrollBar {}

        FocusScope {
            id: content
            x: 48
            y: 48
            width: flick.width - 96
            implicitHeight: col.implicitHeight
            focus: true

            ColumnLayout {
                id: col
                width: parent.width
                spacing: 22

                RowLayout {
                    spacing: 32
                    Layout.fillWidth: true

                    Rectangle {
                        Layout.preferredWidth: page.kind === "episode" ? 360 : 240
                        Layout.preferredHeight: page.kind === "episode" ? 203 : 360
                        Layout.alignment: Qt.AlignTop
                        radius: Theme.radius
                        clip: true
                        color: Theme.surfaceHi
                        Label {
                            anchors.centerIn: parent
                            width: parent.width - 32
                            visible: detailPoster.status !== Image.Ready
                            text: page.d.title || ""
                            color: Theme.textDim
                            font.pixelSize: 22
                            font.bold: true
                            wrapMode: Text.Wrap
                            horizontalAlignment: Text.AlignHCenter
                        }
                        Image {
                            id: detailPoster
                            anchors.fill: parent
                            source: page.kind === "episode" ? (page.d.poster || "") : (page.d.posterFull || page.d.poster || "")
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: 10
                        Label {
                            visible: page.d.parentTitle !== undefined && page.d.parentTitle !== ""
                            text: page.kind === "episode" ? qsTr("%1 · Season %2, Episode %3").arg(page.d.parentTitle).arg(page.d.seasonNo).arg(page.d.episodeNo) : (page.d.parentTitle || "")
                            color: Theme.accent
                            font.pixelSize: Theme.fontBody
                        }
                        Label {
                            Layout.fillWidth: true
                            text: page.d.title || ""
                            color: Theme.text
                            font.pixelSize: 36
                            font.bold: true
                            wrapMode: Text.Wrap
                            Accessible.role: Accessible.Heading
                        }
                        Label {
                            visible: text !== ""
                            text: page.loaded ? page.metaLine() : ""
                            color: Theme.textDim
                            font.pixelSize: Theme.fontBody
                        }
                        // "4K · HDR10 · HEVC 10-bit · TrueHD Atmos 7.1 · 3 subtitles" and badges.
                        RowLayout {
                            visible: page.hasFiles && page.info !== null && !page.info.error
                            spacing: 8
                            Repeater {
                                model: page.info ? MediaInfo.badges(page.info) : []
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
                                text: page.info ? MediaInfo.summary(page.info) : ""
                                color: Theme.textDim
                                font.pixelSize: Theme.fontSmall
                            }
                        }
                        Label {
                            visible: (page.kind === "movie" || page.kind === "show") && (page.d.matchState === "needs_review" || page.d.matchState === "unmatched")
                            text: page.d.matchState === "needs_review" ? qsTr("Several matches are possible. Choose the right one with “Fix match”.") : qsTr("Not matched yet: titles come from the file names.")
                            color: Theme.accent
                            font.pixelSize: Theme.fontSmall
                        }
                        Label {
                            Layout.fillWidth: true
                            Layout.maximumWidth: 900
                            text: page.d.overview || ""
                            color: Theme.text
                            font.pixelSize: Theme.fontBody
                            wrapMode: Text.Wrap
                            maximumLineCount: 7
                            elide: Text.ElideRight
                            lineHeight: 1.2
                        }
                        Label {
                            visible: page.d.cast !== undefined && page.d.cast.length > 0
                            Layout.fillWidth: true
                            Layout.maximumWidth: 900
                            text: qsTr("With %1").arg((page.d.cast || []).slice(0, 6).join(", "))
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                            wrapMode: Text.Wrap
                        }

                        // Issue #34: the version to play, when there are several.
                        RowLayout {
                            visible: page.manyVersions
                            Layout.topMargin: 10
                            spacing: 12
                            Label { text: qsTr("Version"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                            FocusButton {
                                id: versionButton
                                text: (page.file ? MediaInfo.versionLabel(page.file) : "") + "  ▾"
                                tip: qsTr("Choose a version")
                                font.pixelSize: Theme.fontSmall
                                Accessible.name: qsTr("Version: %1").arg(page.file ? MediaInfo.versionLabel(page.file) : "")
                                KeyNavigation.down: playButton
                                KeyNavigation.right: splitButton.visible ? splitButton : null
                                Keys.onLeftPressed: Nav.focusRail()
                                onClicked: versionMenu.open()
                            }
                            FocusButton {
                                id: splitButton
                                visible: page.kind === "movie"
                                text: qsTr("This is a different movie")
                                font.pixelSize: Theme.fontSmall
                                KeyNavigation.left: versionButton
                                KeyNavigation.down: playButton
                                onClicked: splitConfirm.open()
                            }
                        }
                        Label {
                            visible: page.hasFiles && page.file !== null && !page.file.online
                            text: qsTr("This version's source cannot be reached right now.")
                            color: Theme.danger
                            font.pixelSize: Theme.fontSmall
                        }
                        Label {
                            visible: page.hasFiles && !page.sharedResume && (page.d.positionMs || 0) > 0 && !(page.file.positionMs > 0)
                            text: qsTr("Resume not available for this version (a different cut).")
                            color: Theme.textDim
                            font.pixelSize: Theme.fontSmall
                        }

                        RowLayout {
                            Layout.topMargin: 10
                            spacing: 12
                            FocusButton {
                                id: playButton
                                primary: true
                                focus: true
                                glyph: "▶"
                                text: page.loaded ? page.playLabel() : qsTr("Play")
                                enabled: !(page.hasFiles && page.file !== null && !page.file.online)
                                KeyNavigation.up: page.manyVersions ? versionButton : null
                                KeyNavigation.right: watchedButton
                                KeyNavigation.down: page.belowActions()
                                Keys.onLeftPressed: Nav.focusRail()
                                onClicked: page.play()
                            }
                            FocusButton {
                                id: watchedButton
                                text: page.d.watched || (page.kind !== "movie" && page.kind !== "episode" && page.d.unwatched === 0) ? qsTr("Mark unwatched") : qsTr("Mark watched")
                                KeyNavigation.left: playButton
                                KeyNavigation.right: fixButton.visible ? fixButton : null
                                KeyNavigation.down: page.belowActions()
                                onClicked: Core.setWatched(page.itemId, !(page.d.watched || (page.kind !== "movie" && page.kind !== "episode" && page.d.unwatched === 0)))
                            }
                            FocusButton {
                                id: fixButton
                                visible: Core.hasTmdbKey && (page.kind === "movie" || page.kind === "show")
                                text: qsTr("Fix match")
                                KeyNavigation.left: watchedButton
                                KeyNavigation.down: page.belowActions()
                                onClicked: fixDialog.openFor(page.itemId, page.d.title, page.d.year)
                            }
                        }

                        // Issue #4: Save (an offline copy), its progress, and deleting it.
                        RowLayout {
                            visible: page.canSave || page.canExport
                            spacing: 12
                            FocusButton {
                                id: saveButton
                                visible: page.canSave
                                // One button that changes its role, so focus never falls off the row.
                                glyph: page.saveBusy ? "❚❚" : page.sv.state === "done" ? "" : "⤓"
                                text: page.saveLabel()
                                Accessible.description: page.saveStatus()
                                KeyNavigation.up: playButton
                                KeyNavigation.right: deleteSavedButton.visible ? deleteSavedButton : exportButton
                                KeyNavigation.down: page.childView()
                                Keys.onLeftPressed: Nav.focusRail()
                                onActiveFocusChanged: if (activeFocus) page.reveal(this)
                                onClicked: page.saveAction()
                            }
                            FocusButton {
                                id: deleteSavedButton
                                visible: page.hasCopy && page.sv.state !== "done"
                                text: page.saveBusy || page.sv.state === "paused" ? qsTr("Cancel") : page.deleteLabel()
                                KeyNavigation.up: playButton
                                KeyNavigation.left: saveButton
                                KeyNavigation.right: exportButton
                                KeyNavigation.down: page.childView()
                                onActiveFocusChanged: if (activeFocus) page.reveal(this)
                                onClicked: deleteSavedConfirm.open()
                            }
                            FocusButton {
                                id: exportButton
                                visible: page.canExport
                                text: qsTr("Save as…")
                                Accessible.description: page.hasFiles ? qsTr("Copy the file to a place you choose")
                                                                      : qsTr("Copy every episode into a folder you choose")
                                KeyNavigation.up: playButton
                                KeyNavigation.left: deleteSavedButton.visible ? deleteSavedButton : saveButton.visible ? saveButton : null
                                KeyNavigation.down: page.childView()
                                Keys.onLeftPressed: (e) => {
                                    if (deleteSavedButton.visible) deleteSavedButton.forceActiveFocus()
                                    else if (saveButton.visible) saveButton.forceActiveFocus()
                                    else Nav.focusRail()
                                }
                                onActiveFocusChanged: if (activeFocus) page.reveal(this)
                                onClicked: {
                                    if (page.hasFiles) {
                                        const name = page.file.path.substring(page.file.path.lastIndexOf("/") + 1)
                                        exportFileDialog.selectedFile = exportFileDialog.currentFolder + "/" + encodeURIComponent(name)
                                        exportFileDialog.open()
                                    } else {
                                        exportFolderDialog.open()
                                    }
                                }
                            }
                            Label {
                                Layout.fillWidth: true
                                text: page.saveStatus()
                                color: page.sv.state === "failed" || page.sv.state === "outdated" ? Theme.danger : Theme.textDim
                                font.pixelSize: Theme.fontSmall
                                wrapMode: Text.Wrap
                            }
                        }
                        Rectangle {
                            visible: page.canSave && (page.saveBusy || page.sv.state === "paused")
                            Layout.preferredWidth: 360
                            Layout.preferredHeight: 4
                            radius: 2
                            color: Theme.surfaceHi
                            Rectangle {
                                width: parent.width * page.savePercent / 100
                                height: parent.height
                                radius: 2
                                color: Theme.accent
                            }
                        }
                    }
                }

                // Technical details (issue #33): collapsible, from the selected file's probe data.
                RowLayout {
                    visible: page.hasFiles
                    spacing: 12
                    Layout.topMargin: 12
                    FocusButton {
                        id: detailsToggle
                        glyph: page.detailsOpen ? "▾" : "▸"
                        text: qsTr("Technical details")
                        Keys.onUpPressed: (page.canSave ? saveButton : page.canExport ? exportButton : playButton).forceActiveFocus()
                        Keys.onLeftPressed: Nav.focusRail()
                        KeyNavigation.right: analyseButton.visible ? analyseButton : null
                        onActiveFocusChanged: if (activeFocus) page.reveal(this)
                        onClicked: {
                            page.detailsOpen = !page.detailsOpen
                            if (page.detailsOpen) Qt.callLater(() => page.reveal(detailsPanel))
                        }
                    }
                    Label {
                        visible: page.info === null || page.info.error !== undefined
                        text: page.info === null ? qsTr("Analysing…") : qsTr("This file could not be analysed.")
                        color: Theme.textDim
                        font.pixelSize: Theme.fontSmall
                    }
                    FocusButton {
                        id: analyseButton
                        visible: page.info === null || page.info.error !== undefined
                        text: qsTr("Analyse now")
                        KeyNavigation.left: detailsToggle
                        Keys.onUpPressed: playButton.forceActiveFocus()
                        onClicked: Core.probeNow(page.file.id)
                    }
                }
                GridLayout {
                    id: detailsPanel
                    visible: page.hasFiles && page.detailsOpen && page.info !== null && !page.info.error
                    Layout.fillWidth: true
                    Layout.maximumWidth: 1100
                    columns: Math.max(1, Math.min(3, Math.floor(width / 340)))
                    columnSpacing: 32
                    rowSpacing: 18
                    Repeater {
                        model: page.info && !page.info.error ? MediaInfo.groups(page.file, page.info) : []
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

                Label {
                    visible: page.childItems.length > 0
                    text: page.kind === "show" ? qsTr("Seasons") : qsTr("Episodes")
                    color: Theme.textDim
                    font.pixelSize: Theme.fontLarge
                    Layout.topMargin: 12
                }

                // Show: the season switcher. Tabs, or one button with a list for many seasons.
                RowLayout {
                    visible: page.kind === "show" && page.seasons.length > 0
                    Layout.fillWidth: true
                    spacing: 12
                    ListView {
                        id: seasonTabs
                        visible: !page.manySeasons
                        Layout.preferredWidth: Math.min(contentWidth, col.width - seasonWatched.width - 24)
                        Layout.preferredHeight: 48
                        orientation: ListView.Horizontal
                        spacing: 8
                        clip: true
                        interactive: false
                        model: page.manySeasons ? [] : page.seasons
                        currentIndex: page.seasonIndex
                        Accessible.role: Accessible.PageTabList
                        Accessible.name: qsTr("Seasons")
                        Keys.onLeftPressed: page.seasonIndex === 0 ? Nav.focusRail() : page.selectSeason(page.seasonIndex - 1)
                        Keys.onRightPressed: {
                            if (page.seasonIndex < page.seasons.length - 1) page.selectSeason(page.seasonIndex + 1)
                            else seasonWatched.forceActiveFocus()
                        }
                        Keys.onPressed: (e) => { if (e.key === Qt.Key_W) { seasonWatched.toggle(); e.accepted = true } }
                        Keys.onUpPressed: playButton.forceActiveFocus()
                        Keys.onDownPressed: if (episodeList.count > 0) episodeList.forceActiveFocus()
                        onActiveFocusChanged: if (activeFocus) page.reveal(this)
                        delegate: FocusButton {
                            id: tab
                            required property var modelData
                            required property int index
                            text: modelData.title
                            primary: index === page.seasonIndex
                            focus: ListView.isCurrentItem
                            focusPolicy: Qt.NoFocus
                            opacity: modelData.onDisk ? 1 : 0.5
                            Accessible.role: Accessible.PageTab
                            Accessible.name: modelData.title + (modelData.onDisk ? "" : ", " + qsTr("not in your library"))
                            Accessible.selected: index === page.seasonIndex
                            onClicked: page.selectSeason(index)
                        }
                    }
                    FocusButton {
                        id: seasonPicker
                        visible: page.manySeasons
                        text: (page.season ? page.season.title : "") + "  ▾"
                        tip: qsTr("Choose a season")
                        Keys.onLeftPressed: page.seasonIndex === 0 ? Nav.focusRail() : page.selectSeason(page.seasonIndex - 1)
                        Keys.onRightPressed: {
                            if (page.seasonIndex < page.seasons.length - 1) page.selectSeason(page.seasonIndex + 1)
                            else seasonWatched.forceActiveFocus()
                        }
                        Keys.onPressed: (e) => { if (e.key === Qt.Key_W) { seasonWatched.toggle(); e.accepted = true } }
                        Keys.onUpPressed: playButton.forceActiveFocus()
                        Keys.onDownPressed: if (episodeList.count > 0) episodeList.forceActiveFocus()
                        onActiveFocusChanged: if (activeFocus) page.reveal(this)
                        onClicked: seasonMenu.open()
                    }
                    Item { Layout.fillWidth: true }
                    FocusButton {
                        id: seasonWatched
                        readonly property bool done: page.season !== null && page.season.onDisk && Number(page.season.unwatched) === 0
                        visible: page.season !== null && page.season.onDisk
                        text: done ? qsTr("Mark season unwatched") : qsTr("Mark season watched")
                        Keys.onLeftPressed: page.seasonBar().forceActiveFocus()
                        Keys.onUpPressed: playButton.forceActiveFocus()
                        Keys.onDownPressed: if (episodeList.count > 0) episodeList.forceActiveFocus()
                        // W on the season tabs does the same.
                        function toggle() { if (visible) Core.setWatched(page.season.id, !done) }
                        onClicked: toggle()
                    }
                }

                // The episodes of the selected season (show) or of this season (season page).
                ListView {
                    id: episodeList
                    visible: (page.kind === "season" || page.kind === "show") && count > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: contentHeight
                    interactive: false
                    spacing: 8
                    model: page.episodes
                    keyNavigationEnabled: true
                    Keys.onUpPressed: (e) => {
                        if (currentIndex > 0) { e.accepted = false; return }
                        if (page.kind === "show") page.seasonBar().forceActiveFocus()
                        else playButton.forceActiveFocus()
                    }
                    Keys.onLeftPressed: {
                        if (page.kind === "show" && page.seasonIndex > 0) { page.selectSeason(page.seasonIndex - 1); currentIndex = 0 }
                        else Nav.focusRail()
                    }
                    Keys.onRightPressed: {
                        if (page.kind === "show" && page.seasonIndex < page.seasons.length - 1) { page.selectSeason(page.seasonIndex + 1); currentIndex = 0 }
                    }
                    onCurrentIndexChanged: if (activeFocus && currentItem) page.reveal(currentItem)
                    onActiveFocusChanged: if (activeFocus && currentItem) page.reveal(currentItem)
                    delegate: EpisodeRow {
                        required property var modelData
                        width: episodeList.width
                        ep: modelData
                        focus: ListView.isCurrentItem
                        onPlay: Playback.openItem(modelData.id)
                        onInfo: Nav.openItem(modelData.id)
                        onToggleWatched: Core.setWatched(modelData.id, !modelData.watched)
                    }
                }
            }
        }
    }

    // Many seasons: the list behind the season button.
    Popup {
        id: seasonMenu
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: 360
        height: Math.min(480, seasonMenuList.contentHeight + 24)
        modal: true
        padding: 12
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: { seasonMenuList.currentIndex = page.seasonIndex; seasonMenuList.forceActiveFocus() }
        onClosed: seasonPicker.forceActiveFocus()
        ListView {
            id: seasonMenuList
            anchors.fill: parent
            clip: true
            model: page.seasons
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Seasons")
            delegate: ItemDelegate {
                id: menuRow
                required property var modelData
                required property int index
                width: seasonMenuList.width
                focus: ListView.isCurrentItem
                opacity: modelData.onDisk ? 1 : 0.5
                text: modelData.title
                highlighted: index === page.seasonIndex
                contentItem: Label { text: menuRow.text; color: Theme.text; font.bold: menuRow.highlighted }
                background: Rectangle {
                    color: menuRow.activeFocus ? Theme.surfaceHi : "transparent"
                    radius: 6
                    border.width: menuRow.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                onClicked: { page.selectSeason(index); seasonMenu.close() }
                Keys.onReturnPressed: clicked()
                Keys.onEnterPressed: clicked()
            }
        }
    }

    // Issue #34: the versions, offline ones listed but not selectable.
    Popup {
        id: versionMenu
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(720, parent.width - 80)
        height: Math.min(480, versionList.contentHeight + 24)
        modal: true
        padding: 12
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        // After opening, so the list has its rows: start on the selected version.
        onOpened: Qt.callLater(() => { versionList.currentIndex = page.fileIndex; versionList.forceActiveFocus() })
        onClosed: versionButton.forceActiveFocus()
        ListView {
            id: versionList
            anchors.fill: parent
            clip: true
            spacing: 4
            model: page.files
            keyNavigationEnabled: true
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Versions")
            delegate: ItemDelegate {
                id: versionRow
                required property var modelData
                required property int index
                width: versionList.width
                focus: ListView.isCurrentItem
                enabled: modelData.online
                highlighted: index === page.fileIndex
                text: MediaInfo.versionLabel(modelData) + (modelData.online ? "" : "  ·  " + qsTr("offline"))
                Accessible.name: text
                contentItem: Label {
                    text: versionRow.text
                    color: versionRow.enabled ? Theme.text : Theme.textDim
                    font.bold: versionRow.highlighted
                    elide: Text.ElideRight
                }
                background: Rectangle {
                    color: versionRow.activeFocus ? Theme.surfaceHi : "transparent"
                    radius: 6
                    border.width: versionRow.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                }
                onClicked: { page.chooseVersion(index); versionMenu.close() }
                Keys.onReturnPressed: clicked()
                Keys.onEnterPressed: clicked()
            }
        }
    }

    // Issue #34: "This is a different movie" moves the selected version into its own entry.
    Popup {
        id: splitConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: splitCancel.forceActiveFocus()
        onClosed: splitButton.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("Move “%1” into its own entry? It will not be merged with this movie again, and you can choose its match under Settings → Review matches.").arg(page.file ? MediaInfo.versionLabel(page.file) : "")
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: splitCancel
                    text: qsTr("Cancel")
                    KeyNavigation.right: splitOk
                    onClicked: splitConfirm.close()
                }
                FocusButton {
                    id: splitOk
                    primary: true
                    text: qsTr("Move to its own entry")
                    KeyNavigation.left: splitCancel
                    onClicked: {
                        page.splitRequest = Core.splitFile(page.file.id)
                        splitConfirm.close()
                    }
                }
            }
        }
    }

    // Issue #4: deleting a saved copy (or cancelling one) only removes the local files.
    Popup {
        id: deleteSavedConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: deleteKeep.forceActiveFocus()
        onClosed: saveButton.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: page.saveBusy || page.sv.state === "paused"
                      ? qsTr("Stop saving and remove what was copied so far? The file on the server stays.")
                      : qsTr("Delete the saved copy from this computer? Only the local copy is removed; the file on the server stays and can be streamed or saved again.")
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: deleteKeep
                    text: qsTr("Keep")
                    KeyNavigation.right: deleteOk
                    onClicked: deleteSavedConfirm.close()
                }
                FocusButton {
                    id: deleteOk
                    primary: true
                    text: page.saveBusy || page.sv.state === "paused" ? qsTr("Stop and remove") : qsTr("Delete saved copy")
                    KeyNavigation.left: deleteKeep
                    onClicked: {
                        page.saveRequest = Core.deleteSavedItem(page.itemId)
                        deleteSavedConfirm.close()
                    }
                }
            }
        }
    }

    // Issue #5: Save as…. The native dialog asks before replacing a file; for a folder the core
    // reports existing files and the page asks.
    FileDialog {
        id: exportFileDialog
        title: qsTr("Save a copy as")
        fileMode: FileDialog.SaveFile
        currentFolder: Core.exportFolder()
        // Focus goes back to the page either way, or keyboard and remote stop working.
        onAccepted: { exportButton.forceActiveFocus(); page.exportTo(Core.localPath(selectedFile), true) }
        onRejected: exportButton.forceActiveFocus()
    }
    FolderDialog {
        id: exportFolderDialog
        title: qsTr("Copy every episode into")
        currentFolder: Core.exportFolder()
        onAccepted: { exportButton.forceActiveFocus(); page.exportTo(Core.localPath(selectedFolder), false) }
        onRejected: exportButton.forceActiveFocus()
    }
    Popup {
        id: overwriteConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: overwriteKeep.forceActiveFocus()
        onClosed: exportButton.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("%n file(s) exist already in %1. Replace them?", "", page.conflicts.length).arg(page.exportDest)
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: overwriteKeep
                    text: qsTr("Cancel")
                    KeyNavigation.right: overwriteOk
                    onClicked: overwriteConfirm.close()
                }
                FocusButton {
                    id: overwriteOk
                    primary: true
                    text: qsTr("Replace")
                    KeyNavigation.left: overwriteKeep
                    onClicked: {
                        page.exportTo(page.exportDest, true)
                        overwriteConfirm.close()
                    }
                }
            }
        }
    }

    FixMatchDialog {
        id: fixDialog
        anchors.centerIn: parent
    }
}
