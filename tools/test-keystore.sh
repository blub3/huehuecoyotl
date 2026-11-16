#!/bin/sh
# Issue #52: run the keystore tests against a real Secret Service that is not the user's: a
# private D-Bus session with its own gnome-keyring-daemon, in a temporary home that is deleted
# afterwards. Needs dbus-run-session and gnome-keyring-daemon (Linux).
#   tools/test-keystore.sh [ARGS]     the restart round trip, debug and release builds; ARGS go
#                                     to the round-trip test (--nocapture shows each step)
set -eu
cd "$(dirname "$0")/.."
command -v dbus-run-session >/dev/null || { echo "dbus-run-session is missing" >&2; exit 1; }
command -v gnome-keyring-daemon >/dev/null || { echo "gnome-keyring-daemon is missing" >&2; exit 1; }

# Build first, with the normal environment, so the isolated run only runs tests.
cargo test -p huehuecoyotl-core --test keystore --lib --no-run -q
cargo test --release -p huehuecoyotl-core --lib --no-run -q

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -m 700 "$tmp/run"
env -i PATH="$PATH" HOME="$tmp" XDG_DATA_HOME="$tmp/data" XDG_CONFIG_HOME="$tmp/config" \
    XDG_RUNTIME_DIR="$tmp/run" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" \
    RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" COYOTL_TEST_KEYRING=1 \
    COYOTL_TEST_ARGS="$*" dbus-run-session -- sh -ec '
        # A new, unlocked login keyring (the password only protects this throwaway one).
        printf throwaway | gnome-keyring-daemon --unlock --components=secrets >/dev/null
        # Wait until it owns the Secret Service name; asked earlier, D-Bus would start a second
        # daemon that waits for a password prompt.
        for i in $(seq 50); do
            dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
                org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null \
                | grep -q "boolean true" && break
            sleep 0.1
        done
        cargo test -q -p huehuecoyotl-core --test keystore -- --test-threads=1 $COYOTL_TEST_ARGS
        cargo test -q -p huehuecoyotl-core --lib secrets::
        # The release configuration must not use keyring'"'"'s in-memory mock store either.
        cargo test -q --release -p huehuecoyotl-core --lib secrets::
    '
echo "keystore tests passed"
