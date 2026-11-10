#include "mpvplayer.h"
#include "coyotl_stream.h"

#include <QDebug>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMetaObject>
#include <QTimer>
#include <clocale>
#include <stdexcept>

static QVariant nodeToVariant(const mpv_node *n)
{
    switch (n->format) {
    case MPV_FORMAT_STRING: return QString::fromUtf8(n->u.string);
    case MPV_FORMAT_FLAG: return bool(n->u.flag);
    case MPV_FORMAT_INT64: return qlonglong(n->u.int64);
    case MPV_FORMAT_DOUBLE: return n->u.double_;
    default: return {};
    }
}

MpvPlayer::MpvPlayer(QObject *parent, quintptr wid) : QObject(parent)
{
    std::setlocale(LC_NUMERIC, "C"); // required by libmpv
    m_mpv = mpv_create();
    if (!m_mpv)
        throw std::runtime_error("mpv_create failed");

    auto opt = [this](const char *k, const char *v) {
        if (mpv_set_option_string(m_mpv, k, v) < 0)
            qWarning() << "mpv option rejected:" << k << v;
    };
    if (wid == 0) {
        opt("vo", "libmpv");
    } else {
        m_renderReady = true;
        opt("wid", QByteArray::number(qulonglong(wid)).constData());
        opt("vo", "gpu-next");
#ifdef Q_OS_WIN
        // HDR passthrough: d3d11 swapchain in the colourspace of the content.
        opt("gpu-api", "d3d11");
        opt("target-colorspace-hint", "yes");
        opt("d3d11-output-format", "auto");
#endif
        // The QML overlay covers the whole video window and owns all input.
        opt("input-vo-keyboard", "no");
        opt("input-cursor", "no");
        opt("osc", "no");
        opt("cursor-autohide", "no");
        opt("video-sync", "display-resample");
    }
    opt("hwdec", qEnvironmentVariable("COYOTL_HWDEC", "auto-safe").toUtf8().constData());
    opt("keep-open", "yes");
    opt("cache", "auto"); // yes for network (incl. our stream_cb protocol), no for local files
    opt("demuxer-max-bytes", "256MiB");
    opt("demuxer-max-back-bytes", "64MiB");
    opt("sub-auto", "exact");
    opt("terminal", "yes");
    opt("msg-level", qEnvironmentVariable("COYOTL_MPV_LOG", "all=warn,vd=v,vo=info,cplayer=info")
                         .toUtf8().constData());
    // Test override, e.g. COYOTL_MPV_OPTS="demuxer-max-bytes=8MiB;hwdec=no"
    for (const QString &kv : qEnvironmentVariable("COYOTL_MPV_OPTS").split(';', Qt::SkipEmptyParts))
        opt(kv.section('=', 0, 0).toUtf8().constData(), kv.section('=', 1).toUtf8().constData());

    if (mpv_initialize(m_mpv) < 0)
        throw std::runtime_error("mpv_initialize failed");

    // Network sources: mpv's demuxer threads call straight into Rust (SPEC 5.3).
    if (mpv_stream_cb_add_ro(m_mpv, "coyotl", nullptr, coyotl_stream_open) < 0)
        qWarning() << "stream_cb registration failed";

    mpv_observe_property(m_mpv, 0, "time-pos", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "duration", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "pause", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "paused-for-cache", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "eof-reached", MPV_FORMAT_FLAG);

    // Wake-up posts to the GUI thread; nothing polls (SPEC 5.1).
    mpv_set_wakeup_callback(m_mpv, [](void *ctx) {
        QMetaObject::invokeMethod(static_cast<MpvPlayer *>(ctx), &MpvPlayer::drainEvents,
                                  Qt::QueuedConnection);
    }, this);

    auto *t = new QTimer(this); // stats overlay only
    connect(t, &QTimer::timeout, this, &MpvPlayer::refreshStats);
    t->start(500);
}

MpvPlayer::~MpvPlayer()
{
    // The render context must already be freed (MpvItem's renderer does that).
    mpv_terminate_destroy(m_mpv);
}

void MpvPlayer::renderReady()
{
    m_renderReady = true;
    if (!m_pendingUrl.isEmpty()) load(m_pendingUrl, m_pendingSub);
}

void MpvPlayer::load(const QString &url, const QString &subFile)
{
    if (!m_renderReady) {
        m_pendingUrl = url;
        m_pendingSub = subFile;
        return;
    }
    // Per-file options. Spike findings:
    //  - stream_cb streams are not flagged as network, so cache=auto leaves the demuxer
    //    cache off for them; force it on for coyotl:// (SPEC 5.4).
    //  - sub-files is a ':'-separated list and splits URLs; -append adds one item.
    QStringList opts;
    if (url.startsWith("coyotl://")) opts << "cache=yes";
    if (!subFile.isEmpty()) opts << "sub-files-append=" + subFile;
    QByteArray u = url.toUtf8(), o = opts.join(',').toUtf8();
    // The index argument exists since mpv 0.38; keep the short form when no options are needed.
    const char *cmd[] = {"loadfile", u.constData(), "replace", "-1", o.constData(), nullptr};
    const char *plain[] = {"loadfile", u.constData(), nullptr};
    mpv_command_async(m_mpv, 0, opts.isEmpty() ? plain : cmd);
    m_state = "Opening";
    emit changed();
}

