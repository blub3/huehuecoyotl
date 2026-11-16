#include "corebridge.h"
#include "episodemodel.h"

#include <QJsonDocument>
#include <QJsonObject>

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QLoggingCategory>
#include <QMetaObject>
#include <QQmlEngine>
#include <QStandardPaths>
#include <QUrl>

Q_LOGGING_CATEGORY(lcCore, "coyotl.core")

namespace coyotl::core {

EventSink::EventSink(QObject *context, Handler handler) : m_context(context), m_handler(std::move(handler)) {}

void EventSink::post(const CoreEvent &event) const
{
    // Runs on a core thread. Copy the event and queue it; models are only ever touched on
    // the GUI thread (SPEC 8, rule 2).
    QMetaObject::invokeMethod(m_context, [h = m_handler, ev = event] { h(ev); }, Qt::QueuedConnection);
}

} // namespace coyotl::core

namespace {
QString qs(const rust::String &s)
{
    return QString::fromUtf8(s.data(), qsizetype(s.size()));
}
rust::Str rs(const QByteArray &b)
{
    return rust::Str(b.constData(), size_t(b.size()));
}

QString fileUrl(const rust::String &path)
{
    return path.empty() ? QString() : QUrl::fromLocalFile(qs(path)).toString();
}

QVariantMap summaryMap(const coyotl::core::ItemSummary &s)
{
    return {{"id", qlonglong(s.id)},
            {"kind", qs(s.kind)},
            {"title", qs(s.title)},
            {"year", s.year},
            {"seasonNo", s.season_no},
            {"episodeNo", s.episode_no},
            {"poster", fileUrl(s.poster)},
            {"posterLarge", fileUrl(s.poster_large)},
            {"progress", s.progress},
            {"watched", s.watched},
            {"unwatched", qlonglong(s.unwatched)},
            {"overview", qs(s.overview)},
            {"onDisk", s.on_disk},
            {"runtimeMin", s.runtime_min},
            {"saved", s.saved}};
}

// Issue #4: the state of an item's saved copies.
QVariantMap saveMap(const coyotl::core::SaveInfo &i)
{
    return {{"state", qs(i.state)},
            {"copyId", qlonglong(i.copy_id)},
            {"bytesDone", double(i.bytes_done)},
            {"size", double(i.size)},
            {"error", qs(i.error)},
            {"errorKind", qs(i.error_kind)},
            {"saved", qlonglong(i.saved)},
            {"saveable", qlonglong(i.saveable)}};
}

// Issue #3: a saved copy or an export, or one file's progress.
QVariantMap downloadMap(const coyotl::core::DownloadEntry &d)
{
    return {{"id", qlonglong(d.id)},
            {"mainId", qlonglong(d.main_id)},
            {"kind", qs(d.kind)},
            {"itemId", qlonglong(d.item_id)},
            {"fileId", qlonglong(d.file_id)},
            {"title", qs(d.title)},
            {"path", qs(d.path)},
            {"state", qs(d.state)},
            {"bytesDone", double(d.bytes_done)},
            {"size", double(d.size)},
            {"errorKind", qs(d.error_kind)},
            {"error", qs(d.error)},
            {"completedAt", qlonglong(d.completed_at)}};
}

// A movie's or episode's file (version), with its probe as an object for QML (null until the
// file is analysed; trimmed for episode tiles, issue #48).
QVariantMap fileMap(const coyotl::core::FileRef &f)
{
    return {{"id", qlonglong(f.id)},
            {"path", qs(f.path)},
            {"size", qlonglong(f.size)},
            {"sourceName", qs(f.source_name)},
            {"sourceKind", qs(f.source_kind)},
            {"online", f.online},
            {"saved", f.saved},
            {"edition", qs(f.edition)},
            {"positionMs", qulonglong(f.position_ms)},
            {"probe", f.probe_json.empty()
                          ? QVariant()
                          : QJsonDocument::fromJson(QByteArray::fromStdString(std::string(f.probe_json)))
                                .object()
                                .toVariantMap()}};
}

QVariantList fileList(const rust::Vec<coyotl::core::FileRef> &v)
{
    QVariantList out;
    out.reserve(qsizetype(v.size()));
    for (const auto &f : v)
        out.append(fileMap(f));
    return out;
}

QVariantList summaries(const rust::Vec<coyotl::core::ItemSummary> &v)
{
    QVariantList out;
    out.reserve(qsizetype(v.size()));
    for (const auto &s : v)
        out.append(summaryMap(s));
    return out;
}

QVariantList candidateList(const rust::Vec<coyotl::core::MatchCandidate> &v)
{
    QVariantList out;
    for (const auto &c : v)
        out.append(QVariantMap{{"providerId", qlonglong(c.provider_id)},
                               {"title", qs(c.title)},
                               {"year", c.year},
                               {"score", c.score},
                               {"posterUrl", qs(c.poster_url)},
                               {"overview", qs(c.overview)}});
    return out;
}

QStringList stringList(const rust::Vec<rust::String> &v)
{
    QStringList out;
    for (const auto &s : v)
        out.append(qs(s));
    return out;
}
} // namespace

