import QtQuick
import Coyotl

// "Add network share" (SPEC 6; issue #121): the three steps of AddShareFlow over the core. A
// share already in the library opens its "Add folders" in place of this page.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    // Issue #134: what the Back button above the next page calls this one.
    readonly property string pageTitle: qsTr("Add network share")

    onActiveFocusChanged: if (activeFocus) flow.forceActiveFocus()

    AddShareFlow {
        id: flow
        anchors.fill: parent
        anchors.margins: 32
        focus: true
        backend: Core
        onOpenExisting: (share, name) => Nav.replace(Qt.resolvedUrl("ShareFoldersPage.qml"), { share: share, shareName: name })
        onFinished: Nav.back()
        onClosed: Nav.back()
    }
}
