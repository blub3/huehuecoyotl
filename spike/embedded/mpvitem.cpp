#include "mpvitem.h"

#include <QGuiApplication>
#include <QOpenGLContext>
#include <QOpenGLFramebufferObject>
#include <QQuickOpenGLUtils>
#include <QQuickWindow>
#include <QScreen>
#include <QtGui/qguiapplication_platform.h>
#include <mpv/render_gl.h>

namespace {

void *getProcAddress(void *, const char *name)
{
    QOpenGLContext *ctx = QOpenGLContext::currentContext();
    return ctx ? reinterpret_cast<void *>(ctx->getProcAddress(QByteArray(name))) : nullptr;
}

class MpvRenderer : public QQuickFramebufferObject::Renderer {
public:
    explicit MpvRenderer(MpvItem *item) : m_item(item) {}
    ~MpvRenderer() override
    {
        // Runs on the render thread with the GL context current.
        if (m_ctx) mpv_render_context_free(m_ctx);
    }

    void synchronize(QQuickFramebufferObject *item) override
    {
        // GUI thread is blocked here; safe to touch the item and connect render-thread signals.
        if (!m_window && item->window()) {
            m_window = item->window();
            QObject::connect(m_window, &QQuickWindow::frameSwapped, m_window, [this] {
                if (m_ctx) mpv_render_context_report_swap(m_ctx);
            }, Qt::DirectConnection);
            // The render API cannot see the display; without this mpv reports display-fps
            // as unavailable and cannot use display-synced timing.
            if (QScreen *scr = m_window->screen())
                m_item->player()->setOption("display-fps-override", QString::number(scr->refreshRate()));
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
        // hwdec interop (VAAPI/EGL) needs the native display. With an LGPL libmpv
        // only the Wayland display is usable; mpv's X11 support is GPL-only.
        if (auto *wl = qApp->nativeInterface<QNativeInterface::QWaylandApplication>())
            params.push_back({MPV_RENDER_PARAM_WL_DISPLAY, wl->display()});
        else if (auto *x11 = qApp->nativeInterface<QNativeInterface::QX11Application>())
            params.push_back({MPV_RENDER_PARAM_X11_DISPLAY, x11->display()});
        params.push_back({MPV_RENDER_PARAM_INVALID, nullptr});

        if (mpv_render_context_create(&m_ctx, m_item->player()->handle(), params.data()) < 0) {
            qFatal("mpv_render_context_create failed");
        }
        mpv_render_context_set_update_callback(m_ctx, [](void *ctx) {
            QMetaObject::invokeMethod(static_cast<MpvItem *>(ctx), &QQuickItem::update,
                                      Qt::QueuedConnection);
        }, m_item);
        QMetaObject::invokeMethod(m_item->player(), &MpvPlayer::renderReady, Qt::QueuedConnection);
    }

    MpvItem *m_item;
    QQuickWindow *m_window = nullptr;
    mpv_render_context *m_ctx = nullptr;
};

} // namespace

MpvItem::MpvItem(QQuickItem *parent) : QQuickFramebufferObject(parent) {}

QQuickFramebufferObject::Renderer *MpvItem::createRenderer() const
{
    Q_ASSERT(m_player);
    return new MpvRenderer(const_cast<MpvItem *>(this));
}

void MpvItem::setPlayer(MpvPlayer *p)
{
    if (p == m_player) return;
    m_player = p;
    emit playerChanged();
    update();
}
