#include "musicplayer.h"
#include "audiopath.h"
#include "corebridge.h"
#include "musicoptions.h"

#include <QLoggingCategory>
#include <QQmlEngine>
#include <clocale>
#include <cstring>
#include <mpv/client.h>
#include <mpv/stream_cb.h>
#include <stdexcept>

Q_LOGGING_CATEGORY(lcMusic, "coyotl.music")

// Implemented in Rust (crates/stream), linked through the core-api static library.
extern "C" int coyotl_stream_open(void *user_data, char *uri, mpv_stream_cb_info *info);

namespace {
QVariant toVariant(const mpv_node *n)
{
    if (!n) return {};
    switch (n->format) {
    case MPV_FORMAT_STRING:
        return QString::fromUtf8(n->u.string);
    case MPV_FORMAT_FLAG:
        return bool(n->u.flag);
    case MPV_FORMAT_INT64:
        return qint64(n->u.int64);
    case MPV_FORMAT_DOUBLE:
        return n->u.double_;
    case MPV_FORMAT_NODE_ARRAY: {
        QVariantList l;
        for (int i = 0; i < n->u.list->num; ++i)
            l << toVariant(&n->u.list->values[i]);
        return l;
    }
    case MPV_FORMAT_NODE_MAP: {
        QVariantMap m;
        for (int i = 0; i < n->u.list->num; ++i)
            m.insert(QString::fromUtf8(n->u.list->keys[i]), toVariant(&n->u.list->values[i]));
        return m;
    }
    default:
        return {};
    }
}

QList<qint64> idList(const QVariantList &items)
{
    QList<qint64> out;
    for (const QVariant &v : items)
        out << v.toLongLong();
    return out;
}
} // namespace

MusicPlayer::MusicPlayer(QObject *parent) : QObject(parent)
{
    s_instance = this;
    std::setlocale(LC_NUMERIC, "C"); // libmpv requires it
    m_mpv = mpv_create();
    if (!m_mpv) throw std::runtime_error("mpv_create failed");
    if (setMusicOptions(m_mpv) > 0) qCWarning(lcMusic) << "some music options were rejected";
    mpv_set_option_string(m_mpv, "terminal", qEnvironmentVariableIsSet("COYOTL_MPV_LOG") ? "yes" : "no");
    mpv_set_option_string(m_mpv, "msg-level",
                          qEnvironmentVariable("COYOTL_MPV_LOG", "all=warn").toUtf8().constData());
    // COYOTL_MUSIC_MPV_OPTS="ao=pcm;ao-pcm-file=/tmp/x.wav" for tests and diagnosis.
    for (const QString &kv : qEnvironmentVariable("COYOTL_MUSIC_MPV_OPTS").split(';', Qt::SkipEmptyParts))
        mpv_set_option_string(m_mpv, kv.section('=', 0, 0).toUtf8().constData(),
                              kv.section('=', 1).toUtf8().constData());
    if (mpv_initialize(m_mpv) < 0) throw std::runtime_error("mpv_initialize failed");
    if (mpv_stream_cb_add_ro(m_mpv, "coyotl", nullptr, coyotl_stream_open) < 0)
        qCWarning(lcMusic) << "registering the coyotl:// stream protocol failed";
    mpv_request_log_messages(m_mpv, "warn");
    mpv_observe_property(m_mpv, 0, "time-pos", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "duration", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "pause", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "paused-for-cache", MPV_FORMAT_FLAG);
    // Issue #9: the signal path and the devices.
    mpv_observe_property(m_mpv, 0, "audio-params", MPV_FORMAT_NODE);
    mpv_observe_property(m_mpv, 0, "audio-out-params", MPV_FORMAT_NODE);
    mpv_observe_property(m_mpv, 0, "af", MPV_FORMAT_NODE);
    mpv_observe_property(m_mpv, 0, "current-ao", MPV_FORMAT_STRING);
    mpv_observe_property(m_mpv, 0, "audio-device-list", MPV_FORMAT_NODE);
    // The volume is not observed: only this class sets it, and mpv's first report (100) could
    // otherwise overwrite the volume restored at start.
    mpv_set_wakeup_callback(
        m_mpv,
        [](void *ctx) {
            QMetaObject::invokeMethod(static_cast<MusicPlayer *>(ctx), &MusicPlayer::drainEvents,
                                      Qt::QueuedConnection);
        },
        this);

    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::musicTickets, this, &MusicPlayer::onTickets);
    connect(core, &CoreBridge::queueChanged, this, &MusicPlayer::onQueue);
    connect(core, &CoreBridge::requestFailed, this, [this](quint64 id, int, const QString &message) {
        if (!m_requests.removeOne(id)) return;
        if (!m_loaded) setState(Idle);
        emit failed(message);
    });
    // Progress every 5 s while playing, for the restart and the play counts.
    m_progress.setInterval(5000);
    connect(&m_progress, &QTimer::timeout, this, &MusicPlayer::report);
    // What was queued before the last quit (shown paused; nothing loads until Play).
    track(core->queueState());
}

