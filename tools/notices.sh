#!/usr/bin/env bash
# Third-party notices for a release (SPEC 10, step 6): the app's own licence, Qt, the LGPL
# libmpv bundle and every Rust crate linked into the app. Needs cargo-about.
#   tools/notices.sh OUT [LIBMPV_PREFIX]
# COYOTL_NOTICES_FLATPAK=1: for the Flatpak, which takes FFmpeg and Wayland from the KDE runtime
# (issue #161) and so does not ship the bundle's copies.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$1"
mpv="${2:-$root/third_party/mpv/out}"
command -v cargo-about >/dev/null || { echo "notices.sh: cargo-about is not installed (cargo install --locked cargo-about --features cli)" >&2; exit 1; }
tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
flatpak="${COYOTL_NOTICES_FLATPAK:-}"

section() { printf '\n\n================================================================================\n%s\n================================================================================\n' "$1"; }
file() { printf '\n----- %s -----\n\n' "$2"; cat "$1"; }

{
  echo "Huehuecoyotl – third-party notices"
  echo
  echo "Huehuecoyotl is licensed under the Apache License 2.0. It ships the libraries below"
  echo "under their own licences. The LGPL libraries are separate shared libraries that you may"
  echo "replace with your own builds; their exact sources are listed under each library."

  section "Huehuecoyotl"
  cat "$root/NOTICE"
  file "$root/LICENSE" "Apache License 2.0"

  section "Qt 6 (LGPL-3.0-only) – https://www.qt.io"
  echo "Modules: Qt Core, Gui, OpenGL, Qml, Quick, Quick Controls 2, D-Bus (Linux)."
  echo "Source: https://download.qt.io/official_releases/qt/"
  echo "Qt contains third-party code under its own licences, listed at"
  echo "https://doc.qt.io/qt-6/licenses-used-in-qt.html"
  file "$root/packaging/licenses/LGPL-3.0-only.txt" "GNU Lesser General Public License v3"
  file "$root/packaging/licenses/GPL-3.0-only.txt" "GNU General Public License v3 (referenced by the LGPL v3)"

  if [ -n "$flatpak" ]; then
    section "FFmpeg and codecs in the Flatpak (not part of this package)"
    echo "This package contains no FFmpeg. It uses the FFmpeg of the KDE runtime (LGPL), which has"
    echo "no H.264, HEVC, VC-1 or E-AC-3 decoder. Flathub's separate runtime extension"
    echo "org.freedesktop.Platform.codecs-extra adds them. That extension's FFmpeg is licensed"
    echo "under the GPL version 2 or later and links x264 and x265 (GPL), so when it is installed"
    echo "the running app loads GPL libraries. Flathub distributes the runtime and the extension"
    echo "and publishes their sources: https://github.com/flathub/org.kde.Platform and"
    echo "https://gitlab.com/freedesktop-sdk/freedesktop-sdk"
  fi

  section "libmpv and its libraries (LGPL build, -Dgpl=false)"
  if [ -f "$mpv/BUILDINFO.txt" ]; then
    echo "Exact source revisions:"
    if [ -n "$flatpak" ]; then grep -vE '^(ffmpeg|wayland|wayland-protocols) ' "$mpv/BUILDINFO.txt" | sed 's/^/  /'
    else sed 's/^/  /' "$mpv/BUILDINFO.txt"; fi
    echo "Build recipe: third_party/mpv in the Huehuecoyotl source repository."
  fi
  lic="$mpv/share/licenses"
  if [ -d "$lic" ]; then
    # Every licence file the recipe collected. The GPL texts that FFmpeg and mpv carry for their
    # GPL configurations do not apply to these LGPL builds and are left out.
    while IFS= read -r f; do
      rel="${f#"$lic"/}"
      if [ -n "$flatpak" ]; then case "$rel" in ffmpeg/*|wayland/*) continue ;; esac; fi
      case "$(basename "$f")" in *LGPL*|*lgpl*) ;; *GPL*) continue ;; esac
      file "$f" "$rel"
    done < <(find "$lic" -type f | LC_ALL=C sort)
    # nv-codec-headers (MIT) carries its licence only in the header comments.
    hdr="$mpv/include/ffnvcodec/dynlink_loader.h"
    if [ -f "$hdr" ]; then printf '\n----- nv-codec-headers (MIT) -----\n\n'; sed -n '1,/\*\//p' "$hdr"; fi
  else
    echo "(licence texts missing: rebuild the bundle with third_party/mpv/build.sh)" >&2
    exit 1
  fi

  section "Rust crates"
  cargo about generate --locked -m "$root/crates/core-api/Cargo.toml" "$root/tools/about.hbs" -o "$tmp" 2>/dev/null
  cat "$tmp"
} > "$out"
echo "notices.sh: wrote $out ($(wc -c < "$out") bytes)"
