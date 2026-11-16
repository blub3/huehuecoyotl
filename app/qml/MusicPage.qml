import QtQuick

// Music (issue #10): Albums, Artists and Recently added, as tabs over one paged grid of square
// covers. Without a music folder it explains how to add one.
GridPage {
    kind: "album"
    heading: qsTr("Music")
    posterRatio: 1
    tabs: [
        { key: "albums", label: qsTr("Albums"), kind: "album", sort: "title" },
        { key: "artists", label: qsTr("Artists"), kind: "artist", sort: "title" },
        { key: "recent", label: qsTr("Recently added"), kind: "album", sort: "added" }
    ]
    emptyText: qsTr("No music yet. Add a folder under Sources and choose “Music” for it.")
}
