#pragma once
#include <QDateTime>
#include <QObject>
#include <QSet>
#include <QString>
#include <QStringList>
#include <QUrl>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>
#include <functional>
#include <optional>

#include "coyotl_bridge/lib.h"
#include "rust/cxx.h"

class QQmlEngine;
class QJSEngine;

// Owns the Rust core and turns its events into Qt signals on the GUI thread.
// Exposed to QML as the singleton `Core`. Holds no logic of its own.
class CoreBridge : public QObject {
    Q_OBJECT
    QML_NAMED_ELEMENT(Core)
    QML_SINGLETON
    Q_PROPERTY(bool hasTmdbKey READ hasTmdbKey NOTIFY keyStatusChanged)
    Q_PROPERTY(bool tmdbKeyFromEnvironment READ tmdbKeyFromEnvironment NOTIFY keyStatusChanged)
    // Issue #52: keystore, environment, session, none or unavailable ("" until the core looked),
    // and when TMDB last accepted the key (invalid: never).
    Q_PROPERTY(QString tmdbKeyState READ tmdbKeyState NOTIFY keyStatusChanged)
    Q_PROPERTY(QDateTime tmdbKeyValidatedAt READ tmdbKeyValidatedAt NOTIFY keyStatusChanged)
    // Issue #11: music information from MusicBrainz and the Cover Art Archive, or file tags only.
    Q_PROPERTY(bool musicOnline READ musicOnline WRITE setMusicOnline NOTIFY musicOnlineChanged)
    Q_PROPERTY(bool scanning READ scanning NOTIFY activityChanged)
    Q_PROPERTY(int pendingJobs READ pendingJobs NOTIFY activityChanged)
    Q_PROPERTY(int failedJobs READ failedJobs NOTIFY activityChanged)

  public:
    enum ErrorKind { NoError = 0, Auth, Unreachable, NotFound, Unsupported, Internal, NoSpace };
    Q_ENUM(ErrorKind)

    // Opens the core in `dataDir`. On failure `isOpen()` is false and `openError()` explains.
    // `language` is the UI language as a BCP-47 tag ("de-DE") for metadata requests.
    explicit CoreBridge(const QString &dataDir, const QString &language, QObject *parent = nullptr);
    ~CoreBridge() override;

    static CoreBridge *instance() { return s_instance; }
    static CoreBridge *create(QQmlEngine *, QJSEngine *);

    bool isOpen() const { return m_core.has_value(); }
    QString openError() const { return m_openError; }
    Q_INVOKABLE QString version() const;

    // Each returns the request id; the answer arrives as a signal carrying the same id.
    Q_INVOKABLE quint64 browse(const QString &path);
    Q_INVOKABLE quint64 openPath(const QString &path);
    Q_INVOKABLE quint64 openItem(qint64 itemId);
    // Issue #34: one version of a movie or episode; and "This is a different movie" (answers
    // with detailReady for the new item).
    Q_INVOKABLE quint64 openItemFile(qint64 itemId, qint64 fileId);
    Q_INVOKABLE quint64 splitFile(qint64 fileId);
    Q_INVOKABLE quint64 continueWatching(int limit = 20);
    Q_INVOKABLE void setWatched(qint64 itemId, bool watched);
    Q_INVOKABLE void cancel(quint64 requestId);

