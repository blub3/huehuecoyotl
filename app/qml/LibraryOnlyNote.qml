pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #150: on a page whose original was deleted or whose source was removed: the item plays
// from its library copy. `backend`: libraryCall("itemOrigin", {item}).
ColumnLayout {
    id: note
    property var backend: null
    property var itemId: 0
    property var origin: ({})
    property var pending: 0
    visible: origin.libraryOnly === true
    spacing: 4
    function load() { if (backend && itemId) pending = backend.libraryCall("itemOrigin", { item: Number(itemId) }) }
    onItemIdChanged: load()
    Component.onCompleted: load()
    Connections {
        target: note.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === note.pending && kind === "libraryOrigin") note.origin = detail
        }
    }
    Rectangle {
        implicitWidth: badge.implicitWidth + 20
        implicitHeight: badge.implicitHeight + 8
        radius: height / 2
        color: Theme.surfaceHi
        Label { id: badge; anchors.centerIn: parent; text: qsTr("In your library only"); color: Theme.accent; font.pixelSize: Theme.fontSmall; font.bold: true }
    }
    Label {
        objectName: "originNote"
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: Theme.textDim
        font.pixelSize: Theme.fontSmall
        text: qsTr("The original left “%1” on %2. It plays from your library copy.")
              .arg(note.origin.from || "")
              .arg(new Date(Number(note.origin.goneAt || 0) * 1000).toLocaleDateString(Qt.locale(), Locale.ShortFormat))
    }
}
