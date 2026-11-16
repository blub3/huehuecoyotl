#include "mpris.h"
#include "mpvplayer.h"
#include "musicplayer.h"
#include "playbackcontroller.h"

#include <cmath>

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDateTime>
#include <QGuiApplication>
#include <QLoggingCategory>
#include <QMetaProperty>
#include <QUrl>
#include <QWindow>

Q_LOGGING_CATEGORY(lcMpris, "coyotl.mpris")

namespace {
const char *kPath = "/org/mpris/MediaPlayer2";

QWindow *mainWindow()
{
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow *w : windows)
        if (w->isVisible() && !w->transientParent()) return w;
    return windows.isEmpty() ? nullptr : windows.first();
}
} // namespace

// ---- org.mpris.MediaPlayer2 ---------------------------------------------------------------

MprisRoot::MprisRoot(QObject *parent) : QDBusAbstractAdaptor(parent) {}

QString MprisRoot::identity() const
{
    return QGuiApplication::applicationDisplayName();
}

QString MprisRoot::desktopEntry() const
{
    return QGuiApplication::desktopFileName();
}

QStringList MprisRoot::mimeTypes() const
{
    return {"video/mp4",       "video/x-matroska", "video/webm", "video/quicktime",
            "video/x-msvideo", "video/mp2t",       "video/mpeg", "video/ogg"};
}

bool MprisRoot::fullscreen() const
{
    QWindow *w = mainWindow();
    return w && w->visibility() == QWindow::FullScreen;
}

void MprisRoot::setFullscreen(bool on)
{
    if (QWindow *w = mainWindow()) on ? w->showFullScreen() : w->showNormal();
}

void MprisRoot::Raise()
{
    if (QWindow *w = mainWindow()) {
        w->show();
        w->raise();
        w->requestActivate();
    }
}

void MprisRoot::Quit()
{
    QGuiApplication::quit();
}

// ---- org.mpris.MediaPlayer2.Player --------------------------------------------------------

MprisPlayer::MprisPlayer(QObject *parent, MpvPlayer *player, PlaybackController *playback, MusicPlayer *music)
    : QDBusAbstractAdaptor(parent), m_player(player), m_playback(playback), m_music(music)
{
    connect(music, &MusicPlayer::stateChanged, this, [this] {
        notify(
            {"PlaybackStatus", "CanPlay", "CanPause", "CanSeek", "CanGoNext", "CanGoPrevious", "Metadata"});
    });
    connect(music, &MusicPlayer::currentChanged, this, [this] {
        ++m_track;
        notify({"Metadata", "CanGoNext", "CanGoPrevious"});
    });
    connect(music, &MusicPlayer::durationChanged, this, [this] { notify({"Metadata"}); });
    connect(music, &MusicPlayer::queueChanged, this, [this] { notify({"CanGoNext", "CanGoPrevious"}); });
    connect(player, &MpvPlayer::stateChanged, this,
            [this] { notify({"PlaybackStatus", "CanPlay", "CanPause", "CanSeek"}); });
    connect(player, &MpvPlayer::speedChanged, this, [this] { notify({"Rate"}); });
    connect(player, &MpvPlayer::volumeChanged, this, [this] { notify({"Volume"}); });
    connect(player, &MpvPlayer::durationChanged, this, [this] { notify({"Metadata"}); });
    connect(player, &MpvPlayer::mediaChanged, this,
            [this] { notify({"Metadata", "CanGoNext", "CanGoPrevious"}); });
    connect(playback, &PlaybackController::ticketChanged, this, [this] {
        ++m_track;
        notify({"Metadata"});
    });
    // MPRIS clients interpolate Position themselves and only need a signal on jumps.
    connect(player, &MpvPlayer::positionChanged, this, [this] {
        const double pos = m_player->position();
        const qint64 now = QDateTime::currentMSecsSinceEpoch();
        const double expected =
            m_lastPos + (m_player->paused() ? 0.0 : (now - m_lastPosAt) / 1000.0 * m_player->speed());
        if (m_lastPosAt && canControl() && std::abs(pos - expected) > 1.5) emit Seeked(position());
        m_lastPos = pos;
        m_lastPosAt = now;
    });
}

bool MprisPlayer::music() const
{
    return m_player->state() == MpvPlayer::Idle && m_music->active();
}

double MprisPlayer::pos() const
{
    return music() ? m_music->position() : m_player->position();
}

QString MprisPlayer::playbackStatus() const
{
    switch (music() ? MpvPlayer::State(m_music->state()) : m_player->state()) {
    case MpvPlayer::Playing:
    case MpvPlayer::Buffering:
    case MpvPlayer::Opening:
        return "Playing";
    case MpvPlayer::Paused:
        return "Paused";
    default:
        return "Stopped";
    }
}

double MprisPlayer::rate() const
{
    return m_player->speed();
}

void MprisPlayer::setRate(double r)
{
    if (r <= 0) {
        Pause();
        return;
    }
    m_player->setSpeed(qBound(minimumRate(), r, maximumRate()));
}

QDBusObjectPath MprisPlayer::trackId() const
{
    return QDBusObjectPath(QStringLiteral("/io/github/blub3/Huehuecoyotl/track/%1").arg(m_track));
}

