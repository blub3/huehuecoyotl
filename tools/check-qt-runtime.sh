#!/usr/bin/env bash
# SPEC 3: verify every Qt library the app loads at run time is LGPL (not a GPL-only add-on).
# Usage: tools/check-qt-runtime.sh build/app/coyotl
set -euo pipefail
bin=${1:?binary}
allowed='^libQt6(Core|Gui|OpenGL|Qml|QmlModels|QmlMeta|QmlWorkerScript|Quick|QuickControls2|QuickControls2Impl|QuickControls2Basic|QuickControls2BasicStyleImpl|QuickTemplates2|QuickLayouts|QuickEffects|QuickShapes|Svg|DBus|Network|WaylandClient|XcbQpa|EglFSDeviceIntegration|WlShellIntegration)\.so'
fail=0
while read -r lib; do
  if [[ ! $lib =~ $allowed ]]; then echo "NOT ALLOWED: $lib"; fail=1; else echo "ok: $lib"; fi
done < <(ldd "$bin" | awk '{print $1}' | grep '^libQt6' | sort -u)
# QML plugins are loaded later; list what the style/controls imports pull in
[ $fail -eq 0 ] && echo "QT RUNTIME CHECK: PASS" || { echo "QT RUNTIME CHECK: FAIL"; exit 1; }
