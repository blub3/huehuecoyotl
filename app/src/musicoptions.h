#pragma once
// Options of the audio-only mpv handle that plays music (SPEC 5.7, issue #8). Shared with the
// gapless test, which plays through the same options into mpv's file output.
#include <mpv/client.h>

// Returns how many options mpv rejected.
inline int setMusicOptions(mpv_handle *mpv)
{
    static const char *const opts[][2] = {
        {"vid", "no"},
        {"vo", "null"},
        {"audio-display", "no"},
        {"idle", "yes"},
        {"keep-open", "no"},
        // The following entry is in mpv's playlist already: same-format tracks play without a gap.
        {"gapless-audio", "weak"},
        {"prefetch-playlist", "yes"},
        // Smaller than for video, so the next track can be fetched while this one plays.
        {"cache", "auto"},
        {"demuxer-max-bytes", "32MiB"},
        {"demuxer-max-back-bytes", "8MiB"},
        {"input-default-bindings", "no"},
        {"input-vo-keyboard", "no"},
        {"audio-client-name", "Huehuecoyotl"},
        {"replaygain", "no"},
    };
    int rejected = 0;
    for (const auto &o : opts)
        if (mpv_set_option_string(mpv, o[0], o[1]) < 0) ++rejected;
    return rejected;
}

// Issue #9: bit-perfect mode on the running handle: exclusive access where the output offers it,
// no audio filters, ReplayGain off and the software volume at 100 %. Off: shared access and the
// ReplayGain choice ("no", "track" or "album"). mpv already forces no sample rate or format, and
// with gapless-audio=weak it reopens the device when the format changes between tracks.
inline void setBitPerfect(mpv_handle *mpv, bool on, const char *replayGain)
{
    mpv_set_property_string(mpv, "audio-exclusive", on ? "yes" : "no");
    if (on) {
        mpv_set_property_string(mpv, "af", "");
        mpv_set_property_string(mpv, "replaygain", "no");
        mpv_set_property_string(mpv, "volume", "100");
    } else {
        mpv_set_property_string(mpv, "replaygain", replayGain);
    }
}