    // M2: library
    Q_INVOKABLE quint64 libraries();
    // content: "video", "music" (2.0) or "audiobooks" (2.5).
    Q_INVOKABLE quint64 addLibrary(const QString &path, const QString &content = QStringLiteral("video"));
    Q_INVOKABLE quint64 removeLibrary(qint64 sourceId);
    Q_INVOKABLE void rescan(qint64 sourceId = 0);
    // filter: "" (everything), "unwatched" or "saved".
    Q_INVOKABLE quint64 browseItems(const QString &kind, qint64 parent, const QString &sort,
                                    const QString &filter, int offset, int limit);
    Q_INVOKABLE quint64 recentlyAdded(int limit = 20);
    // kind: movie, show, artist, album or track; "" for all (issue #10).
    Q_INVOKABLE quint64 search(const QString &text, const QString &kind = QString());
    Q_INVOKABLE quint64 itemDetail(qint64 itemId);
    // Issue #48: one page of a season's episodes for the tiles (answers with episodesReady).
    Q_INVOKABLE quint64 seasonEpisodes(qint64 seasonId, int offset, int limit);
    // Issue #33: analyse this file before the others ("Analyse now").
    Q_INVOKABLE void probeNow(qint64 fileId);
    Q_INVOKABLE quint64 review();
    Q_INVOKABLE quint64 findMatches(qint64 itemId, const QString &title, int year);
    Q_INVOKABLE void chooseMatch(qint64 itemId, qint64 providerId);
    Q_INVOKABLE quint64 refreshKeyStatus();
    Q_INVOKABLE quint64 setTmdbKey(const QString &key);
    Q_INVOKABLE quint64 clearTmdbKey();
    // Issue #52: ask TMDB whether the key in use is accepted (answers with keyStatusChanged).
    Q_INVOKABLE quint64 testTmdbKey();
    // Issue #12: which of TheAudioDB and Fanart.tv have a key (answers with musicKeysReady);
    // entering a key checks it first, an empty key removes it.
    Q_INVOKABLE quint64 musicKeys();
    Q_INVOKABLE quint64 setMusicKey(const QString &provider, const QString &key);
    // Issue #10: an album's or an artist's page (answers with musicDetailReady).
    // Issue #63: `version` 0 picks the one played last, else by `preference` (quality,
    // smallest, lossless).
    Q_INVOKABLE quint64 albumDetail(qint64 albumId, qint64 version = 0,
                                    const QString &preference = QString());
    // Issue #63: "This is a different album" (answers with the new album's musicDetailReady).
    Q_INVOKABLE quint64 splitAlbumVersion(qint64 version);
    // Issue #63: play this file of a track from now on.
    Q_INVOKABLE void setTrackFile(qint64 track, qint64 file);
    Q_INVOKABLE quint64 artistDetail(qint64 artistId);
    // Issues #65 to #67: a book's, series' or person's page (musicDetailReady of kind "book",
    // "series" or "person"); "Search again" (kind "bookCandidates") and the choice (kind
    // "book"); playing (kind "bookTicket"; positionMs < 0 resumes), progress, finished, speed
    // (<= 0: the default) and "Continue series" (kind "continueSeries").
    Q_INVOKABLE quint64 bookDetail(qint64 id);
    // Issue #78: counts and choices of the music filters for "album" or "track"
    // (musicDetailReady of kind "facets": all, total, hires, lossless, multi, saved, unplayed,
    // genres [[name, n]], decades [[year, n]], letters [[initial, offset]]).
    Q_INVOKABLE quint64 musicFacets(const QString &kind, const QString &filter);
    // Issue #81: playlists (musicDetailReady of kind "playlists", "playlist" or
    // "playlistSaved"). `key` is "smart:<rule>" or an own playlist's id; items are albums,
    // artists or songs; `to` < 0 removes the song at `index`.
    // Issue #82: Now Playing's tint ("#rrggbb" or "") and waveform (peaks 0 to 255, or none
    // yet) for a song (musicDetailReady of kind "visuals"); file 0: the song's best file.
    Q_INVOKABLE quint64 trackVisuals(qint64 track, qint64 file = 0);
    Q_INVOKABLE quint64 playlists();
    Q_INVOKABLE quint64 playlistDetail(const QString &key, int limit = 5000);
    Q_INVOKABLE quint64 playlistCreate(const QString &name, const QVariantList &items = {},
                                       qint64 version = 0);
    Q_INVOKABLE quint64 playlistAdd(qint64 id, const QVariantList &items, qint64 version = 0);
    Q_INVOKABLE quint64 playlistRename(qint64 id, const QString &name);
    Q_INVOKABLE quint64 playlistDelete(qint64 id);
    Q_INVOKABLE quint64 playlistMove(qint64 id, int index, int to);
    Q_INVOKABLE quint64 playPlaylist(const QString &key, int start = 0, bool shuffle = false);
    Q_INVOKABLE quint64 playlistSave(const QString &key);
    Q_INVOKABLE quint64 searchBook(qint64 id, const QString &title = QString(),
                                   const QString &author = QString());
    Q_INVOKABLE quint64 chooseBookMatch(qint64 id, const QString &work);
    // Issue #92: matching again (all answer with musicDetailReady): "matchAgain" (itemId,
    // newItemId, result: unchanged, newly, changed, review, notFound or locked), "matchCandidates"
    // (itemId, itemKind, current, candidates), "matchChosen" (itemId, newItemId) and "matching"
    // (libraries: counts, run, blocked).
    Q_INVOKABLE quint64 matchAgain(qint64 itemId, bool unlock = false);
    Q_INVOKABLE quint64 matchSearch(qint64 itemId, const QVariantMap &query);
    Q_INVOKABLE quint64 matchChoose(qint64 itemId, const QVariantMap &choice);
    Q_INVOKABLE quint64 rematchLibrary(const QString &library, bool all, bool includeLocked);
    Q_INVOKABLE quint64 rematchStop(const QString &library);
    Q_INVOKABLE quint64 matchingStatus();
    Q_INVOKABLE quint64 bookPlay(qint64 book, qint64 positionMs = -1);
    void bookProgress(qint64 book, qint64 positionMs);
    Q_INVOKABLE quint64 bookMarkFinished(qint64 book, bool finished);
    void bookSetSpeed(qint64 book, double speed);
    Q_INVOKABLE quint64 continueSeries(qint64 series);
    // Chapter navigation on a book's timeline (the core's, synchronous).
    static qint64 previousChapter(const QList<qint64> &starts, qint64 position);
    static qint64 nextChapter(const QList<qint64> &starts, qint64 position);
    static qint64 endOfChapter(const QList<qint64> &starts, qint64 duration, qint64 position);
    bool musicOnline() const { return m_musicOnline; }
    void setMusicOnline(bool on);

