// Huehuecoyotl – app entry point. The C++ layer wires Qt to the Rust core and libmpv; it holds
// no business logic and does no I/O of its own (SPEC 2, principle 3).
#include <QCommandLineParser>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QIcon>
#include <QLibraryInfo>
#include <QLocale>
#include <QOpenGLContext>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QStandardPaths>
#include <QTimer>
#include <QTranslator>
#include <QUrl>

#include "benchmark.h"
#include "corebridge.h"
#include "desktopintegration.h"
#if defined(Q_OS_LINUX)
#include "mpris.h"
#endif
#include "bookplayer.h"
#include "mpvplayer.h"
#include <mpv/stream_cb.h>

// Implemented in Rust (crates/stream), linked through the core-api static library.
extern "C" int coyotl_stream_open(void *user_data, char *uri, mpv_stream_cb_info *info);
#include "musicplayer.h"
#include "playbackcontroller.h"

namespace {
QtMessageHandler g_previousHandler = nullptr;

// Qt's messages go to the core's rotating log file (SPEC 10: one file for both languages) and,
// as before, to stderr.
void forwardToCore(QtMsgType type, const QMessageLogContext &ctx, const QString &msg)
{
    const quint8 level = type == QtDebugMsg ? 0 : type == QtInfoMsg ? 1 : type == QtWarningMsg ? 2 : 3;
    const QByteArray cat(ctx.category ? ctx.category : "default"), text = msg.toUtf8();
    coyotl::core::log_message(level, rust::Str(cat.constData(), size_t(cat.size())),
                              rust::Str(text.constData(), size_t(text.size())));
    if (g_previousHandler) g_previousHandler(type, ctx, msg);
}
} // namespace

