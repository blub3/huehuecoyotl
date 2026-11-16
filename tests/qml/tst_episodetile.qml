import QtQuick
import QtTest
import CoyotlUi

// Issue #48: focus and key navigation across episode tiles and their controls, the version
// dropdown, the Save menu's actions and the marks, with the episodes of the "Example Show" fixture.
Item {
    id: root
    width: 1280
    height: 1000

    property int backs: 0
    property var log: []
    property var chosen: ({})
    // Back that no tile handled leaves the page.
    Keys.onEscapePressed: backs++

    readonly property var probe1080: ({ video: [{ codec: "h264", width: 1920, height: 1080 }],
                                        audio: [{ codec: "aac", layout: "stereo", default: true }],
                                        subtitles: [{ language: "eng" }, { language: "ger" }] })
    readonly property var probe2160: ({ video: [{ codec: "hevc", width: 3840, height: 2160, hdr: "HDR10" }],
                                        audio: [{ codec: "truehd", profile: "Dolby TrueHD + Dolby Atmos", layout: "7.1" }] })
    readonly property var eps: [
        // E1: two versions, the larger one on a source that is offline.
        { id: 61, episodeNo: 1, title: "One", onDisk: true, lastFileId: 0,
          files: [{ id: 702, path: "/share/S01E01.2160p.mkv", size: 9e9, sourceName: "NAS", sourceKind: "smb", online: false, probe: probe2160 },
                  { id: 701, path: "/m/S01E01.1080p.mkv", size: 2e9, sourceName: "Media", sourceKind: "local", online: true, probe: probe1080 }],
          save: { state: "none", saveable: 1 } },
        // E2: only TMDB knows it.
        { id: 62, episodeNo: 2, title: "Two", onDisk: false, files: [], save: { state: "none", saveable: 0 } },
        // E3: saved.
        { id: 63, episodeNo: 3, title: "Three", onDisk: true,
          files: [{ id: 703, path: "/share/S01E03.mkv", size: 3e9, sourceName: "NAS", sourceKind: "smb", online: true, saved: true, probe: probe1080 }],
          save: { state: "done", saveable: 1, saved: 1 } },
        // E4: not analysed yet, on this computer only.
        { id: 64, episodeNo: 4, title: "Four", onDisk: true,
          files: [{ id: 704, path: "/m/S01E04.mkv", size: 3e9, sourceName: "Media", sourceKind: "local", online: true, probe: null }],
          save: { state: "none", saveable: 0 } },
        // E5: saving, 30 %.
        { id: 65, episodeNo: 5, title: "Five", onDisk: true,
          files: [{ id: 705, path: "/share/S01E05.mkv", size: 1000, sourceName: "NAS", sourceKind: "smb", online: true, probe: probe1080 }],
          save: { state: "running", saveable: 1, bytesDone: 300, size: 1000 } },
        // E6: the saved copy is outdated.
        { id: 66, episodeNo: 6, title: "Six", onDisk: true,
          files: [{ id: 706, path: "/share/S01E06.mkv", size: 1000, sourceName: "NAS", sourceKind: "smb", online: true, probe: probe1080 }],
          save: { state: "outdated", saveable: 1 } }
    ]

    ListView {
        id: list
        anchors.fill: parent
        keyNavigationEnabled: false
        spacing: 8
        cacheBuffer: 4000
        model: root.eps
        delegate: EpisodeTile {
            id: t
            required property var modelData
            required property int index
            width: list.width
            height: implicitHeight
            ep: modelData
            chosenFileId: root.chosen[modelData.id] || 0
            onMoveUp: if (index > 0) (list.itemAtIndex(index - 1) as EpisodeTile).focusBody()
            onMoveDown: if (index < list.count - 1) (list.itemAtIndex(index + 1) as EpisodeTile).focusBody()
            onLeave: root.log = root.log.concat(["leave"])
            onPlay: (fileId) => root.log = root.log.concat(["play:" + fileId])
            onInfo: root.log = root.log.concat(["info:" + modelData.id])
            onToggleDetails: t.expanded = !t.expanded
            onChooseVersion: (fileId) => {
                const c = Object.assign({}, root.chosen)
                c[modelData.id] = fileId
                root.chosen = c
            }
            onSaveAction: (action, fileId) => root.log = root.log.concat([action + ":" + fileId])
        }
    }

    TestCase {
        name: "EpisodeTile"
        when: windowShown

        function tile(i) { return list.itemAtIndex(i) }
        function part(i, name) { return findChild(tile(i), name) }
        function focused() { return root.Window.activeFocusItem }

        function init() {
            root.backs = 0
            root.log = []
            root.chosen = ({})
            tile(0).focusBody()
            wait(20)
        }

        function test_upAndDownMoveBetweenTiles() {
            verify(part(0, "tileBody").activeFocus)
            keyClick(Qt.Key_Down)
            verify(part(1, "tileBody").activeFocus, "Down reaches the next tile, also a dimmed one")
            keyClick(Qt.Key_Down)
            verify(part(2, "tileBody").activeFocus)
            keyClick(Qt.Key_Up)
            keyClick(Qt.Key_Up)
            verify(part(0, "tileBody").activeFocus)
            keyClick(Qt.Key_Left)
            compare(root.log, ["leave"], "Left on a tile goes to the navigation rail")
        }

        function test_rightEntersTheControlsAndBackLeavesThem() {
            keyClick(Qt.Key_Right)
            verify(part(0, "tileVersions").activeFocus, "the version dropdown comes first")
            keyClick(Qt.Key_Right)
            verify(part(0, "tileDetails").activeFocus)
            keyClick(Qt.Key_Right)
            verify(part(0, "tileSave").activeFocus)
            keyClick(Qt.Key_Left)
            keyClick(Qt.Key_Left)
            verify(part(0, "tileVersions").activeFocus)
            keyClick(Qt.Key_Escape)
            verify(part(0, "tileBody").activeFocus, "Back leaves the controls")
            compare(root.backs, 0)
            keyClick(Qt.Key_Escape)
            compare(root.backs, 1, "a second Back leaves the page")
        }

        function test_upAndDownLeaveTheControls() {
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Right)
            verify(part(0, "tileDetails").activeFocus)
            keyClick(Qt.Key_Down)
            verify(part(1, "tileBody").activeFocus)
            tile(2).focusBody()
            keyClick(Qt.Key_Right)
            verify(part(2, "tileDetails").activeFocus, "one version: no dropdown, Details first")
            keyClick(Qt.Key_Up)
            verify(part(1, "tileBody").activeFocus)
        }

        function test_versionDropdownOnlyWithSeveralVersions() {
            verify(part(0, "tileVersions").visible)
            for (const i of [2, 3, 4, 5]) verify(!part(i, "tileVersions").visible, "episode " + (i + 1))
        }

        function test_dimmedEpisodeHasNoControls() {
            tile(1).focusBody()
            for (const name of ["tileVersions", "tileDetails", "tileSave", "tileSummary"])
                verify(!part(1, name).visible, name)
            keyClick(Qt.Key_Right)
            verify(part(1, "tileBody").activeFocus)
            keyClick(Qt.Key_Return)
            compare(root.log, ["info:62"], "Enter opens its page instead of playing")
        }

        function test_offlineVersionIsMarkedAndCannotBeChosen() {
            // The 2160p version is offline: the online one is preselected and plays.
            compare(tile(0).file.id, 701)
            keyClick(Qt.Key_Return)
            compare(root.log, ["play:701"])
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Return)
            const versions = findChild(root.Window.contentItem, "versionList")
            tryVerify(() => versions && versions.activeFocus)
            const offline = versions.itemAtIndex(0)
            verify(!offline.enabled)
            verify(offline.text.indexOf("offline") >= 0)
            keyClick(Qt.Key_Escape)
            tryVerify(() => part(0, "tileVersions").activeFocus, 1000, "focus returns to the dropdown")
        }

        function test_switchingTheVersionDrivesTheTile() {
            // E1 with both versions online.
            const before = root.eps[0].files
            tile(0).ep = Object.assign({}, root.eps[0], { files: [Object.assign({}, before[0], { online: true }), before[1]] })
            compare(tile(0).file.id, 702, "the best version is preselected")
            tile(0).chooseVersion(701)
            compare(tile(0).file.id, 701)
            keyClick(Qt.Key_Return)
            const save = part(0, "tileSave")
            save.triggered("export")
            compare(root.log, ["play:701", "export:701"], "Play and Save as… take the selected version")
            verify(!save.canSave, "a version on this computer cannot be saved")
            tile(0).chooseVersion(702)
            verify(save.canSave)
            save.triggered("save")
            compare(root.log[2], "save:702")
            tile(0).ep = root.eps[0]
        }

        function test_menuKeyOpensTheSaveMenu() {
            tile(2).focusBody()
            keyClick(Qt.Key_Menu)
            const items = findChild(root.Window.contentItem, "saveMenuList")
            tryVerify(() => items && items.activeFocus)
            verify(findChild(items, "saveAction_delete") !== null, "a saved episode offers Delete saved copy")
            keyClick(Qt.Key_Escape)
            tryVerify(() => part(2, "tileBody").activeFocus, 1000, "focus returns to the tile")
            compare(root.backs, 0)
        }

        function test_saveMenuOffersWhatFitsTheState() {
            const values = (i) => part(i, "tileSave").actions().map(a => a.value)
            // E1 plays its version on this computer (the NAS one is offline): Save as… only.
            compare(values(0), ["export"])
            compare(values(2), ["delete", "export"])
            compare(values(3), ["export"], "a file on this computer: Save as… only, never Delete")
            compare(values(4), ["pause", "cancel", "export"])
            compare(values(5), ["saveAgain", "delete", "export"])
            // An item only exported with Save as… has no saved copy: no Delete.
            const exported = part(2, "tileSave")
            const keep = exported.sv
            exported.sv = { state: "none", saveable: 1 }
            compare(exported.actions().map(a => a.value), ["save", "export"])
            exported.sv = keep
        }

        function test_marksSummaryAndProgress() {
            verify(part(2, "savedMark").visible)
            verify(part(5, "savedMark").visible, "outdated copies are marked too")
            verify(!part(0, "savedMark").visible)
            verify(part(4, "tileProgress").visible)
            compare(tile(4).savePercent, 30)
            verify(part(4, "tileSave").text.indexOf("30") >= 0)
            verify(tile(3).fileInfo === null)
            verify(part(3, "tileSummary").visible, "not analysed yet: Analysing…")
        }

        function test_detailsExpandInline() {
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Right)
            const h = tile(0).height
            keyClick(Qt.Key_Return)
            verify(tile(0).expanded)
            tile(0).fullFile = root.eps[0].files[1]
            tryVerify(() => tile(0).height > h + 100, 1000, "the panel opens under the tile")
            verify(part(0, "tileDetails").activeFocus, "focus stays on Details")
            keyClick(Qt.Key_Return)
            verify(!tile(0).expanded)
            tile(0).fullFile = null
        }
    }
}
