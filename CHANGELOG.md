# Changelog

All notable changes are listed here. The format follows Keep a Changelog; versions follow SemVer.

## Unreleased

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
