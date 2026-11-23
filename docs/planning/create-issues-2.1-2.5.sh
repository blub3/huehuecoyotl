#!/usr/bin/env bash
# Creates the second batch of Huehuecoyotl issues (and any missing labels and milestones).
#
# Usage:  run inside a clone of the repository, with the GitHub CLI signed in:
#           bash create-issues-2.1-2.5.sh            create everything
#           DRY_RUN=1 bash create-issues-2.1-2.5.sh  only print what would be created
#         or name the repository:  REPO=owner/name bash create-issues-2.1-2.5.sh
#
# Safe to run twice: an issue whose exact title already exists is skipped.
set -euo pipefail

command -v gh >/dev/null || { echo "The GitHub CLI (gh) is not installed." >&2; exit 1; }
REPO="${REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
DRY_RUN="${DRY_RUN:-0}"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
echo "Repository: $REPO"

label() {
  if [ "$DRY_RUN" = 1 ]; then echo "label      $1"; return; fi
  gh label create "$1" --repo "$REPO" --color "$2" --description "$3" --force >/dev/null
}

milestone() {
  if [ "$DRY_RUN" = 1 ]; then echo "milestone  $1"; return; fi
  if gh api "repos/$REPO/milestones?state=all&per_page=100" -q '.[].title' | grep -Fxq "$1"; then return; fi
  gh api "repos/$REPO/milestones" -f title="$1" -f description="$2" >/dev/null
}

issue() {  # title, milestone, comma-separated labels, body file
  if [ "$DRY_RUN" = 1 ]; then echo "issue      [$2] $1"; return; fi
  if gh issue list --repo "$REPO" --state all --limit 1000 --json title -q '.[].title' | grep -Fxq "$1"; then
    echo "exists     $1"; return
  fi
  gh issue create --repo "$REPO" --title "$1" --milestone "$2" --label "$3" --body-file "$4"
}

label 'bug' d73a4a 'Something does not work as specified'
label 'feature' 1f883d 'New user-facing capability'
label 'design' 8250df 'Specification or architecture work, no product code'
label 'build' 6e7781 'Build system, CI, third-party bundles'
label 'packaging' 6e7781 'Installers, signing, store packages'
label 'ui' 0969da 'QML or native UI'
label 'core' bf8700 'Rust core'
label 'playback' cf222e 'mpv, video and audio output'
label 'metadata' bf3989 'Metadata providers and artwork'
label 'platform-macos' 57606a 'macOS'
label 'platform-apple-mobile' 57606a 'iOS, iPadOS and tvOS'
label 'needs-human' d4a72c 'Needs a decision, an asset, hardware or an account from the maintainer'
label 'blocked' 24292f 'Must not start before another issue is done'

milestone '2.1' 'Music: album and track versions'
milestone '2.5' 'Audiobooks'

cat > "$TMP/01.md" <<'__ISSUE_BODY_EOF__'
## Context

A music library often holds the same album more than once: for example a FLAC 24-bit/96 kHz download and an MP3 copy, the album on two shares, or a remaster next to the original release. Single tracks can exist twice as well. Movies and episodes already offer a version dropdown. Albums and tracks should get the same, so the user picks which copy to play.

## Dependencies

These must be done first:

- Music library: read tags and group tracks into albums and artists (2.0)
- Music playback: play queue, gapless playback and Now Playing (2.0)
- Music UI: Artists, Albums, tracks and search (2.0)
- Hi-Res audio: output at the file's native sample rate and bit depth (2.0), for the format labels and badges
- Movie page: choose between several versions of the same movie (1.2), whose shared dropdown component, preselection rules and "Preferred version" setting are reused

## What counts as a version

| Case | Example | Treatment |
| --- | --- | --- |
| Same release, different files | The same MusicBrainz release, or the same album artist, title, year and track list, as FLAC and as MP3, or on two sources | Versions of one album: one entry in the grid and a dropdown on the album page |
| Different releases of the same release group | A 2011 remaster next to the original, or a deluxe edition with bonus tracks | Editions of one album: also in the dropdown, labelled with the edition; the track list follows the selection |
| Same recording on a different album | A song on its album and again on a compilation | Not a version. Out of scope (see below). |

The 2.0 grouping rule ("by album artist, album and year within a folder") keeps copies in different folders or on different sources apart. Change it so the cases in the first two rows become one album with versions, across folders and sources. Add a numbered migration that merges existing duplicates and keeps play counts, ratings and the queue.

## What to build

**Album page**

