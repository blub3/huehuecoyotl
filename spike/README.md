# M0 spike (throwaway)

Prototypes for SPEC section 11, M0. None of this code is product code; results are in
[docs/spike/M0-report.md](../docs/spike/M0-report.md) and the direct-mode design in
[docs/spike/direct-mode-design.md](../docs/spike/direct-mode-design.md).

| Folder | What |
| --- | --- |
| `libmpv-lgpl/` | Docker recipe for LGPL libmpv 0.42 + FFmpeg 8.1.3 + libass + libplacebo, plus the licence assertions |
| `embedded/` | Prototype 1: Qt Quick window with the embedded mpv render item |
| `direct/` | Prototype 2: direct mode (mpv in a native window, transparent QML overlay window) |
| `smb-stream/` | Prototype 3: Rust stream callbacks for mpv over the pure-Rust `smb` crate, a byte-exact harness and a Samba test container |
| `media/` | Generated 4K HEVC Main10 test clips (SDR and HDR10) and a sidecar `.srt` |

## Prerequisites (Linux)

- Docker, Rust, a C++20 compiler, CMake.
- Qt 6.12 for gcc_64, e.g. with aqtinstall into `~/Qt`; ninja and meson from pip.
- `libxcb-cursor0` (Qt's xcb plugin needs it; without root, extract the .deb and put it on `LD_LIBRARY_PATH`).

## Build and run

```bash
spike/libmpv-lgpl/build.sh
```

```bash
cmake -S spike/embedded -B spike/embedded/build -G Ninja -DCMAKE_PREFIX_PATH=$HOME/Qt/6.12.0/gcc_64 && cmake --build spike/embedded/build
```

```bash
spike/embedded/build/coyotl-spike-embedded --bench 25 --fullscreen spike/media/uhd-hevc10-hdr10.mkv
```

Test clips (needs an ffmpeg with libx265):

```bash
ffmpeg -f lavfi -i testsrc2=size=3840x2160:rate=60:duration=30 -f lavfi -i sine=duration=30 -c:v libx265 -preset ultrafast -pix_fmt yuv420p10le -b:v 40M -c:a aac spike/media/uhd-hevc10-sdr.mkv
```

Samba path:

```bash
docker build -t coyotl-spike-samba spike/smb-stream/samba && docker run -d --name coyotl-samba -v "$PWD/spike/media:/media:ro" coyotl-spike-samba
```

```bash
cd spike/smb-stream && COYOTL_SMB_USER=coyotl COYOTL_SMB_PASS=spike cargo run --release --bin smb-check coyotl://172.17.0.2/media/uhd-hevc10-sdr.mkv ../media/uhd-hevc10-sdr.mkv
```

```bash
COYOTL_SMB_USER=coyotl COYOTL_SMB_PASS=spike spike/embedded/build/coyotl-spike-embedded --sub coyotl://172.17.0.2/media/uhd-hevc10-sdr.srt coyotl://172.17.0.2/media/uhd-hevc10-sdr.mkv
```

Environment knobs: `COYOTL_HWDEC` (default `auto-safe`), `COYOTL_MPV_LOG`, and
`COYOTL_MPV_OPTS="key=value;key=value"` for any mpv option.

## Windows (not yet run)

`direct/` is the Windows HDR prototype. It needs MSVC, Qt 6.12 msvc2022_64, a Windows LGPL
libmpv (`libmpv-2.dll` + `mpv.lib` import library + headers in `-DLIBMPV_PREFIX`), and an
HDR10 display with Windows HDR switched on. `embedded/` builds the same way on Windows.