CoreBridge::CoreBridge(const QString &dataDir, const QString &language, QObject *parent) : QObject(parent)
{
    s_instance = this;
    auto sink = std::make_unique<coyotl::core::EventSink>(
        this, [this](const coyotl::core::CoreEvent &e) { dispatch(e); });
    const QByteArray dir = dataDir.toUtf8(), lang = language.toUtf8();
    try {
        m_core.emplace(coyotl::core::core_open(rs(dir), rs(lang), std::move(sink)));
        refreshKeyStatus();
        qCInfo(lcCore) << "core" << version() << "opened in" << dataDir;
        EpisodeModel::setBackend(
            {[this](qint64 season, int offset, int limit) { return seasonEpisodes(season, offset, limit); },
             [this](quint64 id) { cancel(id); },
             [this](EpisodeModel *m) {
                 connect(this, &CoreBridge::episodesReady, m, &EpisodeModel::onPage);
                 connect(this, &CoreBridge::requestFailed, m, [m](quint64 id) { m->onFailed(id); });
                 connect(this, &CoreBridge::downloadProgress, m, &EpisodeModel::onProgress);
                 connect(this, &CoreBridge::itemChanged, m, &EpisodeModel::onItemChanged);
                 connect(this, &CoreBridge::libraryChanged, m, &EpisodeModel::refresh);
             }});
    } catch (const rust::Error &e) {
        m_openError = QString::fromUtf8(e.what());
        qCCritical(lcCore) << "cannot open core:" << m_openError;
    }
}

CoreBridge::~CoreBridge()
{
    m_core.reset(); // joins the core's threads before the sink's context goes away
    if (s_instance == this) s_instance = nullptr;
}

CoreBridge *CoreBridge::create(QQmlEngine *, QJSEngine *)
{
    QQmlEngine::setObjectOwnership(s_instance, QQmlEngine::CppOwnership);
    return s_instance;
}

QString CoreBridge::version() const
{
    return qs(coyotl::core::core_version());
}

quint64 CoreBridge::browse(const QString &path)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->browse(id, rs(path.toUtf8()));
    return id;
}

quint64 CoreBridge::openPath(const QString &path)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->open_path(id, rs(path.toUtf8()));
    return id;
}

quint64 CoreBridge::openItemFile(qint64 itemId, qint64 fileId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->open_item_file(id, itemId, fileId);
    return id;
}

quint64 CoreBridge::splitFile(qint64 fileId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->split_file(id, fileId);
    return id;
}

quint64 CoreBridge::openItem(qint64 itemId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->open_item(id, itemId);
    return id;
}

quint64 CoreBridge::continueWatching(int limit)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->continue_watching(id, uint32_t(qMax(1, limit)));
    return id;
}

void CoreBridge::setWatched(qint64 itemId, bool watched)
{
    if (m_core) (*m_core)->set_watched(itemId, watched);
}

void CoreBridge::cancel(quint64 requestId)
{
    if (m_core) (*m_core)->cancel(requestId);
}

