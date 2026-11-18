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

**Windows.** Windows packages follow; see the project's README for the current state.

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

After you pick a folder, Huehuecoyotl asks what is in it: **Movies and shows**, **Music**
(from 2.0) or **Audiobooks** (from 2.5). A folder holds one kind; if a folder has more than
one, add its subfolders separately. For a network share, **Contains** in its settings does the same.

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

The key is checked once and stored in your system's keyring, so you enter it only once.
**Settings → Metadata** then shows **Key stored** and when TMDB last accepted it (the key itself
is never shown), with **Replace key**, **Test key** and **Remove key**. If your desktop has no
keyring (or it is locked), Settings says so: install or unlock one (for example GNOME Keyring
or KWallet) and restart the app; until then a key you enter works only until the app closes.
Metadata is downloaded in the language of the app (German or English), with English as the
fallback.

When the app is not sure which film a file is (two films with the same title, a missing year),
it puts the item on the **Review matches** list in Settings. Choose the right one there, or
use **Fix match** on the item's page to search by any title and year, or paste a TMDB or IMDb
link or id. Fix match shows what the item is matched to now; **Match automatically again** looks
it up once more. Before a change is applied it says what changes. Your choice is never changed by
later scans.

**A film matched as a series, or the reverse.** In Fix match, switch between **Movie** and **TV
show** and search. Choosing a show for a movie turns its file into an episode, using the season
and episode numbers in the file name (you can correct them); other films in the same folder are
offered too. Choosing a movie for a show or an episode turns the files into one film, with
several files as its versions. Watched state and resume points go with the files.

**Match a whole library again.** **Settings → Matching** lists your movies, TV shows, music and
audiobooks with how many are matched, need a choice, were not found or were chosen by you.
**Match again…** looks a library up once more: only the items without a sure match (the
default), or everything. Matches you chose yourself stay unless you untick **Keep the matches you
chose yourself**, and a match is only replaced by a sure one. One library runs at a time, in the
background; you can keep playing, stop it, or close the app: it goes on after the next start.
Music and audiobooks need **Music and book information online**, movies and shows a TMDB key.

## Browse and play

- **Home** shows *Continue watching* and *Recently added*.
- **Movies** and **Shows** are poster grids. **Sort** switches between A–Z, recently added
  and year; **All / Unwatched** hides what you have seen.
  **Saved** shows only what is saved on this computer.
- **Search** finds films and series by title (and music, see below).
- An item's page shows its description, files and, for series, seasons and episodes.
  **Resume** continues where you stopped; **Start over** begins again.
- On a series, every episode shows the technical summary of its file (for example
  "1080p · H.264 · AAC 2.0 · 2 subtitles"). Press Right on an episode to reach its own
  buttons: the **version** to play when there are several files, **Details** for the full
  technical details under the episode, and the **Save** menu. Back returns to the episode.

A film counts as watched once you have seen 92 % of it. A resume point is kept after
30 seconds. Your choice of audio and subtitle language is remembered per series.

Subtitles inside the video file are offered automatically, and so are subtitle files next to
the video with the same name (`Film.mkv` and `Film.en.srt` or `Film.de.forced.srt`).

## Music

From 2.0, folders you added as **Music** appear under **Music** in the navigation rail. From
2.6 a menu at the top switches between **Albums**, **Artists** and **Genres**, and **Search
music** finds albums, artists and tracks. Albums and artists come from the tags
in your files (title, artist, album, album artist, track and disc numbers); untagged files
are named after their folders and file names.

- An album's page shows its cover, artist, year, length, the format ("FLAC · 96 kHz ·
  24 bit") with a **Hi-Res** badge for lossless music above CD quality, and the tracks by
  disc. **Play** and **Shuffle** start it; the **Save** menu keeps a copy on this computer.
- An artist's page lists their albums, EPs, singles and compilations by year, and under
  **Appears on** the albums of others they play on.
- **Search** finds artists, albums and tracks too, in rows of their own. A track opens its
  album with the track selected.
- **Home** shows *Recently added albums* and *Recently played* once you have music.

**Finding albums** (from 2.6). Under Albums, the **Quality lens** shows only albums in
**Hi-Res**, **Lossless**, in **Several versions** or **Saved** on this computer; the lenses
combine, each shows how many albums it finds, and **All** clears them. **Genre** and **Decade**
narrow further, and the sort button orders by recently added, A–Z, year or most played. Every
cover shows the format that would play ("FLAC 24/96"), gold for Hi-Res. On the focused cover,
Play, Shuffle and More (Play next, Add to queue, Save) appear; P plays, Shift+P shuffles, Menu
opens More. Sorted A–Z, a letter rail on the right jumps to an initial: Right from the last
column reaches it. **Genres** shows your genres; choosing one shows its albums.

