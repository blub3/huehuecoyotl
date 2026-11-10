# Direct mode – design (M0 output)

Nov 10, 2026 · status: **draft, the Windows parts are not yet verified on Windows**

Direct mode is the second video path from SPEC 5.2. mpv renders with its own GPU output into a native window, which allows HDR passthrough. The QML controls sit in a transparent window above it. This document fixes the window structure, the ownership and lifecycle rules, and what the Windows run of the spike still has to prove.

## 1. Window structure

```text
┌──────────────────────────── videoWin (top-level QWindow, raster, black) ──┐
│  ┌──────────────── mpv child window (created by mpv inside --wid) ─────┐  │
│  │  vo=gpu-next, gpu-api=d3d11, target-colorspace-hint=yes             │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────────┘
      ▲ owned by (transient parent), same client-area geometry
┌──────────────────────────── overlay (frameless QQuickWindow, transparent) ┐
│  QML controls; covers the whole client area and takes all input          │
└───────────────────────────────────────────────────────────────────────────┘
```

| Decision | Reason |
| --- | --- |
| The **owner** is a plain `QWindow`, not a Qt Quick window | mpv needs a native handle before `mpv_initialize` (`--wid`), and nothing else may draw into it |
| The overlay is an **owned top-level** window (`setTransientParent`) | On Win32 an owned window always stays above its owner, minimises and restores with it, and has no taskbar button. No manual z-order work is needed. On X11 the window manager does the same for transient windows (confirmed in the spike under Cinnamon). |
| The overlay is not a layered **child** window | Child windows with per-pixel alpha (WS_EX_LAYERED on children) need Windows 8 or newer and do not work with flip-model swapchains. A top-level window is composed by DWM like any other. |
| The overlay covers the **whole** client area | mpv's window never gets focus or mouse input, so `input-vo-keyboard=no` and `input-cursor=no` are belt and braces. One input path: QML. |
| Fullscreen is set on the **owner**; the overlay follows | The taskbar entry, window state and Alt+Tab all belong to the owner |

## 2. Lifecycle and synchronisation

1. Create `videoWin` and call `create()` so `winId()` exists.
2. Create the mpv handle with `wid`, `vo=gpu-next` and the HDR options, then `mpv_initialize`.
3. Load the QML overlay, set its flags (`FramelessWindowHint | Tool | NoDropShadowWindowHint`), transparent colour and transient parent.
4. Keep the overlay's geometry equal to `videoWin.geometry()` on `x/y/width/heightChanged` and on visibility changes (minimised → hidden).
5. When `videoWin` becomes active (taskbar click, Alt+Tab), call `overlay->requestActivate()`.
6. Teardown order: overlay (QML bindings hold the player) → mpv handle (`mpv_terminate_destroy`) → `videoWin`. mpv must never outlive the window it draws into.

Spike result on X11 (with the GPL system libmpv, see section 5): stacking, transparency, move and resize all behaved. Resizing caused about 10 dropped frames while mpv rebuilt its swapchain, which is acceptable for an interactive resize.

## 3. HDR

- Options: `vo=gpu-next`, `gpu-api=d3d11`, `target-colorspace-hint=yes`, `d3d11-output-format=auto`. With Windows HDR switched on, mpv sets the swapchain colourspace from the content (PQ / BT.2020 for HDR10). With HDR off, mpv tone-maps as in embedded mode.
- The overlay stays SDR. DWM composes it at the user's "SDR content brightness", so the controls match the rest of the desktop. Nothing in QML needs to know about HDR.
- Moving the window between an HDR and an SDR monitor: mpv re-evaluates the target on reconfiguration. Verify that this happens without reopening the file (test W6).
- Frame timing belongs to mpv (`video-sync=display-resample`). The overlay has its own swapchain, so QML animations cannot delay video frames.

## 4. When to use direct mode, and the graphics-API question

`QQuickWindow::setGraphicsApi` is process-wide and set once at start-up. Embedded mode needs OpenGL, so in a process that can play in embedded mode the overlay also runs on OpenGL. On Windows a translucent OpenGL top-level window relies on DWM's alpha path (Qt uses `DwmEnableBlurBehindWindow`). Whether that works on all three GPU vendors is test W3.

**Proposal (to be decided after the Windows run): on Windows, use direct mode for all playback, SDR and HDR, and drop embedded mode there.**

- Qt Quick can then keep its default Direct3D 11 backend on Windows, which removes the top risk in SPEC section 12 ("Embedded video needs an OpenGL scene graph … driver quality varies").
- One Windows video path to test instead of two.
- Cost: the two-window handling (focus, resize, stacking) applies to every playback on Windows, not only HDR titles. Translucent D3D11 windows have to work in Qt (test W3b).

Linux and macOS keep embedded mode (section 5).

## 5. Platforms

| Platform | Direct mode | Reason |
| --- | --- | --- |
| Windows | Yes (1.0) | `wid` + d3d11 is mpv's best-supported path |
| X11 | **No, with the official LGPL build** | In mpv 0.42 `meson.build` the whole X11 backend requires `gpl=true` (`x11_opt = get_option('x11').require(get_option('gpl'))`). The spike confirmed it: `vo=gpu-next` fails with "Failed initializing any suitable GPU context". The two-window design itself works on X11, as tested with the distribution's GPL libmpv. |
| Wayland | No | No foreign window handles (SPEC 5.6); unchanged |

So SPEC 5.2's "X11 possible" applies only to distribution packages that link a GPL mpv.

## 6. Open items for the Windows run (gate M0 → M1)

| # | Test | Pass criterion |
| --- | --- | --- |
| W1 | Build `spike/direct` with MSVC against an LGPL `libmpv-2.dll` | Runs, licence check passes on the DLL set |
| W2 | 4K HEVC Main10 SDR at 60 fps, windowed and fullscreen, on Intel, AMD and NVIDIA | `d3d11va` or `nvdec` active, 0 decoder drops over 60 s, < 5 VO drops |
| W3 | Overlay transparency with Qt Quick on OpenGL (as in the current spike) | Video visible through the transparent area, controls opaque, no black rectangle |
| W3b | Same overlay with Qt Quick on D3D11 (remove nothing else) | As W3; decides the proposal in section 4 |
| W4 | HDR10 clip with Windows HDR on | `video-out-params/gamma` = `pq`, display shows HDR (monitor OSD), overlay at SDR brightness |
| W5 | Move, resize, minimise, restore, Alt+Tab, Win+D, fullscreen toggle | Overlay always exactly above the video, keyboard focus on the overlay afterwards |
| W6 | Drag the window from an HDR to an SDR monitor and back | Output switches between passthrough and tone-mapping without reopening |
| W7 | Per-monitor DPI (100 % ↔ 150 %) | Overlay geometry matches the client area exactly |
| W8 | Exclusive-style fullscreen (`d3d11-flip=yes`, `d3d11-exclusive-fs=no` defaults) | No tearing, independent-flip mode reported by PresentMon |
