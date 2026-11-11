# Huehuecoyotl user guide

Huehuecoyotl plays the films and series you already have, from folders on your computer or on
a network share (a NAS, a home server, another PC). It builds a library with posters and
descriptions, remembers where you stopped, and works with a mouse, a keyboard or a remote.

- [Install](#install)
- [First start](#first-start)
- [Add a folder on this computer](#add-a-folder-on-this-computer)
- [Add a network share](#add-a-network-share)
- [Posters and descriptions (TMDB)](#posters-and-descriptions-tmdb)
- [Browse and play](#browse-and-play)
- [Keyboard and remote](#keyboard-and-remote)
- [How files should be named](#how-files-should-be-named)
- [Privacy](#privacy)
- [Where your data lives](#where-your-data-lives)
- [Troubleshooting](#troubleshooting)

## Install

**Linux.** Install the Flatpak from Flathub, or a `.flatpak` bundle from the releases page:

```bash
flatpak install --user io.github.blub3.Huehuecoyotl.flatpak
```

Then start Huehuecoyotl from your application menu, or run `flatpak run io.github.blub3.Huehuecoyotl`.

### Windows

Windows 10 (version 2004 or later) and Windows 11, 64-bit. Each release has two Windows
packages. Neither is signed yet, so Windows asks before running them.

**Portable zip (simplest).** Unzip `Huehuecoyotl-<version>-windows-x64.zip` anywhere, for
example into your Documents, and start `Huehuecoyotl\bin\coyotl.exe`. Nothing is installed.
SmartScreen warns the first time: choose **More info → Run anyway**. To update, replace the
folder; your library stays where it is (see below). To remove it, delete the folder.

**MSIX (Start menu, file associations, uninstall in Settings).** Open PowerShell in the folder
with the download and run:

```powershell
Add-AppxPackage -Path .\Huehuecoyotl-<version>-windows-x64.msix -AllowUnsigned
```

Huehuecoyotl then appears in the Start menu and under "Open with" for video files. Remove it in
Settings → Apps like any other app. If PowerShell refuses the unsigned package, use the portable
zip; signed packages follow.

## First start

The Home screen shows a short setup guide while the library is empty. You need two things:

1. **A library folder**: a folder on this computer or on a network share that contains your
   films and series.
2. **Optionally, a TMDB key** for posters and descriptions. Without it everything still plays,
   and titles come from the file names.

## Add a folder on this computer

Open **Sources** and choose **Add folder**. Pick the folder that contains your films or series
and choose **Add this folder**. Huehuecoyotl scans it straight away and again whenever you
choose **Rescan all**. Subfolders are included.

A folder on a disk that is not connected (an unplugged USB drive) is shown as unreachable;
nothing is removed from the library until the folder is back and a scan finds the files gone.

## Add a network share

Open **Sources** and choose **Add network share**.

1. Servers that announce themselves on your network (most NAS systems) appear at the top.
   Choose one to fill in the form, or enter the details yourself.
2. Choose the protocol: **SMB** (Windows shares, Samba, most NAS), **NFS**, **WebDAV**,
   **FTP** or **SFTP** (any server you can reach with SSH). Your NAS's settings tell you which
   ones it offers; SMB is the usual choice.
3. Enter the server name or address, the share name (for SMB and NFS, for example `media`),
   and your user name and password if the share needs them.
4. Choose **Connect**. If it works, the share's folders appear. Open the folder with your
   films or series and choose **Add this folder**, or **Add the whole share**.

The password is kept in your system's keyring (on Linux the Secret Service, usually GNOME
Keyring or KWallet), never in the library database.

If a server is switched off, its library shows **offline** under Sources and its films stay in
the library. Huehuecoyotl checks again every minute and rescans when the server is back.
Pressing Play always tries the server again.

Notes for some servers:

- **NFS** shares must allow connections from unprivileged ports (the `insecure` export option),
  because the app does not run as root.
- **WebDAV** over HTTPS: tick **HTTPS**.
- **SFTP**: log in with your password, or with an SSH key: enter the key file (for example
  `~/.ssh/id_ed25519`) and, if the key has one, its passphrase in the password field. Ed25519
  and ECDSA keys work; RSA keys do not. After **Connect** the server's key fingerprint is shown:
  compare it with the one on the server (`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`)
  before adding the library. The app remembers that key and refuses the server if it later
  presents a different one; remove and re-add the library if the server's key really changed.
  The base folder is optional (empty is the server's starting folder, usually your home).
- Windows PCs that do not announce themselves do not appear in the list; enter their name or
  address by hand.

## Posters and descriptions (TMDB)

Posters, descriptions, ratings and episode titles come from [The Movie Database
(TMDB)](https://www.themoviedb.org). TMDB asks every user for their own free key:

1. Create an account at themoviedb.org.
2. Open **Settings → API** on the TMDB website and request a key for personal use.
3. In Huehuecoyotl, open **Settings**, paste the **API Key** or the **API Read Access Token**,
   and choose **Check and save**.

The key is checked once and stored in your system's keyring. Metadata is downloaded in the
language of the app (German or English), with English as the fallback.

When the app is not sure which film a file is (two films with the same title, a missing year),
it puts the item on the **Review matches** list in Settings. Choose the right one there, or
use **Fix match** on the item's page to search by any title and year. Your choice is never
changed by later scans.

## Browse and play

- **Home** shows *Continue watching* and *Recently added*.
- **Movies** and **Shows** are poster grids. **Sort** switches between A–Z, recently added
  and year; **All / Unwatched** hides what you have seen.
- **Search** finds films and series by title.
- An item's page shows its description, files and, for series, seasons and episodes.
  **Resume** continues where you stopped; **Start over** begins again.

A film counts as watched once you have seen 92 % of it. A resume point is kept after
30 seconds. Your choice of audio and subtitle language is remembered per series.

Subtitles inside the video file are offered automatically, and so are subtitle files next to
the video with the same name (`Film.mkv` and `Film.en.srt` or `Film.de.forced.srt`).

## Keyboard and remote

Everything works with the arrow keys, Enter and Escape (Back). The navigation rail is on the
left: press Left on the first column of any page to reach it.

| Anywhere | |
| --- | --- |
| Arrow keys | Move |
| Enter | Open or activate |
| Escape, Backspace, Back | Go back |
| F11 | Fullscreen on or off |

| In a grid or episode list | |
| --- | --- |
| P or Play | Play the selected item |
| W | Mark as watched or unwatched |
| I | Open an episode's page |

| During playback | |
| --- | --- |
| Space, K, Enter, Play/Pause | Pause or play |
| Left / Right, J / L | Back or forward 10 seconds |
| Up / Down | Show the controls |
| Page Up / Page Down, Previous / Next | Previous or next chapter |
| Home | Start over |
| A | Next audio track |
| S or V | Next subtitle track |
| + / − | Volume |
| M | Mute |
| [ / ] | Slower or faster |
| , / . | One frame back or forward (while paused) |
| F | Fullscreen |
| Escape | Leave the player (your position is saved) |

On Linux the media keys of your keyboard, headset or desktop (the media controls in the panel,
KDE Connect, and so on) control playback too.

## How files should be named

Huehuecoyotl reads the title, year, season and episode from file and folder names. Names like
these work best:

```text
Movies/Alien (1979)/Alien (1979).mkv
Movies/Blade Runner 1982 1080p BluRay.mkv
Shows/Firefly/Season 1/Firefly S01E01.mkv
Shows/Firefly/Firefly 1x02 The Train Job.mkv
```

Release-group noise (resolution, codec, group names) is ignored. Files named `sample` and
folders called `Extras` or `Featurettes` are kept out of the library.

## Privacy

- No account, no server of ours, no telemetry, no automatic updates of its own.
- The app talks to TMDB only when you have entered a key, and only to look up titles and
  download artwork.
- Network share passwords and your TMDB key stay in your system's keyring.
- Discovery listens for servers that announce themselves on your local network (mDNS); it
  does not scan the network.

## Where your data lives

| | Linux (Flatpak) | Linux (other) | Windows |
| --- | --- | --- | --- |
| Library, artwork, log | `~/.var/app/io.github.blub3.Huehuecoyotl/data/coyotl` | `~/.local/share/coyotl` | `%APPDATA%\coyotl` (portable); the MSIX keeps it in `%LOCALAPPDATA%\Packages\io.github.blub3.Huehuecoyotl_…\LocalCache\Roaming\coyotl` and removes it on uninstall |

Deleting this folder resets the library; your video files are never changed or deleted.
Passwords and the TMDB key are in the system keyring under the service name `coyotl`.

## Troubleshooting

**A share does not connect.** Check the server name and share name, try the server's IP
address instead of its name, and make sure the protocol is enabled on the NAS. For NFS, see the
note on the `insecure` option above.

**A film was matched to the wrong title.** Open it and choose **Fix match**.

**Video stutters on a network share.** Over Wi-Fi, 4K films can need more bandwidth than the
connection has. A wired connection or SMB instead of WebDAV usually helps.

**Something else.** Open **Settings → Troubleshooting → Export diagnostics**. This saves a
text file to your Downloads folder with the app's log and version information. Server names,
user names, addresses, keys and your home folder are removed from it. Attach it to a
[bug report](https://github.com/blub3/huehuecoyotl/issues).

To start the app from a terminal with more detailed logging:

```bash
COYOTL_LOG=debug COYOTL_LOG_STDERR=1 flatpak run io.github.blub3.Huehuecoyotl
```