**Artists** (from 2.6) shows each artist as a round picture with the number of albums and Hi-Res
albums. The panel on the right follows the artist you are on: **Play all**, **Shuffle**, **Open
page**, the beginning of the biography and the albums. **Album artists** shows artists with
albums of their own; **Everyone credited** also those who only appear on tracks; **With Hi-Res
albums** narrows to artists with Hi-Res music. P plays all of an artist, Shift+P shuffles.

**Songs** (from 2.6) lists every song with its artist, album, length, quality and how often you
played it. Choose a column heading (or press S) to sort by it; Quality puts the best first.
**Hi-Res**, **Lossless** and **Never played** filter the list, and **Play these** or **Shuffle**
plays exactly the songs shown. Enter plays from a song; Menu offers Play next, Add to queue and
Go to album.

**Playlists** (from 2.6). The smart playlists fill themselves from your library: Hi-Res gems,
Most played, Recently added, Not played yet, Saved offline and Forgotten favourites (played often,
but not in the last six months). To make your own, choose **New playlist**, or press Menu on any
album or song (in the grids, the Songs list, an album's page or Now Playing's queue) and choose
**Add to playlist**. On a playlist's page Enter plays from a song, Ctrl+↑ and Ctrl+↓ move it,
Delete removes it, and **Rename** and **Delete playlist** change the playlist itself. A playlist
remembers which copy of a song you added; if that copy is gone, another version plays.
**Save offline** saves all its songs on this computer.

**Versions of an album** (from 2.1). When the same album is in your library more than once — as
FLAC and as MP3, on two shares, or as a remaster next to the original — it appears once, with a
small mark (for example "3×") on its cover. On the album page, the button under the format line
chooses the version: it shows the format, the size and where the copy is (for example
"FLAC · 24-bit/96 kHz · 1.1 GB · NAS" or "2011 Remaster · FLAC · 16-bit/44.1 kHz"). Play,
Shuffle, the track list, the Hi-Res badge and Save follow the version you choose. Without a choice
the album opens with the version you played last, else the one **Settings → Playback → Preferred
version** asks for: **Highest quality**, **Smallest file** or **Lossless only**. A version on a
share that cannot be reached is marked offline; if it is saved on this computer, it still plays.

A track that exists twice in one version (a replaced file) is marked with "2 versions"; press Menu
on it to choose which file plays. If two different albums were taken for one, choose the wrong
version and **This is a different album**: it becomes an album of its own and stays one.

**Playing music.** **Play** or **Shuffle** on an album starts it; Enter on a track plays the
album from that track. **Play next** (⤵) and **Add to queue** (+) put an album after the
current track or at the end; on a track, press Menu for the same. Music keeps playing while you
browse. The mini player at the bottom of the screen has previous, play or pause, next and
**Now Playing**; with the keyboard or a remote, press Down from the last entry of the menu on
the left to reach it.

**Now Playing** (from 2.6) fills the window in the colour of the album cover. Under the song,
the signal path shows how the music travels from the file through Huehuecoyotl to your speakers,
and says whether it arrives bit-perfect and, if not, why. The seek bar shows the song's waveform
(it appears a moment after a song first plays; until then a plain bar); click it or use Left and
Right to move. The volume slider is fixed at 100 % in bit-perfect mode. **Up next** on the right
reorders by dragging or with Ctrl+↑/↓; Menu offers Play now, Remove and Add to playlist. Back or
"‹ Music" returns to where you were.

**Full screen** (from 2.6; or F11 on Now Playing) is made for the sofa: a large cover, the title
in large type, the progress and big controls, with the next song in a card below. Enter plays
or pauses, Left and Right jump 10 seconds, Up reaches Previous, Next, Shuffle and Repeat, and
Down opens the queue. After five seconds without input the controls fade out and only the
cover, the title and the progress stay; the first key or click brings them back without doing
anything else. Back or F11 leaves full screen.

