#pragma once
#include <QAbstractListModel>
#include <QTimer>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>
#include <functional>

// Issue #48: the episodes of one season for the show page, one tile each, fetched page by page as
// the list scrolls (SPEC 8, rule 3). A page carries everything a tile shows: the episode, its
// versions (probe data trimmed to labels and the summary line) and its saved-copy state.
//
// Changes do not rebuild the list: download progress updates one row in place, and a change event
// re-queries the loaded range in one request and updates the rows in place, so delegates and
// keyboard focus survive.
//
// The model holds no link to the core. CoreBridge installs the backend (requests and the signals
// that answer them); tests install a stub.
class EpisodeModel : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(qint64 season READ season WRITE setSeason NOTIFY seasonChanged)
    Q_PROPERTY(int total READ total NOTIFY totalChanged)
    Q_PROPERTY(int count READ count NOTIFY countChanged)
    Q_PROPERTY(bool loading READ loading NOTIFY loadingChanged)
  public:
    // How the model reaches the core: `fetch(model, season, offset, limit)` returns a request id
    // whose answer goes to `onPage`; `cancel` drops one. `attach` connects the core's signals to
    // the model's slots.
    struct Backend {
        std::function<quint64(qint64 season, int offset, int limit)> fetch;
        std::function<void(quint64 requestId)> cancel;
        std::function<void(EpisodeModel *)> attach;
    };
    static void setBackend(Backend b) { backend() = std::move(b); }

    explicit EpisodeModel(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override
    {
        return parent.isValid() ? 0 : int(m_rows.size());
    }
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;
    bool canFetchMore(const QModelIndex &parent) const override;
    void fetchMore(const QModelIndex &parent) override;

    qint64 season() const { return m_season; }
    void setSeason(qint64 s);
    int total() const { return m_total; }
    int count() const { return int(m_rows.size()); }
    bool loading() const { return m_request != 0; }
    Q_INVOKABLE QVariantMap get(int row) const;
    Q_INVOKABLE int indexOf(qint64 itemId) const;
    // Re-query the loaded range (soon, once for a burst of changes).
    Q_INVOKABLE void refresh() { m_refresh.start(); }

    static constexpr int kPage = 40;

  public slots:
    void onPage(quint64 requestId, int total, int offset, const QVariantList &rows);
    void onFailed(quint64 requestId);
    // A copied file moved on (CoreBridge::downloadProgress).
    void onProgress(const QVariantMap &entry);
    void onItemChanged(qint64 itemId);

  signals:
    void seasonChanged();
    void totalChanged();
    void countChanged();
    void loadingChanged();

  private:
    static Backend &backend()
    {
        static Backend b;
        return b;
    }
    void request(int offset, int limit, bool replace);
    void reload();
    QVariantList m_rows;
    qint64 m_season = 0;
    int m_total = 0;
    quint64 m_request = 0;
    bool m_replace = false;
    QTimer m_refresh;
};
