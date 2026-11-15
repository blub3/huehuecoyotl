# Huehuecoyotl: GitHub issues for versions 1.1 to 4.0

20 issues, written on 2026-11-11 against specification v1.2. Each issue stands on its own, so that Claude can work on it without this conversation. `create-issues.sh` creates the labels, the milestones and all issues with the GitHub CLI.

| # | Milestone | Title | Labels |
| --- | --- | --- | --- |
| 1 | 1.1 | Show the Huehuecoyotl logo instead of the placeholder "H" | feature, ui, needs-human |
| 2 | 1.1 | Cover sizes for windowed and fullscreen layouts | feature, ui, core |
| 3 | 1.5 | Core: download engine and storage for offline copies | feature, core |
| 4 | 1.5 | "Save": keep an offline copy, play from it, delete it | feature, ui, core |
| 5 | 1.5 | "Save as…": export a media file to a chosen location | feature, ui, core |
| 6 | 2.0 | Specification: add music to the scope and the data model | design, needs-human |
| 7 | 2.0 | Music library: read tags and group tracks into albums and artists | feature, core |
| 8 | 2.0 | Music playback: play queue, gapless playback and Now Playing | feature, playback, ui, core |
| 9 | 2.0 | Hi-Res audio: output at the file's native sample rate and bit depth | feature, playback, needs-human |
| 10 | 2.0 | Music UI: Artists, Albums, tracks and search | feature, ui |
| 11 | 2.0 | Music metadata: MusicBrainz and Cover Art Archive | feature, metadata, core |
| 12 | 2.0 | Music metadata: TheAudioDB and Fanart.tv as optional providers | feature, metadata, core, needs-human |
| 13 | 3.0 | macOS: LGPL build of libmpv and FFmpeg as a universal bundle | build, platform-macos |
| 14 | 3.0 | macOS: build and run the app, and validate the video path | feature, playback, platform-macos, needs-human |
| 15 | 3.0 | macOS: system integration | feature, ui, platform-macos |
| 16 | 3.0 | macOS: signed and notarised universal release | build, packaging, platform-macos, needs-human |
| 17 | 4.0 | Decide the architecture for iOS and tvOS | design, platform-apple-mobile, needs-human |
| 18 | 4.0 | Core for iOS and tvOS: build targets, Swift bindings and libmpv frameworks | build, core, platform-apple-mobile, blocked |
| 19 | 4.0 | iOS and iPadOS app | feature, ui, platform-apple-mobile, blocked, needs-human |
| 20 | 4.0 | tvOS app | feature, ui, platform-apple-mobile, blocked, needs-human |

---

# Version 1.1

Windows release (M5 in the specification), the logo and cover sizes.

## 1. Show the Huehuecoyotl logo instead of the placeholder "H"

**Milestone:** 1.1 · **Labels:** feature, ui, needs-human

### Context

The sidebar header shows a yellow circle with the letter "H" next to the app name. That badge is a placeholder. The app should show the real Huehuecoyotl logo there and everywhere else the app identifies itself.

### Input from the maintainer

The logo is supplied by the maintainer as a master file at `assets/logo/huehuecoyotl.svg` (square, transparent background). If that file is not in the repository, stop and ask for it in a comment. Do not draw a logo, generate one, or take an image from the web.

### What to build

- Sidebar header: the logo replaces the "H" badge at the same size and position, in the windowed and the fullscreen layout. Render it from SVG (Qt Svg is on the module allowlist) so it stays sharp at every scale factor.
- Window and taskbar icon, the About screen and the first-run screen use the same logo.
- Linux: icons in the hicolor theme (scalable plus the usual raster sizes), referenced by the `.desktop` file, the Flatpak manifest and the AppStream metadata.
- Windows (released with 1.1): an `.ico` with sizes from 16 to 256 px as the executable's icon, and the image assets the MSIX package needs.
- All derived sizes are produced from the one master file by a script in the repository (for example `tools/make-icons.sh`). Nothing is resized by hand.
- The badge gets an `Accessible.name`.

### Acceptance criteria

- [ ] No "H" placeholder remains anywhere in the app or the packages.
- [ ] The logo is sharp at 100 %, 150 % and 200 % scaling.
- [ ] The Flatpak shows the logo in the desktop launcher and the window switcher.
- [ ] On the About screen the TMDB logo stays less prominent than the app's own logo (specification, section 7).
- [ ] The pull request contains screenshots of the sidebar in windowed and fullscreen mode.

### Out of scope

- Designing the logo.
- A splash screen or animated logo.

### For the maintainer

- Provide the master SVG and state the licence of the logo (logos are often kept out of the Apache-2.0 grant; record the choice in `assets/logo/LICENSE`).
- Confirm the look from the screenshots.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 2. Cover sizes for windowed and fullscreen layouts

**Milestone:** 1.1 · **Labels:** feature, ui, core

### Context

Poster grids use one cover size regardless of how the app is displayed. In a window on a desk the user sits close and wants many covers on screen. In fullscreen on a television the user sits far away and needs larger covers.

### Assumption to confirm

This issue reads the request as: the cover size differs between windowed and fullscreen mode, and the user can adjust each of the two. If only an automatic switch is wanted, drop the settings part.

### What to build

