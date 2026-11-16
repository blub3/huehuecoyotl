#include "bookplayer.h"
#include "bookoptions.h"

#include <QLoggingCategory>
#include <QQmlEngine>
#include <algorithm>
#include <clocale>
#include <cstring>
#include <mpv/client.h>
#include <stdexcept>

Q_LOGGING_CATEGORY(lcBook, "coyotl.book")

namespace {
// The fade at the end of the sleep timer (SPEC 5.8).
constexpr int FadeSeconds = 5;
constexpr int FadeStepMs = 100;
} // namespace

BookPlayer::BookPlayer(QObject *parent) : QObject(parent)
{
    s_instance = this;
    std::setlocale(LC_NUMERIC, "C"); // libmpv requires it
    m_mpv = mpv_create();
    if (!m_mpv) throw std::runtime_error("mpv_create failed");
    if (setBookOptions(m_mpv) > 0) qCWarning(lcBook) << "some book options were rejected";
    mpv_set_option_string(m_mpv, "terminal", qEnvironmentVariableIsSet("COYOTL_MPV_LOG") ? "yes" : "no");
    mpv_set_option_string(m_mpv, "msg-level",
                          qEnvironmentVariable("COYOTL_MPV_LOG", "all=warn").toUtf8().constData());
    // Tests and diagnosis: the music handle's options apply to books too (ao=null keeps a test
    // silent), then the book's own.
    for (const char *env : {"COYOTL_MUSIC_MPV_OPTS", "COYOTL_BOOK_MPV_OPTS"})
        for (const QString &kv : qEnvironmentVariable(env).split(';', Qt::SkipEmptyParts))
            mpv_set_option_string(m_mpv, kv.section('=', 0, 0).toUtf8().constData(),
                                  kv.section('=', 1).toUtf8().constData());
    if (mpv_initialize(m_mpv) < 0) throw std::runtime_error("mpv_initialize failed");
    if (s_backend.setUp) s_backend.setUp(m_mpv);
    mpv_request_log_messages(m_mpv, "warn");
    mpv_observe_property(m_mpv, 0, "time-pos", MPV_FORMAT_DOUBLE);
    mpv_observe_property(m_mpv, 0, "playlist-pos", MPV_FORMAT_INT64);
    mpv_observe_property(m_mpv, 0, "pause", MPV_FORMAT_FLAG);
    mpv_observe_property(m_mpv, 0, "paused-for-cache", MPV_FORMAT_FLAG);
    mpv_set_wakeup_callback(
        m_mpv,
        [](void *ctx) {
            QMetaObject::invokeMethod(static_cast<BookPlayer *>(ctx), &BookPlayer::drainEvents,
                                      Qt::QueuedConnection);
        },
        this);

    m_progress.setInterval(5000);
    connect(&m_progress, &QTimer::timeout, this, &BookPlayer::report);
    m_sleepTimer.setInterval(1000);
    connect(&m_sleepTimer, &QTimer::timeout, this, &BookPlayer::sleepTick);
    m_fade.setInterval(FadeStepMs);
    connect(&m_fade, &QTimer::timeout, this, [this] {
        // From full volume to silence over FadeSeconds, then stop.
        double v = 0;
        mpv_get_property(m_mpv, "volume", MPV_FORMAT_DOUBLE, &v);
        v -= 100.0 * FadeStepMs / (FadeSeconds * 1000.0);
        if (v <= 0)
            stopForSleep();
        else
            setVolume(v);
    });
}

void BookPlayer::onFailed(quint64 requestId, const QString &message)
{
    if (requestId != m_request || requestId == 0) return;
    m_request = 0;
    if (!m_loaded) setState(Idle);
    emit bookChanged();
    emit failed(message);
}

BookPlayer::~BookPlayer()
{
    if (s_instance == this) s_instance = nullptr;
    mpv_set_wakeup_callback(m_mpv, nullptr, nullptr);
    mpv_terminate_destroy(m_mpv);
}

BookPlayer *BookPlayer::create(QQmlEngine *, QJSEngine *)
{
    QQmlEngine::setObjectOwnership(s_instance, QQmlEngine::CppOwnership);
    return s_instance;
}

void BookPlayer::setVolume(double v)
{
    mpv_set_property(m_mpv, "volume", MPV_FORMAT_DOUBLE, &v);
}