- A version dropdown when the album has two or more versions.
- Labels such as "FLAC · 24-bit/96 kHz · 1.1 GB · NAS", "MP3 320 kbit/s · 140 MB · Laptop" or "2011 Remaster · FLAC · 16-bit/44.1 kHz".
- Versions on an unreachable source are marked offline and cannot be played. A saved offline copy (1.5) counts as a reachable version.
- The selection drives Play, Shuffle, the track list with its durations and format line, the Hi-Res badge, and "Save" and "Save as…".

**Track level**

- When one track exists in more than one version within the selected album version (for example a file that was replaced), the track's context menu offers "Version" with the same labels.
- The track list marks such tracks.

**Preselection**

1. The version last played for this album.
2. Otherwise the "Preferred version" setting, extended for music with "Highest quality" (lossless first, then bit depth, then sample rate), "Smallest file" and "Lossless only".

**Playback**

- The queue stores the chosen file for every entry, so a restored queue plays the same files.
- Playing an album uses one version for all its tracks, which keeps gapless playback consistent.
- If a track is missing in the selected version, the player falls back to another version of that track and marks it in the queue.
- Play counts belong to the track, not to the file.

**Album grid and artist page:** one entry per album. A small mark shows that more than one version exists.

**Wrong merges:** a "This is a different album" action splits a version into its own album. The split is locked, so a rescan does not merge it again.

**Input:** keyboard and remote reach every control, and all strings use `qsTr`.

## Tests

- Fixtures generated in the test setup (sine tones with tags; no copyrighted music):
  - one album as FLAC 24/96 and as MP3, in two folders;
  - the same album on a second source;
  - a remaster with one extra track;
  - an album with one replaced track.
- Grouping, preselection and queue restore are unit-tested in the core without Qt.

## Acceptance criteria

- [ ] The fixture album in two formats and on two sources appears once in the grid, with three versions in the dropdown.
- [ ] The remaster appears as an edition, and its extra track shows only when it is selected.
- [ ] Switching the version changes playback, the track list and the badges for that album only.
- [ ] A restored queue plays exactly the files that were queued.
- [ ] An album whose versions are all offline cannot be started and says why.
- [ ] Existing libraries are merged by the migration without losing play counts.

## Out of scope

- Grouping the same recording across different albums ("also appears on").
- Choosing a version for a whole artist at once.
- Deleting lower-quality duplicates.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Music: choose between several versions of an album or a track' '2.1' 'feature,ui,core,playback' "$TMP/01.md"

cat > "$TMP/02.md" <<'__ISSUE_BODY_EOF__'
## Context

Version 2.5 adds audiobooks as a category of its own, next to Movies, Shows and Music, with metadata and covers from an online service. The music specification for 2.0 lists audiobooks as a non-goal. This issue amends `docs/SPEC.md` so the other 2.5 issues have a baseline. It produces no product code.

## What to decide and write down

**Scope**

- Formats (proposal): M4B and M4A with chapters, MP3, Ogg Vorbis, Opus and FLAC.
- Both single-file books and books split into many files.
- Non-goals (proposal): DRM-protected files such as Audible's AAX and AAXC, ebooks, podcasts, audio dramas as a separate category, and syncing progress with other services.

**Data model**

- New item kinds for an audiobook, its author and its series. Plan the table rebuild for the `CHECK` constraint as a migration, as for music.
- Fields per book:
  - authors and narrators (several each);
  - series and position in it (decimal, for "2.5");
  - publisher, publication year, language;
  - ISBN and ASIN;
  - description;
  - total duration.
- A chapter table: book, index, title, start and end on the book's timeline, file.
- Progress per book on one continuous timeline across all its files, plus a finished flag and the date last listened.

**Sources**

- The content type of a library folder (introduced for music) gains "audiobooks". The scanner runs the audiobook pipeline for those folders only.

**Metadata**

- A provider trait for books.
- The priority of local sources over online data.
- The providers and their key policy (see "Audiobook metadata and covers: Open Library and MusicBrainz").

**Playback**

- How audiobook playback shares mpv, the mini player and the media keys with music.
- What happens to a running music queue when a book starts.

**Privacy**

- Lookups send book titles and author names to third parties. Extend the existing "online metadata off" setting to books.

## Deliverable

One pull request that amends the scope, sections 5, 7, 9 and 10, and the roadmap, and lists the open decisions with a recommendation each. The other 2.5 issues start after it is merged.

## Acceptance criteria

- [ ] Every other 2.5 issue can point to a section of the amended specification.
- [ ] The migration plan keeps video and music libraries, watch state and play counts intact.
- [ ] Open decisions are listed with a recommendation each.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Specification: add audiobooks to the scope and the data model' '2.5' 'design,needs-human' "$TMP/02.md"

