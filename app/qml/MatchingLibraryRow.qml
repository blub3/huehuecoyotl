pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// One library in Settings → Matching (issues #93 to #96): its name and source, how much of it is
// matched, and "Match again…"; while it runs, the progress and Stop; afterwards the result. `info`
// is one library of Core.matchingStatus(): library, total, matched, review, none, locked,
// episodes, songs, artists, authors, blocked (noKey, offline) and run (state, total, done, newly,
// changed, review, notFound). Up and down leave the row.
Rectangle {
    id: row
    property var info: ({})
    // The name of the library running before this one, for "waiting".
    property string runningName: ""
    signal startRequested()
    signal stopRequested()
    signal upRequested()
    signal downRequested()
    signal leftEdge()

    readonly property string library: info.library || ""
    readonly property var run: info.run || null
    readonly property string runState: run ? run.state : ""
    readonly property bool running: runState === "running"
    readonly property bool waiting: runState === "waiting"
    readonly property bool blocked: (info.blocked || "") !== ""
    readonly property Item firstButton: running || waiting ? stopButton : startButton

    // Seconds one item takes: TMDB answers quickly, MusicBrainz and Open Library allow one request
    // a second and an album or book takes about two.
    function secondsPerItem() { return library === "music" ? 2 : library === "books" ? 1.5 : 0.25 }
    function minutes(items) { return Math.max(1, Math.round(items * secondsPerItem() / 60)) }
    function name() {
        switch (library) {
        case "movies": return qsTr("Movies")
        case "shows": return qsTr("TV shows")
        case "music": return qsTr("Music")
        case "books": return qsTr("Audiobooks")
        }
        return library
    }
    function source() {
        switch (library) {
        case "movies": case "shows": return qsTr("from TMDB")
        case "music": return qsTr("from MusicBrainz and the Cover Art Archive")
        case "books": return qsTr("from Open Library")
        }
        return ""
    }
    function totalLine() {
        const n = info.total || 0
        switch (library) {
        case "movies": return qsTr("%n movie(s)", "", n)
        case "shows": return qsTr("%n show(s)", "", n) + " (" + qsTr("%n episode(s)", "", info.episodes || 0) + ")"
        case "music": return qsTr("%n album(s)", "", n) + " (" + qsTr("%n song(s)", "", info.songs || 0) + ", "
                             + qsTr("%n artist(s)", "", info.artists || 0) + ")"
        case "books": return qsTr("%n book(s)", "", n) + " (" + qsTr("%n author(s)", "", info.authors || 0) + ")"
        }
        return String(n)
    }
    function resultLine() {
        if (!run) return ""
        const parts = [qsTr("%n newly matched", "", run.newly || 0), qsTr("%n changed", "", run.changed || 0),
                       qsTr("%n need(s) a choice", "", run.review || 0)]
        return (runState === "stopped" ? qsTr("Stopped: %1.") : qsTr("Done: %1.")).arg(parts.join(", "))
    }
    function blockedLine() {
        if (info.blocked === "noKey") return qsTr("Set a TMDB key under Metadata first.")
        if (info.blocked === "offline") return qsTr("Turn on music and book information online under Metadata first: nothing is sent while it is off.")
        return ""
    }

    implicitHeight: content.implicitHeight + 28
    radius: Theme.radius
    color: Theme.surface
    border.width: 1
    border.color: running ? Theme.accent : Theme.surfaceHi

    RowLayout {
        id: content
        anchors.fill: parent
        anchors.margins: 14
        spacing: 16
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            RowLayout {
                spacing: 10
                Label { objectName: "libraryName"; text: row.name(); color: Theme.text; font.pixelSize: Theme.fontBody; font.bold: true }
                Label { text: row.source(); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
            }
            Label {
                objectName: "countsLine"
                visible: !row.running
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: [row.totalLine(), qsTr("%1 matched").arg(row.info.matched || 0),
                       qsTr("%n need(s) a choice", "", row.info.review || 0),
                       qsTr("%1 not found").arg(row.info.none || 0),
                       qsTr("%1 chosen by you").arg(row.info.locked || 0)].join("  ·  ")
            }
            ProgressBar {
                id: bar
                visible: row.running
                Layout.fillWidth: true
                from: 0
                to: Math.max(1, row.run ? row.run.total : 1)
                value: row.run ? row.run.done : 0
                Accessible.name: progressLine.text
                background: Rectangle { implicitHeight: 6; radius: 3; color: Theme.surfaceHi }
                contentItem: Item {
                    Rectangle { width: bar.visualPosition * parent.width; height: parent.height; radius: 3; color: Theme.accent }
                }
            }
            Label {
                id: progressLine
                objectName: "progressLine"
                visible: row.running || row.waiting
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: row.waiting
                      ? (row.runningName !== "" ? qsTr("Waiting for %1 to finish.").arg(row.runningName) : qsTr("Waiting…"))
                      : row.running ? qsTr("Matching… %1 of %2 · about %n min left", "", row.minutes((row.run.total || 0) - (row.run.done || 0)))
                                      .arg(row.run.done || 0).arg(row.run.total || 0) : ""
            }
            Label {
                objectName: "resultLine"
                visible: row.runState === "done" || row.runState === "stopped"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                text: row.resultLine()
            }
            Label {
                objectName: "blockedLine"
                visible: row.blocked
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.focus
                font.pixelSize: Theme.fontSmall
                text: row.blockedLine()
            }
        }
        FocusButton {
            id: stopButton
            objectName: "stopButton"
            visible: row.running || row.waiting
            text: qsTr("Stop")
            Accessible.description: qsTr("Stop matching %1 again; what was matched so far stays.").arg(row.name())
            Keys.onUpPressed: row.upRequested()
            Keys.onDownPressed: row.downRequested()
            Keys.onLeftPressed: row.leftEdge()
            onClicked: row.stopRequested()
        }
        FocusButton {
            id: startButton
            objectName: "startButton"
            visible: !row.running && !row.waiting
            enabled: !row.blocked
            opacity: enabled ? 1 : 0.45
            text: qsTr("Match again…")
            Accessible.description: qsTr("Match %1 again").arg(row.name())
            Keys.onUpPressed: row.upRequested()
            Keys.onDownPressed: row.downRequested()
            Keys.onLeftPressed: row.leftEdge()
            onClicked: row.startRequested()
        }
    }
}
