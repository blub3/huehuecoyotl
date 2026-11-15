# Huehuecoyotl

A desktop media player for Windows and Linux: local and network playback, a scanned library and
TMDB metadata, built on libmpv with a Qt Quick interface and a Rust core. Made for the desk and
the living-room PC alike: everything works with a keyboard or remote.

The name honours Huehuecoyotl, the dancing coyote of the Nahua tradition, patron of music,
dance and storytelling. The command and config folder use the short form `coyotl`.

**Status:** version 1.2 is released for Linux (Flatpak). The Windows release follows without a
version number yet. See the [user guide](docs/user/README.md), the
[architecture specification](docs/SPEC.md) and the [M4 status](docs/milestones/M4.md).

## Features so far (M1–M4)

- Embedded mpv video in Qt Quick with hardware decoding (`hwdec=auto-safe`) and display-synced timing
- Browse local folders and mounted drives; open files from the command line
- Full player controls: seek bar with chapters and buffer, ±10 s, chapters, speed, volume, fullscreen
- Embedded and sidecar subtitles (`Movie.de.forced.srt`), audio track selection
- Resume and watched state; "Continue watching" on the home screen
- Audio and subtitle language remembered per series (per video for loose files)
- Keyboard and remote navigation on every screen; English and German UI
- Library folders scanned into Movies and Shows; moves and renames keep watch state
- TMDB metadata with posters, descriptions and episode titles (free personal API key, stored in the OS keystore); uncertain matches go to a review list
- Poster grids that stay smooth with thousands of titles, detail pages, full-text search
- Media keys and desktop media controls through MPRIS on Linux
- Settings → Troubleshooting exports a redacted diagnostics file; Settings → About lists every third-party licence
- Libraries on network shares: SMB 2/3, NFS, WebDAV and FTP, all in pure Rust (no Samba, no GPL). Servers are found on the local network, passwords go to the OS keystore, and playback streams straight to mpv with adaptive read-ahead and rides out short network drops. Offline shares keep their library entries.

## Building

Linux (Windows needs the Windows libmpv recipe, an open M0 item):

1. Qt 6.8 or newer (6.12 recommended), CMake 3.24+, Ninja, a C++20 compiler, Rust (stable), Docker.
2. Build the LGPL libmpv bundle once (about five minutes):

   ```bash
   third_party/mpv/build.sh
   ```

3. Configure and build:

   ```bash
   cmake -S . -B build -G Ninja -DCMAKE_PREFIX_PATH=$HOME/Qt/6.12.0/gcc_64 && cmake --build build
   ```

4. Run:

   ```bash
   build/app/coyotl
   ```

Flatpak (needs `flatpak-builder` or `org.flatpak.Builder`, the KDE 6.11 SDK, the
`rust-stable` SDK extension and `cargo-about`):

```bash
packaging/flatpak/build.sh --install
```

Distribution packagers can link a system libmpv with `-DCOYOTL_SYSTEM_MPV=ON`; if that libmpv
is a GPL build, the package is GPL as a whole (SPEC section 3).

## Tests and checks

```bash
cargo test --workspace
```

```bash
ctest --test-dir build --output-on-failure
```

```bash
cmake --build build --target all_qmllint
```

```bash
cargo deny check licenses bans sources
```

Performance budgets (SPEC 9) on a synthetic library of 5,000 movies:

```bash
cargo run --release -p huehuecoyotl-library --example bench_seed -- /tmp/bench 5000
```

```bash
build/app/coyotl --data-dir /tmp/bench --benchmark startup
```

```bash
build/app/coyotl --data-dir /tmp/bench --benchmark scroll
```

The headless harness drives the same core API as the UI:

```bash
cargo run -p huehuecoyotl-cli -- browse ~/Videos
```

```bash
COYOTL_TMDB_KEY=your-key cargo run -p huehuecoyotl-cli -- library add ~/Videos
```

## Keys

| Where | Keys |
| --- | --- |
| Everywhere | Arrows move, Enter opens, Esc/Backspace go back, F11 fullscreen |
| Player | Space/K play-pause, ←/→ (J/L) seek 10 s, ↑/↓ show controls, A audio, S subtitles, M mute, +/− volume, [ ] speed, PgUp/PgDn chapters, Home start over, . , frame step, F fullscreen |
| Continue watching | Delete marks the item as watched |

## Layout

```text
app/          C++ shell (Qt Quick, libmpv) and QML
crates/       Rust: core (headless API), core-api (cxx bridge), library (SQLite),
              vfs (sources), cli (harness)
third_party/  LGPL libmpv + FFmpeg build recipe and licence assertions
tests/        Qt Quick tests
docs/         specification and spike results
spike/        throwaway M0 prototypes
```

## Licence

Apache-2.0 (see [LICENSE](LICENSE) and [NOTICE](NOTICE)). Contributions need a DCO sign-off; see
[CONTRIBUTING.md](CONTRIBUTING.md). Qt, libmpv and FFmpeg are used under the LGPL and shipped as
replaceable shared libraries.
