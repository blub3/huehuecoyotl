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
    Component.onCompleted: storageRequest = Core.downloads()

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
        for (const c of [keyField, saveKey, cancelReplace, reviewButton, exportButton, noticesButton]) if (c.activeFocus) return
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
        anchors.fill: parent
        contentHeight: col.implicitHeight + 64
        clip: true
        ScrollBar.vertical: ScrollBar {}

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
                    Keys.onDownPressed: windowSizes.focusSelected()
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

            // ---- Appearance ----------------------------------------------------------------
            Label { text: qsTr("Appearance"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 24 }
            SizeChoice {
                id: windowSizes
                label: qsTr("Cover size in window")
                value: Appearance.coverWindow
                onPicked: (key) => Appearance.coverWindow = key
                onUpRequested: reviewButton.forceActiveFocus()
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
                options: [{ key: "quality", label: qsTr("Highest quality") }, { key: "smallest", label: qsTr("Smallest file") }]
                value: Appearance.preferredVersion
                onPicked: (key) => Appearance.preferredVersion = key
                onUpRequested: fullscreenSizes.focusSelected()
                onDownRequested: storageButton.forceActiveFocus()
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: qsTr("When a movie exists in several versions and none was played yet. “Smallest file” helps on slow networks.")
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
                    Keys.onUpPressed: versionChoice.focusSelected()
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
                    Keys.onUpPressed: versionChoice.focusSelected()
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
