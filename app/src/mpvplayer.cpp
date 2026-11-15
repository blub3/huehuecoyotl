#include "mpvplayer.h"

#include <QHash>
#include <QLoggingCategory>
#include <QMetaObject>
#include <clocale>
#include <cstring>
#include <mpv/client.h>
#include <mpv/stream_cb.h>
#include <stdexcept>

Q_LOGGING_CATEGORY(lcMpv, "coyotl.mpv")

// Implemented in Rust (crates/stream), linked through the core-api static library.
extern "C" int coyotl_stream_open(void *user_data, char *uri, mpv_stream_cb_info *info);

namespace {

constexpr uint64_t kSubAddReply = 0x5AB; // reply_userdata of sub-add commands

// mpv's %len% quoting, for option values that may contain ',' or '='.
QString quoted(const QString &v)
{
    return QStringLiteral("%%1%%2").arg(v.toUtf8().size()).arg(v);
}

const mpv_node *mapGet(const mpv_node *m, const char *key)
{
    if (!m || m->format != MPV_FORMAT_NODE_MAP) return nullptr;
    for (int i = 0; i < m->u.list->num; ++i)
        if (std::strcmp(m->u.list->keys[i], key) == 0) return &m->u.list->values[i];
    return nullptr;
}
QString str(const mpv_node *n)
{
    return n && n->format == MPV_FORMAT_STRING ? QString::fromUtf8(n->u.string) : QString();
}
qint64 num(const mpv_node *n)
{
    return n && n->format == MPV_FORMAT_INT64 ? n->u.int64 : 0;
}
double dbl(const mpv_node *n)
{
    if (!n) return 0;
    if (n->format == MPV_FORMAT_DOUBLE) return n->u.double_;
    if (n->format == MPV_FORMAT_INT64) return double(n->u.int64);
    return 0;
}
bool flag(const mpv_node *n)
{
    return n && n->format == MPV_FORMAT_FLAG && n->u.flag;
}

QString languageName(const QString &code)
{
    // Short list for display; mpv already gives ISO 639 codes.
    static const QHash<QString, QString> names = {
        {"eng", "English"},    {"en", "English"},   {"ger", "Deutsch"},   {"deu", "Deutsch"},
        {"de", "Deutsch"},     {"fre", "Français"}, {"fra", "Français"},  {"fr", "Français"},
        {"spa", "Español"},    {"es", "Español"},   {"ita", "Italiano"},  {"it", "Italiano"},
        {"jpn", "日本語"},     {"ja", "日本語"},    {"por", "Português"}, {"rus", "Русский"},
        {"chi", "中文"},       {"zho", "中文"},     {"kor", "한국어"},    {"dut", "Nederlands"},
        {"nld", "Nederlands"}, {"pol", "Polski"},   {"swe", "Svenska"},   {"dan", "Dansk"},
        {"nor", "Norsk"},      {"fin", "Suomi"},    {"tur", "Türkçe"}};
    return names.value(code.toLower(), code);
}

} // namespace

