// Labels from a file's probe data (issue #33; crates/probe, ProbeInfo): the summary line and
// badges on the detail page, the rows of the technical details panel, and the short labels of
// the version selector (issue #34). Values the probe did not find stay out; nothing is guessed.
.pragma library

function resolution(v) {
    if (!v) return ""
    const w = v.width, h = v.height
    if (w >= 3200 || h >= 2000) return "4K"
    if (w >= 1800 || h >= 1000) return "1080p"
    if (w >= 1200 || h >= 700) return "720p"
    return h > 0 ? h + "p" : ""
}

function hdr(v) {
    if (!v || !v.hdr) return ""
    return v.hdr === "Dolby Vision" && v.dv_profile ? "Dolby Vision " + v.dv_profile : v.hdr
}

function videoCodec(v) {
    if (!v) return ""
    const names = { hevc: "HEVC", h264: "H.264", av1: "AV1", vp9: "VP9", vp8: "VP8",
                    mpeg2video: "MPEG-2", mpeg4: "MPEG-4", vc1: "VC-1", prores: "ProRes" }
    const name = names[v.codec] || (v.codec || "").toUpperCase()
    return v.bit_depth > 8 ? name + " " + v.bit_depth + "-bit" : name
}

// "TrueHD Atmos", "DTS-HD MA", "DTS:X", "E-AC-3 Atmos": extensions only when FFmpeg signalled them.
function audioFormat(a) {
    if (!a) return ""
    const p = a.profile || ""
    const names = { truehd: "TrueHD", eac3: "E-AC-3", ac3: "AC-3", dts: "DTS", aac: "AAC", flac: "FLAC",
                    opus: "Opus", mp3: "MP3", mp2: "MP2", vorbis: "Vorbis", alac: "ALAC" }
    let name = names[a.codec] || (a.codec || "").toUpperCase()
    if (a.codec === "dts") {
        if (p.indexOf("DTS:X") >= 0) return "DTS:X"
        if (p.indexOf("HD MA") >= 0) name = "DTS-HD MA"
        else if (p.indexOf("HD HRA") >= 0) name = "DTS-HD HRA"
    } else if (a.codec && a.codec.indexOf("pcm") === 0) {
        name = "PCM"
    }
    if (p.indexOf("Atmos") >= 0) name += " Atmos"
    return name
}

function mainAudio(info) {
    if (!info || !info.audio || info.audio.length === 0) return null
    for (const a of info.audio) if (a.default) return a
    return info.audio[0]
}

function size(bytes) {
    if (bytes === 0) return "0 MB"
    if (!bytes) return ""
    const gb = bytes / 1073741824
    if (gb >= 10) return Math.round(gb) + " GB"
    if (gb >= 1) return gb.toFixed(1) + " GB"
    return Math.round(bytes / 1048576) + " MB"
}

function bitrate(b) {
    if (!b || !b.bps) return ""
    const v = b.bps >= 1000000 ? (b.bps / 1000000).toFixed(1) + " Mbit/s" : Math.round(b.bps / 1000) + " kbit/s"
    return b.estimated ? qsTr("approx. %1").arg(v) : v
}

function duration(ms) {
    if (!ms) return ""
    const s = Math.round(ms / 1000), h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60)
    if (s < 60) return qsTr("%1 s").arg(s)
    return h > 0 ? qsTr("%1 h %2 min").arg(h).arg(m) : qsTr("%1 min").arg(m)
}

function language(code) {
    if (!code) return ""
    const names = { eng: qsTr("English"), ger: qsTr("German"), deu: qsTr("German"), fre: qsTr("French"),
                    fra: qsTr("French"), spa: qsTr("Spanish"), ita: qsTr("Italian"), jpn: qsTr("Japanese"),
                    dut: qsTr("Dutch"), nld: qsTr("Dutch"), por: qsTr("Portuguese"), rus: qsTr("Russian"),
                    pol: qsTr("Polish"), swe: qsTr("Swedish"), dan: qsTr("Danish"), nor: qsTr("Norwegian"),
                    fin: qsTr("Finnish"), kor: qsTr("Korean"), chi: qsTr("Chinese"), zho: qsTr("Chinese"),
                    tur: qsTr("Turkish"), ces: qsTr("Czech"), cze: qsTr("Czech"), hun: qsTr("Hungarian") }
    return names[code] || code
}

