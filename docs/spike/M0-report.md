# M0 spike – report

Nov 10, 2026 · first run, Linux only

**Gate status: not passed yet.** On Linux/X11 with NVIDIA, every prototype that can run there works. Still open: everything on Windows (including the HDR passthrough prototype), Intel and AMD graphics, Wayland, and the Windows build of the LGPL libmpv. Section 4 lists the spec changes the results suggest.

## Test system

| | |
| --- | --- |
| OS | Linux Mint 22.3 (Ubuntu 24.04 base), Cinnamon on **X11** |
| GPU | NVIDIA GeForce RTX 3080, driver 580.178.04 |
| Display | 3440×1440 @ 59.97 Hz, SDR |
| Qt | 6.12.0 (aqtinstall, gcc_64) |
| libmpv | 0.42.0, own LGPL build (FFmpeg 8.1.3, libplacebo 7.387.0, libass 0.17.5) |
| Test media | Synthetic 4K HEVC Main10 60 fps, 40 Mbit/s, SDR and HDR10 (PQ/BT.2020, mastering metadata), plus a sidecar `.srt` |

## 1. Results per deliverable

| Deliverable (SPEC 11, M0) | Status | Evidence |
| --- | --- | --- |
| Reproducible LGPL libmpv + FFmpeg build | **Pass (Linux)**; Windows not started | `spike/libmpv-lgpl`: Docker recipe, sources pinned by tag and recorded by commit in `BUILDINFO.txt`, all shared libs, rpath `$ORIGIN`. `check-licence.sh` asserts `CONFIG_GPL 0`, `CONFIG_NONFREE 0`, the libavutil licence string "LGPL version 2.1 or later", and `-Dgpl=false` in libmpv's embedded configuration. |
| Embedded render item, 4K HEVC | **Pass on NVIDIA/X11**; Intel, AMD, Wayland, Windows open | See 2.1 |
| Direct mode + HDR10 passthrough on Windows | **Not run** (no Windows machine); the window architecture was tested on X11 | See 2.2 and the [design](direct-mode-design.md) |
| One file from Samba through the Rust stream callbacks | **Pass** | See 2.3 |
| Written design for direct mode | **Draft** | [direct-mode-design.md](direct-mode-design.md) |

## 2. Measurements

### 2.1 Embedded mode (`spike/embedded`)

25 s runs, fullscreen 3440×1440, `hwdec=auto-safe`, numbers from mpv's own counters.

| Clip | Decode | Sync | Decoder drops | VO drops | Delayed | Mistimed | vsync jitter |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 4K HEVC10 SDR | nvdec (CUDA–GL interop) | audio | 0 | 0 | 0 | – | – |
| 4K HEVC10 HDR10, tone-mapped | nvdec | audio | 0 | 0 | 0 | – | – |
| 4K HEVC10 HDR10, tone-mapped | nvdec | display-resample | 0 | 0 | 0 | 2 | 0.008 |
| 4K HEVC10 SDR | software (32 threads) | audio | 0 | 3 | 0 | – | – |

- The picture is upright (no FLIP_Y needed with `QQuickFramebufferObject`), QML controls composite on top, and libass subtitles render.
- Qt Quick runs on OpenGL via `QQuickWindow::setGraphicsApi(OpenGL)`.

### 2.2 Direct mode architecture (`spike/direct`)

- With the **LGPL** libmpv on X11: `vo=gpu-next` fails ("Failed initializing any suitable GPU context"), because mpv's X11 backend is GPL-only. Expected after reading mpv's `meson.build`; now confirmed.
- With the distribution's **GPL** libmpv 0.37 (test only, never shipped): video in the native window, transparent QML overlay above it, overlay geometry followed a move and a resize exactly (`video=960x540+400+200 overlay=960x540+400+200`), and the overlay kept keyboard focus. 18 VO drops in a 14 s run, all at the resize.
- HDR passthrough: not testable here (Windows only in 1.0, and the test display is SDR).

### 2.3 Samba through the stream callbacks (`spike/smb-stream`)

Samba 4.19 in a container (SMB2_10 minimum, user/password), `smb` crate 0.12.1, callbacks registered with `mpv_stream_cb_add_ro("coyotl", …)`.

