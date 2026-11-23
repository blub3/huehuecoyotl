#include "benchmark.h"

#include <QCoreApplication>
#include <QFile>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QQmlEngine>
#include <QQuickWindow>
#include <QScreen>
#include <QTimer>
#include <algorithm>
#include <cstdio>

#if defined(Q_OS_LINUX)
#include <unistd.h>
#endif

Benchmark::Benchmark(const QString &mode, const QString &outFile, QObject *parent)
    : QObject(parent), m_mode(mode), m_out(outFile)
{
    s_instance = this;
    m_clock.start();
    m_startOffsetMs = processAgeMs(); // time spent before main(): loader, static initialisers
}

Benchmark *Benchmark::create(QQmlEngine *, QJSEngine *)
{
    QQmlEngine::setObjectOwnership(s_instance, QQmlEngine::CppOwnership);
    return s_instance;
}

// Milliseconds since the process started, from /proc (Linux); 0 where unknown.
double Benchmark::processAgeMs()
{
#if defined(Q_OS_LINUX)
    QFile stat(QStringLiteral("/proc/self/stat")), up(QStringLiteral("/proc/uptime"));
    if (!stat.open(QIODevice::ReadOnly) || !up.open(QIODevice::ReadOnly)) return 0;
    const QByteArray s = stat.readAll();
    const auto fields = s.mid(s.lastIndexOf(')') + 2).split(' ');
    if (fields.size() < 20) return 0;
    const double startTicks = fields.at(19).toDouble(); // field 22: starttime
    const double uptime = up.readAll().split(' ').first().toDouble();
    return std::max(0.0, (uptime - startTicks / double(sysconf(_SC_CLK_TCK))) * 1000.0);
#else
    return 0;
#endif
}

void Benchmark::attach(QQuickWindow *window)
{
    m_window = window;
    connect(window, &QQuickWindow::frameSwapped, this, [this] {
        const qint64 now = m_clock.nsecsElapsed();
        if (m_mode == QLatin1String("startup") && m_homeReady && !m_done) {
            const double ms = m_startOffsetMs + now / 1e6;
            QJsonObject phases;
            for (const auto &[name, at] : m_marks)
                phases.insert(name, qRound(at * 10) / 10.0);
            finish(QJsonObject{{"benchmark", "startup"},
                               {"phases_ms", phases},
                               {"first_frame_ms", qRound(ms * 10) / 10.0},
                               {"before_main_ms", qRound(m_startOffsetMs * 10) / 10.0},
                               {"budget_ms", 2000},
                               {"within_budget", ms < 2000}});
            return;
        }
        if (m_recording && m_lastFrame) m_intervals.append((now - m_lastFrame) / 1e6);
        m_lastFrame = now;
        if (m_recording) m_window->update(); // keep frames coming even when nothing changes
    });
}

void Benchmark::mark(const QString &phase)
{
    if (m_mode != QLatin1String("startup")) return;
    m_marks.append({phase, m_startOffsetMs + m_clock.nsecsElapsed() / 1e6});
}

void Benchmark::homeReady()
{
    if (m_homeReady) return;
    m_homeReady = true;
    mark(QStringLiteral("home_data"));
    if (m_window) m_window->update();
}

void Benchmark::beginFrames()
{
    if (m_recording) return;
    m_intervals.clear();
    m_lastFrame = 0;
    m_recording = true;
    if (m_window) m_window->update();
}

void Benchmark::endFrames(int items, double pixels)
{
    if (!m_recording) return;
    m_recording = false;
    QVector<double> v = m_intervals;
    std::sort(v.begin(), v.end());
    auto pct = [&v](double p) { return v.isEmpty() ? 0.0 : v.at(qMin(v.size() - 1, int(p * v.size()))); };
    const double refresh = m_window && m_window->screen() ? m_window->screen()->refreshRate() : 60.0;
    const double frame = 1000.0 / (refresh > 1 ? refresh : 60.0);
    // A frame counts as dropped when it took longer than one and a half refresh intervals.
    const auto dropped = std::count_if(v.begin(), v.end(), [frame](double d) { return d > 1.5 * frame; });
    double total = 0;
    for (double d : v)
        total += d;
    auto r1 = [](double x) { return qRound(x * 100) / 100.0; };
    finish(QJsonObject{{"benchmark", m_mode},
                       {"items", items},
                       {"pixels", qRound(pixels)},
                       {"seconds", r1(total / 1000)},
                       {"frames", int(v.size())},
                       {"refresh_hz", r1(refresh)},
                       {"p50_ms", r1(pct(0.5))},
                       {"p95_ms", r1(pct(0.95))},
                       {"p99_ms", r1(pct(0.99))},
                       {"max_ms", r1(v.isEmpty() ? 0 : v.last())},
                       {"dropped", int(dropped)},
                       {"dropped_pct", r1(v.isEmpty() ? 0 : 100.0 * dropped / v.size())},
                       {"platform", QGuiApplication::platformName()}});
}

void Benchmark::finish(const QJsonObject &result)
{
    m_done = true;
    const QByteArray line = QJsonDocument(result).toJson(QJsonDocument::Compact);
    std::fprintf(stdout, "%s\n", line.constData());
    std::fflush(stdout);
    if (!m_out.isEmpty()) {
        QFile f(m_out);
        if (f.open(QIODevice::WriteOnly | QIODevice::Truncate)) f.write(line + '\n');
    }
    QTimer::singleShot(0, qApp, &QCoreApplication::quit);
}