MpvPlayer::MpvPlayer(QObject *parent) : QObject(parent)
{
    std::setlocale(LC_NUMERIC, "C"); // libmpv requires it
    m_mpv = mpv_create();
    if (!m_mpv) throw std::runtime_error("mpv_create failed");

    auto opt = [this](const char *k, const char *v) {
        if (mpv_set_option_string(m_mpv, k, v) < 0) qCWarning(lcMpv) << "option rejected:" << k << v;
    };
    opt("vo", "libmpv");
    opt("hwdec", qEnvironmentVariable("COYOTL_HWDEC", "auto-safe").toUtf8().constData());
    opt("keep-open", "yes");
    opt("idle", "yes");
    opt("input-default-bindings", "no");
    opt("input-vo-keyboard", "no");
    opt("cache", "auto");
    opt("demuxer-max-bytes", "256MiB");
    opt("demuxer-max-back-bytes", "64MiB");
    opt("sub-auto", "no"); // sidecars come from the core's file system layer (SPEC 5.5)
    opt("sub-font-size", "46");
    opt("sub-border-size", "2.5");
    opt("video-sync", "display-resample");
    opt("audio-client-name", "Huehuecoyotl");
    opt("terminal", qEnvironmentVariableIsSet("COYOTL_MPV_LOG") ? "yes" : "no");
    opt("msg-level", qEnvironmentVariable("COYOTL_MPV_LOG", "all=warn").toUtf8().constData());
    for (const QString &kv : qEnvironmentVariable("COYOTL_MPV_OPTS").split(';', Qt::SkipEmptyParts))
        opt(kv.section('=', 0, 0).toUtf8().constData(), kv.section('=', 1).toUtf8().constData());

    if (mpv_initialize(m_mpv) < 0) throw std::runtime_error("mpv_initialize failed");
    // Network sources stream through the core (SPEC 5.3): coyotl://<source id>/<path>.
    if (mpv_stream_cb_add_ro(m_mpv, "coyotl", nullptr, coyotl_stream_open) < 0)
        qCWarning(lcMpv) << "registering the coyotl:// stream protocol failed";
    mpv_request_log_messages(m_mpv, "warn");

    mpv_observe_property(m_mpv, 0, "time-pos", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "duration", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "pause", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "paused-for-cache", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "eof-reached", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "demuxer-cache-duration", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "volume", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "mute", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "speed", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "media-title", MPV_FORMAT_STRING);
    mpv_observe_property(m_mpv, 0, "track-list", MPV_FORMAT_NODE);
    mpv_observe_property(m_mpv, 0, "chapter-list", MPV_FORMAT_NODE);
    mpv_observe_property(m_mpv, 0, "hwdec-current", MPV_FORMAT_STRING);
    mpv_observe_property(m_mpv, 0, "video-params", MPV_FORMAT_NODE);

    mpv_set_wakeup_callback(
        m_mpv,
        [](void *ctx) {
            QMetaObject::invokeMethod(static_cast<MpvPlayer *>(ctx), &MpvPlayer::drainEvents,
                                      Qt::QueuedConnection);
        },
        this);
}

MpvPlayer::~MpvPlayer()
{
    // The render context (owned by MpvVideoItem's renderer) must be gone already.
    mpv_set_wakeup_callback(m_mpv, nullptr, nullptr);
    mpv_terminate_destroy(m_mpv);
}

void MpvPlayer::command(std::initializer_list<const char *> args)
{
    std::vector<const char *> v(args);
    v.push_back(nullptr);
    mpv_command_async(m_mpv, 0, v.data());
}

void MpvPlayer::renderReady()
{
    m_renderReady = true;
    if (m_pending) {
        auto p = std::move(*m_pending);
        m_pending.reset();
        load(p.url, p.opts);
    }
}

void MpvPlayer::setDisplayFps(double fps)
{
    if (fps > 1) mpv_set_property_string(m_mpv, "display-fps-override", QByteArray::number(fps).constData());
}

void MpvPlayer::load(const QString &url, const LoadOptions &o)
{
    m_error.clear();
    m_eof = false;
    m_position = 0;
    m_duration = 0;
    m_fileOpen = false;
    setState(Opening);
    emit positionChanged();
    emit durationChanged();
    if (!m_renderReady) { // vo=libmpv fails without a render context (spike finding)
        m_pending = Pending{url, o};
        return;
    }
    // Per-file options; they reset when the next file loads.
    QStringList opts;
    if (o.startSeconds > 0) opts << QStringLiteral("start=%1").arg(o.startSeconds, 0, 'f', 3);
    if (o.network) opts << "cache=yes"; // mpv does not count stream callbacks as network
    if (!o.audioLangs.isEmpty()) opts << "alang=" + quoted(o.audioLangs);
    if (o.subOff)
        opts << "sid=no";
    else if (!o.subLangs.isEmpty())
        opts << "slang=" + quoted(o.subLangs);
    // Sidecar subtitles are added with sub-add after the file loads, so they carry the
    // language the core parsed from the file name (sub-files cannot).
    m_current = o;
    m_subAddsPending = 0;
    const QByteArray u = url.toUtf8(), joined = opts.join(',').toUtf8();
    command({"loadfile", u.constData(), "replace", "-1", joined.constData()});
    qCInfo(lcMpv) << "load" << url << joined;
}

void MpvPlayer::stop()
{
    command({"stop"});
    m_fileOpen = false;
    setState(Idle);
}

void MpvPlayer::togglePause()
{
    setPaused(!m_paused);
}

void MpvPlayer::setPaused(bool p)
{
    if (m_state == Ended && !p) {
        seek(0, false);
    }
    int f = p;
    mpv_set_property_async(m_mpv, 0, "pause", MPV_FORMAT_FLAG, &f);
}