MusicPlayer::~MusicPlayer()
{
    if (s_instance == this) s_instance = nullptr;
    mpv_set_wakeup_callback(m_mpv, nullptr, nullptr);
    mpv_terminate_destroy(m_mpv);
}

MusicPlayer *MusicPlayer::create(QQmlEngine *, QJSEngine *)
{
    QQmlEngine::setObjectOwnership(s_instance, QQmlEngine::CppOwnership);
    return s_instance;
}

void MusicPlayer::command(std::initializer_list<const char *> args)
{
    std::vector<const char *> v(args);
    v.push_back(nullptr);
    mpv_command_async(m_mpv, 0, v.data());
}

quint64 MusicPlayer::ticket() const
{
    return m_entries.isEmpty() ? 0 : m_entries.first().value("ticket").toULongLong();
}

void MusicPlayer::setState(int s)
{
    if (s == m_state) return;
    m_state = s;
    if (playing())
        m_progress.start();
    else
        m_progress.stop();
    emit stateChanged();
}

void MusicPlayer::updateState()
{
    if (!m_loaded) return;
    setState(m_buffering ? Buffering : m_pause ? Paused : Playing);
}

void MusicPlayer::setCurrent(const QVariantMap &entry)
{
    if (entry == m_current) return;
    m_current = entry;
    emit currentChanged();
}

// ---- requests ---------------------------------------------------------------------------------

void MusicPlayer::playItems(const QVariantList &items, qint64 startItem, bool shuffle)
{
    track(CoreBridge::instance()->queuePlay(idList(items), startItem, shuffle));
    setState(Opening);
}

void MusicPlayer::playNext(const QVariantList &items)
{
    track(CoreBridge::instance()->queueInsert(idList(items), true));
}

void MusicPlayer::addToQueue(const QVariantList &items)
{
    track(CoreBridge::instance()->queueInsert(idList(items), false));
}

void MusicPlayer::togglePause()
{
    if (m_loaded && !m_pause)
        pause();
    else
        play();
}

void MusicPlayer::play()
{
    if (m_awaitNext) return; // the next track is on its way
    if (!m_loaded) {
        // Restored after a restart: the current entry where it was left. After the end of the
        // queue: what was added since, or the queue from its start.
        if (m_count > 0) {
            track(CoreBridge::instance()->queueResume(m_state == Ended));
            setState(Opening);
        }
        return;
    }
    int no = 0;
    mpv_set_property_async(m_mpv, 0, "pause", MPV_FORMAT_FLAG, &no);
}

void MusicPlayer::pause()
{
    if (!m_loaded) return;
    int yes = 1;
    mpv_set_property_async(m_mpv, 0, "pause", MPV_FORMAT_FLAG, &yes);
    report();
}

void MusicPlayer::next()
{
    track(CoreBridge::instance()->queueStep(1, 0));
}

void MusicPlayer::previous()
{
    track(CoreBridge::instance()->queueStep(-1, qint64(m_position * 1000)));
}

void MusicPlayer::seek(double seconds, bool relative)
{
    if (!m_loaded) return;
    const QByteArray s = QByteArray::number(seconds, 'f', 3);
    command({"seek", s.constData(), relative ? "relative" : "absolute"});
}

void MusicPlayer::jump(int position)
{
    track(CoreBridge::instance()->queueJump(position));
}

void MusicPlayer::remove(int position)
{
    track(CoreBridge::instance()->queueRemove(position));
}

void MusicPlayer::move(int from, int to)
{
    track(CoreBridge::instance()->queueMove(from, to));
}

void MusicPlayer::setShuffle(bool on)
{
    if (on == m_shuffle) return;
    track(CoreBridge::instance()->queueSetMode(on, m_repeat));
}

void MusicPlayer::cycleRepeat()
{
    const QString next = m_repeat == "off" ? "all" : m_repeat == "all" ? "one" : "off";
    track(CoreBridge::instance()->queueSetMode(m_shuffle, next));
}

void MusicPlayer::clear()
{
    track(CoreBridge::instance()->queueClear());
}

