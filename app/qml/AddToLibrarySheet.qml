pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// Issue #154: Add to Library. What will be copied, from where to where, the tags before and after
// and the file names; nothing happens before Add. `backend` is the core or a test stub:
// libraryCall("importPlan", {item}) and ("importStart", {item}), answered with musicDetailReady
// ("importSheet", "imports") or requestFailed. Emits `started` and `settingsRequested`.
Popup {
    id: sheet
    property var backend: null
    property var itemId: 0
    property Item from: null
    property var plan: null
    property string error: ""
    property var pending: ({ plan: 0, start: 0 })
    signal started()
    signal settingsRequested()
    // Issue #155: choose another release before adding.
    signal anotherRelease(var item, string title, string artist)

    readonly property var first: plan && plan.plans && plan.plans.length > 0 ? plan.plans[0] : null
    readonly property int count: plan ? Number(plan.files || 0) : 0

    function show(id, item) {
        itemId = id
        from = item || null
        plan = null
        error = ""
        open()
        pending.plan = backend.libraryCall("importPlan", { item: Number(id) })
    }
    function fieldName(f) {
        return ({ title: qsTr("Title"), artist: qsTr("Artist"), album: qsTr("Album"), date: qsTr("Date"),
                  track: qsTr("Track"), ids: qsTr("MusicBrainz IDs"), isrc: qsTr("ISRC"), barcode: qsTr("Barcode") })[f] || f
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(880, (parent ? parent.width : 900) - 60)
    height: Math.min(720, (parent ? parent.height : 760) - 40)
    modal: true
    focus: true
    padding: 24
    closePolicy: Popup.CloseOnEscape
    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }
    onOpened: cancel.forceActiveFocus()
    onClosed: if (from) from.forceActiveFocus()

    Connections {
        target: sheet.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === sheet.pending.plan && kind === "importSheet") {
                sheet.plan = detail
                Qt.callLater(() => (detail.configured && sheet.count > 0 ? add : cancel).forceActiveFocus())
            } else if (requestId === sheet.pending.start && kind === "imports") {
                sheet.started()
                sheet.close()
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === sheet.pending.plan || requestId === sheet.pending.start) sheet.error = message
        }
    }

    contentItem: ColumnLayout {
        spacing: 12
        Label { text: qsTr("Add to Library"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; Accessible.role: Accessible.Heading }
        Label {
            objectName: "sheetTitle"
            visible: !!sheet.first
            Layout.fillWidth: true
            elide: Text.ElideRight
            color: Theme.text
            font.bold: true
            text: sheet.plan && sheet.plan.plans.length > 1
                  ? qsTr("%n item(s)", "", sheet.plan.plans.length)
                  : sheet.first ? [sheet.first.title, sheet.first.subtitle].filter(x => x).join("  ·  ") : ""
        }
        FocusButton {
            id: another
            objectName: "sheetAnother"
            visible: !!sheet.first && sheet.first.kind === "music" && sheet.plan.plans.length === 1 && sheet.count > 0
            text: qsTr("Another release…")
            KeyNavigation.down: cancel
            onClicked: {
                const f = sheet.first
                sheet.close()
                sheet.anotherRelease(f.item, f.title, f.subtitle)
            }
        }
        Label {
            objectName: "sheetNotConfigured"
            visible: !!sheet.plan && !sheet.plan.configured
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.accent
            text: qsTr("Choose where your library lives first.")
        }
        Label {
            objectName: "sheetReview"
            visible: !!sheet.plan && Number(sheet.plan.review) > 0
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.accent
            text: qsTr("%n item(s) are not surely matched or are already in your library. They wait under Imports → To review, and nothing is guessed.", "", Number(sheet.plan ? sheet.plan.review : 0))
        }
        Label {
            objectName: "sheetAlready"
            visible: !!sheet.plan && sheet.plan.configured && sheet.count === 0
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.text
            text: qsTr("Already in your library. Nothing to copy.")
        }
        GridLayout {
            visible: !!sheet.first && sheet.count > 0
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 12
            rowSpacing: 4
            Label { text: qsTr("From"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            Label { objectName: "sheetFrom"; Layout.fillWidth: true; text: sheet.plan ? sheet.plan.from : ""; color: Theme.text; font.family: "monospace"; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle }
            Label { text: qsTr("To"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            Label { objectName: "sheetTo"; Layout.fillWidth: true; text: sheet.first ? sheet.first.to : ""; color: Theme.accent; font.family: "monospace"; font.pixelSize: Theme.fontSmall; elide: Text.ElideMiddle }
        }
        RowLayout {
            visible: sheet.count > 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 18
            ColumnLayout {
                visible: !!sheet.plan && (sheet.plan.tags || []).length > 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.preferredWidth: 3
                spacing: 6
                Label { text: qsTr("Tags written to the copies"); color: Theme.text; font.bold: true }
                RowLayout {
                    Layout.fillWidth: true
                    Label { Layout.preferredWidth: 130; text: qsTr("Field"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    Label { Layout.fillWidth: true; Layout.preferredWidth: 1; text: qsTr("In the file now"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    Label { Layout.fillWidth: true; Layout.preferredWidth: 1; text: qsTr("In the copy"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                }
                Repeater {
                    model: sheet.plan ? (sheet.plan.tags || []) : []
                    RowLayout {
                        id: tagRow
                        required property var modelData
                        Layout.fillWidth: true
                        Label { Layout.preferredWidth: 130; text: sheet.fieldName(tagRow.modelData.field); color: Theme.text; font.pixelSize: Theme.fontSmall }
                        Label { Layout.fillWidth: true; Layout.preferredWidth: 1; text: tagRow.modelData.now || "—"; color: Theme.textDim; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                        Label { Layout.fillWidth: true; Layout.preferredWidth: 1; text: tagRow.modelData.then || "—"; color: tagRow.modelData.changed ? Theme.accent : Theme.text; font.pixelSize: Theme.fontSmall; elide: Text.ElideRight }
                    }
                }
                Item { Layout.fillHeight: true }
            }
            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.preferredWidth: 2
                spacing: 4
                Label { text: qsTr("Files"); color: Theme.text; font.bold: true }
                Repeater {
                    model: sheet.first ? sheet.first.names : []
                    Label {
                        required property var modelData
                        Layout.fillWidth: true
                        text: modelData
                        color: Theme.text
                        font.family: "monospace"
                        font.pixelSize: Theme.fontSmall
                        elide: Text.ElideRight
                    }
                }
                Label {
                    visible: sheet.count > (sheet.first ? sheet.first.names.length : 0)
                    text: qsTr("… %n more", "", sheet.count - (sheet.first ? sheet.first.names.length : 0))
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
                Item { Layout.fillHeight: true }
            }
        }
        Item { visible: sheet.count === 0; Layout.fillHeight: true }
        Label {
            objectName: "sheetError"
            visible: sheet.error !== ""
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.danger
            text: sheet.error
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: sheet.count > 0 ? MediaInfo.size(Number(sheet.plan.size || 0)) + "  ·  " + qsTr("The original files stay where they are.") : ""
            }
            FocusButton { id: cancel; objectName: "sheetCancel"; text: qsTr("Cancel"); KeyNavigation.up: another.visible ? another : null; KeyNavigation.right: add.visible ? add : settings; onClicked: sheet.close() }
            FocusButton {
                id: settings
                visible: !!sheet.plan && !sheet.plan.configured
                text: qsTr("Settings → Library")
                KeyNavigation.left: cancel
                onClicked: { sheet.close(); sheet.settingsRequested() }
            }
            FocusButton {
                id: add
                objectName: "sheetAdd"
                visible: !!sheet.plan && sheet.plan.configured && sheet.count > 0
                primary: true
                text: qsTr("Add %n file(s) to Library", "", sheet.count)
                KeyNavigation.left: cancel
                onClicked: sheet.pending.start = sheet.backend.libraryCall("importStart", { item: Number(sheet.itemId) })
            }
        }
    }
}
