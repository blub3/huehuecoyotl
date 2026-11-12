#include "corebridge.h"

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
            {"runtimeMin", s.runtime_min}};
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

quint64 CoreBridge::browseItems(const QString &kind, qint64 parent, const QString &sort, bool unwatchedOnly,
                                int offset, int limit)
{
    const quint64 id = nextId();
    const QByteArray k = kind.toUtf8(), so = sort.toUtf8();
    if (m_core)
        (*m_core)->browse_items(id, rs(k), parent, rs(so), unwatchedOnly, uint32_t(qMax(0, offset)),
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

quint64 CoreBridge::itemDetail(qint64 itemId)
{
    const quint64 id = nextId();
    if (m_core) (*m_core)->item_detail(id, itemId);
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
    } else if (e.kind == K::Detail) {
        const auto &d = e.detail;
        QVariantList files;
        for (const auto &f : d.files)
            files.append(QVariantMap{{"path", qs(f.path)}, {"size", qlonglong(f.size)}});
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
                  {"files", files},
                  {"positionMs", qulonglong(d.position_ms)},
                  {"children", summaries(d.children)},
                  {"nextUp", d.next_up.empty() ? QVariant() : QVariant(summaryMap(d.next_up[0]))}});
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
