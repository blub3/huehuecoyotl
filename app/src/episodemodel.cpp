#include "episodemodel.h"

namespace {
// Roles: the summary keys (CoreBridge::summaryMap) plus what a tile adds.
const QList<QByteArray> &keys()
{
    static const QList<QByteArray> k{"id",         "kind",       "title",    "year",    "seasonNo",
                                     "episodeNo",  "poster",     "progress", "watched", "overview",
                                     "onDisk",     "runtimeMin", "saved",    "files",   "lastFileId",
                                     "durationMs", "positionMs", "save"};
    return k;
}
const int kSaveRole = Qt::UserRole + int(keys().indexOf("save"));
} // namespace

EpisodeModel::EpisodeModel(QObject *parent) : QAbstractListModel(parent)
{
    m_refresh.setSingleShot(true);
    m_refresh.setInterval(250);
    connect(&m_refresh, &QTimer::timeout, this, [this] {
        if (m_season > 0) request(0, qMax(kPage, int(m_rows.size())), true);
    });
    if (backend().attach) backend().attach(this);
}

QVariant EpisodeModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_rows.size()) return {};
    const int i = role - Qt::UserRole;
    return i >= 0 && i < keys().size()
               ? m_rows.at(index.row()).toMap().value(QString::fromLatin1(keys().at(i)))
               : QVariant();
}

QHash<int, QByteArray> EpisodeModel::roleNames() const
{
    QHash<int, QByteArray> r;
    for (int i = 0; i < keys().size(); ++i)
        r.insert(Qt::UserRole + i, keys().at(i));
    return r;
}

bool EpisodeModel::canFetchMore(const QModelIndex &parent) const
{
    return !parent.isValid() && m_season > 0 && m_request == 0 && m_rows.size() < m_total;
}

void EpisodeModel::fetchMore(const QModelIndex &parent)
{
    if (canFetchMore(parent)) request(int(m_rows.size()), kPage, false);
}

void EpisodeModel::setSeason(qint64 s)
{
    if (s == m_season) return;
    m_season = s;
    emit seasonChanged();
    reload();
}

QVariantMap EpisodeModel::get(int row) const
{
    return row >= 0 && row < m_rows.size() ? m_rows.at(row).toMap() : QVariantMap();
}

int EpisodeModel::indexOf(qint64 itemId) const
{
    for (int i = 0; i < m_rows.size(); ++i)
        if (m_rows.at(i).toMap().value("id").toLongLong() == itemId) return i;
    return -1;
}

void EpisodeModel::reload()
{
    m_refresh.stop();
    beginResetModel();
    m_rows.clear();
    endResetModel();
    if (m_total != 0) {
        m_total = 0;
        emit totalChanged();
    }
    emit countChanged();
    if (m_season > 0) request(0, kPage, true);
}

void EpisodeModel::request(int offset, int limit, bool replace)
{
    if (!backend().fetch) return;
    if (m_request && backend().cancel) backend().cancel(m_request);
    m_replace = replace;
    m_request = backend().fetch(m_season, offset, limit);
    emit loadingChanged();
}

void EpisodeModel::onPage(quint64 requestId, int total, int offset, const QVariantList &rows)
{
    if (requestId != m_request || m_request == 0) return;
    m_request = 0;
    const int before = int(m_rows.size());
    if (m_replace) {
        if (rows.size() == m_rows.size() && !rows.isEmpty()) {
            m_rows = rows; // same shape: update in place, keep delegates and focus
            emit dataChanged(index(0), index(int(m_rows.size()) - 1));
        } else if (rows.size() > m_rows.size() && !m_rows.isEmpty()) {
            // More rows than before (an episode appeared): update the known ones, add the rest.
            const int n = int(m_rows.size());
            for (int i = 0; i < n; ++i)
                m_rows[i] = rows.at(i);
            emit dataChanged(index(0), index(n - 1));
            beginInsertRows({}, n, int(rows.size()) - 1);
            for (int i = n; i < rows.size(); ++i)
                m_rows.append(rows.at(i));
            endInsertRows();
        } else {
            beginResetModel();
            m_rows = rows;
            endResetModel();
        }
    } else if (offset == m_rows.size() && !rows.isEmpty()) {
        beginInsertRows({}, int(m_rows.size()), int(m_rows.size() + rows.size()) - 1);
        m_rows.append(rows);
        endInsertRows();
    }
    if (total != m_total) {
        m_total = total;
        emit totalChanged();
    }
    if (int(m_rows.size()) != before) emit countChanged();
    emit loadingChanged();
}

void EpisodeModel::onFailed(quint64 requestId)
{
    if (requestId != m_request || m_request == 0) return;
    m_request = 0;
    emit loadingChanged();
}

void EpisodeModel::onProgress(const QVariantMap &e)
{
    if (e.value("kind").toString() != QLatin1String("saved")) return; // exports are not the tile's
    const int row = indexOf(e.value("itemId").toLongLong());
    if (row < 0) return;
    const QString state = e.value("state").toString();
    // Subtitles and other parts, and changes of state, come back with the refresh their
    // itemChanged starts. A running main file moves its tile's bar here.
    if (state != QLatin1String("running") || e.value("id") != e.value("mainId")) return;
    QVariantMap r = m_rows.at(row).toMap();
    QVariantMap save = r.value("save").toMap();
    save.insert("state", state);
    save.insert("bytesDone", e.value("bytesDone"));
    save.insert("size", qMax(e.value("size").toDouble(), save.value("size").toDouble()));
    r.insert("save", save);
    m_rows[row] = r;
    emit dataChanged(index(row), index(row), {kSaveRole});
}

void EpisodeModel::onItemChanged(qint64 itemId)
{
    if (itemId == m_season || indexOf(itemId) >= 0) refresh();
}