- Every poster grid (Home rows, Movies, Shows, Search results) has a target cover width that depends on the display mode.
- Settings, under Appearance: "Cover size in window" and "Cover size in fullscreen", each with Small, Medium and Large. Defaults: Medium in a window, Large in fullscreen. Changes apply at once and are remembered.
- The number of columns follows from the available width and the target cover width. Covers keep their 2:3 shape, no column is cut off at the edge, and spacing and title text scale with the cover.
- Switching between window and fullscreen keeps the focused item focused and visible. The grid does not jump back to the top.
- Thumbnails: the core writes pre-scaled thumbnails and QML never decodes an original poster in a grid (specification, sections 7 and 9). Add thumbnail tiers so that the largest cover size is sharp at a scale factor of 2, and let QML request the smallest tier that covers the displayed size. For existing libraries a background job creates missing tiers from the stored originals, without network access.

### Acceptance criteria

- [ ] Six combinations work: three sizes in each of the two modes.
- [ ] The focused item survives a mode switch in both directions.
- [ ] The scroll benchmark meets the budget (60 Hz, 5,000 items, integrated graphics) at every size in both modes.
- [ ] After an upgrade, covers are never blurry or missing while the new tiers are being generated; the grid falls back to the nearest existing tier.
- [ ] The settings are reachable and usable with keyboard and remote.

### Out of scope

- A list view, per-library sizes, backdrop sizes.
- A free zoom slider or zoom shortcuts.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


---

# Version 1.5

Media file download: Save and Save as.

## 3. Core: download engine and storage for offline copies

**Milestone:** 1.5 · **Labels:** feature, core

### Context

Version 1.5 adds two ways to copy a media file from a network source to the local machine:

- **Save** keeps an offline copy inside storage that the app manages. The app knows about it, plays from it and can delete it.
- **Save as…** writes a copy to a place the user chooses. After that the file belongs to the user; the app does not track it and cannot delete it.

This issue builds the shared engine in the Rust core. The two user-facing features are separate issues.

### What to build

- A new job kind `download` in the existing job queue: persisted, resumable after a restart, with pause, resume and cancel by id.
- Data is read through the `Source` trait with `read_at`, so every protocol works. A download continues from the last written offset after a dropped connection or a restart.
- Write to `<name>.part` and rename when complete. Verify the size and the fingerprint (specification, section 7) before marking a download as done.
- Priorities: playback reads outrank downloads; downloads share the per-source concurrency cap.
- Progress reaches the UI through the event sink, at most twice per second per download.
- Sidecar subtitles and every part of a multi-part file are downloaded with the item. A season or a whole show expands into one job per episode.
- Managed storage: a directory under the app's data directory by default, configurable, with a readable layout (`Title (Year)/original file name`).
- A numbered migration adds a table for managed copies, for example `local_copy(id, file_id, path, size, bytes_done, state, error, created_at, completed_at)` with the states queued, running, paused, done, failed and outdated.
- Check free disk space before starting and while writing. Running out of space is reported as its own error category; add it to the list in section 8 of the specification.
- Deleting a managed copy removes its files and its row. It never touches the file on the source.
- At start-up: remove orphaned `.part` files, and mark rows whose file has disappeared.
- At scan time: if the source file's size or modification time changed, mark the copy as outdated.
- Command-line harness: `download`, `downloads list`, `downloads delete`.

### Acceptance criteria

- [ ] A file downloaded from each protocol (SMB, NFS, WebDAV, FTP) is byte-identical to the original.
- [ ] The conformance suite drops the connection in the middle of a download; the download resumes and completes without starting over.
- [ ] Killing the process mid-download and restarting continues the job.
- [ ] Starting playback of another network file while a download runs does not stutter.
- [ ] A full disk produces a clear error and leaves no partial file behind.

### Out of scope

- Any user interface (see the "Save" and "Save as…" issues).
- Transcoding or shrinking files.
- Automatic downloads of new episodes.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 4. "Save": keep an offline copy, play from it, delete it

**Milestone:** 1.5 · **Labels:** feature, ui, core

### Context

"Save" keeps an offline copy of a movie or episode in storage managed by the app, so it plays without the network. Because the app manages this copy, the app can also delete it. Depends on "Core: download engine and storage for offline copies".

### What to build

- Detail page: a "Save" action for movies and episodes, "Save season" on a season and "Save all" on a show. The action is offered for items on network sources only.
- The action shows its state: Save, then Saving with a percentage and pause and cancel, then Saved.
- A saved item offers "Delete saved copy" with a confirmation. The text makes clear that only the local copy is removed and the file on the server stays.
- Playback uses the saved copy when it is complete and not outdated, even when the source is offline. Resume position and watched state stay with the item, whichever copy is played.
- A small mark on covers of saved items, and a "Saved" filter in Movies and Shows.
- A Downloads screen: running, queued, failed and completed downloads with sizes, and the total space used.
- An outdated copy (the source file changed) is marked and offers "Save again".
- Settings, under Storage: location of saved copies, space used, "Delete all saved copies".

### Acceptance criteria

- [ ] With the network unplugged, a saved movie plays from start to end, including its sidecar subtitles.
- [ ] Delete removes the local files and frees the space; the item itself stays in the library and streams again when the source is reachable.
- [ ] "Delete saved copy" appears only for copies made with "Save", never for files exported with "Save as…".
- [ ] Download progress survives leaving and re-entering the detail page and restarting the app.
- [ ] Every action is reachable with keyboard and remote, all strings use `qsTr`, and controls have `Accessible` properties.

