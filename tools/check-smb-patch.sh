#!/usr/bin/env bash
# Checks that third_party/smb is the published smb 0.12.1 crate with its upstream licence file
# added and one change: the sspi
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
# Issue #163: the one added file is the upstream licence (afiffon/smb-rs, LICENSE.md, commit
# 7c7d4a60c930, from which 0.12.1 was published); the crate does not contain it.
echo "b1ae904b7ae199a3cce5c5b8ca5b3fa323b98c51ac4d30e40775af11e9967788  $root/third_party/smb/LICENSE.md" | sha256sum -c --quiet
# Everything except Cargo.toml and that file is byte for byte the published crate.
diff -r --exclude=Cargo.toml --exclude=LICENSE.md "$upstream" "$root/third_party/smb"
[ ! -e "$upstream/LICENSE.md" ] || { echo "the published crate now has its own LICENSE.md" >&2; exit 1; }
# Cargo.toml differs only in the sspi version.
changes=$(diff "$upstream/Cargo.toml" "$root/third_party/smb/Cargo.toml" | grep '^[<>]' || true)
expected=$'< version = "=0.21.3"\n> version = "=0.23.1"'
if [ "$changes" != "$expected" ]; then
  echo "third_party/smb/Cargo.toml has unexpected changes:" >&2
  echo "$changes" >&2
  exit 1
fi
echo "third_party/smb: smb $version from crates.io, with its upstream licence and only the sspi version changed"
