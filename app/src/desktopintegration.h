#pragma once
#include <QObject>

class MpvPlayer;

// OS integration that is not QML's job (SPEC 9): keep the screen awake during playback.
// Linux: org.freedesktop.ScreenSaver over D-Bus (the portal under Flatpak is a later step).
// Windows: SetThreadExecutionState. Media keys: MPRIS in mpris.h on Linux; SMTC on Windows is open.
class DesktopIntegration : public QObject {
    Q_OBJECT
  public:
    DesktopIntegration(MpvPlayer *player, QObject *parent = nullptr);
    ~DesktopIntegration() override;

  private:
    void setInhibited(bool on);
    MpvPlayer *m_player;
    bool m_inhibited = false;
    quint32 m_cookie = 0;
};