### Out of scope

- "Save as…" (separate issue).
- Storage limits with automatic clean-up; deleting watched copies automatically.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 5. "Save as…": export a media file to a chosen location

**Milestone:** 1.5 · **Labels:** feature, ui, core

### Context

"Save as…" writes a copy of a media file to a location the user picks, for example a USB drive. The exported file belongs to the user: the app does not track it afterwards and offers no way to delete it. Depends on "Core: download engine and storage for offline copies".

### What to build

- Detail page: "Save as…" for movies and episodes opens the native file dialog with the original file name as the suggestion. On a season or show it opens a folder dialog and writes one file per episode.
- The export uses the download engine: a `.part` file in the target folder, renamed on completion, resumable while the app runs.
- Sidecar subtitles are written next to the video with the same base name.
- If the item already has a saved offline copy, export from that copy instead of the network.
- The Downloads screen shows a running export with progress and cancel. A finished export shows its target path and "Show in file manager" until dismissed. It has no delete action.
- Cancelling removes the partial file.
- Flatpak: the file dialog goes through the desktop portal, which grants access to the chosen location only. Make sure the core can write there, and do not add broad file-system permissions to the manifest.
- The file dialog needs the Qt Quick Dialogs module. Check that it is LGPL, then add it to the module allowlist in the specification and in the CMake check.

### Acceptance criteria

- [ ] An exported file is byte-identical to the source file.
- [ ] Export works to the home folder and to a removable drive, in the Flatpak build.
- [ ] After completion and a restart, the app holds no record of the exported file, and no "Delete" is offered for it.
- [ ] Overwriting an existing file asks first.
- [ ] Removing the target drive during an export produces a clear error.

### Out of scope

- Converting or renaming files by a pattern.
- Uploading to another network source.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


---

# Version 2.0

Music support, Hi-Res audio and music metadata.

## 6. Specification: add music to the scope and the data model

**Milestone:** 2.0 · **Labels:** design, needs-human

### Context

Version 2.0 adds music: common formats including FLAC, MP3 and Ogg, with attention to Hi-Res audio, and metadata from MusicBrainz, the Cover Art Archive, TheAudioDB and Fanart.tv. The specification covers video only. This issue changes `docs/SPEC.md` so that the implementation issues have a baseline. It produces no product code.

### What to decide and write down

- **Scope.** Formats in 2.0 (proposal: FLAC, ALAC, WAV, AIFF, MP3, AAC, Ogg Vorbis, Opus, WavPack). What "Hi-Res" means for badges (proposal: lossless with more than 16 bits or a sample rate above 48 kHz). Non-goals for 2.0 (proposal: playlists files, cue sheets, DSD, audiobooks, podcasts, lyrics, streaming services).
- **Data model.** `item.kind` gains artist, album and track; SQLite cannot alter a CHECK constraint, so plan the table rebuild as a migration. Music-specific fields (disc and track numbers, album artist, release date, release type, artist credits with more than one artist per track). Audio properties per file (codec, sample rate, bit depth, channels, lossless). A general table for provider ids, since `tmdb_id` and `imdb_id` columns do not scale to MusicBrainz ids. Play counts for tracks.
- **Sources.** Each library folder gets a content type, video or music, so the scanner knows which pipeline to run. Existing folders migrate as video.
- **Providers.** A provider trait for music, the order in which artwork sources are tried (proposal: embedded and sidecar art first, then the Cover Art Archive, then the optional providers), and the key policy for TheAudioDB and Fanart.tv (see the provider issue for the facts).
- **Playback.** How the play queue is split between the core (order, shuffle, repeat) and the C++ shell (feeding mpv), and what happens to music when a video starts.
- **Bundle.** Which FFmpeg demuxers and decoders the LGPL build must contain for these formats. The current recipe is "decoders only"; list what has to be added.
- **Privacy.** Music lookups send artist and album names to third parties. Propose a setting that restricts music metadata to file tags.

### Deliverable

One pull request that amends `docs/SPEC.md` (scope, sections 5, 7, 8, 9, 10 and the roadmap) and lists the open decisions for the maintainer. The other 2.0 issues start after it is merged.

### Acceptance criteria

- [ ] Every 2.0 issue can point to a section of the amended specification.
- [ ] The migration plan keeps existing video libraries and watch state intact.
- [ ] Open decisions are listed with a recommendation each.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 7. Music library: read tags and group tracks into albums and artists

**Milestone:** 2.0 · **Labels:** feature, core

### Context

The scanner understands video file names. Music is organised by embedded tags, not by file names. Depends on "Specification: add music to the scope and the data model".

### What to build

- A music pipeline in the scanner for folders with the content type music.
- Tag reading in Rust for the formats named in the specification. Candidate crate: `lofty`; check its licence against the allowlist and its maintenance state before adopting it. Tags are read through `Source::read_at` with range reads, never by downloading whole files.
- Fields: title, artists, album, album artist, track and disc numbers, date, genre, compilation flag, MusicBrainz ids, ReplayGain values, duration, codec, sample rate, bit depth, channels.
- Embedded cover art and sidecar images (`cover.jpg`, `folder.jpg`, `front.png`) go into the artwork cache in the thumbnail tiers the UI uses.
- Grouping: by MusicBrainz release id when present; otherwise by album artist, album and year within a folder. Handle multi-disc albums and compilations.
- Fallback for untagged files: folder and file name patterns such as `Artist/Album/01 - Title.flac`.
- Incremental scans by path, size and modification time, as for video.
- Command-line harness: list artists, albums and tracks.