void MusicPlayer::setReplayGain(const QString &mode)
{
    const QString m = (mode == "track" || mode == "album") ? mode : QStringLiteral("off");
    if (m == m_replayGain) return;
    m_replayGain = m;
    // Bit-perfect mode keeps ReplayGain off; the choice applies again when it is turned off.
    if (!m_bitPerfect) {
        const QByteArray v = m == "off" ? QByteArray("no") : m.toUtf8();
        mpv_set_property_string(m_mpv, "replaygain", v.constData());
    }
    emit replayGainChanged();
}

// ---- output (issue #9) ------------------------------------------------------------------------

void MusicPlayer::applyOutput()
{
    const QByteArray rg = m_replayGain == "off" ? QByteArray("no") : m_replayGain.toUtf8();
    ::setBitPerfect(m_mpv, m_bitPerfect, rg.constData());
    // Changing audio-device or audio-exclusive makes mpv reopen the output by itself.
    const QByteArray dev = (m_fellBack ? QStringLiteral("auto") : m_device).toUtf8();
    mpv_set_property_string(m_mpv, "audio-device", dev.constData());
    if (!m_bitPerfect) {
        double v = m_volume;
        mpv_set_property(m_mpv, "volume", MPV_FORMAT_DOUBLE, &v);
    }
}

void MusicPlayer::setBitPerfect(bool on)
{
    if (on == m_bitPerfect) return;
    m_bitPerfect = on;
    m_fellBack = false;
    applyOutput();
    emit outputChanged();
    emit volumeChanged();
    updateSignal();
}

void MusicPlayer::setAudioDevice(const QString &name)
{
    const QString n = name.isEmpty() ? QStringLiteral("auto") : name;
    if (n == m_device && !m_fellBack) return;
    m_device = n;
    m_fellBack = false;
    applyOutput();
    emit outputChanged();
    updateSignal();
}

void MusicPlayer::setVolume(int v)
{
    if (m_bitPerfect) return; // fixed at 100 % (SPEC 5.7)
    v = qBound(0, v, 100);
    if (v == m_volume) return;
    m_volume = v;
    double d = v;
    mpv_set_property(m_mpv, "volume", MPV_FORMAT_DOUBLE, &d);
    emit volumeChanged();
}

void MusicPlayer::updateSignal()
{
    const QString device = m_fellBack ? QStringLiteral("auto") : m_device;
    const bool exclusive = audiopath::exclusive(m_bitPerfect, m_ao, device);
    QVariantMap s{
        {"sourceRate", m_in.value("samplerate").toInt()},
        {"sourceFormat", m_in.value("format").toString()},
        {"sourceChannels", m_in.value("channel-count").toInt()},
        {"outRate", m_out.value("samplerate").toInt()},
        {"outFormat", m_out.value("format").toString()},
        {"outChannels", m_out.value("channel-count").toInt()},
        {"ao", m_ao},
        {"device", device},
        {"exclusive", exclusive},
        {"filters", m_filters},
        {"bitPerfect", audiopath::bitPerfect(m_in, m_out, exclusive, m_filters)},
        {"fallback", m_fellBack},
    };
    if (s == m_signal) return;
    m_signal = s;
    emit signalPathChanged();
}

// The chosen device could not be opened (busy, gone, or it refused exclusive access): play on the
// system's default device in shared mode rather than in silence, and say so.
void MusicPlayer::fallBack()
{
    if (m_fellBack || (m_device == "auto" && !m_bitPerfect)) return;
    qCWarning(lcMusic) << "the audio device could not be opened; using the default device";
    m_fellBack = true;
    m_retry = m_loaded;
    mpv_set_property_string(m_mpv, "audio-exclusive", "no");
    mpv_set_property_string(m_mpv, "audio-device", "auto");
    emit outputChanged();
    updateSignal();
}

void MusicPlayer::report()
{
    if (!m_loaded || ticket() == 0) return;
    CoreBridge::instance()->musicProgress(ticket(), qint64(m_position * 1000), qint64(m_duration * 1000));
}

// ---- from the core ----------------------------------------------------------------------------

void MusicPlayer::onQueue(quint64 requestId, const QVariantMap &q)
{
    m_requests.removeOne(requestId);
    m_queue = q.value("entries").toList();
    m_offset = q.value("offset").toInt();
    m_count = q.value("count").toInt();
    m_queuePos = q.value("position").toInt();
    m_shuffle = q.value("shuffle").toBool();
    m_repeat = q.value("repeat").toString();
    // Nothing loaded (after a restart): the queue's current entry is what the player shows.
    if (!m_loaded) {
        const int i = m_queuePos - m_offset;
        setCurrent(i >= 0 && i < m_queue.size() ? m_queue.at(i).toMap() : QVariantMap());
    }
    emit queueChanged();
}