**Now Playing** shows the cover, the format, a seek bar (Left and Right jump 10 seconds),
shuffle, repeat (all, or the current track) and the queue: Enter plays an entry, Menu or Delete
offers to remove it, Ctrl+Up and Ctrl+Down move it. Space plays or pauses. Tracks of the same
format play without a gap between them. The queue and where you were come back after a
restart; press Play to go on. Starting a film or an episode pauses the music. The media keys
and your desktop's media controls work too.

**Artist images and biographies** (optional). Under **Settings → Metadata** you can enter
your own key for TheAudioDB and for Fanart.tv. Each key is checked with the service before it
is kept; a key the service does not accept is reported at once. With a key, artist pages show
the artist's image, logo, background and biography (in your language when there is one, else
in English), and albums without a cover get one. Without keys the music section works just the
same, with initials instead of artist images. TheAudioDB gives keys to supporters; for
Fanart.tv, request a key on your profile there.

**Bit-perfect music.** Under **Settings → Audio**, **Bit-perfect music** sends music to the
device as it is in the file: the app does not resample it, ReplayGain is off and the volume is
fixed at 100 % (turn the volume down on your amplifier or DAC). **Music output** chooses the
device for music; videos always use the system default.

- Devices marked **direct** are the sound card or DAC itself (ALSA). In bit-perfect mode the
  app uses them exclusively, so no other program can play at the same time, and the DAC gets
  each track at its own sample rate. Now Playing then shows **Bit-perfect**.
- Through PipeWire (the system default, and every device in the Flatpak, which has no direct
  device access) the system's mixer has the last word: Now Playing shows the rate that goes
  out and never says **Bit-perfect**. PipeWire switches the device's rate only for rates you
  allow, and only while no other program plays. To allow the usual music rates, create
  `~/.config/pipewire/pipewire.conf.d/10-rates.conf` with

  ```
  context.properties = {
      default.clock.allowed-rates = [ 44100 48000 88200 96000 176400 192000 ]
  }
  ```

  and restart PipeWire (`systemctl --user restart pipewire`).
- If the chosen device cannot be opened (another program holds it, or it was unplugged), music
  plays on the system default instead and Now Playing says so. If the device does not take a
  track's rate, the music is converted to one it takes, and Now Playing says that too.

**ReplayGain** (Settings → Playback) evens out loudness from the tags in your files: per track,
or per album so an album keeps its own dynamics.

Without a music folder the Music page explains how to add one.

**Match music again.** The **⋯** menu of an album, a song or an artist (and the album page's
**⋯** button) has **Match again** and **Search MusicBrainz…**. The search lists the exact
editions of an album with their track count, country, label and format; press Enter on one to
compare it with your files track by track, and Enter again to use it. Songs are compared by
length, artists of the same name by which of your albums are theirs. An artist without a match
takes the one its album or song names; its page says so, with **Not this artist?** to choose
another. Matches you choose are kept; your tags are never changed.

## Audiobooks

From 2.5, a folder added as **Audiobooks** becomes a library of books, authors, narrators and
series. Each book is either one file (an `.m4b` or an `.mp3` with chapters) or one folder of
files that play in a row; parts in `CD1`, `CD2` or `Disc 1` subfolders belong to the book above
them. Names like these work best:

```text
Audiobooks/Ursula K. Le Guin/A Wizard of Earthsea.m4b
Audiobooks/Ursula K. Le Guin/Earthsea/Book 2 - The Tombs of Atuan/01.mp3
Audiobooks/Ursula K. Le Guin/The Dispossessed/CD1/01.mp3
```

Huehuecoyotl takes what a book says about itself, in this order: a `metadata.opf` file (as
Calibre and many download tools write it), `desc.txt` and `reader.txt`, a `.nfo` file, the tags
of the first file (album = title, artist = author, composer = narrator, the movement fields =
series and number), and last the folder and file names. Chapters come from the file (M4B, MP3
with chapters); a book of many files gets one chapter per file. Covers come from the tags or a
`cover.jpg` or `folder.jpg` next to the files.

**Listening.** **Audiobooks** in the navigation rail has Books, Authors, Series and Narrators;
on Books, the filter button shows all, unread, in progress or finished books. A book's page has
**Play** (or **Resume**), **Mark as finished**, **Save** and the chapters; Enter on a chapter
plays from there. A series page has **Continue series**, which plays the next book you have not
finished. Home shows **Continue listening**: Enter resumes a book, I opens its page.

