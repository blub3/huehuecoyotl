import QtQuick
import Coyotl

// Issue #120: a share's "Add folders": the folder browser over the core, back to Sources once
// the folders are added (their scans start at once).
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var share: 0
    property string shareName: ""

    onActiveFocusChanged: if (activeFocus) Qt.callLater(() => browser.focusFirst())

    ShareFolderBrowser {
        id: browser
        anchors.fill: parent
        anchors.margins: 32
        focus: true
        backend: Core
        target: ({ share: page.share })
        shareName: page.shareName
        onAdded: Nav.back()
        onCancelled: Nav.back()
    }
}
