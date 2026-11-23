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
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: qsTr("Settings")
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

    Component.onCompleted: { storageRequest = Core.downloads(); Core.musicKeys(); matchingRequest = Core.matchingStatus() }

    // Issues #93 to #96: Settings → Matching. The status comes from Core.matchingStatus(); it is
    // asked again every few seconds while the page is shown, every second while a library runs.
    property var matching: ({ libraries: [] })
    property var matchingRequest: 0
    property string matchingMessage: ""
    readonly property bool matchingBusy: (matching.libraries || []).some(l => l.run && (l.run.state === "running" || l.run.state === "waiting"))
    readonly property int reviewCount: (matching.libraries || []).filter(l => l.library === "movies" || l.library === "shows")
                                                                  .reduce((n, l) => n + (l.review || 0), 0)
    readonly property string runningName: {
        const r = (matching.libraries || []).find(l => l.run && l.run.state === "running")
        if (!r) return ""
        return { movies: qsTr("movies"), shows: qsTr("TV shows"), music: qsTr("music"), books: qsTr("audiobooks") }[r.library] || ""
    }
    function focusMatchRow(i) {
        const r = matchRows.itemAt(Math.max(0, Math.min(i, matchRows.count - 1))) as MatchingLibraryRow
        if (r) r.firstButton.forceActiveFocus(); else reviewButton.forceActiveFocus()
    }
    Timer {
        interval: page.matchingBusy ? 1000 : 5000
        running: page.visible && page.tab === "matching"
        repeat: true
        triggeredOnStart: true
        onTriggered: Core.matchingStatus()
    }
    RematchDialog {
        id: rematchDialog
        onStartRequested: (library, all, includeLocked) => {
            page.matchingMessage = ""
            page.matchingRequest = Core.rematchLibrary(library, all, includeLocked)
        }
    }
    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (kind === "matching") page.matching = detail
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === page.matchingRequest) page.matchingMessage = message
        }
    }

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

    // Issue #117: the sections as tabs. The tab used last is kept (Appearance.settingsTab, saved in
    // ui.ini); Nav.settings(tab) opens Settings on a given tab.
    readonly property var tabs: [
        { key: "metadata", label: qsTr("Metadata") }, { key: "matching", label: qsTr("Matching") },
        { key: "library", label: qsTr("Library") },
        { key: "appearance", label: qsTr("Appearance") }, { key: "playback", label: qsTr("Playback") },
        { key: "audio", label: qsTr("Audio") }, { key: "storage", label: qsTr("Storage") },
        { key: "troubleshooting", label: qsTr("Troubleshooting") }, { key: "about", label: qsTr("About") }
    ]
    property string tab: tabs.some(t => t.key === Appearance.settingsTab) ? Appearance.settingsTab : "metadata"
    onTabChanged: { Appearance.settingsTab = tab; flick.contentY = 0 }
    function focusTabs() { tabBar.focusCurrent() }
    // The first control of the current section.
    function focusSection() {
        switch (tab) {
        case "metadata": (keyStatus.firstButton || (page.showKeyField ? keyField : musicOnline)).forceActiveFocus(); break
        case "matching": reviewButton.forceActiveFocus(); break
        case "library": librarySettings.focusFirst(); break
        case "appearance": windowSizes.focusSelected(); break
        case "playback": versionChoice.focusSelected(); break
        case "audio": bitPerfectButton.forceActiveFocus(); break
        case "storage": storageButton.forceActiveFocus(); break
        case "troubleshooting": exportButton.forceActiveFocus(); break
        case "about": noticesButton.forceActiveFocus(); break
        }
    }

    // When the page receives focus, hand it to the current tab.
    function focusFirst() {
        if (!page.activeFocus) return
        if (page.Window.activeFocusItem && page.Window.activeFocusItem !== page) return
        focusTabs()
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

            // Issue #117: one tab per section; the tab changes as focus moves along the bar.
            SettingsTabs {
                id: tabBar
                objectName: "settingsTabs"
                Layout.fillWidth: true
                Layout.bottomMargin: 8
                tabs: page.tabs
                current: page.tab
                onPicked: (key) => page.tab = key
                onDownRequested: page.focusSection()
                onLeftEdge: Nav.focusRail()
            }

            // ---- Metadata --------------------------------------------------------------------
            ColumnLayout {
                id: metadataSection
                visible: page.tab === "metadata"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
                // Issue #52: the key's state (stored, from the environment, no keystore) and its actions.
                TmdbKeyStatus {
                    id: keyStatus
                    Layout.fillWidth: true
                    keyState: Core.tmdbKeyState
                    validatedAt: Core.tmdbKeyValidatedAt
                    busy: page.keyBusy
                    downTarget: page.showKeyField ? keyField : musicOnline
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
                        KeyNavigation.down: musicOnline
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
                        KeyNavigation.down: musicOnline
                        onClicked: { page.keyBusy = true; page.keyMessage = ""; page.keyRequest = Core.setTmdbKey(keyField.text) }
                    }
                    FocusButton {
                        id: cancelReplace
                        visible: page.replacing
                        text: qsTr("Cancel")
                        KeyNavigation.left: saveKey
                        KeyNavigation.down: musicOnline
                        onClicked: { page.replacing = false; keyField.text = ""; keyStatus.firstButton.forceActiveFocus() }
                    }
                }
                Label { visible: page.keyNote !== ""; text: page.keyNote; color: Theme.accent; Layout.fillWidth: true; wrapMode: Text.Wrap }
                Label { visible: page.keyMessage !== ""; text: page.keyMessage; color: Theme.danger; Layout.fillWidth: true; wrapMode: Text.Wrap }

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
                        KeyNavigation.up: page.showKeyField ? keyField : keyStatus.firstButton
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
                }
            }

            // ---- Library (issue #150) ---------------------------------------------------------
            LibrarySettings {
                id: librarySettings
                objectName: "librarySection"
                visible: page.tab === "library"
                Layout.fillWidth: true
                backend: Core
                onLeaveUp: page.focusTabs()
                onLeaveLeft: Nav.focusRail()
                onPickLocal: libraryDialog.open()
                onAddShare: Nav.open(Qt.resolvedUrl("NetworkSharePage.qml"), {})
                onVisibleChanged: if (visible) load()
            }

            // ---- Matching (issues #93 to #96) ------------------------------------------------
            ColumnLayout {
                id: matchingSection
                visible: page.tab === "matching"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.text
                    text: qsTr("Look up a whole library again, for example after a provider fixed its data or after you renamed many files. Your own metadata from tags, NFO and OPF files is never replaced. Items with several possible matches wait under Review matches.")
                }
                RowLayout {
                    spacing: 12
                    FocusButton {
                        id: reviewButton
                        objectName: "reviewButton"
                        text: page.reviewCount > 0 ? qsTr("Review matches (%1)").arg(page.reviewCount) : qsTr("Review matches")
                        Keys.onUpPressed: page.focusTabs()
                        Keys.onDownPressed: page.focusMatchRow(0)
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
                Repeater {
                    id: matchRows
                    model: page.matching.libraries || []
                    MatchingLibraryRow {
                        id: libRow
                        required property var modelData
                        required property int index
                        Layout.fillWidth: true
                        info: modelData
                        runningName: page.runningName
                        onStartRequested: rematchDialog.show(libRow.modelData, libRow.firstButton)
                        onStopRequested: page.matchingRequest = Core.rematchStop(libRow.modelData.library)
                        onUpRequested: libRow.index === 0 ? reviewButton.forceActiveFocus() : page.focusMatchRow(libRow.index - 1)
                        onDownRequested: if (libRow.index < matchRows.count - 1) page.focusMatchRow(libRow.index + 1)
                        onLeftEdge: Nav.focusRail()
                    }
                }
                Label { visible: page.matchingMessage !== ""; text: page.matchingMessage; color: Theme.danger; Layout.fillWidth: true; wrapMode: Text.Wrap }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    text: qsTr("One library at a time; the others wait in line. Playing and browsing go on while it runs, and it continues after a restart.")
                }
            }

            // ---- Appearance ----------------------------------------------------------------
            ColumnLayout {
                id: appearanceSection
                visible: page.tab === "appearance"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
                SizeChoice {
                    id: windowSizes
                    label: qsTr("Cover size in window")
                    value: Appearance.coverWindow
                    onPicked: (key) => Appearance.coverWindow = key
                    onUpRequested: page.focusTabs()
                    onDownRequested: fullscreenSizes.focusSelected()
                }
                SizeChoice {
                    id: fullscreenSizes
                    label: qsTr("Cover size in fullscreen")
                    value: Appearance.coverFullscreen
                    onPicked: (key) => Appearance.coverFullscreen = key
                    onUpRequested: windowSizes.focusSelected()
                }
            }

            // ---- Playback ------------------------------------------------------------------
            ColumnLayout {
                id: playbackSection
                visible: page.tab === "playback"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
                SizeChoice {
                    id: versionChoice
                    label: qsTr("Preferred version")
                    options: [{ key: "quality", label: qsTr("Highest quality") }, { key: "smallest", label: qsTr("Smallest file") },
                              { key: "lossless", label: qsTr("Lossless only") }]
                    value: Appearance.preferredVersion
                    onPicked: (key) => Appearance.preferredVersion = key
                    onUpRequested: page.focusTabs()
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
                    onDownRequested: bookSpeedChoice.focusSelected()
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    text: qsTr("Evens out the loudness of music from the ReplayGain tags in the files: per track, or per album so an album keeps its own dynamics.")
                }
                // Issue #66: audiobooks: the speed of books without one of their own, and skipping.
                SizeChoice {
                    id: bookSpeedChoice
                    label: qsTr("Audiobook speed")
                    options: [0.8, 1.0, 1.2, 1.5, 1.8, 2.0].map(v => ({ key: String(v), label: Number(v).toLocaleString(Qt.locale(), "f", 1) + "×" }))
                    value: String(Math.round(Book.defaultSpeed * 10) / 10)
                    onPicked: (key) => { Book.defaultSpeed = Number(key) }
                    onUpRequested: replayGainChoice.focusSelected()
                    onDownRequested: skipBackChoice.focusSelected()
                }
                SizeChoice {
                    id: skipBackChoice
                    label: qsTr("Skip back")
                    options: [5, 10, 15, 30, 60].map(v => ({ key: String(v), label: qsTr("%1 s").arg(v) }))
                    value: String(Book.skipBack)
                    onPicked: (key) => { Book.skipBack = Number(key) }
                    onUpRequested: bookSpeedChoice.focusSelected()
                    onDownRequested: skipForwardChoice.focusSelected()
                }
                SizeChoice {
                    id: skipForwardChoice
                    label: qsTr("Skip forward")
                    options: [10, 15, 30, 60, 120].map(v => ({ key: String(v), label: qsTr("%1 s").arg(v) }))
                    value: String(Book.skipForward)
                    onPicked: (key) => { Book.skipForward = Number(key) }
                    onUpRequested: skipBackChoice.focusSelected()
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                    text: qsTr("Books you set a speed for keep their own. The pitch stays the same at every speed.")
                }
            }

            // ---- Audio (issue #9) --------------------------------------------------------------
            ColumnLayout {
                id: audioSection
                visible: page.tab === "audio"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
                RowLayout {
                    spacing: 12
                    Layout.fillWidth: true
                    FocusButton {
                        id: bitPerfectButton
                        glyph: Music.bitPerfect ? "✓" : ""
                        text: Music.bitPerfect ? qsTr("Bit-perfect music: on") : qsTr("Bit-perfect music: off")
                        Accessible.description: bitPerfectNote.text
                        Keys.onUpPressed: page.focusTabs()
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
            }

            // ---- Storage (issue #4) -----------------------------------------------------------
            ColumnLayout {
                id: storageSection
                visible: page.tab === "storage"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
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
                        Keys.onUpPressed: page.focusTabs()
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
                        Keys.onUpPressed: page.focusTabs()
                        onClicked: { page.storageMessage = ""; page.storageRequest = Core.setStorageDir("") }
                    }
                    FocusButton {
                        id: deleteAllButton
                        enabled: page.storageUsed > 0
                        text: qsTr("Delete all saved copies")
                        KeyNavigation.left: storageDefault
                        Keys.onUpPressed: storageButton.forceActiveFocus()
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
            }

            // ---- Troubleshooting -----------------------------------------------------------
            ColumnLayout {
                id: troubleshootingSection
                visible: page.tab === "troubleshooting"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
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
                        Keys.onUpPressed: page.focusTabs()
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
            }

            // ---- About ---------------------------------------------------------------------
            ColumnLayout {
                id: aboutSection
                visible: page.tab === "about"
                Layout.fillWidth: true
                spacing: 14
                // Up from a control that has no other place to go: back to the tab bar.
                Keys.onUpPressed: page.focusTabs()
                RowLayout {
                    spacing: 18
                    AppLogo { size: 96; Layout.preferredWidth: 96; Layout.preferredHeight: 96 }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        color: Theme.text
                        // "1.1.0", or "1.1.0-dev+7870d67" for a development build.
                        readonly property var version: Qt.application.version.split("-dev+")
                        text: qsTr("Huehuecoyotl %1 · Apache License 2.0. Playback by libmpv and FFmpeg, interface by Qt. Their licences, and those of any codec extension, are under Third-party licences.")
                              .arg(version.length > 1 ? qsTr("%1 (development build %2)").arg(version[0]).arg(version[1]) : version[0])
                    }
                }
                FocusButton {
                    id: noticesButton
                    text: qsTr("Third-party licences")
                    Keys.onUpPressed: page.focusTabs()
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: Nav.open(Qt.resolvedUrl("NoticesPage.qml"))
                }
                RowLayout {
                    spacing: 14
                    Image {   // Issue #162: TMDB's approved logo (assets/tmdb), smaller than the app's.
                        objectName: "tmdbLogo"
                        Layout.preferredWidth: 123; Layout.preferredHeight: 16
                        source: "qrc:/tmdb/tmdb-blue-short.png"
                        sourceSize.width: 246
                        fillMode: Image.PreserveAspectFit
                        smooth: true
                        mipmap: true
                        Accessible.role: Accessible.Graphic
                        Accessible.name: qsTr("TMDB logo")
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
    }

    // Issue #150: under Flatpak the portal grants write access to this one folder.
    FolderDialog {
        id: libraryDialog
        title: qsTr("Your library folder")
        onAccepted: librarySettings.checkLocal(decodeURIComponent(String(selectedFolder).replace(/^file:\/\//, "")))
        onRejected: librarySettings.focusFirst()
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