// "4K · HDR10 · HEVC 10-bit · TrueHD Atmos 7.1 · 3 subtitles"
function summary(info) {
    if (!info || info.error) return ""
    const v = info.video && info.video.length ? info.video[0] : null
    const a = mainAudio(info)
    const parts = [resolution(v), hdr(v), videoCodec(v)]
    if (a) parts.push((audioFormat(a) + " " + (a.layout || "")).trim())
    const subs = info.subtitles ? info.subtitles.length : 0
    if (subs > 0) parts.push(qsTr("%n subtitle(s)", "", subs))
    return parts.filter(p => p !== "").join("  ·  ")
}

// Small badges: 4K, the HDR format, Atmos or DTS:X.
function badges(info) {
    if (!info || info.error) return []
    const v = info.video && info.video.length ? info.video[0] : null
    const out = []
    if (resolution(v) === "4K") out.push("4K")
    if (v && v.hdr) out.push(v.hdr)
    for (const a of info.audio || []) {
        const p = a.profile || ""
        if (p.indexOf("Atmos") >= 0) { out.push("Atmos"); break }
        if (p.indexOf("DTS:X") >= 0) { out.push("DTS:X"); break }
    }
    return out
}

function flags(t) {
    const f = []
    if (t.default) f.push(qsTr("default"))
    if (t.forced) f.push(qsTr("forced"))
    if (t.sdh) f.push(qsTr("SDH"))
    if (t.external) f.push(qsTr("external file"))
    return f.join(", ")
}

function row(label, value) { return value === "" || value === undefined || value === null ? null : [label, String(value)] }

// The technical details panel: [{ title, rows: [[label, value], ...] }, ...]
function groups(file, info) {
    const out = []
    const fileRows = [
        row(qsTr("File"), (file.path || "").split("/").pop()),
        row(qsTr("Source"), file.sourceName + (file.sourceKind !== "local" ? " (" + file.sourceKind.toUpperCase() + ")" : "")),
        row(qsTr("Path"), file.path),
        row(qsTr("Size"), size(file.size)),
        row(qsTr("Container"), info.container),
        row(qsTr("Duration"), duration(info.duration_ms)),
        row(qsTr("Overall bit rate"), bitrate(info.bitrate)),
        row(qsTr("Chapters"), info.chapters > 0 ? info.chapters : "")
    ].filter(r => r)
    out.push({ title: qsTr("File"), rows: fileRows })
    for (const v of info.video || []) {
        out.push({ title: qsTr("Video"), rows: [
            row(qsTr("Codec"), videoCodec(v) + (v.profile ? ", " + v.profile : "") + (v.level ? ", " + qsTr("level %1").arg(v.level) : "")),
            row(qsTr("Resolution"), v.width && v.height ? v.width + " × " + v.height : ""),
            row(qsTr("Aspect ratio"), v.dar),
            row(qsTr("Frame rate"), v.fps ? v.fps + " fps" : ""),
            row(qsTr("Scan type"), v.scan === "progressive" ? qsTr("Progressive") : v.scan === "interlaced" ? qsTr("Interlaced") : ""),
            row(qsTr("Bit depth"), v.bit_depth ? v.bit_depth + " bit" : ""),
            row(qsTr("Chroma subsampling"), v.chroma),
            row(qsTr("HDR"), hdr(v)),
            row(qsTr("Colour primaries"), v.primaries),
            row(qsTr("Transfer"), v.transfer),
            row(qsTr("Bit rate"), bitrate(v.bitrate))
        ].filter(r => r) })
    }
    (info.audio || []).forEach((a, i) => {
        out.push({ title: qsTr("Audio %1").arg(i + 1), rows: [
            row(qsTr("Language"), language(a.language)),
            row(qsTr("Title"), a.title),
            row(qsTr("Format"), audioFormat(a) + (a.profile && audioFormat(a).indexOf(a.profile) < 0 && a.codec !== "aac" ? " (" + a.profile + ")" : "")),
            row(qsTr("Channels"), a.layout ? a.layout + (a.channels ? " (" + a.channels + ")" : "") : (a.channels || "")),
            row(qsTr("Sample rate"), a.sample_rate ? (a.sample_rate / 1000) + " kHz" : ""),
            row(qsTr("Bit depth"), a.bit_depth ? a.bit_depth + " bit" : ""),
            row(qsTr("Bit rate"), bitrate(a.bitrate)),
            row(qsTr("Flags"), flags(a))
        ].filter(r => r) })
    });
    (info.subtitles || []).forEach((s, i) => {
        out.push({ title: qsTr("Subtitles %1").arg(i + 1), rows: [
            row(qsTr("Language"), language(s.language)),
            row(qsTr("Title"), s.title),
            row(qsTr("Format"), s.format),
            row(qsTr("Flags"), flags(s))
        ].filter(r => r) })
    })
    return out
}