| Test | Result |
| --- | --- |
| Harness: sequential read of 145 MiB through the exact `extern "C"` callbacks | Byte-identical, 87 MiB/s (731 Mbit/s) over the Docker bridge |
| Harness: 200 random seek + 64 KiB read | Byte-identical, 44 ms average, 49 ms worst |
| Playback of 4K HEVC from `coyotl://…` with the sidecar `.srt` also through `coyotl://` | nvdec, 0 decoder drops, subtitle rendered, demuxer cache filled to 20 s |
| Server stopped for 8.6 s mid-playback (demuxer cache cut to 8 MiB to force it) | UI showed **Buffering**; the stream retried with backoff (0.2 → 1.6 s), reconnected once, playback resumed and finished; 0 decoder drops |

### 2.4 Licences of the Rust tree

302 crates, all permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, BSL-1.0 as an option). `smb` is MIT. `r-efi` is "MIT OR Apache-2.0 OR LGPL-2.1-or-later", so it can be taken as MIT; `cargo-deny` needs a matching exception. No GPL-only crate.

## 3. Problems found and fixed during the spike

| Problem | Fix (applies to the product) |
| --- | --- |
| `loadfile` before the render context exists → "No render context set", no video | `MpvPlayer` queues the load until the render item reports its context is ready |
| `display-fps` unknown in embedded mode | Pass `display-fps-override` from `QScreen::refreshRate()`; call `mpv_render_context_report_swap` on `frameSwapped` |
| `cache=auto` leaves the demuxer cache **off** for `stream_cb` protocols (mpv does not count them as network) | Set `cache=yes` per file for `coyotl://` URLs |
| `sub-files` is a `:`-separated list and splits `coyotl://` URLs | Use `sub-files-append` (or `sub-add`) |
| Rust panic when mpv closes a stream: `smb` handles need a tokio runtime context even to drop, and mpv's threads are not tokio threads | Enter the runtime at the start of every callback (`rt().enter()`); panics were already caught at the boundary |
| `loadfile <url> replace -1 <options>` is rejected by mpv < 0.38 | Irrelevant for our own builds; matters only for distribution packages linking an older system mpv |
| mpv 0.42 needs libwayland ≥ 1.23 and libplacebo ≥ 7.387; Ubuntu 24.04 has 1.22 | Bundle libwayland 1.24 (MIT) with libmpv |

## 4. Amendments to SPEC v1.0 (applied in v1.1)

1. **Section 3, Qt module allowlist:** add **Qt OpenGL** (LGPLv3). `QQuickFramebufferObject::Renderer` returns a `QOpenGLFramebufferObject`, which lives in that module. The alternative is a `QSGRenderNode`-based item without it.
2. **Section 5.2, direct mode platforms:** "X11 possible" holds only for GPL builds. With the official LGPL libmpv, direct mode exists on Windows only.
3. **Section 5.2/12, Windows:** consider direct mode for **all** playback on Windows, so Qt Quick can stay on Direct3D 11 and the OpenGL-on-Windows risk goes away. Decide after tests W3/W3b in the design document.
4. **Section 5.3/5.4:** record the `cache=yes` and `sub-files-append` rules above.
5. **Section 5.4, Rust read-ahead:** the spike fetches a whole 4 MiB chunk synchronously after every seek (44 ms per random seek on a LAN-speed link). The product should fetch a small first block after a seek and widen the window as reads continue sequentially. The planned index block cache covers the start-up probes.
6. **Section 10, libmpv build:** bundle libwayland (and pin libplacebo) alongside libmpv on Linux. FFmpeg 9.0 was not tried; 8.1.3 works with mpv 0.42.

## 5. Still to do before the M0 gate

- [ ] Windows: LGPL libmpv build (MSVC or MinGW cross-build in Docker, as mpv's own CI does), licence check on the DLLs
- [ ] Windows: embedded mode on Intel, AMD and NVIDIA (`spike/embedded` builds unchanged)
- [ ] Windows: direct mode tests W1–W8 from the design document, including HDR10 passthrough
- [ ] Linux: embedded mode on Intel and AMD (VAAPI through the DRM display, since the LGPL build has no X11 display for interop), under X11 and Wayland
- [ ] Linux: embedded mode under Wayland on NVIDIA
- [ ] Re-check the upstream state of the Wayland HDR options C and D (SPEC 5.6)
