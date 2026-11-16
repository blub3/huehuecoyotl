#pragma once
#include <QObject>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

struct mpv_handle;
class QQmlEngine;
class QJSEngine;

// Music playback (SPEC 5.7, issue #8): a second, audio-only mpv handle next to MpvPlayer. The
// core owns the play queue; this class only feeds mpv the current entry and the following one
// (as tickets from the core), tells the core when mpv moved on, and reports progress. It keeps
// playing while the user browses. Exposed as the QML singleton `Music`.
class MusicPlayer : public QObject {
    Q_OBJECT
    QML_NAMED_ELEMENT(Music)
    QML_SINGLETON

    // MpvPlayer::State values: Idle, Opening, Buffering, Playing, Paused, Ended, Error.
    Q_PROPERTY(int state READ state NOTIFY stateChanged)
    Q_PROPERTY(bool paused READ paused NOTIFY stateChanged)
    Q_PROPERTY(bool playing READ playing NOTIFY stateChanged)
    // There is a queue (the mini player shows).
    Q_PROPERTY(bool active READ active NOTIFY queueChanged)
    Q_PROPERTY(double position READ position NOTIFY positionChanged)
    Q_PROPERTY(double duration READ duration NOTIFY durationChanged)
    // The current entry: id, title, artists, album, albumId, artistId, durationMs, poster,
    // posterFull, codec, sampleRate, bitDepth, lossless, hires.
    Q_PROPERTY(QVariantMap current READ current NOTIFY currentChanged)
    Q_PROPERTY(QVariantList queue READ queue NOTIFY queueChanged)
    Q_PROPERTY(int queueOffset READ queueOffset NOTIFY queueChanged)
    Q_PROPERTY(int queueCount READ queueCount NOTIFY queueChanged)
    Q_PROPERTY(int queuePosition READ queuePosition NOTIFY queueChanged)
    Q_PROPERTY(bool shuffle READ shuffle WRITE setShuffle NOTIFY queueChanged)
    Q_PROPERTY(QString repeat READ repeat NOTIFY queueChanged)
    // Issue #63: which version of an album plays when none was chosen: quality, smallest or
    // lossless (the "Preferred version" setting).
    Q_PROPERTY(QString preference MEMBER m_preference NOTIFY preferenceChanged)
    // "off", "track" or "album" (mpv's replaygain).
    Q_PROPERTY(QString replayGain READ replayGain WRITE setReplayGain NOTIFY replayGainChanged)
    // Issue #9: bit-perfect mode, the output device (mpv's audio-device; "auto" is the system's
    // default), the devices mpv offers ({name, description}), the volume (fixed at 100 in
    // bit-perfect mode) and the signal path. `devices` holds only those worth offering, each with
    // `direct` when it can be used exclusively.
    Q_PROPERTY(bool bitPerfect READ bitPerfect WRITE setBitPerfect NOTIFY outputChanged)
    Q_PROPERTY(QString audioDevice READ audioDevice WRITE setAudioDevice NOTIFY outputChanged)
    Q_PROPERTY(QVariantList devices READ devices NOTIFY devicesChanged)
    Q_PROPERTY(int volume READ volume WRITE setVolume NOTIFY volumeChanged)
    // {sourceRate, sourceFormat, sourceChannels, outRate, outFormat, outChannels, ao, device,
    //  exclusive, filters, bitPerfect, fallback}: what goes in and what goes out (5.7).
    Q_PROPERTY(QVariantMap signalPath READ signalPath NOTIFY signalPathChanged)

  public:
    // No default argument: QML must not build a second player of its own (it would, for a
    // default-constructible singleton, instead of calling create()).
    explicit MusicPlayer(QObject *parent);
    ~MusicPlayer() override;
    static MusicPlayer *create(QQmlEngine *, QJSEngine *);
    static MusicPlayer *instance() { return s_instance; }