int main(int argc, char *argv[])
{
    // Embedded video needs an OpenGL scene graph (Qt defaults to Direct3D 11 on Windows).
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    QGuiApplication::setHighDpiScaleFactorRoundingPolicy(Qt::HighDpiScaleFactorRoundingPolicy::PassThrough);

    QGuiApplication app(argc, argv);
    app.setApplicationName("coyotl"); // config and data folders (SPEC 12)
    app.setApplicationDisplayName("Huehuecoyotl");
    app.setApplicationVersion(QStringLiteral(COYOTL_VERSION));
    app.setDesktopFileName("io.github.blub3.Huehuecoyotl");
    {
        QIcon icon;
        for (int n : {32, 64, 256})
            icon.addFile(QStringLiteral(":/icons/%1x%1/apps/io.github.blub3.Huehuecoyotl.png").arg(n));
        app.setWindowIcon(icon);
    }
    QQuickStyle::setStyle("Basic");

    QTranslator translator;
    if (translator.load(QLocale(), "coyotl", "_", ":/i18n")) app.installTranslator(&translator);

    QCommandLineParser cli;
    cli.setApplicationDescription("Huehuecoyotl media player");
    cli.addHelpOption();
    cli.addVersionOption();
    cli.addOption({"fullscreen", QCoreApplication::translate("main", "Start in fullscreen.")});
    cli.addOption({"data-dir", QCoreApplication::translate("main", "Use this data folder."), "dir"});
    cli.addOption(
        {"benchmark",
         QCoreApplication::translate(
             "main",
             "Measure a performance budget and quit: startup, scroll, episodes, music, songs or books."),
         "name"});
    cli.addOption({"benchmark-out",
                   QCoreApplication::translate("main", "Also write the benchmark result to this file."),
                   "file"});
    cli.addPositionalArgument("file", QCoreApplication::translate("main", "Video file to play."), "[file]");
    cli.process(app);

    const QString dataDir = cli.isSet("data-dir")
                                ? cli.value("data-dir")
                                : QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
    QDir().mkpath(dataDir);

    // Metadata language follows the UI language, e.g. "de-DE" (TMDB falls back to English).
    const QString language = QLocale().name().replace('_', '-');
    auto *bench = new Benchmark(cli.value("benchmark"), cli.value("benchmark-out"), &app);
    bench->mark(QStringLiteral("main"));
    auto *core = new CoreBridge(dataDir, language, &app);
    bench->mark(QStringLiteral("core"));
    if (!core->isOpen()) {
        qCritical("%s", qPrintable(core->openError()));
        return 1;
    }
    g_previousHandler = qInstallMessageHandler(forwardToCore);
    auto *player = new MpvPlayer(&app);
    bench->mark(QStringLiteral("mpv"));
    core->setAppInfoProvider([player] {
        QStringList info{QStringLiteral("app %1").arg(QCoreApplication::applicationVersion()),
                         QStringLiteral("qt %1 (runtime %2)")
                             .arg(QLatin1String(QT_VERSION_STR), QLibraryInfo::version().toString()),
                         QStringLiteral("platform %1").arg(QGuiApplication::platformName())};
        if (QOpenGLContext *gl = QOpenGLContext::globalShareContext())
            info << QStringLiteral("opengl %1.%2")
                        .arg(gl->format().majorVersion())
                        .arg(gl->format().minorVersion());
        info << player->versionInfo();
        return info.join('\n');
    });
    auto *playback = new PlaybackController(player, &app);
    // Issue #8: music plays in its own audio-only handle; a video pauses it and keeps the queue.
    auto *music = new MusicPlayer(&app);
    QObject::connect(playback, &PlaybackController::started, music, &MusicPlayer::pause);
    // Issue #66: books play in a third audio-only handle. A book pauses the music (its queue
    // stays), music pauses the book (its position is saved), a video pauses both; the media
    // controls and the mini player show whichever played last.
    BookPlayer::setBackend({
        [core](qint64 b, qint64 pos) { return core->bookPlay(b, pos); },
        [core](qint64 b, qint64 pos) { core->bookProgress(b, pos); },
        [core](qint64 b, double speed) { core->bookSetSpeed(b, speed); },
        &CoreBridge::previousChapter,
        &CoreBridge::nextChapter,
        &CoreBridge::endOfChapter,
        [](mpv_handle *mpv) {
            if (mpv_stream_cb_add_ro(mpv, "coyotl", nullptr, coyotl_stream_open) < 0)
                qWarning() << "registering the coyotl:// stream protocol for books failed";
        },
    });
    auto *book = new BookPlayer(&app);
    QObject::connect(core, &CoreBridge::musicDetailReady, book, &BookPlayer::onTicket);
    QObject::connect(core, &CoreBridge::requestFailed, book,
                     [book](quint64 id, int, const QString &message) { book->onFailed(id, message); });
    QObject::connect(playback, &PlaybackController::started, book, &BookPlayer::pause);
    QObject::connect(book, &BookPlayer::started, music, &MusicPlayer::pause);
    QObject::connect(music, &MusicPlayer::stateChanged, book, [music, book] {
        if (music->playing() && music->state() != MusicPlayer::Opening) {
            book->pause();
            book->setFront(false);
        }
    });
    // Only video keeps the screensaver away.
    new DesktopIntegration(player, &app);
#if defined(Q_OS_LINUX)
    registerMpris(&app, player, playback, music, book);
#endif

    int rc = 0;
    {
        QQmlApplicationEngine engine;
        engine.setInitialProperties(
            {{"startFullscreen", cli.isSet("fullscreen")},
             // UI preferences live next to the library, so --data-dir keeps test runs apart.
             {"settingsFile", QUrl::fromLocalFile(QDir(dataDir).filePath(QStringLiteral("ui.ini")))}});
        engine.loadFromModule("Coyotl", "Main");
        if (engine.rootObjects().isEmpty()) return 1;
        bench->mark(QStringLiteral("qml"));
        if (auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first())) bench->attach(window);

        if (!cli.positionalArguments().isEmpty()) {
            const QString file = QFileInfo(cli.positionalArguments().first()).absoluteFilePath();
            QTimer::singleShot(0, playback, [playback, file] { playback->openPath(file); });
        }
        rc = app.exec();
        playback->stop(); // save progress
        music->report();
        book->report();
        // engine (and the video item's render context) goes out of scope before the player
    }
    delete playback;
    delete music;
    delete book;
    delete player;
    qInstallMessageHandler(g_previousHandler);
    delete core; // joins core threads
    return rc;
}