### Tests

- A fixture set of tiny audio files generated in the test setup (silence or sine tones with tags). Do not commit copyrighted music.
- A corpus of real-world folder and file name patterns, like the video corpus.

### Acceptance criteria

- [ ] A library of 50,000 tracks on an SMB share scans without loading whole files, and a second scan with no changes finishes quickly.
- [ ] Multi-disc albums, compilations and tracks with several artists are grouped correctly in the fixtures.
- [ ] A file with broken or missing tags never aborts the scan; it appears under a readable fallback name.
- [ ] Video libraries are unaffected.

### Out of scope

- Online metadata (separate issues), cue sheets, playlist files, editing tags.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 8. Music playback: play queue, gapless playback and Now Playing

**Milestone:** 2.0 · **Labels:** feature, playback, ui, core

### Context

Music playback reuses mpv through the existing `MpvPlayer`; the project still does not write its own player. What is new is a play queue, gapless transitions and a player that keeps running while the user browses. Depends on the specification issue and on the music library issue.

### What to build

- **Formats.** Extend the LGPL libmpv and FFmpeg recipe with the demuxers and decoders the specification lists, keep the licence assertions, and add a test that plays one fixture per format.
- **Play queue in the core.** Play an album, play from a track, shuffle, repeat one and repeat all, next and previous, "play next" and "add to queue". The queue survives a restart. The C++ shell only hands mpv the current and the following entry.
- **Gapless playback** between consecutive tracks of the same format, using mpv's gapless and prefetch options.
- **ReplayGain** setting: off, track or album.
- **Now Playing screen:** cover, title, artist, album, a format line such as "FLAC · 96 kHz · 24 bit", seek bar and the queue.
- **Mini player** in the shell that stays visible while browsing, with play, pause, next and a way back to Now Playing.
- **Media keys:** MPRIS on Linux and SMTC on Windows show track, artist, album and cover.
- Starting a video pauses music and keeps the queue.
- The screensaver is not inhibited for audio-only playback.
- Network tracks stream through `coyotl://` with a smaller demuxer cache than video.
- Play counts are recorded per track.

### Tests

- Gapless: two fixture files that hold one continuous sine tone are played back to back into mpv's file output (`ao=pcm`); the test fails if the result has a gap or a discontinuity.
- Queue logic is unit-tested in the core without Qt.

### Acceptance criteria

- [ ] An album on an SMB share plays through without gaps or clicks between tracks.
- [ ] Music keeps playing while the user browses any screen.
- [ ] Media keys and the desktop's media controls work and show the right metadata.
- [ ] The queue and the position in it are restored after a restart.
- [ ] Every control is reachable with keyboard and remote.

### Out of scope

- Bit-perfect output and device selection (separate issue).
- Crossfade, equaliser, lyrics, visualisations.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 9. Hi-Res audio: output at the file's native sample rate and bit depth

**Milestone:** 2.0 · **Labels:** feature, playback, needs-human

### Context

Hi-Res files are only worth having if the app does not resample or alter them on the way out. The aim is a bit-perfect path where the system allows one, and an honest display of what is happening where it does not. Depends on "Music playback: play queue, gapless playback and Now Playing".

### What to build

- **Bit-perfect mode** (a setting, off by default): no forced sample rate or sample format, no audio filters, software volume fixed at 100 %, ReplayGain off, and exclusive access to the device where the system offers it. The device is reopened when the format changes between tracks.
- **Output device selection** in Settings, under Audio.
- **Linux.** With PipeWire the app can request a rate, but the system only switches if its allowed rates are configured and no other stream is playing. Direct ALSA hardware devices give a true exclusive path. Offer both, and document the PipeWire configuration in the user guide. Find out which Flatpak permission direct ALSA access needs, and get the maintainer's approval before changing the manifest.
- **Windows.** WASAPI exclusive mode.
- **Signal path display** on Now Playing: source format and output format, read from mpv's input and output audio parameters. Show "Bit-perfect" only when sample rate and bit depth match, no filter is active and the output is exclusive. In shared mode, show the actual output rate and say that the system mixer may resample.
- **Hi-Res badge** on albums and tracks, as defined in the specification.

### Tests

- Automated: a 24-bit, 96 kHz FLAC fixture is played in bit-perfect mode into mpv's file output, and the result is bit-identical to a reference decode. This proves the app adds no processing; it cannot prove what the hardware receives.
- Manual: a checklist in `docs/testing/hires-audio.md` for a USB DAC with a sample-rate display, covering 44.1, 48, 88.2, 96 and 192 kHz and a change of rate between two tracks.

### Acceptance criteria

- [ ] The automated bit-identity test passes in CI.
- [ ] The app never shows "Bit-perfect" in shared mode.
- [ ] Turning bit-perfect mode off restores volume control and ReplayGain.
- [ ] A device that rejects a format falls back to a supported one with a visible note, not with silence.

### Out of scope

- DSD output, upsampling, room correction, multichannel bitstream passthrough.

### For the maintainer

- Run the manual checklist on real hardware; CI has no audio device.
- Decide on the Flatpak permission for direct ALSA access.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 10. Music UI: Artists, Albums, tracks and search

**Milestone:** 2.0 · **Labels:** feature, ui

### Context

