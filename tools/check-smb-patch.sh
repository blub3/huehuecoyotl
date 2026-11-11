#!/usr/bin/env bash
# Checks that third_party/smb is the published smb 0.12.1 crate with one change: the sspi
# version (0.21.3 -> 0.23.1, upstream afiffon/smb-rs#189). Run in CI; remove this script and the
# [patch.crates-io] entry once smb releases with the newer sspi.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
version=0.12.1
sha256=b60b8c4d8566c3cf19d17837958e67acc0534e68f331e898a642f4f57e9ecc4c
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sSfL -A "huehuecoyotl-ci" -o "$tmp/smb.crate" "https://static.crates.io/crates/smb/smb-$version.crate"
echo "$sha256  $tmp/smb.crate" | sha256sum -c --quiet
tar -xzf "$tmp/smb.crate" -C "$tmp"
upstream="$tmp/smb-$version"
# Everything except Cargo.toml is byte for byte the published crate.
diff -r --exclude=Cargo.toml "$upstream" "$root/third_party/smb"
# Cargo.toml differs only in the sspi version.
changes=$(diff "$upstream/Cargo.toml" "$root/third_party/smb/Cargo.toml" | grep '^[<>]' || true)
expected=$'< version = "=0.21.3"\n> version = "=0.23.1"'
if [ "$changes" != "$expected" ]; then
  echo "third_party/smb/Cargo.toml has unexpected changes:" >&2
  echo "$changes" >&2
  exit 1
fi
echo "third_party/smb: smb $version from crates.io, only the sspi version changed"
