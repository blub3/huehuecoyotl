#pragma once
#include <QObject>
#include <QString>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>
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

  public:
    enum ErrorKind { NoError = 0, Auth, Unreachable, NotFound, Unsupported, Internal };
    Q_ENUM(ErrorKind)

    // Opens the core in `dataDir`. On failure `isOpen()` is false and `openError()` explains.
    explicit CoreBridge(const QString &dataDir, QObject *parent = nullptr);
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
    Q_INVOKABLE quint64 continueWatching(int limit = 20);
    Q_INVOKABLE void setWatched(qint64 itemId, bool watched);
    Q_INVOKABLE void cancel(quint64 requestId);

    void reportProgress(quint64 ticket, qint64 positionMs, qint64 durationMs, int state);
    void setTrackPref(quint64 ticket, const QString &audioLang, const QString &subLang, bool subOff);

    static QString errorText(int kind, const QString &message);

  signals:
    void folderListed(quint64 requestId, const QString &path, const QString &parent,
                      const QVariantList &entries);
    void playbackReady(quint64 requestId, const QVariantMap &ticket);
    void continueWatchingListed(quint64 requestId, const QVariantList &items);
    void itemChanged(qint64 itemId);
    void requestFailed(quint64 requestId, int kind, const QString &message);

  private:
    void dispatch(const coyotl::core::CoreEvent &e);
    quint64 nextId() { return ++m_lastId; }

    static inline CoreBridge *s_instance = nullptr;
    std::optional<rust::Box<coyotl::core::Core>> m_core;
    QString m_openError;
    quint64 m_lastId = 0;
};
