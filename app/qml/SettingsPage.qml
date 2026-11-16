pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Dialogs
import Coyotl
import "MediaInfo.js" as MediaInfo

FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var keyRequest: 0
    property string keyMessage: ""
    property bool keyBusy: false
    // Issue #52: "Replace key" shows the field while the stored key stays in use.
    property bool replacing: false
    property bool keyTesting: false
    property bool keyRemoving: false
    property string keyNote: ""
    readonly property bool showKeyField: Core.tmdbKeyState === "none" || Core.tmdbKeyState === "unavailable" || replacing
    property string exportMessage: ""
    property bool exportFailed: false
    // Issue #4: Storage.
    property string storageDir: ""
    property real storageUsed: 0
    property var storageRequest: 0
    property string storageMessage: ""
    // Issue #12: TheAudioDB and Fanart.tv keys (their states only; never a key).
    property var musicKeys: ({ fanarttv: "none", theaudiodb: "none" })
    property var musicKeyRequest: 0
    property string musicKeyBusy: ""
    property string musicKeyFor: ""
    property string musicKeyMessage: ""
    function setMusicKey(provider, key) {
        musicKeyBusy = provider
        musicKeyFor = provider
        musicKeyMessage = ""
        musicKeyRequest = Core.setMusicKey(provider, key)
    }
    Connections {
        target: Core
        function onMusicKeysReady(requestId, keys) {
            page.musicKeys = keys
            if (requestId !== 0 && requestId !== page.musicKeyRequest) return
            if (requestId !== 0) {
                page.musicKeyMessage = keys.message || ""
                if (page.musicKeyMessage === "" || keys[page.musicKeyFor] === "session") {
                    if (page.musicKeyFor === "fanarttv") fanartKey.clear(); else audioDbKey.clear()
                }
                page.musicKeyBusy = ""
            }
        }
    }

    Component.onCompleted: { storageRequest = Core.downloads(); Core.musicKeys() }

    // One cover-size setting: a label and Small / Medium / Large, moved through with the arrows.
    component SizeChoice: RowLayout {
        id: choice
        property string label
        property string value
        property var options: Appearance.sizes
        signal picked(string key)
        signal upRequested()
        signal downRequested()
        function focusSelected() {
            const i = choice.options.findIndex(s => s.key === choice.value)
            choiceButtons.itemAt(Math.max(0, i)).forceActiveFocus()
        }
        spacing: 10
        Label { text: choice.label; color: Theme.text; Layout.preferredWidth: 240; wrapMode: Text.Wrap }
        Repeater {
            id: choiceButtons
            model: choice.options
            FocusButton {
                id: sizeButton
                required property var modelData
                required property int index
                text: modelData.label
                primary: choice.value === modelData.key
                Accessible.name: choice.label + ": " + modelData.label
                Accessible.checkable: true
                Accessible.checked: primary
                onClicked: choice.picked(modelData.key)
                Keys.onLeftPressed: {
                    if (sizeButton.index === 0) Nav.focusRail()
                    else choiceButtons.itemAt(sizeButton.index - 1).forceActiveFocus()
                }
                Keys.onRightPressed: {
                    if (sizeButton.index < choiceButtons.count - 1) choiceButtons.itemAt(sizeButton.index + 1).forceActiveFocus()
                }
                Keys.onUpPressed: choice.upRequested()
                Keys.onDownPressed: choice.downRequested()
            }
        }
    }

    // When the page receives focus, hand it to its first control.
    function focusFirst() {
        if (!page.activeFocus) return
        if (keyStatus.activeFocus) return
        for (const c of [keyField, saveKey, cancelReplace, reviewButton, musicOnline, exportButton, noticesButton]) if (c.activeFocus) return
        (keyStatus.firstButton || (page.showKeyField ? keyField : reviewButton)).forceActiveFocus()
    }
    onActiveFocusChanged: if (activeFocus) Qt.callLater(focusFirst)

    Connections {
        target: Core
        function onDownloadsListed(requestId, items, used, dir) {
            page.storageDir = dir
            page.storageUsed = used
            if (requestId === page.storageRequest) page.storageRequest = 0
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === page.storageRequest) page.storageMessage = message
        }
        function onKeyStatusChanged(requestId, message) {
            if (requestId !== 0 && requestId !== page.keyRequest) return
            page.keyBusy = false
            page.keyMessage = message
            // "Test key" succeeded: say so (failures show as the message).
            page.keyNote = requestId === page.keyRequest && page.keyTesting && message === "" ? qsTr("TMDB accepted the key.") : ""
            page.keyTesting = false
            // "Remove key" took away the button that had focus: focus what is there now.
            if (requestId === page.keyRequest && page.keyRemoving) {
                page.keyRemoving = false
                Qt.callLater(() => (keyStatus.firstButton || keyField).forceActiveFocus())
            }
            // A new key was accepted (a failure keeps the field and its text for another try).
            if (requestId === page.keyRequest && message === "" && Core.hasTmdbKey) {
                keyField.text = ""
                if (page.replacing) { page.replacing = false; if (keyStatus.firstButton) keyStatus.firstButton.forceActiveFocus() }
            }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.implicitHeight + 64
        clip: true
        ScrollBar.vertical: ScrollBar {}
        // Keyboard and remote: keep the focused control in view (the page is longer than a
        // screen since the music settings).
        function reveal(item) {
            if (!item || !col.contains(col.mapFromItem(item, 0, 0))) return
            const p = item.mapToItem(contentItem, 0, 0)
            if (p.y < contentY + 24) contentY = Math.max(0, p.y - 24)
            else if (p.y + item.height > contentY + height - 24)
                contentY = Math.min(p.y + item.height - height + 24, Math.max(0, contentHeight - height))
        }
        Connections {
            target: flick.Window.window
            function onActiveFocusItemChanged() { flick.reveal(flick.Window.activeFocusItem) }
        }

        ColumnLayout {
            id: col
            x: 32; y: 32
            width: Math.min(parent.width - 64, 860)
            spacing: 14

            Label { text: qsTr("Settings"); color: Theme.text; font.pixelSize: Theme.fontTitle; font.bold: true }

            // ---- Metadata --------------------------------------------------------------------
            Label { text: qsTr("Metadata"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 12 }
            // Issue #52: the key's state (stored, from the environment, no keystore) and its actions.
            TmdbKeyStatus {
                id: keyStatus
                Layout.fillWidth: true
                keyState: Core.tmdbKeyState
                validatedAt: Core.tmdbKeyValidatedAt
                busy: page.keyBusy
                downTarget: page.showKeyField ? keyField : reviewButton
                onLeftEdge: Nav.focusRail()
                onReplaceRequested: { page.replacing = true; page.keyMessage = ""; Qt.callLater(() => keyField.forceActiveFocus()) }
                onTestRequested: { page.keyBusy = true; page.keyTesting = true; page.keyMessage = ""; page.keyNote = ""; page.keyRequest = Core.testTmdbKey() }
                onRemoveRequested: { page.keyMessage = ""; page.keyNote = ""; page.keyRemoving = true; page.keyRequest = Core.clearTmdbKey() }
            }
            Label {
                Layout.fillWidth: true
                visible: page.showKeyField
                wrapMode: Text.Wrap
                color: Theme.text
                text: keyStatus.hasKey
                      ? qsTr("Enter the new key. The stored key stays until TMDB accepts the new one.")
                      : qsTr("Posters, descriptions and episode titles come from The Movie Database (TMDB). TMDB needs a free personal API key: create an account at themoviedb.org, open Settings → API, request a key for personal use, and paste the “API Key” or the “API Read Access Token” here. Without a key the app still plays everything and shows titles from the file names.")
            }
            Label {
                visible: page.showKeyField
                text: "https://www.themoviedb.org/settings/api"
                color: Theme.accent
                font.underline: true
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Qt.openUrlExternally(parent.text) }
            }
            RowLayout {
                visible: page.showKeyField
                spacing: 10
                TextField {
                    id: keyField
                    Layout.fillWidth: true
                    placeholderText: qsTr("TMDB API key or read access token")
                    echoMode: TextInput.Password
                    color: Theme.text
                    placeholderTextColor: Theme.textDim
                    Accessible.name: qsTr("TMDB API key")
                    KeyNavigation.right: saveKey
                    KeyNavigation.up: keyStatus.firstButton
                    KeyNavigation.down: reviewButton
                    Keys.onLeftPressed: (e) => { if (cursorPosition === 0) Nav.focusRail(); else e.accepted = false }
                    onAccepted: saveKey.clicked()
                    background: Rectangle {
                        implicitHeight: 44
                        radius: Theme.radius
                        color: Theme.surface
                        border.width: keyField.activeFocus ? Theme.focusWidth : 1
                        border.color: keyField.activeFocus ? Theme.focus : Theme.surfaceHi
                    }
                }
                FocusButton {
                    id: saveKey
                    text: page.keyBusy ? qsTr("Checking…") : qsTr("Check and save")
                    enabled: !page.keyBusy && keyField.text.length > 10
                    primary: true
                    KeyNavigation.left: keyField
                    KeyNavigation.right: cancelReplace.visible ? cancelReplace : null
                    KeyNavigation.up: keyStatus.firstButton
                    KeyNavigation.down: reviewButton
                    onClicked: { page.keyBusy = true; page.keyMessage = ""; page.keyRequest = Core.setTmdbKey(keyField.text) }
                }
                FocusButton {
                    id: cancelReplace
                    visible: page.replacing
                    text: qsTr("Cancel")
                    KeyNavigation.left: saveKey
                    KeyNavigation.down: reviewButton
                    onClicked: { page.replacing = false; keyField.text = ""; keyStatus.firstButton.forceActiveFocus() }
                }
            }
            Label { visible: page.keyNote !== ""; text: page.keyNote; color: Theme.accent; Layout.fillWidth: true; wrapMode: Text.Wrap }
            Label { visible: page.keyMessage !== ""; text: page.keyMessage; color: Theme.danger; Layout.fillWidth: true; wrapMode: Text.Wrap }

            RowLayout {
                spacing: 12
                Layout.topMargin: 6
                FocusButton {
                    id: reviewButton
                    text: qsTr("Review matches")
                    KeyNavigation.up: page.showKeyField ? keyField : keyStatus.firstButton
                    KeyNavigation.down: musicOnline
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Nav.open(Qt.resolvedUrl("ReviewPage.qml"))
                }
                Label {
                    text: Core.pendingJobs > 0 ? qsTr("%1 tasks queued").arg(Core.pendingJobs) : ""
                    color: Theme.textDim
                }
                Label {
                    visible: Core.failedJobs > 0
                    text: qsTr("%1 failed (see the log)").arg(Core.failedJobs)
                    color: Theme.danger
                }
            }

            // Issue #11: music information online, or from file tags only.
            RowLayout {
                spacing: 12
                Layout.topMargin: 6
                Layout.fillWidth: true
                FocusButton {
                    id: musicOnline
                    objectName: "musicOnline"
                    glyph: Core.musicOnline ? "✓" : ""
                    text: Core.musicOnline ? qsTr("Music and book information online: on") : qsTr("Music and book information online: off")
                    Accessible.description: musicNote.text
                    KeyNavigation.up: reviewButton
                    Keys.onDownPressed: audioDbKey.firstItem.forceActiveFocus()
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Core.musicOnline = !Core.musicOnline
                }
                Label {
                    id: musicNote
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    text: Core.musicOnline
                          ? qsTr("Album and artist names from your music are sent to MusicBrainz to find release types, dates and ids, and covers come from the Cover Art Archive. Titles, authors and ISBNs of your audiobooks are sent to Open Library for descriptions, years and covers. Your own metadata is never replaced. No account or key is needed.")
                          : qsTr("Music and book information comes from your files, their tags and metadata files only; nothing is sent.")
                }
            }

            // Issue #12: optional providers, each off until the user enters their own key.
            Label {
                Layout.fillWidth: true
                Layout.topMargin: 6
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: qsTr("Optional: with your own key, artist images, backgrounds, logos and biographies, and covers for albums without one, come from TheAudioDB and Fanart.tv. Artist and album ids from MusicBrainz are sent to them. Keys stay in the system keystore.")
            }
            MusicKeyRow {
                id: audioDbKey
                Layout.fillWidth: true
                name: "TheAudioDB"
                keyState: page.musicKeys.theaudiodb || "none"
                busy: page.musicKeyBusy === "theaudiodb"
                message: page.musicKeyFor === "theaudiodb" ? page.musicKeyMessage : ""
                hint: qsTr("TheAudioDB gives an API key to supporters on their profile page.")
                link: "https://www.theaudiodb.com"
                onSaveRequested: (key) => page.setMusicKey("theaudiodb", key)
                onRemoveRequested: page.setMusicKey("theaudiodb", "")
                onUpRequested: musicOnline.forceActiveFocus()
                onDownRequested: fanartKey.firstItem.forceActiveFocus()
            }
            MusicKeyRow {
                id: fanartKey
                Layout.fillWidth: true
                name: "Fanart.tv"
                keyState: page.musicKeys.fanarttv || "none"
                busy: page.musicKeyBusy === "fanarttv"
                message: page.musicKeyFor === "fanarttv" ? page.musicKeyMessage : ""
                hint: qsTr("Create an account at fanart.tv and request an API key on your profile.")
                link: "https://fanart.tv"
                onSaveRequested: (key) => page.setMusicKey("fanarttv", key)
                onRemoveRequested: page.setMusicKey("fanarttv", "")
                onUpRequested: audioDbKey.firstItem.forceActiveFocus()
                onDownRequested: windowSizes.focusSelected()
            }

            // ---- Appearance ----------------------------------------------------------------
            Label { text: qsTr("Appearance"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 24 }
            SizeChoice {
                id: windowSizes
                label: qsTr("Cover size in window")
                value: Appearance.coverWindow
                onPicked: (key) => Appearance.coverWindow = key
                onUpRequested: fanartKey.firstItem.forceActiveFocus()
                onDownRequested: fullscreenSizes.focusSelected()
            }
            SizeChoice {
                id: fullscreenSizes
                label: qsTr("Cover size in fullscreen")
                value: Appearance.coverFullscreen
                onPicked: (key) => Appearance.coverFullscreen = key
                onUpRequested: windowSizes.focusSelected()
                onDownRequested: versionChoice.focusSelected()
            }

            // ---- Playback ------------------------------------------------------------------
            Label { text: qsTr("Playback"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 24 }
            SizeChoice {
                id: versionChoice
                label: qsTr("Preferred version")
                options: [{ key: "quality", label: qsTr("Highest quality") }, { key: "smallest", label: qsTr("Smallest file") },
                          { key: "lossless", label: qsTr("Lossless only") }]
                value: Appearance.preferredVersion
                onPicked: (key) => Appearance.preferredVersion = key
                onUpRequested: fullscreenSizes.focusSelected()
                onDownRequested: replayGainChoice.focusSelected()
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: qsTr("When a movie or an album exists in several versions and none was played yet. “Smallest file” helps on slow networks. For music, “Highest quality” prefers lossless, then bit depth, then sample rate; “Lossless only” never picks a lossy copy when a lossless one can play (for films it means the highest quality).")
            }
            // Issue #8: music volume levelling from the ReplayGain tags.
            SizeChoice {
                id: replayGainChoice
                label: qsTr("ReplayGain")
                options: [{ key: "off", label: qsTr("Off") }, { key: "track", label: qsTr("Track") }, { key: "album", label: qsTr("Album") }]
                value: Music.replayGain
                onPicked: (key) => Music.replayGain = key
                onUpRequested: versionChoice.focusSelected()
                onDownRequested: bitPerfectButton.forceActiveFocus()
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: qsTr("Evens out the loudness of music from the ReplayGain tags in the files: per track, or per album so an album keeps its own dynamics.")
            }

            // ---- Audio (issue #9) --------------------------------------------------------------
            Label { text: qsTr("Audio"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 24 }
            RowLayout {
                spacing: 12
                Layout.fillWidth: true
                FocusButton {
                    id: bitPerfectButton
                    glyph: Music.bitPerfect ? "✓" : ""
                    text: Music.bitPerfect ? qsTr("Bit-perfect music: on") : qsTr("Bit-perfect music: off")
                    Accessible.description: bitPerfectNote.text
                    Keys.onUpPressed: replayGainChoice.focusSelected()
                    KeyNavigation.down: deviceButton
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Music.bitPerfect = !Music.bitPerfect
                }
                Label {
                    id: bitPerfectNote
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    text: qsTr("Music goes to the device as it is in the file: no resampling by the app, no ReplayGain, volume fixed at 100 %, and exclusive access where the device allows it (an ALSA hardware device). Through PipeWire the system mixer has the last word; Now Playing shows what happens.")
                }
            }
            RowLayout {
                spacing: 12
                Layout.fillWidth: true
                FocusButton {
                    id: deviceButton
                    readonly property var current: (Music.devices || []).find(d => d.name === Music.audioDevice)
                    text: qsTr("Music output: %1").arg(Music.audioDevice === "auto" ? qsTr("system default")
                                                       : current ? current.description : Music.audioDevice) + "  ▾"
                    KeyNavigation.up: bitPerfectButton
                    KeyNavigation.down: storageButton
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: deviceMenu.show(deviceButton, (Music.devices || []).map(d => ({
                        value: d.name,
                        label: d.name === "auto" ? qsTr("System default")
                             : (d.description || d.name) + (d.direct ? "  ·  " + qsTr("direct") : "")
                    })))
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    text: qsTr("Devices marked “direct” are the hardware itself; in bit-perfect mode the app uses them exclusively. Videos always use the system default.")
                }
            }
            ActionMenu {
                id: deviceMenu
                menuWidth: Math.min(640, page.width - 120)
                onChosen: (v) => Music.audioDevice = v
            }

            // ---- Storage (issue #4) -----------------------------------------------------------
            Label { text: qsTr("Storage"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 24 }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.WrapAnywhere
                color: Theme.text
                text: qsTr("Saved copies go to %1").arg(page.storageDir)
            }
            Label {
                Layout.fillWidth: true
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: qsTr("Space used by saved copies: %1").arg(MediaInfo.size(page.storageUsed))
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: storageButton
                    text: qsTr("Change location…")
                    Accessible.description: qsTr("New saved copies go to the chosen folder; existing ones stay where they are.")
                    Keys.onUpPressed: deviceButton.forceActiveFocus()
                    KeyNavigation.right: storageDefault
                    KeyNavigation.down: deleteAllButton
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: storageDialog.open()
                }
                FocusButton {
                    id: storageDefault
                    text: qsTr("Use default")
                    KeyNavigation.left: storageButton
                    KeyNavigation.right: deleteAllButton
                    KeyNavigation.down: deleteAllButton
                    Keys.onUpPressed: deviceButton.forceActiveFocus()
                    onClicked: { page.storageMessage = ""; page.storageRequest = Core.setStorageDir("") }
                }
                FocusButton {
                    id: deleteAllButton
                    enabled: page.storageUsed > 0
                    text: qsTr("Delete all saved copies")
                    KeyNavigation.left: storageDefault
                    Keys.onUpPressed: storageButton.forceActiveFocus()
                    KeyNavigation.down: exportButton
                    onClicked: deleteAllConfirm.open()
                }
            }
            Label {
                Layout.fillWidth: true
                visible: page.storageMessage !== ""
                text: page.storageMessage
                color: Theme.danger
                wrapMode: Text.Wrap
            }

            // ---- Troubleshooting -----------------------------------------------------------
            Label { text: qsTr("Troubleshooting"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 24 }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("If something does not work, export a diagnostics file and attach it to a bug report. It contains the app's log and version information. Server names, user names, addresses, keys and your home folder are removed.")
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: exportButton
                    text: qsTr("Export diagnostics")
                    Keys.onUpPressed: storageButton.forceActiveFocus()
                    KeyNavigation.down: noticesButton
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: {
                        const path = Core.exportDiagnostics()
                        page.exportFailed = path === ""
                        page.exportMessage = path !== "" ? qsTr("Saved to %1").arg(path)
                                                         : qsTr("Export failed: %1").arg(Core.lastExportError())
                    }
                }
                Label {
                    Layout.fillWidth: true
                    visible: page.exportMessage !== ""
                    text: page.exportMessage
                    wrapMode: Text.WrapAnywhere
                    color: page.exportFailed ? Theme.danger : Theme.textDim
                }
            }

            // ---- About ---------------------------------------------------------------------
            Label { text: qsTr("About"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 24 }
            RowLayout {
                spacing: 18
                AppLogo { size: 96; Layout.preferredWidth: 96; Layout.preferredHeight: 96 }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.text
                    // "1.1.0", or "1.1.0-dev+7870d67" for a development build.
                    readonly property var version: Qt.application.version.split("-dev+")
                    text: qsTr("Huehuecoyotl %1 · Apache License 2.0. Playback by libmpv and FFmpeg, interface by Qt, all used under the LGPL.")
                          .arg(version.length > 1 ? qsTr("%1 (development build %2)").arg(version[0]).arg(version[1]) : version[0])
                }
            }
            FocusButton {
                id: noticesButton
                text: qsTr("Third-party licences")
                KeyNavigation.up: exportButton
                Keys.onLeftPressed: Nav.focusRail()
                onClicked: Nav.open(Qt.resolvedUrl("NoticesPage.qml"))
            }
            RowLayout {
                spacing: 14
                Rectangle {   // TMDB attribution (SPEC 7); the approved logo asset replaces this mark.
                    Layout.preferredWidth: 74; Layout.preferredHeight: 28; radius: 5
                    color: "#0d253f"
                    Label { anchors.centerIn: parent; text: "TMDB"; color: "#01b4e4"; font.bold: true; font.pixelSize: 13 }
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textDim
                    text: qsTr("This product uses the TMDB API but is not endorsed or certified by TMDB.")
                }
            }
            // Issue #11: music information and covers (MusicBrainz core data is CC0).
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: qsTr("Music information from MusicBrainz (musicbrainz.org, data under CC0). Album covers from the Cover Art Archive (coverartarchive.org), a joint project of the Internet Archive and MusicBrainz.")
            }
            // Issue #67: book information and covers.
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: qsTr("Book information and covers from Open Library (openlibrary.org), a project of the Internet Archive.")
            }
            // Issue #12: only when used (a key is entered).
            Label {
                Layout.fillWidth: true
                visible: page.musicKeys.theaudiodb === "keystore" || page.musicKeys.theaudiodb === "session"
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: qsTr("Artist images, biographies and album art from TheAudioDB (theaudiodb.com), contributed by its community.")
            }
            Label {
                Layout.fillWidth: true
                visible: page.musicKeys.fanarttv === "keystore" || page.musicKeys.fanarttv === "session"
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: qsTr("Artist images, logos and album art from Fanart.tv (fanart.tv), contributed by its community.")
            }
        }
    }

    FolderDialog {
        id: storageDialog
        title: qsTr("Where saved copies go")
        currentFolder: page.storageDir !== "" ? "file://" + page.storageDir : ""
        onAccepted: {
            storageButton.forceActiveFocus()
            page.storageMessage = ""
            const url = String(selectedFolder)
            page.storageRequest = Core.setStorageDir(decodeURIComponent(url.replace(/^file:\/\//, "")))
        }
        onRejected: storageButton.forceActiveFocus()
    }

    Popup {
        id: deleteAllConfirm
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 80)
        modal: true
        padding: 24
        background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
        onOpened: deleteAllKeep.forceActiveFocus()
        onClosed: storageButton.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                text: qsTr("Delete every saved copy from this computer (%1)? The files on your servers stay, and everything can be streamed or saved again.").arg(MediaInfo.size(page.storageUsed))
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: deleteAllKeep
                    text: qsTr("Keep")
                    KeyNavigation.right: deleteAllOk
                    onClicked: deleteAllConfirm.close()
                }
                FocusButton {
                    id: deleteAllOk
                    primary: true
                    text: qsTr("Delete all saved copies")
                    KeyNavigation.left: deleteAllKeep
                    onClicked: { page.storageRequest = Core.deleteAllSaved(); deleteAllConfirm.close() }
                }
            }
        }
    }
}
