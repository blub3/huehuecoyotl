pragma Singleton
import QtQuick

// Custom dark theme on the Basic style, so Windows and Linux look the same (SPEC 9).
QtObject {
    readonly property color bg: "#0d1015"
    readonly property color surface: "#161a21"
    readonly property color surfaceHi: "#202632"
    readonly property color text: "#eceef1"
    readonly property color textDim: "#9aa3b0"
    readonly property color accent: "#e3a33b"      // coyote amber
    readonly property color focus: "#ffd27d"
    readonly property color danger: "#e5534b"
    readonly property color scrim: "#c0080a0e"

    readonly property int pad: 16
    readonly property int radius: 10
    readonly property int focusWidth: 3
    readonly property int fontSmall: 13
    readonly property int fontBody: 16
    readonly property int fontLarge: 20
    readonly property int fontTitle: 30

    function time(seconds) {
        if (!isFinite(seconds) || seconds < 0) seconds = 0
        const s = Math.floor(seconds % 60), m = Math.floor(seconds / 60) % 60, h = Math.floor(seconds / 3600)
        const pad2 = n => (n < 10 ? "0" : "") + n
        return h > 0 ? h + ":" + pad2(m) + ":" + pad2(s) : m + ":" + pad2(s)
    }
}
