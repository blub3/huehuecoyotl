# Changelog

All notable changes are listed here. The format follows Keep a Changelog; versions follow SemVer.

## Unreleased

- Music: a new layout (#78, part of 2.6). A top menu with **Albums**, **Artists** and **Genres**, and **Search music** next to it. The **Quality lens** filters albums by **Hi-Res**, **Lossless**, **Several versions** and **Saved** (they combine, and each shows how many albums it finds), together with a genre and a decade; sort by recently added, A–Z, year or most played. Every cover shows the format of the version that plays, gold for Hi-Res. Play, Shuffle and More appear on the focused cover. Sorted A–Z, a letter rail jumps to an initial. **Genres** lists the genres of your music.

## 2.5.0 – 2026-11-18 (Linux)

Linux release as a Flatpak bundle. Built locally (GitHub Actions is disabled). Audiobooks: library folders can now hold audiobooks.

- Audiobook library (#65, part of 2.5): library folders can hold audiobooks. Adding a folder asks for **Movies and shows**, **Music** or **Audiobooks**, and a network share's **Contains** offers audiobooks too. Each book is one file (M4B, M4A, MP3, FLAC, Ogg, Opus) or one folder of files, with `CD1`/`Disc 1` subfolders in order, on one timeline. Title, authors, narrators, series and number, publisher, year, language, ISBN and description come from `metadata.opf`, `desc.txt`/`reader.txt`, `.nfo`, the tags and the names, in that order. Chapters come from the files (M4B and MP3 chapters), else one per file. Covers from the tags or `cover.jpg`. Audible `.aax`/`.aaxc` books are listed and marked as protected. Rescans read only changed books, and only file headers. Also through `coyotl-cli library add PATH --audiobooks`, `books`, `authors`, `series` and `book ID`.
- Book information online (#67, part of 2.5): audiobooks are looked up at Open Library, by ISBN when the book's metadata has one (one search), else by title and author, at most one request per second with the app's name in every request. Online data fills only what your files leave empty (description, year, language, subjects) and gives books without a cover one; it never replaces your own metadata. Books tagged by MusicBrainz Picard are looked up at MusicBrainz and the Cover Art Archive. Books with several possible matches wait for a choice: **Search again** on the book's page (or `coyotl-cli book search ID` and `book choose ID WORK`). The setting is now **Music and book information online**; off, nothing is sent. Attribution under Settings → About.
- Audiobooks category and player (#66, part of 2.5): **Audiobooks** in the navigation rail with Books, Authors, Series and Narrators; the book grid shows square and portrait covers side by side; filters for unread, in progress and finished books. Book pages with cover, authors, narrators, series, duration, time left, description and chapters; Play or Resume, Mark as finished, Save and Save as… (all of a book's files) and Search again. Series pages list the books in order with their progress, with **Continue series**. The player keeps one timeline across a book's files without gaps and resumes exactly where you stopped, also inside a later file; chapters (previous, next, list, time left in the chapter); skip back and forward; speed from 0.5× to 3.0× without a change of pitch, remembered per book; a sleep timer (5 to 120 minutes or the end of the chapter) that fades out. Home shows **Continue listening**. Starting a book pauses the music and the reverse; the mini player and the media keys follow what played last. Settings → Playback: audiobook speed, skip back and forward. `coyotl --benchmark books` measures the book grid.
- The database moves to schema 10 on first start (the audiobook tables). A copy of the previous database is kept as `library.db.before-10`.

## 2.1.0 – 2026-11-16 (Linux)

Linux release as a Flatpak bundle. Built locally (GitHub Actions is disabled).

- Album versions (#63, part of 2.1): an album that is in your library more than once — in two formats, on two shares, or as a remaster next to the original — appears once, with a mark showing how many versions it has. The album page chooses the version (format, size and where it is, or the edition such as "2011 Remaster"); Play, Shuffle, the track list, the Hi-Res badge and Save follow it. The album opens with the version played last, else the one **Preferred version** asks for, now also **Lossless only**. A track with two files offers both in its menu. **This is a different album** splits a wrong match off for good. The queue remembers the exact files, also after a restart, and plays another version of a track whose file cannot be reached.
- The database moves to schema 9 on first start: copies of one album are merged, with their play counts, the queue and saved copies. A copy of the previous database is kept as `library.db.before-9`.

## 2.0.0 – 2026-11-16 (Linux)

Linux release as a Flatpak bundle. Built locally (GitHub Actions is disabled). Music: library folders can now hold music.

- Music library (#7, part of 2.0): library folders have a content type, video or music. Music folders are scanned from their tags (FLAC, ALAC, WAV, AIFF, MP3, AAC, Ogg Vorbis, Opus, WavPack) into artists, albums and tracks, reading only the tag blocks of each file, with multi-disc albums, compilations, several artists per track, names from folders and file names for untagged files, and covers from the tags or a `cover.jpg`. So far through `coyotl-cli library add PATH --music` and `coyotl-cli music`; the Music section follows.
- Music information online (#11, part of 2.0): albums are looked up at MusicBrainz, one request for albums tagged with their MusicBrainz id, a search otherwise; an album is matched only when the results clearly agree with it, else it goes to "Review matches". Albums without a cover get one from the Cover Art Archive. Settings → Metadata → "Music information online" turns this off (file tags only, nothing sent). Attribution under Settings → About.
- Music section (#10, part of 2.0): **Music** in the navigation rail with Albums, Artists and Recently added; album pages with the tracks by disc, format line, Hi-Res badge, Play, Shuffle and Save; artist pages with albums by release type and "Appears on". Search shows movies, shows, artists, albums and tracks in rows by kind. Home shows recently added albums. Adding a folder asks whether it holds video or music. Works from the local database alone.
- Grids keep loading while much background work runs: a refresh no longer cancels the page being loaded. Music files are not analysed with the media probe (their format comes from the tags). `coyotl --benchmark music` measures scrolling the Albums grid.
- Music playback (#8, part of 2.0): a play queue that keeps playing while you browse, with Play, Shuffle, Play next and Add to queue on albums and tracks, repeat (all or one), and gapless playback between tracks of the same format, also from network shares. Now Playing shows the cover, the format, the seek bar and the queue (reorder, remove, clear); a mini player at the bottom of every screen leads there. The queue and its position come back after a restart. Media keys and the desktop's media controls (MPRIS) show title, artist, album and cover. Starting a video pauses the music. Plays are counted per track, and Home shows "Recently played". Settings → Playback → ReplayGain (off, track or album).
- Artist images and biographies (#12, part of 2.0): optionally, with your own TheAudioDB and Fanart.tv keys (Settings → Metadata), artist pages show the artist's image, logo, background and biography (in your language, else English), and albums without a cover get one, plus disc art. Keys are checked when entered, kept in the system keystore and never logged; without keys nothing is sent to either service. Credits under Settings → About while in use. `coyotl-cli music keys` and `music key PROVIDER`.
- Bit-perfect music (#9, part of 2.0): Settings → Audio → **Bit-perfect music** sends music to the device unchanged (no resampling by the app, no ReplayGain, volume fixed at 100 %, exclusive access on direct ALSA devices), and **Music output** chooses the device for music. Now Playing shows the signal path: source and output format, "Bit-perfect" only when it is, and what the system does otherwise. A device that cannot be opened falls back to the default with a note, never silence. Music has its own volume (Now Playing, media controls). The user guide explains PipeWire's allowed rates; `docs/testing/hires-audio.md` is the checklist for a USB DAC.
- Settings scroll to the focused control when moving with the keyboard or a remote.
- The database moves to schema 8 on first start (the music tables). A copy of the previous database is kept as `library.db.before-8`.

## 1.6.1 – 2026-11-16 (Linux)

Linux release as a Flatpak bundle. Built locally (GitHub Actions is disabled).

- Fixed (#52): the TMDB key field in Settings was empty after every restart, although the key was stored and in use. The app asked for the key's state before it had read the system keyring and was not told afterwards. Settings now shows "Key stored", where the key comes from and when TMDB last accepted it, with Replace key, Test key and Remove key. If no system keyring is available, Settings explains what to do instead of showing an empty field. The key is never shown or logged.
- `coyotl-cli key status`, `key test` and `key remove`.

## 1.6.0 – 2026-11-16 (Linux)

Linux release as a Flatpak bundle. Built locally (GitHub Actions is disabled).

- Show page (#48): every episode has its own version dropdown (when it has several files), technical summary with badges, "Details" panel and Save menu (Save, Save as…, Delete saved copy), with progress and saved or outdated marks on the episode. The selected version drives Play, the details and saving. Right moves into an episode's buttons, Back returns, Menu opens its Save menu. Long seasons load page by page as you scroll.
- Movie and episode pages: Save, Save as… and Delete saved copy are now one Save menu.
- A version whose server is offline but which has a saved copy can be chosen and plays the copy.
- `coyotl --benchmark episodes` measures scrolling a show's episodes while ten of them save.
- `coyotl-cli episodes SEASON` lists a season's episodes with their versions and saved state.

## 1.5.2 – 2026-11-16 (Linux)

- Fixed: after choosing a file or folder in the "Save as…" or storage-folder dialog, the app no longer reacted to the keyboard or a remote until it was clicked.
- Downloads: a saved copy whose file was removed from the server says so and no longer offers "Save again".

## 1.5.1 – 2026-11-16 (Linux)

- Fixed (Flatpak): "Save as…" of a single file tried to write its subtitle files next to it, which the desktop portal does not allow; a hidden temporary file was left in the target folder. Subtitles are now skipped in that case and the app says so; exporting a season into a folder includes them.
- Fixed: the "Saving a copy to …" notice after "Save as…" did not appear.

## 1.5.0 – 2026-11-16 (Linux)

Linux release as a Flatpak bundle. Built locally (GitHub Actions is disabled).

- Save (#4): keep an offline copy of a movie, an episode, a season or a whole show from a network source. It plays from the copy, also when the server is off, and keeps resume and watched state. The detail page shows progress with pause, resume and cancel; "Delete saved copy" removes only the local copy. Saved items have a mark on their cover, and Movies and Shows filter All, Unwatched or Saved. A new Downloads screen lists copies in progress, failed, outdated and done, with the space used; Settings → Storage sets the folder and deletes all saved copies.
- Save as… (#5): copy a movie or episode to a file you choose, or a season or show into a folder, for example on a USB drive. Subtitles go along under the new name. The app keeps no record of exported files.
- Download engine (#3): copies continue after a dropped connection or a restart, are checked against the library's fingerprint, and never fill the disk (a full disk is its own error and leaves no partial file). Playback reads go before downloads and the media analysis. `coyotl-cli download`, `downloads` and `export` drive it headless.

## 1.2.0 – 2026-11-15 (Linux)

Linux release as a Flatpak bundle. Built locally (GitHub Actions is disabled).

Known limitations: no Windows release; a real Dolby Vision file has not been tested; episodes
with several versions use the movie code path without their own test library.

- Versions (#34): a movie or episode with several files has a version selector with labels such as "Director's Cut · 2160p · Dolby Vision · HEVC · TrueHD Atmos · 61 GB · NAS". The page preselects the version played last, else the highest quality (or the smallest file, Settings → Playback). Offline versions are listed but cannot be played. Versions of the same cut share the resume point; another cut keeps its own. "This is a different movie" splits a wrongly merged version into its own entry.
- Fixed: a switched-off server answering "No route to host" was not marked offline.

- Technical details for movies and episodes (#33): a summary line ("4K · HDR10 · HEVC 10-bit · TrueHD Atmos 7.1 · 3 subtitles") with badges, and a collapsible panel with file, video, audio and subtitle details. Files are analysed in the background with the FFmpeg libraries the app already ships, also on network shares (only the start of each file is read), and playback fills in what the analysis could not read. `coyotl-cli probe FILE` prints the analysis.

## 1.1.0 – 2026-11-12 (Linux)

Linux release as a Flatpak bundle. The Windows release follows without a version number yet.

Known limitations: no Windows release; FTPS, Windows servers and integrated graphics are untested;
no gamepad support; the Flatpak bundle is unsigned and not on Flathub. Built locally rather than
by CI (GitHub Actions was unavailable).

- Development builds show their commit, e.g. "1.1.0 (development build 7870d67)" in Settings → About and `1.1.0-dev+7870d67` for `coyotl --version`; only the tagged release shows the plain version.

- Show page (#32): one page per show with a season switcher (tabs, or a list for many seasons) and the selected season's episodes, including the ones TMDB knows but your library does not have (dimmed). The main button plays the next episode, resumes a started one, or offers "Play again". Mark episodes, seasons or the whole show watched; left/right switch seasons, up/down move through episodes, W marks watched.

- Fixed: shows appeared many times in the Shows grid, once per season release folder or even once per episode (#32). The scanner now names a show after the folder above season, release and per-episode folders (or the release name), and the TMDB id is unique per show and movie. Existing libraries are repaired on upgrade without network access: duplicates are merged, watch state is kept. Show matching now handles punctuation differences, country suffixes ("US", "UK"), folder years, and titles used by several shows (decided by the seasons and episodes on disk).

- The Huehuecoyotl logo replaces the placeholder "H" in the sidebar and appears on the welcome card and in Settings → About (#1). All icon sizes, including the MSIX image assets, are generated from `assets/logo/huehuecoyotl.png`; the logo has its own licence (`assets/logo/LICENSE`).
- Cover sizes for windowed and fullscreen layouts (#2): Settings → Appearance sets Small, Medium or Large for each mode (defaults Medium and Large). Grids and the "Recently added" row work out their columns from it, and the focused cover stays in view when switching modes. A sharper 540 px thumbnail tier is downloaded with new posters and made offline for existing libraries.
- SFTP sources in the app (#26): password or SSH key login (Ed25519, ECDSA; the passphrase is kept in the keystore), and the server's host key fingerprint shown before adding, then pinned. Discovered `_sftp-ssh` servers now appear in the list. To build SFTP together with SMB, `third_party/smb` carries the published `smb` 0.12.1 with only its `sspi` dependency raised (upstream fix pending); CI checks that nothing else differs.

## 1.0.0 – 2026-11-11 (Linux)

First stable release, for Linux, as a Flatpak bundle. Windows follows in 1.1 (milestone M5).

Known limitations: no Windows release yet; SFTP shares are not in the app; Windows servers,
FTPS and integrated graphics are untested; no gamepad support; the Flatpak bundle is unsigned
and not on Flathub.

- Fixed: the app crashed after leaving the player and moving around the menu. The video renderer stayed connected to the window after it was freed.
- Fixed: `packaging/flatpak/build.sh --install` wrote a desktop entry that pointed at the sandboxed builder's `flatpak`, so the app was missing from the application menu.
- New app icon (a coyote with a drum and rattle), installed as PNG sizes from 16 to 512 px for Linux and the Flatpak, set as the window icon, and embedded in `coyotl.exe` on Windows. `tools/make-icons.py` regenerates every size from `packaging/icons/source.webp`.

## 0.9.0 – 2026-11-11 (Linux preview)

First release, for Linux only, as a Flatpak bundle. Windows and HDR follow in 1.0.

Known limitations: no Windows build yet; SFTP shares are not in the app; Windows servers,
FTPS and integrated graphics are untested; no gamepad support; the Flatpak bundle is
unsigned and not on Flathub.

- M4 release work (Linux): Flatpak manifest building the LGPL libmpv stack from pinned sources, desktop entry, AppStream metadata and icon; MPRIS media keys; one rotating log file for Rust and Qt with a redacted "Export diagnostics" action; third-party notices generated at build time and shown under Settings → About; startup and scroll benchmarks with a 5,000-movie seeder, run in CI; an index that cuts the Home screen's "recently added" query from 1.2 s to milliseconds on large libraries; user guide; release workflow with SBOM and checksums.
- M3 network: SMB, NFS, WebDAV and FTP library sources, an "Add network share" page with mDNS discovery and a folder picker, share passwords in the OS keystore, `coyotl://` streaming to mpv with adaptive read-ahead and retry, offline tracking with automatic reconnect, a protocol conformance suite against containerised servers in CI. The SFTP backend is built and tested but not in the app yet.
- M2 library: library folders, scanner with move detection, filename parser with an 89-name corpus, persisted job queue, TMDB matching with a review list and "fix match", poster grids, detail pages for movies, shows, seasons and episodes, full-text search, watched state for whole shows, TMDB key in the OS keystore, navigation rail.
- M1 player: Qt Quick shell with embedded mpv video, local files and folders, tracks, resume, keyboard navigation, Rust core over a `cxx` bridge.
