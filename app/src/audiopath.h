#pragma once
// Issue #9 (SPEC 5.7): when the music output is exclusive, and when the signal path is
// bit-perfect. Shared with the tests.
#include <QString>
#include <QVariantMap>

namespace audiopath {

// mpv's sample formats without the planar suffix: "s32p" and "s32" carry the same samples.
inline QString packed(const QString &format)
{
    return format.endsWith('p') ? format.chopped(1) : format;
}

// An ALSA device that is the hardware itself: mpv lists them as front:, hdmi: and iec958: (and
// hw: when named by hand). plughw: converts, dmix: and sysdefault: mix, the rest are plug-ins.
inline bool directAlsa(const QString &device)
{
    for (const char *p : {"alsa/hw:", "alsa/front:", "alsa/hdmi:", "alsa/iec958:"})
        if (device.startsWith(QLatin1String(p))) return true;
    return false;
}

// Exclusive access: requested, and an output that gives it (a direct ALSA hardware device;
// WASAPI; Core Audio). PipeWire and PulseAudio always mix.
inline bool exclusive(bool requested, const QString &ao, const QString &device)
{
    if (!requested) return false;
    if (ao == "alsa") return directAlsa(device);
    return ao == "wasapi" || ao == "coreaudio";
}

// The devices worth offering, from mpv's audio-device-list ({name, description}): the system's
// default, the sound server's outputs (PipeWire, else PulseAudio), direct ALSA hardware, and
// WASAPI or Core Audio devices. Each gets `direct` (exclusive access possible). In the Flatpak
// (`sandboxed`) the hardware is not reachable (no device permission, SPEC 12), so direct ALSA
// devices are left out.
inline QVariantList offered(const QVariantList &all, bool sandboxed = false)
{
    bool pipewire = false;
    for (const QVariant &v : all)
        if (v.toMap().value("name").toString().startsWith("pipewire/")) pipewire = true;
    QVariantList out;
    for (const QVariant &v : all) {
        QVariantMap d = v.toMap();
        const QString n = d.value("name").toString();
        const bool keep = n == "auto" || n.startsWith(pipewire ? "pipewire/" : "pulse/") ||
                          (directAlsa(n) && !sandboxed) || n.startsWith("wasapi/") ||
                          n.startsWith("coreaudio/");
        if (!keep) continue;
        d.insert("direct", directAlsa(n) || n.startsWith("wasapi/") || n.startsWith("coreaudio/"));
        out << d;
    }
    return out;
}

// Bit-perfect: exclusive, no audio filter, and mpv's input and output parameters (samplerate,
// format, channel-count) the same.
inline bool bitPerfect(const QVariantMap &in, const QVariantMap &out, bool exclusive, bool filters)
{
    if (!exclusive || filters || in.isEmpty() || out.isEmpty()) return false;
    return in.value("samplerate").toInt() > 0 &&
           in.value("samplerate").toInt() == out.value("samplerate").toInt() &&
           packed(in.value("format").toString()) == packed(out.value("format").toString()) &&
           in.value("channel-count").toInt() == out.value("channel-count").toInt();
}

} // namespace audiopath