void BookPlayer::setFront(bool on)
{
    if (on == m_front) return;
    m_front = on;
    emit frontChanged();
}

void BookPlayer::setState(int s)
{
    if (s == m_state) return;
    const bool was = playing();
    m_state = s;
    if (playing())
        m_progress.start();
    else
        m_progress.stop();
    if (playing() && !was) {
        setFront(true);
        emit started();
    }
    emit stateChanged();
}

void BookPlayer::updateState()
{
    if (!m_loaded) return;
    setState(m_buffering ? Buffering : m_pause ? Paused : Playing);
}

// ---- the timeline -----------------------------------------------------------------------------

QList<qint64> BookPlayer::starts() const
{
    QList<qint64> out;
    for (const QVariant &c : m_chapters)
        out << c.toMap().value("startMs").toLongLong();
    return out;
}

double BookPlayer::fileStart(int index) const
{
    return m_files.value(index).toMap().value("startMs").toDouble() / 1000.0;
}

int BookPlayer::fileAt(double seconds) const
{
    int i = 0;
    for (int k = 0; k < m_files.size(); ++k)
        if (fileStart(k) <= seconds) i = k;
    return i;
}

void BookPlayer::updatePosition()
{
    const int index = m_base + m_playlistPos;
    const double p = std::clamp(fileStart(index) + m_filePos, 0.0, m_duration);
    if (qFuzzyCompare(p + 1, m_position + 1)) return;
    m_position = p;
    emit positionChanged();
    updateChapter();
    // "End of chapter": fade out over the last seconds before the chapter ends.
    if (m_sleep == "chapter" && !m_fading && m_position >= m_sleepAt - FadeSeconds * m_speed) {
        m_fading = true;
        m_fade.start();
    }
    // The end of the book: finished, and saved as such.
    if (m_position >= m_duration - 0.5 && m_duration > 0 && m_state == Playing &&
        m_base + m_playlistPos == m_files.size() - 1)
        report();
}

void BookPlayer::updateChapter()
{
    const QList<qint64> s = starts();
    const qint64 pos = qint64(m_position * 1000);
    const int i = int(std::upper_bound(s.begin(), s.end(), pos) - s.begin()) - 1;
    if (i == m_chapter) return;
    const bool moved = m_chapter >= 0;
    m_chapter = i;
    emit chapterChanged();
    if (moved && playing()) report();
}

QString BookPlayer::chapterTitle() const
{
    return m_chapters.value(m_chapter).toMap().value("title").toString();
}

double BookPlayer::chapterLeft() const
{
    if (m_chapter < 0) return std::max(0.0, m_duration - m_position);
    const double end = m_chapters.value(m_chapter).toMap().value("endMs").toDouble() / 1000.0;
    return std::max(0.0, end - m_position);
}

// ---- requests ---------------------------------------------------------------------------------

void BookPlayer::playBook(qint64 bookId, qint64 positionMs)
{
    report();
    m_request = s_backend.play ? s_backend.play(bookId, positionMs) : 0;
    setFront(true);
    setState(Opening);
    emit bookChanged();
}

void BookPlayer::onTicket(quint64 requestId, const QString &kind, const QVariantMap &t)
{
    if (requestId != m_request || kind != "bookTicket") return;
    m_request = 0;
    m_book = t;
    m_bookId = t.value("bookId").toLongLong();
    m_chapters = t.value("chapters").toList();
    m_files = t.value("files").toList();
    m_duration = t.value("durationMs").toDouble() / 1000.0;
    const QVariant sp = t.value("speed");
    m_speed = sp.isNull() || sp.toDouble() <= 0 ? m_defaultSpeed : sp.toDouble();
    m_speed = std::clamp(std::round(m_speed * 10) / 10, 0.5, 3.0);
    mpv_set_property(m_mpv, "speed", MPV_FORMAT_DOUBLE, &m_speed);
    m_chapter = -1;
    m_position = t.value("positionMs").toDouble() / 1000.0;
    emit bookChanged();
    emit speedChanged();
    emit positionChanged();
    updateChapter();
    const int index = fileAt(m_position);
    loadFrom(index, m_position - fileStart(index), false);
}

