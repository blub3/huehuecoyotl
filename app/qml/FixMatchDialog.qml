pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Fix match for movies, shows and episodes (issues #97 to #100): the current match, a search at
// TMDB by title and year or by a pasted TMDB or IMDb id or link, a Movie / TV show switch for an
// item matched as the wrong kind, a confirm step that says what the choice changes (a merge,
// episodes the new show lacks, the files that move as what), and "Match automatically again".
// A choice is locked: rescans keep it. `backend` is the core (Core in the app, a stub in tests):
// matchSearch, matchPreview, matchChoose, matchAgain, and the signals musicDetailReady and
// requestFailed. `chosen(newItemId)` tells the page which item now carries the match.
Popup {
    id: dialog
    property var backend: null
    property var itemId
    property string itemKind: "movie"
    property string itemTitle: ""
    property int itemYear: 0
    property string state_: "search"
    // movie | show: what the search looks for.
    property string searchKind: "movie"
    property var current: ({})
    property var candidates: []
    property bool searching: false
    property string message: ""
    property string notice: ""
    property var picked: null
    property var preview: null
    property var files: []
    property var request: 0
    signal chosen(var newItemId)

    function openFor(id, title, year, kind) {
        itemId = id
        itemKind = kind || "movie"
        itemTitle = title || ""
        itemYear = year > 0 ? year : 0
        searchKind = itemKind === "show" ? "show" : "movie"
        titleField.text = itemTitle
        yearField.text = itemYear > 0 ? String(itemYear) : ""
        candidates = []
        current = ({})
        message = ""
        notice = ""
        picked = null
        preview = null
        state_ = "search"
        open()
        search()
    }
    function search() {
        if (!backend) return
        searching = true
        message = ""
        request = backend.matchSearch(itemId, { text: titleField.text, year: parseInt(yearField.text) || 0, kind: searchKind })
    }
    function setKind(k) {
        if (k === searchKind) return
        searchKind = k
        search()
    }
    function pick(c) {
        if (c.current) return
        picked = c
        preview = null
        files = []
        state_ = "confirm"
        message = ""
        request = backend.matchPreview(itemId, { id: c.id, kind: c.kind })
        Qt.callLater(() => confirmButton.forceActiveFocus())
    }
    function confirm() {
        if (!picked || !preview) return
        const moving = files.filter(f => f.fixed || f.on).map(f => ({ fileId: f.fileId, season: f.season, episode: f.episode }))
        request = backend.matchChoose(itemId, { id: picked.id, kind: picked.kind, files: moving })
    }
    function matchAutomatically() {
        notice = ""
        message = ""
        request = backend.matchAgain(itemId, true)
    }
    readonly property bool kindChange: preview !== null && preview.change !== "none"
    readonly property bool numbersMissing: kindChange && preview.change === "toShow"
                                           && files.some(f => (f.fixed || f.on) && (!(f.season > 0) || !(f.episode > 0)))
    function heading() {
        if (!preview) return qsTr("Change the match?")
        if (preview.change === "toShow") return qsTr("Make this a TV show?")
        if (preview.change === "toMovie") return qsTr("Make this a movie?")
        return qsTr("Change the match?")
    }
    function kindName(k) { return k === "show" ? qsTr("TV show") : qsTr("Movie") }
    function explanation() {
        if (!preview) return ""
        if (preview.change === "toShow")
            return qsTr("The files become episodes of the show, by the season and episode numbers in their names. Other movies in the same folders are offered too; untick the ones that are not this show.")
        if (preview.change === "toMovie")
            return itemKind === "episode"
                   ? qsTr("The file leaves the show and becomes a movie. The show's other files in the same folder go with it as versions; untick the ones that belong to the show.")
                   : qsTr("The ticked files leave the show and become one movie, as its versions. A show left without files goes.")
        return itemKind === "show"
               ? qsTr("Poster, description and episode titles are replaced by the new match; episodes are assigned by their numbers and keep their watched state. Your own NFO data stays.")
               : qsTr("Poster, description and cast are replaced by the new match. Watched state and versions stay. Your own NFO data stays.")
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    modal: true
    focus: true
    width: Math.min(parent ? parent.width - 80 : 800, 860)
    height: Math.min(parent ? parent.height - 80 : 700, 760)
    padding: 24
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    onOpened: titleField.forceActiveFocus()

    background: Rectangle { radius: Theme.radius; color: Theme.surface; border.color: Theme.surfaceHi }

    Connections {
        target: dialog.backend
        ignoreUnknownSignals: true
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId !== dialog.request) return
            if (kind === "matchCandidates") {
                dialog.searching = false
                dialog.current = detail.current || ({})
                dialog.candidates = detail.candidates || []
            } else if (kind === "matchPreview") {
                dialog.preview = detail
                dialog.files = (detail.files || []).map(f => Object.assign({}, f))
            } else if (kind === "matchChosen") {
                dialog.chosen(detail.newItemId)
                dialog.close()
            } else if (kind === "matchAgain") {
                const titled = detail.title ? " " + qsTr("Now: %1.").arg(detail.title) : ""
                switch (detail.result) {
                case "unchanged": dialog.notice = qsTr("Matched again: no change."); break
                case "newly": case "changed": dialog.notice = qsTr("Matched again.") + titled; break
                case "review": dialog.notice = qsTr("Several matches fit. Choose one below."); break
                case "notFound": dialog.notice = qsTr("Nothing sure was found; the match stays."); break
                }
                if (detail.result === "newly" || detail.result === "changed") dialog.chosen(detail.newItemId)
                dialog.search()
            }
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId !== dialog.request) return
            dialog.searching = false
            dialog.message = message
        }
    }

    contentItem: StackLayout {
        currentIndex: dialog.state_ === "confirm" ? 1 : 0
        Keys.onPressed: (e) => {
            if (e.key === Qt.Key_Back) {
                if (dialog.state_ === "confirm") dialog.state_ = "search"; else dialog.close()
                e.accepted = true
            }
        }

        // ---- search ------------------------------------------------------------------------
        ColumnLayout {
            spacing: 12
            RowLayout {
                Layout.fillWidth: true
                Label { text: qsTr("Fix match"); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true; Layout.fillWidth: true }
                FocusButton {
                    id: againButton
                    objectName: "matchAgainButton"
                    text: qsTr("Match automatically again")
                    visible: dialog.itemKind !== "episode"
                    KeyNavigation.down: movieKind
                    onClicked: dialog.matchAutomatically()
                }
            }
            Rectangle {
                Layout.fillWidth: true
                visible: dialog.current.title !== undefined
                implicitHeight: currentRow.implicitHeight + 16
                radius: 8
                color: Theme.bg
                RowLayout {
                    id: currentRow
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 10
                    Label {
                        objectName: "currentLine"
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        color: Theme.text
                        font.pixelSize: Theme.fontSmall
                        text: qsTr("Matched now: %1").arg(dialog.current.title + (dialog.current.year ? " (" + dialog.current.year + ")" : ""))
                              + "  ·  " + (dialog.current.state !== "matched" ? qsTr("not sure yet")
                                           : dialog.current.locked ? qsTr("chosen by you") : qsTr("chosen automatically"))
                    }
                }
            }
            Label {
                objectName: "noticeLine"
                visible: dialog.notice !== ""
                text: dialog.notice
                color: Theme.accent
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            RowLayout {
                spacing: 10
                Layout.fillWidth: true
                // Movie / TV show: what the search looks for.
                Row {
                    spacing: 4
                    Accessible.role: Accessible.RadioButton
                    FocusButton {
                        id: movieKind
                        objectName: "movieKind"
                        text: qsTr("Movie")
                        primary: dialog.searchKind === "movie"
                        Accessible.checkable: true
                        Accessible.checked: primary
                        KeyNavigation.right: showKind.visible ? showKind : titleField
                        KeyNavigation.up: againButton.visible ? againButton : null
                        KeyNavigation.down: results
                        onClicked: dialog.setKind("movie")
                    }
                    FocusButton {
                        id: showKind
                        objectName: "showKind"
                        visible: dialog.itemKind !== "episode"
                        text: qsTr("TV show")
                        primary: dialog.searchKind === "show"
                        Accessible.checkable: true
                        Accessible.checked: primary
                        KeyNavigation.left: movieKind
                        KeyNavigation.right: titleField
                        KeyNavigation.up: againButton.visible ? againButton : null
                        KeyNavigation.down: results
                        onClicked: dialog.setKind("show")
                    }
                }
                TextField {
                    id: titleField
                    objectName: "titleField"
                    Layout.fillWidth: true
                    placeholderText: qsTr("Title, or a TMDB or IMDb id or link")
                    font.pixelSize: Theme.fontBody
                    Accessible.name: qsTr("Title")
                    KeyNavigation.right: yearField
                    KeyNavigation.down: results
                    Keys.onLeftPressed: (e) => { if (cursorPosition === 0) (showKind.visible ? showKind : movieKind).forceActiveFocus(); else e.accepted = false }
                    onAccepted: dialog.search()
                }
                TextField {
                    id: yearField
                    Layout.preferredWidth: 90
                    placeholderText: qsTr("Year")
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator { bottom: 0; top: 2100 }
                    Accessible.name: qsTr("Year")
                    KeyNavigation.left: titleField
                    KeyNavigation.right: searchButton
                    KeyNavigation.down: results
                    onAccepted: dialog.search()
                }
                FocusButton { id: searchButton; text: qsTr("Search"); KeyNavigation.left: yearField; KeyNavigation.down: results; onClicked: dialog.search() }
            }
            Label {
                objectName: "kindHint"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: (dialog.searchKind === "movie"
                       ? (dialog.itemKind === "episode" ? qsTr("Searching movies: is this episode really a film?") : qsTr("Searching movies. Is it a series? Switch to TV show."))
                       : qsTr("Searching TV shows. Is it a single film? Switch to Movie."))
                      + " " + qsTr("Or paste a TMDB or IMDb id or link.")
            }
            Label { visible: dialog.message !== ""; color: Theme.danger; text: dialog.message; Layout.fillWidth: true; wrapMode: Text.Wrap }
            BusyIndicator { visible: dialog.searching; running: visible; Layout.alignment: Qt.AlignHCenter }
            Label {
                visible: !dialog.searching && dialog.candidates.length === 0 && dialog.message === ""
                text: qsTr("No results. Try another title or remove the year.")
                color: Theme.textDim
            }
            ListView {
                id: results
                objectName: "results"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 6
                model: dialog.candidates
                keyNavigationEnabled: true
                Accessible.role: Accessible.List
                Accessible.name: qsTr("Matches")
                Keys.onUpPressed: (e) => { if (currentIndex === 0) titleField.forceActiveFocus(); else e.accepted = false }
                delegate: ItemDelegate {
                    id: cand
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    height: 104
                    focus: ListView.isCurrentItem
                    enabled: !cand.modelData.current
                    Accessible.name: modelData.title + (modelData.year > 0 ? " (" + modelData.year + ")" : "") + ", " + dialog.kindName(modelData.kind)
                                     + (modelData.current ? ", " + qsTr("matched now") : "")
                    onClicked: dialog.pick(cand.modelData)
                    Keys.onReturnPressed: dialog.pick(cand.modelData)
                    Keys.onEnterPressed: dialog.pick(cand.modelData)
                    background: Rectangle {
                        radius: Theme.radius
                        color: cand.activeFocus || cand.hovered ? Theme.surfaceHi : "transparent"
                        border.width: cand.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        spacing: 14
                        opacity: cand.modelData.current ? 0.6 : 1
                        Rectangle {
                            Layout.preferredWidth: 58; Layout.preferredHeight: 86; radius: 6; clip: true; color: Theme.surfaceHi
                            Image { anchors.fill: parent; source: cand.modelData.image || ""; asynchronous: true; fillMode: Image.PreserveAspectCrop }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 3
                            RowLayout {
                                spacing: 8
                                Label {
                                    text: cand.modelData.title + (cand.modelData.year > 0 ? "  (" + cand.modelData.year + ")" : "")
                                    color: Theme.text; font.bold: true; font.pixelSize: Theme.fontBody
                                }
                                Rectangle {
                                    implicitWidth: kindLabel.implicitWidth + 12; implicitHeight: kindLabel.implicitHeight + 4; radius: 4
                                    color: cand.modelData.kind === "show" ? "#c9e2f2" : Theme.surfaceHi
                                    Label { id: kindLabel; anchors.centerIn: parent; text: dialog.kindName(cand.modelData.kind); font.pixelSize: 11; font.bold: true
                                            color: cand.modelData.kind === "show" ? Theme.bg : Theme.text }
                                }
                                Label { visible: !!cand.modelData.current; text: qsTr("matched now"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                            }
                            Label {
                                Layout.fillWidth: true
                                text: cand.modelData.overview || ""
                                color: Theme.textDim; font.pixelSize: Theme.fontSmall
                                wrapMode: Text.Wrap; maximumLineCount: 2; elide: Text.ElideRight
                            }
                        }
                        Label {
                            visible: cand.modelData.score !== undefined
                            text: qsTr("%1 % sure").arg(Math.round((cand.modelData.score || 0) * 100))
                            color: Theme.textDim; font.pixelSize: Theme.fontSmall
                        }
                    }
                }
            }
            Label {
                text: qsTr("← → switches Movie / TV show · ↓ to the results · Enter chooses · Back closes")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
            }
        }

        // ---- confirm -----------------------------------------------------------------------
        ColumnLayout {
            spacing: 14
            Label { objectName: "confirmHeading"; text: dialog.heading(); color: Theme.text; font.pixelSize: Theme.fontLarge; font.bold: true }
            RowLayout {
                spacing: 16
                Layout.fillWidth: true
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: nowCol.implicitHeight + 24
                    radius: 10
                    color: Theme.bg
                    ColumnLayout {
                        id: nowCol
                        anchors.fill: parent; anchors.margins: 12
                        spacing: 4
                        Label { text: qsTr("Now"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                        Label { text: dialog.current.title || dialog.itemTitle; color: Theme.text; font.bold: true; Layout.fillWidth: true; elide: Text.ElideRight }
                        Label { text: dialog.itemKind === "movie" ? qsTr("Movie") : dialog.itemKind === "episode" ? qsTr("Episode") : qsTr("TV show"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    }
                }
                Label { text: "→"; color: Theme.accent; font.pixelSize: 26 }
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: toCol.implicitHeight + 24
                    radius: 10
                    color: Theme.bg
                    border.color: Theme.accent
                    ColumnLayout {
                        id: toCol
                        anchors.fill: parent; anchors.margins: 12
                        spacing: 4
                        Label { text: qsTr("Becomes"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                        Label {
                            objectName: "becomesTitle"
                            text: dialog.picked ? dialog.picked.title + (dialog.picked.year > 0 ? " (" + dialog.picked.year + ")" : "") : ""
                            color: Theme.text; font.bold: true; Layout.fillWidth: true; elide: Text.ElideRight
                        }
                        Label { text: dialog.picked ? dialog.kindName(dialog.picked.kind) : ""; color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                    }
                }
            }
            BusyIndicator { visible: dialog.preview === null && dialog.message === ""; running: visible; Layout.alignment: Qt.AlignHCenter }
            Label { objectName: "explanation"; text: dialog.explanation(); color: Theme.text; Layout.fillWidth: true; wrapMode: Text.Wrap }
            Label {
                objectName: "mergeNote"
                visible: dialog.preview !== null && !!dialog.preview.merge
                text: dialog.preview && dialog.preview.merge ? qsTr("“%1” is in your library already with this match: they become one.").arg(dialog.preview.merge) : ""
                color: Theme.focus; Layout.fillWidth: true; wrapMode: Text.Wrap
            }
            Label {
                objectName: "missingNote"
                visible: dialog.preview !== null && (dialog.preview.missing || []).length > 0
                text: dialog.preview ? qsTr("Not in this show at TMDB: %1. They stay as unmatched episodes.").arg((dialog.preview.missing || []).join(", ")) : ""
                color: Theme.focus; Layout.fillWidth: true; wrapMode: Text.Wrap
            }
            Label {
                visible: dialog.kindChange
                text: dialog.preview && dialog.preview.change === "toShow" ? qsTr("Files that move into the show") : qsTr("Files that become the movie")
                color: Theme.textDim; font.pixelSize: Theme.fontSmall
            }
            ListView {
                id: fileList
                objectName: "fileList"
                visible: dialog.kindChange
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 4
                model: dialog.kindChange ? dialog.files : []
                keyNavigationEnabled: true
                Keys.onDownPressed: (e) => { if (currentIndex === count - 1) confirmButton.forceActiveFocus(); else e.accepted = false }
                delegate: FocusScope {
                    id: fileRow
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    implicitHeight: fileLine.implicitHeight
                    focus: ListView.isCurrentItem
                    readonly property bool toShow: dialog.preview !== null && dialog.preview.change === "toShow"
                    function update(change) {
                        const f = dialog.files.slice()
                        f[fileRow.index] = Object.assign({}, f[fileRow.index], change)
                        dialog.files = f
                    }
                    RowLayout {
                        id: fileLine
                        width: parent.width
                        spacing: 10
                        CheckBox {
                            id: tick
                            objectName: "fileTick" + fileRow.index
                            focus: true
                            checked: fileRow.modelData.fixed || fileRow.modelData.on
                            enabled: !fileRow.modelData.fixed
                            Accessible.name: fileRow.modelData.name
                            KeyNavigation.right: fileRow.toShow ? seasonBox : null
                            onToggled: fileRow.update({ on: checked })
                            background: Rectangle { radius: 6; color: "transparent"; border.width: tick.activeFocus ? Theme.focusWidth : 0; border.color: Theme.focus }
                        }
                        Label { text: fileRow.modelData.name; color: Theme.text; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; elide: Text.ElideMiddle }
                        Label { visible: fileRow.toShow; text: qsTr("Season"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                        SpinBox {
                            id: seasonBox
                            objectName: "season" + fileRow.index
                            visible: fileRow.toShow
                            from: 0; to: 99
                            value: fileRow.modelData.season || 0
                            editable: true
                            Accessible.name: qsTr("Season of %1").arg(fileRow.modelData.name)
                            KeyNavigation.left: tick
                            KeyNavigation.right: episodeBox
                            onValueModified: fileRow.update({ season: value })
                        }
                        Label { visible: fileRow.toShow; text: qsTr("Episode"); color: Theme.textDim; font.pixelSize: Theme.fontSmall }
                        SpinBox {
                            id: episodeBox
                            objectName: "episode" + fileRow.index
                            visible: fileRow.toShow
                            from: 0; to: 999
                            value: fileRow.modelData.episode || 0
                            editable: true
                            Accessible.name: qsTr("Episode of %1").arg(fileRow.modelData.name)
                            KeyNavigation.left: seasonBox
                            onValueModified: fileRow.update({ episode: value })
                        }
                    }
                }
            }
            Item { visible: !dialog.kindChange; Layout.fillHeight: true }
            Label {
                visible: dialog.numbersMissing
                objectName: "numbersNote"
                text: qsTr("Give every ticked file a season and an episode number.")
                color: Theme.focus; Layout.fillWidth: true; wrapMode: Text.Wrap
            }
            Label { visible: dialog.message !== ""; color: Theme.danger; text: dialog.message; Layout.fillWidth: true; wrapMode: Text.Wrap }
            Label {
                text: qsTr("Watched state and resume points stay with the files. The new match is locked: rescans keep it.")
                color: Theme.textDim; font.pixelSize: Theme.fontSmall; Layout.fillWidth: true; wrapMode: Text.Wrap
            }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 10
                FocusButton {
                    id: backButton
                    objectName: "backButton"
                    text: qsTr("Back")
                    KeyNavigation.right: confirmButton
                    KeyNavigation.up: fileList.visible ? fileList : null
                    onClicked: { dialog.state_ = "search"; results.forceActiveFocus() }
                }
                FocusButton {
                    id: confirmButton
                    objectName: "confirmButton"
                    primary: true
                    enabled: dialog.preview !== null && !dialog.numbersMissing
                    text: !dialog.preview || dialog.preview.change === "none" ? qsTr("Use this match")
                          : dialog.preview.change === "toShow" ? qsTr("Make it a TV show") : qsTr("Make it a movie")
                    KeyNavigation.left: backButton
                    KeyNavigation.up: fileList.visible ? fileList : null
                    onClicked: dialog.confirm()
                }
            }
        }
    }
}