    int state() const { return m_state; }
    bool paused() const { return m_state == Paused; }
    bool playing() const { return m_state == Playing || m_state == Buffering || m_state == Opening; }
    bool active() const { return m_count > 0; }
    double position() const { return m_position; }
    double duration() const { return m_duration; }
    QVariantMap current() const { return m_current; }
    QVariantList queue() const { return m_queue; }
    int queueOffset() const { return m_offset; }
    int queueCount() const { return m_count; }
    int queuePosition() const { return m_queuePos; }
    bool shuffle() const { return m_shuffle; }
    QString repeat() const { return m_repeat; }
    QString replayGain() const { return m_replayGain; }
    void setReplayGain(const QString &mode);
    void setShuffle(bool on);
    bool bitPerfect() const { return m_bitPerfect; }
    void setBitPerfect(bool on);
    QString audioDevice() const { return m_device; }
    void setAudioDevice(const QString &name);
    QVariantList devices() const { return m_devices; }
    int volume() const { return m_bitPerfect ? 100 : m_volume; }
    void setVolume(int v);
    QVariantMap signalPath() const { return m_signal; }

    // Items: track, album or artist ids. startItem 0: the first (or a random one with shuffle).
    // Issue #63: `version` is an album version to play (0: the one played last, else by
    // preference).
    Q_INVOKABLE void playItems(const QVariantList &items, qint64 startItem = 0, bool shuffle = false,
                               qint64 version = 0);
    Q_INVOKABLE void playNext(const QVariantList &items, qint64 version = 0);
    Q_INVOKABLE void addToQueue(const QVariantList &items, qint64 version = 0);
    Q_INVOKABLE void togglePause();
    Q_INVOKABLE void play();
    Q_INVOKABLE void pause();
    Q_INVOKABLE void next();
    Q_INVOKABLE void previous();
    Q_INVOKABLE void seek(double seconds, bool relative = true);
    Q_INVOKABLE void jump(int position);
    Q_INVOKABLE void remove(int position);
    Q_INVOKABLE void move(int from, int to);
    Q_INVOKABLE void cycleRepeat();
    Q_INVOKABLE void clear();
    // Save the position (on quit).
    void report();

    enum State { Idle = 0, Opening, Buffering, Playing, Paused, Ended, Error };

  signals:
    void stateChanged();
    void positionChanged();
    void durationChanged();
    void currentChanged();
    void queueChanged();
    void replayGainChanged();
    void preferenceChanged();
    void outputChanged();
    void devicesChanged();
    void volumeChanged();
    void signalPathChanged();
    void failed(const QString &message);

  private:
    void drainEvents();
    void onTickets(const QVariantMap &t);
    void onQueue(quint64 requestId, const QVariantMap &q);
    void append(const QVariantMap &ticket, bool replace, double startSeconds);
    void command(std::initializer_list<const char *> args);
    void setState(int s);
    void updateState();
    void setCurrent(const QVariantMap &entry);
    quint64 ticket() const;
    void track(quint64 requestId) { m_requests.append(requestId); }
    void applyOutput();
    void updateSignal();
    void fallBack();

    static inline MusicPlayer *s_instance = nullptr;
    mpv_handle *m_mpv = nullptr;
    int m_state = Idle;
    double m_position = 0, m_duration = 0;
    bool m_pause = false, m_buffering = false, m_loaded = false;
    // The current track ended before the core's ticket for the following one arrived.
    bool m_awaitNext = false;
    // Tickets in mpv's playlist: the current entry, then the following one.
    QList<QVariantMap> m_entries;
    QVariantMap m_current;
    QVariantList m_queue;
    int m_offset = 0, m_count = 0, m_queuePos = 0;
    bool m_shuffle = false;
    QString m_repeat = QStringLiteral("off");
    QString m_replayGain = QStringLiteral("off");
    QString m_preference = QStringLiteral("quality");
    bool m_bitPerfect = false;
    QString m_device = QStringLiteral("auto");
    QVariantList m_devices;
    int m_volume = 100;
    QVariantMap m_in, m_out, m_signal;
    QString m_ao;
    bool m_filters = false;
    // The chosen device could not be opened: playing on the default device instead.
    bool m_fellBack = false;
    // mpv gives up on the track that found no device: load it again once on the default one.
    bool m_retry = false;
    QList<quint64> m_requests;
    QTimer m_progress;
};
