#include "mpvvideoitem.h"
#include "mpvplayer.h"

#include <QGuiApplication>
#include <QOpenGLContext>
#include <QOpenGLFramebufferObject>
#include <QQuickOpenGLUtils>
#include <QQuickWindow>
#include <QScreen>
#include <QtGui/qguiapplication_platform.h>
#include <mpv/render_gl.h>
#include <vector>

namespace {

void *getProcAddress(void *, const char *name)
{
    QOpenGLContext *ctx = QOpenGLContext::currentContext();
    return ctx ? reinterpret_cast<void *>(ctx->getProcAddress(QByteArray(name))) : nullptr;
}

class MpvRenderer : public QQuickFramebufferObject::Renderer {
  public:
    explicit MpvRenderer(MpvVideoItem *item) : m_item(item) {}

    ~MpvRenderer() override
    {
        // Render thread, GL context current. Must run before the mpv handle is destroyed.
        if (m_ctx) mpv_render_context_free(m_ctx);
    }

    void synchronize(QQuickFramebufferObject *item) override
    {
        // GUI thread is blocked here.
        if (!m_window && item->window()) {
            m_window = item->window();
            QObject::connect(
                m_window, &QQuickWindow::frameSwapped, m_window,
                [this] {
                    if (m_ctx) mpv_render_context_report_swap(m_ctx);
                },
                Qt::DirectConnection);
        }
        if (m_window && m_window->screen() && m_window->screen() != m_screen) {
            m_screen = m_window->screen();
            m_item->player()->setDisplayFps(m_screen->refreshRate());
        }
    }

    QOpenGLFramebufferObject *createFramebufferObject(const QSize &size) override
    {
        if (!m_ctx) init();
        return new QOpenGLFramebufferObject(size);
    }

    void render() override
    {
        if (!m_ctx) return;
        QOpenGLFramebufferObject *fbo = framebufferObject();
        mpv_opengl_fbo mfbo{static_cast<int>(fbo->handle()), fbo->width(), fbo->height(), 0};
        int flipY = 0;
        mpv_render_param params[] = {
            {MPV_RENDER_PARAM_OPENGL_FBO, &mfbo},
            {MPV_RENDER_PARAM_FLIP_Y, &flipY},
            {MPV_RENDER_PARAM_INVALID, nullptr},
        };
        mpv_render_context_render(m_ctx, params);
        QQuickOpenGLUtils::resetOpenGLState();
    }

  private:
    void init()
    {
        mpv_opengl_init_params gl{getProcAddress, nullptr};
        int advanced = 0;
        std::vector<mpv_render_param> params = {
            {MPV_RENDER_PARAM_API_TYPE, const_cast<char *>(MPV_RENDER_API_TYPE_OPENGL)},
            {MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &gl},
            {MPV_RENDER_PARAM_ADVANCED_CONTROL, &advanced},
        };
#if defined(Q_OS_LINUX)
        // Hardware-decoding interop needs the native display. The LGPL libmpv has no X11
        // support, so on X11 it falls back to the DRM device for VAAPI.
        if (auto *wl = qApp->nativeInterface<QNativeInterface::QWaylandApplication>())
            params.push_back({MPV_RENDER_PARAM_WL_DISPLAY, wl->display()});
        else if (auto *x11 = qApp->nativeInterface<QNativeInterface::QX11Application>())
            params.push_back({MPV_RENDER_PARAM_X11_DISPLAY, x11->display()});
#endif
        params.push_back({MPV_RENDER_PARAM_INVALID, nullptr});

        if (mpv_render_context_create(&m_ctx, m_item->player()->handle(), params.data()) < 0)
            qFatal("mpv_render_context_create failed");
        mpv_render_context_set_update_callback(
            m_ctx,
            [](void *ctx) {
                QMetaObject::invokeMethod(static_cast<MpvVideoItem *>(ctx), &QQuickItem::update,
                                          Qt::QueuedConnection);
            },
            m_item);
        QMetaObject::invokeMethod(m_item->player(), &MpvPlayer::renderReady, Qt::QueuedConnection);
    }

    MpvVideoItem *m_item;
    QQuickWindow *m_window = nullptr;
    QScreen *m_screen = nullptr;
    mpv_render_context *m_ctx = nullptr;
};

} // namespace

MpvVideoItem::MpvVideoItem(QQuickItem *parent) : QQuickFramebufferObject(parent) {}

QQuickFramebufferObject::Renderer *MpvVideoItem::createRenderer() const
{
    Q_ASSERT(m_player);
    return new MpvRenderer(const_cast<MpvVideoItem *>(this));
}

void MpvVideoItem::setPlayer(MpvPlayer *p)
{
    if (p == m_player) return;
    m_player = p;
    emit playerChanged();
    update();
}