quint64 CoreBridge::libraries()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->libraries(id);
    return id;
}

quint64 CoreBridge::addLibrary(const QString &path)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->add_library(id, rs(path.toUtf8()));
    return id;
}

quint64 CoreBridge::removeLibrary(qint64 sourceId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->remove_library(id, sourceId);
    return id;
}

void CoreBridge::rescan(qint64 sourceId)
{
    if (m_core) (*m_core)->rescan(sourceId);
}

quint64 CoreBridge::browseItems(const QString &kind, qint64 parent, const QString &sort,
                                const QString &filter, int offset, int limit)
{
    const quint64 id = nextId();
    const QByteArray k = kind.toUtf8(), so = sort.toUtf8(), fi = filter.toUtf8();
    if (m_core)
        (*m_core)->browse_items(id, rs(k), parent, rs(so), rs(fi), uint32_t(qMax(0, offset)),
                                uint32_t(qMax(1, limit)));
    return id;
}

quint64 CoreBridge::recentlyAdded(int limit)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->recently_added(id, uint32_t(qMax(1, limit)));
    return id;
}

quint64 CoreBridge::search(const QString &text)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->search(id, rs(text.toUtf8()));
    return id;
}

void CoreBridge::probeNow(qint64 fileId)
{
    if (m_core) (*m_core)->probe_now(fileId);
}

quint64 CoreBridge::saveItem(qint64 itemId, qint64 fileId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->save_item(id, itemId, fileId);
    return id;
}

quint64 CoreBridge::exportItem(qint64 itemId, qint64 fileId, const QString &dest, bool overwrite)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->export_item(id, itemId, fileId, rs(dest.toUtf8()), overwrite);
    return id;
}

QUrl CoreBridge::exportFolder()
{
    QString dir = QStandardPaths::writableLocation(QStandardPaths::MoviesLocation);
    if (dir.isEmpty() || !QDir(dir).exists()) dir = QDir::homePath();
    return QUrl::fromLocalFile(dir);
}

bool CoreBridge::isPortalPath(const QString &path)
{
    // Same rule as the core's is_portal_path: /run/user/<uid>/doc/<id>/name
    const QStringList parts = path.split(u'/');
    return path.startsWith(QStringLiteral("/run/user/")) && parts.size() > 4 && parts.at(4) == u"doc";
}

QString CoreBridge::localPath(const QUrl &url)
{
    return url.isLocalFile() ? url.toLocalFile() : url.toString();
}

void CoreBridge::pauseDownload(qint64 copyId)
{
    if (m_core) (*m_core)->pause_download(copyId);
}

void CoreBridge::resumeDownload(qint64 copyId)
{
    if (m_core) (*m_core)->resume_download(copyId);
}

quint64 CoreBridge::deleteDownload(qint64 copyId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->delete_download(id, copyId);
    return id;
}

quint64 CoreBridge::deleteSavedItem(qint64 itemId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->delete_saved_item(id, itemId);
    return id;
}

void CoreBridge::pauseSavedItem(qint64 itemId)
{
    if (m_core) (*m_core)->pause_saved_item(itemId);
}

void CoreBridge::resumeSavedItem(qint64 itemId)
{
    if (m_core) (*m_core)->resume_saved_item(itemId);
}

quint64 CoreBridge::saveState(qint64 itemId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->save_state(id, itemId);
    return id;
}

quint64 CoreBridge::deleteAllSaved()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->delete_all_saved(id);
    return id;
}

quint64 CoreBridge::downloads()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->downloads(id);
    return id;
}

quint64 CoreBridge::setStorageDir(const QString &path)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->set_storage_dir(id, rs(path.toUtf8()));
    return id;
}

QString CoreBridge::downloadErrorText(const QString &kind, const QString &message)
{
    static const QHash<QString, int> kinds{{"auth", Auth},
                                           {"unreachable", Unreachable},
                                           {"not_found", NotFound},
                                           {"unsupported", Unsupported},
                                           {"no_space", NoSpace}};
    return errorText(kinds.value(kind, Internal), message);
}

