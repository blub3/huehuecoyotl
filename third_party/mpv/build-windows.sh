#!/usr/bin/env bash
# Builds the Windows LGPL libmpv bundle in Docker (MinGW-w64 cross-build) and copies it to ./out-win
set -euo pipefail
cd "$(dirname "$0")"
docker build -f Dockerfile.windows -t coyotl-libmpv-lgpl-win .
rm -rf out-win && mkdir -p out-win
cid=$(docker create coyotl-libmpv-lgpl-win)
docker cp "$cid:/opt/coyotl-mpv-win/." out-win/
docker rm "$cid" >/dev/null
