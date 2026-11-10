#include "models.h"
#include "corebridge.h"

QVariant VariantListModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_rows.size()) return {};
    const QList<QByteArray> k = keys();
    const int i = role - Qt::UserRole;
    if (i < 0 || i >= k.size()) return {};
    return m_rows.at(index.row()).toMap().value(QString::fromLatin1(k.at(i)));
}

QHash<int, QByteArray> VariantListModel::roleNames() const
{
    QHash<int, QByteArray> r;
    const QList<QByteArray> k = keys();
    for (int i = 0; i < k.size(); ++i)
        r.insert(Qt::UserRole + i, k.at(i));
    return r;
}

QVariantMap VariantListModel::get(int row) const
{
    return row >= 0 && row < m_rows.size() ? m_rows.at(row).toMap() : QVariantMap();
}

void VariantListModel::request()
{
    if (m_request) CoreBridge::instance()->cancel(m_request);
    m_request = send();
    emit loadingChanged();
}

void VariantListModel::setRows(quint64 requestId, const QVariantList &rows)
{
    if (requestId != m_request) return;
    m_request = 0;
    if (rows.size() == m_rows.size() && !rows.isEmpty()) {
        // Same shape (typically a refresh): update in place so delegates, and with them
        // keyboard focus, survive.
        m_rows = rows;
        emit dataChanged(index(0), index(int(m_rows.size()) - 1));
    } else {
        beginResetModel();
        m_rows = rows;
        endResetModel();
    }
    if (!m_error.isEmpty()) {
        m_error.clear();
        emit errorChanged();
    }
    emit countChanged();
    emit loadingChanged();
}

void VariantListModel::fail(quint64 requestId, const QString &message)
{
    if (requestId != m_request) return;
    m_request = 0;
    m_error = message;
    emit errorChanged();
    emit loadingChanged();
}

FolderModel::FolderModel(QObject *parent) : VariantListModel(parent)
{
    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::folderListed, this,
            [this](quint64 id, const QString &path, const QString &parent, const QVariantList &rows) {
                if (id != m_request) return;
                const bool changed = path != m_path || parent != m_parent;
                m_path = path;
                m_parent = parent;
                setRows(id, rows);
                if (changed) emit pathChanged();
            });
    connect(core, &CoreBridge::requestFailed, this,
            [this](quint64 id, int, const QString &m) { fail(id, m); });
}

void FolderModel::setPath(const QString &p)
{
    m_requested = p;
    request();
}

quint64 FolderModel::send()
{
    return CoreBridge::instance()->browse(m_requested);
}

int FolderModel::indexOfPath(const QString &path) const
{
    for (int i = 0; i < count(); ++i)
        if (get(i).value("path").toString() == path) return i;
    return -1;
}

ContinueWatchingModel::ContinueWatchingModel(QObject *parent) : VariantListModel(parent)
{
    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::continueWatchingListed, this,
            [this](quint64 id, const QVariantList &rows) { setRows(id, rows); });
    connect(core, &CoreBridge::requestFailed, this,
            [this](quint64 id, int, const QString &m) { fail(id, m); });
    connect(core, &CoreBridge::itemChanged, this, [this] { request(); });
    request();
}

quint64 ContinueWatchingModel::send()
{
    return CoreBridge::instance()->continueWatching(20);
}