quint64 CoreBridge::itemDetail(qint64 itemId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->item_detail(id, itemId);
    return id;
}

quint64 CoreBridge::seasonEpisodes(qint64 seasonId, int offset, int limit)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->season_episodes(id, seasonId, uint32_t(qMax(0, offset)), uint32_t(qMax(1, limit)));
    return id;
}

quint64 CoreBridge::review()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->review(id);
    return id;
}

quint64 CoreBridge::findMatches(qint64 itemId, const QString &title, int year)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->find_matches(id, itemId, rs(title.toUtf8()), uint16_t(qBound(0, year, 9999)));
    return id;
}

void CoreBridge::chooseMatch(qint64 itemId, qint64 providerId)
{
    if (m_core) (*m_core)->choose_match(itemId, providerId);
}

quint64 CoreBridge::refreshKeyStatus()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->key_status(id);
    return id;
}

quint64 CoreBridge::setTmdbKey(const QString &key)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->set_tmdb_key(id, rs(key.toUtf8()));
    return id;
}

quint64 CoreBridge::clearTmdbKey()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->clear_tmdb_key(id);
    return id;
}

quint64 CoreBridge::testTmdbKey()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->test_tmdb_key(id);
    return id;
}

static QByteArray specJson(const QVariantMap &spec)
{
    QJsonObject o = QJsonObject::fromVariantMap(spec);
    if (o.value("port").toInt() <= 0) o.remove("port");
    return QJsonDocument(o).toJson(QJsonDocument::Compact);
}

quint64 CoreBridge::browseNetwork(const QVariantMap &spec, const QString &password, const QString &path)
{
    const quint64 id = nextId();
    const QByteArray j = specJson(spec), pw = password.toUtf8(), p = path.toUtf8();
    if (m_core) (*m_core)->browse_network(id, rs(j), rs(pw), rs(p));
    return id;
}

QString CoreBridge::networkHostKey() const
{
    return m_core ? qs((*m_core)->network_host_key()) : QString();
}

quint64 CoreBridge::addNetworkLibrary(const QVariantMap &spec, const QString &password)
{
    const quint64 id = nextId();
    const QByteArray j = specJson(spec), pw = password.toUtf8();
    if (m_core) (*m_core)->add_network_library(id, rs(j), rs(pw));
    return id;
}

quint64 CoreBridge::discover()
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->discover(id);
    return id;
}

void CoreBridge::reportMediaInfo(quint64 ticket, const QString &mpvJson)
{
    const QByteArray j = mpvJson.toUtf8();
    if (m_core) (*m_core)->report_media_info(ticket, rs(j));
}

void CoreBridge::reportProgress(quint64 ticket, qint64 positionMs, qint64 durationMs, int state)
{
    if (m_core)
        (*m_core)->report_progress(ticket, uint64_t(qMax<qint64>(0, positionMs)),
                                   uint64_t(qMax<qint64>(0, durationMs)), uint8_t(state));
}

void CoreBridge::setTrackPref(quint64 ticket, const QString &audioLang, const QString &subLang, bool subOff)
{
    if (m_core) (*m_core)->set_track_pref(ticket, rs(audioLang.toUtf8()), rs(subLang.toUtf8()), subOff);
}

QString CoreBridge::errorText(int kind, const QString &message)
{
    switch (kind) {
    case Auth:
        return tr("Access denied. Check the permissions or credentials.");
    case Unreachable:
        return tr("The location cannot be reached right now.");
    case NotFound:
        return tr("Not found. It may have been moved or deleted.");
    case Unsupported:
        return tr("This is not supported.");
    case NoSpace:
        return tr("There is not enough free space on the drive.");
    default:
        return tr("Something went wrong: %1").arg(message);
    }
}

