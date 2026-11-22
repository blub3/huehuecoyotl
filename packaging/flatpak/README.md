# Flatpak release sources

Build from the release tag with `packaging/flatpak/build.sh --bundle-sources`.
The script also enables source bundling automatically when HEAD has the app's release tag.
Keep these files from that build together on the GitHub release:

- `build/io.github.blub3.Huehuecoyotl.flatpak`, the app.
- `build/io.github.blub3.Huehuecoyotl.Sources.flatpak`, the source bundle.
- `build/flatpak-BUILDINFO.txt`, the release version, app revision, runtime and SDK commits, and builder version.
- `packaging/flatpak/generated/THIRD_PARTY_NOTICES.txt`, the licence notices.

The release workflow stages these files, the Rust SBOM, and `SHA256SUMS`. Releases before
2.9.2 instead carry `flatpak-lgpl-sources.tar.gz` and `SHA256SUMS-lgpl`, with their own rebuild
instructions inside the archive, and a `libmpv-BUILDINFO.txt` that describes the separate
Docker build of libmpv rather than the Flatpak; it is no longer published.
For a manual release, include the same assets and generate the checksums from the staged files.
Run `sha256sum -c SHA256SUMS` in the download directory to verify the pair.

The source bundle contains the build manifest and source snapshots for FFmpeg, libmpv and
libplacebo, including Git submodules, plus the other modules' sources held by Flatpak Builder.
The Huehuecoyotl application source is available from the Git tag for the release.
KDE Platform supplies Qt and other runtime libraries through Flathub. The source bundle covers
the libraries distributed inside the app Flatpak. See the [KDE runtime build repository](https://github.com/flathub/org.kde.Platform) for runtime sources.

## Rebuild the bundled playback libraries

You need Flatpak, Flatpak Builder and the user-installed KDE SDK and Rust SDK extension listed
in `flatpak-BUILDINFO.txt`. Flatpak Builder is either a native `flatpak-builder` command or the
Flathub app `org.flatpak.Builder`; for the app, replace `flatpak-builder` below with
`flatpak run --cwd="$PWD" --env=FLATPAK_USER_DIR="$HOME/.local/share/flatpak" --command=flatpak-builder org.flatpak.Builder`. Use the recorded SDK commits if you
need the original toolchain. From an empty working directory with the source bundle:

```sh
app=io.github.blub3.Huehuecoyotl
flatpak install --user --bundle --no-deps "./$app.Sources.flatpak"
sources="$(flatpak info --user --show-location "$app.Sources//master")/files"
mkdir .flatpak-builder
cp -a "$sources/git" .flatpak-builder/
cp "$sources/manifest/$app.json" "$app.yml"
flatpak-builder --user --disable-download --stop-at=cxxbridge \
  build-dir "$app.yml"
```

The bundled manifest has a `.json` filename. Copying it to `.yml` lets Flatpak Builder
read the YAML used by this project. Stopping before `cxxbridge` builds all the bundled playback libraries into
`build-dir/files/lib` using the original manifest options. Qt remains part of the KDE runtime.

## Use a rebuilt or modified library

From 2.9.2 the app binary finds its libraries through `DT_RUNPATH`, so `LD_LIBRARY_PATH`
comes first. Run the installed app with your build of the libraries:

```sh
lib="$PWD/build-dir/files/lib"
flatpak run --filesystem="$lib:ro" --env=LD_LIBRARY_PATH="$lib" io.github.blub3.Huehuecoyotl
```

A replacement must keep the library's soname and interface (`libmpv.so.2`, `libavcodec.so.62`
and so on). Releases before 2.9.2 use `DT_RPATH`, which `LD_LIBRARY_PATH` cannot override; for
those, rebuild the whole Flatpak from its manifest with the changed module.

Checked on Nov 22, 2026 with the 2.9.2 development build: the libraries were rebuilt offline
from the source bundle with the steps above (Flathub's Flatpak Builder), and the app ran on all
four rebuilt libraries (`libmpv`, `libavcodec`, `libplacebo`, `libass`, seen in its memory maps).
