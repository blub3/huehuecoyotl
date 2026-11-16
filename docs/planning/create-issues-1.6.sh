#!/usr/bin/env bash
# Creates the second batch of Huehuecoyotl issues (and any missing labels and milestones).
#
# Usage:  run inside a clone of the repository, with the GitHub CLI signed in:
#           bash create-issues-1.6.sh            create everything
#           DRY_RUN=1 bash create-issues-1.6.sh  only print what would be created
#         or name the repository:  REPO=owner/name bash create-issues-1.6.sh
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

milestone '1.6' 'Episode tiles: versions, technical details and saving'

cat > "$TMP/01.md" <<'__ISSUE_BODY_EOF__'
## Context

Movies get three things on their detail page: a version dropdown, the technical details, and "Save" and "Save as…". Episodes get the same, but only on their own detail page. On the show page the user browses a season as a list of episode tiles, and should be able to do all three there, directly on each tile, without opening every episode.

## Dependencies

These issues must be done first. This issue reuses what they built and adds no new playback, probing or download logic.

- Shows: one cover per show with seasons inside, and fix duplicate entries (1.1.1): the show page and its episode list.
- Technical details for movies and episodes: video, audio, subtitles and bitrates (1.2): probe data, summary line and details panel.
- Movie page: choose between several versions of the same movie (1.2): version labels, preselection rules and resume across versions.
- Core: download engine and storage for offline copies (1.5).
- The 1.5 issues for "Save" (offline copy with delete) and "Save as…" (export to a chosen location).

## Assumption to confirm

"Tile" means each episode entry in the season's episode list on the show page. The episode detail page stays as it is.

## What to build

**Shared components first**

Move the movie page's version dropdown, summary line, details panel and Save menu into shared QML components, and use them on the movie page, the episode page and the episode tiles. Nothing is duplicated.

**On each episode tile**

- **Version dropdown.**
  - Shown only when the episode has two or more files.
  - Same labels as for movies, for example "2160p · HDR10 · HEVC · E-AC-3 5.1 · 4.2 GB · NAS".
  - Versions on an unreachable source are marked offline and cannot be played.
  - Preselection follows the movie rules: last played version, else the preferred-version setting.
- **Technical summary** on the tile, for example "1080p · H.264 · AAC 2.0 · 2 subtitles", with small badges for 4K, HDR10, Dolby Vision and Atmos.
  - "Details" expands the full technical details panel inline under the tile. Only one tile is expanded at a time.
  - While the file is not probed yet, the tile shows "Analysing…" and the panel offers "Analyse now".
- **Save menu.** One button that opens a menu:
  - "Save": offline copy in managed storage, with progress, pause and cancel on the tile while it runs.
  - "Save as…": export through the file dialog, or the portal under Flatpak.
  - "Delete saved copy": only when a copy made with "Save" exists, with the same confirmation as on the episode page.
  - The tile shows a saved mark, and a mark when the copy is outdated.
- **The selected version drives everything on that tile:** Play and Resume, the summary, the details panel, Save and Save as.
- **Episodes that are not on disk** (shown dimmed) get none of these controls.
- **Season actions stay** as built in 1.5: "Save season", and "Save as…" for a whole season.

**Core and data**

- The episode list query returns, per episode and in one paged query:
  - the versions with their labels;
  - the summary of the preselected version;
  - the saved-copy state.
- Load each tile's full details panel only when it is expanded. Never query per tile while scrolling.
- Download progress for many episodes reaches the list through the existing events (ids, at most twice per second per download). Updating one tile must not rebuild the list.

**Input**

- Keyboard and remote:
  - Up and down move between tiles.
  - Right moves into the tile's controls (version, Details, Save menu).
  - Back leaves the controls, and a second Back leaves the page.
  - The Menu key opens the Save menu of the focused tile.
- Focus is never lost when a tile expands, a menu closes or a download finishes.

## Tests

- Fixture library "Example Show" with one season of six episodes:
  - one episode in two versions, one of them on a source that is offline;
  - one episode not on disk;
  - one episode already saved;
  - one episode not probed yet.
- Qt Quick Test for focus and navigation across tiles and their controls.
- A model test proves that scrolling a season of 200 episodes issues paged queries only, and no per-tile queries.

## Acceptance criteria

- [ ] Every episode with two or more versions shows a version dropdown on its tile, and episodes with one version show none.
- [ ] Switching the version on a tile changes Play, the summary, the details panel and the Save target for that episode only.
- [ ] Save, Save as… and Delete saved copy work from the tile, and the episode detail page shows the same state.
- [ ] "Delete saved copy" never appears for a file that was only exported with "Save as…".
- [ ] Starting downloads for ten episodes keeps scrolling within the budget (60 Hz, integrated graphics), and each tile shows its own progress.
- [ ] The page works offline for probed and saved episodes; offline versions are marked and cannot be played.
- [ ] Everything is reachable with keyboard and remote, and all strings use `qsTr`, including the German translation.

## Out of scope

- A version choice that applies to a whole season at once (possible follow-up).
- Choosing audio or subtitle tracks from the tile.
- Multi-select of several episodes for saving; use "Save season" instead.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'Show page: version selection, technical details and Save menu on every episode' '1.6' 'feature,ui,core' "$TMP/01.md"

echo "Done."