void BookPlayer::loadFrom(int index, double offset, bool paused)
{
    if (m_files.isEmpty()) return;
    index = std::clamp(int(index), 0, int(m_files.size()) - 1);
    int flag = paused ? 1 : 0;
    mpv_set_property(m_mpv, "pause", MPV_FORMAT_FLAG, &flag);
    m_base = index;
    m_playlistPos = 0;
    m_filePos = std::max(0.0, offset);
    m_seeking = true;
    for (int k = index; k < m_files.size(); ++k) {
        const QVariantMap f = m_files.at(k).toMap();
        const QByteArray url = f.value("url").toString().toUtf8();
        QByteArray opts;
        if (k == index && offset > 0) opts = "start=" + QByteArray::number(offset, 'f', 3);
        // Network files: mpv does not count stream callbacks as network, so ask for the cache.
        if (f.value("isNetwork").toBool()) opts += QByteArray(opts.isEmpty() ? "" : ",") + "cache=yes";
        const char *args[] = {"loadfile", url.constData(),  k == index ? "replace" : "append",
                              "-1",       opts.constData(), nullptr};
        if (mpv_command(m_mpv, args) < 0) qCWarning(lcBook) << "loadfile failed for file" << k;
    }
    m_loaded = true;
    if (!paused) setState(Opening);
}

void BookPlayer::togglePause()
{
    if (m_loaded && !m_pause)
        pause();
    else
        play();
}

void BookPlayer::play()
{
    if (!m_loaded) {
        if (m_bookId > 0) playBook(m_bookId, qint64(m_position * 1000));
        return;
    }
    if (m_state == Ended) {
        loadFrom(0, 0, false);
        return;
    }
    int no = 0;
    mpv_set_property_async(m_mpv, 0, "pause", MPV_FORMAT_FLAG, &no);
}

void BookPlayer::pause()
{
    if (!m_loaded) return;
    int yes = 1;
    mpv_set_property_async(m_mpv, 0, "pause", MPV_FORMAT_FLAG, &yes);
    report();
}

void BookPlayer::seekTo(double seconds)
{
    if (m_files.isEmpty()) return;
    seconds = std::clamp(seconds, 0.0, std::max(0.0, m_duration - 0.5));
    const int index = fileAt(seconds);
    const double offset = seconds - fileStart(index);
    m_position = seconds;
    emit positionChanged();
    updateChapter();
    if (!m_loaded) {
        report();
        return;
    }
    if (index == m_base + m_playlistPos) {
        const QByteArray s = QByteArray::number(offset, 'f', 3);
        const char *args[] = {"seek", s.constData(), "absolute", nullptr};
        mpv_command_async(m_mpv, 0, args);
    } else {
        loadFrom(index, offset, m_pause);
    }
    report();
}

void BookPlayer::skip(int direction)
{
    seekTo(m_position + (direction < 0 ? -m_skipBack : m_skipForward));
}

void BookPlayer::previousChapter()
{
    if (s_backend.previousChapter)
        seekTo(s_backend.previousChapter(starts(), qint64(m_position * 1000)) / 1000.0);
}

void BookPlayer::nextChapter()
{
    const qint64 next =
        s_backend.nextChapter ? s_backend.nextChapter(starts(), qint64(m_position * 1000)) : -1;
    if (next >= 0) seekTo(next / 1000.0);
}

void BookPlayer::jumpToChapter(int index)
{
    if (index < 0 || index >= m_chapters.size()) return;
    seekTo(m_chapters.at(index).toMap().value("startMs").toDouble() / 1000.0);
    if (m_loaded && m_pause) play();
}

void BookPlayer::setSpeed(double speed)
{
    speed = std::clamp(std::round(speed * 10) / 10, 0.5, 3.0);
    if (qFuzzyCompare(speed, m_speed)) return;
    m_speed = speed;
    mpv_set_property(m_mpv, "speed", MPV_FORMAT_DOUBLE, &m_speed);
    if (m_bookId > 0 && s_backend.setSpeed) s_backend.setSpeed(m_bookId, m_speed);
    emit speedChanged();
}

void BookPlayer::stop()
{
    report();
    cancelSleep();
    const char *args[] = {"stop", nullptr};
    mpv_command(m_mpv, args);
    m_loaded = false;
    m_bookId = 0;
    m_book.clear();
    m_chapters.clear();
    m_files.clear();
    m_chapter = -1;
    setState(Idle);
    setFront(false);
    emit bookChanged();
}

void BookPlayer::report()
{
    if (m_bookId <= 0) return;
    if (s_backend.progress) s_backend.progress(m_bookId, qint64(m_position * 1000));
}

