pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Persistent navigation (SPEC 9): Home, Movies, Shows, Search, Downloads, Sources, Settings.
FocusScope {
    id: rail
    property string current: "home"
    signal chosen(string section)

    // Move keyboard focus to the rail, starting on the current section.
    function focusCurrent() {
        list.currentIndex = Math.max(0, sections.findIndex(s => s.key === current))
        list.forceActiveFocus()
    }

    implicitWidth: 210
    readonly property var sections: [
        { key: "home", glyph: "⌂", label: qsTr("Home") },
        { key: "movies", glyph: "▶", label: qsTr("Movies") },
        { key: "shows", glyph: "▦", label: qsTr("Shows") },
        { key: "search", glyph: "⌕", label: qsTr("Search") },
        { key: "downloads", glyph: "⤓", label: qsTr("Downloads") },
        { key: "sources", glyph: "▤", label: qsTr("Sources") },
        { key: "settings", glyph: "⚙", label: qsTr("Settings") }
    ]

    Rectangle { anchors.fill: parent; color: Theme.surface }

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: 28
        anchors.bottomMargin: 20
        spacing: 6

        RowLayout {
            Layout.leftMargin: 20
            Layout.bottomMargin: 18
            spacing: 10
            AppLogo {
                size: 34
                Layout.preferredWidth: 34; Layout.preferredHeight: 34
            }
            Label { text: "Huehuecoyotl"; color: Theme.text; font.pixelSize: 17; font.bold: true }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.preferredHeight: contentHeight
            interactive: false
            spacing: 4
            focus: true
            keyNavigationEnabled: true
            model: rail.sections
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Navigation")
            delegate: ItemDelegate {
                id: entry
                required property var modelData
                required property int index
                width: ListView.view.width
                height: 50
                focus: ListView.isCurrentItem
                focusPolicy: Qt.StrongFocus
                Accessible.name: modelData.label
                readonly property bool selected: rail.current === modelData.key
                onClicked: rail.chosen(entry.modelData.key)
                Keys.onReturnPressed: rail.chosen(entry.modelData.key)
                Keys.onEnterPressed: rail.chosen(entry.modelData.key)
                Keys.onRightPressed: rail.chosen(entry.modelData.key)
                background: Rectangle {
                    anchors.fill: parent
                    anchors.leftMargin: 10; anchors.rightMargin: 10
                    radius: Theme.radius
                    color: entry.activeFocus ? Theme.surfaceHi : entry.selected ? Qt.rgba(1, 1, 1, 0.05) : "transparent"
                    border.width: entry.activeFocus ? Theme.focusWidth : 0
                    border.color: Theme.focus
                    Rectangle {
                        visible: entry.selected
                        width: 4; height: 22; radius: 2
                        anchors.verticalCenter: parent.verticalCenter
                        color: Theme.accent
                    }
                }
                contentItem: RowLayout {
                    spacing: 14
                    Label {
                        Layout.leftMargin: 22
                        Layout.preferredWidth: 22
                        text: entry.modelData.glyph
                        color: entry.selected ? Theme.accent : Theme.textDim
                        font.pixelSize: 20
                        horizontalAlignment: Text.AlignHCenter
                    }
                    Label {
                        Layout.fillWidth: true
                        text: entry.modelData.label
                        color: entry.selected || entry.activeFocus ? Theme.text : Theme.textDim
                        font.pixelSize: Theme.fontBody
                    }
                }
            }
        }

        Item { Layout.fillHeight: true }

        // Library activity (scan, metadata): visible while work is queued.
        Label {
            Layout.leftMargin: 22
            Layout.rightMargin: 12
            Layout.fillWidth: true
            visible: Core.scanning || Core.pendingJobs > 0
            text: Core.scanning ? qsTr("Scanning folders…") : qsTr("Updating library… %1").arg(Core.pendingJobs)
            color: Theme.textDim
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
        }
    }
}