The player (**Now Playing** in the mini player) shows the chapter and the time left in it and in
the book. The seek bar covers the whole book, with a mark for each chapter. Left and Right on the
seek bar skip back 15 and forward 30 seconds, Page Up and Page Down change the chapter, Space
plays or pauses. **−** and **+** change the speed from 0.5× to 3.0× without changing the pitch;
each book keeps its own speed. **Sleep timer** steps through 5 to 120 minutes and "end of
chapter"; when it runs out, the sound fades and the book pauses where it stopped. A book
remembers its place every few seconds and when you pause or quit, also inside a book of many
files; its last 30 seconds count as finished. Starting a book pauses the music and starting music
pauses the book; the mini player and your keyboard's media keys follow whichever you started
last. **Settings → Playback** sets the speed for books without one of their own and how far the
skip buttons jump.

Online, Open Library fills in what your files leave out: a description, the year and a cover
(a book tagged by MusicBrainz Picard is looked up at MusicBrainz). What your files say is never
replaced. When Open Library has several books that could be yours, the book waits for you:
**Search Open Library…** on its page shows them to choose from. **Settings → Metadata → Music and
book information online** turns this off.

Books with copy protection (Audible `.aax` and `.aaxc`) are listed with a note but cannot be
played. Rescans read only books whose files changed, and a book is read by its headers, not
whole, also on a network share.

**Match a book or an author again.** A book's page has **Match again** and **Search Open
Library…** (title, author or ISBN, or a pasted Open Library link). Before you choose, the search
shows field by field what the match would add and that your own metadata stays. An author's page
shows the photo and biography from Open Library once the author is matched, which happens on its
own when one of their books is matched; **Search Open Library…** (or **Not this author?**) tells
authors of the same name apart by which of your books are theirs.

## Watch without the network: Save and Save as…

- **Save** in the Save menu of a film, an episode or an episode in a series' list
  (**Save season**, **Save all** on a series) keeps a copy of a file from a network share on
  this computer. It then plays from the copy, also when the server is off; resume points and
  watched marks stay the same. While it copies, the menu pauses, resumes or cancels it.
  Copying continues after the network drops or the app restarts. A cover or episode with a ⤓
  mark is saved. The Menu key opens the Save menu of the selected episode.
- **Delete saved copy** removes only the copy on this computer; the file on the server stays.
  When the file on the server changes, the copy is marked outdated and offers **Save again**.
- **Downloads** in the navigation lists copies in progress, failed and done, and the space
  they use. **Settings → Storage** chooses the folder for new copies and deletes all of them.
- **Save as…** copies a film or episode to a file you choose (a season or series into a
  folder), for example on a USB drive. The app does not keep track of these copies and
  never deletes them.
  In the Flatpak version, a single file picked in the system's dialog is copied without its
  subtitle files (the dialog grants that one file only); a season copied into a folder
  includes them.

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
| Right | Move into an episode's buttons (version, Details, Save menu); Back returns |
| Menu | Open the selected episode's Save menu |

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

For audiobooks see [Audiobooks](#audiobooks).

## Privacy

- No account, no server of ours, no telemetry, no automatic updates of its own.
- The app talks to TMDB only when you have entered a key, and only to look up titles and
  download artwork.
- For music folders (from 2.0), album and artist names are sent to MusicBrainz to find
  release types, dates and ids, and album covers come from the Cover Art Archive. For
  audiobook folders (from 2.5), titles, authors and ISBNs are sent to Open Library for
  descriptions, years and covers (books tagged with MusicBrainz ids go to MusicBrainz instead).
  Turn both off under **Settings → Metadata → Music and book information online**;
  information then comes from your files, their tags and metadata files only, and nothing is
  sent.
- Optionally, with your own keys (see *Music*), MusicBrainz ids of your artists and albums
  are sent to TheAudioDB and Fanart.tv for artist images, biographies and album art. Without
  a key nothing is sent to either.
- Network share passwords, your TMDB key and your TheAudioDB and Fanart.tv keys stay in your
  system's keyring.
- Discovery listens for servers that announce themselves on your local network (mDNS); it
  does not scan the network.

## Where your data lives

| | Linux (Flatpak) | Linux (other) | Windows |
| --- | --- | --- | --- |
| Library, artwork, log | `~/.var/app/io.github.blub3.Huehuecoyotl/data/coyotl` | `~/.local/share/coyotl` | `%APPDATA%\coyotl` |

Deleting this folder resets the library; your video files are never changed or deleted.
Saved copies are in its `Saved` folder unless you chose another one under Settings → Storage.
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
