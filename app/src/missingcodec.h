#pragma once
#include <QCoreApplication>
#include <QFile>
#include <QHash>
#include <QRegularExpression>
#include <QString>
#include <cstring>
#include <optional>

// Issue #171: mpv found no decoder for a track. The Flatpak uses the KDE runtime's FFmpeg, which
// has no H.264, HEVC, VC-1 or E-AC-3 decoder without Flathub's codecs-extra extension. mpv only
// says so in its log ("Failed to initialize a decoder for codec 'h264'."), so all three players
// read that line and turn it into a notice.
namespace MissingCodec {

struct Hit {
    bool video = false;
    QString codec; // the name for people: "H.264"
};

// The codec's usual name.
inline QString name(const QString &codec)
{
    static const QHash<QString, QString> names{
        {"h264", "H.264"},          {"hevc", "HEVC (H.265)"}, {"vc1", "VC-1"},  {"wmv3", "VC-1 (WMV3)"},
        {"eac3", "E-AC-3"},         {"ac3", "AC-3"},          {"aac", "AAC"},   {"mp3", "MP3"},
        {"mpeg2video", "MPEG-2"},   {"mpeg4", "MPEG-4"},      {"av1", "AV1"},   {"vp9", "VP9"},
        {"truehd", "Dolby TrueHD"}, {"dts", "DTS"},           {"flac", "FLAC"}, {"opus", "Opus"},
    };
    return names.value(codec, codec.toUpper());
}

// A line of mpv's log, from its video ("vd") or audio ("ad") decoder.
inline std::optional<Hit> parse(const char *prefix, const QString &text)
{
    const bool video = !std::strcmp(prefix, "vd");
    if (!video && std::strcmp(prefix, "ad")) return std::nullopt;
    static const QRegularExpression line(
        QStringLiteral("Failed to initialize a decoder for codec '([^']+)'"));
    const auto m = line.match(text);
    if (!m.hasMatch()) return std::nullopt;
    return Hit{video, name(m.captured(1))};
}

// The command that adds the codecs; only the Flatpak has this extension.
inline QString command(bool flatpak = QFile::exists(QStringLiteral("/.flatpak-info")))
{
    return flatpak
               ? QStringLiteral("flatpak install flathub org.freedesktop.Platform.codecs-extra//25.08-extra")
               : QString();
}

inline QString noPicture(const QString &codec)
{
    return QCoreApplication::translate("MissingCodec",
                                       "No picture: there is no decoder for %1 on this system. "
                                       "The sound keeps playing.")
        .arg(codec);
}

inline QString noSound(const QString &codec)
{
    return QCoreApplication::translate("MissingCodec", "No sound: there is no decoder for %1 on this system. "
                                                       "The picture keeps playing.")
        .arg(codec);
}

// Nothing can play: `codecs` is one name or "HEVC (H.265), E-AC-3".
inline QString cannotPlay(const QString &codecs)
{
    return QCoreApplication::translate("MissingCodec",
                                       "Cannot play this file: there is no decoder for %1 on this system.")
        .arg(codecs);
}

inline QString cannotPlayTitled(const QString &title, const QString &codec)
{
    return QCoreApplication::translate("MissingCodec",
                                       "Cannot play “%1”: there is no decoder for %2 on this system.")
        .arg(title, codec);
}

// A message with how to fix it, for the places that show one text.
inline QString withFix(const QString &message, bool flatpak = QFile::exists(QStringLiteral("/.flatpak-info")))
{
    const QString cmd = command(flatpak);
    if (cmd.isEmpty()) return message;
    return message + '\n' +
           QCoreApplication::translate("MissingCodec",
                                       "Install the codec extension, then start the app again:") +
           '\n' + cmd;
}

} // namespace MissingCodec
