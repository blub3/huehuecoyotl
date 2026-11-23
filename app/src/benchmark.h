#pragma once
// Performance budgets (SPEC 9), measured, not asserted:
//   coyotl --benchmark startup   time from process start to the first frame of a loaded Home
//   coyotl --benchmark scroll    frame times while the Movies grid scrolls on its own
//   coyotl --benchmark episodes  frame times while the first show's episode tiles scroll on their
//                                own, with ten episodes saving (issue #48; use a test library)
//   coyotl --benchmark music     frame times while the Music page's albums grid scrolls (issue #10)
// Results go to stdout as one JSON line (and to --benchmark-out FILE), then the app quits.
#include <QElapsedTimer>
#include <QObject>
#include <QVector>
#include <QtQml/qqmlregistration.h>

class QQmlEngine;
class QJSEngine;
class QQuickWindow;

class Benchmark : public QObject {
    Q_OBJECT
    QML_NAMED_ELEMENT(Bench)
    QML_SINGLETON
    Q_PROPERTY(QString mode READ mode CONSTANT)
  public:
    Benchmark(const QString &mode, const QString &outFile, QObject *parent = nullptr);
    static Benchmark *create(QQmlEngine *, QJSEngine *);
    QString mode() const { return m_mode; }

    // Startup: Home has its content; the next frame on screen ends the measurement.
    Q_INVOKABLE void homeReady();
    // Scroll: start and stop recording frame intervals.
    Q_INVOKABLE void beginFrames();
    Q_INVOKABLE void endFrames(int items, double pixels);

    void attach(QQuickWindow *window);
    // Startup phases, reported with the result (milliseconds since process start).
    Q_INVOKABLE void mark(const QString &phase);

  private:
    void finish(const QJsonObject &result);
    static double processAgeMs();

    static inline Benchmark *s_instance = nullptr;
    QString m_mode, m_out;
    QQuickWindow *m_window = nullptr;
    QElapsedTimer m_clock;
    double m_startOffsetMs = 0;
    bool m_homeReady = false, m_recording = false, m_done = false;
    qint64 m_lastFrame = 0;
    QVector<double> m_intervals;
    QList<QPair<QString, double>> m_marks;
};
