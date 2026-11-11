#!/usr/bin/env bash
# Third-party notices for a release (SPEC 10, step 6): the app's own licence, Qt, the LGPL
# libmpv bundle and every Rust crate linked into the app. Needs cargo-about.
#   tools/notices.sh OUT [LIBMPV_PREFIX]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$1"
mpv="${2:-$root/third_party/mpv/out}"
command -v cargo-about >/dev/null || { echo "notices.sh: cargo-about is not installed (cargo install --locked cargo-about --features cli)" >&2; exit 1; }
tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT

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

  section "libmpv and its libraries (LGPL build, -Dgpl=false)"
  if [ -f "$mpv/BUILDINFO.txt" ]; then
    echo "Exact source revisions:"; sed 's/^/  /' "$mpv/BUILDINFO.txt"
    echo "Build recipe: third_party/mpv in the Huehuecoyotl source repository."
  fi
  lic="$mpv/share/licenses"
  if [ -d "$lic" ]; then
    for f in mpv/Copyright mpv/LICENSE.LGPL ffmpeg/LICENSE.md ffmpeg/COPYING.LGPLv2.1 libass/COPYING \
             libplacebo/LICENSE libplacebo/3rdparty-glad-LICENSE libplacebo/3rdparty-fast_float-LICENSE-MIT \
             libplacebo/3rdparty-Vulkan-Headers-LICENSE.md wayland/COPYING; do
      [ -f "$lic/$f" ] && file "$lic/$f" "$f"
    done
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