void MpvPlayer::togglePause()
{
    const char *cmd[] = {"cycle", "pause", nullptr};
    mpv_command_async(m_mpv, 0, cmd);
}

void MpvPlayer::seek(double s, bool relative)
{
    QByteArray v = QByteArray::number(s);
    const char *cmd[] = {"seek", v.constData(), relative ? "relative" : "absolute", nullptr};
    mpv_command_async(m_mpv, 0, cmd);
}

void MpvPlayer::setOption(const QString &name, const QString &value)
{
    mpv_set_property_string(m_mpv, name.toUtf8().constData(), value.toUtf8().constData());
}

QVariant MpvPlayer::property(const QString &name) const
{
    mpv_node n;
    if (mpv_get_property(m_mpv, name.toUtf8().constData(), MPV_FORMAT_NODE, &n) < 0)
        return {};
    QVariant v = nodeToVariant(&n);
    mpv_free_node_contents(&n);
    return v;
}

void MpvPlayer::drainEvents()
{
    bool dirty = false;
    while (true) {
        mpv_event *e = mpv_wait_event(m_mpv, 0);
        if (e->event_id == MPV_EVENT_NONE)
            break;
        switch (e->event_id) {
        case MPV_EVENT_PROPERTY_CHANGE: {
            auto *p = static_cast<mpv_event_property *>(e->data);
            QString name = QString::fromUtf8(p->name);
            if (name == "time-pos" && p->format == MPV_FORMAT_DOUBLE) {
                m_pos = *static_cast<double *>(p->data);
                if (m_state == "Opening") m_state = m_paused ? "Paused" : "Playing";
            } else if (name == "duration" && p->format == MPV_FORMAT_DOUBLE) {
                m_dur = *static_cast<double *>(p->data);
            } else if (name == "pause" && p->format == MPV_FORMAT_FLAG) {
                m_paused = *static_cast<int *>(p->data);
                if (m_state == "Playing" || m_state == "Paused") m_state = m_paused ? "Paused" : "Playing";
            } else if (name == "paused-for-cache" && p->format == MPV_FORMAT_FLAG) {
                if (*static_cast<int *>(p->data)) m_state = "Buffering";
                else if (m_state == "Buffering") m_state = m_paused ? "Paused" : "Playing";
            } else if (name == "eof-reached" && p->format == MPV_FORMAT_FLAG) {
                if (*static_cast<int *>(p->data)) { m_state = "Ended"; emit endReached(); }
            }
            dirty = true;
            break;
        }
        case MPV_EVENT_END_FILE: {
            auto *ef = static_cast<mpv_event_end_file *>(e->data);
            if (ef->reason == MPV_END_FILE_REASON_ERROR) {
                qWarning() << "playback error:" << mpv_error_string(ef->error);
                m_state = "Error";
                dirty = true;
            }
            break;
        }
        case MPV_EVENT_SHUTDOWN:
            return;
        default:
            break;
        }
    }
    if (dirty) emit changed();
}

void MpvPlayer::refreshStats()
{
    auto s = [this](const char *n) { return property(n).toString(); };
    m_stats = QStringLiteral("%1 %2x%3 %4 | hwdec: %5 | vf-fps %6 / display %7 | drops dec %8 vo %9 | cache %10s")
                  .arg(s("video-codec").section(' ', 0, 0), s("width"), s("height"),
                       s("video-params/pixelformat"), s("hwdec-current"),
                       QString::number(property("estimated-vf-fps").toDouble(), 'f', 2),
                       QString::number(property("display-fps").toDouble(), 'f', 2),
                       s("decoder-frame-drop-count"), s("frame-drop-count"),
                       QString::number(property("demuxer-cache-duration").toDouble(), 'f', 1));
    emit changed();
}

QString MpvPlayer::benchReport() const
{
    QJsonObject o;
    for (const char *k : {"path", "video-codec", "width", "height", "video-params/pixelformat",
                          "video-params/gamma", "video-params/primaries", "hwdec-current",
                          "estimated-vf-fps", "container-fps", "display-fps",
                          "frame-drop-count", "decoder-frame-drop-count",
                          "vo-delayed-frame-count", "mistimed-frame-count", "vsync-jitter", "vo-passes/fresh/0/desc",
                          "video-out-params/gamma", "video-out-params/max-luma", "target-colorspace-hint", "mpv-version", "gpu-api",
                          "time-pos", "current-vo", "current-gpu-context", "sub-text",
                          "track-list/count", "demuxer-cache-duration", "cache-speed"})
        o.insert(k, QJsonValue::fromVariant(property(k)));
    return QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact));
}