// ---- sleep timer ------------------------------------------------------------------------------

void BookPlayer::sleepAfter(int minutes)
{
    if (minutes <= 0) {
        cancelSleep();
        return;
    }
    cancelSleep();
    m_sleep = QStringLiteral("time");
    m_sleepLeft = std::clamp(minutes, 5, 120) * 60;
    m_sleepTimer.start();
    emit sleepChanged();
}

void BookPlayer::sleepAtChapterEnd()
{
    cancelSleep();
    m_sleep = QStringLiteral("chapter");
    const qint64 end = s_backend.endOfChapter ? s_backend.endOfChapter(starts(), qint64(m_duration * 1000),
                                                                       qint64(m_position * 1000))
                                              : qint64(m_duration * 1000);
    m_sleepAt = end / 1000.0;
    emit sleepChanged();
}

void BookPlayer::cancelSleep()
{
    m_sleepTimer.stop();
    if (m_fading) {
        m_fade.stop();
        setVolume(100);
    }
    m_fading = false;
    const bool changed = m_sleep != "off";
    m_sleep = QStringLiteral("off");
    m_sleepLeft = 0;
    if (changed) emit sleepChanged();
}

void BookPlayer::sleepTick()
{
    // The timer runs while the book plays; paused time does not count.
    if (!playing()) return;
    m_sleepLeft = std::max(0, m_sleepLeft - 1);
    emit sleepChanged();
    if (m_sleepLeft <= FadeSeconds && !m_fading) {
        m_fading = true;
        m_fade.start();
    }
}

void BookPlayer::stopForSleep()
{
    m_fade.stop();
    m_fading = false;
    int yes = 1;
    mpv_set_property(m_mpv, "pause", MPV_FORMAT_FLAG, &yes);
    setVolume(100);
    m_sleepTimer.stop();
    m_sleep = QStringLiteral("off");
    m_sleepLeft = 0;
    emit sleepChanged();
    report();
}

// ---- mpv events -------------------------------------------------------------------------------

void BookPlayer::drainEvents()
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
                if (none || m_seeking) break;
                m_filePos = *static_cast<double *>(p->data);
                if (m_state == Opening) updateState();
                updatePosition();
            } else if (std::strcmp(p->name, "playlist-pos") == 0) {
                // While a reload is under way, reports may be of the old playlist.
                if (none || m_seeking) break;
                const qint64 pos = *static_cast<int64_t *>(p->data);
                if (pos >= 0 && pos != m_playlistPos) {
                    m_playlistPos = int(pos);
                    m_filePos = 0;
                    updatePosition();
                }
            } else if (std::strcmp(p->name, "pause") == 0) {
                m_pause = !none && *static_cast<int *>(p->data);
                updateState();
            } else if (std::strcmp(p->name, "paused-for-cache") == 0) {
                m_buffering = !none && *static_cast<int *>(p->data);
                updateState();
            }
            break;
        }
        case MPV_EVENT_FILE_LOADED: {
            if (m_seeking) {
                int64_t pos = 0;
                if (mpv_get_property(m_mpv, "playlist-pos", MPV_FORMAT_INT64, &pos) >= 0 && pos >= 0)
                    m_playlistPos = int(pos);
                m_seeking = false;
            }
            updateState();
            break;
        }
        case MPV_EVENT_END_FILE: {
            auto *ef = static_cast<mpv_event_end_file *>(e->data);
            if (ef->reason == MPV_END_FILE_REASON_ERROR) {
                emit failed(tr("Cannot play “%1”: %2")
                                .arg(m_book.value("title").toString(),
                                     QString::fromUtf8(mpv_error_string(ef->error))));
                m_loaded = false;
                setState(Error);
            } else if (ef->reason == MPV_END_FILE_REASON_EOF &&
                       m_base + m_playlistPos >= m_files.size() - 1) {
                // The end of the book.
                m_position = m_duration;
                emit positionChanged();
                report();
                m_loaded = false;
                cancelSleep();
                setState(Ended);
            }
            break;
        }
        case MPV_EVENT_LOG_MESSAGE: {
            auto *m = static_cast<mpv_event_log_message *>(e->data);
            qCWarning(lcBook).noquote() << m->prefix << QString::fromUtf8(m->text).trimmed();
            break;
        }
        default:
            break;
        }
    }
}
