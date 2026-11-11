**Linux:** install the Flatpak bundle (it pulls the KDE 6.11 runtime from Flathub), then start Huehuecoyotl from the application menu:

```bash
flatpak install --user io.github.blub3.Huehuecoyotl.flatpak
```

**Windows 10 and 11 (x64):** two ways, neither is signed yet, so Windows warns when you open them.

- **Portable (simplest):** unzip `Huehuecoyotl-…-windows-x64.zip` anywhere and run `Huehuecoyotl\bin\coyotl.exe`. Nothing is installed; SmartScreen asks once (*More info → Run anyway*).
- **MSIX (Start menu entry, file associations, clean uninstall):** in PowerShell, `Add-AppxPackage -Path .\Huehuecoyotl-…-windows-x64.msix -AllowUnsigned`. See the [user guide](https://github.com/blub3/huehuecoyotl/blob/main/docs/user/README.md#windows) for details.

The [user guide](https://github.com/blub3/huehuecoyotl/blob/main/docs/user/README.md) covers libraries, network shares and the TMDB key; [CHANGELOG.md](https://github.com/blub3/huehuecoyotl/blob/main/CHANGELOG.md) lists the changes and known limitations.

Assets: the Flatpak bundle, the Windows zip and MSIX, the third-party notices for each platform, a CycloneDX SBOM of the Rust crates, the libmpv source revisions, and `SHA256SUMS`. Nothing is signed yet.