    // M3: network sources. `spec`: protocol, host, port, share, user, tls, path, name.
    // browseNetwork answers with folderListed (or requestFailed: wrong password, unreachable).
    Q_INVOKABLE quint64 browseNetwork(const QVariantMap &spec, const QString &password, const QString &path);
    Q_INVOKABLE quint64 addNetworkLibrary(const QVariantMap &spec, const QString &password);
    Q_INVOKABLE quint64 discover();
    // SFTP: the host key fingerprint of the server browseNetwork last reached ("" otherwise).
    Q_INVOKABLE QString networkHostKey() const;

    // 1.5: offline copies (issues #3, #4, #5). saveItem, exportFile, deleteDownload,
    // deleteSavedItem, deleteAllSaved, downloads and setStorageDir answer with downloadsListed;
    // progress arrives as downloadProgress, an item's state as saveStateReady.
    Q_INVOKABLE quint64 saveItem(qint64 itemId, qint64 fileId = 0);
    // Save as… (issue #5): a movie or episode to the file `dest`, or a season or show into the
    // folder `dest`. Without `overwrite`, existing files answer with exportConflict.
    Q_INVOKABLE quint64 exportItem(qint64 itemId, qint64 fileId, const QString &dest, bool overwrite);
    // A dialog's file:// URL as a local path (portal paths under Flatpak included).
    Q_INVOKABLE static QString localPath(const QUrl &url);
    // Where Save as… starts: the Videos folder, else the home folder.
    Q_INVOKABLE static QUrl exportFolder();
    // A file the Flatpak document portal grants on its own (no files can be added next to it).
    Q_INVOKABLE static bool isPortalPath(const QString &path);
    Q_INVOKABLE void pauseDownload(qint64 copyId);
    Q_INVOKABLE void resumeDownload(qint64 copyId);
    Q_INVOKABLE quint64 deleteDownload(qint64 copyId);
    Q_INVOKABLE quint64 deleteSavedItem(qint64 itemId);
    Q_INVOKABLE void pauseSavedItem(qint64 itemId);
    Q_INVOKABLE void resumeSavedItem(qint64 itemId);
    Q_INVOKABLE quint64 saveState(qint64 itemId);
    Q_INVOKABLE quint64 deleteAllSaved();
    Q_INVOKABLE quint64 downloads();
    Q_INVOKABLE quint64 setStorageDir(const QString &path);
    // The translated text for a download's error category ("no_space", ...).
    Q_INVOKABLE static QString downloadErrorText(const QString &kind, const QString &message);

    // M4: writes a redacted diagnostics file to the Downloads folder and returns its path,
    // or an empty string (the reason goes to the log and the toast via `lastError`).
    Q_INVOKABLE QString exportDiagnostics();
    Q_INVOKABLE QString lastExportError() const { return m_exportError; }
    // The licences of everything the app ships, embedded at build time.
    Q_INVOKABLE QString thirdPartyNotices() const;
    // Versions and settings from the shell that the core cannot see (Qt, mpv, GPU).
    void setAppInfoProvider(std::function<QString()> f) { m_appInfo = std::move(f); }

    bool hasTmdbKey() const { return m_hasKey; }
    bool tmdbKeyFromEnvironment() const { return m_keyFromEnv; }
    QString tmdbKeyState() const { return m_keyState; }
    QDateTime tmdbKeyValidatedAt() const { return m_keyValidatedAt; }
    bool scanning() const { return !m_scanning.isEmpty(); }
    int pendingJobs() const { return m_pending; }
    int failedJobs() const { return m_failed; }

