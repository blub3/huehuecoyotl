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
