#pragma once
#include <QObject>
#include <QString>
#include <QVariant>
#include <QtQml/qqmlregistration.h>
#include <mpv/client.h>

// Owns the mpv handle (SPEC 5.1). In the product the same handle also drives direct mode.
class MpvPlayer : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("created in main")
    Q_PROPERTY(QString state READ state NOTIFY changed)
    Q_PROPERTY(double position READ position NOTIFY changed)
    Q_PROPERTY(double duration READ duration NOTIFY changed)
    Q_PROPERTY(bool paused READ paused NOTIFY changed)
    Q_PROPERTY(QString stats READ stats NOTIFY changed)

public:
    // wid == 0: embedded mode (vo=libmpv, render API). wid != 0: direct mode, mpv renders
    // with its own GPU output into that native window (SPEC 5.2).
    explicit MpvPlayer(QObject *parent = nullptr, quintptr wid = 0);
    ~MpvPlayer() override;

    mpv_handle *handle() const { return m_mpv; }
    QString state() const { return m_state; }
    double position() const { return m_pos; }
    double duration() const { return m_dur; }
    bool paused() const { return m_paused; }
    QString stats() const { return m_stats; }

    Q_INVOKABLE void load(const QString &url, const QString &subFile = {});
    Q_INVOKABLE void togglePause();
    Q_INVOKABLE void seek(double seconds, bool relative = true);
    Q_INVOKABLE void setOption(const QString &name, const QString &value);
    Q_INVOKABLE QVariant property(const QString &name) const;
    QString benchReport() const;
    // Called (queued) once the render context exists; vo=libmpv fails before that.
    Q_INVOKABLE void renderReady();

signals:
    void changed();
    void endReached();

private:
    void drainEvents();
    void refreshStats();

    mpv_handle *m_mpv = nullptr;
    QString m_state = QStringLiteral("Idle");
    double m_pos = 0, m_dur = 0;
    bool m_paused = false;
    QString m_stats;
    bool m_renderReady = false;
    QString m_pendingUrl, m_pendingSub;
};
