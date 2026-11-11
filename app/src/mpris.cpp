#include "mpris.h"
#include "mpvplayer.h"
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

MprisPlayer::MprisPlayer(QObject *parent, MpvPlayer *player, PlaybackController *playback)
    : QDBusAbstractAdaptor(parent), m_player(player), m_playback(playback)
{
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

QString MprisPlayer::playbackStatus() const
{
    switch (m_player->state()) {
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
    return qlonglong(m_player->position() * 1e6);
}

bool MprisPlayer::canControl() const
{
    const auto s = m_player->state();
    return s != MpvPlayer::Idle && s != MpvPlayer::Error;
}

bool MprisPlayer::canGoNext() const
{
    return canControl() && m_player->chapters().size() > 1;
}

bool MprisPlayer::canGoPrevious() const
{
    return canGoNext();
}

void MprisPlayer::Next()
{
    if (canGoNext()) m_player->addChapter(1);
}

void MprisPlayer::Previous()
{
    if (canGoPrevious()) m_player->addChapter(-1);
}

void MprisPlayer::Pause()
{
    if (canControl()) m_player->setPaused(true);
}

void MprisPlayer::Play()
{
    if (canControl()) m_player->setPaused(false);
}

void MprisPlayer::PlayPause()
{
    if (canControl()) m_player->togglePause();
}

void MprisPlayer::Stop()
{
    if (canControl()) m_playback->requestLeave();
}

void MprisPlayer::Seek(qlonglong offsetUs)
{
    if (canControl()) m_player->seek(offsetUs / 1e6, true);
}

void MprisPlayer::SetPosition(const QDBusObjectPath &id, qlonglong positionUs)
{
    if (!canControl() || id != trackId()) return;
    if (positionUs < 0 || (m_player->duration() > 0 && positionUs > m_player->duration() * 1e6)) return;
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

void registerMpris(QObject *owner, MpvPlayer *player, PlaybackController *playback)
{
    auto bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        qCInfo(lcMpris) << "no session bus; media keys are not available";
        return;
    }
    auto *obj = new QObject(owner);
    new MprisRoot(obj);
    new MprisPlayer(obj, player, playback);
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
