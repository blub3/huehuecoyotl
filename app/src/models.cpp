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

// ---- LibraryModel -----------------------------------------------------------------------------

LibraryModel::LibraryModel(QObject *parent) : QAbstractListModel(parent)
{
    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::pageReady, this, &LibraryModel::onPage);
    connect(core, &CoreBridge::requestFailed, this, [this](quint64 id) {
        if (id != m_request) return;
        m_request = 0;
        emit loadingChanged();
    });
    connect(core, &CoreBridge::libraryChanged, this, &LibraryModel::refresh);
    connect(core, &CoreBridge::itemChanged, this, &LibraryModel::refresh);
    QMetaObject::invokeMethod(this, &LibraryModel::reload, Qt::QueuedConnection);
}

QVariant LibraryModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() >= m_rows.size()) return {};
    const auto k = summaryKeys();
    const int i = role - Qt::UserRole;
    return i >= 0 && i < k.size() ? m_rows.at(index.row()).toMap().value(QString::fromLatin1(k.at(i)))
                                  : QVariant();
}

QHash<int, QByteArray> LibraryModel::roleNames() const
{
    QHash<int, QByteArray> r;
    const auto k = summaryKeys();
    for (int i = 0; i < k.size(); ++i)
        r.insert(Qt::UserRole + i, k.at(i));
    return r;
}

bool LibraryModel::canFetchMore(const QModelIndex &parent) const
{
    return !parent.isValid() && m_request == 0 && m_rows.size() < m_total;
}

void LibraryModel::fetchMore(const QModelIndex &parent)
{
    if (!canFetchMore(parent)) return;
    m_replace = false;
    m_request = CoreBridge::instance()->browseItems(m_kind, 0, m_sort, m_filter, int(m_rows.size()), kPage);
    emit loadingChanged();
}

void LibraryModel::setKind(const QString &k)
{
    if (k == m_kind) return;
    m_kind = k;
    emit queryChanged();
    reload();
}

void LibraryModel::setSort(const QString &s)
{
    if (s == m_sort) return;
    m_sort = s;
    emit queryChanged();
    reload();
}

void LibraryModel::setFilter(const QString &f)
{
    if (f == m_filter) return;
    m_filter = f;
    emit queryChanged();
    reload();
}

QVariantMap LibraryModel::get(int row) const
{
    return row >= 0 && row < m_rows.size() ? m_rows.at(row).toMap() : QVariantMap();
}

void LibraryModel::reload()
{
    beginResetModel();
    m_rows.clear();
    endResetModel();
    refresh();
}

// Re-query everything loaded so far in one request and replace it.
void LibraryModel::refresh()
{
    if (m_request) CoreBridge::instance()->cancel(m_request);
    m_replace = true;
    m_request =
        CoreBridge::instance()->browseItems(m_kind, 0, m_sort, m_filter, 0, qMax(kPage, int(m_rows.size())));
    emit loadingChanged();
}

void LibraryModel::onPage(quint64 id, int total, int offset, const QVariantList &items)
{
    if (id != m_request) return;
    m_request = 0;
    if (m_replace) {
        if (items.size() == m_rows.size() && !items.isEmpty()) {
            m_rows = items; // same shape: update in place, keep delegates and focus
            emit dataChanged(index(0), index(int(m_rows.size()) - 1));
        } else {
            beginResetModel();
            m_rows = items;
            endResetModel();
        }
    } else if (offset == m_rows.size() && !items.isEmpty()) {
        beginInsertRows({}, int(m_rows.size()), int(m_rows.size() + items.size()) - 1);
        m_rows.append(items);
        endInsertRows();
    }
    if (total != m_total) {
        m_total = total;
        emit totalChanged();
    }
    emit loadingChanged();
}

// ---- small models -----------------------------------------------------------------------------

SearchModel::SearchModel(QObject *parent) : VariantListModel(parent)
{
    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::pageReady, this,
            [this](quint64 id, int, int, const QVariantList &rows) { setRows(id, rows); });
    connect(core, &CoreBridge::requestFailed, this,
            [this](quint64 id, int, const QString &m) { fail(id, m); });
}

void SearchModel::setText(const QString &t)
{
    if (t == m_text) return;
    m_text = t;
    emit textChanged();
    request();
}

quint64 SearchModel::send()
{
    return CoreBridge::instance()->search(m_text);
}

RecentlyAddedModel::RecentlyAddedModel(QObject *parent) : VariantListModel(parent)
{
    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::pageReady, this,
            [this](quint64 id, int, int, const QVariantList &rows) { setRows(id, rows); });
    connect(core, &CoreBridge::requestFailed, this,
            [this](quint64 id, int, const QString &m) { fail(id, m); });
    connect(core, &CoreBridge::libraryChanged, this, [this] { request(); });
    request();
}

quint64 RecentlyAddedModel::send()
{
    return CoreBridge::instance()->recentlyAdded(24);
}

LibraryFoldersModel::LibraryFoldersModel(QObject *parent) : VariantListModel(parent)
{
    auto *core = CoreBridge::instance();
    connect(core, &CoreBridge::librariesListed, this,
            [this](quint64 id, const QVariantList &rows) { setRows(id, rows); });
    connect(core, &CoreBridge::requestFailed, this,
            [this](quint64 id, int, const QString &m) { fail(id, m); });
    connect(core, &CoreBridge::activityChanged, this, [this] { request(); });
    request();
}

quint64 LibraryFoldersModel::send()
{
    return CoreBridge::instance()->libraries();
}

void LibraryFoldersModel::add(const QString &path)
{
    if (m_request) CoreBridge::instance()->cancel(m_request);
    m_request = CoreBridge::instance()->addLibrary(path);
    emit loadingChanged();
}

void LibraryFoldersModel::remove(qint64 id)
{
    if (m_request) CoreBridge::instance()->cancel(m_request);
    m_request = CoreBridge::instance()->removeLibrary(id);
    emit loadingChanged();
}
