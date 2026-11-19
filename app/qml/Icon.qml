import QtQuick
import QtQuick.Shapes

// Drawn icons (issues #133, #137): vector paths on a 24 × 24 grid, so their size never depends
// on a font. `name` picks the path; stroked icons use `color` for the line, filled ones fill.
Item {
    id: icon
    property string name: ""
    property color color: Theme.text
    property real size: 24
    property real lineWidth: 2.2
    implicitWidth: size
    implicitHeight: size

    readonly property var paths: ({
        back: { d: "M15 5 L8 12 L15 19", fill: false },
        play: { d: "M8 4.5 L8 19.5 L20 12 Z", fill: true },
        pause: { d: "M6 4 L10 4 L10 20 L6 20 Z M14 4 L18 4 L18 20 L14 20 Z", fill: true },
        previous: { d: "M6 5 L8 5 L8 19 L6 19 Z M20 5 L20 19 L9 12 Z", fill: true },
        next: { d: "M16 5 L18 5 L18 19 L16 19 Z M4 5 L4 19 L15 12 Z", fill: true },
        shuffle: { d: "M16 3 L21 3 L21 8 M4 20 L21 3 M21 16 L21 21 L16 21 M15 15 L21 21 M4 4 L9 9", fill: false },
        repeat: { d: "M17 2 L21 6 L17 10 M3 11 L3 9 Q3 6 6 6 L21 6 M7 22 L3 18 L7 14 M21 13 L21 15 Q21 18 18 18 L3 18", fill: false },
        volume: { d: "M11 5 L6 9 L3 9 L3 15 L6 15 L11 19 Z M15.5 8.5 Q17.5 12 15.5 15.5 M18.5 5.5 Q22.5 12 18.5 18.5", fill: false },
        list: { d: "M4 6 L20 6 M4 12 L20 12 M4 18 L14 18", fill: false },
        skipBack: { d: "M3 12 A9 9 0 1 0 6 5.3 M3 4 L3 9 L8 9", fill: false },
        skipAhead: { d: "M21 12 A9 9 0 1 1 18 5.3 M21 4 L21 9 L16 9", fill: false }
    })
    readonly property var current: paths[name] || ({ d: "", fill: false })

    Shape {
        anchors.centerIn: parent
        width: 24
        height: 24
        scale: icon.size / 24
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeColor: icon.current.fill ? "transparent" : icon.color
            strokeWidth: icon.current.fill ? 0 : icon.lineWidth
            fillColor: icon.current.fill ? icon.color : "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: icon.current.d }
        }
    }
}