The shell has Home, Movies, Shows, Search, Sources and Settings. Music needs its own section. Depends on the music library issue; playback comes from the music playback issue.

### What to build

- Sidebar: a Music section with Albums, Artists and Recently added. Home gains rows for recently added albums and recently played music.
- Albums: a grid of square covers that follows the cover-size settings introduced in 1.1.
- Album page: cover, artist, year, total duration, format and Hi-Res badge, the track list grouped by disc, and the actions Play, Shuffle and, from 1.5, Save for offline use.
- Artist page: albums grouped by type (album, EP, single, compilation) and sorted by year, with the artist's image and text when a provider supplied them.
- Search covers artists, albums and tracks, with results grouped by kind next to movies and shows.
- Adding a library folder in Sources asks for its content type: video or music.
- Long lists are virtualised through paged C++ models, as for video. QML holds no business logic.

### Acceptance criteria

- [ ] A library of 50,000 tracks stays within the scroll budget in every music list and grid.
- [ ] Every screen works with keyboard and remote, in the windowed and the fullscreen layout.
- [ ] The whole music section works offline from the local database.
- [ ] All strings use `qsTr`, and the German translation is complete.
- [ ] A user without any music folder sees no empty music screens; the section explains how to add one.

### Out of scope

- Playlists, genres as a browsing axis, editing metadata, choosing alternative artwork.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 11. Music metadata: MusicBrainz and Cover Art Archive

**Milestone:** 2.0 · **Labels:** feature, metadata, core

### Context

MusicBrainz identifies releases, artists and recordings; the Cover Art Archive serves cover images keyed by MusicBrainz ids. Neither needs an API key. Depends on the specification issue and the music library issue.

### Facts checked on 2026-11-11 (check again when implementing)

- MusicBrainz allows about one request per second per IP address on average and answers with HTTP 503 above that.
- Every request needs a User-Agent that names the application, its version and a contact URL or address, for example `Huehuecoyotl/2.0.0 ( https://github.com/<owner>/<repo> )`. Anonymous user agents are throttled.
- Source: https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting

### What to build

- A MusicBrainz provider behind the music provider trait, using the JSON API.
- One limiter for the whole app that keeps to one request per second, with backoff on 503. The limiter state is shared by all jobs.
- Matching: look up by MusicBrainz id when the tags carry one, with a single request that includes the related data needed. Otherwise search by artist, album and track count. A low-confidence match goes to "needs review" and never overwrites tags silently. Items the user corrected are locked.
- Cover Art Archive: front cover by release id, falling back to the release group, fetched in a size that fits the largest thumbnail tier. Embedded and sidecar art take precedence, as the specification defines.
- Jobs are persisted, resumable and prioritised so that albums on screen come first. The library is usable from tags alone while lookups run; at one request per second a large library takes a long time.
- A setting that turns online music metadata off.
- About screen: attribution for MusicBrainz and the Cover Art Archive according to their current terms. Check the data licences and record them in the third-party notices.

### Tests

- Recorded fixtures for unit tests. Tests against the live service sit behind a Cargo feature, respect the rate limit and do not run in CI.
- A test proves that the limiter never exceeds one request per second under concurrent jobs.

### Acceptance criteria

- [ ] An album tagged with MusicBrainz ids is matched with one metadata request.
- [ ] An untagged but well-named album is matched or lands in "needs review"; it is never matched to a wrong release with high confidence in the fixtures.
- [ ] The User-Agent is sent on every request and contains the real version.
- [ ] With online metadata turned off, no request leaves the machine.

### Out of scope

- Acoustic fingerprinting, submitting data to MusicBrainz, writing tags back to files.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 12. Music metadata: TheAudioDB and Fanart.tv as optional providers

**Milestone:** 2.0 · **Labels:** feature, metadata, core, needs-human

### Context

TheAudioDB and Fanart.tv add what MusicBrainz and the Cover Art Archive lack: artist photos, biographies, logos, backgrounds, and covers for albums without one. Both are looked up by MusicBrainz id, so this depends on "Music metadata: MusicBrainz and Cover Art Archive".

### Facts checked on 2026-11-11 (check again when implementing)

- **Fanart.tv** requires a project API key for every application. A user can add a personal key, which gives access to newer images. Source: https://medium.com/fanart-tv/what-are-fanart-tv-personal-api-keys-472f60222856
- **TheAudioDB** requires a key. A third-party integration guide reports a shared free key, `123`, limited to about 30 requests per minute, with higher limits for paying supporters. Confirm this on TheAudioDB's own site before relying on it. Source: https://whatsnowplaying.github.io/whats-now-playing/dev/extras/theaudiodb/

### Decision needed from the maintainer

The specification says the project ships no API keys, because an open-source client cannot keep one secret. Fanart.tv's model assumes a key per application.

- **Option A (recommended, consistent with TMDB):** both providers are off until the user enters their own key in Settings, under Metadata.
- **Option B:** ask each service for a project key and inject it into official builds from a CI secret. The key is never in the repository, and builds by others run without these providers. This needs the services' consent.

Implement option A unless the maintainer decides otherwise in a comment.

### What to build

- Two providers behind the music provider trait, each with its own rate limiter.
- Keys live in the OS keystore, are validated with a test request when entered, and are redacted from all logs. Fixtures contain no keys.
- Artwork roles: artist image, artist background, artist logo, album cover (only when no other source has one), disc art.
- Text: artist biography in the user's language with an English fallback.
- Both providers are optional. With neither enabled, the music section works and looks complete.
- Attribution on the About screen according to each service's terms.

