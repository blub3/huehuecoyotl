#pragma once
// MPRIS 2 on the session bus (SPEC 9: media keys on Linux). Desktop shells, media keys and
// widgets such as KDE Connect find the player as org.mpris.MediaPlayer2.coyotl.
#include <QDBusAbstractAdaptor>
#include <QDBusObjectPath>
#include <QStringList>
#include <QVariantMap>

class MpvPlayer;
class PlaybackController;

class MprisRoot : public QDBusAbstractAdaptor {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.mpris.MediaPlayer2")
    Q_PROPERTY(bool CanQuit READ canQuit CONSTANT)
    Q_PROPERTY(bool CanRaise READ canRaise CONSTANT)
    Q_PROPERTY(bool HasTrackList READ hasTrackList CONSTANT)
    Q_PROPERTY(QString Identity READ identity CONSTANT)
    Q_PROPERTY(QString DesktopEntry READ desktopEntry CONSTANT)
    Q_PROPERTY(QStringList SupportedUriSchemes READ uriSchemes CONSTANT)
    Q_PROPERTY(QStringList SupportedMimeTypes READ mimeTypes CONSTANT)
    Q_PROPERTY(bool Fullscreen READ fullscreen WRITE setFullscreen)
    Q_PROPERTY(bool CanSetFullscreen READ canSetFullscreen CONSTANT)

  public:
    explicit MprisRoot(QObject *parent);
    bool canQuit() const { return true; }
    bool canRaise() const { return true; }
    bool hasTrackList() const { return false; }
    QString identity() const;
    QString desktopEntry() const;
    QStringList uriSchemes() const { return {"file"}; }
    QStringList mimeTypes() const;
    bool fullscreen() const;
    void setFullscreen(bool on);
    bool canSetFullscreen() const { return true; }

  public slots:
    void Raise();
    void Quit();
};

class MprisPlayer : public QDBusAbstractAdaptor {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.mpris.MediaPlayer2.Player")
    Q_PROPERTY(QString PlaybackStatus READ playbackStatus)
    Q_PROPERTY(double Rate READ rate WRITE setRate)
    Q_PROPERTY(QVariantMap Metadata READ metadata)
    Q_PROPERTY(double Volume READ volume WRITE setVolume)
    Q_PROPERTY(qlonglong Position READ position)
    Q_PROPERTY(double MinimumRate READ minimumRate CONSTANT)
    Q_PROPERTY(double MaximumRate READ maximumRate CONSTANT)
    Q_PROPERTY(bool CanGoNext READ canGoNext)
    Q_PROPERTY(bool CanGoPrevious READ canGoPrevious)
    Q_PROPERTY(bool CanPlay READ canControl)
    Q_PROPERTY(bool CanPause READ canControl)
    Q_PROPERTY(bool CanSeek READ canControl)
    Q_PROPERTY(bool CanControl READ alwaysTrue CONSTANT)

  public:
    MprisPlayer(QObject *parent, MpvPlayer *player, PlaybackController *playback);

    QString playbackStatus() const;
    double rate() const;
    void setRate(double r);
    QVariantMap metadata() const;
    double volume() const;
    void setVolume(double v);
    qlonglong position() const;
    double minimumRate() const { return 0.25; }
    double maximumRate() const { return 4.0; }
    bool canGoNext() const;
    bool canGoPrevious() const;
    bool canControl() const;
    bool alwaysTrue() const { return true; }

  public slots:
    void Next();
    void Previous();
    void Pause();
    void PlayPause();
    void Stop();
    void Play();
    void Seek(qlonglong offsetUs);
    void SetPosition(const QDBusObjectPath &trackId, qlonglong positionUs);
    void OpenUri(const QString &uri);

  signals:
    void Seeked(qlonglong positionUs);

  private:
    void notify(const QStringList &names);
    QDBusObjectPath trackId() const;

    MpvPlayer *m_player;
    PlaybackController *m_playback;
    double m_lastPos = 0;
    qint64 m_lastPosAt = 0;
    int m_track = 0;
};

// Registers both adaptors on /org/mpris/MediaPlayer2. Does nothing without a session bus.
void registerMpris(QObject *owner, MpvPlayer *player, PlaybackController *playback);
