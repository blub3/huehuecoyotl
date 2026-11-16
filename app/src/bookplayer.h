#pragma once
#include <QObject>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>
#include <functional>

struct mpv_handle;

// What the player needs from the core: the app installs one backed by CoreBridge (main.cpp), the
// playback test a stub. Chapter navigation is the core's arithmetic (SPEC 5.8).
struct BookBackend {
    // Ask for a book's ticket (positionMs < 0: where it stopped); returns the request id. The
    // ticket arrives through BookPlayer::onTicket, a failure through onFailed.
    std::function<quint64(qint64 book, qint64 positionMs)> play;
    std::function<void(qint64 book, qint64 positionMs)> progress;
    std::function<void(qint64 book, double speed)> setSpeed;
    std::function<qint64(const QList<qint64> &starts, qint64 position)> previousChapter;
    // -1 in the last chapter.
    std::function<qint64(const QList<qint64> &starts, qint64 position)> nextChapter;
    std::function<qint64(const QList<qint64> &starts, qint64 duration, qint64 position)> endOfChapter;
    // Called on the initialised handle: registers the coyotl:// stream protocol.
    std::function<void(mpv_handle *)> setUp;
};
class QQmlEngine;
class QJSEngine;

// Audiobook playback (SPEC 5.8, issue #66): a third, audio-only mpv handle. A book's files are
// one timeline; the core hands over a ticket with every file and where it starts, and this class
// keeps the position on that timeline, saves it every 5 s while playing (and on pause, seek,
// chapter change, stop and exit), and offers chapters, skipping, speed without a change of pitch
// and a sleep timer that fades out. Exposed as the QML singleton `Book`.
class BookPlayer : public QObject {
    Q_OBJECT
    QML_NAMED_ELEMENT(Book)
    QML_SINGLETON

    // MpvPlayer::State values: Idle, Opening, Buffering, Playing, Paused, Ended, Error.
    Q_PROPERTY(int state READ state NOTIFY stateChanged)
    Q_PROPERTY(bool playing READ playing NOTIFY stateChanged)
    // A book is loaded (or opening).
    Q_PROPERTY(bool active READ active NOTIFY bookChanged)
    // The book, not the music queue, is what played last: the mini player and the media controls
    // show it.
    Q_PROPERTY(bool front READ front NOTIFY frontChanged)
    Q_PROPERTY(qint64 bookId READ bookId NOTIFY bookChanged)
    // {bookId, title, authors, poster, posterFull, durationMs, chapters, files}
    Q_PROPERTY(QVariantMap book READ book NOTIFY bookChanged)
    Q_PROPERTY(QVariantList chapters READ chapters NOTIFY bookChanged)
    // Seconds on the book's timeline.
    Q_PROPERTY(double position READ position NOTIFY positionChanged)
    Q_PROPERTY(double duration READ duration NOTIFY bookChanged)
    Q_PROPERTY(int chapter READ chapter NOTIFY chapterChanged)
    Q_PROPERTY(QString chapterTitle READ chapterTitle NOTIFY chapterChanged)
    // Seconds left in the current chapter.
    Q_PROPERTY(double chapterLeft READ chapterLeft NOTIFY positionChanged)
    Q_PROPERTY(double speed READ speed NOTIFY speedChanged)
    // From Settings → Playback.
    Q_PROPERTY(double defaultSpeed MEMBER m_defaultSpeed NOTIFY settingsChanged)
    Q_PROPERTY(int skipBack MEMBER m_skipBack NOTIFY settingsChanged)
    Q_PROPERTY(int skipForward MEMBER m_skipForward NOTIFY settingsChanged)
    // "off", "time" or "chapter"; seconds left for "time".
    Q_PROPERTY(QString sleep READ sleep NOTIFY sleepChanged)
    Q_PROPERTY(int sleepLeft READ sleepLeft NOTIFY sleepChanged)

  public:
    // The backend must be set before the player is made.
    static void setBackend(BookBackend b) { s_backend = std::move(b); }
    explicit BookPlayer(QObject *parent);
    ~BookPlayer() override;
    static BookPlayer *create(QQmlEngine *, QJSEngine *);
    static BookPlayer *instance() { return s_instance; }

