// Huehuecoyotl – app entry point. The C++ layer wires Qt to the Rust core and libmpv; it holds
// no business logic and does no I/O of its own (SPEC 2, principle 3).
#include <QCommandLineParser>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QLocale>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QStandardPaths>
#include <QTimer>
#include <QTranslator>

#include "corebridge.h"
#include "desktopintegration.h"
#include "mpvplayer.h"
#include "playbackcontroller.h"

int main(int argc, char *argv[])
{
    // Embedded video needs an OpenGL scene graph (Qt defaults to Direct3D 11 on Windows).
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    QGuiApplication::setHighDpiScaleFactorRoundingPolicy(Qt::HighDpiScaleFactorRoundingPolicy::PassThrough);

    QGuiApplication app(argc, argv);
    app.setApplicationName("coyotl"); // config and data folders (SPEC 12)
    app.setApplicationDisplayName("Huehuecoyotl");
    app.setApplicationVersion(QStringLiteral(COYOTL_VERSION));
    app.setDesktopFileName("io.github.huehuecoyotl.Huehuecoyotl");
    QQuickStyle::setStyle("Basic");

    QTranslator translator;
    if (translator.load(QLocale(), "coyotl", "_", ":/i18n")) app.installTranslator(&translator);

    QCommandLineParser cli;
    cli.setApplicationDescription("Huehuecoyotl media player");
    cli.addHelpOption();
    cli.addVersionOption();
    cli.addOption({"fullscreen", QCoreApplication::translate("main", "Start in fullscreen.")});
    cli.addOption({"data-dir", QCoreApplication::translate("main", "Use this data folder."), "dir"});
    cli.addPositionalArgument("file", QCoreApplication::translate("main", "Video file to play."), "[file]");
    cli.process(app);

    const QString dataDir = cli.isSet("data-dir")
                                ? cli.value("data-dir")
                                : QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
    QDir().mkpath(dataDir);

    auto *core = new CoreBridge(dataDir, &app);
    if (!core->isOpen()) {
        qCritical("%s", qPrintable(core->openError()));
        return 1;
    }
    auto *player = new MpvPlayer(&app);
    auto *playback = new PlaybackController(player, &app);
    new DesktopIntegration(player, &app);

    int rc = 0;
    {
        QQmlApplicationEngine engine;
        engine.setInitialProperties({{"startFullscreen", cli.isSet("fullscreen")}});
        engine.loadFromModule("Coyotl", "Main");
        if (engine.rootObjects().isEmpty()) return 1;

        if (!cli.positionalArguments().isEmpty()) {
            const QString file = QFileInfo(cli.positionalArguments().first()).absoluteFilePath();
            QTimer::singleShot(0, playback, [playback, file] { playback->openPath(file); });
        }
        rc = app.exec();
        playback->stop(); // save progress
        // engine (and the video item's render context) goes out of scope before the player
    }
    delete playback;
    delete player;
    delete core; // joins core threads
    return rc;
}
