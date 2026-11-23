import QtQuick

// Audiobooks (issue #66): Books, Authors, Series and Narrators as tabs over one paged grid.
// Square and portrait covers share the grid: every cell is square and a cover keeps its own
// aspect, standing on the cell's bottom. Without an audiobook folder it explains how to add one.
GridPage {
    kind: "audiobook"
    heading: qsTr("Audiobooks")
    posterRatio: 1
    tabs: [
        { key: "books", label: qsTr("Books"), kind: "audiobook", sort: "title" },
        { key: "authors", label: qsTr("Authors"), kind: "person", sort: "title", filter: "author" },
        { key: "series", label: qsTr("Series"), kind: "book_series", sort: "title" },
        { key: "narrators", label: qsTr("Narrators"), kind: "person", sort: "title", filter: "narrator" }
    ]
    emptyText: qsTr("No audiobooks yet. Add a folder under Sources and choose “Audiobooks” for it.")
}