void CoreBridge::dispatch(const coyotl::core::CoreEvent &e)
{
    using K = coyotl::core::EventKind;
    if (e.kind == K::Folder) {
        QVariantList entries;
        entries.reserve(qsizetype(e.entries.size()));
        for (const auto &f : e.entries)
            entries.append(QVariantMap{{"name", qs(f.name)},
                                       {"path", qs(f.path)},
                                       {"isDir", f.is_dir},
                                       {"isVideo", f.is_video},
                                       {"size", qulonglong(f.size)},
                                       {"progress", f.progress},
                                       {"watched", f.watched}});
        emit folderListed(e.request_id, qs(e.path), qs(e.parent), entries);
    } else if (e.kind == K::Playback) {
        const auto &t = e.ticket;
        QVariantList subs;
        for (const auto &s : t.subtitles)
            subs.append(QVariantMap{{"url", qs(s.url)}, {"lang", qs(s.lang)}, {"forced", s.forced}});
        emit playbackReady(e.request_id, QVariantMap{{"ticket", qulonglong(t.ticket)},
                                                     {"itemId", qlonglong(t.item_id)},
                                                     {"title", qs(t.title)},
                                                     {"url", qs(t.url)},
                                                     {"isNetwork", t.is_network},
                                                     {"resumeMs", qulonglong(t.resume_ms)},
                                                     {"subtitles", subs},
                                                     {"audioLangs", qs(t.audio_langs)},
                                                     {"subLangs", qs(t.sub_langs)},
                                                     {"subOff", t.sub_off}});
    } else if (e.kind == K::ContinueWatching) {
        QVariantList items;
        for (const auto &r : e.recent)
            items.append(QVariantMap{{"itemId", qlonglong(r.item_id)},
                                     {"title", qs(r.title)},
                                     {"path", qs(r.path)},
                                     {"positionMs", qulonglong(r.position_ms)},
                                     {"durationMs", qulonglong(r.duration_ms)}});
        emit continueWatchingListed(e.request_id, items);
    } else if (e.kind == K::ItemChanged) {
        emit itemChanged(e.item_id);
    } else if (e.kind == K::Page) {
        emit pageReady(e.request_id, int(e.total), int(e.offset), summaries(e.items));
    } else if (e.kind == K::Episodes) {
        QVariantList rows;
        rows.reserve(qsizetype(e.episodes.size()));
        for (const auto &ep : e.episodes) {
            QVariantMap m = summaryMap(ep.item);
            m.insert({{"files", fileList(ep.files)},
                      {"lastFileId", qlonglong(ep.last_file_id)},
                      {"durationMs", qulonglong(ep.duration_ms)},
                      {"positionMs", qulonglong(ep.position_ms)},
                      {"save", saveMap(ep.save)}});
            rows.append(m);
        }
        emit episodesReady(e.request_id, int(e.total), int(e.offset), rows);
    } else if (e.kind == K::Detail) {
        const auto &d = e.detail;
        QVariantMap m = summaryMap(d.item);
        m.insert({{"originalTitle", qs(d.original_title)},
                  {"rating", d.rating},
                  {"runtimeMin", d.runtime_min},
                  {"genres", stringList(d.genres)},
                  {"cast", stringList(d.cast)},
                  {"backdrop", fileUrl(d.backdrop)},
                  {"posterFull", fileUrl(d.poster_full)},
                  {"matchState", qs(d.match_state)},
                  {"tmdbId", qlonglong(d.tmdb_id)},
                  {"parentTitle", qs(d.parent_title)},
                  {"files", fileList(d.files)},
                  {"positionMs", qulonglong(d.position_ms)},
                  {"durationMs", qulonglong(d.duration_ms)},
                  {"lastFileId", qlonglong(d.last_file_id)},
                  {"children", summaries(d.children)},
                  {"nextUp", d.next_up.empty() ? QVariant() : QVariant(summaryMap(d.next_up[0]))},
                  {"save", saveMap(d.save)}});
        emit detailReady(e.request_id, m);
    } else if (e.kind == K::Libraries) {
        QVariantList libs;
        for (const auto &l : e.libraries)
            libs.append(QVariantMap{{"id", qlonglong(l.id)},
                                    {"kind", qs(l.kind)},
                                    {"online", l.online},
                                    {"name", qs(l.name)},
                                    {"root", qs(l.root)},
                                    {"files", qlonglong(l.files)},
                                    {"lastScanMs", qlonglong(l.last_scan_ms)},
                                    {"scanning", l.scanning}});
        emit librariesListed(e.request_id, libs);
    } else if (e.kind == K::Review) {
        QVariantList items;
        for (const auto &r : e.review) {
            QVariantMap m = summaryMap(r.item);
            m.insert("candidates", candidateList(r.candidates));
            items.append(m);
        }
        emit reviewReady(e.request_id, items);
    } else if (e.kind == K::Candidates) {
        emit candidatesReady(e.request_id, e.item_id, candidateList(e.candidates));
    } else if (e.kind == K::ScanState) {
        if (e.flag) {
            m_scanning.insert(e.source_id);
        } else {
            m_scanning.remove(e.source_id);
            emit scanFinished(e.source_id, qs(e.message));
            emit libraryChanged();
        }
        emit activityChanged();
    } else if (e.kind == K::LibraryChanged) {
        m_pending = int(e.queued);
        m_failed = int(e.failed);
        emit activityChanged();
        emit libraryChanged();
    } else if (e.kind == K::KeyStatus) {
        m_hasKey = e.flag;
        m_keyFromEnv = e.flag2;
        m_keyState = qs(e.key_state);
        m_keyValidatedAt = e.validated_at > 0 ? QDateTime::fromSecsSinceEpoch(e.validated_at) : QDateTime();
        emit keyStatusChanged(e.request_id, qs(e.message));
    } else if (e.kind == K::Discovered) {
        QVariantList servers;
        for (const auto &d : e.servers)
            servers.append(QVariantMap{{"name", qs(d.name)},
                                       {"protocol", qs(d.protocol)},
                                       {"host", qs(d.host)},
                                       {"port", int(d.port)},
                                       {"tls", d.tls}});
        emit serversDiscovered(e.request_id, servers);
    } else if (e.kind == K::Downloads) {
        QVariantList items;
        for (const auto &d : e.downloads)
            items.append(downloadMap(d));
        emit downloadsListed(e.request_id, items, double(e.used_bytes), qs(e.path));
    } else if (e.kind == K::DownloadProgress) {
        emit downloadProgress(downloadMap(e.download));
    } else if (e.kind == K::ExportConflict) {
        emit exportConflict(e.request_id, stringList(e.paths));
    } else if (e.kind == K::SaveState) {
        emit saveStateReady(e.request_id, qlonglong(e.item_id), saveMap(e.save));
    } else if (e.kind == K::Error) {
        const int kind = int(e.error);
        qCWarning(lcCore) << "request" << e.request_id << "failed:" << qs(e.message);
        emit requestFailed(e.request_id, kind, errorText(kind, qs(e.message)));
    }
}

