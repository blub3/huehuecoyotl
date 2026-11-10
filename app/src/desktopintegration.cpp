#include "desktopintegration.h"
#include "mpvplayer.h"

#include <QLoggingCategory>

#if defined(Q_OS_LINUX)
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#elif defined(Q_OS_WIN)
#include <windows.h>
#endif

Q_LOGGING_CATEGORY(lcDesktop, "coyotl.desktop")

DesktopIntegration::DesktopIntegration(MpvPlayer *player, QObject *parent) : QObject(parent), m_player(player)
{
    connect(m_player, &MpvPlayer::stateChanged, this, [this] {
        const auto s = m_player->state();
        setInhibited(s == MpvPlayer::Playing || s == MpvPlayer::Buffering);
    });
}

DesktopIntegration::~DesktopIntegration()
{
    setInhibited(false);
}

void DesktopIntegration::setInhibited(bool on)
{
    if (on == m_inhibited) return;
    m_inhibited = on;
#if defined(Q_OS_LINUX)
    QDBusInterface ss("org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver",
                      "org.freedesktop.ScreenSaver", QDBusConnection::sessionBus());
    if (!ss.isValid()) {
        qCInfo(lcDesktop) << "no org.freedesktop.ScreenSaver service; screensaver not inhibited";
        return;
    }
    if (on) {
        QDBusReply<quint32> r = ss.call("Inhibit", "Huehuecoyotl", tr("Playing video"));
        if (r.isValid())
            m_cookie = r.value();
        else
            qCWarning(lcDesktop) << "Inhibit failed:" << r.error().message();
    } else if (m_cookie) {
        ss.call("UnInhibit", m_cookie);
        m_cookie = 0;
    }
#elif defined(Q_OS_WIN)
    SetThreadExecutionState(on ? (ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED) : ES_CONTINUOUS);
#endif
    qCDebug(lcDesktop) << "screensaver inhibited:" << on;
}