cat > "$TMP/03.md" <<'__ISSUE_BODY_EOF__'
## Context

Audiobooks are organised differently from music: one book can be one file with embedded chapters or dozens of files, and the author, narrator and series matter more than tags like album or track number. Depends on "Specification: add audiobooks to the scope and the data model".

## What to build

**An audiobook pipeline in the scanner**, for folders whose content type is audiobooks.

**Recognising a book.** One book is one of these:

- a single file such as an M4B;
- a folder of audio files;
- a folder with disc subfolders (`CD1`, `Disc 2` and similar).

Common layouts:

- `Author/Title/`
- `Author/Series/01 - Title/`
- `Author/Series/Book 2.5 - Title/`
- `Title - Author.m4b`, flat in a folder

**Local metadata**, from highest to lowest priority:

1. A Calibre `metadata.opf` file.
2. `desc.txt` and `reader.txt` (description and narrator).
3. A `.nfo` file.
4. Tags of the first audio file. Common conventions: album = title, artist or album artist = author, composer = narrator, plus series tags where present.
5. Folder and file names.

**Chapters**

- Take embedded chapters from the probe data (FFmpeg, from 1.2) when present.
- Otherwise each file becomes a chapter, named from its title tag or file name.
- Durations come from the probe as well, never from a full read.

**Covers.** Embedded art and sidecar images (`cover.jpg`, `folder.jpg`) go into the artwork cache in the thumbnail tiers the UI uses. Audiobook covers are often square and book covers portrait; store the aspect ratio.

**Grouping and series**

- Group books by author and normalised title.
- Several books of a series form a series item, ordered by position.

**Rescans.** Incremental by path, size and modification time, as for video and music.

**Command-line harness.** List authors, series and books, and print a book's chapters.

## Tests

- Fixtures generated in the test setup: short silent or sine-tone files with tags.
  - an M4B with embedded chapters;
  - a 12-file MP3 book;
  - a two-disc book;
  - a series of three books with an OPF file;
  - an untagged book in an `Author/Title` folder.
- For manual tests, LibriVox recordings are freely usable (they are in the public domain in the USA, and LibriVox releases its recordings into the public domain). Do not commit audio files to the repository.

## Acceptance criteria

- [ ] Each fixture layout produces exactly one book with the correct author, narrator, series position and chapters.
- [ ] A 40-file book on an SMB share scans without reading whole files.
- [ ] A book with broken tags never aborts the scan and appears under a readable fallback name.
- [ ] Music folders and video folders are unaffected.

## Out of scope

- Online metadata (separate issue).
- Splitting a single long file into chapters by silence detection.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Audiobook library: detect books, chapters and series' '2.5' 'feature,core' "$TMP/03.md"

cat > "$TMP/04.md" <<'__ISSUE_BODY_EOF__'
## Context

Audiobooks get their own category in the sidebar, next to Movies, Shows and Music, and a player suited to listening for hours: exact resume, chapters, speed and a sleep timer. Playback still runs through mpv; the project does not write its own player.

## Dependencies

- "Audiobook library: detect books, chapters and series" (this milestone)
- Music playback: play queue, gapless playback and Now Playing (2.0), for the mini player, media keys and gapless playback across files

## What to build

**Category**

- Sidebar: "Audiobooks" with Books, Authors, Series and Narrators.
- Home gains a "Continue listening" row.
- Book grid that handles square and portrait covers in one grid. Covers keep their own aspect and the tiles align on a common height.
- Filters: unread, in progress, finished.

**Book page**

- Cover, title, authors, narrators, series with position (linking to the series page), duration, time left, description.
- The chapter list with the current chapter marked.
- Play or Resume, Mark as finished, and Save and Save as… (1.5).

**Series page** lists the books in order with their progress. "Continue series" starts the next unfinished book.

**Player**

