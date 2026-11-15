#pragma once
#include <QObject>
#include <QTimer>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include "mpvplayer.h"

class QQmlEngine;
class QJSEngine;

// Connects playback tickets from the core to MpvPlayer and reports progress back
// (SPEC 8: "PlayerController connects tickets to MpvPlayer"). Exposed as QML singleton `Playback`.
class PlaybackController : public QObject {
    Q_OBJECT
    QML_NAMED_ELEMENT(Playback)
    QML_SINGLETON
    Q_PROPERTY(MpvPlayer *player READ player CONSTANT)
    Q_PROPERTY(QString title READ title NOTIFY ticketChanged)
    Q_PROPERTY(double resumedFrom READ resumedFrom NOTIFY ticketChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY failed)

  public:
    explicit PlaybackController(MpvPlayer *player, QObject *parent = nullptr);
    ~PlaybackController() override;
    static PlaybackController *create(QQmlEngine *, QJSEngine *);

    MpvPlayer *player() const { return m_player; }
    QString title() const { return m_ticket.value("title").toString(); }
    double resumedFrom() const { return m_resumedFrom; }
    bool busy() const { return m_pendingRequest != 0; }
    QString lastError() const { return m_lastError; }

    Q_INVOKABLE void openPath(const QString &path);
    Q_INVOKABLE void openItem(qint64 itemId);
    // Issue #34: a chosen version of a movie or episode.
    Q_INVOKABLE void openItemFile(qint64 itemId, qint64 fileId);
    // Leave the player: saves progress and stops mpv.
    Q_INVOKABLE void stop();
    Q_INVOKABLE void startOver();
    // Ask the player page to close (media keys, MPRIS Stop); it calls stop() itself.
    void requestLeave() { emit leaveRequested(); }

  signals:
    void ticketChanged();
    void busyChanged();
    // A ticket arrived and playback is starting: the UI shows the player.
    void started();
    void failed(const QString &message);
    void leaveRequested();

  private:
    void onTicket(quint64 requestId, const QVariantMap &ticket);
    void report();
    void rememberTracks();

    static inline PlaybackController *s_instance = nullptr;
    MpvPlayer *m_player;
    QVariantMap m_ticket;
    quint64 m_pendingRequest = 0;
    double m_resumedFrom = 0;
    QString m_lastError;
    QTimer m_progressTimer;
    MpvPlayer::State m_lastReported = MpvPlayer::Idle;
};
