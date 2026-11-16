#pragma once
#include <QAbstractListModel>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>

// List models over core queries. They hold the last answer only and re-query on change
// events (SPEC 8, rule 4: change events carry ids, models invalidate and re-query).

class VariantListModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)
    Q_PROPERTY(bool loading READ loading NOTIFY loadingChanged)
    Q_PROPERTY(QString errorString READ errorString NOTIFY errorChanged)
  public:
    using QAbstractListModel::QAbstractListModel;
    int rowCount(const QModelIndex &parent = {}) const override
    {
        return parent.isValid() ? 0 : int(m_rows.size());
    }
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;
    int count() const { return int(m_rows.size()); }
    bool loading() const { return m_request != 0; }
    QString errorString() const { return m_error; }
    Q_INVOKABLE QVariantMap get(int row) const;
    Q_INVOKABLE void refresh() { request(); }
  signals:
    void countChanged();
    void loadingChanged();
    void errorChanged();

  protected:
    virtual QList<QByteArray> keys() const = 0;
    virtual quint64 send() = 0;
    void request();
    void setRows(quint64 requestId, const QVariantList &rows);
    void fail(quint64 requestId, const QString &message);
    quint64 m_request = 0;

  private:
    QVariantList m_rows;
    QString m_error;
};

// Folder browser. `path` empty = starting places.
class FolderModel : public VariantListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString path READ path WRITE setPath NOTIFY pathChanged)
    Q_PROPERTY(QString parentPath READ parentPath NOTIFY pathChanged)
  public:
    explicit FolderModel(QObject *parent = nullptr);
    QString path() const { return m_path; }
    void setPath(const QString &p);
    QString parentPath() const { return m_parent; }
    // Index of the entry with this path, or -1 (to restore focus after going up).
    Q_INVOKABLE int indexOfPath(const QString &path) const;
  signals:
    void pathChanged();

  protected:
    QList<QByteArray> keys() const override
    {
        return {"name", "path", "isDir", "isVideo", "size", "progress", "watched"};
    }
    quint64 send() override;

  private:
    QString m_path, m_requested, m_parent;
};

class ContinueWatchingModel : public VariantListModel {
    Q_OBJECT
    QML_ELEMENT
  public:
    explicit ContinueWatchingModel(QObject *parent = nullptr);

  protected:
    QList<QByteArray> keys() const override
    {
        return {"itemId", "title", "path", "positionMs", "durationMs"};
    }
    quint64 send() override;
};

// Keys of an item summary as the bridge delivers it (CoreBridge::summaryMap).
inline QList<QByteArray> summaryKeys()
{
    return {"id",     "kind",        "title",    "year",     "seasonNo",  "episodeNo",
            "poster", "posterLarge", "progress", "watched",  "unwatched", "overview",
            "onDisk", "runtimeMin",  "saved",    "subtitle", "parentId",  "versions"};
}

// Movies or shows, or the children of an item, fetched page by page as the view scrolls
// (SPEC 8, rule 3). Re-queries the loaded range when the library changes.
class LibraryModel : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString kind READ kind WRITE setKind NOTIFY queryChanged)
    Q_PROPERTY(QString sort READ sort WRITE setSort NOTIFY queryChanged)
    // "" (everything), "unwatched" or "saved" (issue #4).
    Q_PROPERTY(QString filter READ filter WRITE setFilter NOTIFY queryChanged)
    Q_PROPERTY(int total READ total NOTIFY totalChanged)
    Q_PROPERTY(bool loading READ loading NOTIFY loadingChanged)
  public:
    explicit LibraryModel(QObject *parent = nullptr);
    int rowCount(const QModelIndex &parent = {}) const override
    {
        return parent.isValid() ? 0 : int(m_rows.size());
    }
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;
    bool canFetchMore(const QModelIndex &parent) const override;
    void fetchMore(const QModelIndex &parent) override;

    QString kind() const { return m_kind; }
    void setKind(const QString &k);
    QString sort() const { return m_sort; }
    void setSort(const QString &s);
    QString filter() const { return m_filter; }
    void setFilter(const QString &f);
    int total() const { return m_total; }
    bool loading() const { return m_request != 0; }
    Q_INVOKABLE QVariantMap get(int row) const;
    Q_INVOKABLE void refresh();

  signals:
    void queryChanged();
    void totalChanged();
    void loadingChanged();

  private:
    void reload();
    void onPage(quint64 id, int total, int offset, const QVariantList &items);
    static constexpr int kPage = 120;
    QString m_kind = QStringLiteral("movie"), m_sort = QStringLiteral("title");
    QString m_filter;
    QVariantList m_rows;
    int m_total = 0;
    quint64 m_request = 0;
    bool m_replace = false;
    // A refresh or a page was asked for while a request was in flight.
    bool m_dirty = false, m_morePending = false;
};

class SearchModel : public VariantListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString text READ text WRITE setText NOTIFY textChanged)
    // Issue #10: one kind of result (movie, show, artist, album, track), or "" for all.
    Q_PROPERTY(QString kind READ kind WRITE setKind NOTIFY kindChanged)
  public:
    explicit SearchModel(QObject *parent = nullptr);
    QString text() const { return m_text; }
    void setText(const QString &t);
    QString kind() const { return m_kind; }
    void setKind(const QString &k);
  signals:
    void textChanged();
    void kindChanged();

  protected:
    QList<QByteArray> keys() const override { return summaryKeys(); }
    quint64 send() override;

  private:
    QString m_text, m_kind;
};

class RecentlyAddedModel : public VariantListModel {
    Q_OBJECT
    QML_ELEMENT
  public:
    explicit RecentlyAddedModel(QObject *parent = nullptr);

  protected:
    QList<QByteArray> keys() const override { return summaryKeys(); }
    quint64 send() override;
};

class LibraryFoldersModel : public VariantListModel {
    Q_OBJECT
    QML_ELEMENT
  public:
    explicit LibraryFoldersModel(QObject *parent = nullptr);
    // content: "video", "music" (2.0) or "audiobooks" (2.5).
    Q_INVOKABLE void add(const QString &path, const QString &content = QStringLiteral("video"));
    Q_INVOKABLE void remove(qint64 id);

  protected:
    QList<QByteArray> keys() const override
    {
        return {"id", "kind", "name", "root", "files", "lastScanMs", "scanning", "online", "content"};
    }
    quint64 send() override;
};
