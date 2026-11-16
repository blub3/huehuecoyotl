pragma Singleton
import QtQuick

// Cover sizes (issue #2): a target poster width per display mode, chosen in Settings and stored
// by Main.qml. Grids derive their columns from it; cards scale their text with it.
QtObject {
    property string coverWindow: "medium"
    property string coverFullscreen: "large"
    property bool fullscreen: false
    // Issue #34, Settings → Playback: which version of a movie is preselected when none was
    // played yet: "quality" (resolution, then HDR, then bit rate) or "smallest" (slow networks).
    property string preferredVersion: "quality"

    readonly property var sizes: [
        { key: "small", label: qsTr("Small"), width: 140 },
        { key: "medium", label: qsTr("Medium"), width: 180 },
        { key: "large", label: qsTr("Large"), width: 260 }
    ]
    function widthOf(key) { return (sizes.find(s => s.key === key) ?? sizes[1]).width }
    readonly property real coverWidth: widthOf(fullscreen ? coverFullscreen : coverWindow)

    // Title text, spacing and gaps grow and shrink with the poster.
    function textScale(posterWidth) { return Math.max(0.85, Math.min(1.35, posterWidth / 180)) }
    // A card: the poster (2:3, or square for music) plus two lines of text.
    function cardHeight(posterWidth, ratio) { return posterWidth * (ratio ?? 1.5) + 54 * textScale(posterWidth) }
}
