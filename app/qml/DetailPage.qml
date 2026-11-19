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
// Episodes are tiles (issue #48) with their own version, technical summary, details panel and
// Save menu; they are the page's list, loaded page by page as it scrolls, with everything above
// them as its header. Keys on a show: left/right on the season tabs switch seasons, up/down move
// through episodes, W marks the focused episode or (on the season tabs) the whole season watched.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: d.title || ""
    readonly property string pageKind: kind === "show" ? qsTr("Show") : kind === "season" ? qsTr("Season") : kind === "episode" ? qsTr("Episode") : qsTr("Movie")
    property var itemId
    property var d: ({})
    property var request: 0
    readonly property string kind: d.kind || ""
    readonly property var childItems: d.children || []
    readonly property bool loaded: d.id !== undefined

    // Show page: the seasons (specials last), the selected one and its episodes.
    readonly property var seasons: kind === "show" ? childItems : []
    property int seasonIndex: -1
    readonly property var season: seasonIndex >= 0 && seasonIndex < seasons.length ? seasons[seasonIndex] : null
    readonly property bool manySeasons: seasons.length > 8
    // The season whose episodes the tiles show (issue #48).
    readonly property var tileSeason: kind === "show" ? (season ? season.id : 0) : kind === "season" ? itemId : 0
    readonly property bool hasTiles: tileSeason > 0 && episodeModel.count > 0

    function load() { request = Core.itemDetail(itemId) }
    function selectSeason(i) {
        if (seasons.length === 0) return
        seasonIndex = Math.max(0, Math.min(i, seasons.length - 1))
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
        if (kind === "season" && hasTiles) return flick.itemAtIndex(0)
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

    // Issue #48: the episode tiles' view state: chosen versions, the one open details panel.
    property var chosen: ({})
    property var expandedId: 0
    property var expandedRequest: 0
    property var expandedDetail: null
    property var tileRequests: []
    function chooseTileVersion(epId, fileId) {
        const c = Object.assign({}, chosen)
        c[epId] = fileId
        chosen = c
    }
    function toggleTileDetails(epId) {
        if (expandedId === epId) { expandedId = 0; expandedDetail = null; return }
        expandedId = epId
        expandedDetail = null
        expandedRequest = Core.itemDetail(epId)
    }
    function fullFileOf(epId, fileId) {
        if (expandedId !== epId || !expandedDetail) return null
        return expandedDetail.files.find(f => f.id === fileId) || null
    }
    // A tile, by its index in the list, gets focus (and is scrolled into view).
    function focusTile(i) {
        if (i < 0 || i >= episodeModel.count) return
        flick.currentIndex = i
        const t = flick.currentItem
        if (!t) return
        t.forceActiveFocus()
        reveal(t)
    }
    // Above the first tile: the season switcher (show) or the save row (season page).
    function aboveTiles() {
        if (kind === "show") return seasonBar()
        return canSave ? saveButton : canExport ? exportButton : playButton
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
        if (sv.state === "done") {
            returnFocus = saveButton
            confirmTarget = { itemId: itemId, busy: false, many: true }
            deleteSavedConfirm.open()
            return
        }
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
    // What Save as… copies (this page's item, or an episode tile's) and where focus returns.
    property var exportTarget: ({ itemId: 0, fileId: 0 })
    property Item returnFocus: null
    function exportTo(dest, overwrite) {
        exportDest = dest
        exportRequest = Core.exportItem(exportTarget.itemId, exportTarget.fileId, dest, overwrite)
    }
    function refocus() { (returnFocus || playButton).forceActiveFocus() }
    // Issue #48: one Save menu action (SaveMenu.triggered) for a movie or episode, from this
    // page's menu or an episode tile's. `sv` is that item's save state, `from` gets focus back.
    function saveFor(id, fileId, path, action, info, from) {
        returnFocus = from
        const req = (r) => { if (id !== itemId) tileRequests = tileRequests.concat([r]); else saveRequest = r }
        switch (action) {
        case "save": case "saveAgain": req(Core.saveItem(id, fileId)); break
        case "pause": Core.pauseSavedItem(id); break
        case "resume": case "retry": Core.resumeSavedItem(id); break
        case "cancel": case "delete":
            confirmTarget = { itemId: id, busy: info.state === "queued" || info.state === "running" || info.state === "paused", many: false }
            deleteSavedConfirm.open()
            return
        case "export":
            exportTarget = { itemId: id, fileId: fileId }
            const name = path.substring(path.lastIndexOf("/") + 1)
            exportFileDialog.selectedFile = exportFileDialog.currentFolder + "/" + encodeURIComponent(name)
            exportFileDialog.open()
            return
        }
        if (id === itemId) saveTimer.restart()
        else episodeModel.refresh()
    }
    // The item a delete or cancel asks about.
    property var confirmTarget: ({ itemId: 0, busy: false, many: false })
    function belowActions() {
        if (hasFiles) return pageSave.visible ? pageSave : childView()
        return canSave ? saveButton : canExport ? exportButton : childView()
    }
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
        const top = flick.originY, bottom = Math.max(flick.originY, flick.originY + flick.contentHeight - flick.height)
        if (p.y < flick.contentY + 40) flick.contentY = Math.max(top, p.y - 40)
        else if (p.y + item.height > flick.contentY + flick.height - 40)
            flick.contentY = Math.min(p.y + item.height - flick.height + 40, bottom)
        stickTop = flick.contentY <= top + 1
    }
    // The view shows the top of the page (until it is scrolled or focus moves down).
    property bool stickTop: true
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
    onActiveFocusChanged: if (activeFocus && loaded && !flick.activeFocus) playButton.forceActiveFocus()

    EpisodeModel {
        id: episodeModel
        season: page.tileSeason
        onTotalChanged: if (Bench.mode === "episodes" && total > 0 && !benchSave.running && !benchScroll.running) benchSave.start()
    }

    // --benchmark episodes (issue #48): save the first ten episodes that can be saved, then
    // scroll the tiles on their own while they copy, and record frame times.
    Timer {
        id: benchSave
        interval: 1500
        onTriggered: {
            let n = 0
            for (let i = 0; i < episodeModel.count && n < 10; ++i) {
                const ep = episodeModel.get(i)
                const f = (ep.files || []).find(x => x.sourceKind !== "local" && x.online)
                if (!f || ep.save.state !== "none") continue
                Core.saveItem(ep.id, f.id)
                n++
            }
            benchStart.start()
        }
    }
    Timer { id: benchStart; interval: 1000; onTriggered: { Bench.beginFrames(); benchScroll.start() } }
    FrameAnimation {
        id: benchScroll
        property real travelled: 0
        onTriggered: {
            const step = Math.min(frameTime, 0.05) * 2500
            const max = flick.originY + flick.contentHeight - flick.height
            flick.contentY = Math.min(flick.contentY + step, max)
            travelled += step
            if (elapsedTime > 20 || (flick.contentY >= max && episodeModel.count >= episodeModel.total)) {
                stop()
                Bench.endFrames(episodeModel.count, travelled)
            }
        }
    }

    Connections {
        target: Core
        function onDetailReady(requestId, detail) {
            if (requestId === page.expandedRequest) {
                if (detail.id === page.expandedId) page.expandedDetail = detail
                return
            }
            if (requestId === page.splitRequest) {
                // The version is now its own entry; this page shows the remaining versions.
                (page.Window.window as Main).showError(qsTr("“%1” is now a separate entry. Choose its match under Settings → Matching → Review matches.").arg(detail.title))
                page.selectedFileId = 0
                page.load()
                return
            }
            if (requestId !== page.request) return
            const first = !page.loaded
            page.liveSave = null
            page.d = detail
            if (page.hasFiles) page.pickVersion()
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
            if (page.tileRequests.indexOf(requestId) >= 0) {
                page.tileRequests = page.tileRequests.filter(r => r !== requestId)
                episodeModel.refresh()
            }
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
            if (requestId === page.saveRequest || requestId === page.exportRequest || page.tileRequests.indexOf(requestId) >= 0)
                (page.Window.window as Main).showError(message)
        }
        function onItemChanged(id) {
            if (id === page.itemId || page.kind === "show" || page.kind === "season") page.load()
            // The open details panel follows its episode (a finished analysis, a new version).
            if (id === page.expandedId) page.expandedRequest = Core.itemDetail(id)
        }
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

    // The page scrolls as one list: the episode tiles, with everything above them as the header
    // (`content`, placed into the list's content over a header of its height). Only tiles near the
    // visible part exist, and pages of episodes load as the list scrolls.
    ListView {
        id: flick
        objectName: "tileList"
        anchors.fill: parent
        clip: true
        focus: true
        keyNavigationEnabled: false
        spacing: 8
        cacheBuffer: 800
        model: page.tileSeason > 0 ? episodeModel : null
        ScrollBar.vertical: ScrollBar {}
        // The header grows while the page loads; at the top of the page, the view stays there.
        header: Item {
            width: flick.width
            height: content.implicitHeight + 48 + 22
            onHeightChanged: if (page.stickTop) Qt.callLater(() => flick.positionViewAtBeginning())
        }
        onMovementStarted: page.stickTop = false
        footer: Item { width: flick.width; height: 80 }
        // Scrolled with the mouse: the focused tile stays the current one, so it is never released.
        Accessible.role: Accessible.List
        Accessible.name: qsTr("Episodes")
        delegate: FocusScope {
            id: cell
            required property var model
            required property int index
            width: flick.width
            height: tile.implicitHeight
            EpisodeTile {
                id: tile
                x: 48
                width: parent.width - 96
                height: parent.height
                focus: true
                ep: cell.model
                chosenFileId: page.chosen[cell.model.id] || 0
                preference: Appearance.preferredVersion
                expanded: page.expandedId === cell.model.id
                fullFile: expanded && file ? page.fullFileOf(cell.model.id, file.id) : null
                onActiveFocusChanged: if (activeFocus) { flick.currentIndex = cell.index; page.reveal(cell) }
                onHeightChanged: if (activeFocus) Qt.callLater(() => page.reveal(cell))
                onPlay: (fileId) => { if (fileId) Playback.openItemFile(cell.model.id, fileId); else Playback.openItem(cell.model.id) }
                onInfo: Nav.openItem(cell.model.id)
                onToggleWatched: Core.setWatched(cell.model.id, !cell.model.watched)
                onChooseVersion: (fileId) => page.chooseTileVersion(cell.model.id, fileId)
                onToggleDetails: page.toggleTileDetails(cell.model.id)
                onAnalyse: (fileId) => Core.probeNow(fileId)
                onSaveAction: (action, fileId) => page.saveFor(cell.model.id, fileId, file ? file.path : "", action, sv, cell.Window.activeFocusItem || tile)
                onMoveUp: cell.index > 0 ? page.focusTile(cell.index - 1) : page.aboveTiles().forceActiveFocus()
                onMoveDown: page.focusTile(cell.index + 1)
                onLeave: Nav.focusRail()
            }
        }
    }
        FocusScope {
            id: content
            parent: flick.contentItem
            x: 48
            y: (flick.headerItem ? flick.headerItem.y : 0) + 48
            width: flick.width - 96
            implicitHeight: col.implicitHeight
            focus: true
            Keys.onPressed: (e) => {
                if (e.key === Qt.Key_Menu && page.hasFiles && pageSave.visible) { pageSave.openMenu(content.Window.activeFocusItem); e.accepted = true }
            }
            // Issue #134: Up from the top row goes to the Back button (before the list takes it).
            Keys.onUpPressed: (e) => { e.accepted = Nav.focusBack() }

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
                        TechSummary {
                            Layout.fillWidth: true
                            visible: page.hasFiles && usable
                            showPending: false
                            info: page.info
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
                            VersionPicker {
                                id: versionButton
                                files: page.files
                                index: page.fileIndex
                                KeyNavigation.down: playButton
                                KeyNavigation.right: splitButton.visible ? splitButton : null
                                Keys.onLeftPressed: Nav.focusRail()
                                onChosen: (i) => page.chooseVersion(i)
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
                            text: page.file && page.file.saved ? qsTr("This version's source cannot be reached right now; the saved copy plays.")
                                                               : qsTr("This version's source cannot be reached right now.")
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
                                enabled: !(page.hasFiles && page.file !== null && !MediaInfo.playable(page.file))
                                Keys.onUpPressed: page.manyVersions ? versionButton.forceActiveFocus() : Nav.focusBack()
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
                                visible: Core.hasTmdbKey && (page.kind === "movie" || page.kind === "show" || page.kind === "episode")
                                text: qsTr("Fix match")
                                KeyNavigation.left: watchedButton
                                KeyNavigation.down: page.belowActions()
                                onClicked: fixDialog.openFor(page.itemId, page.d.title, page.d.year, page.kind)
                            }
                        }

                        // Issues #4, #5 and #48: a movie's or episode's Save menu (Save, Save as…, Delete
                        // saved copy), with its state and progress beside it.
                        RowLayout {
                            visible: page.hasFiles
                            spacing: 12
                            SaveMenu {
                                id: pageSave
                                sv: page.sv
                                networkFile: page.file !== null && page.file.sourceKind !== "local"
                                KeyNavigation.up: playButton
                                KeyNavigation.down: page.childView()
                                Keys.onLeftPressed: Nav.focusRail()
                                onActiveFocusChanged: if (activeFocus) page.reveal(this)
                                onTriggered: (action) => page.saveFor(page.itemId, page.file ? page.file.id : 0, page.file ? page.file.path : "", action, page.sv, pageSave)
                            }
                            Label {
                                Layout.fillWidth: true
                                text: page.saveStatus()
                                color: page.sv.state === "failed" || page.sv.state === "outdated" ? Theme.danger : Theme.textDim
                                font.pixelSize: Theme.fontSmall
                                wrapMode: Text.Wrap
                            }
                        }
                        // Seasons and shows (issue #4): Save season or Save all, its progress, and deleting.
                        RowLayout {
                            visible: !page.hasFiles && (page.canSave || page.canExport)
                            spacing: 12
                            FocusButton {
                                id: saveButton
                                visible: page.canSave && !page.hasFiles
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
                                visible: !page.hasFiles && page.hasCopy && page.sv.state !== "done"
                                text: page.saveBusy || page.sv.state === "paused" ? qsTr("Cancel") : page.deleteLabel()
                                KeyNavigation.up: playButton
                                KeyNavigation.left: saveButton
                                KeyNavigation.right: exportButton
                                KeyNavigation.down: page.childView()
                                onActiveFocusChanged: if (activeFocus) page.reveal(this)
                                onClicked: {
                                    page.returnFocus = deleteSavedButton
                                    page.confirmTarget = { itemId: page.itemId, busy: page.saveBusy || page.sv.state === "paused", many: true }
                                    deleteSavedConfirm.open()
                                }
                            }
                            FocusButton {
                                id: exportButton
                                visible: page.canExport && !page.hasFiles
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
                                    page.returnFocus = exportButton
                                    page.exportTarget = { itemId: page.itemId, fileId: 0 }
                                    exportFolderDialog.open()
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
                            visible: (page.canSave || page.hasFiles) && (page.saveBusy || page.sv.state === "paused")
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
                        Keys.onUpPressed: (pageSave.visible ? pageSave : playButton).forceActiveFocus()
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
                TechDetails {
                    id: detailsPanel
                    visible: page.hasFiles && page.detailsOpen && page.info !== null && !page.info.error
                    Layout.fillWidth: true
                    Layout.maximumWidth: 1100
                    file: page.file
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
                        Keys.onDownPressed: if (page.hasTiles) page.focusTile(0)
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
                        Keys.onDownPressed: if (page.hasTiles) page.focusTile(0)
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
                        Keys.onDownPressed: if (page.hasTiles) page.focusTile(0)
                        // W on the season tabs does the same.
                        function toggle() { if (visible) Core.setWatched(page.season.id, !done) }
                        onClicked: toggle()
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
                text: qsTr("Move “%1” into its own entry? It will not be merged with this movie again, and you can choose its match under Settings → Matching → Review matches.").arg(page.file ? MediaInfo.versionLabel(page.file) : "")
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
        onClosed: page.refocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: page.confirmTarget.busy
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
                    text: page.confirmTarget.busy ? qsTr("Stop and remove") : page.confirmTarget.many ? qsTr("Delete saved copies") : qsTr("Delete saved copy")
                    KeyNavigation.left: deleteKeep
                    onClicked: {
                        const id = page.confirmTarget.itemId
                        const r = Core.deleteSavedItem(id)
                        if (id === page.itemId) page.saveRequest = r
                        else page.tileRequests = page.tileRequests.concat([r])
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
        onAccepted: { page.refocus(); page.exportTo(Core.localPath(selectedFile), true) }
        onRejected: page.refocus()
    }
    FolderDialog {
        id: exportFolderDialog
        title: qsTr("Copy every episode into")
        currentFolder: Core.exportFolder()
        onAccepted: { page.refocus(); page.exportTo(Core.localPath(selectedFolder), false) }
        onRejected: page.refocus()
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
        onClosed: page.refocus()
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

    // Issues #97 to #100: the page moves to the item that carries the chosen match (a merge or a
    // movie that became a show).
    FixMatchDialog {
        id: fixDialog
        backend: Core
        onChosen: (newItemId) => {
            if (newItemId !== undefined && newItemId !== page.itemId) page.itemId = newItemId
            page.load()
            fixButton.forceActiveFocus()
        }
    }
}