void MpvPlayer::seek(double s, bool relative)
{
    const QByteArray v = QByteArray::number(s);
    command({"seek", v.constData(), relative ? "relative" : "absolute+exact"});
}

void MpvPlayer::cycleAudio()
{
    command({"cycle", "aid"});
    QMetaObject::invokeMethod(this, &MpvPlayer::userChangedTracks, Qt::QueuedConnection);
}

void MpvPlayer::cycleSubtitle()
{
    command({"cycle", "sid"});
    QMetaObject::invokeMethod(this, &MpvPlayer::userChangedTracks, Qt::QueuedConnection);
}

void MpvPlayer::addChapter(int delta)
{
    const QByteArray v = QByteArray::number(delta);
    command({"add", "chapter", v.constData()});
}

void MpvPlayer::frameStep(bool back)
{
    command({back ? "frame-back-step" : "frame-step"});
}

void MpvPlayer::setVolume(int v)
{
    double d = qBound(0, v, 130);
    mpv_set_property_async(m_mpv, 0, "volume", MPV_FORMAT_DOUBLE, &d);
}

void MpvPlayer::setMuted(bool m)
{
    int f = m;
    mpv_set_property_async(m_mpv, 0, "mute", MPV_FORMAT_FLAG, &f);
}

void MpvPlayer::setSpeed(double s)
{
    double d = qBound(0.25, s, 4.0);
    mpv_set_property_async(m_mpv, 0, "speed", MPV_FORMAT_DOUBLE, &d);
}

void MpvPlayer::setSubtitleLift(int px)
{
    if (px == m_subLift) return;
    m_subLift = px;
    const QByteArray v = QByteArray::number(22 + px); // mpv default sub-margin-y is 22
    mpv_set_property_string(m_mpv, "sub-margin-y", v.constData());
    emit subtitleLiftChanged();
}

void MpvPlayer::setAudioTrack(int id)
{
    const QByteArray v = id > 0 ? QByteArray::number(id) : QByteArray("no");
    mpv_set_property_string(m_mpv, "aid", v.constData());
    emit userChangedTracks();
}

void MpvPlayer::setSubtitleTrack(int id)
{
    const QByteArray v = id > 0 ? QByteArray::number(id) : QByteArray("no");
    mpv_set_property_string(m_mpv, "sid", v.constData());
    emit userChangedTracks();
}

QString MpvPlayer::languageOf(int trackId, const QString &type) const
{
    for (const auto &v : type == "audio" ? m_audio : m_subs) {
        const auto m = v.toMap();
        if (m.value("id").toInt() == trackId) return m.value("lang").toString();
    }
    return {};
}

void MpvPlayer::setState(State s)
{
    if (s == m_state) return;
    m_state = s;
    emit stateChanged();
}

void MpvPlayer::updatePlayState()
{
    if (!m_fileOpen || m_state == Error || m_state == Idle) return;
    if (m_eof)
        setState(Ended);
    else if (m_buffering)
        setState(Buffering);
    else
        setState(m_paused ? Paused : Playing);
}

void MpvPlayer::drainEvents()
{
    while (m_mpv) {
        mpv_event *e = mpv_wait_event(m_mpv, 0);
        switch (e->event_id) {
        case MPV_EVENT_NONE:
            return;
        case MPV_EVENT_SHUTDOWN:
            return;
        case MPV_EVENT_PROPERTY_CHANGE: {
            auto *p = static_cast<mpv_event_property *>(e->data);
            handleProperty(p->name, p->format, p->data);
            break;
        }
        case MPV_EVENT_FILE_LOADED:
            addExternalSubtitles();
            m_fileOpen = true;
            updatePlayState();
            emit fileLoaded();
            break;
        case MPV_EVENT_END_FILE: {
            auto *ef = static_cast<mpv_event_end_file *>(e->data);
            if (ef->reason == MPV_END_FILE_REASON_ERROR) {
                m_error = tr("Cannot play this file: %1").arg(QString::fromUtf8(mpv_error_string(ef->error)));
                m_fileOpen = false;
                setState(Error);
                emit stateChanged();
            }
            break;
        }
        case MPV_EVENT_COMMAND_REPLY:
            if (e->reply_userdata == kSubAddReply && --m_subAddsPending == 0) applySubtitleChoice();
            break;
        case MPV_EVENT_LOG_MESSAGE: {
            auto *m = static_cast<mpv_event_log_message *>(e->data);
            qCWarning(lcMpv).noquote() << m->prefix << QString::fromUtf8(m->text).trimmed();
            break;
        }
        default:
            break;
        }
    }
}

