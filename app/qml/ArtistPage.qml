pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// An artist (issue #10): the image and biography when an optional provider supplied them
// (issue #12), else the initials on a tinted tile; then the albums in rows by release type
// (album, EP, single, compilation, other), each sorted by year, and the albums by others that
// credit the artist ("Appears on"). Up and down move between rows, left and right along a row.
FocusScope {
    id: page
    readonly property bool isPlayer: false
    property var itemId
    property var d: ({})
    property var request: 0
    readonly property bool loaded: d.id !== undefined

    readonly property var groups: {
        const labels = { album: qsTr("Albums"), ep: qsTr("EPs"), single: qsTr("Singles"),
                         compilation: qsTr("Compilations"), other: qsTr("Other releases") }
        const out = []
        for (const key of ["album", "ep", "single", "compilation", "other"]) {
            const albums = (d.albums || []).filter(a => a.releaseType === key)
            if (albums.length > 0) out.push({ label: labels[key], albums: albums })
        }
        if ((d.appearsOn || []).length > 0) out.push({ label: qsTr("Appears on"), albums: d.appearsOn })
        return out
    }
    function initials(name) {
        return (name || "").split(/\s+/).filter(w => w.length > 0).slice(0, 2).map(w => w[0].toUpperCase()).join("")
    }
    function focusRow(i) {
        const r = rows.itemAt(i)
        if (r) r.forceActiveFocus()
    }

    Component.onCompleted: request = Core.artistDetail(itemId)
    onActiveFocusChanged: if (activeFocus && loaded) Qt.callLater(() => { if (!page.Window.activeFocusItem || page.Window.activeFocusItem === page) page.focusRow(0) })

    Connections {
        target: Core
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId !== page.request) return
            const first = !page.loaded
            page.d = detail
            if (first) Qt.callLater(() => page.focusRow(0))
        }
        function onItemChanged(id) { if (id === page.itemId) page.request = Core.artistDetail(page.itemId) }
        function onLibraryChanged() { page.request = Core.artistDetail(page.itemId) }
    }

    // Issue #12: a background from an optional provider, faded into the page.
    Image {
        id: backdrop
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: Math.min(parent.height * 0.6, width * 9 / 16)
        source: page.d.background || ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        sourceSize.width: 1920
        opacity: status === Image.Ready ? 0.28 : 0
        Behavior on opacity { NumberAnimation { duration: 250 } }
    }
    // Fades the background into the page (at full strength, so its lower edge disappears).
    Rectangle {
        anchors.fill: backdrop
        visible: backdrop.status === Image.Ready
        gradient: Gradient {
            GradientStop { position: 0.0; color: "transparent" }
            GradientStop { position: 0.55; color: Qt.rgba(Theme.bg.r, Theme.bg.g, Theme.bg.b, 0.5) }
            GradientStop { position: 1.0; color: Theme.bg }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.implicitHeight + 96
        clip: true
        ScrollBar.vertical: ScrollBar {}
        function reveal(item) {
            const p = item.mapToItem(contentItem, 0, 0)
            if (p.y < contentY + 40) contentY = Math.max(0, p.y - 40)
            else if (p.y + item.height > contentY + height - 40)
                contentY = Math.min(p.y + item.height - height + 40, Math.max(0, contentHeight - height))
        }

        ColumnLayout {
            id: col
            x: 48
            y: 48
            width: flick.width - 96
            spacing: 24

            RowLayout {
                spacing: 32
                Layout.fillWidth: true
                Rectangle {
                    Layout.preferredWidth: 200
                    Layout.preferredHeight: 200
                    // Like the album covers.
                    radius: Theme.radius
                    clip: true
                    color: Qt.hsla(((page.d.title || "").length * 53 % 360) / 360, 0.32, 0.28, 1)
                    Label {
                        anchors.centerIn: parent
                        visible: photo.status !== Image.Ready
                        text: page.initials(page.d.title)
                        color: Theme.text
                        font.pixelSize: 64
                        font.bold: true
                    }
                    Image {
                        id: photo
                        anchors.fill: parent
                        source: page.d.image || ""
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        sourceSize.width: 400
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    // Issue #12: the artist's logo when a provider has one; the name stays.
                    Image {
                        visible: status === Image.Ready
                        Layout.preferredHeight: visible ? 72 : 0
                        Layout.preferredWidth: visible ? Math.min(360, implicitWidth * 72 / Math.max(1, implicitHeight)) : 0
                        source: page.d.logo || ""
                        fillMode: Image.PreserveAspectFit
                        asynchronous: true
                        sourceSize.height: 144
                        Accessible.ignored: true
                    }
                    Label {
                        Layout.fillWidth: true
                        text: page.d.title || ""
                        color: Theme.text
                        font.pixelSize: 36
                        font.bold: true
                        wrapMode: Text.Wrap
                        Accessible.role: Accessible.Heading
                    }
                    Label {
                        text: qsTr("%n album(s)", "", (page.d.albums || []).length)
                        color: Theme.textDim
                        font.pixelSize: Theme.fontBody
                    }
                    Label {
                        Layout.fillWidth: true
                        Layout.maximumWidth: 900
                        visible: (page.d.biography || "") !== ""
                        text: page.d.biography || ""
                        color: Theme.text
                        font.pixelSize: Theme.fontBody
                        wrapMode: Text.Wrap
                        maximumLineCount: 8
                        elide: Text.ElideRight
                    }
                }
            }

            Repeater {
                id: rows
                model: page.groups
                FocusScope {
                    id: rowBox
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    implicitHeight: rowCol.implicitHeight
                    ColumnLayout {
                    id: rowCol
                    width: parent.width
                    spacing: 10
                    Label {
                        text: rowBox.modelData.label
                        color: Theme.textDim
                        font.pixelSize: Theme.fontLarge
                    }
                    ListView {
                        id: list
                        focus: true
                        Layout.fillWidth: true
                        Layout.preferredHeight: Appearance.cardHeight(Appearance.coverWidth, 1) + 24
                        orientation: ListView.Horizontal
                        spacing: Math.round(26 * Appearance.textScale(Appearance.coverWidth))
                        clip: true
                        model: rowBox.modelData.albums
                        keyNavigationEnabled: true
                        topMargin: 10
                        leftMargin: 10
                        Accessible.role: Accessible.List
                        Accessible.name: rowBox.modelData.label
                        Keys.onUpPressed: (e) => { if (rowBox.index > 0) page.focusRow(rowBox.index - 1) }
                        Keys.onDownPressed: (e) => { if (rowBox.index < rows.count - 1) page.focusRow(rowBox.index + 1) }
                        Keys.onLeftPressed: (e) => { if (currentIndex === 0) Nav.focusRail(); else e.accepted = false }
                        onActiveFocusChanged: if (activeFocus) flick.reveal(rowBox)
                        delegate: PosterCard {
                            id: card
                            required property var modelData
                            title: modelData.title
                            caption: modelData.year > 0 ? String(modelData.year) : ""
                            poster: modelData.poster || ""
                            posterLarge: modelData.posterLarge || ""
                            posterWidth: Appearance.coverWidth
                            posterRatio: 1
                            focus: ListView.isCurrentItem
                            onClicked: Nav.openItem(modelData.id, "album")
                            Keys.onReturnPressed: Nav.openItem(modelData.id, "album")
                            Keys.onEnterPressed: Nav.openItem(modelData.id, "album")
                        }
                    }
                    }
                }
            }
        }
    }
}
