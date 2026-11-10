#pragma once
#include <QQuickFramebufferObject>
#include <QtQml/qqmlregistration.h>

class MpvPlayer;

// Embedded video path (SPEC 5.2): mpv's OpenGL render API draws into an FBO that the Qt Quick
// scene graph composites under the QML controls. Requires Qt Quick on OpenGL (set in main).
class MpvVideoItem : public QQuickFramebufferObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(MpvPlayer *player READ player WRITE setPlayer NOTIFY playerChanged)
  public:
    explicit MpvVideoItem(QQuickItem *parent = nullptr);
    Renderer *createRenderer() const override;
    MpvPlayer *player() const { return m_player; }
    void setPlayer(MpvPlayer *p);
  signals:
    void playerChanged();

  private:
    MpvPlayer *m_player = nullptr;
};
