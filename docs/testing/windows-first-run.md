# Windows first run – test checklist

The Windows build has been compiled by CI but never run. This checklist is for the first run
on a real Windows PC. Note what happens at each step; a failure early on usually explains the
rest.

The portable package is a folder: unzip `Huehuecoyotl-<version>-windows-x64.zip` anywhere and run
`Huehuecoyotl\bin\coyotl.exe`. Nothing is installed; the Visual C++ runtime is included. Windows SmartScreen will warn because the
build is unsigned: choose **More info → Run anyway**.

Use test clips you own; the app's data goes to `%APPDATA%\coyotl` (delete it to start over).

## 1. Start

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| 1 | Start `coyotl.exe` | The window opens with the Home screen | |
| 2 | MSIX instead: `Add-AppxPackage -Path .\Huehuecoyotl-<version>-windows-x64.msix -AllowUnsigned` in PowerShell, then start it from the Start menu | Installs; Start menu entry with the logo; "Open with" lists it for video files | |
| 3 | Move around with the arrow keys, Enter, Escape | Everything reachable by keyboard | |

## 2. Playback (embedded mode, OpenGL)

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| 4 | Sources → Add folder → a folder with films | Scan finds them; Movies shows them | |
| 5 | Play a 1080p H.264 file | Plays, sound, the top right of the controls names the decoder (`d3d11va` or `nvdec`) | |
| 6 | Play a 4K HEVC 10-bit file | Smooth; decoder as above | |
| 7 | Play an HDR10 file with Windows HDR **off** | Plays tone-mapped (looks normal, not washed out) | |
| 8 | Same with Windows HDR **on** | Plays (still tone-mapped: HDR passthrough comes with direct mode) | |
| 9 | Seek, pause, subtitles, audio tracks, F11 fullscreen | All work | |
| 10 | Close the player, play again | Resumes where it stopped | |

## 3. Library and network

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| 11 | Settings → enter the TMDB key | Saved (Windows Credential Manager, entry `coyotl`) | |
| 12 | Sources → Add network share → your NAS (SMB) | Found by discovery or by address; folder picker works; scan finds files | |
| 13 | Play a film from the share, seek around | Plays without stutter | |

## 4. When something fails

Settings → Troubleshooting → **Export diagnostics** writes a file to your Downloads folder with
the log and version information (server names, addresses, keys and your user folder removed).
If the app does not start at all, the log is in `%APPDATA%\coyotl\logs\coyotl.log`.
