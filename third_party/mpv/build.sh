#!/usr/bin/env bash
# Builds the LGPL libmpv bundle in Docker and copies it to ./out
set -euo pipefail
cd "$(dirname "$0")"
docker build -t coyotl-libmpv-lgpl .
rm -rf out && mkdir -p out
cid=$(docker create coyotl-libmpv-lgpl)
docker cp "$cid:/opt/coyotl-mpv/." out/
docker rm "$cid" >/dev/null
./check-licence.sh out