    enum State { Idle = 0, Opening, Buffering, Playing, Paused, Ended, Error };

    int state() const { return m_state; }
    bool playing() const { return m_state == Playing || m_state == Buffering || m_state == Opening; }
    bool active() const { return m_bookId > 0 || m_request != 0; }
    bool front() const { return m_front; }
    void setFront(bool on);
    qint64 bookId() const { return m_bookId; }
    QVariantMap book() const { return m_book; }
    QVariantList chapters() const { return m_chapters; }
    double position() const { return m_position; }
    double duration() const { return m_duration; }
    int chapter() const { return m_chapter; }
    QString chapterTitle() const;
    double chapterLeft() const;
    double speed() const { return m_speed; }
    QString sleep() const { return m_sleep; }
    int sleepLeft() const { return m_sleepLeft; }

    // Play a book (positionMs < 0: where it stopped).
    Q_INVOKABLE void playBook(qint64 bookId, qint64 positionMs = -1);
    Q_INVOKABLE void togglePause();
    Q_INVOKABLE void play();
    Q_INVOKABLE void pause();
    // Seconds on the book's timeline.
    Q_INVOKABLE void seekTo(double seconds);
    // -1: back by skipBack, 1: forward by skipForward.
    Q_INVOKABLE void skip(int direction);
    Q_INVOKABLE void previousChapter();
    Q_INVOKABLE void nextChapter();
    Q_INVOKABLE void jumpToChapter(int index);
    // 0.5 to 3.0 in steps of 0.1, remembered for the book.
    Q_INVOKABLE void setSpeed(double speed);
    // Minutes (5 to 120); 0 cancels.
    Q_INVOKABLE void sleepAfter(int minutes);
    Q_INVOKABLE void sleepAtChapterEnd();
    Q_INVOKABLE void cancelSleep();
    // Unload the book (its position is saved).
    Q_INVOKABLE void stop();
    // Save the position (on quit).
    void report();
    // From the core: a ticket ("bookTicket" detail) or a failed request.
    void onTicket(quint64 requestId, const QString &kind, const QVariantMap &data);
    void onFailed(quint64 requestId, const QString &message);

  signals:
    void stateChanged();
    void frontChanged();
    void bookChanged();
    void positionChanged();
    void chapterChanged();
    void speedChanged();
    void settingsChanged();
    void sleepChanged();
    void failed(const QString &message);
    // Playback started or resumed (the music pauses).
    void started();

  private:
    void drainEvents();
    // Load the files from `index` on, the first at `offset` seconds.
    void loadFrom(int index, double offset, bool paused);
    void setState(int s);
    void updateState();
    void updatePosition();
    void updateChapter();
    QList<qint64> starts() const;
    int fileAt(double seconds) const;
    double fileStart(int index) const;
    void sleepTick();
    void stopForSleep();
    void setVolume(double v);

    static inline BookPlayer *s_instance = nullptr;
    static inline BookBackend s_backend;
    mpv_handle *m_mpv = nullptr;
    int m_state = Idle;
    bool m_front = false;
    qint64 m_bookId = 0;
    QVariantMap m_book;
    QVariantList m_chapters, m_files;
    double m_position = 0, m_duration = 0, m_filePos = 0;
    // Index in m_files of mpv's first playlist entry, and mpv's playlist position.
    int m_base = 0, m_playlistPos = 0;
    int m_chapter = -1;
    double m_speed = 1.0, m_defaultSpeed = 1.0;
    int m_skipBack = 15, m_skipForward = 30;
    bool m_pause = false, m_buffering = false, m_loaded = false;
    // A load is under way: positions mpv reports until the file starts are stale.
    bool m_seeking = false;
    quint64 m_request = 0;
    QString m_sleep = QStringLiteral("off");
    int m_sleepLeft = 0;
    // Timeline seconds where "end of chapter" stops.
    double m_sleepAt = 0;
    bool m_fading = false;
    QTimer m_progress, m_sleepTimer, m_fade;
};
