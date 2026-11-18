.pragma library
// Issue #81: the smart playlists' names and rules, by their key in the core.

function name(smart) {
    switch (smart) {
    case "hires": return qsTr("Hi-Res gems")
    case "most_played": return qsTr("Most played")
    case "recently_added": return qsTr("Recently added")
    case "not_played": return qsTr("Not played yet")
    case "saved": return qsTr("Saved offline")
    case "forgotten": return qsTr("Forgotten favourites")
    }
    return smart
}

function rule(smart) {
    switch (smart) {
    case "hires": return qsTr("Lossless above CD quality, best first")
    case "most_played": return qsTr("Your 50 most played songs")
    case "recently_added": return qsTr("Albums added in the last 30 days")
    case "not_played": return qsTr("Songs you have never played")
    case "saved": return qsTr("Everything saved on this computer")
    case "forgotten": return qsTr("Played often, not in the last six months")
    }
    return ""
}

function glyph(smart) {
    switch (smart) {
    case "hires": return "HR"
    case "most_played": return "↑"
    case "recently_added": return "+"
    case "not_played": return "○"
    case "saved": return "↓"
    case "forgotten": return "↺"
    }
    return "♫"
}

// The card's title: an own playlist's name, a smart one's name.
function title(card) {
    if (!card) return ""
    return card.smart ? name(card.smart) : (card.name || "")
}