void MpvPlayer::handleProperty(const char *name, int format, void *data)
{
    const QByteArray n(name);
    const bool none = format == MPV_FORMAT_NONE || !data;
    if (n == "time-pos") {
        m_position = none ? 0 : *static_cast<double *>(data);
        if (!none && m_state == Opening) updatePlayState();
        emit positionChanged();
    } else if (n == "duration") {
        m_duration = none ? 0 : *static_cast<double *>(data);
        emit durationChanged();
    } else if (n == "demuxer-cache-duration") {
        m_cache = none ? 0 : *static_cast<double *>(data);
    } else if (n == "pause") {
        m_paused = !none && *static_cast<int *>(data);
        if (!m_paused) m_eof = false;
        updatePlayState();
        emit stateChanged();
    } else if (n == "paused-for-cache") {
        m_buffering = !none && *static_cast<int *>(data);
        updatePlayState();
    } else if (n == "eof-reached") {
        const bool eof = !none && *static_cast<int *>(data);
        if (eof && !m_eof) {
            m_eof = true;
            updatePlayState();
            emit endOfFile();
        } else if (!eof) {
            m_eof = false;
            updatePlayState();
        }
    } else if (n == "volume") {
        m_volume = none ? 100 : int(*static_cast<double *>(data) + 0.5);
        emit volumeChanged();
    } else if (n == "mute") {
        m_muted = !none && *static_cast<int *>(data);
        emit volumeChanged();
    } else if (n == "speed") {
        m_speed = none ? 1 : *static_cast<double *>(data);
        emit speedChanged();
    } else if (n == "media-title") {
        m_mediaTitle = none ? QString() : QString::fromUtf8(*static_cast<char **>(data));
        emit mediaChanged();
    } else if (n == "track-list") {
        parseTracks(none ? nullptr : static_cast<mpv_node *>(data));
    } else if (n == "chapter-list") {
        parseChapters(none ? nullptr : static_cast<mpv_node *>(data));
    } else if (n == "hwdec-current" || n == "video-params") {
        char *hw = mpv_get_property_string(m_mpv, "hwdec-current");
        char *codec = mpv_get_property_string(m_mpv, "video-format");
        char *w = mpv_get_property_string(m_mpv, "video-params/w");
        char *h = mpv_get_property_string(m_mpv, "video-params/h");
        char *gamma = mpv_get_property_string(m_mpv, "video-params/gamma");
        auto s = [](char *c) { return c ? QString::fromUtf8(c) : QString(); };
        m_videoInfo =
            QStringLiteral("%1 %2×%3 %4 · %5")
                .arg(s(codec).toUpper(), s(w), s(h), s(gamma) == "pq" || s(gamma) == "hlg" ? "HDR" : "SDR",
                     s(hw).isEmpty() || s(hw) == "no" ? tr("software decoding") : s(hw));
        for (char *c : {hw, codec, w, h, gamma})
            mpv_free(c);
        emit mediaChanged();
    }
}

void MpvPlayer::addExternalSubtitles()
{
    for (const ExternalSub &s : m_current.subtitles) {
        const QByteArray url = s.url.toUtf8(), lang = s.lang.toUtf8();
        const QByteArray title = s.forced ? tr("Forced").toUtf8() : QByteArray();
        const char *cmd[] = {"sub-add",         url.constData(),  "auto",
                             title.constData(), lang.constData(), nullptr};
        if (mpv_command_async(m_mpv, kSubAddReply, cmd) >= 0) ++m_subAddsPending;
    }
    if (m_subAddsPending == 0) applySubtitleChoice();
}

