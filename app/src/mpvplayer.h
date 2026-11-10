#pragma once
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>
#include <optional>

struct mpv_handle;
struct mpv_node;

// Owns the mpv handle (SPEC 5.1) and exposes playback to QML. All mpv calls happen on the GUI
// thread except the render calls in MpvVideoItem; mpv's wake-up callback posts here, nothing polls.
class MpvPlayer : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Use Playback.player")

    Q_PROPERTY(State state READ state NOTIFY stateChanged)
    Q_PROPERTY(QString errorString READ errorString NOTIFY stateChanged)
    Q_PROPERTY(double position READ position NOTIFY positionChanged)
    Q_PROPERTY(double duration READ duration NOTIFY durationChanged)
    Q_PROPERTY(double cacheSeconds READ cacheSeconds NOTIFY positionChanged)
    Q_PROPERTY(bool paused READ paused NOTIFY stateChanged)
    Q_PROPERTY(int volume READ volume WRITE setVolume NOTIFY volumeChanged)
    Q_PROPERTY(bool muted READ muted WRITE setMuted NOTIFY volumeChanged)
    Q_PROPERTY(double speed READ speed WRITE setSpeed NOTIFY speedChanged)
    Q_PROPERTY(QString mediaTitle READ mediaTitle NOTIFY mediaChanged)
    Q_PROPERTY(QVariantList audioTracks READ audioTracks NOTIFY tracksChanged)
    Q_PROPERTY(QVariantList subtitleTracks READ subtitleTracks NOTIFY tracksChanged)
    Q_PROPERTY(int audioTrack READ audioTrack WRITE setAudioTrack NOTIFY tracksChanged)
    Q_PROPERTY(int subtitleTrack READ subtitleTrack WRITE setSubtitleTrack NOTIFY tracksChanged)
    Q_PROPERTY(QVariantList chapters READ chapters NOTIFY mediaChanged)
    Q_PROPERTY(QString videoInfo READ videoInfo NOTIFY mediaChanged)
    // Extra bottom margin for subtitles (in mpv's 720-line units) while controls are shown.
    Q_PROPERTY(int subtitleLift READ subtitleLift WRITE setSubtitleLift NOTIFY subtitleLiftChanged)

  public:
    // SPEC 5.5: the UI derives every control from this one property.
    // Values match the core's PlayerState.
    enum State { Idle = 0, Opening, Buffering, Playing, Paused, Ended, Error };
    Q_ENUM(State)

    explicit MpvPlayer(QObject *parent = nullptr);
    ~MpvPlayer() override;

    mpv_handle *handle() const { return m_mpv; }

    State state() const { return m_state; }
    QString errorString() const { return m_error; }
    double position() const { return m_position; }
    double duration() const { return m_duration; }
    double cacheSeconds() const { return m_cache; }
    bool paused() const { return m_paused; }
    int volume() const { return m_volume; }
    bool muted() const { return m_muted; }
    double speed() const { return m_speed; }
    QString mediaTitle() const { return m_mediaTitle; }
    QVariantList audioTracks() const { return m_audio; }
    QVariantList subtitleTracks() const { return m_subs; }
    int audioTrack() const { return m_aid; }
    int subtitleTrack() const { return m_sid; }
    QVariantList chapters() const { return m_chapters; }
    QString videoInfo() const { return m_videoInfo; }
    int subtitleLift() const { return m_subLift; }
    void setSubtitleLift(int px);

    struct ExternalSub {
        QString url, lang;
        bool forced = false;
    };
    struct LoadOptions {
        double startSeconds = 0;
        QList<ExternalSub> subtitles;
        QString audioLangs, subLangs; // comma-separated, every spelling (from the core)
        bool subOff = false;
        bool network = false;
    };
    void load(const QString &url, const LoadOptions &opts);

    Q_INVOKABLE void stop();
    Q_INVOKABLE void togglePause();
    Q_INVOKABLE void setPaused(bool p);
    Q_INVOKABLE void seek(double seconds, bool relative = true);
    Q_INVOKABLE void cycleAudio();
    Q_INVOKABLE void cycleSubtitle();
    Q_INVOKABLE void addChapter(int delta);
    Q_INVOKABLE void frameStep(bool back = false);
    Q_INVOKABLE QString languageOf(int trackId, const QString &type) const;
    void setVolume(int v);
    void setMuted(bool m);
    void setSpeed(double s);
    void setAudioTrack(int id);
    void setSubtitleTrack(int id);

    // Called (queued) by the video item once its render context exists; loads wait for it.
    Q_INVOKABLE void renderReady();
    // For the embedded item: the display refresh rate, which the render API cannot see.
    void setDisplayFps(double fps);

  signals:
    void stateChanged();
    void positionChanged();
    void durationChanged();
    void volumeChanged();
    void speedChanged();
    void mediaChanged();
    void tracksChanged();
    // The user picked a track (not mpv's automatic choice): remember the language.
    void userChangedTracks();
    void endOfFile();
    void fileLoaded();
    void subtitleLiftChanged();

  private:
    void drainEvents();
    void handleProperty(const char *name, int format, void *data);
    void parseTracks(const mpv_node *list);
    void parseChapters(const mpv_node *list);
    void setState(State s);
    void updatePlayState();
    void command(std::initializer_list<const char *> args);
    void addExternalSubtitles();
    void applySubtitleChoice();

    mpv_handle *m_mpv = nullptr;
    State m_state = Idle;
    QString m_error;
    double m_position = 0, m_duration = 0, m_cache = 0, m_speed = 1;
    bool m_paused = false, m_buffering = false, m_eof = false, m_fileOpen = false;
    int m_volume = 100;
    bool m_muted = false;
    QString m_mediaTitle, m_videoInfo;
    QVariantList m_audio, m_subs, m_chapters;
    int m_aid = 0, m_sid = 0;
    bool m_renderReady = false;
    int m_subLift = 0;
    struct Pending {
        QString url;
        LoadOptions opts;
    };
    std::optional<Pending> m_pending;
    LoadOptions m_current;
    int m_subAddsPending = 0;
};