void MusicPlayer::append(const QVariantMap &t, bool replace, double startSeconds)
{
    QVariantMap e = t;
    const QByteArray url = t.value("url").toString().toUtf8();
    QByteArray opts;
    if (startSeconds > 0) opts = "start=" + QByteArray::number(startSeconds, 'f', 3);
    // Network tracks: mpv does not count stream callbacks as network, so ask for the cache.
    if (t.value("isNetwork").toBool()) opts += QByteArray(opts.isEmpty() ? "" : ",") + "cache=yes";
    const char *args[] = {"loadfile", url.constData(),  replace ? "replace" : "append",
                          "-1",       opts.constData(), nullptr};
    mpv_node result;
    // Synchronous, so the playlist is in a known state for the next command.
    if (mpv_command_ret(m_mpv, args, &result) >= 0) {
        if (result.format == MPV_FORMAT_NODE_MAP)
            for (int i = 0; i < result.u.list->num; ++i)
                if (std::strcmp(result.u.list->keys[i], "playlist_entry_id") == 0)
                    e["mpvId"] = qint64(result.u.list->values[i].u.int64);
        mpv_free_node_contents(&result);
    } else {
        qCWarning(lcMusic) << "loadfile failed for ticket" << t.value("ticket");
    }
    m_entries.append(e);
}

void MusicPlayer::onTickets(const QVariantMap &t)
{
    const QString action = t.value("action").toString();
    qCDebug(lcMusic) << "tickets" << action << t.value("current").toMap().value("ticket")
                     << t.value("next").toMap().value("ticket") << "entries" << m_entries.size();
    if (action != "next") m_awaitNext = false;
    if (action == "stop") {
        report();
        const char *args[] = {"stop", nullptr};
        mpv_command(m_mpv, args);
        m_entries.clear();
        m_loaded = false;
        setState(Idle);
        setCurrent({});
        return;
    }
    if (action == "play") {
        report(); // the track that played until now
        m_entries.clear();
        int no = 0;
        mpv_set_property(m_mpv, "pause", MPV_FORMAT_FLAG, &no);
        const QVariantMap current = t.value("current").toMap();
        append(current, true, t.value("startMs").toDouble() / 1000.0);
        if (!t.value("next").toMap().isEmpty()) append(t.value("next").toMap(), false, 0);
        m_loaded = true;
        m_position = 0;
        emit positionChanged();
        setCurrent(current.value("entry").toMap());
        setState(Opening);
        return;
    }
    // "next" after the current track ended already: it becomes the current one.
    if (m_awaitNext) {
        m_awaitNext = false;
        const QVariantMap n = t.value("next").toMap();
        if (n.isEmpty()) {
            setState(Ended);
            return;
        }
        append(n, true, 0);
        m_loaded = true;
        setCurrent(n.value("entry").toMap());
        setState(Opening);
        CoreBridge::instance()->queueAdvanced(n.value("ticket").toULongLong());
        return;
    }
    // "next": replace what follows the current entry.
    if (m_entries.size() > 1) {
        const char *args[] = {"playlist-remove", "1", nullptr};
        mpv_command(m_mpv, args);
        m_entries.resize(1);
    }
    if (m_loaded && !t.value("next").toMap().isEmpty()) append(t.value("next").toMap(), false, 0);
}

// ---- mpv events -------------------------------------------------------------------------------