QVariantMap MprisPlayer::metadata() const
{
    if (!canControl()) return {};
    QVariantMap m;
    m.insert("mpris:trackid", QVariant::fromValue(trackId()));
    if (music()) {
        const QVariantMap e = m_music->current();
        m.insert("xesam:title", e.value("title").toString());
        if (!e.value("artists").toString().isEmpty())
            m.insert("xesam:artist", QStringList{e.value("artists").toString()});
        if (!e.value("album").toString().isEmpty()) m.insert("xesam:album", e.value("album").toString());
        const QString art = e.value("posterFull").toString().isEmpty() ? e.value("poster").toString()
                                                                       : e.value("posterFull").toString();
        if (!art.isEmpty()) m.insert("mpris:artUrl", art);
        const double d =
            m_music->duration() > 0 ? m_music->duration() : e.value("durationMs").toDouble() / 1000;
        if (d > 0) m.insert("mpris:length", qlonglong(d * 1e6));
        return m;
    }
    const QString title = m_playback->title().isEmpty() ? m_player->mediaTitle() : m_playback->title();
    if (!title.isEmpty()) m.insert("xesam:title", title);
    if (m_player->duration() > 0) m.insert("mpris:length", qlonglong(m_player->duration() * 1e6));
    return m;
}

double MprisPlayer::volume() const
{
    return m_player->muted() ? 0.0 : m_player->volume() / 100.0;
}

void MprisPlayer::setVolume(double v)
{
    m_player->setVolume(int(qBound(0.0, v, 1.0) * 100 + 0.5));
}

qlonglong MprisPlayer::position() const
{
    return qlonglong(pos() * 1e6);
}

bool MprisPlayer::canControl() const
{
    if (music()) return true; // a queue: Play loads its current entry
    const auto s = m_player->state();
    return s != MpvPlayer::Idle && s != MpvPlayer::Error;
}

bool MprisPlayer::canGoNext() const
{
    if (music()) return m_music->queueCount() > 1 || m_music->repeat() != "off";
    return canControl() && m_player->chapters().size() > 1;
}

bool MprisPlayer::canGoPrevious() const
{
    if (music()) return true; // restarts the track at least
    return canGoNext();
}

void MprisPlayer::Next()
{
    if (music())
        m_music->next();
    else if (canGoNext())
        m_player->addChapter(1);
}

void MprisPlayer::Previous()
{
    if (music())
        m_music->previous();
    else if (canGoPrevious())
        m_player->addChapter(-1);
}

void MprisPlayer::Pause()
{
    if (music())
        m_music->pause();
    else if (canControl())
        m_player->setPaused(true);
}

void MprisPlayer::Play()
{
    if (music())
        m_music->play();
    else if (canControl())
        m_player->setPaused(false);
}

void MprisPlayer::PlayPause()
{
    if (music())
        m_music->togglePause();
    else if (canControl())
        m_player->togglePause();
}

void MprisPlayer::Stop()
{
    if (music())
        m_music->pause();
    else if (canControl())
        m_playback->requestLeave();
}

void MprisPlayer::Seek(qlonglong offsetUs)
{
    if (music())
        m_music->seek(offsetUs / 1e6, true);
    else if (canControl())
        m_player->seek(offsetUs / 1e6, true);
}

void MprisPlayer::SetPosition(const QDBusObjectPath &id, qlonglong positionUs)
{
    if (!canControl() || id != trackId()) return;
    const double d = music() ? m_music->duration() : m_player->duration();
    if (positionUs < 0 || (d > 0 && positionUs > d * 1e6)) return;
    if (music())
        m_music->seek(positionUs / 1e6, false);
    else
        m_player->seek(positionUs / 1e6, false);
}

void MprisPlayer::OpenUri(const QString &uri)
{
    const QUrl url(uri);
    if (url.isLocalFile()) m_playback->openPath(url.toLocalFile());
}

void MprisPlayer::notify(const QStringList &names)
{
    QVariantMap changed;
    for (const QString &n : names) {
        const int idx = metaObject()->indexOfProperty(n.toLatin1().constData());
        if (idx >= 0) changed.insert(n, metaObject()->property(idx).read(this));
    }
    auto msg = QDBusMessage::createSignal(kPath, "org.freedesktop.DBus.Properties", "PropertiesChanged");
    msg << QStringLiteral("org.mpris.MediaPlayer2.Player") << changed << QStringList();
    QDBusConnection::sessionBus().send(msg);
}

void registerMpris(QObject *owner, MpvPlayer *player, PlaybackController *playback, MusicPlayer *music)
{
    auto bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        qCInfo(lcMpris) << "no session bus; media keys are not available";
        return;
    }
    auto *obj = new QObject(owner);
    new MprisRoot(obj);
    new MprisPlayer(obj, player, playback, music);
    if (!bus.registerObject(kPath, obj, QDBusConnection::ExportAdaptors)) {
        qCWarning(lcMpris) << "cannot register" << kPath << bus.lastError().message();
        return;
    }
    // A second instance gets a unique suffix, as the MPRIS specification asks.
    QString name = QStringLiteral("org.mpris.MediaPlayer2.coyotl");
    if (!bus.registerService(name)) {
        name += QStringLiteral(".instance%1").arg(QCoreApplication::applicationPid());
        if (!bus.registerService(name)) qCWarning(lcMpris) << "cannot register" << name;
    }
    qCInfo(lcMpris) << "registered" << name;
}
