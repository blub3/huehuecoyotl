Linux release. Install the Flatpak bundle (it pulls the KDE 6.11 runtime from Flathub):

```bash
flatpak install --user io.github.blub3.Huehuecoyotl.flatpak
```

Then start Huehuecoyotl from the application menu. The [user guide](https://github.com/blub3/huehuecoyotl/blob/main/docs/user/README.md) covers libraries, network shares and the TMDB key; [CHANGELOG.md](https://github.com/blub3/huehuecoyotl/blob/main/CHANGELOG.md) lists the changes and known limitations.

The Flatpak uses FFmpeg under LGPL 2.1 or later. Download the
`io.github.blub3.Huehuecoyotl.Sources.flatpak` asset for the corresponding source of FFmpeg,
libmpv and libplacebo, their bundled dependencies, and the pinned build manifest.
See the [Flatpak source rebuild instructions](https://github.com/blub3/huehuecoyotl/blob/main/packaging/flatpak/README.md).
Qt comes from the separately downloaded KDE runtime.

The release also includes the third-party notices, Rust SBOM, Flatpak build information and
`SHA256SUMS` for the binary and source assets. The bundles are unsigned.
