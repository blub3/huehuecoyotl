// M0 spike, prototype 2: direct mode.
//
// Window layout (SPEC 5.2, direct mode):
//   videoWin   plain top-level QWindow; its native handle goes to mpv as --wid. mpv creates
//              its own child window inside it and renders with vo=gpu-next (d3d11 on Windows,
//              which gives HDR passthrough with target-colorspace-hint).
//   overlay    frameless, translucent QQuickWindow with the QML controls. It is an *owned*
//              window of videoWin (transient parent): on Windows an owned window always stays
//              above its owner, minimises with it and has no taskbar entry. Its geometry
//              follows videoWin's client area, and it covers it completely, so mpv's child
//              window never receives input.
//
//   coyotl-spike-direct [--bench SECONDS] [--fullscreen] [--sub FILE] <path-or-coyotl-url>
//
// Wayland has no foreign window handles (SPEC 5.6), so this exits there.
#include <QCommandLineParser>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QTimer>
#include <QWindow>
#include <cstdio>

#include "mpvplayer.h"

namespace {

class VideoWindow : public QWindow {
public:
    VideoWindow() { setSurfaceType(QSurface::RasterSurface); }
};

} // namespace

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    app.setApplicationName("coyotl-spike-direct");
    if (QGuiApplication::platformName().startsWith("wayland")) {
        std::fprintf(stderr, "direct mode needs a native window handle: not available on Wayland\n");
        return 2;
    }

    QCommandLineParser cli;
    cli.addHelpOption();
    cli.addOption({"bench", "Play N seconds, print stats JSON, quit.", "seconds"});
    cli.addOption({"fullscreen", "Start fullscreen."});
    cli.addOption({"sub", "Sidecar subtitle file or URL.", "file"});
    cli.addPositionalArgument("media", "File path or coyotl:// URL");
    cli.process(app);
    if (cli.positionalArguments().isEmpty()) cli.showHelp(1);

    VideoWindow videoWin;
    videoWin.setTitle("coyotl spike — direct");
    videoWin.resize(1280, 720);
    videoWin.create(); // native handle must exist before mpv_initialize
    auto *player = new MpvPlayer(&app, videoWin.winId());

    QQmlApplicationEngine engine;
    engine.setInitialProperties({{"player", QVariant::fromValue(player)}});
    engine.loadFromModule("SpikeDirect", "Overlay");
    if (engine.rootObjects().isEmpty()) return 1;
    auto *overlay = qobject_cast<QQuickWindow *>(engine.rootObjects().first());

    overlay->setFlags(Qt::FramelessWindowHint | Qt::Tool | Qt::NoDropShadowWindowHint);
    overlay->setColor(Qt::transparent);
    overlay->setTransientParent(&videoWin);

    auto follow = [&] {
        // geometry() is the client area in screen coordinates on every platform.
        overlay->setGeometry(videoWin.geometry());
    };
    QObject::connect(&videoWin, &QWindow::xChanged, overlay, follow);
    QObject::connect(&videoWin, &QWindow::yChanged, overlay, follow);
    QObject::connect(&videoWin, &QWindow::widthChanged, overlay, follow);
    QObject::connect(&videoWin, &QWindow::heightChanged, overlay, follow);
    QObject::connect(&videoWin, &QWindow::visibilityChanged, overlay, [&](QWindow::Visibility v) {
        overlay->setVisible(v != QWindow::Hidden && v != QWindow::Minimized);
        follow();
    });
    // Clicking the taskbar entry or alt-tabbing activates videoWin; hand focus to the overlay.
    QObject::connect(&videoWin, &QWindow::activeChanged, overlay, [&] {
        if (videoWin.isActive()) overlay->requestActivate();
    });
    // Fullscreen is a property of the owner window; QML toggles it, the overlay follows.
    overlay->setProperty("videoWindow", QVariant::fromValue<QObject *>(&videoWin));

    if (cli.isSet("fullscreen")) videoWin.showFullScreen(); else videoWin.show();
    follow();
    overlay->show();
    overlay->requestActivate();

    player->load(cli.positionalArguments().first(), cli.value("sub"));

    if (cli.isSet("bench")) {
        int ms = int(cli.value("bench").toDouble() * 1000);
        QTimer::singleShot(ms, &app, [player, &app, overlay, &videoWin] {
            std::printf("BENCH %s\n", player->benchReport().toUtf8().constData());
            std::printf("WINDOWS video=%dx%d+%d+%d overlay=%dx%d+%d+%d overlayActive=%d\n",
                        videoWin.width(), videoWin.height(), videoWin.x(), videoWin.y(),
                        overlay->width(), overlay->height(), overlay->x(), overlay->y(),
                        overlay->isActive());
            std::fflush(stdout);
            app.quit();
        });
    }
    int rc = app.exec();
    delete overlay;  // QML bindings reference the player
    delete player;   // before the window whose handle mpv draws into
    return rc;
}
