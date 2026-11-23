pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "MediaInfo.js" as MediaInfo

// Issue #154: Imports to Library, under Downloads: what waits for you, what is copying, what is
// done. `backend`: libraryCall("imports") and importPause, importResume, importRetry, importSkip
// with {job}, answered with musicDetailReady ("imports"). `reviewRequested(job)` opens the review
// screens. Up from the first control emits `leaveUp`.
FocusScope {
    id: panel
    property var backend: null
    property var data_: ({ jobs: [], waiting: 0, active: 0 })
    property var pending: 0
    signal reviewRequested(var job)
    signal leaveUp()
    signal leaveLeft()

    readonly property var review: (data_.jobs || []).filter(j => j.state === "review")
    readonly property var now: (data_.jobs || []).filter(j => j.state === "queued" || j.state === "paused")
    readonly property var done: (data_.jobs || []).filter(j => j.state === "done" || j.state === "failed")
    readonly property var rows: [].concat(
        review.length ? [{ head: qsTr("To review (%1)").arg(review.length) }] : [], review,
        now.length ? [{ head: qsTr("Now") }] : [], now,
        done.length ? [{ head: qsTr("Done") }] : [], done)

    function load() { if (backend) pending = backend.libraryCall("imports", {}) }
    function act(name, job) { pending = backend.libraryCall(name, { job: job.id }) }
    function focusFirst() { jobList.forceActiveFocus() }
    function whyText(j) {
        if (j.review === "match") return qsTr("Not surely matched: which one is it?")
        if (j.review === "clash") return qsTr("The library already has this, or other files are in its folder.")
        return ""
    }
    function resultText(j) {
        if (j.state === "failed") return qsTr("Not copied: %1").arg(j.note)
        if (j.note === "already") return qsTr("Already in your library  ·  nothing copied")
        return qsTr("Added %n file(s)", "", Number(j.filesDone))
    }
    function progressText(j) {
        if (j.state === "paused") return qsTr("Paused at %1 of %2 files").arg(j.filesDone).arg(j.files)
        if (j.note) return qsTr("Waiting for the source: %1").arg(j.note)
        return qsTr("Copying %1 of %2 files").arg(Math.min(Number(j.filesDone) + 1, Number(j.files))).arg(j.files)
    }

    Component.onCompleted: load()
    Timer { interval: 1000; repeat: true; running: panel.visible && Number(panel.data_.active || 0) > 0; onTriggered: panel.load() }
    onVisibleChanged: if (visible) load()
    Connections {
        target: panel.backend
        function onMusicDetailReady(requestId, kind, detail) {
            if (kind === "imports") panel.data_ = detail
        }
    }

    Label {
        objectName: "importsEmpty"
        visible: panel.rows.length === 0
        anchors { left: parent.left; right: parent.right; top: parent.top; topMargin: 10 }
        wrapMode: Text.Wrap
        color: Theme.textDim
        text: qsTr("Nothing is being imported. Add to Library is in the menu of a song, album, artist, movie, show or book.")
    }
    ListView {
        id: jobList
        objectName: "jobList"
        anchors.fill: parent
        clip: true
        spacing: 6
        focus: true
        model: panel.rows
        keyNavigationEnabled: false
        ScrollBar.vertical: ScrollBar {}
        function move(d) {
            let i = currentIndex + d
            while (i >= 0 && i < count && panel.rows[i].head !== undefined) i += d
            if (i >= 0 && i < count) { currentIndex = i; return true }
            return false
        }
        onCountChanged: if (count > 0 && (currentIndex < 0 || !panel.rows[currentIndex] || panel.rows[currentIndex].head !== undefined)) { currentIndex = 0; move(1) }
        Keys.onUpPressed: if (!jobList.move(-1)) panel.leaveUp()
        Keys.onDownPressed: jobList.move(1)
        Keys.onLeftPressed: panel.leaveLeft()
        delegate: FocusScope {
            id: row
            required property var modelData
            required property int index
            readonly property bool isHead: modelData.head !== undefined
            width: ListView.view.width
            height: isHead ? 34 : 74
            focus: ListView.isCurrentItem
            Label {
                visible: row.isHead
                anchors.bottom: parent.bottom
                text: row.modelData.head || ""
                color: row.modelData.head && row.index === 0 && panel.review.length ? Theme.accent : Theme.text
                font.bold: true
            }
            Rectangle {
                visible: !row.isHead
                anchors.fill: parent
                radius: Theme.radius
                color: row.activeFocus ? Theme.surfaceHi : Theme.surface
                border.width: row.activeFocus ? Theme.focusWidth : 0
                border.color: Theme.focus
                RowLayout {
                    anchors { fill: parent; leftMargin: 14; rightMargin: 12 }
                    spacing: 12
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 3
                        Label { Layout.fillWidth: true; text: row.modelData.title || ""; color: Theme.text; font.bold: true; elide: Text.ElideRight }
                        ProgressBar {
                            visible: row.modelData.state === "queued" || row.modelData.state === "paused"
                            Layout.fillWidth: true
                            implicitHeight: 5
                            from: 0
                            to: Math.max(1, Number(row.modelData.size || 0))
                            value: Number(row.modelData.sizeDone || 0)
                        }
                        Label {
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                            font.pixelSize: Theme.fontSmall
                            color: row.modelData.state === "review" ? Theme.accent : row.modelData.state === "failed" ? Theme.danger : Theme.textDim
                            text: row.isHead ? "" : row.modelData.state === "review" ? panel.whyText(row.modelData)
                                  : row.modelData.state === "done" || row.modelData.state === "failed" ? panel.resultText(row.modelData)
                                  : panel.progressText(row.modelData)
                        }
                    }
                    FocusButton {
                        id: mainAction
                        objectName: "jobAction" + row.index
                        focus: true
                        visible: !row.isHead && row.modelData.state !== "done"
                        primary: row.modelData.state === "review"
                        text: row.modelData.state === "review" ? (row.modelData.review === "match" ? qsTr("Choose…") : qsTr("Decide…"))
                              : row.modelData.state === "queued" ? qsTr("Pause")
                              : row.modelData.state === "paused" ? qsTr("Resume") : qsTr("Retry")
                        KeyNavigation.right: skipAction.visible ? skipAction : null
                        onClicked: {
                            const j = row.modelData
                            if (j.state === "review") panel.reviewRequested(j)
                            else panel.act(j.state === "queued" ? "importPause" : j.state === "paused" ? "importResume" : "importRetry", j)
                        }
                    }
                    FocusButton {
                        id: skipAction
                        objectName: "jobSkip" + row.index
                        visible: !row.isHead && (row.modelData.state === "review" || row.modelData.state === "paused" || row.modelData.state === "failed")
                        text: qsTr("Skip")
                        KeyNavigation.left: mainAction
                        onClicked: panel.act("importSkip", row.modelData)
                    }
                }
            }
        }
    }
}