QString CoreBridge::exportDiagnostics()
{
    m_exportError.clear();
    if (!m_core) return {};
    QString dir = QStandardPaths::writableLocation(QStandardPaths::DownloadLocation);
    if (dir.isEmpty() || !QDir().mkpath(dir)) dir = QDir::homePath();
    const QString path =
        QDir(dir).filePath(QStringLiteral("coyotl-diagnostics-%1.txt")
                               .arg(QDateTime::currentDateTime().toString("yyyyMMdd-HHmmss")));
    const QByteArray dest = path.toUtf8(), info = (m_appInfo ? m_appInfo() : QString()).toUtf8();
    try {
        const auto bytes = (*m_core)->export_diagnostics(rs(dest), rs(info));
        qCInfo(lcCore) << "diagnostics exported," << bytes << "bytes";
        return path;
    } catch (const rust::Error &e) {
        m_exportError = QString::fromUtf8(e.what());
        qCWarning(lcCore) << "diagnostics export failed:" << m_exportError;
        return {};
    }
}

QString CoreBridge::thirdPartyNotices() const
{
    QFile f(QStringLiteral(":/notices/THIRD_PARTY_NOTICES.txt"));
    if (!f.open(QIODevice::ReadOnly)) return tr("The licence texts are missing from this build.");
    return QString::fromUtf8(f.readAll());
}
