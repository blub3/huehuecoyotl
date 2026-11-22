#!/usr/bin/env bash
# Build the Flatpak locally: vendors the Rust crates, generates the notices, runs flatpak-builder
# and exports a single-file bundle.
#   packaging/flatpak/build.sh [--install] [--bundle-sources]
# Needs: flatpak-builder (or org.flatpak.Builder), org.kde.Sdk//6.11,
# org.freedesktop.Sdk.Extension.rust-stable, cargo-about.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
gen="$here/generated"
app=io.github.blub3.Huehuecoyotl
builder_args=()
source_bundle=false
install=false
for arg in "$@"; do
  case "$arg" in
    --bundle-sources) source_bundle=true ;;
    --install) install=true ;;
    *) echo "Usage: $0 [--install] [--bundle-sources]" >&2; exit 2 ;;
  esac
done

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
else fb=(flatpak run --cwd="$here" --env=FLATPAK_USER_DIR="$HOME/.local/share/flatpak" --command=flatpak-builder org.flatpak.Builder); fi

# --user: find the SDK in the per-user installation.
# The Flatpak build has no .git folder: the commit goes into BUILD_ID for the app's version
# (empty for the commit tagged v<version>, a release; see app/CMakeLists.txt).
version=$(sed -n 's/^project(Huehuecoyotl VERSION \([0-9.]*\).*/\1/p' "$root/CMakeLists.txt")
if git -C "$root" describe --tags --exact-match --match "v$version" HEAD >/dev/null 2>&1; then
  : > "$root/BUILD_ID"
  source_bundle=true
else
  git -C "$root" rev-parse --short=7 HEAD > "$root/BUILD_ID"
fi
cd "$here"
out="$root/build"
mkdir -p "$out"
if "$source_bundle"; then
  builder_args+=(--bundle-sources)
fi
"${fb[@]}" --user --force-clean --delete-build-dirs --rebuild-on-sdk-change --repo=repo --state-dir=.flatpak-builder \
  "${builder_args[@]}" build-dir "$app.yml"
staging=$(mktemp -d "$out/.flatpak.XXXXXX")
trap 'rm -rf "$staging"' EXIT
flatpak build-bundle repo "$staging/$app.flatpak" "$app" master --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
if "$source_bundle"; then
  flatpak build-bundle --runtime repo "$staging/$app.Sources.flatpak" "$app.Sources" master
  {
    printf 'Version: %s\nApplication source: %s\n' "$version" "$(git -C "$root" rev-parse HEAD)"
    for ref in org.kde.Platform//6.11 org.kde.Sdk//6.11 org.freedesktop.Sdk.Extension.rust-stable//25.08; do
      printf '%s: ' "$ref"
      flatpak info --user --show-commit "$ref"
    done
    "${fb[@]}" --version
  } > "$staging/flatpak-BUILDINFO.txt"
  echo "sources: build/$app.Sources.flatpak"
fi
mv "$staging/"* "$out/"
echo "bundle: build/$app.flatpak"
# Installed with the host's flatpak, not flatpak-builder's --install: inside the sandboxed
# builder that writes /app/bin/flatpak into the exported desktop entry, which then does not
# start and is left out of the application menu.
if "$install"; then
  flatpak install --user --noninteractive --reinstall "$root/build/$app.flatpak"
fi
