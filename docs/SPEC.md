# Huehuecoyotl – Architecture Specification v1.12

Nov 9, 2026 · @Björn Blankenhagen · amended Nov 10, 2026 after the first M0 spike run · amended Nov 11, 2026 for the 1.0 release and for the 1.1 scope

**Changes in v1.14:** 2.7, matching again (issues #92 to #110), in progress: the shared core for matching items again, searching and choosing by hand and library rematches (section 7, *Matching again*; bridge additions in 8); Settings → Matching (#93 to #96) and Fix match for movies, shows and episodes with Movie ↔ TV show (#97 to #100), matching music again with artists following their albums and songs (#101 to #107), matching audiobooks and authors again with author photos and biographies (#108 to #110, decided in section 12), sections 7 and 9.

**Changes in v1.13:** 2.6 is released for Linux on Nov 18, 2026: the music redesign (issues #78 to #83), with migrations 11 (playlists) and 12 (cover colours and waveforms), as built in sections 9 and 10.

**Changes in v1.12:** 2.5 is released for Linux on Nov 18, 2026. Audiobooks for 2.5 (issues #64 to #67): scope, formats and non-goals (section 2), the audiobook data model and migration 10, the scan pipeline, book providers and privacy (7), the content type (6), audiobook playback (5.8), bridge additions (8), the Audiobooks section (9), the bundle (10), the 2.5 plan (11) and the maintainer's decisions: Open Library without a contact address at 1 request per second, books share the music switch for online lookups, a third audio handle for books, no Audible-based provider (12).

**Changes in v1.11:** 2.1 is released for Linux on Nov 16, 2026 (issue #63): album versions and editions, migration 9 and how a version is chosen (section 7, *Album versions and editions*; section 9).

**Changes in v1.10:** 2.0 is released for Linux on Nov 16, 2026: the music issues (#7 to #12) are built, each section noting what was built and how it was checked (sections 5.7, 7, 8, 9, 10, 11).

**Changes in v1.9:** Music for 2.0 (issue #6): scope and formats (section 2), the music data model and its migration, the music scan pipeline, music metadata providers, artwork order and the privacy setting (7), library content types (6), the play queue, gapless playback and the bit-perfect path (5.7), bridge additions (8), the Music section (9), the bundle (10), the 2.0 plan (11) and the maintainer's decisions for music: online lookups by default, user-supplied keys for TheAudioDB and Fanart.tv, no ALSA device permission in the Flatpak, 2.0 waits for all seven issues, two mpv handles (12).

**Changes in v1.8:** 1.6.1 (Nov 16, 2026, issue #52): Settings shows whether a TMDB key is stored, where it comes from and when TMDB last accepted it, with Replace, Test and Remove; the core looks for the key before it answers; a missing keystore is reported instead of being silent. Under Flatpak the Secret Service is reached directly over D-Bus, not through a portal (sections 6, 7).

**Changes in v1.7:** 1.6 is released for Linux on Nov 16, 2026: the show page's episodes are tiles with their own version, technical summary, details panel and Save menu (issue #48), loaded page by page (sections 8, 9); one Save menu on the movie and episode pages too; a version whose source is offline plays its saved copy (7, 9).

**Changes in v1.6:** 1.5 is released for Linux on Nov 16, 2026: offline copies ("Save"), "Save as…" and the download engine behind them (section 7), read priorities per source (6), the NoSpace error category (8), Qt Quick Dialogs on the module allowlist (3), the Downloads screen (9).

**Changes in v1.5:** 1.2 is released for Linux on Nov 15, 2026: technical details from a media probe in the Rust core (FFmpeg's libavformat loaded at runtime, sections 3, 7, 10) and a version selector for movies with several files (section 9).

**Changes in v1.4:** 1.1 is released for Linux on Nov 12, 2026: SFTP, the logo, cover sizes per display mode, one item per show with the repair of older libraries, and the show page (#32). The Windows release has no version number yet (GitHub milestone "Windows"): it needs GitHub Actions minutes or a Windows build machine for its packages (sections 2, 11).

**Changes in v1.3:** HDR passthrough on Windows (direct mode) leaves 1.1 and is postponed without a planned version (milestone "Unknown"), because no Windows PC with an HDR display is available for its tests. Until then Windows plays HDR10 tone-mapped in embedded mode, like Linux (sections 2, 5.2, 11, 12).

**Changes in v1.2:** 1.0 is released for Linux on Nov 11, 2026. Windows moves to version 1.1 with its own milestone, M5: the Windows build, direct mode with HDR passthrough, MSIX and winget, code signing and Windows media keys. SFTP in the app moves to 1.1 as well (sections 2, 11, 12).

**Changes in v1.1** (from the [M0 spike report](spike/M0-report.md)): Qt OpenGL added to the module allowlist (section 3); direct mode limited to Windows for LGPL builds (5.2); playback rules for the embedded item and the `coyotl://` protocol (5.3, 5.4); adaptive read-ahead after seeks (5.4); libmpv bundle contents and pinned versions (10); M0 status, one new open decision and an updated risk (11, 12).

## 1. Review of the first draft

The split into a headless core, a separate UI and libmpv holds. This revision replaces Flutter with Qt Quick, because the project is desktop-only and playback quality comes first, and it corrects eight further points.

| Draft v0.1 says | Problem | Decision |
| --- | --- | --- |
| Flutter UI over Dart FFI | Video must pass through Flutter's compositor as a texture, with no route to HDR, and desktop is Flutter's least mature target. | Qt 6 with QML. A C++ shell owns libmpv directly, which allows an embedded and a native-window video path (section 5). |
| "Split-process boundary", "zero-cost FFI" | FFI runs in one process, and bridge calls convert their arguments. | One process. The C++ to Rust bridge carries commands and events only, never video frames or file bytes. |
| Hand-written C ABI with `c_char` callbacks | Untyped and unsafe. | A typed bridge generated by the `cxx` crate (section 8). |
| `smbc` crate for SMB | It binds Samba's libsmbclient, which is GPLv3. Shipped binaries would become GPLv3. | Pure-Rust `smb` crate (section 6). |
| "libmpv or FFmpeg" | Building on FFmpeg directly means writing a player: A/V sync, seeking, subtitles, HDR, caching. | libmpv only, in its LGPL configuration. |
| Textures from "the libmpv swapchain" | libmpv has no swapchain. Its render API draws into a target the host supplies. | mpv's OpenGL render API inside a Qt Quick item, plus a native-window mode for HDR. |
| 64 MB ring buffer in Rust | mpv already has a demuxer cache with seek-back. A second buffer doubles memory and fights mpv's seeking. | mpv owns the playback cache. Rust serves byte ranges through mpv's stream callbacks. |
| tokio or rayon; SQL on the worker pool | rusqlite is blocking, and concurrent writers contend. | tokio for I/O, one dedicated database writer thread, a small read pool, WAL mode. |
| Identical pixels on desktop and mobile; guaranteed 60 to 120 fps | Mobile is out of scope. Frame rate is a budget to measure, not a guarantee. | Desktop layouts for windowed and fullscreen use; performance budgets checked in CI (section 9). |

Missing from the draft and added here: licensing, credential storage, the scanner and job model, a database schema, audio and subtitle handling, packaging per platform, testing, and a roadmap.

## 2. Scope and design principles

Huehuecoyotl is a desktop player for Windows and Linux with local and network playback, a scanned library and TMDB metadata. Versions 1.0 to 1.6 are released for Linux; the Windows release follows without a version number yet. Version 2.0 adds music (below). Mobile and TV platforms are out of scope.

| Tier | Platforms | Status |
| --- | --- | --- |
| 1 | Linux x64 (Wayland and X11) | Released with 1.0 |
| 1 | Windows 10/11 x64 | Built in CI since M4; released with 1.1 |
| 2 | macOS (universal), Linux arm64, Windows arm64 | Compiles in CI; released after 1.0 |

**Feature staging**

| Stage | Features |
| --- | --- |
| 1.0 (Linux, released) | Playback of common containers and codecs with hardware decoding; embedded and sidecar subtitles; audio track selection; resume and watched state; library scan; TMDB matching for movies and series; SMB, NFS, WebDAV and FTP sources; keyboard, mouse and remote-style navigation; MPRIS media keys; Flatpak |
| 1.5 (Linux, released) | Saving offline copies of movies, episodes, seasons and shows from network sources, played without the network; a Downloads screen and storage settings; "Save as…" to a chosen file or folder |
| 1.6 (Linux, released) | The show page's episode tiles: version, technical summary, details and the Save menu on every episode; one Save menu on movie and episode pages |
| 1.2 (Linux, released) | Technical details of every file (codecs, HDR, audio and subtitle tracks, bit rates), analysed in the background; choosing between several versions of a movie or episode |
| 1.1 (Linux, released) | SFTP sources; the logo; cover sizes per display mode; one item per show, with the repair of libraries from older versions; the show page with season switching and episodes not on disk |
| Windows (no version yet) | The Windows build; MSIX and winget packages; code signing; Windows media keys (SMTC) |
| 2.0 (Linux, released) | Music: a music library from tags (FLAC, ALAC, WAV, AIFF, MP3, AAC, Ogg Vorbis, Opus, WavPack), Albums and Artists, a play queue with gapless playback and a mini player, Hi-Res badges, MusicBrainz and the Cover Art Archive, optional TheAudioDB and Fanart.tv, a bit-perfect output mode (see **Music in 2.0** below) |
| 2.1 (Linux, released) | Music: versions and editions of an album (several formats, sources or a remaster) and of a track, chosen on the album page, with the preferred version and "This is a different album" (issue #63) |
| 2.5 (Linux, released) | Audiobooks: books from single files or folders (M4B, M4A, MP3, Ogg Vorbis, Opus, FLAC) with chapters, authors, narrators and series; local metadata (OPF, desc.txt, reader.txt, .nfo, tags, names); Open Library and MusicBrainz; a category with Books, Authors, Series and Narrators; a player with exact resume across files, chapters, speed and a sleep timer (see **Audiobooks in 2.5** below) |
| 2.6 (Linux, released) | Music redesign: a top menu (Albums, Artists, Songs, Genres, Playlists), the Quality lens (Hi-Res, Lossless, Several versions, Saved), format ribbons, a letter rail, an Artists preview panel, a Songs list with a quality column, smart and own playlists, a new Now Playing with a waveform and the signal path as a chain, and a full-screen Now Playing (issues #78 to #83) |
| Unscheduled | HDR passthrough on Windows through direct mode (needs a Windows PC with an HDR display for tests W1–W8) |
| After 1.1 | HDR passthrough on Linux; audio passthrough polish; Jellyfin, Emby and Plex as sources; subtitle download; multiple profiles; sync of watch state between devices; trailers; collections |
| Non-goals | Phones, tablets and TV boxes; DRM streaming services; transcoding or acting as a media server; SMB1; cloud accounts run by the project |
| Not in 2.0 (music) | Playlist files (M3U, PLS), cue sheets, DSD, audiobooks and podcasts, lyrics, music streaming services, editing tags, acoustic fingerprinting, crossfade, equaliser, visualisations, upsampling, room correction, multichannel bitstream for music |

**Music in 2.0** (issues #6 to #12)

- **Formats.** FLAC, ALAC (in MP4/M4A), WAV, AIFF, MP3, AAC (MP4/M4A and ADTS), Ogg Vorbis, Opus and WavPack. The file extensions the music scanner looks at: `.flac`, `.m4a`, `.mp4` (audio only), `.wav`, `.aif`, `.aiff`, `.mp3`, `.aac`, `.ogg`, `.oga`, `.opus`, `.wv`.
- **Lossless** means FLAC, ALAC, PCM (WAV, AIFF) and WavPack in its lossless mode. **Hi-Res** means lossless with more than 16 bits per sample or a sample rate above 48 kHz; the badge sits on a track when its file is Hi-Res and on an album when all of its tracks are. Lossy files are never Hi-Res, whatever their sample rate.
- Music shares the library, the network sources, the job queue, the offline copies and the player with video; what differs is the scan pipeline (tags, not file names), the metadata providers and the playback model (a queue that plays while the user browses).
- Where each issue is specified: the music library from tags (#7) in section 7, *Music library* (data model, scan pipeline); playback, the queue and Now Playing (#8) in 5.7 and 9; bit-perfect output (#9) in 5.7; the Music section (#10) in 9; MusicBrainz and the Cover Art Archive (#11) and TheAudioDB and Fanart.tv (#12) in section 7, *Providers*; content types of library folders in 6; formats in the bundle in 10; the order of work in 11; open decisions in 12.

**Audiobooks in 2.5** (issues #64 to #67)

- **What a book is.** One audiobook, from one place: a single file (an M4B, or any one audio file), a folder of audio files, or a folder with disc subfolders (`CD1`, `Disc 2`). Common layouts are recognised: `Author/Title/`, `Author/Series/01 - Title/`, `Author/Series/Book 2.5 - Title/`, and `Title - Author.m4b` flat in a folder.
- **Formats.** M4B and M4A (AAC or ALAC, with or without chapters), MP3, Ogg Vorbis, Opus and FLAC; the extensions `.m4b`, `.m4a`, `.mp4` (audio only), `.mp3`, `.ogg`, `.oga`, `.opus`, `.flac`. The bundle decodes all of them already (section 10).
- **Not in 2.5:** DRM-protected files (Audible AAX and AAXC, which are recognised and listed as not playable, never decrypted), ebooks, podcasts, audio dramas as a category of their own, syncing progress with other services or devices, bookmarks and notes, text-to-speech, splitting a single long file into chapters by silence, and an Audible-based metadata provider (decided in v1.12, section 12).
- Audiobooks share the library, the network sources, the job queue, the offline copies, the probe and the mini player with video and music. What differs: the scan pipeline (books, not tracks), the metadata providers (Open Library, and MusicBrainz for tagged books), and playback (one continuous timeline per book, exact resume, chapters, speed and a sleep timer, in an audio handle of its own).
- Where each issue is specified: the scope and decisions (#64) here and in section 12; the audiobook library (#65) in section 7, *Audiobook library*, and section 6 (content type); metadata and covers (#67) in section 7, *Book providers*; the category and the player (#66) in 5.8 and section 9; formats in section 10; the plan in section 11.

**Principles**

1. Do not write a player. mpv does demuxing, decoding, sync, subtitles and caching.
2. The core is headless. Every feature except video output works from a command-line harness without the Qt UI, which makes it testable.
3. C++ only where Qt or mpv APIs are used directly; Rust everywhere else. The C++ layer holds no business logic and does no I/O.
4. The dependency tree stays permissive. Official builds link nothing GPL, and CI enforces it.
5. The library is offline-first. The UI reads only from SQLite; the network never blocks a screen.
6. Sources and metadata providers are plug-ins behind one trait each.

## 3. Licensing strategy

Apache 2.0 for the project's own code is workable if Qt, libmpv and FFmpeg are used under their LGPL terms and linked dynamically. This is an engineering summary, not legal advice.

| Component | Licence | Consequence | Decision |
| --- | --- | --- | --- |
| Project code (C++, QML and Rust) | Apache-2.0 | Permissive, with a patent grant | Keep; require DCO sign-off on contributions |
| [Qt 6](https://doc.qt.io/qt-6/licensing.html) | LGPLv3 for most modules; some add-on modules are GPL-only in the open-source edition | Using one GPL-only module makes the binary GPL. The Qt FAQ names Qt Charts, Data Visualization and Virtual Keyboard as examples. | Use only LGPL modules: Core, Gui, OpenGL, Qml, Quick, Quick Controls, Quick Dialogs (LGPLv3, part of qtdeclarative; the native file and folder dialogs since 1.5, through the portal under Flatpak), Svg, DBus. Link dynamically. OpenGL is needed because the embedded video item's renderer returns a `QOpenGLFramebufferObject` (spike finding). |
| [libmpv](https://github.com/mpv-player/mpv/blob/master/Copyright) | GPLv2+ by default; LGPLv2.1+ when built without the GPL-only files (`-Dgpl=false`) | LGPL mode drops the X11 video output, VDPAU and a few minor features. Linked libraries can still change the result. | Own LGPL build, shipped as a separate shared library. In mpv 0.42 the entire X11 backend is GPL-only, so the LGPL build has no X11 support at all (see 5.2). |
| FFmpeg | LGPLv2.1+ unless configured with GPL or non-free parts | A GPL-configured FFmpeg makes the whole libmpv build GPL | Configure without GPL or non-free options; decoders only |
| mpvqt (reference only, not linked) | LGPL-2.1 or LGPL-3 | None, as long as no code is copied | Not a dependency; the video item is written from mpv's API documentation |
| Samba libsmbclient (`smbc` crate) | GPLv3 | mpv's own notes say Samba support makes a build GPLv3+ | Not used |
| [`smb` crate](https://docs.rs/crate/smb/latest), [`nfs-rs`](https://docs.rs/crate/nfs-rs/latest/source/README.md), [Apache OpenDAL](https://opendal.apache.org/core), `cxx` | Permissive (nfs-rs and OpenDAL are Apache-2.0; confirm the others in the audit) | None | Used |
| [TMDB API](https://developer.themoviedb.org/docs/faq) | API terms, not a software licence | Free for non-commercial use with attribution; charging for the app makes it commercial use | See section 7 |

**LGPL obligations for official builds** ([Qt's summary](https://www.qt.io/faq/3.7.-what-are-my-obligations-when-using-qt-under-the-lgpl))

- Ship Qt, libmpv and FFmpeg as replaceable shared libraries, never statically linked. The media probe (section 7) uses the same FFmpeg libraries from the Rust core: it loads them at runtime (`libloading`, ISC) with bindings generated for the shipped major version, so nothing new is shipped and nothing links FFmpeg statically.
- Include the licence texts and make the exact sources and build recipes available under the project's own control, not only as links to upstream.
- Users must be able to run the app against their own modified build of those libraries. On desktop systems, ordinary shared libraries satisfy this.

**Enforcement in CI**

- `cargo-deny` with a licence allowlist fails the build on any GPL crate.
- A CMake check fails the build if a Qt module outside the allowlist is linked.
- The libmpv build job asserts `-Dgpl=false` and checks the FFmpeg configuration string.
- Third-party notices for the About screen are generated from the Rust crates and from Qt's own attribution files.

Distribution packagers may still link a system mpv that is a GPL build. That is allowed, since Apache-2.0 code can be combined into a GPLv3 work, but the resulting package is then GPL as a whole.

Two further points sit outside copyright licensing. Other companies' product names must not appear in the project name, branding or documentation. Codec patents (for example HEVC, AC-3, DTS) are separate from software licences; prefer OS and hardware decoders and take advice before distributing through app stores.

## 4. Revised system architecture

Everything runs in one process: a thin C++ and QML shell on top, a Rust core underneath, and libmpv beside them. Three paths are kept apart: control over the bridge, file bytes from the core to libmpv, and frames from libmpv to the screen.

&#91;embedded content: system architecture · UI, bridge, Rust core, libmpv\]

The highlighted box is libmpv, shipped as a separate LGPL library. It pulls bytes from the core through stream callbacks and draws frames into the Qt Quick scene; the bridge on the left carries only small typed messages.

**Which language where**

| Part | Language | Reason |
| --- | --- | --- |
| Screens and components | QML | Declarative UI with hot reload |
| App shell, list models, OS integration (media keys, idle inhibit, window handling) | C++ | These are Qt APIs. C++ is their native language, and the tooling (moc, qmllint, the debugger) assumes it. |
| Video item and mpv control | C++ | mpv's client and render APIs are C, Qt's scene-graph hooks are C++, and the existing reference code is C++. |
| Library, scanner, metadata, sources, stream reader | Rust | No Qt dependency. This code parses untrusted network input, where memory safety matters most, and the protocol clients exist as crates. |
| Bridge | Rust, with C++ generated by `cxx` | Small and typed (section 8) |

The C++ layer should stay at a few thousand lines. If logic starts to accumulate there, it belongs in the core.

**Threading model**

| Thread or pool | Runs | Rule |
| --- | --- | --- |
| Qt GUI thread | QML, models, `MpvPlayer` | Never blocks; results from the core arrive as queued events |
| Qt Quick render thread | Scene graph and mpv's render calls in embedded mode | No application logic |
| tokio runtime | Network I/O, sources, TMDB client | No blocking calls; CPU work such as hashing and image scaling goes to `spawn_blocking` |
| Database writer thread | All writes, fed by a channel | Exactly one writer; SQLite in WAL mode |
| Database read pool | UI queries | Read-only connections |
| mpv threads | Demuxing and decoding | Owned by libmpv; they call the Rust stream callbacks, which enter the tokio runtime context and block on it |

A crash in native code takes the whole app down, as in any in-process design. The mitigations are a thin C++ layer, a memory-safe core, and crash-safe state: SQLite transactions and a persisted job queue.

## 5. Playback subsystem

The C++ shell owns the mpv handle directly and offers two video paths: an embedded Qt Quick item on every platform, and a native-window mode on Windows for HDR passthrough.

**5.1 Ownership**

- `MpvPlayer` is a C++ `QObject`. It creates the mpv handle, sets options, and observes properties such as position, duration, tracks, pause and cache state. It exposes them to QML as properties and signals.
- mpv's wake-up callback posts to the Qt GUI thread, so nothing polls.
- The Rust core never links libmpv. It supplies the stream callbacks and the playback ticket (file, resume position, sidecar subtitles) and records progress.
- [mpvqt](https://jellyfin.org/posts/state-of-the-fin-2026-01-06) is a KDE library that wraps libmpv for Qt Quick; Jellyfin Desktop builds on it and it needs Qt 6.5 or newer. Decision: the project writes its own video item and does not depend on mpvqt. mpvqt's item is built on the OpenGL framebuffer path and keeps the mpv controller inside that item, so it covers embedded mode only, while direct mode needs the same mpv handle to drive a native window. The embedded item is a few hundred lines, so the dependency would save little and add one more library to build and ship on Windows. mpvqt stays a reference for behaviour; none of its LGPL code is copied.

**5.2 Two video paths**

| Mode | How it works | HDR | Platforms | Milestone |
| --- | --- | --- | --- | --- |
| Embedded | mpv's OpenGL render API draws into a framebuffer inside a Qt Quick item; QML controls are composited on top | Tone-mapped to SDR | Windows, Linux (X11 and Wayland), macOS | M1 |
| Direct | mpv renders with its own GPU output into a native child window; QML controls sit in a transparent overlay window above it | Passthrough, with mpv's own frame timing | Windows only in official builds; X11 only with a GPL libmpv (distribution packages); Wayland open | Unscheduled (was M4, then 1.1) |

- Embedded mode requires Qt Quick itself to run on OpenGL, set once at start-up before the first window. Qt's default on Windows is Direct3D 11, and mpv's render API offers only OpenGL and software targets. For the same reason Qt's newer [QQuickRhiItem](https://doc.qt.io/qt-6.11/qquickrhiitem.html) cannot be used.
- Direct mode needs careful handling of focus, resizing and stacking between the two windows. The window structure and lifecycle are fixed in the [direct mode design](spike/direct-mode-design.md): the video window owns the overlay as a transient (owned) top-level window that covers the whole client area and takes all input.
- Direct mode on Linux: mpv's window embedding does not work on Wayland, and on X11 it needs mpv's X11 backend, which is GPL-only. The official LGPL build therefore has no direct mode on Linux; the spike confirmed that `vo=gpu-next` cannot create a GPU context there.
- Embedded mode rules found in the spike: load a file only after the render context exists (`vo=libmpv` fails before that); pass the screen's refresh rate as `display-fps-override`, because the render API cannot see the display; call `mpv_render_context_report_swap` on every `frameSwapped`.
- HDR is not automatic with Qt. Jellyfin's Qt and mpv player is a working precedent, yet the developer of its successor [cited HDR support and Qt maintenance](https://github.com/andrewrabert/jellium-desktop/issues/609) as reasons for starting a rewrite. Direct mode is therefore prototyped in the spike, not assumed.

**5.3 Getting bytes to mpv**

- Local files: mpv opens the path directly.
- Network sources: the app registers a custom protocol with mpv's stream callback API. The open, read, seek, size and close functions are implemented in Rust and exported with C linkage, and mpv's threads call them directly.
- Sidecar subtitles and multi-part files use the same protocol. Add them with `sub-files-append` or `sub-add`, never `sub-files`: that option is a `:`-separated list and splits `coyotl://` URLs.
- mpv's threads are not tokio threads. Every callback enters the tokio runtime context before touching a source, since protocol handles may need it even to drop, and catches panics at the boundary (section 8, rule 7).

This replaces the loopback HTTP server of the previous revision: there is no socket, port or token. The command-line harness can still expose the same streams over loopback HTTP, so sources can be tested with a stock mpv or curl.

**5.4 Buffering**

mpv's demuxer cache replaces the draft's Rust ring buffer.

| Setting | Proposed default | Purpose |
| --- | --- | --- |
| `cache` | yes for network sources, set per file | Enable the demuxer cache. mpv does not count stream-callback protocols as network, so `cache=auto` leaves it off for `coyotl://`; the player sets `cache=yes` explicitly. |
| `demuxer-max-bytes` | 256 MiB, user-configurable | Forward buffer for high-bitrate UHD files |
| `demuxer-max-back-bytes` | 64 MiB | Instant short seeks backwards |
| `hwdec` | `auto-safe` | Hardware decoding where it is known to be reliable |

The Rust side adds only what mpv cannot: sequential read-ahead in chunks of a few MiB per source, a small block cache for the index regions players seek to at the start, and transparent reconnect with retry so a short network drop becomes a pause, not an error.

Read-ahead is adaptive: after a seek, fetch a small first block and widen the window only while reads stay sequential. The spike fetched a full 4 MiB chunk after every seek, which cost 44 ms per random seek even on a fast link. Reconnect with exponential backoff was verified in the spike: an 8.6-second server outage showed as Buffering and playback resumed by itself.

**5.5 Audio, subtitles and state**

- Audio output through WASAPI on Windows and PipeWire or PulseAudio on Linux. Bitstream passthrough is opt-in per device.
- Subtitles render through mpv (libass): embedded tracks plus sidecar files found by the file system layer under the same base name.
- Remembered per item: position, watched flag. Remembered per series: audio and subtitle language choice.
- States: Idle, Opening, Buffering, Playing, Paused, Ended, Error. The UI derives every control from this one state property.

**5.7 Music playback (2.0, issues #8 and #9)**

- **Two mpv handles.** Music plays in a second, audio-only mpv handle (`vid=no`, `vo=null`, no render context) owned by a C++ `MusicPlayer` next to `MpvPlayer`. The video player keeps its own handle and lifecycle. Both handles load the same bundled libmpv.
- **Queue in the core.** The core owns the play queue: its entries (track items and the chosen file), the order, shuffle (a permutation stored with the queue, so turning shuffle off returns to the original order), repeat (off, one, all), "play next" and "add to queue". The queue, its position and the playback position are stored in the database and restored after a restart (paused). The C++ shell only feeds mpv: it holds the current entry and the following one in mpv's playlist, reports when mpv moves on, and asks the core for the next entry. Play counts are recorded per track when a track has played to 50 % or for four minutes, whichever comes first.
- **Gapless.** `gapless-audio=weak` and `prefetch-playlist=yes`, with the following entry already in mpv's playlist. Consecutive tracks with the same sample rate and channel layout play without a gap; at a format change mpv reopens the output, which the user may hear. A test plays two fixture files that hold one continuous sine tone into mpv's file output (`ao=pcm`) and fails on a gap or a discontinuity.
- **Network tracks** stream through `coyotl://` like video, with a smaller demuxer cache (`demuxer-max-bytes` 32 MiB, `demuxer-max-back-bytes` 8 MiB), so the next track can be prefetched while the current one plays.
- **ReplayGain:** off, track or album (mpv's `replaygain`), from the tags the scanner stored.
- **Music and video.** Starting a video pauses music and keeps the queue; music does not resume by itself when the video ends. Starting music while a video plays is not offered (the player page is in front). The screensaver is not inhibited while only audio plays. MPRIS (and SMTC in the Windows release) shows the player that plays: track, artist, album and cover for music.

*As built (issue #8).* The queue logic is plain Rust without I/O (`crates/library/src/queue.rs`: `Queue` with the original order, the play order as a permutation of it, the position, shuffle and repeat), unit-tested on its own; it is stored in `play_queue` and the settings `queue_pos`, `queue_current`, `queue_shuffle`, `queue_repeat` and `queue_position_ms`. In the core one task owns the queue and runs the operations one at a time in call order (`crates/core/src/playqueue.rs`). Every change answers with `QueueChanged` (the count, the position, the modes and up to 500 entries around the current one) and, where the player has to act, `MusicTickets`: "play" (load the current entry, from a position after a restart, with the following one behind it), "next" (replace only what follows the current entry) or "stop". Tickets of the queue are prepared like the video player's (files on this computer, then saved copies, then network files) from a range of their own. `MusicPlayer` (C++, the QML singleton `Music`) keeps those two entries in mpv's playlist by their playlist entry ids, sets the start position per entry (`loadfile … start=`), and on mpv's start of the following entry removes the finished one and reports `queue_advanced`; the core takes that only from the ticket it handed out as the following one. If a track ends before the ticket of the next one arrived (a track shorter than the time to prepare a network ticket, about 150 ms on the SMB test server), the player waits for it instead of stopping. Previous within 3 s of a track's start goes to the track before, later it restarts the track; Next with repeat one still moves on. Play after the end of the queue goes on with what was added since, else starts the queue over. Progress goes to `music_progress` every 5 s, on pause and at a track's end: it keeps the position for a restart and counts a play (`playstate.play_count`, `last_played_at`) once per ticket at half the track or after four minutes. After a restart the queue is shown paused and nothing loads until Play. The ReplayGain choice is a UI setting (Settings → Playback, kept in `ui.ini`) that sets mpv's `replaygain` on the music handle. Media keys reach the queue through MPRIS (Play, Pause, Next, Previous, Seek, SetPosition) and, while the shell is in front, as keys. Tests: the queue logic and its storage (library), the queue in the core without Qt (`crates/core/tests/music_queue.rs`: tickets, advancing, play next, repeat one, play counts, restart, play after the end, remove, clear), and `tests/tst_musicplayback.cpp` through the bundled libmpv with the music handle's options (`app/src/musicoptions.h`) into `ao=pcm`: a 441 Hz sine split at a sample boundary into two FLAC files plays back sample for sample (a 5 ms gap between them fails it), and one fixture per format (FLAC, ALAC, WAV, AIFF, MP3, AAC in MP4 and ADTS, Ogg Vorbis, Opus, WavPack) decodes to about a second of sound. Checked by hand over SMB: two tracks of one tone join sample for sample through `coyotl://`.
- **Bit-perfect mode (issue #9)**, a setting under Settings → Audio, off by default: no forced sample rate or format, no audio filters, software volume fixed at 100 %, ReplayGain off, and exclusive access where the output offers it (`audio-exclusive=yes`). The output device is chosen in the same place (mpv's `audio-device`; the list from `audio-device-list`). On Linux the choices are PipeWire (which switches its rate only when its allowed rates are configured and no other stream plays; the user guide documents the configuration) and, outside the Flatpak, direct ALSA hardware devices, the only truly exclusive path. The Flatpak gets no device permission for that (decided in v1.9, section 12): there bit-perfect mode uses PipeWire, and the user guide explains the difference. Windows uses WASAPI exclusive mode.
- **Signal path.** Now Playing shows the source format (`audio-params`) and the output format (`audio-out-params`). It says "Bit-perfect" only when sample rate and sample format match, no filter is active and the output is exclusive; in shared mode it shows the output rate and that the system mixer may resample. A device that rejects a format falls back to one it supports, with a visible note.

*As built (issue #9).* `MusicPlayer` has `bitPerfect`, `audioDevice` (mpv's `audio-device`, "auto" for the system's), `devices` and `volume`, kept in `ui.ini`; Settings → Audio has "Bit-perfect music" and "Music output" (videos keep the system default). Bit-perfect mode (`setBitPerfect` in `app/src/musicoptions.h`) sets `audio-exclusive=yes`, clears `af`, turns `replaygain` off and the volume to 100 % (the volume buttons on Now Playing and MPRIS volume do nothing then); off restores shared access, the ReplayGain choice and the volume. mpv forces no rate or format in either mode, and `gapless-audio=weak` reopens the device when the format changes between tracks. mpv lists no `alsa/hw:` devices: the hardware itself appears as `alsa/front:`, `alsa/hdmi:` and `alsa/iec958:` ("direct" in the list; `hw:` counts too when named by hand); `plughw:` converts, `dmix:` and `sysdefault:` mix, and the ALSA plug-ins are not offered. The list offers the system default, the sound server's outputs (PipeWire, else PulseAudio), direct ALSA devices (not in the Flatpak, which cannot reach them; section 12) and WASAPI or Core Audio devices. The signal path comes from mpv's `audio-params`, `audio-out-params`, `af` and `current-ao` (`app/src/audiopath.h`): exclusive means bit-perfect mode on a direct ALSA device, WASAPI or Core Audio; "Bit-perfect" needs exclusive, no filter, and the same rate, sample format (planar or not) and channel count in and out. Now Playing shows the source format, "Output: rate · bit depth · output" (and "exclusive"), the badge, and a note: shared output ("the system mixer may resample"), filters, or the rate or format the device took instead. When the chosen device cannot be opened (mpv: "Could not open/initialize audio device"), the player switches to the default device in shared mode, plays the track again from where it was, and says so in red. Tests (`tests/tst_musicplayback.cpp`): a 24-bit, 96 kHz FLAC (tone and noise) played in bit-perfect mode into `ao=pcm` is byte for byte FFmpeg's reference decode, and at 50 % volume it is not; the rules for exclusive, bit-perfect and the offered devices. The manual checklist for a USB DAC is `docs/testing/hires-audio.md`. While building this, a second `MusicPlayer` turned up: QML built its own for the default-constructible singleton instead of calling `create()`, so two audio handles reacted to every ticket; the constructor now needs a parent.

**5.8 Audiobook playback (2.5, issue #66)**

- **An audio handle of its own.** Books play in a third mpv handle, audio-only like the music handle (`vid=no`, `vo=null`), owned by a C++ `BookPlayer` (decided in v1.12, section 12). The music queue and its handle are not touched by a book. Starting a book pauses music and keeps its queue and position; starting music pauses the book and saves its position. Video pauses both. The mini player, MPRIS and the media keys show what played last: for a book its title, the current chapter and the author, with its cover. The screensaver is not inhibited for books, as for music.
- **One timeline per book.** A book's files, in their order, form one timeline (`book_file.start_ms`); a position is a point on it. The core turns a timeline position into (file, offset) and back (unit-tested without Qt); `BookPlayer` loads the book's files as one mpv playlist with `gapless-audio=yes` and `prefetch-playlist=yes`, so a multi-file book plays without gaps between files, and starts at the file and offset of the saved position. Network files stream through `coyotl://` with the music handle's smaller cache.
- **Resume.** The position on the timeline is saved every 5 s while playing, on pause, on seek, on chapter change, on exit and when the sleep timer stops playback (`playstate.position_ms`, with `last_played_at`); a book resumes within one second of where it stopped, also inside a later file. A book counts as finished at its last 30 s or by "Mark as finished" (`playstate.watched`); finishing does not reset the position until the book is played again.
- **Chapters.** Previous and next chapter (previous within 3 s of a chapter's start goes to the one before, later to the chapter's start), a chapter list, and the current chapter's title and the time left in it. Chapter lookup is a binary search over the book's chapters (core, unit-tested).
- **Skip.** Back 15 s and forward 30 s by default; both configurable (5 to 120 s).
- **Speed.** 0.5× to 3.0× in steps of 0.1, pitch kept by mpv's `scaletempo2` audio filter (`af=scaletempo2`, the default for speed changes; set explicitly). Remembered per book (`book.speed`); new books use the default from Settings. Bit-perfect mode (5.7) does not apply to books.
- **Sleep timer.** 5 to 120 minutes, or "end of chapter". The core computes when to stop (unit-tested); `BookPlayer` fades the volume out over the last 5 s, pauses, restores the volume and saves the position.
- *As built (issue #66).* `app/src/bookplayer.{h,cpp}` with its options in `bookoptions.h` (`gapless-audio=yes`, `af=scaletempo2`); the QML singleton `Book`. It loads the files from the one holding the start position on (`start=` on that file), so a seek into another file reloads from there; positions mpv reports during a reload are ignored until the file is loaded. What it needs from the core comes through a `BookBackend` (ticket request, progress, speed, the chapter arithmetic, the `coyotl://` registration), set by `main.cpp` from `CoreBridge` and by `tests/tst_bookplayback.cpp` from a stub. A book in its last 30 s counts as finished; the next Play starts it over. "Front": whichever of book and music started last is shown by the mini player and MPRIS (for a book: the chapter as title, the book as album, the authors as artist; next and previous are chapters); starting music pauses the book and the reverse. The sleep timer counts playing time only. Save and Save as… on a book copy all of its files (one saved copy per file; `copies::copy_owners` lets pause, resume and delete find them).

**5.6 Direct mode on Wayland**

There is no clean way today to show mpv's HDR output inside a Qt window on Wayland. Version 1.0 uses embedded mode there, and three routes exist for later.

The cause is structural. On Windows and X11 an application can hand mpv a window handle to draw into. Wayland deliberately has no global window handles, and mpv has no equivalent option. mpv does support HDR on Wayland [in its own window](https://github.com/mpv-player/mpv/releases), through the colour-management protocol on compositors that implement it, such as KDE Plasma and GNOME. Separately, the render API that embedded mode uses is OpenGL-only and lacks mpv's newer renderer; a [request to change that](https://github.com/mpv-player/mpv/issues/10810) has been open since 2022.

| Option | How it works | Controls | Cost | Assessment |
| --- | --- | --- | --- | --- |
| A. Embedded only | As on every platform in 1.0 | QML | None | The default. HDR content is tone-mapped. |
| B. Separate mpv window | For HDR titles, mpv opens its own fullscreen Wayland window and the app window steps back | mpv's on-screen controller or a small script, not QML, because Wayland does not let an app stack its own overlay on another top-level window | Low | Realistic as an opt-in "HDR playback window" after 1.0. The player UI differs from the rest of the app. |
| C. Subsurface with a patched libmpv | The app creates a Wayland subsurface beneath its QML scene, and a patched libmpv renders into it with its normal GPU output | QML above the video | High: a libmpv patch to maintain or get accepted upstream | Best result, but a real engineering project |
| D. HDR through the Qt scene graph | mpv renders HDR frames into a texture through a non-OpenGL render API, and Qt Quick presents them on an HDR-capable surface | QML | Depends on upstream work in both mpv and Qt | Cleanest in the long run. A third-party libmpv patch with a Vulkan render API exists; watch upstream. |

Recommendation: A in 1.0, B as an opt-in afterwards if users ask for it, and a yearly check on C and D. Running the app under XWayland does not help, because HDR is not available there. The spike should re-check the upstream state, since this area moves quickly.

## 6. Network sources

Every source, local or remote, implements one small trait, and nothing above that layer knows which protocol it is talking to.

```rust
#[async_trait]
pub trait Source: Send + Sync {
    async fn list(&self, dir: &VPath) -> Result<Vec<Entry>>;
    async fn stat(&self, path: &VPath) -> Result<Entry>;
    async fn read_at(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes>;
    fn capabilities(&self) -> Caps; // range reads, change notifications, stable ids
}
```

| Protocol | Implementation | Notes | Milestone |
| --- | --- | --- | --- |
| Local and OS-mounted folders | `tokio::fs` plus file-system change notifications | Also covers shares the user has already mounted | M1 |
| SMB 2 and 3 | [`smb`](https://docs.rs/crate/smb/latest) 0.12.1, pure Rust | All SMB 2.x and 3.x dialects, NTLM and Kerberos, encryption, tokio. Still below 1.0, so pin the version and test against Samba and Windows servers. | M3 |
| NFS v3 and v4.1 | [`nfs-rs`](https://docs.rs/crate/nfs-rs/latest/source/README.md) 0.12, pure Rust | Servers that enforce privileged source ports need the `insecure` export option or elevated rights; document this for users. | M3 |
| WebDAV | `reqwest` (rustls) with a small PROPFIND client | Range requests map directly onto `read_at`. | M3 |
| FTP (FTPS later) | [`suppaftp`](https://docs.rs/suppaftp) on tokio | No ranged reads in FTP: the backend keeps one transfer open while reads are sequential and restarts it with `REST` on a seek. | M3 |
| SFTP | [`russh`](https://docs.rs/russh) with `russh-sftp` | Password or key-file login (Ed25519, ECDSA; the passphrase goes to the keystore). Host keys are trusted on first use: the dialog shows the fingerprint before the library is added, and a different key is refused later. Lives in `crates/vfs-sftp`, which the core links. `smb` 0.12.1 pins pre-release RustCrypto crates (through `sspi` 0.21.3) that clash with `russh`, so the build uses `third_party/smb`: the published crate with only `sspi` raised to 0.23.1, as in the upstream fix afiffon/smb-rs#189, checked in CI by `tools/check-smb-patch.sh`. It goes away once `smb` releases the fix. | M3 (in the app since 1.1) |
| Jellyfin, Emby, Plex | Their HTTP APIs | These bring their own metadata, so they bypass the scanner | After 1.0 |

OpenDAL was evaluated in M3 and dropped: its SFTP service runs the system `ssh` binary (not available on Windows by default) and its HTTP layer pulls in `aws-lc`. Small direct clients are less code than adapting it.

If a pure-Rust client proves unreliable, the C libraries libsmb2 and libnfs can sit behind the same trait after a licence check. Each backend is a Cargo feature, so a build can leave any of them out.

**Discovery.** mDNS and DNS-SD find NAS devices advertising SMB, NFS, WebDAV or SFTP. WS-Discovery finds Windows hosts. Manual entry of an address always works.

**Credentials.** Secrets go into the OS keystore through the `keyring` crate (its platform stores, never the in-memory mock store; a test checks this in debug and release builds): Windows Credential Manager, the Secret Service on Linux, Keychain on macOS. Under Flatpak the app talks to the Secret Service (`org.freedesktop.secrets`) directly over D-Bus; the manifest allows exactly that name. The database stores only a reference. A secret that cannot be stored (no keystore installed, a locked one) is never written anywhere else: it works until the app closes, and the UI says so. `tools/test-keystore.sh` checks that secrets survive a restart against a throwaway Secret Service (its own D-Bus session and gnome-keyring-daemon).

**Content type (2.0).** Every library folder, local or on a network source, has a content type: `video` or `music` (`source.content`, migration 8; existing folders become `video`). The scanner runs the video or the music pipeline by it. Adding a folder asks for the type; a folder holds one type, and a share with both is added twice, once per folder.

**Content type: audiobooks (2.5).** `source.content` gains `audiobooks` (migration 10 rebuilds `source` for the new CHECK; existing folders keep their type). The scanner runs the audiobook pipeline (section 7) for those folders only; adding a folder asks for Movies and shows, Music or Audiobooks.

**Connection handling**

- One connection per source, reconnected transparently. Reads are scheduled per source: playback reads outrank background reads (downloads and the media probe), which wait while playback is reading (at most two seconds at a time, so they never starve) and share a cap of two reads in flight per source.
- Exponential backoff on failures, with an offline state the UI can show per source.
- Files are stored as source id plus relative path, so a changed server address does not orphan the library.

**Testing.** CI starts Samba, NFS, WebDAV, SFTP and FTP servers in containers and runs the same conformance suite against each backend, including injected latency and dropped connections.

## 7. Library, database and metadata

The schema separates physical files from logical items, so one movie can have several versions and watch state survives a file being moved or renamed.

```sql
CREATE TABLE source   (id INTEGER PRIMARY KEY, kind TEXT, name TEXT,
                       config_json TEXT, credential_ref TEXT);

CREATE TABLE item     (id INTEGER PRIMARY KEY,
                       kind TEXT CHECK (kind IN ('movie','show','season','episode','video',
                                                 'artist','album','track')),  -- music: 2.0, migration 8
                       parent_id INTEGER REFERENCES item(id),
                       title TEXT, sort_title TEXT, year INTEGER,
                       season_no INTEGER, episode_no INTEGER,
                       tmdb_id INTEGER, imdb_id TEXT,
                       match_state TEXT,          -- matched | needs_review | unmatched
                       locked INTEGER DEFAULT 0,  -- user-corrected, never overwritten
                       meta_json TEXT, meta_fetched_at INTEGER);

CREATE TABLE file     (id INTEGER PRIMARY KEY,
                       source_id INTEGER REFERENCES source(id),
                       path TEXT, size INTEGER, mtime INTEGER,
                       fingerprint BLOB,          -- size + hash of first and last 64 KiB
                       probe_json TEXT,           -- tracks, resolution, HDR, bit rates (crates/probe)
                       item_id INTEGER REFERENCES item(id),
                       last_seen_scan INTEGER,
                       UNIQUE (source_id, path));

CREATE TABLE artwork  (item_id INTEGER, role TEXT, remote_path TEXT,
                       local_path TEXT, width INTEGER, height INTEGER);

CREATE TABLE playstate(item_id INTEGER, profile_id INTEGER,
                       position_ms INTEGER, duration_ms INTEGER,
                       watched INTEGER, updated_at INTEGER,
                       PRIMARY KEY (item_id, profile_id));

CREATE TABLE job      (id INTEGER PRIMARY KEY, kind TEXT, payload_json TEXT,
                       priority INTEGER, state TEXT, attempts INTEGER, run_after INTEGER);

CREATE TABLE local_copy(id INTEGER PRIMARY KEY, kind TEXT,  -- saved | export
                       main_id INTEGER,            -- subtitles and other parts point at the main file
                       file_id INTEGER REFERENCES file(id), item_id INTEGER REFERENCES item(id),
                       source_id INTEGER, source_path TEXT, src_mtime INTEGER,
                       path TEXT, size INTEGER, bytes_done INTEGER,
                       state TEXT,                 -- queued | running | paused | done | failed | outdated
                       error TEXT, error_kind TEXT, created_at INTEGER, completed_at INTEGER);

CREATE VIRTUAL TABLE item_fts USING fts5(title, original_title, overview);
```

- The fingerprint detects moves and renames without re-reading whole files.
- `updated_at` on play state allows last-writer-wins sync between devices later.
- Schema changes go through numbered migrations from the first commit.

**Scan pipeline**

1. Enumerate each source and diff against the `file` table by path, size and modification time.
2. Parse file and folder names into title, year, season, episode, resolution and codec tokens. Read `.nfo` files with explicit ids when present.
3. Group files into items; several files can attach to one item. A show is named by the folder above any season, release or per-episode folder ("Show/Season 1/…", "Show/Show.S01.2160p.WEB/…", "Show.S02E01.Title/…"); without such a folder, by the title in front of the season marker of a release folder, else by the file name. A folder that shares no word with the episode's own title is a category, not the show. Episodes group by a show key: the normalised title without a leading article; a country suffix ("US", "UK") stays in the key, and the year is compared separately (an unknown year matches any). When items are merged, their old keys stay in `group_alias`, so later scans find the surviving item.
4. Match against the metadata provider. A low-confidence match goes to a "needs review" list instead of becoming a wrong poster. For shows, a country suffix becomes an origin-country filter, shorter leading parts of the title are searched when the title finds nothing, and when several shows fit the title, the seasons and episodes on disk decide (a show with three seasons on disk cannot be a one-season show). After matching, the TMDB id is the identity: at most one top-level movie and one show per TMDB id (a unique index); an item that would take an id another item already has is merged into it, keeping both items' files and the newer watch state per episode. A user's locked choice always wins.
5. Fetch details and artwork; for shows, every season and episode TMDB lists, also the ones not on disk (virtual items for the show page). Downscale posters in Rust to the grid sizes the UI uses and keep the original for the detail page. Grid thumbnails come in two tiers, 360 px (`poster_grid`) and 540 px (`poster_grid_l`), both made from one w780 download. For libraries from before the 540 px tier, a background `tiers` job makes it from the stored originals without network access; until then the grid uses the 360 px tier.
6. Probe tracks and HDR flags when idle (issue #33): a low-priority `probe` job with its own worker analyses every library file with FFmpeg's libavformat in the Rust core (`crates/probe`). It reads through the file's `Source`, so network shares work like local folders, and only the start of a file plus what the container points at, capped at 24 MiB per file (a test asserts the bytes read). The result is `file.probe_json` with a schema version; changed files lose it in the scan and are probed again, as are files with an older schema. A file FFmpeg cannot read gets an entry with the error and is not retried until it changes; "Analyse now" moves a file to the front. When a file is played, what mpv reports (`track-list`, `video-params`) fills in files without a usable probe. Bit rates come from the stream or Matroska's statistics tags; the overall rate is size over duration, and a video rate derived from it minus the audio is marked as estimated. Atmos and DTS:X are shown only when FFmpeg's profile signals them. `coyotl-cli probe FILE` prints the result.

**Offline copies (1.5, issue #3).** "Save" keeps a copy of a network file in storage the app manages; "Save as…" writes one to a place the user picks. Both run on one engine in the core:

- Each copied file is a `local_copy` row (migration 7) and a `download` job with its own worker, two at a time. A copy is persisted, survives a restart and can be paused, resumed and cancelled by id.
- Data is read with `Source::read_at` in 1 MiB chunks, so every protocol works, and written to `<name>.part`. A failed read is retried with backoff (about 90 s) and continues from the last written byte; after a restart the `.part` file's length is where the copy continues. When the copy is complete, its size and fingerprint are checked against the library's before the file is renamed.
- Sidecar subtitles and the other parts of a multi-part file are rows of their own that point at the main file's row. A season or a show becomes one copy per episode (the largest network version of each).
- Managed storage is `Saved` in the data directory, or a folder chosen in the settings (the `setting` table, key `storage_dir`), laid out as `Title (Year)/original file name` and `Show (Year)/Season 01/original file name`.
- Free space is checked before a copy starts and every 64 MiB, keeping 32 MiB in reserve. Running out of space is the `NoSpace` error category and removes the `.part` file.
- A source file that changed in place (size or modification time) or disappeared makes its saved copy `outdated` (triggers on `file`). Playback uses a complete copy that is not outdated in place of the network file, and keeps the item's watch state; such a version stays playable while its source is offline.
- Deleting a copy removes its local files and rows, never the file on the source. Removing a library deletes its saved copies.
- At start-up, exports of the last session are forgotten (their `.part` files removed), unfinished copies are queued again, a saved copy whose file has gone is marked failed, and `.part` files in managed storage that no row knows are removed.
- In the UI (issue #4): the detail page offers Save (movies and episodes), Save season and Save all (shows) for items with a file on a network source. Seasons and shows have one button that changes from Save to Pause, Resume, Try again, Save again and Delete saved copies, beside Cancel and the progress. Movies, episodes and every episode tile on the show page (since 1.6, issue #48) have one Save menu instead: its button names the state (Save, Saving 38 %, Paused, Saved, Saving failed, Saved copy outdated) and its menu offers what fits it (Save, Pause or Resume, Cancel, Try again, Save again, Delete saved copy, Save as…); Save is offered for a version on a network source only, Delete saved copy only when a copy made with Save exists, never for an export. Deleting asks first and says the server's file stays. Saved items carry a mark on their cover and episode row, and Movies and Shows filter All, Unwatched or Saved. The Downloads screen lists copies in progress, failed or outdated, and done, with sizes and the space used; Settings → Storage sets the folder (a native folder dialog, the portal under Flatpak), shows the space used and deletes all saved copies.
- "Save as…" (issue #5): a movie or episode goes to a file picked in the native save dialog (the original name suggested, the dialog asks before replacing), a season or show into a picked folder, one file per episode; existing files there are reported first (`ExportConflict`) and replaced only after the user agrees. Subtitles follow the video's new base name. An export reads from a complete saved copy when there is one. It runs on the same engine (`.part`, rename, verification) and shows on the Downloads screen with progress and cancel; a finished export shows its path, "Show in file manager" and "Dismiss", never "Delete", and is forgotten at the next start. Under Flatpak the dialogs go through the desktop portal, which grants the chosen file or folder only; the manifest gets no new file-system permission. A single file picked there lives in a document-portal folder that accepts no other file, so its subtitles are not copied along (the app says so); a folder export includes them. A target that cannot be written (a removed drive) fails with "Could not write to …".
- Progress reaches the UI as `DownloadProgress` events, at most twice a second per file, with the copy's kind (saved or export) since 1.6. `coyotl-cli download ITEM [FILE]`, `export ITEM DEST [--overwrite]`, `downloads list|delete|pause|resume ID` and `storage [DIR]` drive the engine headless.

Libraries scanned before 1.1.1 are repaired by migration 4 (TMDB duplicates merged) and the `regroup` job, which derives every show's key again from its file paths, without network access or rescanning, and re-keys or merges the shows; shows whose title changed and that have no match are matched again. `crates/scanner/examples/regroup.rs` runs the same repair on a copy of a database and reports the counts.

Every step is a row in `job`: persisted, resumable after a restart, rate-limited per source, and prioritised so items on screen are processed first. The filename parser is tested against a large fixture corpus of real-world names; that corpus is the most valuable test asset in the project.

**Music library (2.0)**

*Data model (migration 8).* SQLite cannot change a CHECK constraint, so migration 8 rebuilds `item` to allow the kinds `artist`, `album` and `track`, following SQLite's twelve-step procedure: foreign keys off, one transaction, create `item_new` with the new CHECK, copy every row with its id, drop `item`, rename, recreate every index on `item` (the list is read from `sqlite_master` before the drop, so the unique TMDB index and the others come back as they were), `PRAGMA foreign_key_check`, commit, foreign keys on. Ids are kept, so files, play state, artwork, offline copies and `item_fts` stay attached; video libraries and watch state are untouched. Before migration 8 the app copies the database to `library.db.before-8` (`VACUUM INTO`, before anything is changed). A migration of this kind is a `Migration::Rebuild`: it runs with foreign keys off and commits only when `PRAGMA foreign_key_check` finds nothing. `crates/library/examples/migrate.rs` migrates a copy of a real library and reports its counts before and after (done for 1.6 libraries before 2.0). A test migrates a version-7 database with movies, shows, play state and saved copies and compares them before and after.

```sql
-- migration 8, after the rebuild of item
ALTER TABLE source ADD COLUMN content TEXT NOT NULL DEFAULT 'video' CHECK (content IN ('video','music'));
CREATE TABLE music      (item_id INTEGER PRIMARY KEY REFERENCES item(id) ON DELETE CASCADE,
                         disc_no INTEGER, disc_total INTEGER, track_no INTEGER, track_total INTEGER,
                         release_date TEXT,        -- as tagged: 2024, 2024-03 or 2024-03-15
                         release_type TEXT,        -- album | ep | single | compilation | other
                         compilation INTEGER NOT NULL DEFAULT 0,
                         genre TEXT,
                         rg_track_gain REAL, rg_track_peak REAL, rg_album_gain REAL, rg_album_peak REAL);
CREATE TABLE credit     (item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         artist_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         role TEXT NOT NULL,       -- artist | album_artist
                         position INTEGER NOT NULL, join_phrase TEXT NOT NULL DEFAULT '',
                         PRIMARY KEY (item_id, role, position));
CREATE TABLE external_id(item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         provider TEXT NOT NULL,   -- musicbrainz, theaudiodb, ...
                         kind TEXT NOT NULL,       -- artist | release | release_group | recording | track
                         value TEXT NOT NULL,
                         PRIMARY KEY (provider, kind, value, item_id));
CREATE TABLE file_audio (file_id INTEGER PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
                         codec TEXT, sample_rate INTEGER, bit_depth INTEGER, channels INTEGER,
                         lossless INTEGER NOT NULL, bitrate INTEGER, duration_ms INTEGER);
CREATE TABLE play_queue (position INTEGER PRIMARY KEY, item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         file_id INTEGER REFERENCES file(id) ON DELETE SET NULL,
                         shuffled_position INTEGER);
ALTER TABLE playstate ADD COLUMN play_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE playstate ADD COLUMN last_played_at INTEGER;
```

- The hierarchy is artist → album → track: an album's `parent_id` is its first album artist, a track's is its album. Compilations without one album artist hang below one "Various Artists" item. `credit` holds every artist of a track and every album artist of an album, in order with their join phrases ("feat.", "&"), so "A feat. B" is one credit line with two artists, and B's page lists the track.
- `external_id` holds provider ids for music; `tmdb_id` and `imdb_id` stay on `item` for video. A track also keeps its disc and track number in `item.season_no` and `item.episode_no`, so the ordered child queries that list a season's episodes list an album's tracks too. The queue's state (current position, playback position, shuffle, repeat) lives in `setting`.
- Track numbers, durations and audio properties come from the file, not from the providers; providers fill in what tags lack (release type, dates, artist ids) and never overwrite a tag silently.

*Scan pipeline for music folders (issue #7).*

1. Enumerate and diff by path, size and modification time, as for video. Unchanged files are not read again.
2. Read tags with the `lofty` crate (MIT or Apache-2.0; it needs Rust 1.89, the workspace's minimum since 2.0) through `Source::read_at` with range reads: lofty parses a sparse in-memory view of the file, and a block it touches that is not loaded yet (64 KiB, aligned) is fetched and the parse repeated. The fingerprint's two reads (the first and last 64 KiB) are reused, so a typical file costs those two reads. Tests count the bytes read (a 60-second 192 kHz FLAC with an embedded cover: at most four blocks) and prove that a scan without changes reads nothing. Measured over SMB against the test server: 2,000 tracks scan in 26 s the first time; a rescan without changes reads no file. Fields: title, artists, album, album artist, track and disc numbers, date, genre, compilation flag, MusicBrainz ids, ReplayGain, duration, codec, sample rate, bit depth, channels.
3. Group tracks into albums (`item.group_key`): by MusicBrainz release id when present (`mb:<id>`); otherwise by the album's folder (the file's folder, or its parent for `CD1`, `Disc 2` subfolders), the album artist when tagged, the album title and the year (`dir:<folder>|<album artist>|<album>|<year>`). Multi-disc albums keep their disc numbers. An album's artist is its first album artist; without one, the artist all its tracks share, else "Various Artists" and the compilation flag. Artists are keyed by their normalised name, or found by their MusicBrainz id.
4. Files without usable tags fall back to the folder and file name: `Artist/Album/01 - Title.ext`, `Artist - Album/01. Title.ext`, `01 Artist - Title.ext`, `[1959] Album`, `1977 - Album`, disc-track numbers such as `1-02`; "Artist - Title" in a file name wins over a folder that only says "Various". A broken file never stops the scan; it appears under its file name ("Unknown artist"). The fallback is tested against a corpus of real-world names.
   Moved files are found by their fingerprint and keep their track, its play count and offline copies; removed files take their tracks along, and albums and artists without tracks go too. The music scan never touches video items.
5. Artwork: embedded pictures (the front cover first) and sidecar images (`cover`, `folder`, `front`, `album` with `.jpg`, `.jpeg` or `.png`, in that order) go into the artwork cache in the same tiers as posters: `poster` (at most 1000 px), `poster_grid` and `poster_grid_l`. Only albums without a cover get one from a scan. `coyotl-cli library add PATH --music` adds a music folder (a network share's settings take `"content":"music"`), and `coyotl-cli music artists|albums|tracks ALBUM` lists what the scan made.
6. Match online (below), unless music metadata is limited to file tags.

*Album versions and editions (2.1, issue #63, migration 9).* An album can exist more than once: the same release as FLAC and as MP3, in two folders or on two sources, or a remaster next to the original. A **version** (`album_version`) is one copy: the files of one folder on one source, keyed `v:<source>:<2.0 key>` (the 2.0 album key: `mb:<release>` or `dir:<folder>|<album artist>|<album>|<year>`), with the copy's tagged title, year, release and release group; `file.version_id` names a file's version. A folder that holds the album twice in two audio formats (found in a real library: FLAC and MP3 side by side) is split into one version per format (`…|fmt:<codec>-<rate>-<depth>`) when every format has the same tracks; a single replaced file stays a second file of its track. An **album** is what its versions have in common, its `group_key`: the release group (`rg:`; its releases are *editions*, labelled from the version's title, "Parachutes (2011 Remaster)" → "2011 Remaster", or its year), else the release (`rel:`), else album artist, title and year (`tag:`), where copies count as versions of one album only when their track lists are the same; a copy with other songs under the same name becomes an album of its own (`tag:…#v<version>`), and later copies join the one whose track list they share. A **track** is keyed by disc and normalised title (`t:<disc>|<title>`) and has a file in every version that has it; one title twice with different numbers within a version (a deluxe edition's acoustic take) is two tracks (`…#<number>`). The album is named after its plainest version ("Other" rather than "Other (2011 Remaster)"). A lookup that finds an album's release group or release (#11) merges the other albums of it (`merge_same_release`). "This is a different album" (`split_version`) moves a version into an album of its own, locked (`item.locked`, key `split:<version>`), so scans and lookups leave it apart; tracks other versions share are copied for it. Merging keeps everything that hangs on a track or album: files, play counts (added up), the last play, the queue entries, saved copies, artwork and ids the kept one lacks. Migration 9 (a `Merge` migration, so the database is copied to `library.db.before-9` first) turns every 2.0 album into one version, keys albums and tracks, merges copies and splits folders by format; run on a copy of the maintainer's library (89 albums, 426 tracks, Nov 16, 2026) it found no duplicate albums, made two versions of three albums whose folders held two formats, kept two pairs of same-titled songs apart, and left no broken reference. `normalize` and `sort_title` moved from the scanner to `huehuecoyotl_library::text`, so the migration computes the scanner's keys.

*Choosing a version.* `album_detail(album, version, preference)` lists the versions best first (lossless, bit depth, sample rate, bit rate) with their format, size, source, whether the source can be reached and whether every track has a saved copy, and shows the selected version's tracks, format line and Hi-Res badge. Without a choice the core preselects (`versions::preselect`, unit-tested): the version played last (the album's `playstate.last_file_id` names a file of it), else by the "Preferred version" setting, extended for music: "Highest quality" (lossless first, then bit depth, then sample rate), "Smallest file" and "Lossless only" (the best lossless one; the best of all when none is lossless; for films the same as highest quality); versions that cannot be played only when nothing else is left. A track with two files in the selected version picks the one played last, else by preference; its menu offers each file ("Version: …", `set_track_file`). The queue stores the chosen file of every entry (`play_queue.file_id`), so a restored queue plays the same files; playing an album uses one version for all its tracks; a queued file that cannot be reached plays another version of its track and the queue entry says so; the file that plays becomes the one played last for its track and album; play counts belong to the track. "Save" on an album saves the selected version's files. Grids, Home, search and the artist page mark albums with more than one version ("3×").

*Providers.* A `MusicProvider` trait sits next to `MetadataProvider` in `crates/metadata`, with release lookup by id, release search by artist, title and track count, artist lookup, and images by id. Implementations: MusicBrainz and the Cover Art Archive (issue #11), TheAudioDB and Fanart.tv (issue #12).

- MusicBrainz: about one request per second per IP on average, HTTP 503 above it; every request carries a User-Agent with the application, its real version and the project URL: `Huehuecoyotl/<version> ( https://github.com/blub3/huehuecoyotl )`. One limiter for the whole app keeps to that rate across all jobs and backs off on 503. A lookup by release id takes one request that includes the related data needed (`inc=` artists, recordings, release group). A match without ids that is not clearly right goes to "needs review", like video; the user's choice is locked.
- Artwork order for an album: embedded art, then sidecar images, then the Cover Art Archive (the release, then its release group), then the optional providers. The optional providers' album covers are used only when nothing else has one. Artist images, backgrounds, logos and biographies come only from the optional providers; without them an artist shows its initials on a tinted tile.
- Keys: MusicBrainz and the Cover Art Archive need none. TheAudioDB and Fanart.tv need keys: each user enters their own in Settings → Metadata, as for TMDB; they are validated when entered, kept in the OS keystore and redacted from logs, and both providers stay off until then (decided in v1.9, section 12). The project ships no keys.
- Data licences (to confirm when issue #11 is implemented): MusicBrainz core data is CC0; the Cover Art Archive's images belong to their rights holders and are fetched for display only. The About screen credits MusicBrainz, the Cover Art Archive and, when used, TheAudioDB and Fanart.tv, according to their terms.
- Every lookup is a persisted job (`music_match`, with a worker of its own so MusicBrainz's pace never holds up TMDB) with the music limiter; albums on screen go first (`bump_items`). The library works from tags alone while lookups run, which take long for a large library at one request per second.
- Matching (issue #11): an album tagged with its release id is looked up directly (one request: `release/<id>?inc=artist-credits+recordings+release-groups`). Otherwise a search by album artist, title and track count (`release:"…" AND artist:"…" AND tracks:N`); it is matched only when the results that score 90 or more and agree with the album (same normalised title, the same artist or any for "Various Artists", the same track count) all belong to one release group (editions of one album), and then the first of them is looked up. Anything else scoring 60 or more goes to review with up to eight candidates (`match_candidate`, with the release id as `provider_id`); nothing found is tried again after a week. A match adds what the tags lack: the release and release group ids, the release type and compilation flag, the date, the album artists' ids (by name) and the tracks' recording ids (by disc and position, or by title when the counts differ). Locked albums are not matched.
- The client (`crates/metadata/src/musicbrainz.rs`): one limiter per client, and the core has one client, spaces requests at least 1.1 s apart across every job; HTTP 503 or 429 waits for `Retry-After` or backs off (2, 4, 8, 16 s) and retries, then the job is retried later. Covers: the release's front image at 1200 px, else its release group's, written in the poster tiers. Tests run against a mock server with responses recorded from the live service; `--features live-musicbrainz` runs one live lookup and is not part of CI. `coyotl-cli music online [on|off]` shows or changes the setting.

*TheAudioDB and Fanart.tv as built (issue #12).* Both implement the `MusicProvider` trait, which now has default answers for what a provider does not offer, plus `artist_extras` (artist image, background, logo, biographies, and the art of the artist's albums by release group), `album_art` (cover and disc art by release group), `image` and `validate` (`crates/metadata/src/theaudiodb.rs`, `fanarttv.rs`). Checked against the services on Nov 16, 2026: TheAudioDB v1 takes the key as a path segment (`/api/v1/json/{key}/artist-mb.php?i={artist id}`, `album-mb.php?i={release group id}`), answers an unknown key with HTTP 404 and an unknown id with `{"artists":null}`, allows 30 requests a minute on free keys (the limiter spaces requests 2.1 s apart), and names the English biography `strBiography` and the others `strBiographyDE`, `strBiographyFR` and so on. Fanart.tv v3 takes the key in the `api-key` header (`/v3/music/{artist id}` returns thumbs, backgrounds, logos and every album's cover and disc art in one answer; an unknown id is HTTP 400); the most liked image of a kind wins, logos in the UI language or without text first, then English. Fanart.tv's documentation describes a user's personal key (`client_key`) as meant to accompany an application's project key. The project ships none (decided in v1.9, section 12), so the user's key is sent as the API key, and a key Fanart.tv refuses on its own is reported when it is entered (not yet verified with a real key). TheAudioDB's terms allow its free key for development and ask for a paid key for apps in app stores; the user brings their own key. In the core (`crates/core/src/extras.rs`) a `music_artist` job per artist with a MusicBrainz id runs on the music worker: Fanart.tv's images first, TheAudioDB's to fill gaps and for the biography (UI language, else English, kept in `item.overview`). The image becomes the artist's poster tiers, the background a `backdrop` (at most 1920 px); the logo and disc art are kept as they are (`logo`, `disc`, PNG with transparency). An album gets a cover only when it has none, from the art that came with the artist or one `album_art` lookup. Artists are looked up when their id becomes known, at start-up when not looked up yet, and all again when a key is entered (the job's de-duplication key carries a generation, so a lookup still running with the old keys does not stand in for the new one). A key is checked with one request before it is kept (an unknown key is reported at once), lives in the OS keystore under `music-key:theaudiodb` and `music-key:fanarttv` (for the session only without a keystore), and never appears in an error message, a log line or the UI; a key a service rejects later stops that provider, and Settings says so. Settings → Metadata has a row per provider (state, key field, "Check and save", "Remove key"); the artist page shows the image, the logo above the name, the biography and the background faded behind the header, with the initials and no biography as the fallback. Tests: both clients against a mock server with fixtures in the structure of recorded answers (placeholder text, no keys), and the core against a mock server (`crates/core/tests/music_extras.rs`): no request and no job without keys, a wrong key reported and not kept, a full lookup, and no key in any file of the data folder (logs, database) afterwards. `coyotl-cli music keys` and `music key theaudiodb|fanarttv` (the key from `COYOTL_MUSIC_KEY`, never the command line).


*Privacy.* Online lookups send artist and album names (and ids) to third parties. Settings → Metadata has "Music information: from file tags only, or also from MusicBrainz and the Cover Art Archive". With tags only, no request leaves the machine for music; a test asserts that. The default is online (decided in v1.9, section 12): lookups start with the first music folder, and the user guide and the setting say what is sent. The setting is the `setting` key `music_online` ("0" turns it off); turning it on queues the albums never looked up. The attribution sits under Settings → About and in `NOTICE`.

**Audiobook library (2.5)**

*Data model (migration 10).* SQLite cannot change a CHECK constraint, so migration 10 rebuilds `item` (the kinds `audiobook`, `person` and `book_series` join) and `source` (content `audiobooks`) with the twelve-step procedure of migration 8, foreign keys off and `PRAGMA foreign_key_check` before the commit; the database is copied to `library.db.before-10` first. Ids are kept, so video and music items, watch state, play counts, files, artwork, offline copies and the queue stay as they are; a test migrates a 2.1 library with video, music, play counts and a queue and compares them.

```sql
-- migration 10, after the rebuilds of item (kinds + 'audiobook','person','book_series') and source (content + 'audiobooks')
CREATE TABLE book       (item_id INTEGER PRIMARY KEY REFERENCES item(id) ON DELETE CASCADE,
                         key TEXT NOT NULL,              -- the place: b:<source>:<folder or file>
                         series_position REAL,           -- 2.5 for "Book 2.5"
                         publisher TEXT, published TEXT,  -- as given: 1997 or 1997-06-26
                         language TEXT,                  -- ISO 639-1
                         isbn TEXT, asin TEXT,
                         duration_ms INTEGER NOT NULL DEFAULT 0,
                         cover_ratio REAL,               -- height / width of the cover (1.0 square, 1.5 portrait)
                         speed REAL,                     -- remembered playback speed; NULL: the default
                         locked TEXT NOT NULL DEFAULT '[]',  -- fields the user corrected (JSON list)
                         drm INTEGER NOT NULL DEFAULT 0);
CREATE UNIQUE INDEX book_key ON book(key);
CREATE TABLE book_credit(book_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         person_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         role TEXT NOT NULL CHECK (role IN ('author','narrator')),
                         position INTEGER NOT NULL,
                         PRIMARY KEY (book_id, role, position));
CREATE INDEX book_credit_person ON book_credit(person_id, role);
CREATE TABLE book_file  (file_id INTEGER PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
                         book_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         idx INTEGER NOT NULL,            -- order on the timeline
                         start_ms INTEGER NOT NULL, duration_ms INTEGER NOT NULL);
CREATE INDEX book_file_book ON book_file(book_id, idx);
CREATE TABLE chapter    (book_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         idx INTEGER NOT NULL,
                         title TEXT NOT NULL,
                         start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,  -- on the book's timeline
                         file_id INTEGER REFERENCES file(id) ON DELETE SET NULL,
                         PRIMARY KEY (book_id, idx));
```

- Items: a book is `audiobook` (title, sort title, year, overview = description, `parent_id` = its series or NULL); authors and narrators are `person` items (one per normalised name, `group_key = person:<name>`), linked through `book_credit` in order; a series is `book_series` (`group_key = series:<author>|<name>`), its books ordered by `series_position`. Progress is the book's `playstate` (position on the timeline, duration, finished as `watched`, `last_played_at`), so "Continue listening" and the unread, in progress and finished filters read one table. Covers use the poster tiers (`poster`, `poster_grid`, `poster_grid_l`); `cover_ratio` lets one grid show square and portrait covers.
- A book is keyed by its place (`book.key`): `b:<source>:<folder>` for a folder (its disc subfolders included) or `b:<source>:<file>` for a single file. Grouping by author and normalised title happens above that: two places with the same author and title are two copies of one book; 2.5 keeps the first one found as the book and lists the others as "also in" (playing a copy is a later change, as albums got versions in 2.1). This keeps rescans incremental and never merges two different books silently.

*Scan pipeline for audiobook folders (issue #65).*

1. Enumerate and diff by path, size and modification time, as for music; unchanged files are not read again. Group the files into books by the layouts of section 2: a folder with audio files is a book, its `CD1`/`Disc 2` subfolders included; a folder whose audio files are each a complete book (single M4B files, or files named `Title - Author`) holds one book per file (one file per book when every file has chapters or the files' titles differ and none has a track number).
2. Local metadata, highest first: a Calibre `metadata.opf` (`quick-xml`; title, creators by role `aut`/`nrt`, series and `calibre:series_index`, publisher, date, language, ISBN and ASIN identifiers, description), `desc.txt` (description) and `reader.txt` (narrators), a `.nfo` file (Kodi-style XML: title, author, narrator, series, year, plot), the tags of the first audio file (`lofty`, read as for music: album = title, album artist or artist = author, composer = narrator, the movement fields for series and position (`MVNM`/`MVIN` in MP4, `MOVEMENTNAME`/`MOVEMENT` in Vorbis comments, `MVNM`/`MVIN` in ID3; `lofty` maps no free-form `SERIES` key, so tools that write only that are covered by the OPF and the names), publisher or label, year, language, description or comment), then the folder and file names (`Author/Series/Book 2.5 - Title`, `Title - Author`). A field filled by a higher source is never replaced by a lower one. Broken or unreadable tags never stop the scan: the book appears under the name its folder or file gives.
3. Durations come from the tag reader's stream properties (headers, never a full read; the tests count the bytes read over SMB as for music: a 40-file book costs about two block reads per file). The files' order (disc, track number, natural sort of the names) gives the timeline.
4. Chapters: embedded chapters come from the probe (section 7, scan step 6; 2.5 extends `ProbeInfo` with the chapter list: start, end and title per chapter) and replace the file chapters when the probe has run; until then, and for files without chapters, each file is one chapter named from its title tag or file name. A `probe` job is queued at high priority for new books with a single file or M4B files.
5. Covers: embedded art or `cover.jpg`/`folder.jpg`/`cover.png` into the poster tiers, with `cover_ratio`.
6. Series: books with the same series name and author form one `book_series` item, ordered by position.
7. Match online (*Book providers*), unless online information is off.

`coyotl-cli library add PATH --audiobooks`, `books`, `authors`, `series`, `book ID` (with chapters and files) drive it headless. Tests use generated fixtures (tones or silence with tags, an M4B with chapters made by FFmpeg, an OPF): one M4B with chapters, a 12-file MP3 book, a two-disc book, a series of three books with an OPF, and an untagged book in `Author/Title`; each gives exactly one book with the right author, narrator, series position and chapters.

*Book providers (issue #67).* A `BookProvider` trait next to `MusicProvider`: search by title and author (ISBN first when the local metadata has one), a work's details (description, subjects, first publication year), and a cover by cover id or Open Library id. Implementations:

- **Open Library** (no key). Checked on Nov 16, 2026: 1 request per second without a contact in the User-Agent, 3 with an email or phone number; no bulk use (dumps exist for that); covers by cover id or OLID are not rate-limited, by ISBN at most 100 per 5 minutes per IP; covers must not be crawled. Open Library asserts no new rights over its data and asks contributors for CC0; covers carry no stated licence and are fetched for display only. The client sends `Huehuecoyotl/<version> ( https://github.com/blub3/huehuecoyotl )` on every request, has one limiter for the app at 1 request per second (decided in v1.12: no contact email), backs off on 403 and 429, looks books up one at a time as jobs on demand (never in bulk), caches what it gets in the library, and fetches covers by cover id (`https://covers.openlibrary.org/b/id/<id>-L.jpg?default=false`), never by ISBN. Search: `search.json?title=…&author=…&fields=key,title,author_name,author_key,first_publish_year,cover_i,language` (`author_key` since 2.7, #109) (or `q=isbn:…`; the `isbn` field is not requested, a work lists thousands). An ISBN search whose first work has the book's title is a match; by title and author, the first work whose title (main title, articles and punctuation aside) and an author (initials allowed) agree is the match, since Open Library lists some works twice; works that only share words go to review with the candidates. The work (`/works/<id>.json`) is fetched only for a description the book lacks. Built as `crates/metadata/src/books.rs` (trait, decision) and `openlibrary.rs`; the job `book_match` runs in a worker of its own; fields filled online are listed in `item.meta_json.online` so a match chosen later replaces them, and subjects are kept in `meta_json.subjects`.
- **MusicBrainz** for books tagged with MusicBrainz ids (Picard): the release is looked up exactly with the existing client and limiter (its date gives the year), its cover from the Cover Art Archive; release groups of the secondary type "Audiobook" count as books. A release id MusicBrainz does not know falls back to Open Library.

Online data fills only what the local metadata left empty and never overwrites a local field or one the user corrected (`book.locked`). "Search again" on a book shows the candidates to choose from (core `search_book` and `choose_book_match`, `coyotl-cli book search|choose`; the button is on the book page, #66). The About screen credits Open Library with a link. *Privacy:* lookups send titles, authors and ISBNs. The existing setting covers books too and is renamed "Music and book information online" (`setting.music_online`, on by default; decided in v1.12); off, no request leaves the machine for books (a test asserts it).

*Matching again (2.7, issue #92).* One way, for every kind, to match an item again, to search for a match by hand and to choose one (`crates/core/src/matching.rs`, `crates/library/src/matching.rs`):

- `match_again(item, unlock)` runs the automatic match of a new item for this one item: movies and shows as in matching above, albums as in *Music providers* (a search, never the tag's release id alone), books as in *Book providers*. A match the user chose (`item.locked`) is kept: the answer is "locked" until the user unlocks it, and the lock is lifted only when the match changes. An automatic rematch never removes a match it cannot replace with a sure one: when nothing sure is found the match stays; when several fit and the current match is not among the first three, the item goes to "needs review" with its candidates but keeps its data until the user chooses. Results: unchanged, newly (matched now, not before), changed, review, notFound.
- `match_search(item, query)` searches by the item's kind: TMDB movies or shows (a switch, so a wrong kind can be searched; the change itself is #99 and #100), MusicBrainz releases (with country, label, formats, type and track count, to tell editions apart), Open Library works. The query's text may be an id or a link instead of a title (`crates/metadata/src/ids.rs`): TMDB movie and show links, IMDb ids and links (resolved through TMDB's `/find`), MusicBrainz release, release group, recording and artist links or bare MBIDs, Open Library work and author links or ids, and ISBN-10/13 with a valid check digit. Candidates carry the provider, id, kind, title, year, a thumbnail URL (TMDB `w185`, the Cover Art Archive's `front-250`, Open Library `-M`), a confidence or a "fit" text, and whether it is the current match. `match_choose(item, candidate)` applies the choice and locks it (a TMDB id another item has merges the two, as above; a release joins other copies of it, *Album versions and editions*).
- `rematch_library(library, all, include_locked)` for movies, shows, music (albums) and books: only unmatched and "needs review" items, or all; locked items only when included. Runs wait in line, one library at a time (`setting` key `rematch_runs`: library, choices, state waiting | running | done | stopped, total, done and the counts newly, changed, review, notFound). A started run queues one low-priority job per item (`rematch_video` in the TMDB worker, `rematch_music` and `rematch_book` in their workers), so the providers' limiters apply, playback and browsing go on, and a run resumes after a restart (waiting runs start with the next start). `rematch_stop` removes the run's queued jobs; what was matched stays. A run whose jobs are gone (failed for good) counts as done. `matching_status` reports each library's counts (total, matched, needs review, not found, chosen by the user; episodes, songs, artists and authors), its run, and whether it is blocked (no TMDB key; online information off). Without a key or with online information off nothing is queued or sent (a test asserts it).
- `coyotl-cli match ID [--unlock]`, `match search ID [TEXT|ID|LINK] [--year Y] [--kind movie|show] [--artist A] [--author A] [--isbn I]`, `match choose ID PROVIDER_ID [KIND]`, `library rematch movies|shows|music|books [--all] [--include-locked]` and `library rematch status|stop LIBRARY` drive it headless.
- Checked by `crates/core/tests/matching.rs` (a fake TMDB: the scopes, locked items, a match kept when nothing sure replaces it, a changed match, links and IMDb ids, a run resumed after a restart, no key and online off) and unit tests of the runs, the id parser and the new provider answers.

**TMDB**

- [Terms](https://developer.themoviedb.org/docs/faq): free for non-commercial use with attribution. The About screen must carry an approved TMDB logo, less prominent than the app's own, and the notice that the product uses the TMDB API but is not endorsed or certified by TMDB.
- API key: an open-source client cannot keep a key secret. The project therefore ships no key. Each user enters their own free TMDB key in the Settings menu, under Metadata. A first-run screen explains how to get one, the app validates it with a test request, and it is stored in the OS keystore. Without a key the app still works as a player and shows titles parsed from file names. For development and automated tests the core also accepts the key from the `COYOTL_TMDB_KEY` environment variable; it is never written to the repository, to fixtures or to logs.
- Key status (issue #52): the core looks for the key once at start-up, before anything else, tells the UI what it found, and answers status requests only after that, so a stored key is never shown as missing. Settings → Metadata shows the state without ever showing the key: "Key stored" (in the keystore), set for development (from the environment variable), in use until the app closes (the keystore could not keep it), no keystore available (with what to do), or no key (the field to enter one). It shows when TMDB last accepted the key (`setting` key `tmdb_key_validated_at`, set by Check and save and by Test key) and offers Replace key (the stored key stays until TMDB accepts the new one), Test key and Remove key (which deletes the keystore entry). `coyotl-cli key status|test|remove` does the same headless.
- Client behaviour: request the user's language with an English fallback, combine detail calls with `append_to_response`, apply a client-side rate limiter, and back off on HTTP 429.
- Metadata sits behind a `MetadataProvider` trait, so local `.nfo` files or other databases can be added without touching the scanner.

## 8. C++ to Rust bridge

The core's API is a small set of Rust functions and one event sink, bound to C++ with the `cxx` crate; a thin C++ adapter turns them into QML models and signals.

```rust
// crates/core-api/src/bridge.rs  (sketch)
#[cxx::bridge(namespace = "core")]
mod ffi {
    // Shared plain-data structs: ItemSummary, Page, BrowseQuery, PlaybackTicket, CoreEvent
    struct ItemSummary { id: i64, kind: u8, title: String, year: i32,
                         poster_path: String, progress: f32 }

    extern "Rust" {
        type Core;
        fn core_open(data_dir: &str, sink: UniquePtr<EventSink>) -> Result<Box<Core>>;

        // Every call returns at once; the answer arrives as an event with the same id.
        fn browse(self: &Core, request_id: u64, query: &BrowseQuery);
        fn item_detail(self: &Core, request_id: u64, item_id: i64);
        fn open_for_playback(self: &Core, request_id: u64, item_id: i64);
        fn report_progress(self: &Core, ticket: u64, position_ms: u64, state: u8);
        fn cancel(self: &Core, request_id: u64);
    }

    unsafe extern "C++" {
        include!("app/event_sink.h");
        type EventSink;
        // Thread-safe: queues the event onto the Qt GUI thread.
        fn post(self: &EventSink, event: &CoreEvent);
    }
}
```

**Contract rules**

1. No call blocks. `cxx` has no async support, so each request carries an id and its result comes back as an event on the GUI thread.
2. `EventSink::post` is the only C++ function the core calls. It marshals with a queued invocation, so models are only ever touched on the GUI thread.
3. Queries are paged and return view models sized for one screen, not database rows. A page of episodes (`season_episodes`, since 1.6) carries everything a tile shows.
4. Change events carry ids, not payloads. Models invalidate and re-query, which keeps one source of truth.
5. Any request can be cancelled by id.
6. Errors carry a typed category the UI can present: Auth, Unreachable, NotFound, Unsupported, Internal, NoSpace (a copy does not fit on the drive, since 1.5).
7. No panic may unwind across the boundary; the facade converts panics into `Internal` errors.
8. The mpv stream callbacks are plain `extern "C"` functions outside this bridge, because mpv calls them, not Qt.

On the C++ side, `LibraryModel` and `EpisodeModel` (`QAbstractListModel`s) fetch pages on demand and `PlayerController` connects tickets to `MpvPlayer`. The command-line harness uses the same Rust API with a channel as its sink.

**Music additions (2.0).** The same rules apply: paged queries and change events with ids. New calls: `browse_music` (albums or artists, sorted and filtered, paged), `album_detail` and `artist_detail` (the track list grouped by disc, the artist's albums grouped by release type), music results in `search` grouped by kind, and the queue: `queue_play` (items, start index, shuffle), `queue_insert` (play next or add to the end), `queue_move`, `queue_remove`, `queue_step` (next or previous), `queue_set_mode` (shuffle, repeat) and `queue_state`. The core answers with `QueueChanged` (the entries around the current one, the position, the modes) and hands the shell the current and the following entry as playback tickets, as for video. As built (#8): `queue_play` takes the items (tracks, albums or artists) and the item to start from; `queue_step` is next or previous with the playback position; there are also `queue_jump`, `queue_advanced` (mpv went on by itself), `queue_resume` (load where it was left, or go on after the end), `queue_clear`, and `music_progress`, which records play counts for tracks (not `report_progress`); the tickets come as `MusicTickets` events, both events carrying JSON.

**Matching additions (2.7, issue #92).** `match_again(item, unlock)`, `match_search(item, query_json)` and `match_choose(item, choice_json)` for every kind, `match_preview(item, choice_json)` ("matchPreview", #97 to #100), and `rematch_library(library, all, include_locked)`, `rematch_stop(library)` and `matching_status()`; all answer as `MusicDetail` events of the kinds "matchAgain", "matchCandidates", "matchChosen" and "matching" with JSON, so the shared structs stay as they are (section 7, *Matching again*). In QML: `Core.matchAgain`, `matchSearch` and `matchChoose` take maps, `rematchLibrary`, `rematchStop` and `matchingStatus`.

**Audiobook additions (2.5).** Paged browsing of books, persons (authors, narrators) and series through `browse_items` with the new kinds and the filters `unread`, `in_progress` and `finished`; `book_detail` (the book with its credits, series, chapters, files, progress and speed), `series_detail` and `person_detail` as JSON events like `MusicDetail`; `book_play` (book, optional position) answering with a `BookTicket` (the files with their offsets, the start file and offset, the speed, the chapters); `book_progress` (book, timeline position, playing or paused, speed) to save; `book_mark_finished`, `book_set_speed`, `continue_series`, and `book_search_again` with `book_choose_match`. Timeline arithmetic, chapter lookup, the sleep timer's stop time and "continue series" live in the core. *As built:* `browse_items` filters `unread`, `in_progress`, `finished` (books) and `author`, `narrator` (persons); summaries carry the cover `ratio` and, for books, the first author as subtitle. Series and person pages are `book_detail` with kind "series"/"person" (`{id, title, books}`), so there are no separate `series_detail`/`person_detail` calls. The ticket comes as `MusicDetail` kind "bookTicket" (`book_play`), "Continue series" as kind "continueSeries"; progress, speed (`book_set_speed`, <= 0 for the default) and `book_mark_finished` as named; "Search again" is `search_book` and `choose_book_match` (#67). The chapter navigation (`previous_chapter` with the 3 s rule, `next_chapter`, `end_of_chapter`) is exported synchronously over cxx as `book_previous_chapter`, `book_next_chapter`, `book_end_of_chapter`.

[CXX-Qt](https://github.com/KDAB/cxx-qt) from KDAB would allow the models themselves to be written in Rust. It is still below 1.0, so the spec uses plain `cxx` now and leaves that as a later option.

## 9. Qt Quick UI layer

The UI is QML on top of a few C++ models and controllers: it holds no business logic and must be fully usable with a keyboard or remote, not only a mouse.

| Area | Decision |
| --- | --- |
| State | C++ list models and controller objects exposed to QML as singletons; QML keeps only view state |
| Navigation | A `StackView` inside a persistent shell: Home, Movies, Shows, Search, Downloads, Sources, Settings, Player |
| Controls and style | Qt Quick Controls on the Basic style with a custom dark theme, so the app looks the same on Windows and Linux |
| Input | Mouse and keyboard, plus directional focus navigation (`KeyNavigation`, `FocusScope`) on every screen from the first milestone; gamepad buttons mapped to the same keys. Living-room PCs on Linux are a core audience. |
| Layout | Windowed and fullscreen "TV" layouts from the same components |
| Desktop integration (C++) | Fullscreen and cursor auto-hide; screensaver inhibit during playback; media keys through SMTC on Windows and MPRIS over D-Bus on Linux |
| Localisation and accessibility | `qsTr` strings with Qt Linguist files and `Accessible` properties from the start; retrofitting both is expensive |
| Tooling | `qmllint` and the QML compiler run in every build |

**Music section (2.0, issue #10)**

- The navigation rail gains **Music**, with Albums, Artists and Recently added as tabs of its page. Home gains rows for recently added albums and recently played music, shown only when there is music.
- Albums: a grid of square covers that follows the cover-size settings. Album page: cover, artist, year, total duration, format and Hi-Res badge, the tracks grouped by disc, and Play, Shuffle, Save (offline copies, section 7) and Save as…. Artist page: albums grouped by release type (album, EP, single, compilation), sorted by year, with the artist's image and biography when a provider supplied them.
- Now Playing: cover, title, artist, album, a format line ("FLAC · 96 kHz · 24 bit"), the signal path (5.7), the seek bar and the queue. A mini player at the bottom of the shell stays visible while music plays and leads back to Now Playing; it takes no focus of its own until the user moves down to it. As built (#8): Now Playing (`NowPlayingPage.qml`) has the artist and the album as buttons to their pages, shuffle, previous, play or pause, next and repeat (off, all, one), the seek bar (Left and Right jump 10 s) and the queue (Enter plays an entry, Menu or Delete offers Play now and Remove, Ctrl+Up and Ctrl+Down move it, "Clear queue" asks once more); the signal path follows with #9. The mini player (`MiniPlayer.qml`) shows while there is a queue, on every screen but Now Playing; Down from the last entry of the navigation rail reaches it, and its buttons are previous, play or pause, next and Now Playing (a click on the cover or title opens it too).
- Search shows artists, albums and tracks grouped by kind next to movies and shows. Adding a library folder asks for its content type.
- Long lists use paged C++ models, as for video; QML holds no business logic. Without a music folder the Music page explains how to add one instead of showing empty grids.
- Keys: everywhere as for video; on Now Playing and in the mini player, Space plays or pauses, and the media keys work throughout.

*As built (issue #10).* The Music page (`MusicPage.qml`) is a `GridPage` with tabs (Albums, Artists, Recently added) over the same paged `LibraryModel`; grids take a cover ratio (`posterRatio`, 1 for music), and the filter offers All and Saved (no watched state for music; none for artists). Album and artist pages (`AlbumPage.qml`, `ArtistPage.qml`) load one JSON document each from `album_detail` and `artist_detail` (event `MusicDetail`): the album's tracks by disc with their format, the album's format line and the Hi-Res badge (lossless and above 44.1 kHz or 16 bit), and the artist's albums by release type plus "Appears on" (albums by others with a track that credits the artist). A track opened from search opens its album with the track focused. Since #8 Play and Shuffle start the album in the play queue, Play next and Add to queue put it after the current track or at the end, Enter on a track plays the album from it, and Menu on a track offers Play next and Add to queue. The album's Save menu offers Save and Delete saved copies; Save as… for a whole album is not offered yet. Search runs one query per kind (`search(text, kind)`) and shows a row for each kind with results; item summaries carry a `subtitle` (an album's artist, a track's album) and the `parent_id` (a track's album). Home shows "Recently added albums" when there is music, and since #8 "Recently played" (albums by their tracks' last play; `browse` sort `played`). Adding a folder in Sources, and a network share's settings, ask whether it holds video or music. Since #63 the album page has a version button under the format line (the shared `VersionPicker`, with music labels) and "This is a different album" when there are several versions, notes when the selected version or every version is offline (Play is then off), and a "Version: …" entry per file in the menu of a track with two files.

Under heavy background work (tens of thousands of queued jobs) the grids kept growing slowly: every finished job refreshed them, a refresh cancelled the page in flight, and page answers waited for the "on screen first" write (`bump_items`) behind the busy writer. Since #10 `LibraryModel` never cancels a request for a refresh or a page: a refresh asked for while one is in flight runs once its answer is in, and a page wanted meanwhile is fetched right after. `bump_items` is sent without waiting for it. Music files are never probed (their format comes from the tags), so a music folder queues no `probe` jobs. `coyotl --benchmark music` scrolls the Albums grid: with 2,500 albums (50,000 tracks) it dropped no frame at 60 Hz and loaded 1,080 albums in 20 s.

*Music in 2.6 (issue #78), as built.* `MusicPage.qml` is a page of its own (no longer a `GridPage`): a top menu (Albums, Artists, Genres; Songs and Playlists join with #80 and #81), "Search music" (opens Search with the text), and for Albums the Quality lens: toggle chips Hi-Res, Lossless, Several versions and Saved that combine, with counts, plus Genre and Decade choosers and the sorts Recently added, A–Z, Year and Most played (`browse` sort `plays`: an album's tracks' play counts). "Recently added" is a sort now, not a tab. `browse_items`' filter is a list of tokens, one per line (`hires`, `lossless`, `multi`, `saved`, `unplayed`, `genre=…`, `decade=…`; one token alone keeps working); an album matches a lens when one of its versions does (`catalog::music_where`). Summaries of albums and tracks carry `format` ("FLAC 24/96", "MP3 320k", "DSD"; the best file) and `hires`, which every cover shows as a ribbon, gold for Hi-Res. `music_facets(kind, filter)` (MusicDetail "facets") gives the counts per lens within the chosen genre and decade, the genres and decades within the other filters, and each initial's offset in the A–Z order; the letter rail (sorted by title only) jumps there through `LibraryModel.loadTo(row)`. Genres is a grid of the genres with album counts; one opens Albums filtered to it. Covers show Play, Shuffle and More (Play next, Add to queue, Save, Open) under focus or the mouse; P plays, Shift+P shuffles, Menu opens More. `coyotl --benchmark music` with 2,500 albums (25,000 tracks) and ribbons: no dropped frame at 60 Hz in two runs (p99 17.8 ms).

*Artists in 2.6 (issue #79), as built.* Artists are round tiles (`PosterCard.round`: the image masked to a circle with `MultiEffect`, the initials without one) with "N albums · M Hi-Res" (summaries carry `albums` and `hires_albums` for artists). Filters: **Album artists** (artists with an album of their own; the default, so artists who only have a track credit are listed under **Everyone credited**, `credited`) and **With Hi-Res albums** (`hires`; `catalog::filter_where` applies the artist rules). A preview panel beside the grid follows the focused artist (the existing `artist_detail`, asked for 180 ms after focus settles): image, name, **Play all**, **Shuffle**, **Open page**, the start of the biography, up to six albums with a Hi-Res mark, and the number of albums the artist appears on. The letter rail works on artists too (`music_facets` for "artist"); Right from the last column reaches the panel, then the rail.

*Songs in 2.6 (issue #80), as built.* **Songs** in the top menu is one list of every track (`browse` kind `track`; summaries carry `artist`, `duration_ms` and `plays`), with the columns #, Title, Artist, Album, Time, Quality (the best file's format, gold for Hi-Res) and Plays. A column header sorts (`title`, `artist`, `album`, `duration`, `quality`, `plays`; S cycles them); quality sorts DSD first, then lossless by sample rate and bit depth, then lossy by bit rate. The lenses Hi-Res, Lossless and Never played (`unplayed`) filter it with counts. **Play these** and **Shuffle** queue the filtered list in its order through `queue_play_query(kind, sort, filter, start, shuffle)`: the core reads the ids itself (`catalog::browse_ids`, at most 100,000), so a long list never crosses the bridge; Enter plays from the focused song, Menu offers Play next, Add to queue and Go to album. The song playing is marked. `coyotl --benchmark songs` scrolls the list: with 50,000 tracks, no dropped frame at 60 Hz in two runs (p99 18.8 to 18.9 ms).

*Playlists in 2.6 (issue #81), as built.* Migration 11 adds `playlist(id, name, created_at, updated_at)` and `playlist_entry(playlist_id, position, item_id, file_id)`: like the play queue, an entry keeps the song and the file chosen for it (`ON DELETE SET NULL`: when that file goes away the song stays and plays another version; a song that leaves the library leaves its playlists). `crates/library/src/playlists.rs` holds six smart playlists, computed when asked, never stored: **Hi-Res gems** (Hi-Res songs, best quality first, at most 1,000), **Most played** (50), **Recently added** (songs of albums added in the last 30 days), **Not played yet**, **Saved offline** (songs with a complete saved copy) and **Forgotten favourites** (played at least 5 times, not in the last six months, at most 100); and own playlists (create, rename, delete, add, remove, move). The core answers `playlists` and `playlist_detail(key)` as MusicDetail "playlists" / "playlist" (key "smart:<rule>" or an id); `playlist_create`, `playlist_add`, `playlist_rename`, `playlist_move` (`to` < 0 removes) and `playlist_delete` change them; items added are expanded like the queue's (albums in a version, artists) with their files. `queue_play_playlist(key, start, shuffle)` plays the songs with their files through a queue operation (`Op::PlayPlaylist` → `Op::PlayTracks`); `playlist_save` saves their copies (1.5). The UI: **Playlists** in the Music menu (smart cards, own playlists as cover mosaics with **New playlist**, a preview panel with Play, Shuffle, Save offline, Open), `PlaylistPage.qml` (Ctrl+↑/↓ moves, Delete removes, Rename, Delete playlist with a second press), and `AddToPlaylist.qml` from the Menu of albums and songs in the Albums grid, the Songs list, the album page (also a button), playlist pages and the Now Playing queue. `.m3u8` import and export are a later issue.

*Now Playing in 2.6 (issue #82), as built.* The page uses the whole window (the shell hides the navigation rail while it is open; "‹ Music" or Back returns) and is tinted with the album cover's colour: `metadata::artwork::tint` scales the cover to 24 × 24, averages it weighted by how colourful each pixel is, keeps the hue, caps the saturation at 0.55 and sets the lightness to 0.17, so the app's light text keeps at least 4.5:1 (unit-tested on yellow, blue, white and black covers). It is computed when Now Playing first asks (`track_visuals`) and kept in `artwork.color` (migration 12). The seek bar is the song's waveform: a `waveform` job in the probe worker decodes the file with FFmpeg (`probe::waveform`, the probe's custom I/O and a decoder; bindings regenerated with `AVFrame` and `AVCodecContext`) and folds the absolute sample levels into 160 peaks, kept in `file_audio.waveform`; it is queued when Now Playing asks for a file that has none, only when its source is reachable, and posts ItemChanged for the song when done; until then a plain bar shows, and a file that cannot be decoded keeps an empty waveform (a plain bar for good). Bars are scaled to the song's loudest point; click or drag seeks, Left and Right seek 10 s. The signal path is a chain File → Huehuecoyotl → Output with a verdict (Bit-perfect, Converted or Shared output) and the sentence why (SPEC 5.7's rules). Volume is a slider with Quieter and Louder, fixed at 100 % in bit-perfect mode; a ReplayGain line shows in the chain. Up next is a panel on the right with drag handles (mouse), Ctrl+↑/↓, Menu or Delete, the "other version" mark for fallbacks, and Clear queue. The music player's current entry carries the playing file (`fileId`). The cover's size follows the page, not the layout it sits in (no layout feedback).

*Full-screen Now Playing in 2.6 (issue #83), as built.* **Full screen** on Now Playing, or F11 while it is open (the window's F11 shortcut asks the shell first, `Shell.toggleFullView`), loads `MusicFullScreen.qml` over the page and puts the window in full screen unless it already was; Back or F11 leaves both, and leaving the page or the queue ending leaves too. Designed at 1920 × 1080 and scaled by min(width / 1920, height / 1080): a 760 px cover, the title at 76 px, artist and album, the format tags, a progress bar with elapsed and remaining time, Previous, Play / Pause (132 px), Next, Shuffle and Repeat with the focus ring, an Up next card (the next entry and "and N more", hidden after the last), and a line of key hints. Keys on Play / Pause (focused on entry): Enter plays or pauses, ← → seek 10 s, ↑ reaches the other controls (← → move there, ↓ returns), ↓ opens the queue as an overlay (Enter plays an entry, Back or ← closes). After 5 s without input (a key the view sees, a focus change, an action, a real pointer move: Qt's repaint hover events do not count) the controls, the card and the hints fade out; cover, title and progress stay. While faded a catcher item holds the focus, so the first key (media keys included) or click only wakes the view and gives the focus back. The view takes the music player as a property, so the QML test drives it with a stub (`tst_musicfullscreen.qml`: focus on entry, keys, the fade and the silent wake, the queue, track changes). The screensaver is not inhibited, as for music elsewhere.


**Audiobooks section (2.5, issue #66)**

- The navigation rail gains **Audiobooks** with the tabs Books, Authors, Series and Narrators. Home gains "Continue listening" (books in progress, most recent first), shown only when there are books.
- The book grid shows square and portrait covers in one grid: every tile has the same height, a cover keeps its own aspect inside it (`cover_ratio`), and titles align. Filters: all, unread, in progress, finished. It stays within the scroll budget for 2,000 books (measured with `--benchmark books`).
- Book page: cover, title, authors, narrators, the series with position (opens the series page), duration and time left, description, the chapter list with the current chapter marked, Play or Resume, Mark as finished (or unfinished), Save and Save as… (1.5, all of the book's files). Series page: the books in order with their progress, and "Continue series" (the first unfinished book after the last one listened to).
- Player: a book's Now Playing page with cover, title, author, the chapter title and the time left in the chapter and in the book, a seek bar over the whole book with chapter marks, previous and next chapter, skip back and forward, speed, sleep timer, and the chapter list. Space plays or pauses; Left and Right skip; Page Up and Page Down change chapter. The mini player shows book and chapter while a book plays.
- Settings: default speed, skip back and forward, under Playback. All of it works from the local database.
- *As built (issue #66).* `AudiobooksPage.qml` (tabs over `GridPage`, square cells with the cover in its own aspect on the cell's bottom, `PosterCard.coverRatio`), `BookPage.qml`, `BookListPage.qml` (series and persons), `BookPlayerPage.qml` (speed as − and + around the value, the sleep timer stepping through off, 5 to 120 minutes and end of chapter with Enter), Home's "Continue listening" (Enter resumes, I opens the page), the mini player and media keys following the front player. Settings → Playback: Audiobook speed (0.8× to 2.0×), Skip back (5 to 60 s), Skip forward (10 to 120 s), kept in `ui.ini`. `coyotl --benchmark books`: 2,000 books with square and portrait covers, three runs on the development machine (60 Hz): 0, 2 and 3 of 1,200 frames dropped, p99 17.7 to 18.4 ms, 1,080 books loaded in 20 s.

**Performance budgets**

- Poster grids scroll without dropped frames at 60 Hz on integrated graphics with a library of 5,000 items.
- Grids use `GridView` with delegate reuse and asynchronous image loading. QML never decodes a full-size poster in a grid; it loads the pre-scaled thumbnails the core wrote to disk, the smallest tier that covers the displayed size at the screen's scale factor.
- A movie or episode with several files (versions) has a version selector on its page (issue #34). Each version is labelled from its probe data, or from file-name tokens until it is analysed: edition (Director's Cut, Extended, Theatrical, IMAX, `{edition-…}`), resolution, HDR, video codec, main audio, size and source. Versions on an unreachable source are listed as offline and cannot be chosen, unless a complete saved copy of them exists (since 1.6: "offline, plays the saved copy"). The page preselects the version played last, else the best playable one: resolution, then HDR, then bit rate, or the smallest file (Settings → Playback → "Preferred version"). Play, Resume and the technical details follow the selection. Watched state belongs to the movie; versions of the same cut (durations within 2 %) share its resume position, and a version of another cut keeps its own (`file_state`) and says so. "This is a different movie" moves a version into its own item, locked so a rescan does not merge it again, and sends it to the review list.
- A show has one page (issue #32): backdrop, poster, details, a main button for the next episode ("Play S2 E3", "Resume S2 E3" for a started one, "Play again" when everything is watched), a season switcher (tabs, or a list with more than eight seasons; specials last; it opens on the season of the next episode) and the selected season's episodes with still, runtime, overview, progress and watched state. Seasons and episodes TMDB knows but the library has no file of are kept as virtual items (`item.virtual`): shown dimmed, not playable, and turned into normal items when a scan finds their file. Everything comes from the local database; TMDB data is fetched by the job queue. Left and right on the season switcher switch seasons, up and down move through episodes, W marks an episode or (on the season switcher) a season watched, I opens an episode's own page.
- Every episode on the show and season pages is a tile (issue #48) with what the movie page has, from the same QML components (`VersionPicker`, `TechSummary`, `TechDetails`, `SaveMenu`): a version dropdown when it has two or more files (same labels, offline marks and preselection as movies), the technical summary and badges of the selected version ("Analysing…" until it is probed), "Details" to open the full details panel under the tile (one at a time, with "Analyse now"), and the Save menu. The selected version drives Play, the summary, the panel, Save and Save as… of that tile; the choice is kept while the page is open. The tile shows a mark for a saved copy and for an outdated one, and the progress while it saves. Episodes not on disk get none of these. Keys: Enter plays, Right moves into the tile's controls (version, Details, Save menu), Left and Right move between them, up and down leave them for the next tile, Back leaves the controls and a second Back the page, Left on a tile goes to the navigation rail, Menu opens the focused tile's Save menu. Focus stays put when a tile expands, a menu closes or a copy finishes.
- The tiles are the page's list, with the rest of the page as its header, so only tiles near the visible part exist. `EpisodeModel` fetches 40 episodes per request (`season_episodes`): each with its versions (probe data trimmed to what labels and the summary need), its resume state and its saved-copy state, so scrolling never queries per tile. An episode's full probe is read (`item_detail`) only when its details panel opens. Download progress updates the one tile in place; a change of state re-reads the loaded episodes in one request and updates them in place.
- Cover size depends on the display mode: Settings → Appearance has a size for the window (default Medium) and one for fullscreen (default Large), each Small, Medium or Large (140, 180 or 260 px target width). Grids derive their columns from the available width and the target, so cells fill the width exactly; text and spacing scale with the cover. Switching modes keeps the focused item focused and in view. The choice is stored in `ui.ini` in the data folder.
- Cold start to an interactive Home screen in under two seconds from a warm disk cache.
- These are measured by an automated scroll benchmark in CI, not asserted. `--benchmark episodes` (since 1.6) scrolls the first show's episode tiles while ten episodes save; on the development machine (NVIDIA, 60 Hz) a 200-episode season scrolled with no dropped frames (p99 20.5 ms). Rerun for 2.6 with the screen on (60 Hz, NVIDIA): `--benchmark music` with 2,500 albums and the new covers (ribbons, quick actions), 0 of 1,200 frames dropped in two runs (p99 20.0 to 21.3 ms); `--benchmark songs` with 50,000 tracks, 0 dropped in two runs (p99 18.8 to 18.9 ms); `--benchmark books` with 2,000 books, 0 dropped in three runs (p99 20.5 to 21.1 ms).

**Qt version policy.** Develop against the current release, [Qt 6.12](https://doc.qt.io/qt-6.12/qt-releases.html), with 6.8 as the minimum. Official packages bundle Qt, so the version a distribution ships does not matter; Jellyfin Desktop could not support Ubuntu 24.04 because that release carries Qt 6.4. Later patch releases of Qt's long-term-support versions are reserved for commercial customers, so an open-source project should follow the newest minor release instead of staying on one.

**Matching again (2.7, issues #93 to #110)**

- *Settings → Matching (#93 to #96, as built).* Its own section after Metadata: a short explanation, **Review matches (N)** (moved here from Metadata; N counts the movies and shows waiting) and one row per library (`MatchingLibraryRow.qml`): name, source, the total (movies; shows with episodes; albums with songs and artists; books with authors), matched, needing a choice, not found and chosen by the user, and **Match again…**. The dialog (`RematchDialog.qml`) offers "Unmatched and uncertain only" (the default) or "Everything", "Keep the matches you chose yourself (N)" (on), the number of items and an estimate (TMDB about 0.25 s an item; MusicBrainz and Open Library one request a second, about 2 s an album and 1.5 s a book, which the dialog says). While a run goes on its row shows a bar, "Matching… n of N · about m min left" and **Stop**; a waiting row says which library it waits for; afterwards "Done: x newly matched, y changed, z need a choice" (or "Stopped: …"). Without a TMDB key the movie and show rows, and with online information off the music and book rows, are disabled and say why. The page asks for the status every 5 s, every second while a run goes on. Keyboard and remote: up and down move between the rows, the dialog's choices and its buttons; Enter starts, Back cancels. QML tests (`tests/qml/tst_matching.qml`): the rows' states and the dialog's choices reaching the request. Core tests (`crates/core/tests/matching_libraries.rs`): a show whose match changes keeps its episodes' watch state; a music run skips the albums the user chose and sends nothing with online information off; a book run fills only what the book's own metadata lacks.
- *Fix match for movies, shows and episodes (#97 to #100, as built).* `FixMatchDialog.qml` shows the current match (title, year, chosen automatically or by the user, or not sure yet), a **Movie / TV show** switch (← →; an episode offers only Movie), the title and year fields, which also take a TMDB or IMDb id or link, and the results with poster, title, year, kind, the start of the description and how sure the match is; the current match is marked and cannot be chosen again. **Match automatically again** runs `match_again` for the item (unlocking it) and says what happened. Choosing a result opens a confirm step from `match_preview`: Now → Becomes, what the change means, the item it merges with when another one has that TMDB id, for a show the episodes on disk the new match does not have ("Not in this show at TMDB: S03E09"; they stay as unmatched episodes), and for a kind change the files that move. A movie that is really a show: its own files (fixed) and the other movies in the same folders (ticked when their names carry episode numbers), each with a season and an episode number read from the name (`S01E02`, `1x02`, `Episode 2`, `Folge 2`, `Part 2`) and editable; a ticked file without numbers blocks the change. A show (or an episode) that is really a movie: an episode's own files are fixed, the show's other files in the same folder are ticked as versions; from a show, all its files are ticked for the user to untick. The page then moves to the item that carries the match. In the core (`crates/core/src/kinds.rs`, `crates/library/src/kinds.rs`): files move with their watch state to the existing movie or show with that TMDB id, else a new one, locked, keyed so later scans put new episodes there; movies left without a file go; episodes left without a file become ones only TMDB knows, and a show left without any file goes; the details job then fills seasons and episode titles. Tests: `tests/qml/tst_fixmatch.qml` (a stub core), `crates/core/tests/kind_change.rs`, unit tests of the moves and of the numbers read from names.
- *Matching music again (#101 to #107, as built).* `MusicMatch.qml` is shared by the album page (a **⋯** button after Save, and each song's menu), the Albums, Artists and Songs lists (their menus), the artist page and Now Playing's queue: **Match again** and **Search MusicBrainz…**. Match again on an item the user chose asks first ("Keep my choice" or "Match again"); a result for review opens the search; the result is a notice. With online information off nothing is sent and the notice says where to turn it on. The search ("Find the album", "Find the song", "Find the artist") takes the title (or name) and the artist, or a MusicBrainz link: albums list releases with type, year, country, label, formats and track count ("9 of 9 tracks"), and Enter on one compares it track by track (`match_preview`: number, title and length, yours → MusicBrainz, and the release's tracks you lack); songs list recordings with their release, year and length ("Length agrees: 6:41 → 6:41", "Length differs by 1:17", "Other artist"); artists list name, description, type, country and years with "n of your m albums are theirs" (checked for the first three, one request each). Enter again, or **Use this match**, chooses; the choice is locked. In the core (`crates/core/src/music_matching.rs`): a song is matched within its album's release when the album is matched (by disc and position with the same title, else by title), else by a recording search whose title, artist and length (within 3 s) agree; several such recordings: the one on the song's album, else review; the recording id and ISRCs are stored (`external_id`), tags never change. An artist is matched by name; several artists of that name: the one whose release groups hold the user's albums, if exactly one does (`artist_groups`), else review; a chosen or changed artist has its information looked up again and its unmatched albums queued. Following (#106, #107): when an album's release or a song's recording is applied, each credited artist of it (an album's album artist and credits; a song's own track artists, never the album artist) that has no match and is not locked takes the id the release or recording gives the same name; "Various Artists" never does. The artist keeps where it came from (`external_id` provider `coyotl`, kind `matched_via`, "album:ID" or "track:ID", moved along when albums merge), its information is looked up, and its unmatched albums are looked up again. The artist page says "Not matched yet…", "Matched through the album “…”." (or song) with **Not this artist?**, or nothing for a match of its own. Tests: `tests/qml/tst_musicmatch.qml` (a stub core) and `crates/core/tests/music_matching.rs` (follow through an album, never a locked artist, a song's own artist follows but not the album artist, recordings compared by length, artists of one name told apart, a release compared track by track, a matched artist's albums looked up again).
- *Matching audiobooks again (#108 to #110, as built).* The book page's "Search again" becomes **Match again** and **Search Open Library…** (the button row wraps in a narrow window). `BookMatch.qml` (shared by the book page and the person page) asks first for a match the user chose, opens the search for a result that needs a choice, and sends nothing with online information off. "Find the book" takes title, author and ISBN, or an Open Library link or ISBN; results show cover, title, authors, year and "Title and author agree". Enter on one shows, field by field (description, year, subjects, cover), yours, Open Library's and which is shown (`match_preview`: a field the user corrected or a local value no lookup filled stays "yours"); Enter again chooses, locked. "Find the author" shows dates, number of works, the best-known work and "n of your m books are theirs" (the author's works, `/authors/<id>/works.json`, for the first three). In the core (`crates/core/src/books_matching.rs`): an author is matched by name (`search/authors.json`), several of one name told apart by the user's books as for artists; a matched author gets the Open Library biography (`/authors/<id>.json`; kept in `item.overview` only when the person has none of its own, `meta_json.onlineBio` marks one filled online) and photo (`covers.openlibrary.org/a/id/<id>-L.jpg`, the person's poster tiers), decided by the maintainer in #110. A book's match (automatic or chosen) brings its authors without a match and not locked along: the work's `author_key`s from the search or the work, at most three looked up, matched by name (`same_author`), with "Matched through the book …" on the person page. No work is fetched only for its authors (a test counts the requests). The person page shows the photo (initials without one), the state, the biography and **Match again**, **Search Open Library…** or **Not this author?**. Tests: `crates/core/tests/author_matching.rs` (the field preview keeps local data, a chosen work brings its author with photo and biography but not a locked one, authors of one name told apart, a chosen author locked), `tests/qml/tst_bookmatch.qml`. *Privacy:* author names and Open Library author ids are sent too, under the same switch.

## 10. Build, packaging and CI

CMake is the top-level build. It compiles the C++ shell and QML, drives Cargo for the Rust workspace through the Corrosion CMake module, and links the core into the app as a static library next to the separately built libmpv.

```text
/app/qml             QML screens and components
/app/src             C++ shell: main, models, MpvPlayer, video item, OS integration
/crates/core         headless core API (shared by the bridge and the CLI)
/crates/core-api     cxx bridge
/crates/library      SQLite schema, migrations, queries
/crates/scanner      enumeration, filename parser, job queue
/crates/metadata     provider trait, TMDB client, artwork cache
/crates/vfs          Source trait and protocol backends (Cargo features)
/crates/stream       mpv stream callbacks and read-ahead
/crates/probe        media probe: FFmpeg's libavformat loaded at runtime, generated bindings
/crates/cli          headless harness for tests and debugging
/third_party/mpv     build recipes for LGPL libmpv, FFmpeg and their bundled libraries
/tests/servers       containers: Samba, NFS, WebDAV, SFTP, FTP
CMakeLists.txt       top-level build
Cargo.toml           Rust workspace
```

**Logo and icons.** The logo's master is `assets/logo/huehuecoyotl.png` (raster artwork, 1254 px). `tools/make-icons.py` generates every size from it: the hicolor icons for Linux and the Flatpak (16 to 512 px; there is no scalable SVG), the Windows `.ico`, the MSIX image assets and the 512 px image the UI scales to the screen's scale factor. Nothing is resized by hand. The logo is not under the Apache License 2.0; `assets/logo/LICENSE` states its terms.

**FFmpeg bindings for the probe.** `crates/probe/src/ffi_types.rs` holds the FFmpeg types the probe reads, generated once by `tools/gen-ffmpeg-bindings.sh` (bindgen) from the headers of the libmpv bundle and committed, so builds need neither libclang nor FFmpeg headers. The functions are looked up at runtime from the libraries the app already loaded with libmpv; a library with another major version is refused, and then the playback fallback fills in. Regenerate the file when FFmpeg's major version changes.

**libmpv bundle.** The Linux recipe builds in a container on the Ubuntu 24.04 base and ships libmpv with FFmpeg, libass, libplacebo and libwayland as shared libraries with an `$ORIGIN` rpath. libwayland is bundled because mpv 0.42 needs version 1.23 or newer and Ubuntu 24.04 has 1.22. Versions are pinned by tag and recorded by commit. The spike baseline is mpv 0.42.0, FFmpeg 8.1.3, libplacebo 7.387.0, libass 0.17.5 and libwayland 1.24.0.

| Platform | Formats | Notes |
| --- | --- | --- |
| Windows | MSIX and a portable zip; winget manifest | MSVC toolchain; `windeployqt` collects the Qt libraries. Code signing is needed to avoid SmartScreen warnings and has a recurring cost. |
| Linux | Flatpak on Flathub first; AppImage second | The Flatpak builds on the KDE runtime, which provides Qt 6; it reaches the Secret Service directly (section 6) and uses portals for files and idle inhibit. The AppImage bundles Qt. Distribution packages are left to the community. |
| macOS | Universal, notarised app bundle | Tier 2, after 1.0 |

**Formats in the bundle (2.0).** The FFmpeg recipe already builds every decoder and demuxer: it disables only encoders, muxers and devices. The bundle therefore already decodes FLAC, ALAC, PCM (WAV, AIFF), MP3, AAC, Vorbis, Opus and WavPack (checked on the 1.6 bundle); nothing has to be added for 2.0. mpv is built with PulseAudio, PipeWire and ALSA outputs and its file output (`ao=pcm`) for the tests. Issue #8 added the test that plays one generated fixture per format through the bundled libmpv (`tests/tst_musicplayback.cpp`); the recipe needed no change. Fixtures are generated in the test setup (tones or silence with tags); no copyrighted music is committed.

**Audiobooks in the bundle (2.5).** M4B is MP4 with chapters, which FFmpeg demuxes already; mpv's `scaletempo2` filter is part of mpv. `tests/tst_musicplayback.cpp` gains an M4B fixture with chapters and a two-file book played back to back without a gap. Nothing is added to the recipe.

**CI pipeline (GitHub Actions, Windows and Linux runners)**

1. Rust: format, clippy, unit tests, `cargo-deny` for licences and advisories.
2. C++ and QML: clang-format, clang-tidy, `qmllint`, Qt Test for the models.
3. Integration: source conformance suite against the protocol containers.
4. UI: Qt Quick Test for navigation and focus; scroll benchmark.
5. Native: reproducible libmpv and FFmpeg build with the licence assertions from section 3.
6. Release: packages, third-party notices, a software bill of materials.

Updates are delivered by Flathub, winget or the Store; 1.0 ships no updater of its own. There is no telemetry by default. Logs from both languages go to one rotating file (`tracing` in Rust, Qt's logging categories in C++), with an "export diagnostics" action that redacts host names and credentials.

## 11. Roadmap

Five milestones led to 1.0 and a sixth leads to 1.1. The first is a throwaway spike that tests the two riskiest assumptions before any product code exists. No dates are set until the spike is done. All design decisions except the Windows video path (section 12) are made; this version is the baseline for implementation.

&#91;embedded content: roadmap · 5 milestones, 4 gates\]

Gates are the diamonds between milestones. Until v1.2 every gate had to pass on both Windows and Linux; since 1.0 shipped for Linux alone, the Windows parts of the M0 to M4 gates are carried by the M5 gate.

**What each milestone delivers**

- **M0 Spike.** Three throwaway prototypes and one build. First, a Qt Quick window playing a 4K HEVC file through the embedded render item on Intel, AMD and NVIDIA graphics, under Windows, Wayland and X11. Second, direct mode on Windows showing HDR10 passthrough with QML controls overlaid. Third, one file read from a Samba share through the Rust stream callbacks. Plus a reproducible LGPL build of libmpv and FFmpeg. Output: a written design for direct mode, which is committed for 1.0.
- **M1 Player.** C++ shell, `MpvPlayer`, embedded video; open local files and folders; full player controls; subtitles and audio tracks; resume; keyboard and remote navigation; the `cxx` bridge, the core skeleton and CI in place.
- **M2 Library.** Scanner, filename parser with its fixture corpus, job queue, TMDB matching with a review list, poster grids, detail pages, search, watched state.
- **M3 Network.** SMB, NFS, WebDAV, SFTP and FTP sources; discovery; keystore credentials; reconnect handling; the protocol conformance suite.
- **M4 Release 1.0 (Linux).** Flatpak package, media keys, third-party notices, user documentation, translations, performance budgets met.
- **M5 Windows release** (was "Release 1.1" until v1.4). The Windows parts of the M0 to M4 gates (first run on Windows, Windows servers, Intel and AMD graphics); MSIX and winget packages; code signing; SMTC media keys; SFTP in the app. Direct mode with HDR passthrough was part of M5 until v1.3 and is now unscheduled.

**M5 status (Nov 12, 2026).** Paused: GitHub Actions has no minutes left, and the Windows build needs it (or a Windows build machine). The portable Windows package of 1.0 ran on a Windows PC without problems ([first-run checklist](testing/windows-first-run.md)). The Windows release workflow is ready in PR #31 but untested. Open: that PR, media keys (SMTC), signing, Windows SMB servers and integrated graphics.

**2.5 (planned): audiobooks.** The specification (#64) first, then in this order: the audiobook library (#65), Open Library and MusicBrainz for books (#67), and the Audiobooks category and player (#66). 2.5 is released when all four are done. The Flatpak needs no new permission.

**2.6 (Nov 18, 2026): released for Linux.** The music redesign: the top menu and the Quality lens (#78), Artists (#79), Songs (#80), playlists with migration 11 (#81), the new Now Playing with migration 12 (#82) and the full-screen Now Playing (#83). Migrations 11 and 12 only add tables and columns, so no copy of the database is kept; 2.5 refuses to open a schema-12 database. The maintainer ran the manual checklist on the test Flatpak; built and checked locally, with GitHub Actions disabled.

**2.5 (Nov 18, 2026): released for Linux.** Audiobooks: the specification (#64), the library with migration 10 (#65), Open Library and MusicBrainz for books (#67), and the Audiobooks section and player (#66). Built and checked locally, with GitHub Actions disabled; released at the maintainer's request.

**2.1 (Nov 16, 2026): released for Linux.** Versions and editions of albums and tracks (#63), with migration 9. Built and checked locally, with GitHub Actions disabled; the migration was checked on a copy of the maintainer's library.

**2.0 (Nov 16, 2026): released for Linux.** Music, from the seven issues below. The manual Hi-Res checklist (`docs/testing/hires-audio.md`) was run by the maintainer with a USB DAC and passed. Built and checked locally, with GitHub Actions disabled.

**2.0 (planned): music.** The specification (#6) comes first; then, in this order: the music library from tags (#7), MusicBrainz and the Cover Art Archive (#11), the Music section (#10, which can start once #7 is merged), the play queue, gapless playback and Now Playing (#8), TheAudioDB and Fanart.tv (#12), and the bit-perfect output (#9). 2.0 is released when all seven are done (decided in v1.9); #9's manual checklist needs a USB DAC with a rate display and is run by the maintainer. Where an issue's work is Windows-only (SMTC metadata for music, WASAPI exclusive mode) it is built with the Windows release.

**1.6.1 (Nov 16, 2026): released for Linux.** The TMDB key status in Settings (#52). The key was never lost: the core loaded it at start-up, but Settings asked before that and was not told afterwards.

**1.6 (Nov 16, 2026): released for Linux.** Episode tiles on the show page with versions, technical details and the Save menu (#48). Built and checked locally, with GitHub Actions disabled; scrolling measured on this machine's NVIDIA graphics, not on integrated graphics.

**1.5 (Nov 16, 2026): released for Linux.** Offline copies (Save), Save as… and the download engine. Built and checked locally, with GitHub Actions disabled.

**1.2 (Nov 15, 2026): released for Linux.** Technical details for movies and episodes, and a version selector. Built and checked locally, with GitHub Actions disabled.

**1.1 (Nov 12, 2026): released for Linux.** SFTP, the logo, cover sizes, one item per show (with the repair of older libraries) and the show page. Built and checked locally, since GitHub Actions was unavailable.

**M4 status (Nov 11, 2026): released as 1.0.0 for Linux.** Flatpak, MPRIS media keys, diagnostics export, third-party notices, user guide, German translation, and the startup and scroll budgets measured within limits on one machine; used without errors by the maintainer. See the [M4 status](milestones/M4.md).

**M3 status (Nov 11, 2026).** SMB, NFS, WebDAV and FTP libraries work end to end on Linux; SFTP joined them in 1.1 (#26). Windows servers are untested. See the [M3 status](milestones/M3.md).

**M0 status (Nov 10, 2026).** First run on Linux, X11 and NVIDIA: the LGPL build, the embedded item and Samba streaming pass, and the direct mode design is drafted. Still open for the gate: everything on Windows (including HDR passthrough), Intel and AMD graphics, and Wayland. See the [M0 report](spike/M0-report.md).

After 1.1, in rough priority order: direct mode on Linux (which needs one of the routes in 5.6, since the LGPL build has no X11 backend), macOS, media-server sources, watch-state sync.

## 12. Risks and open decisions

The two video paths are the risk that could still change the architecture, so both are prototyped before feature work starts.

| Risk | Why it matters | Mitigation |
| --- | --- | --- |
| Embedded video needs an OpenGL scene graph | It forces OpenGL on Windows, where Qt defaults to Direct3D, and driver quality varies | Spike across Intel, AMD and NVIDIA; mpvqt as a proven reference; direct mode as a second path. On Windows, direct mode for all playback would remove the risk there (open decision below). |
| HDR direct mode | Two stacked windows are fiddly (focus, resize, stacking), mpv cannot embed on Wayland, and another Qt and mpv project cited HDR as a reason to rewrite | Prototype in the spike; Windows only. Postponed in v1.3 until HDR hardware is available for the tests; until then HDR is tone-mapped on every platform |
| Two languages | Contributors need C++ and Rust, and the bridge adds friction | Keep the C++ layer thin and free of logic; stable `cxx` instead of a pre-1.0 binding |
| SMB and NFS crates below 1.0 | NAS playback is the main use case | Conformance suite; pinned versions; C-library fallbacks behind the same trait |
| Licence contamination | One GPL crate or GPL-only Qt module changes the licence of every binary | `cargo-deny`, the Qt module allowlist and build assertions in CI |
| Qt version drift | Distributions ship old Qt versions, and late long-term-support patches are commercial-only | Bundle Qt in official packages; follow the newest minor release |
| TMDB key and terms | Every user needs their own key, which adds a setup step before the library looks good | Guided first-run setup with key validation; provider trait; local `.nfo` support |
| Scope | The commercial reference product has years of work behind it | Milestone gates; non-goals list; no mobile work |
| Codec patents | Separate from copyright licences | Prefer hardware and OS decoders; take advice before store distribution |

**Open decisions**

- [x] Project name: Huehuecoyotl, with `coyotl` as the short form (see below)
- [x] Video item: own implementation, with mpvqt as a reference only (section 5.1)
- [x] Direct mode and HDR passthrough on Windows: postponed without a planned version (v1.3; was 1.0, then 1.1 in v1.2)
- [x] 1.0 ships for Linux alone; Windows follows as 1.1 (v1.2)
- [x] Direct mode on Wayland: embedded mode in 1.0, a separate mpv window as a later opt-in (section 5.6)
- [x] TMDB key policy: user-supplied only, entered in the Settings menu
- [x] Governance: DCO, Contributor Covenant and CI-only releases (section 13)
- [x] **Music, online lookups by default** (2.0, v1.9): online. MusicBrainz and Cover Art Archive lookups start with the first music folder; Settings → Metadata can limit music information to file tags (no request then). The recommendation had been to ask first.
- [x] **Music, keys for TheAudioDB and Fanart.tv** (issue #12, v1.9): option A, each user's own keys in the OS keystore, both providers off until entered. Project keys (option B) were not chosen.
- [x] **Music, direct ALSA access in the Flatpak** (issue #9, v1.9): no manifest change; under Flatpak bit-perfect mode uses PipeWire with configured rates. Revisit if users ask for it.
- [x] **Music, scope of 2.0** (v1.9): 2.0 is released when all seven music issues are done, bit-perfect mode (#9) and the optional providers (#12) included.

- [x] **Audiobooks, Open Library contact** (#67, v1.12): the User-Agent names the app, its version and the project URL, no email or phone; the limiter keeps to the 1 request per second Open Library allows without a contact. Not chosen: a contact email for 3 requests per second.
- [x] **Audiobooks, online lookups** (#67, v1.12): books share the music switch ("Music and book information online"), on by default as for music. Not chosen: a separate switch, off by default.
- [x] **Audiobooks, playback** (#66, v1.12): a third audio-only mpv handle for books; the music queue is never touched by a book. Not chosen: sharing the music handle.
- [x] **Audiobooks, author photos and biographies** (#110, v1.14): a matched author shows the Open Library photo and biography; the person's own metadata wins. Not chosen: the id only.
- [x] **Audiobooks, Audible data** (#67, v1.12): no Audible-based provider (such as Audnexus); it can be reconsidered after a terms review. 2.5 uses local metadata, Open Library and MusicBrainz.
- [ ] **Audiobooks, Open Library's terms**: the developer pages allow on-demand lookups by open-source apps with caching; the Internet Archive's terms of service (archive.org/about/terms.php) could not be read automatically on Nov 16, 2026. Recommendation: the maintainer reads them before 2.5 is released. 2.5 was released on Nov 18, 2026 at the maintainer's request; this item stays open until the maintainer confirms the terms review.
- [ ] **Audiobooks, release**: recommendation: 2.5 is released when #64 to #67 are done, as 2.0 was.
- [x] **Music, two mpv handles** (5.7, v1.9): an audio-only handle for music next to the video one.
- [ ] Windows video path: direct mode for all playback (SDR and HDR) so Qt Quick stays on Direct3D 11, or embedded mode for SDR with Qt forced to OpenGL. Decided by tests W3 and W3b of the [direct mode design](spike/direct-mode-design.md).

**Project name: Huehuecoyotl.** The app is named after the Aztec god of music, dance and storytelling, who is depicted as a dancing coyote. The README should explain the name in one respectful line, since it comes from a living Nahua tradition.

| Use | Name |
| --- | --- |
| Project and app name | Huehuecoyotl |
| Command, binary, config folder | `coyotl` |
| Internal Rust crates | `huehuecoyotl-core`, `huehuecoyotl-vfs` and so on |

A web search, which is not a legal clearance, found no software product or trademark called Huehuecoyotl. The short form has one collision: an unrelated, inactive noise-generator library is published as [`coyotl` on crates.io](https://docs.rs/crate/coyotl/latest), which is why the internal crates carry the long prefix. The app itself is distributed through Flathub, winget and downloads, not crates.io.

Still to do before the first public release: search the DPMA, EUIPO and USPTO registers, check the Debian, Arch and Fedora package lists for an existing `coyotl` binary, and register the domain.

Names considered and dropped because they were taken or misleading: OpenFuse, OpenMediaLib (OML), Open Media Fuse Player (OMF), Spider Player, Bragi, OpenReel, Cinethek.

## 13. Governance

Start with the lightest structure that keeps contributions legally clean and releases trustworthy: a DCO, a standard code of conduct, and releases that only CI can build.

**13.1 Contribution terms: DCO or CLA**

|  | DCO (Developer Certificate of Origin) | CLA (Contributor License Agreement) |
| --- | --- | --- |
| What it is | A `Signed-off-by` line in each commit, by which the contributor certifies they have the right to submit the code under the project licence | A separate agreement each contributor signs, granting the project owner broad rights over the contribution |
| Effort | `git commit -s` and a CI check | A signing bot, record keeping, and in practice a legal entity |
| Effect on contributors | Almost no friction | Deters some, especially employees who need their employer's approval |
| Relicensing later | Needs every contributor's consent | The owner can relicense or sell commercial licences |

Recommendation: DCO. Apache-2.0 already states that contributions come under the same licence and includes a patent grant. A CLA mainly helps with dual licensing, which is not a goal here.

**13.2 Code of conduct**

Adopt the Contributor Covenant unchanged, with a contact address that reaches the maintainers. It states the expected behaviour and how reports are handled. It costs nothing now and is hard to introduce in the middle of a conflict.

**13.3 Roles and releases**

- **Decisions.** At the start the founder is the sole maintainer and has the final say. Say so in `GOVERNANCE.md`; unclear authority causes more trouble than concentrated authority.
- **Maintainers.** Invited after sustained, good contributions. They get merge rights and are listed in a `MAINTAINERS` file.
- **Changes.** Everything goes through pull requests with CI passing. Once there are two maintainers, each change needs a review from the other. Larger changes, such as architecture or a dependency with a new licence, start as a short written proposal in an issue.
- **Releases.** Only maintainers create release tags. A signed tag on the main branch triggers CI, which builds, signs and publishes every package. Nothing is built on a personal machine, signing keys live only in CI secrets, and all maintainer accounts use two-factor authentication.
- **Versions.** Semantic versioning and a changelog kept with each pull request.
- **Security.** A `SECURITY.md` with a private reporting channel; fixes ship before details are published.
- **Name and accounts.** The project name, domain and store accounts are held by the founder at first. Record who else has access, so the project survives one person becoming unavailable. If the project grows, these can move to a fiscal host or foundation.

Files to add in the first commit: `LICENSE`, `NOTICE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `GOVERNANCE.md`, `SECURITY.md`, `MAINTAINERS`.

## Sources

- [mpv Copyright file](https://github.com/mpv-player/mpv/blob/master/Copyright): GPL default, LGPL mode and what it disables
- [Qt licensing](https://doc.qt.io/qt-6/licensing.html) and [Qt's LGPL obligations FAQ](https://www.qt.io/faq/3.7.-what-are-my-obligations-when-using-qt-under-the-lgpl): LGPLv3 modules, GPL-only modules, obligations
- [Qt releases](https://doc.qt.io/qt-6.12/qt-releases.html): current versions and long-term-support policy
- [QQuickRhiItem documentation](https://doc.qt.io/qt-6.11/qquickrhiitem.html): Qt Quick custom rendering and the OpenGL-only predecessor
- [Jellyfin, State of the Fin, January 2026](https://jellyfin.org/posts/state-of-the-fin-2026-01-06): Jellyfin Desktop on Qt 6 and mpvqt
- [jellium-desktop issue 609](https://github.com/andrewrabert/jellium-desktop/issues/609): HDR and Qt maintenance as reasons for a rewrite
- [CXX-Qt on GitHub](https://github.com/KDAB/cxx-qt): Rust and Qt binding options
- [smb crate on docs.rs](https://docs.rs/crate/smb/latest): version, dialects, authentication
- [nfs-rs README](https://docs.rs/crate/nfs-rs/latest/source/README.md): protocol versions, licence, privileged ports
- [Apache OpenDAL core](https://opendal.apache.org/core): FTP, SFTP and WebDAV services
- [TMDB API FAQ](https://developer.themoviedb.org/docs/faq): non-commercial use and attribution

* [mpv releases](https://github.com/mpv-player/mpv/releases) and [mpv issue 10810](https://github.com/mpv-player/mpv/issues/10810): HDR on Wayland in mpv's own window; limits of the render API
* [openfuse crate on docs.rs](https://docs.rs/crate/openfuse/latest): existing project of that name, one reason OpenFuse was dropped

- [mpvqt source, `mpvabstractitem.h`](https://lxr.kde.org/source/libraries/mpvqt/src/mpvabstractitem.h): item built on the OpenGL framebuffer class; licence
