# Changelog

All notable changes are listed here. The format follows Keep a Changelog; versions follow SemVer.

## Unreleased

- M3 network: SMB, NFS, WebDAV and FTP library sources, an "Add network share" page with mDNS discovery and a folder picker, share passwords in the OS keystore, `coyotl://` streaming to mpv with adaptive read-ahead and retry, offline tracking with automatic reconnect, a protocol conformance suite against containerised servers in CI. The SFTP backend is built and tested but not in the app yet.
- M2 library: library folders, scanner with move detection, filename parser with an 89-name corpus, persisted job queue, TMDB matching with a review list and "fix match", poster grids, detail pages for movies, shows, seasons and episodes, full-text search, watched state for whole shows, TMDB key in the OS keystore, navigation rail.
- M1 player: Qt Quick shell with embedded mpv video, local files and folders, tracks, resume, keyboard navigation, Rust core over a `cxx` bridge.
