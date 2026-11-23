**Linux:** install the Flatpak bundle (it pulls the KDE 6.11 runtime from Flathub), then start Huehuecoyotl from the application menu:

```bash
flatpak install --user io.github.blub3.Huehuecoyotl.flatpak
```

**Windows (x64):** neither package is signed yet, so Windows warns when you open them.

- **Portable (Windows 10 and 11, simplest):** unzip `Huehuecoyotl-…-windows-x64.zip` anywhere and run `Huehuecoyotl\bin\coyotl.exe`. Nothing is installed; SmartScreen asks once (*More info → Run anyway*).
- **MSIX (Windows 11 only while unsigned; Start menu entry, file associations, clean uninstall):** in PowerShell, `Add-AppxPackage -Path .\Huehuecoyotl-…-windows-x64.msix -AllowUnsigned`. See the [user guide](https://github.com/blub3/huehuecoyotl/blob/main/docs/user/README.md#windows) for details.

The [user guide](https://github.com/blub3/huehuecoyotl/blob/main/docs/user/README.md) covers libraries, network shares and the TMDB key; [CHANGELOG.md](https://github.com/blub3/huehuecoyotl/blob/main/CHANGELOG.md) lists the changes and known limitations.

**Sources and licences.** The Flatpak contains no FFmpeg. Qt and FFmpeg come from the separately downloaded KDE runtime.
H.264, HEVC, VC-1 and E-AC-3 need Flathub's `org.freedesktop.Platform.codecs-extra` extension,
whose FFmpeg is GPL; with it installed, the running app loads those GPL libraries. Download the
`io.github.blub3.Huehuecoyotl.Sources.flatpak` asset for the corresponding source of libmpv,
libplacebo and libass, and the pinned build manifest.
See the [Flatpak source rebuild instructions](https://github.com/blub3/huehuecoyotl/blob/main/packaging/flatpak/README.md).

The Windows packages contain an LGPL build of libmpv and FFmpeg. `huehuecoyotl-windows-lgpl-sources.tar.xz`
holds the exact sources of that bundle and its build recipe.

The release also includes the third-party notices for each platform, Rust SBOM, Flatpak build
information and `SHA256SUMS` for the binary and source assets. Nothing is signed yet.
