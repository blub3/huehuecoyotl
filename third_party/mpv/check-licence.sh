#!/usr/bin/env bash
# Licence assertions from SPEC section 3 ("The libmpv build job asserts -Dgpl=false
# and checks the FFmpeg configuration string"). Exits non-zero on any violation.
set -euo pipefail
out=${1:-out}
fail=0

echo "== FFmpeg config.h flags"
cat "$out/ffmpeg-licence-flags.txt"
grep -q '^#define CONFIG_GPL 0'      "$out/ffmpeg-licence-flags.txt" || { echo "FAIL: FFmpeg built with GPL"; fail=1; }
grep -q '^#define CONFIG_NONFREE 0'  "$out/ffmpeg-licence-flags.txt" || { echo "FAIL: FFmpeg built nonfree"; fail=1; }

echo "== FFmpeg licence string embedded in libavutil"
lic=$(strings "$out"/lib/libavutil.so.* | grep -m1 -E '^libavutil license: ' | sed 's/^libavutil license: //' || true)
echo "$lic"
case "$lic" in LGPL\ version\ 2.1*) ;; *) echo "FAIL: unexpected avutil licence: $lic"; fail=1;; esac

echo "== mpv build configuration"
cfg=$(strings "$out"/lib/libmpv.so.* | grep -m1 -E '(^| )-Dgpl=' || true)
echo "${cfg:0:300}"
echo "$cfg" | grep -q -- '-Dgpl=false' || { echo "FAIL: libmpv not configured with -Dgpl=false"; fail=1; }

echo "== Shared libraries in bundle (no static archives allowed)"
ls "$out"/lib/*.a 2>/dev/null && { echo "FAIL: static archives present"; fail=1; }
ls "$out"/lib/ | grep -E '\.so' | sed 's/^/  /'

echo "== External dependencies of libmpv"
deps=$(LD_LIBRARY_PATH="$out/lib" ldd "$out"/lib/libmpv.so.2)
echo "$deps" | awk '{print "  "$1" -> "$3}'
# Libraries we build must resolve inside the bundle. Desktop system libraries (PulseAudio,
# VA-API, ...) may be missing on a bare CI runner; that is a warning, not a failure.
missing=$(echo "$deps" | awk '/not found/{print $1}')
for m in $missing; do
  case "$m" in
    libav*|libsw*|libmpv*|libass*|libplacebo*|libwayland*) echo "FAIL: bundle library unresolved: $m"; fail=1;;
    *) echo "WARN: system library not present on this machine: $m";;
  esac
done

[ $fail -eq 0 ] && echo "LICENCE CHECK: PASS" || echo "LICENCE CHECK: FAIL"
exit $fail