### Acceptance criteria

- [ ] Without keys, no request is sent to either service.
- [ ] An invalid key is reported when entered, not later during a scan.
- [ ] A log search after a scan finds no key in any file.
- [ ] Artist pages show the image and biography when a provider has them, and a clean fallback when it does not.

### For the maintainer

- Choose option A or B.
- Read both services' terms for use in a free, open-source, non-commercial application.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


---

# Version 3.0

macOS release.

## 13. macOS: LGPL build of libmpv and FFmpeg as a universal bundle

**Milestone:** 3.0 · **Labels:** build, platform-macos

### Context

Version 3.0 releases the app for macOS (the specification's tier 2; "Mac OS X" is called macOS today). Official builds must ship the project's own LGPL build of libmpv, as on Linux and Windows.

### What to build

- A recipe in `third_party/mpv` that builds libmpv, FFmpeg and their bundled libraries for `arm64` and `x86_64` and joins them into universal dynamic libraries.
- The same pinned versions as the other platforms, recorded by commit.
- The same assertions: mpv built with `-Dgpl=false`, FFmpeg without GPL or non-free options.
- Hardware decoding through VideoToolbox and audio output through CoreAudio are enabled.
- Install names use `@rpath`, so the libraries can sit in the app bundle and can be replaced by the user (an LGPL obligation).
- Find out which of mpv's macOS-specific parts are GPL-only, as was found for X11 in mpv 0.42, and record the result in section 3 of the specification.
- A CI job on macOS runners builds the bundle and uploads it as an artifact.
- The deployment target follows the Qt version in use (Qt 6.12 targets macOS 14.4 or newer; check when implementing).

### Acceptance criteria

- [ ] `lipo -archs` reports both architectures for every bundled library.
- [ ] `otool -L` shows only `@rpath` and system paths.
- [ ] The licence assertions fail the build when a GPL option is switched on.
- [ ] A small test program opens a file with the bundled libmpv and decodes HEVC with VideoToolbox on a macOS runner.

### Out of scope

- The app itself, signing and notarisation (separate issues).
- Using Homebrew's mpv or FFmpeg in a release; those are GPL builds and are for local development only.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 14. macOS: build and run the app, and validate the video path

**Milestone:** 3.0 · **Labels:** feature, playback, platform-macos, needs-human

### Context

The app has never run on macOS. The largest unknown is video: embedded mode needs Qt Quick on OpenGL, Qt defaults to Metal on macOS, and Apple has deprecated OpenGL and froze it at version 4.1. Depends on "macOS: LGPL build of libmpv and FFmpeg as a universal bundle".

### What to do

- Make the CMake and Cargo build produce a runnable app for `arm64` and `x86_64`.
- Embedded video: request an OpenGL 4.1 core profile before the application object is created, force Qt Quick to OpenGL, and run the existing video item against mpv's render API with VideoToolbox decoding.
- Measure a 4K HEVC file on Apple Silicon and, if available, on an Intel Mac: dropped frames, CPU load, and whether decoding stays in hardware.
- Write `docs/spike/macos-video.md` with the measurements. If the OpenGL path misses the playback budget, stop and report; do not start a Metal renderer on your own.
- HDR: embedded mode tone-maps. Record HDR on macOS as an open item in section 5 of the specification.
- Platform conventions: data and cache folders under `~/Library`, native fullscreen, Retina scale factors for thumbnails, cursor auto-hide.
- File names: macOS and some servers use decomposed Unicode. Compare paths in a normalisation-insensitive way so that a library scanned on Linux matches on macOS.
- Secrets: the keystore abstraction uses the macOS Keychain; cover it with a test.
- CI: build and unit tests on macOS runners for both architectures.

### Acceptance criteria

- [ ] The app starts, scans a local folder and plays a file on macOS.
- [ ] The 4K HEVC measurement is documented for at least one Apple Silicon Mac.
- [ ] A file whose name contains umlauts or accents is found on an SMB share and keeps its watch state across scans.
- [ ] No Linux or Windows behaviour changes; CI stays green on all platforms.

### For the maintainer

- Run the playback measurements on real Macs. CI runners cannot judge smoothness.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 15. macOS: system integration

**Milestone:** 3.0 · **Labels:** feature, ui, platform-macos

### Context

A Mac app is expected to behave like one. This issue covers the operating-system integration that Linux gets through MPRIS and portals. Objective-C++ files in `app/src` count as part of the C++ shell under the language rule: they wrap Apple APIs and hold no logic. Depends on "macOS: build and run the app, and validate the video path".

### What to build

- **Media keys and Now Playing:** the system's Now Playing widget and the keyboard's media keys control playback and show title and artwork.
- **Sleep:** the display stays awake during video playback and may sleep during audio-only playback.
- **Local network access:** recent macOS versions ask the user before an app may reach devices on the local network. Add the usage description and the service types that discovery browses for to `Info.plist`. If the user declines, sources show a clear explanation instead of a generic connection error.
- **Menu bar:** the standard application menu with About, Settings and Quit, and the usual shortcuts.
- **Files:** opening a media file from Finder or by dropping it on the Dock icon plays it; register the file types.
- **App icon:** an `.icns` file produced by the icon script from the logo master.
- **Audio:** output device selection and, for the Hi-Res feature of 2.0, exclusive output with sample-rate switching through CoreAudio.

### Acceptance criteria

- [ ] Play, pause, next and previous work from the keyboard and from the Now Playing widget.
- [ ] Discovery finds an SMB server after the user grants local network access, and the app explains what to do when access was declined.
- [ ] Double-clicking a video file in Finder opens it in the app.
- [ ] The Hi-Res checklist from 2.0 passes on macOS for at least 44.1, 96 and 192 kHz.

### Out of scope

- Touch Bar, widgets, Shortcuts actions, AirPlay.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 16. macOS: signed and notarised universal release

**Milestone:** 3.0 · **Labels:** build, packaging, platform-macos, needs-human

### Context

macOS blocks apps that are not signed and notarised by Apple. Releases are built only by CI from a signed tag (specification, section 13). Depends on the other macOS issues.

### What to build

- An app bundle assembled with `macdeployqt`: the Qt frameworks and the libmpv libraries inside the bundle as dynamic libraries, never statically linked.
- Code signing with the hardened runtime and the smallest set of entitlements that works.
- Notarisation and stapling, then a disk image as the download.
- A release job on macOS runners that runs from a signed tag. Signing identities and notarisation credentials live in CI secrets only.
- Third-party notices and the software bill of materials include the macOS bundle.
- The user guide gets an installation section for macOS.
- The specification moves macOS to tier 1 and records the minimum macOS version.

### Decision to raise

The LGPL requires that users can replace Qt, libmpv and FFmpeg with their own builds. The hardened runtime's library validation refuses libraries signed by someone else. Describe the options (an entitlement that relaxes library validation, or documented steps for re-signing the app after replacing a library), recommend one, and wait for the maintainer's choice.

### Acceptance criteria

- [ ] On a Mac that has never seen the app, the downloaded disk image opens and the app starts without a security warning.
- [ ] `spctl --assess` accepts the app and `stapler validate` succeeds.
- [ ] The app runs on Apple Silicon and Intel from the same download.
- [ ] The documented way of replacing a bundled LGPL library works.

### Out of scope

- The Mac App Store.

### For the maintainer

- Join the Apple Developer Program (a yearly fee), create a Developer ID certificate and add the credentials to the CI secrets. None of this can be done from an issue.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


---

# Version 4.0

iOS and tvOS.

## 17. Decide the architecture for iOS and tvOS

**Milestone:** 4.0 · **Labels:** design, platform-apple-mobile, needs-human

### Context

Version 4 is planned for iOS and tvOS. This reverses a decision in the specification, which lists phones, tablets and TV boxes as non-goals and chose Qt because the project was desktop-only. The architecture has to be decided before any app code is written. This issue produces a report, two small prototypes and an amendment to the specification.

### Facts checked on 2026-11-11 (check again when starting)

- Qt 6.12 lists iOS 18 or newer as a supported target and does not list tvOS. The Qt Quick UI cannot run on Apple TV. Source: https://doc.qt.io/qt-6/supported-platforms.html
- Qt's open-source edition is LGPLv3. Whether an App Store release can meet the LGPL's requirement that users can replace the library is disputed, and Qt's own old wiki page on the subject leaves it open. This needs legal advice, not an engineering opinion.
- libmpv has been packaged for iOS and tvOS by third parties, for example MPVKit, whose default build is LGPL. Its documentation calls Metal support a patch that is not officially supported by mpv. Source: https://github.com/mpvkit/MPVKit
- mpv's render API, which embedded mode uses, is OpenGL-only, and Apple has deprecated OpenGL ES on iOS and tvOS.

### Questions to answer

1. **UI technology.** Proposal to test: one native SwiftUI codebase for iOS, iPadOS and tvOS on top of the unchanged Rust core. The headless-core principle was written for this case. Compare it with Qt Quick on iOS plus a separate tvOS UI.
2. **Language rule.** Extend it: Swift only for UI and Apple APIs, no business logic, as for C++ today.
3. **Bindings.** How Swift calls the core: generated bindings (UniFFI is MPL-2.0, which the allowlist permits) or a C interface. The contract rules of section 8 stay.
4. **Video path.** How libmpv draws on iOS and tvOS, whether HDR and Dolby Vision output are reachable, and whether the display can switch to the content's frame rate and dynamic range on Apple TV.
5. **Licensing.** LGPL libraries on the App Store as dynamic frameworks; the App Store's terms; codec patents. List what needs a lawyer.
6. **Core portability.** Which crates build for the iOS and tvOS targets; the support tier of Rust's tvOS targets; NFS servers that demand privileged ports; discovery, which on Apple's mobile systems has to go through the system's Bonjour API and needs the local network permission.
7. **Storage on tvOS.** Apple TV gives apps little or no guaranteed persistent storage. Decide where the library database lives, whether it must be rebuildable at any time, and how watch state survives.
8. **TMDB key on Apple TV.** Typing a key with a remote is unreasonable. Propose a hand-off, for example from the iOS app or a QR code.
9. **Feature mapping.** Which features carry over: offline copies (1.5), music with background playback (2.0), Hi-Res output.
10. **Translations.** One source for strings across Qt and Swift.

### Deliverables

- `docs/spike/apple-mobile.md` with an answer and a recommendation for each question.
- Prototype 1: the Rust core linked into an empty SwiftUI app that lists a library, running in the iOS and the tvOS simulator.
- Prototype 2: libmpv playing a file in a SwiftUI view on a real iPhone or Apple TV.
- A pull request that amends the specification: scope, non-goals, architecture, licensing and roadmap.

### Acceptance criteria

- [ ] Every question above has a written answer.
- [ ] Both prototypes build from the repository with documented steps.
- [ ] The maintainer has approved the amended specification. The other 4.0 issues stay blocked until then.

### For the maintainer

- An Apple Developer Program membership, a Mac with Xcode, an iPhone and an Apple TV are needed for the device tests.
- Take legal advice on the licensing questions before committing to an App Store release.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 18. Core for iOS and tvOS: build targets, Swift bindings and libmpv frameworks

**Milestone:** 4.0 · **Labels:** build, core, platform-apple-mobile, blocked

### Context

Blocked by "Decide the architecture for iOS and tvOS". The text below assumes that issue confirms a native Swift UI on the Rust core; adjust it to the approved specification before starting.

### What to build

- The Rust workspace builds for the iOS and tvOS device and simulator targets and is packaged as an XCFramework by CI.
- Swift bindings for the core API: requests with ids going in, one event stream coming out and delivered on the main actor. The contract rules of section 8 apply unchanged: nothing blocks, events carry ids, no panic crosses the boundary.
- The mpv stream callbacks for `coyotl://` are exported as on the desktop.
- Protocol backends that cannot work on a platform are switched off by Cargo features, and the UI can ask the core which protocols are available.
- Secrets use the Keychain. Data and cache locations follow the storage decision for each platform.
- libmpv, FFmpeg and their libraries are built from the project's LGPL recipe as dynamic frameworks for both platforms, with the same licence assertions and pinned versions.
- CI builds everything on macOS runners and runs a smoke test in the simulators.

### Acceptance criteria

- [ ] A sample Swift package can browse a test library and receive change events in the iOS and tvOS simulators.
- [ ] The desktop builds are unchanged and CI stays green on every platform.
- [ ] The licence checks cover the new targets.
- [ ] No LGPL library is statically linked into an app binary.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 19. iOS and iPadOS app

**Milestone:** 4.0 · **Labels:** feature, ui, platform-apple-mobile, blocked, needs-human

### Context

Blocked by "Decide the architecture for iOS and tvOS" and by "Core for iOS and tvOS: build targets, Swift bindings and libmpv frameworks". Adjust the scope to the approved specification before starting.

### What to build

- Library: Home, Movies, Shows, Music, Search, with layouts for iPhone and iPad in both orientations.
- Detail pages and playback with subtitles, audio tracks, resume and watched state, using touch gestures for seeking and volume.
- Sources: adding SMB, NFS, WebDAV, FTP and SFTP sources as far as the core supports them on iOS, with the local network permission explained before the system asks.
- Settings, including the TMDB key with validation.
- Offline copies from 1.5: "Save" into app storage with delete, and "Save as…" through the system's file export.
- Music from 2.0: background playback, lock-screen controls and the Now Playing information.
- Accessibility: VoiceOver labels and Dynamic Type.
- Translations from the shared string source.
- A TestFlight build produced by CI from a signed tag.

### Acceptance criteria

- [ ] A user can add an SMB share, wait for the scan and play a movie on an iPhone and an iPad.
- [ ] Music keeps playing with the screen locked and reacts to the lock-screen controls.
- [ ] A saved movie plays in flight mode.
- [ ] The app passes Apple's review for TestFlight.

### Out of scope

- Picture in picture, AirPlay, CarPlay, widgets.

### For the maintainer

- App Store Connect setup, screenshots, the privacy declaration and the review submission.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.


## 20. tvOS app

**Milestone:** 4.0 · **Labels:** feature, ui, platform-apple-mobile, blocked, needs-human

### Context

Blocked by "Decide the architecture for iOS and tvOS" and by "Core for iOS and tvOS: build targets, Swift bindings and libmpv frameworks". The Apple TV is the device where an app in the spirit of Infuse is used most, so playback quality matters most here. Adjust the scope to the approved specification before starting.

### What to build

- A ten-foot UI driven by the focus engine: Home, Movies, Shows, Music, Search and Settings.
- Playback with the Siri Remote: play and pause, swipe to seek, and a panel for subtitles and audio tracks.
- Display matching: switch the television to the content's frame rate and dynamic range when the user allows it, as far as the video path decision makes possible.
- Top Shelf: continue watching and recently added.
- Storage as decided in the architecture issue: the app must survive the system purging its data, by rebuilding the library and restoring sources and watch state.
- Setup hand-off for the TMDB key and source credentials from the iOS app or a QR code.
- Music playback with the system's Now Playing.
- One purchase entry shared with the iOS app (the app is free).

### Acceptance criteria

- [ ] Every screen is fully usable with the Siri Remote alone.
- [ ] A 4K HDR file plays without dropped frames on a current Apple TV, and the result of the display-matching test is documented.
- [ ] After the app's data is purged, sources and watch state come back without the user entering anything again.
- [ ] The app passes Apple's review for TestFlight.

### Out of scope

- "Save" and "Save as…": Apple TV has no user-accessible file storage.
- Bit-perfect Hi-Res output, unless the architecture report shows that tvOS allows it.

### For the maintainer

- Tests on a real Apple TV with an HDR television, and the App Store submission.

### Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.

