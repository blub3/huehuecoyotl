#!/usr/bin/env bash
# Builds the Windows LGPL libmpv bundle in Docker (MinGW-w64 cross-build) and copies it to ./out-win.
# ./out-win-src gets the bundle's corresponding source: the exact trees that were built, without
# build output, plus BUILDINFO.txt and this recipe.
set -euo pipefail
cd "$(dirname "$0")"
image=coyotl-libmpv-lgpl-win
if [ "${1:-}" != "--sources-only" ]; then
  docker build -f Dockerfile.windows -t "$image" .
  rm -rf out-win && mkdir -p out-win
  cid=$(docker create "$image")
  docker cp "$cid:/opt/coyotl-mpv-win/." out-win/
  docker rm "$cid" >/dev/null
fi
rm -rf out-win-src && mkdir -p out-win-src
docker run --rm -e XZ_OPT=-T0 -v "$PWD:/recipe:ro" "$image" tar -cJf - \
  --no-anchored --exclude=.git \
  --anchored --no-wildcards-match-slash \
  --exclude='src/*/builddir' --exclude='src/mpv/build' --exclude='src/*.tar.*' \
  -C / src -C /opt/coyotl-mpv-win BUILDINFO.txt \
  -C /recipe Dockerfile.windows build-windows-deps.sh build-windows.sh \
  > out-win-src/huehuecoyotl-windows-lgpl-sources.tar.xz
