#!/usr/bin/env bash
# The Rust SBOM for a release (SPEC 10, step 6), without paths of the machine that built it:
# cargo-cyclonedx names the workspace's crates by their absolute path, so the checkout becomes
# /src/huehuecoyotl. Stops if the checkout or the home folder is still in the file.
#   tools/sbom.sh OUT
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$1"
command -v cargo-cyclonedx >/dev/null || { echo "sbom.sh: cargo-cyclonedx is not installed" >&2; exit 1; }
name=".sbom-rust-$$"
trap 'find "$root/crates" -maxdepth 2 -name "$name.json" -delete' EXIT
cargo cyclonedx --manifest-path "$root/crates/core-api/Cargo.toml" --format json --override-filename "$name" >/dev/null
tmp="$root/crates/core-api/$name.json"
ROOT="$root" perl -pi -e 's/\Q$ENV{ROOT}\E/\/src\/huehuecoyotl/g' "$tmp"
if grep -qF -e "$root" -e "$HOME" "$tmp"; then
  echo "sbom.sh: the SBOM still contains a local path" >&2
  exit 1
fi
mv "$tmp" "$out"
