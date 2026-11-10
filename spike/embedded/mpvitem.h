#pragma once
#include <QQuickFramebufferObject>
#include <QtQml/qqmlregistration.h>
#include "mpvplayer.h"

// Embedded video path (SPEC 5.2): mpv's OpenGL render API draws into an FBO that the
// Qt Quick scene graph composites under the QML controls. Written from mpv's render API
// docs; mpvqt was used only as a behavioural reference.
class MpvItem : public QQuickFramebufferObject {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(MpvPlayer *player READ player WRITE setPlayer NOTIFY playerChanged)
public:
    explicit MpvItem(QQuickItem *parent = nullptr);
    Renderer *createRenderer() const override;
    MpvPlayer *player() const { return m_player; }
    void setPlayer(MpvPlayer *p);
signals:
    void playerChanged();
private:
    MpvPlayer *m_player = nullptr;
};
