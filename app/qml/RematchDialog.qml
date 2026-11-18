pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// "Match again…" for a whole library (issues #93 to #96): only the unmatched and uncertain items
// (the default) or everything, whether the matches the user chose stay, how many items that is
// and about how long it takes. Start emits `startRequested`; focus goes back to `from` on close.
Popup {
    id: dialog
    property var info: ({})
    property Item from: null
    property bool all: false
    property bool keepLocked: true
    signal startRequested(string library, bool all, bool includeLocked)

    function show(libraryInfo, item) {
        info = libraryInfo
        from = item
        all = false
        keepLocked = true
        open()
    }
    function name() {
        switch (info.library) {
        case "movies": return qsTr("movies")
        case "shows": return qsTr("TV shows")
        case "music": return qsTr("the music library")
        case "books": return qsTr("audiobooks")
        }
        return ""
    }
    readonly property int openItems: (info.review || 0) + (info.none || 0)
    readonly property int count: all ? (info.total || 0) - (keepLocked ? (info.locked || 0) : 0)
                                     : openItems
    // As MatchingLibraryRow estimates it.
    readonly property real secondsPerItem: info.library === "music" ? 2 : info.library === "books" ? 1.5 : 0.25
    readonly property int estimate: Math.max(1, Math.round(count * secondsPerItem / 60))

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(620, (parent ? parent.width : 800) - 80)
    modal: true
    focus: true
    padding: 24
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onOpened: openOnly.forceActiveFocus()
    onClosed: if (from) from.forceActiveFocus()

    contentItem: ColumnLayout {
        spacing: 14
        Keys.onPressed: (e) => { if (e.key === Qt.Key_Back) { dialog.close(); e.accepted = true } }
        Label {
            objectName: "dialogTitle"
            text: qsTr("Match %1 again").arg(dialog.name())
            color: Theme.text
            font.pixelSize: Theme.fontLarge
            font.bold: true
            Accessible.role: Accessible.Heading
        }
        Label { text: qsTr("Which items"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
        RadioButton {
            id: openOnly
            objectName: "openOnly"
            checked: !dialog.all
            text: qsTr("Unmatched and uncertain only")
            Accessible.description: openHint.text
            KeyNavigation.down: everything
            onToggled: if (checked) dialog.all = false
            Keys.onReturnPressed: start.clicked()
            Keys.onEnterPressed: start.clicked()
            background: Rectangle { radius: 8; color: "transparent"; border.width: openOnly.activeFocus ? Theme.focusWidth : 0; border.color: Theme.focus }
            padding: 6
            contentItem: Label { leftPadding: openOnly.indicator.width + 8; text: openOnly.text; color: Theme.text; font.bold: openOnly.checked }
        }
        Label {
            id: openHint
            Layout.fillWidth: true
            Layout.leftMargin: 32
            wrapMode: Text.Wrap
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
            text: qsTr("The %n item(s) without a sure match. Matched items stay as they are.", "", dialog.openItems)
        }
        RadioButton {
            id: everything
            objectName: "everything"
            checked: dialog.all
            text: qsTr("Everything")
            Accessible.description: allHint.text
            KeyNavigation.up: openOnly
            KeyNavigation.down: keep
            onToggled: if (checked) dialog.all = true
            Keys.onReturnPressed: start.clicked()
            Keys.onEnterPressed: start.clicked()
            background: Rectangle { radius: 8; color: "transparent"; border.width: everything.activeFocus ? Theme.focusWidth : 0; border.color: Theme.focus }
            padding: 6
            contentItem: Label { leftPadding: everything.indicator.width + 8; text: everything.text; color: Theme.text; font.bold: everything.checked }
        }
        Label {
            id: allHint
            Layout.fillWidth: true
            Layout.leftMargin: 32
            wrapMode: Text.Wrap
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
            text: qsTr("All %n item(s), as if they were new. A match is only replaced by a sure one.", "", dialog.info.total || 0)
        }
        CheckBox {
            id: keep
            objectName: "keepLocked"
            checked: dialog.keepLocked
            text: qsTr("Keep the matches you chose yourself (%1)").arg(dialog.info.locked || 0)
            Accessible.description: qsTr("Off: they are looked up again too and may change.")
            KeyNavigation.up: everything
            KeyNavigation.down: start
            onToggled: dialog.keepLocked = checked
            Keys.onReturnPressed: start.clicked()
            Keys.onEnterPressed: start.clicked()
            background: Rectangle { radius: 8; color: "transparent"; border.width: keep.activeFocus ? Theme.focusWidth : 0; border.color: Theme.focus }
            padding: 6
            contentItem: Label { leftPadding: keep.indicator.width + 8; text: keep.text; color: Theme.text; wrapMode: Text.Wrap }
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: estimateLine.implicitHeight + 20
            radius: 8
            color: Theme.bg
            Label {
                id: estimateLine
                objectName: "estimateLine"
                anchors.fill: parent
                anchors.margins: 10
                wrapMode: Text.Wrap
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                text: dialog.count === 0
                      ? qsTr("Nothing to do: every item has a sure match.")
                      : (dialog.info.library === "music" || dialog.info.library === "books"
                         ? qsTr("%n item(s). One request a second is allowed: about %1 min.", "", dialog.count)
                         : qsTr("%n item(s): about %1 min.", "", dialog.count)).arg(dialog.estimate)
            }
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 10
            FocusButton {
                id: cancel
                objectName: "cancelButton"
                text: qsTr("Cancel")
                KeyNavigation.up: keep
                KeyNavigation.right: start
                onClicked: dialog.close()
            }
            FocusButton {
                id: start
                objectName: "startButton"
                primary: true
                enabled: dialog.count > 0
                text: qsTr("Start")
                KeyNavigation.up: keep
                KeyNavigation.left: cancel
                onClicked: {
                    dialog.startRequested(dialog.info.library, dialog.all, !dialog.keepLocked)
                    dialog.close()
                }
            }
        }
        Label {
            text: qsTr("↑/↓ chooses · Space ticks · Enter starts · Back cancels")
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
        }
    }
}
