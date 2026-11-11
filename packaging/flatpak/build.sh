#!/usr/bin/env bash
# Build the Flatpak locally: vendors the Rust crates, generates the notices, runs flatpak-builder
# and exports a single-file bundle.
#   packaging/flatpak/build.sh [--install]
# Needs: flatpak-builder (or org.flatpak.Builder), org.kde.Sdk//6.11,
# org.freedesktop.Sdk.Extension.rust-stable, cargo-about.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
gen="$here/generated"
app=io.github.blub3.Huehuecoyotl

rm -rf "$gen" && mkdir -p "$gen"
# Rust crates for an offline build. The config maps crates.io (and any git sources) to the copy.
(cd "$root" && cargo vendor --locked --versioned-dirs "$gen/vendor" 2>/dev/null > "$gen/cargo-config.toml")
grep -q vendored-sources "$gen/cargo-config.toml" || { echo "build.sh: cargo vendor printed no config" >&2; exit 1; }
# Cargo resolves this relative to the parent of the folder holding config.toml, i.e. the
# module root, where each module puts its vendor copy.
sed -i 's|directory = ".*"|directory = "vendor"|' "$gen/cargo-config.toml"
"$root/tools/notices.sh" "$gen/THIRD_PARTY_NOTICES.txt"

if command -v flatpak-builder >/dev/null; then fb=(flatpak-builder)
# The sandboxed builder forwards its own XDG_DATA_HOME to host commands, which hides per-user
# runtimes from them; FLATPAK_USER_DIR points them back at the real installation.
else fb=(flatpak run --env=FLATPAK_USER_DIR="$HOME/.local/share/flatpak" --command=flatpak-builder org.flatpak.Builder); fi

# --user: find the SDK in the per-user installation (and install there with --install).
install=(--user)
[ "${1:-}" = "--install" ] && install+=(--install)
cd "$here"
"${fb[@]}" --force-clean --repo=repo --state-dir=.flatpak-builder "${install[@]}" build-dir "$app.yml"
flatpak build-bundle repo "$root/build/$app.flatpak" "$app" --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
echo "bundle: build/$app.flatpak"
