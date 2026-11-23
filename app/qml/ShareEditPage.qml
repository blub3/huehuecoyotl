import QtQuick
import Coyotl

// Issue #122: "Edit share" from a share's card: ShareEditor over the core. Add folders opens
// the share's folder browser; removing the share goes back to Sources.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: qsTr("Edit share")
    property var share: 0

    onActiveFocusChanged: if (activeFocus) editor.forceActiveFocus()

    ShareEditor {
        id: editor
        anchors.fill: parent
        anchors.margins: 32
        focus: true
        backend: Core
        shareId: page.share
        onAddFoldersRequested: (share, name) => Nav.open(Qt.resolvedUrl("ShareFoldersPage.qml"), { share: share, shareName: name })
        onClosed: Nav.back()
    }
    // Back from Add folders: the folders may have changed.
    onVisibleChanged: if (visible) editor.load()
}