// The remembered choice wins over mpv's default selection once all sidecars are known.
void MpvPlayer::applySubtitleChoice()
{
    mpv_node list;
    if (mpv_get_property(m_mpv, "track-list", MPV_FORMAT_NODE, &list) >= 0) {
        parseTracks(&list);
        mpv_free_node_contents(&list);
    }
    if (m_current.subOff) {
        mpv_set_property_string(m_mpv, "sid", "no");
        return;
    }
    const QStringList want = m_current.subLangs.split(',', Qt::SkipEmptyParts);
    if (want.isEmpty()) return;
    int best = 0;
    for (const QVariant &v : std::as_const(m_subs)) {
        const QVariantMap t = v.toMap();
        if (!want.contains(t.value("lang").toString().toLower())) continue;
        if (!t.value("forced").toBool()) {
            best = t.value("id").toInt();
            break;
        }
        if (!best) best = t.value("id").toInt();
    }
    if (best) mpv_set_property_string(m_mpv, "sid", QByteArray::number(best).constData());
}

void MpvPlayer::parseTracks(const mpv_node *list)
{
    m_audio.clear();
    m_subs.clear();
    m_aid = m_sid = 0;
    if (list && list->format == MPV_FORMAT_NODE_ARRAY) {
        for (int i = 0; i < list->u.list->num; ++i) {
            const mpv_node *t = &list->u.list->values[i];
            const QString type = str(mapGet(t, "type"));
            const int id = int(num(mapGet(t, "id")));
            const QString lang = str(mapGet(t, "lang"));
            QString title = str(mapGet(t, "title"));
            const bool external = flag(mapGet(t, "external"));
            if (external && title.contains('.')) title.clear(); // mpv's fallback title is the file name
            const QString codec = str(mapGet(t, "codec"));
            QStringList parts;
            if (!lang.isEmpty()) parts << languageName(lang);
            if (!title.isEmpty() && title != lang) parts << title;
            if (type == "audio") {
                const qint64 ch = num(mapGet(t, "demux-channel-count"));
                parts << codec.toUpper() + (ch ? QStringLiteral(" %1ch").arg(ch) : QString());
            } else {
                parts << codec.toUpper();
            }
            if (external) parts << tr("external");
            if (flag(mapGet(t, "forced"))) parts << tr("forced");
            if (parts.isEmpty()) parts << tr("Track %1").arg(id);
            const bool selected = flag(mapGet(t, "selected"));
            QVariantMap m{{"id", id},
                          {"lang", lang},
                          {"label", parts.join(" · ")},
                          {"selected", selected},
                          {"forced", flag(mapGet(t, "forced")) || title == tr("Forced")}};
            if (type == "audio") {
                m_audio.append(m);
                if (selected) m_aid = id;
            } else if (type == "sub") {
                m_subs.append(m);
                if (selected) m_sid = id;
            }
        }
    }
    emit tracksChanged();
}

void MpvPlayer::parseChapters(const mpv_node *list)
{
    m_chapters.clear();
    if (list && list->format == MPV_FORMAT_NODE_ARRAY)
        for (int i = 0; i < list->u.list->num; ++i) {
            const mpv_node *c = &list->u.list->values[i];
            m_chapters.append(
                QVariantMap{{"title", str(mapGet(c, "title"))}, {"time", dbl(mapGet(c, "time"))}});
        }
    emit mediaChanged();
}

QString MpvPlayer::mediaInfoJson() const
{
    // mpv formats node properties (track-list, video-params) as JSON strings.
    QStringList parts;
    for (const char *name : {"track-list", "video-params", "file-format", "duration", "file-size"}) {
        char *v = m_mpv ? mpv_get_property_string(m_mpv, name) : nullptr;
        if (!v) continue;
        const QString raw = QString::fromUtf8(v);
        mpv_free(v);
        const bool json = raw.startsWith('[') || raw.startsWith('{');
        const bool number = !json && !raw.isEmpty() && (raw.at(0).isDigit());
        parts << QStringLiteral("\"%1\": %2")
                     .arg(QLatin1String(name),
                          json || number ? raw
                                         : QStringLiteral("\"%1\"").arg(QString(raw).replace('"', "'")));
    }
    return QStringLiteral("{%1}").arg(parts.join(QStringLiteral(", ")));
}

QString MpvPlayer::versionInfo() const
{
    QStringList out;
    for (const char *name : {"mpv-version", "ffmpeg-version", "libass-version", "hwdec", "hwdec-current",
                             "current-vo", "current-ao", "display-fps-override"}) {
        char *v = m_mpv ? mpv_get_property_string(m_mpv, name) : nullptr;
        out << QStringLiteral("%1 %2").arg(QLatin1String(name),
                                           v ? QString::fromUtf8(v) : QStringLiteral("-"));
        mpv_free(v);
    }
    return out.join('\n');
}
