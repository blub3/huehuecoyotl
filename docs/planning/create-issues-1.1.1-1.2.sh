#!/usr/bin/env bash
# Creates the second batch of Huehuecoyotl issues (and any missing labels and milestones).
#
# Usage:  run inside a clone of the repository, with the GitHub CLI signed in:
#           bash create-issues-1.1.1-1.2.sh            create everything
#           DRY_RUN=1 bash create-issues-1.1.1-1.2.sh  only print what would be created
#         or name the repository:  REPO=owner/name bash create-issues-1.1.1-1.2.sh
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

milestone '1.1.1' 'Bug fixes for 1.1'
milestone '1.2' 'Technical details and version selection on detail pages'

cat > "$TMP/01.md" <<'__ISSUE_BODY_EOF__'
## What happens

The Shows screen lists the same show many times. On the maintainer's library the header says "Shows 213", far more than the number of shows in it. The examples below use a Creative Commons series and made-up titles; they describe the patterns seen, not the maintainer's library.

| Pattern | Example | What the grid shows |
| --- | --- | --- |
| Matched show, split per episode | Pioneer One (2010), a web series released under a Creative Commons licence ([TMDB 33050](https://www.themoviedb.org/tv/33050)) | one cover per episode, each with the badge 1 |
| Matched show, split per season | "Example Show", three seasons | three covers whose badges equal each season's episode count |
| Not matched: punctuation differs | folder "Example Show The Return", TMDB title "Example Show: The Return" | placeholder covers without a year, still one per season |
| Not matched: country suffix | folder "Example Show US" | placeholder covers |
| Not matched: the title exists more than once | a title used by a 1970s and a 2000s series | placeholder covers |

What the patterns suggest:

- **Matched shows are split too.** Several entries carry the same title, year and poster, so they have the same TMDB id. Grouping is evidently not keyed on the TMDB show id.
- **Some shows split per season, some per episode.** This points at how the scanner derives the show from the folder layout: probably the nearest folder, or the file itself, becomes the show.
- **Some titles never match TMDB** because of punctuation, a country suffix, or an ambiguous title.

## Expected behaviour

- **One entry per show.** All seasons and episodes of a show sit under exactly one `show` item, with `season` and `episode` items below it (specification, section 7).
- The header count is the number of shows.
- An unmatched show is still grouped into one entry, by normalised title and year, and appears in the "needs review" list as one show, not once per season.

## Part 1: fix the grouping

1. **Reproduce first.** Add scanner fixtures for the common layouts, and write tests that fail today:
   - `Show (2022)/Season 01/Show - S01E01 - Title.mkv`
   - `Show/S01/Show.S01E01.1080p.mkv`
   - `Show/Show.S01.1080p.WEB/Show.S01E01.1080p.WEB.mkv` (release folder per season)
   - `Shows/Show.S01E01.mkv` (flat, no show folder)
   - `Show/Specials/Show - S00E01.mkv`
   - one show spread over two sources (two shares)
2. **Find the cause** and describe it in the pull request.
3. **Grouping rules:**
   - The show is derived from the file name tokens and the folder above any season or release folder, never from the season folder itself.
   - Before matching, episodes are grouped by a normalised show key: case-folded, punctuation removed, articles and a trailing year handled, country suffixes such as US and UK kept as a hint.
   - After matching, the TMDB show id is the identity. Two show items with the same TMDB id must never exist; enforce this with a unique index or an equivalent check.
   - A user's manual correction (`locked`) always wins.
4. **Matching improvements** for the unmatched cases above:
   - Search with punctuation-insensitive titles, so "Example Show The Return" finds "Example Show: The Return".
   - Turn a country suffix into an origin-country filter.
   - Use a year from the folder name when present.
   - When several candidates fit, compare the number of seasons and episodes on disk with each candidate. If it is still ambiguous, send the show to review, once.
5. **Repair existing libraries** with a numbered migration plus a background job:
   - Merge show items with the same TMDB id.
   - Re-parent their seasons and episodes, merging duplicate season items.
   - Merge watch state, keeping the newest `updated_at` per episode.
   - Delete the empty show items and their orphaned artwork rows.
   - Re-group unmatched shows by the normalised key.
   - No file is rescanned from the network for this, and no watch state is lost.
6. **Check Movies for the same defect.** If one movie appears more than once in the Movies grid for the same TMDB id, fix it here as well. Several files of one movie belong to one item (see the issue about choosing between versions).

## Part 2: show page with season switching

One cover per show only helps if the seasons are reachable from it. Selecting a show opens a show page where the user moves between seasons and episodes.

**Shows grid**

- One cover per show (Part 1).
- The badge shows the number of unwatched episodes and disappears when everything is watched. Today the badge's meaning is unclear: it shows season or episode counts.

**Show page**

- Header: backdrop, poster, title, years, rating, genres, overview.
- Main button: "Play S02E03" for the next unwatched episode, or "Resume S02E03" when an episode is partly watched, or "Play again" when the show is finished.
- Season switcher:
  - A row of season tabs, or a dropdown when there are more than about eight seasons.
  - Specials (season 0) come last.
  - The page opens on the season that holds the next episode.
- Episode list for the selected season. Each row shows:
  - A still image, number, title, runtime and a short overview.
  - Watched state, and a progress bar for a partly watched episode.
  - An "on disk" marker, since episodes that TMDB knows but that are not in the library are shown dimmed and cannot be played.
- Actions:
  - On each episode: play, mark watched or unwatched.
  - On the season: mark the whole season watched or unwatched.
  - On the show: mark the whole show watched or unwatched.
- An episode opens its own detail page with the technical details and the version selection (issues in milestone 1.2).

**Data**

- Season and episode data come from the local database only, so the page works offline. Missing season data is fetched by the job queue in the background.
- Paged queries through the existing bridge; events carry ids, not payloads (specification, section 8).

**Input**

- Keyboard and remote work throughout:
  - Left and right switch seasons.
  - Up and down move through episodes.
  - Back returns to the grid with the same cover focused.

## Acceptance criteria

**Grouping**


- [ ] Every fixture layout produces exactly one show item, with the right seasons and episodes.
- [ ] A library database written by 1.1.0 from the fixtures, with the duplicates above, is repaired by the upgrade: each show appears once and the count in the header drops accordingly.
- [ ] Against recorded TMDB responses, the fixture cases for punctuation, a country suffix and an ambiguous title resolve as specified: matched, or sent to review once.
- [ ] With the `live-tmdb` feature, a fixture folder `Pioneer One (2010)/Season 01/` with episode files matches TMDB show 33050.
- [ ] A test asserts that no two show items share a TMDB id after a scan.
- [ ] Watch state and resume positions from before the upgrade are intact.
- [ ] The scroll budget is still met.

**Show page**

- [ ] Selecting a show in the grid always opens one page that contains all its seasons.
- [ ] The main button picks the right episode in four cases: nothing watched, one episode half watched, a season finished, everything watched.
- [ ] Marking a season watched updates the episode list, the main button and the grid badge without a restart.
- [ ] Episodes not on disk are visibly distinct and cannot be started.
- [ ] Works offline, in windowed and fullscreen layouts, and stays within the scroll budget for a show with 20 seasons.
- [ ] All strings use `qsTr`, including the German translation.

## Out of scope

- Absolute episode order or DVD order for anime.
- Downloading missing episodes.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Shows: one cover per show with seasons inside, and fix duplicate entries' '1.1.1' 'bug,core,ui,metadata' "$TMP/01.md"

cat > "$TMP/02.md" <<'__ISSUE_BODY_EOF__'
## Context

The detail page of a movie (and of an episode) should show the technical properties of the file: resolution, HDR format, codecs, audio tracks, subtitles, bitrates and so on. The specification already plans for this: `file.probe_json` holds "tracks, resolution, HDR, duration", filled "lazily, on first play or when idle" (section 7). Nothing reads it in the UI yet, and it is not clear how it is filled today.

## Where the data comes from

Find out how `probe_json` is filled today and report it in the pull request. The probe has to work without starting playback and for files on every source type.

**Recommended approach (confirm with the maintainer before building):**

- Probe in the Rust core with FFmpeg's libavformat. That library is already shipped in the LGPL bundle, so no new dependency is shipped.
- Access it through small bindings generated in this repository (bindgen). Do not use a binding crate whose licence is not on the allowlist.
- Feed it a custom I/O context that reads through `Source::read_at`, so SMB, NFS, WebDAV and FTP work like local files.

**Alternatives:**

- A headless mpv instance in the C++ shell. Under the language rule the shell should not take on a scanning job.
- Pure-Rust container parsers. These do not cover all containers, for example MPEG-TS and AVI.

**Fallback in any case:** when a file is played, `MpvPlayer` reads mpv's `track-list`, `video-params` and `audio-params`. It reports them to the core over the bridge, which fills or corrects the stored data.

Amend sections 3, 7 and 10 of the specification for the chosen approach.

## Probe job

- Low priority and network-friendly: limit how much is read per file, and never read the whole file.
- Re-probe when size or modification time change.
- `probe_json` gets a schema version, so later fields can be added with a re-probe.
- Command-line harness: `probe <file>` prints the result.

## What to show

**Summary line** under the title, built from the probe data, for example: "4K · HDR10 · HEVC 10-bit · TrueHD 7.1 · 3 subtitles". The same data supplies small badges (4K, HDR10, Dolby Vision, Atmos when detectable).

**Technical details panel** below it, collapsible and fully usable with the remote:

| Group | Fields |
| --- | --- |
| File | file name, source and path, size, container, duration, overall bitrate, chapters |
| Video | codec with profile and level, resolution, display aspect ratio, frame rate, scan type, bit depth, chroma subsampling, HDR format (HDR10, HDR10+, Dolby Vision with profile, HLG), colour primaries and transfer, bitrate |
| Audio, per track | language, title, codec and format (for example TrueHD Atmos, DTS-HD MA, E-AC-3 JOC), channel layout, sample rate, bit depth, bitrate, default and forced flags |
| Subtitles, per track | language, title, format (SRT, ASS, PGS, VobSub), embedded or external file, default, forced and SDH flags |

**Rules for the values**

- **Bitrates:** take them from the stream, or from container statistics such as Matroska's `BPS` tags. When neither exists, estimate from size and duration and label the value "approx.". Never show an invented number.
- **Atmos and DTS:X:** show them only when the stream signals them reliably. Otherwise show the base format.
- **Not yet probed:** show "Analysing…" while the job runs, and a "Analyse now" action that moves the file to the front of the queue.
- **Episodes** get the same panel on their detail page.

## Test material

Automated fixtures are generated in the test setup. For manual tests, use freely licensed films such as the Blender Foundation's open movies (released under CC BY), for example Tears of Steel or Sintel. Do not commit video files to the repository.

## Acceptance criteria

- [ ] Fixture files cover the main cases: MKV with HEVC 10-bit HDR10 and several audio and subtitle tracks, MP4 with H.264 and AAC, and MPEG-TS. Each produces the expected `probe_json`; generate the fixtures in the test setup, without copyrighted content.
- [ ] Probing a 50 GB file on an SMB share reads only a small part of it; the test asserts the number of bytes read.
- [ ] A Dolby Vision file shows its profile, and an HDR10 file shows HDR10.
- [ ] Estimated bitrates are labelled as estimated.
- [ ] The details work offline once probed, and the panel works with keyboard and remote.
- [ ] Licence checks stay green; no new library is shipped.

## Out of scope

- Choosing audio and subtitle tracks before playback (later issue).
- MediaInfo-style raw dumps of every field.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Technical details for movies and episodes: video, audio, subtitles and bitrates' '1.2' 'feature,core,ui,playback' "$TMP/02.md"

cat > "$TMP/03.md" <<'__ISSUE_BODY_EOF__'
## Context

The library can hold the same movie more than once: for example Tears of Steel (a Blender Foundation open movie under CC BY) in 4K and in 1080p, a director's cut of a film, or the same file on two shares. The data model already links several files to one item (specification, section 7). The movie page should offer a dropdown to choose which version to play.

## Dependencies

- Version labels use the probe data from "Technical details for movies and episodes: video, audio, subtitles and bitrates". Until a file is probed, the label falls back to tokens from the file name.
- If movies currently appear more than once in the Movies grid, that is fixed in "Shows: one cover per show with seasons inside, and fix duplicate entries". This issue assumes one item per movie.

## What to build

**Dropdown**

- The movie page shows a version dropdown when the item has two or more files. With one file it shows nothing.
- Each entry has a short label: resolution, HDR format, video codec, main audio format, size and source. For example: "2160p · Dolby Vision · HEVC · TrueHD Atmos · 61 GB · NAS".
- Editions are recognised from the file name and shown in the label: Director's Cut, Extended, Theatrical, IMAX, and the `{edition-…}` naming convention that other media servers use.
- Versions on an unreachable source are listed but marked as offline and cannot be selected for playback. A saved offline copy (from 1.5) counts as its own reachable version.

**What the selection drives**

- The Play and Resume button.
- The technical details panel.
- From 1.5, "Save" and "Save as…".

**Which version is preselected**

1. The version last played for this movie.
2. Otherwise the highest-quality reachable version: resolution first, then HDR, then bitrate.
3. A setting under Playback, "Preferred version", offers "Highest quality" (the default) and "Smallest file", for slow networks.

**Resume and watched state** belong to the movie, not to a file.

- Switching versions keeps the resume position when the durations differ by less than 2 %.
- For different editions (a larger difference), the position is stored per file and the page says "Resume not available for this version".

**Wrong merges**

- A "This is a different movie" action on a version splits that file into its own item and sends it to the review list.
- The split is locked, so the next scan does not merge it again.

**Episodes:** the same dropdown on the episode detail page when an episode exists in several versions.

## Acceptance criteria

- [ ] A movie with three files on two sources shows one cover in the grid and three entries in the dropdown, with distinguishable labels.
- [ ] Preselection follows the rules above, including the last-played version.
- [ ] Playing a version, switching to another and back keeps one shared watched state for the movie.
- [ ] An offline source's version is marked and cannot be played; the other versions still work.
- [ ] A split movie stays split after a rescan.
- [ ] The dropdown works with keyboard and remote, and all strings use `qsTr`.

## Out of scope

- Automatic deletion of lower-quality duplicates.
- Choosing the version from the grid without opening the page.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Movie page: choose between several versions of the same movie' '1.2' 'feature,ui,core' "$TMP/03.md"

echo "Done."
