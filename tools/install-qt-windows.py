#!/usr/bin/env python3
"""Install Qt for MSVC from Qt's official online repository (CI on Windows).

Since Qt 6.11 the repository has one folder per compiler (qt6_6120/qt6_6120_msvc2022_64/),
which aqtinstall 3.3 cannot read yet. This installs the base package (qtbase, qtdeclarative,
qttools, qtsvg, translations, the D3D compiler and the software OpenGL fallback), checks every
archive against the repository's SHA-1 and unpacks it with 7-Zip.

    python tools/install-qt-windows.py 6.12.0 C:/Qt
Prints the Qt prefix (C:/Qt/6.12.0/msvc2022_64).
"""
import hashlib
import subprocess
import sys
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

REPO = "https://download.qt.io/online/qtsdkrepository/windows_x86/desktop"
COMPILER = "msvc2022_64"
SKIP = ("qtdoc",)


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=120) as r:
        return r.read()


def main() -> int:
    version, dest = sys.argv[1], Path(sys.argv[2])
    tag = "qt6_" + version.replace(".", "")
    base = f"{REPO}/{tag}/{tag}_{COMPILER}"
    root = ET.fromstring(fetch(f"{base}/Updates.xml"))
    name = f"qt.qt6.{version.replace('.', '')}.win64_{COMPILER}"
    pkg = next(p for p in root.findall("PackageUpdate") if p.findtext("Name") == name)
    pkg_version = pkg.findtext("Version")
    archives = [a.strip() for a in pkg.findtext("DownloadableArchives").split(",") if a.strip()]
    # The 6.11+ archives hold the prefix's contents directly (bin/, lib/, ...).
    prefix = dest / version / COMPILER
    prefix.mkdir(parents=True, exist_ok=True)
    for arc in archives:
        if arc.startswith(SKIP):
            continue
        url = f"{base}/{name}/{pkg_version}{arc}"
        data = fetch(url)
        want = fetch(url + ".sha1").decode().split()[0]
        got = hashlib.sha1(data).hexdigest()
        if got != want:
            print(f"checksum mismatch for {arc}: {got} != {want}", file=sys.stderr)
            return 1
        tmp = dest / arc
        tmp.write_bytes(data)
        subprocess.run(["7z", "x", "-y", f"-o{prefix}", str(tmp)], check=True, stdout=subprocess.DEVNULL)
        tmp.unlink()
        print(f"installed {arc.split('-')[0]}", file=sys.stderr)
    if not (prefix / "lib" / "cmake" / "Qt6" / "Qt6Config.cmake").exists():
        print(f"no Qt found at {prefix}", file=sys.stderr)
        return 1
    print(prefix.as_posix())
    return 0


if __name__ == "__main__":
    sys.exit(main())