- **Resume:** exact position per book on its whole timeline, across files. Saved every few seconds, on pause and on exit.
- **Chapters:** previous and next chapter, a chapter list, and the chapter title and time left in the chapter on the player.
- **Skip back and forward:** default 15 seconds back and 30 forward, configurable.
- **Speed:** 0.5× to 3.0× in 0.1 steps, without changing the pitch (mpv's `scaletempo2`). Remembered per book, with a default in Settings.
- **Sleep timer:** 5 to 120 minutes, or "end of chapter". The audio fades out over a few seconds.
- **Files:** a multi-file book plays as one continuous timeline, without gaps between files.

**Shared parts**

- The mini player and media keys show book, chapter and author.
- Starting a book pauses a running music queue and keeps it. Starting music pauses the book and saves its position.
- The screensaver is not inhibited, as for music.

**Input:** keyboard and remote on every screen, and all strings use `qsTr`, including the German translation.

## Tests

- Progress logic (timeline across files, chapter lookup, sleep timer, "continue series") is unit-tested in the core without Qt.
- A playback test with a two-file fixture book checks that the position is restored exactly after a restart, including inside the second file.

## Acceptance criteria

- [ ] After a restart, every book resumes within one second of where it stopped, also in multi-file books.
- [ ] Speed changes keep the pitch, and the chosen speed is remembered per book.
- [ ] The sleep timer stops playback at the end of the chapter or after the chosen time, and the position is saved.
- [ ] Switching between music and an audiobook keeps both positions.
- [ ] The whole category works offline from the local database and stays within the scroll budget for 2,000 books.

## Out of scope

- Bookmarks and notes.
- Syncing progress with other devices or services.
- Text-to-speech or ebook reading.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Audiobooks category and player: progress, chapters, speed and sleep timer' '2.5' 'feature,ui,playback,core' "$TMP/04.md"

cat > "$TMP/05.md" <<'__ISSUE_BODY_EOF__'
## Context

Local metadata comes first (see "Audiobook library: detect books, chapters and series"). An online service fills gaps: description, publication year, series, subjects and a cover for books that have none. Depends on "Specification: add audiobooks to the scope and the data model".

## Facts checked on 2026-11-16 (check again when implementing)

**Open Library**

- Free, without an API key.
- Unidentified requests are limited to 1 per second. Requests that send a User-Agent with the application's name and a contact address get 3 per second.
- Open Library asks not to use the API for bulk downloads, and points to its monthly data dumps for that.
- Covers come in sizes S, M and L.
- Cover lookups by ISBN and other identifiers are limited to 100 requests per IP every 5 minutes (HTTP 403 above that). Lookups by cover id or Open Library id are not limited in that way.
- Covers must not be crawled.
- The developer pages do not state a licence for the data; read the terms of service before shipping.
- Sources: https://openlibrary.org/developers/api and https://openlibrary.org/dev/docs/api/covers

**MusicBrainz**

- Has an "Audiobook" secondary release-group type.
- The 2.0 MusicBrainz and Cover Art Archive provider already exists.
- Audiobooks tagged with MusicBrainz ids (for example by Picard) can be matched there exactly.

**Audible**

- Many audiobook tools use Audible data for narrators, series and square covers.
- I found no official public metadata API from Audible. Community services that return Audible data exist, for example Audnexus.
- Their status under Audible's terms is unclear, so they are not part of this issue.

## What to build

**Open Library provider** behind the book provider trait:

- Search by title and author; prefer a match by ISBN when the local metadata has one.
- A low-confidence match goes to the "needs review" list.
- Fill only fields that the local metadata left empty. Fields the user corrected stay locked.
- Fetch covers by cover id or Open Library id where possible, size L, downscaled to the thumbnail tiers. Avoid ISBN-based cover lookups.
- A client-side rate limiter that keeps below the identified limit, with backoff on 403 and 429.
- A User-Agent with the application name, version and contact, for example `Huehuecoyotl/2.5.0 (https://github.com/<owner>/<repo>)`.
- Requests are made per book on demand by the job queue and cached permanently. No bulk fetching.

**MusicBrainz for audiobooks**

- Reuse the existing provider when a book's tags carry MusicBrainz ids, including the Cover Art Archive for the cover.

**Settings**

- The existing "online metadata off" switch covers books.
- A per-book "Search again" action that lets the user pick from the candidates.

**About screen:** attribution for Open Library according to its terms.

## Tests

- Recorded fixtures without any personal data. Tests against the live service sit behind a Cargo feature and do not run in CI.
- A test proves that the limiter keeps to the configured rate under concurrent jobs.
- For live manual tests, use public-domain works such as LibriVox recordings of classic novels.

## Acceptance criteria

- [ ] A book with an ISBN in its OPF file is matched with one search request.
- [ ] A book without local cover art gets a cover from Open Library or the Cover Art Archive.
- [ ] Local metadata is never overwritten by online data.
- [ ] With online metadata off, no request leaves the machine.
- [ ] The User-Agent is sent on every request.

## For the maintainer

- Read the Open Library terms of service and confirm that the planned use fits them.
- Decide whether an optional Audible-based community provider should be considered later, after checking its terms. It is not part of this issue.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Audiobook metadata and covers: Open Library and MusicBrainz' '2.5' 'feature,metadata,core,needs-human' "$TMP/05.md"

echo "Done."
