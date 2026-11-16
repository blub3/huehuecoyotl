#pragma once
// Options of the audio-only mpv handle that plays audiobooks (SPEC 5.8, issue #66). Shared with
// the playback test, which plays a two-file book through the same options.
#include <mpv/client.h>

// Returns how many options mpv rejected.
inline int setBookOptions(mpv_handle *mpv)
{
    static const char *const opts[][2] = {
        {"vid", "no"},
        {"vo", "null"},
        {"audio-display", "no"},
        {"idle", "yes"},
        {"keep-open", "no"},
        // A book's files are one timeline: the next file is in the playlist and plays on without
        // a gap.
        {"gapless-audio", "yes"},
        {"prefetch-playlist", "yes"},
        // Speed without a change of pitch.
        {"af", "scaletempo2"},
        {"audio-pitch-correction", "yes"},
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
