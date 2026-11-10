#include "corebridge.h"

#include <QCoreApplication>
#include <QLoggingCategory>
#include <QMetaObject>
#include <QQmlEngine>

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
} // namespace

CoreBridge::CoreBridge(const QString &dataDir, QObject *parent) : QObject(parent)
{
    s_instance = this;
    auto sink = std::make_unique<coyotl::core::EventSink>(
        this, [this](const coyotl::core::CoreEvent &e) { dispatch(e); });
    const QByteArray dir = dataDir.toUtf8();
    try {
        m_core.emplace(coyotl::core::core_open(rs(dir), std::move(sink)));
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
    } else if (e.kind == K::Error) {
        const int kind = int(e.error);
        qCWarning(lcCore) << "request" << e.request_id << "failed:" << qs(e.message);
        emit requestFailed(e.request_id, kind, errorText(kind, qs(e.message)));
    }
}