// ---- versions (issue #34) -----------------------------------------------------------------

// Resolution and HDR from the file name, until the file is analysed.
function nameTokens(path) {
    const n = (path || "").split("/").pop().toLowerCase()
    let res = ""
    if (/(2160p|\b4k\b|\buhd\b)/.test(n)) res = "2160p"
    else if (/1080[pi]/.test(n)) res = "1080p"
    else if (/720p/.test(n)) res = "720p"
    else if (/(576p|480p)/.test(n)) res = n.indexOf("576p") >= 0 ? "576p" : "480p"
    let h = ""
    if (/(\bdv\b|\bdovi\b|dolby.?vision)/.test(n)) h = "Dolby Vision"
    else if (/hdr10(\+|plus)/.test(n)) h = "HDR10+"
    else if (/\bhdr(10)?\b/.test(n)) h = "HDR10"
    else if (/\bhlg\b/.test(n)) h = "HLG"
    return { resolution: res, hdr: h }
}

function lines(v) {
    if (!v) return ""
    if (v.width >= 3200 || v.height >= 2000) return "2160p"
    if (v.width >= 1800 || v.height >= 1000) return "1080p"
    if (v.width >= 1200 || v.height >= 700) return "720p"
    return v.height > 0 ? v.height + "p" : ""
}

// "Director's Cut · 2160p · Dolby Vision · HEVC · TrueHD Atmos · 61 GB · NAS"
function versionLabel(file) {
    const info = file.probe && !file.probe.error ? file.probe : null
    const v = info && info.video && info.video.length ? info.video[0] : null
    const names = nameTokens(file.path)
    const a = mainAudio(info)
    const codecs = { hevc: "HEVC", h264: "H.264", av1: "AV1", vp9: "VP9", mpeg2video: "MPEG-2", vc1: "VC-1" }
    const parts = [
        file.edition || "",
        v ? lines(v) : names.resolution,
        v ? (v.hdr || "") : names.hdr,
        v ? (codecs[v.codec] || (v.codec || "").toUpperCase()) : "",
        a ? audioFormat(a) : "",
        size(file.size),
        file.sourceName || ""
    ]
    return parts.filter(p => p !== "").join(" · ")
}

// For "Highest quality": pixels, then HDR, then bit rate (the size when unknown).
function qualityKey(file) {
    const info = file.probe && !file.probe.error ? file.probe : null
    const v = info && info.video && info.video.length ? info.video[0] : null
    const names = nameTokens(file.path)
    const fromName = { "2160p": 3840 * 2160, "1080p": 1920 * 1080, "720p": 1280 * 720, "576p": 720 * 576, "480p": 720 * 480 }
    const px = v ? v.width * v.height : (fromName[names.resolution] || 0)
    const hdr = (v ? !!v.hdr : names.hdr !== "") ? 1 : 0
    const rate = info && info.bitrate ? info.bitrate.bps : 0
    return [px, hdr, rate || file.size || 0]
}

function better(a, b) {
    const ka = qualityKey(a), kb = qualityKey(b)
    for (let i = 0; i < ka.length; i++) if (ka[i] !== kb[i]) return ka[i] > kb[i]
    return false
}

// A version can be played when its source can be reached or a saved copy of it is on this
// computer (issue #48).
function playable(file) { return !!file && (file.online || file.saved === true) }

// The version list's entry: the label, and why an unreachable one is (not) playable.
function versionChoice(file) {
    if (file.online) return versionLabel(file)
    return versionLabel(file) + "  ·  " + (file.saved ? qsTr("offline, plays the saved copy") : qsTr("offline"))
}

// Which version the page opens with: the one played last when it can be played, else the
// best playable one by `preference` ("quality" or "smallest"); unplayable ones only when nothing
// else is left.
function preselect(files, lastFileId, preference) {
    if (!files || files.length === 0) return 0
    const last = files.findIndex(f => f.id === lastFileId && playable(f))
    if (last >= 0) return last
    let best = -1
    files.forEach((f, i) => {
        if (!playable(f)) return
        if (best < 0) { best = i; return }
        const b = files[best]
        const pick = preference === "smallest" ? f.size < b.size : better(f, b)
        if (pick) best = i
    })
    return best >= 0 ? best : 0
}

// Same cut as the item's last played one (durations within 2 %): they share the resume point.
function sameCut(itemMs, file) {
    const ms = file && file.probe && file.probe.duration_ms ? file.probe.duration_ms : 0
    if (!itemMs || !ms) return true
    return Math.abs(itemMs - ms) / Math.max(itemMs, ms) < 0.02
}
