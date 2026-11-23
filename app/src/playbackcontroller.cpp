#include "playbackcontroller.h"
#include "corebridge.h"

#include <QQmlEngine>

PlaybackController::PlaybackController(MpvPlayer *player, QObject *parent) : QObject(parent), m_player(player)
{
    s_instance = this;
    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::playbackReady, this, &PlaybackController::onTicket);
    connect(core, &CoreBridge::requestFailed, this, [this](quint64 id, int, const QString &msg) {
        if (id != m_pendingRequest) return;
        m_pendingRequest = 0;
        m_lastError = msg;
        emit busyChanged();
        emit failed(msg);
    });

    // Progress: every 10 s while playing, and on every state change worth recording.
    m_progressTimer.setInterval(10'000);
    connect(&m_progressTimer, &QTimer::timeout, this, &PlaybackController::report);
    connect(m_player, &MpvPlayer::stateChanged, this, [this] {
        const auto s = m_player->state();
        if (s == MpvPlayer::Playing)
            m_progressTimer.start();
        else
            m_progressTimer.stop();
        if (s != m_lastReported && (s == MpvPlayer::Paused || s == MpvPlayer::Ended)) report();
        if (s == MpvPlayer::Error) {
            m_lastError = m_player->errorString();
            emit failed(m_lastError);
        }
    });
    // Issue #33: once the first frames are out (video-params set), tell the core what mpv sees;
    // it fills in the technical details of files the probe could not analyse.
    connect(m_player, &MpvPlayer::fileLoaded, this, [this] {
        const quint64 ticket = m_ticket.value("ticket").toULongLong();
        QTimer::singleShot(4000, this, [this, ticket] {
            if (ticket != 0 && m_ticket.value("ticket").toULongLong() == ticket)
                CoreBridge::instance()->reportMediaInfo(ticket, m_player->mediaInfoJson());
        });
    });
    connect(m_player, &MpvPlayer::userChangedTracks, this, [this] {
        // Track ids update asynchronously; read the languages once mpv has applied the change.
        QTimer::singleShot(300, this, &PlaybackController::rememberTracks);
    });
}

PlaybackController::~PlaybackController()
{
    if (s_instance == this) s_instance = nullptr;
}

PlaybackController *PlaybackController::create(QQmlEngine *, QJSEngine *)
{
    QQmlEngine::setObjectOwnership(s_instance, QQmlEngine::CppOwnership);
    return s_instance;
}

void PlaybackController::openPath(const QString &path)
{
    m_pendingRequest = CoreBridge::instance()->openPath(path);
    emit busyChanged();
}

void PlaybackController::openItemFile(qint64 itemId, qint64 fileId)
{
    m_pendingRequest = CoreBridge::instance()->openItemFile(itemId, fileId);
    emit busyChanged();
}

void PlaybackController::openItem(qint64 itemId)
{
    m_pendingRequest = CoreBridge::instance()->openItem(itemId);
    emit busyChanged();
}

void PlaybackController::onTicket(quint64 requestId, const QVariantMap &t)
{
    if (requestId != m_pendingRequest) return; // superseded or not ours
    if (!m_ticket.isEmpty()) report();         // switching files: save the old one
    m_pendingRequest = 0;
    m_ticket = t;
    m_lastReported = MpvPlayer::Idle;
    m_resumedFrom = t.value("resumeMs").toDouble() / 1000.0;

    MpvPlayer::LoadOptions o;
    o.startSeconds = m_resumedFrom;
    for (const QVariant &v : t.value("subtitles").toList()) {
        const QVariantMap m = v.toMap();
        o.subtitles.append(
            {m.value("url").toString(), m.value("lang").toString(), m.value("forced").toBool()});
    }
    o.audioLangs = t.value("audioLangs").toString();
    o.subLangs = t.value("subLangs").toString();
    o.subOff = t.value("subOff").toBool();
    o.network = t.value("isNetwork").toBool();
    m_player->load(t.value("url").toString(), o);
    emit busyChanged();
    emit ticketChanged();
    emit started();
}

void PlaybackController::report()
{
    if (m_ticket.isEmpty()) return;
    const auto s = m_player->state();
    if (s == MpvPlayer::Opening || s == MpvPlayer::Error || s == MpvPlayer::Idle) return;
    m_lastReported = s;
    CoreBridge::instance()->reportProgress(m_ticket.value("ticket").toULongLong(),
                                           qint64(m_player->position() * 1000),
                                           qint64(m_player->duration() * 1000), int(s));
}

void PlaybackController::rememberTracks()
{
    if (m_ticket.isEmpty()) return;
    const int sid = m_player->subtitleTrack();
    CoreBridge::instance()->setTrackPref(m_ticket.value("ticket").toULongLong(),
                                         m_player->languageOf(m_player->audioTrack(), "audio"),
                                         sid > 0 ? m_player->languageOf(sid, "sub") : QString(), sid == 0);
}

void PlaybackController::stop()
{
    report();
    m_progressTimer.stop();
    m_player->stop();
    m_ticket.clear();
    emit ticketChanged();
}

void PlaybackController::startOver()
{
    m_player->seek(0, false);
    m_resumedFrom = 0;
    emit ticketChanged();
}
