#!/usr/bin/env bash
# Creates the second batch of Huehuecoyotl issues (and any missing labels and milestones).
#
# Usage:  run inside a clone of the repository, with the GitHub CLI signed in:
#           bash create-issues-1.6.1.sh            create everything
#           DRY_RUN=1 bash create-issues-1.6.1.sh  only print what would be created
#         or name the repository:  REPO=owner/name bash create-issues-1.6.1.sh
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

milestone '1.6.1' 'Bug fixes for 1.6'

cat > "$TMP/01.md" <<'__ISSUE_BODY_EOF__'
## What happens

After the app is restarted, the TMDB key field under Settings → Metadata is empty. The user cannot tell whether the key was lost and has to be entered again, or whether it is still stored and only not shown.

## Expected behaviour

The key is entered once and survives restarts and updates. This is required by the specification (sections 6 and 7): secrets are stored in the OS keystore through the `keyring` crate, and the database stores only a reference.

After a restart, Settings shows that a key is present, without revealing it:

- the status "Key stored";
- where it comes from: the keystore, or the environment variable `MDB_API_TOKEN` (for development only);
- when it was last validated successfully;
- the actions "Replace key", "Test key" and "Remove key".

The key itself is never shown, not even partly, and never logged.

## Fix

**1. Find out which of these is happening**, and describe the cause in the pull request:

| Case | Meaning |
| --- | --- |
| A. Display only | The key is stored and used, but the Settings screen never reads its state back and starts with an empty field. |
| B. Not persisted | The key is never written to a persistent keystore, so it is really gone after a restart. |
| C. Write fails | Writing to the keystore fails, and the error is swallowed. |

**2. Check these likely causes for B and C:**

- **`keyring` without a platform backend.** Recent major versions of the `keyring` crate select their backends through Cargo features. Built without them, the crate silently falls back to an in-memory mock store that is lost on exit. Check the enabled features and the version in `Cargo.toml`.
- **Flatpak permission.** The `keyring` Secret Service backend talks to `org.freedesktop.secrets` over D-Bus. Check that the Flatpak manifest grants that access, or that the chosen backend works through the portal.
- **No Secret Service.** The desktop may have no Secret Service running, or its collection may be locked.
- **Masked during development.** If the app is started from a shell with `MDB_API_TOKEN` set, metadata works even when nothing is stored. Test without the variable: for example the installed Flatpak launched from the desktop, or with `env -u MDB_API_TOKEN`.

**3. Check the other secrets too.** Source passwords for SMB, WebDAV, FTP and SFTP go through the same keystore code. If the cause is B or C, they are affected as well and are fixed in this issue.

**4. Fix the cause, and change the behaviour as follows:**

- A failed keystore write is reported in the UI with a clear message. Never fall back to storing a secret in a plain file or in the database.
- If no keystore is available, say so in Settings and explain what the user can do (for example install or unlock a keyring). The app keeps working without metadata.
- After the fix, users who lost their key see the first-run key screen once, explaining that the key has to be entered one more time.

## Tests

- Round trip across a process restart: store a key, end the process, start again, and read the state back.
  - Linux CI: run against a real Secret Service, for example `gnome-keyring-daemon` inside `dbus-run-session`.
  - Windows CI: run against Credential Manager.
- A test fails the build if `keyring` would use its mock store in a release configuration.
- A Settings model test checks every status: stored, from the environment variable, absent, keystore unavailable.
- A log search after the tests finds no key.

## Acceptance criteria

- [ ] After entering a key and restarting, Settings shows "Key stored" with its source and validation date, and metadata loads without entering the key again.
- [ ] This holds for the Flatpak started from the desktop launcher, with no `MDB_API_TOKEN` in its environment.
- [ ] Source passwords also survive a restart.
- [ ] "Remove key" deletes the keystore entry, and after a restart no key is reported.
- [ ] With no keystore available, the user sees an explanation instead of an empty field, and no secret is written anywhere else.
- [ ] The key never appears in the UI, the logs or the diagnostics export.

## Out of scope

- Storing keys anywhere other than the OS keystore.
- Syncing keys between devices.

## Working rules

- Read `docs/SPEC.md` and `CLAUDE.md` first. Their rules apply: C++ only where Qt or mpv APIs are used and Rust everywhere else, no GPL dependencies, offline-first, no secrets in the repository or in logs.
- Sign off every commit (`git commit -s`), open a pull request that references this issue, and keep CI green.
- Update `docs/SPEC.md` in the same pull request when behaviour described there changes.
- If something in this issue contradicts the specification or cannot be done, stop and say so in a comment. Do not choose a different design on your own.
__ISSUE_BODY_EOF__
issue 'TMDB key appears empty in Settings after every restart' '1.6.1' 'bug,core,ui,metadata' "$TMP/01.md"

echo "Done."
