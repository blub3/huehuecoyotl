// M0 spike, prototype 1: Qt Quick window playing video through the embedded mpv item.
//
//   coyotl-spike-embedded [--bench SECONDS] [--fullscreen] [--sub FILE] <path-or-coyotl-url>
//
// --bench plays for SECONDS, prints one JSON line with mpv's timing counters and quits.
#include <QCommandLineParser>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QTimer>
#include <cstdio>

#include "mpvplayer.h"

int main(int argc, char *argv[])
{
    // Embedded mode needs an OpenGL scene graph (Qt defaults to D3D11 on Windows).
    // Must happen before the first window exists.
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);

    QGuiApplication app(argc, argv);
    app.setApplicationName("coyotl-spike-embedded");

    QCommandLineParser cli;
    cli.addHelpOption();
    cli.addOption({"bench", "Play N seconds, print stats JSON, quit.", "seconds"});
    cli.addOption({"fullscreen", "Start fullscreen."});
    cli.addOption({"sub", "Sidecar subtitle file or URL.", "file"});
    cli.addPositionalArgument("media", "File path or coyotl:// URL");
    cli.process(app);
    if (cli.positionalArguments().isEmpty()) cli.showHelp(1);

    auto *player = new MpvPlayer(&app);

    QQmlApplicationEngine engine;
    engine.setInitialProperties({{"player", QVariant::fromValue(player)},
                                 {"startFullscreen", cli.isSet("fullscreen")}});
    engine.loadFromModule("Spike", "Main");
    if (engine.rootObjects().isEmpty()) return 1;

    player->load(cli.positionalArguments().first(), cli.value("sub"));

    if (cli.isSet("bench")) {
        int ms = int(cli.value("bench").toDouble() * 1000);
        QTimer::singleShot(ms, &app, [player, &app] {
            std::printf("BENCH %s\n", player->benchReport().toUtf8().constData());
            std::fflush(stdout);
            app.quit();
        });
    }
    int rc = app.exec();
    // Destroy the QML scene (and with it the render context) before the mpv handle.
    engine.deleteLater();
    QCoreApplication::processEvents();
    return rc;
}