void MusicPlayer::drainEvents()
{
    while (m_mpv) {
        mpv_event *e = mpv_wait_event(m_mpv, 0);
        switch (e->event_id) {
        case MPV_EVENT_NONE:
        case MPV_EVENT_SHUTDOWN:
            return;
        case MPV_EVENT_PROPERTY_CHANGE: {
            auto *p = static_cast<mpv_event_property *>(e->data);
            const bool none = p->format == MPV_FORMAT_NONE || !p->data;
            if (std::strcmp(p->name, "time-pos") == 0) {
                m_position = none ? 0 : *static_cast<double *>(p->data);
                if (!none && m_state == Opening) updateState();
                emit positionChanged();
            } else if (std::strcmp(p->name, "duration") == 0) {
                m_duration = none ? 0 : *static_cast<double *>(p->data);
                emit durationChanged();
            } else if (std::strcmp(p->name, "pause") == 0) {
                m_pause = !none && *static_cast<int *>(p->data);
                updateState();
            } else if (std::strcmp(p->name, "paused-for-cache") == 0) {
                m_buffering = !none && *static_cast<int *>(p->data);
                updateState();
            } else if (std::strcmp(p->name, "audio-params") == 0) {
                m_in = none ? QVariantMap() : toVariant(static_cast<mpv_node *>(p->data)).toMap();
                updateSignal();
            } else if (std::strcmp(p->name, "audio-out-params") == 0) {
                m_out = none ? QVariantMap() : toVariant(static_cast<mpv_node *>(p->data)).toMap();
                updateSignal();
            } else if (std::strcmp(p->name, "af") == 0) {
                m_filters = !none && !toVariant(static_cast<mpv_node *>(p->data)).toList().isEmpty();
                updateSignal();
            } else if (std::strcmp(p->name, "current-ao") == 0) {
                m_ao = none ? QString() : QString::fromUtf8(*static_cast<char **>(p->data));
                updateSignal();
            } else if (std::strcmp(p->name, "audio-device-list") == 0) {
                m_devices = none ? QVariantList()
                                 : audiopath::offered(toVariant(static_cast<mpv_node *>(p->data)).toList(),
                                                      qEnvironmentVariableIsSet("FLATPAK_ID"));
                emit devicesChanged();
            }
            break;
        }
        case MPV_EVENT_START_FILE: {
            const qint64 id = static_cast<mpv_event_start_file *>(e->data)->playlist_entry_id;
            qCDebug(lcMusic) << "start file" << id << "entries"
                             << (m_entries.isEmpty() ? -1 : m_entries.first().value("mpvId").toLongLong())
                             << (m_entries.size() > 1 ? m_entries.at(1).value("mpvId").toLongLong() : -1);
            // mpv went on to the following entry by itself: drop the finished one, tell the core.
            if (m_entries.size() > 1 && m_entries.at(1).value("mpvId").toLongLong() == id) {
                m_entries.removeFirst();
                command({"playlist-remove", "0"});
                setCurrent(m_entries.first().value("entry").toMap());
                CoreBridge::instance()->queueAdvanced(ticket());
            }
            break;
        }
        case MPV_EVENT_FILE_LOADED:
            updateState();
            break;
        case MPV_EVENT_END_FILE: {
            auto *ef = static_cast<mpv_event_end_file *>(e->data);
            const qint64 id = ef->playlist_entry_id;
            qCDebug(lcMusic) << "end file" << id << "reason" << ef->reason << "entries" << m_entries.size();
            if (m_entries.isEmpty() || m_entries.first().value("mpvId").toLongLong() != id) break;
            if (ef->reason == MPV_END_FILE_REASON_EOF) {
                // Played to the end: counts, and the next start is from the beginning.
                CoreBridge::instance()->musicProgress(ticket(), qint64(m_duration * 1000),
                                                      qint64(m_duration * 1000));
                if (m_entries.size() == 1) {
                    // Nothing follows in mpv: the end of the queue, or the core's ticket for the
                    // next track is still on its way (a very short track); then wait for it.
                    const bool more = m_queuePos + 1 < m_count || m_repeat != "off";
                    m_loaded = false;
                    m_entries.clear();
                    m_awaitNext = more;
                    setState(more ? Buffering : Ended);
                }
            } else if (ef->reason == MPV_END_FILE_REASON_ERROR && m_retry) {
                // The device could not be opened for it: start it again on the default device.
                m_retry = false;
                m_loaded = false;
                m_entries.clear();
                const char *args[] = {"stop", nullptr};
                mpv_command(m_mpv, args);
                track(CoreBridge::instance()->queueResume(false));
                setState(Opening);
            } else if (ef->reason == MPV_END_FILE_REASON_ERROR) {
                const QString title = m_current.value("title").toString();
                emit failed(
                    tr("Cannot play “%1”: %2").arg(title, QString::fromUtf8(mpv_error_string(ef->error))));
                m_loaded = false;
                m_entries.clear();
                setState(Error);
            }
            break;
        }
        case MPV_EVENT_LOG_MESSAGE: {
            auto *m = static_cast<mpv_event_log_message *>(e->data);
            const QString text = QString::fromUtf8(m->text).trimmed();
            qCWarning(lcMusic).noquote() << m->prefix << text;
            // Only the final failure: with the automatic choice mpv tries several drivers.
            if (text.contains("Could not open/initialize audio device")) fallBack();
            break;
        }
        default:
            break;
        }
    }
}