    void reportProgress(quint64 ticket, qint64 positionMs, qint64 durationMs, int state);

    // Issue #8: the music play queue (answers with queueChanged and musicTickets).
    // Issue #80: play what browseItems would list ("Play these" on Songs).
    quint64 queuePlayQuery(const QString &kind, const QString &sort, const QString &filter, qint64 startItem,
                           bool shuffle);
    quint64 queuePlay(const QList<qint64> &items, qint64 startItem, bool shuffle, qint64 version,
                      const QString &preference);
    quint64 queueInsert(const QList<qint64> &items, bool next, qint64 version, const QString &preference);
    quint64 queueStep(int delta, qint64 positionMs);
    quint64 queueJump(int position);
    quint64 queueRemove(int position);
    quint64 queueMove(int from, int to);
    quint64 queueSetMode(bool shuffle, const QString &repeat);
    void queueAdvanced(quint64 ticket);
    quint64 queueResume(bool afterEnd);
    quint64 queueState();
    quint64 queueClear();
    void musicProgress(quint64 ticket, qint64 positionMs, qint64 durationMs);
    void reportMediaInfo(quint64 ticket, const QString &mpvJson);
    void setTrackPref(quint64 ticket, const QString &audioLang, const QString &subLang, bool subOff);

    static QString errorText(int kind, const QString &message);

  signals:
    void folderListed(quint64 requestId, const QString &path, const QString &parent,
                      const QVariantList &entries);
    void playbackReady(quint64 requestId, const QVariantMap &ticket);
    void continueWatchingListed(quint64 requestId, const QVariantList &items);
    void itemChanged(qint64 itemId);
    void requestFailed(quint64 requestId, int kind, const QString &message);
    void pageReady(quint64 requestId, int total, int offset, const QVariantList &items);
    void detailReady(quint64 requestId, const QVariantMap &detail);
    void episodesReady(quint64 requestId, int total, int offset, const QVariantList &episodes);
    void librariesListed(quint64 requestId, const QVariantList &libraries);
    void reviewReady(quint64 requestId, const QVariantList &items);
    void candidatesReady(quint64 requestId, qint64 itemId, const QVariantList &candidates);
    // requestId 0: unsolicited (e.g. TMDB rejected the stored key).
    void keyStatusChanged(quint64 requestId, const QString &message);
    void scanFinished(qint64 sourceId, const QString &message);
    void activityChanged();
    // The library changed in bulk; grids re-query.
    void libraryChanged();
    void serversDiscovered(quint64 requestId, const QVariantList &servers);
    void downloadsListed(quint64 requestId, const QVariantList &items, double usedBytes,
                         const QString &storageDir);
    // One copied file moved on or changed state; `mainId` is the row Downloads shows.
    void downloadProgress(const QVariantMap &entry);
    void saveStateReady(quint64 requestId, qint64 itemId, const QVariantMap &info);
    void exportConflict(quint64 requestId, const QStringList &paths);
    void musicOnlineChanged();
    // kind: "album" or "artist"; the map as in the core's AlbumDetail / ArtistDetail (camelCase).
    void musicDetailReady(quint64 requestId, const QString &kind, const QVariantMap &detail);
    // Issue #8: the play queue (count, position, shuffle, repeat, offset, entries) and what the
    // music player loads ({action, startMs, current, next}); cover paths are file URLs.
    void queueChanged(quint64 requestId, const QVariantMap &state);
    // Issue #12: {fanarttv, theaudiodb: none|keystore|session|rejected, message}.
    void musicKeysReady(quint64 requestId, const QVariantMap &keys);
    void musicTickets(const QVariantMap &tickets);

  private:
    void dispatch(const coyotl::core::CoreEvent &e);
    quint64 nextId() { return ++m_lastId; }

    static inline CoreBridge *s_instance = nullptr;
    std::optional<rust::Box<coyotl::core::Core>> m_core;
    QString m_openError;
    quint64 m_lastId = 0;
    bool m_hasKey = false, m_keyFromEnv = false;
    QString m_keyState;
    QDateTime m_keyValidatedAt;
    bool m_musicOnline = true;
    QSet<qint64> m_scanning;
    int m_pending = 0, m_failed = 0;
    std::function<QString()> m_appInfo;
    QString m_exportError;
};
