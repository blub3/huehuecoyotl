//! The names of managed copies (3.0, issue #152, SPEC 7 "Managed library names"): pure functions
//! from an item's canonical metadata to a path inside the library. The same input always gives
//! the same name; a missing field is left out together with its separator.

/// The longest file or folder name, in bytes (most file systems allow 255).
const MAX_NAME: usize = 240;

/// One file or folder name that every supported file system accepts: `/ \ : * ? " < > |` become
/// `-`, control characters go, runs of spaces become one, and leading spaces and trailing dots
/// and spaces go.
pub fn clean(s: &str) -> String {
    let mapped: String = s
        .chars()
        // Tabs and line breaks separate words; other control characters go.
        .filter(|c| c.is_whitespace() || !c.is_control())
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    let spaced = mapped.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut t = spaced.trim_end_matches(['.', ' ']).trim_start().to_string();
    // Names Windows reserves, also on a share a Windows machine reads.
    let stem = t.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        t.push('_');
    }
    t
}

fn limit(mut s: String) -> String {
    if s.len() > MAX_NAME {
        let mut end = MAX_NAME;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s = s.trim_end_matches(['.', ' ', '-']).to_string();
    }
    s
}

/// `stem.ext`, shortening the stem only: extensions stay.
fn file(stem: &str, ext: &str) -> String {
    let ext = ext.trim_start_matches('.');
    if ext.is_empty() {
        return limit(clean(stem));
    }
    let room = MAX_NAME.saturating_sub(ext.len() + 1);
    let mut s = clean(stem);
    if s.len() > room {
        let mut end = room;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s = s.trim_end_matches(['.', ' ', '-']).to_string();
    }
    format!("{s}.{ext}")
}

/// The parts that are there, joined.
fn words(parts: &[&str], sep: &str) -> String {
    parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(sep)
}

fn titled(title: &str, fallback: &str) -> String {
    let t = title.trim();
    if t.is_empty() {
        fallback.into()
    } else {
        t.into()
    }
}

fn with_year(title: &str, year: Option<i32>) -> String {
    match year {
        Some(y) => format!("{title} ({y})"),
        None => title.to_string(),
    }
}

/// `{imdb-tt…}` when the IMDb ID is known, else `{tmdb-…}`.
fn id_tag(imdb: &str, tmdb: Option<i64>) -> String {
    let imdb = imdb.trim();
    if !imdb.is_empty() {
        format!("{{imdb-{imdb}}}")
    } else {
        tmdb.map(|t| format!("{{tmdb-{t}}}")).unwrap_or_default()
    }
}

/// What the file holds, for the name of a video.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoTech {
    pub resolution: String,
    pub hdr: String,
    pub vcodec: String,
    pub bitdepth: String,
    pub acodec: String,
    pub atmos: bool,
    pub channels: String,
}

impl VideoTech {
    fn audio(&self) -> String {
        words(
            &[
                &self.acodec,
                if self.atmos { "Atmos" } else { "" },
                &self.channels,
            ],
            " ",
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Movie {
    pub title: String,
    pub year: Option<i32>,
    pub imdb: String,
    pub tmdb: Option<i64>,
    pub edition: String,
    pub tech: VideoTech,
    pub part: Option<u32>,
    pub ext: String,
}

/// `Movies/{title} ({year}) {id}/{title} ({year}) {id} - {edition} - {video} - {audio} - {part}.{ext}`.
/// Two versions of a film share its folder.
pub fn movie(m: &Movie) -> String {
    let base = words(
        &[
            &with_year(&titled(&m.title, "Unknown movie"), m.year),
            &id_tag(&m.imdb, m.tmdb),
        ],
        " ",
    );
    let t = &m.tech;
    let video = words(&[&t.resolution, &t.hdr, &t.vcodec, &t.bitdepth], " ");
    let part = m.part.map(|p| format!("part{p}")).unwrap_or_default();
    let stem = words(&[&base, &m.edition, &video, &t.audio(), &part], " - ");
    format!("Movies/{}/{}", limit(clean(&base)), file(&stem, &m.ext))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Episode {
    pub show: String,
    pub first_air_year: Option<i32>,
    pub show_imdb: String,
    pub show_tmdb: Option<i64>,
    pub season: u32,
    pub episode: u32,
    /// A file holding several episodes: the last one.
    pub last_episode: Option<u32>,
    pub title: String,
    pub imdb: String,
    pub tmdb: Option<i64>,
    pub tech: VideoTech,
    pub ext: String,
}

/// `TV Shows/{show} ({year}) {id}/Season {nn}/{show} ({year}) - SnnEnn - {title} {id} - {video} - {audio}.{ext}`.
/// Specials are season 0, so they land in `Season 00`.
pub fn episode(e: &Episode) -> String {
    let show = with_year(&titled(&e.show, "Unknown show"), e.first_air_year);
    let folder = words(&[&show, &id_tag(&e.show_imdb, e.show_tmdb)], " ");
    let mut number = format!("S{:02}E{:02}", e.season, e.episode);
    if let Some(l) = e.last_episode.filter(|l| *l > e.episode) {
        number.push_str(&format!("-E{l:02}"));
    }
    let title = words(&[&e.title, &id_tag(&e.imdb, e.tmdb)], " ");
    let t = &e.tech;
    let video = words(&[&t.resolution, &t.hdr, &t.vcodec], " ");
    let stem = words(&[&show, &number, &title, &video, &t.audio()], " - ");
    format!(
        "TV Shows/{}/Season {:02}/{}",
        limit(clean(&folder)),
        e.season,
        file(&stem, &e.ext)
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Album {
    pub album_artist: String,
    pub title: String,
    pub year: Option<i32>,
    /// The artist's or release's disambiguation at MusicBrainz.
    pub disambiguation: String,
    /// "Vinyl", "Deluxe Edition".
    pub edition: String,
    pub release_id: String,
}

/// The album's folder: its plain name, and when that is taken by another release, with its
/// disambiguation, then its edition, then both, and only then its MBID. `taken` is asked with
/// the whole relative path.
pub fn album_folder(a: &Album, taken: &dyn Fn(&str) -> bool) -> String {
    let artist = limit(clean(&titled(&a.album_artist, "Unknown artist")));
    let base = with_year(&titled(&a.title, "Unknown album"), a.year);
    let path = |label: &str| {
        let name = if label.is_empty() {
            base.clone()
        } else {
            format!("{base} [{label}]")
        };
        format!("Music/{artist}/{}", limit(clean(&name)))
    };
    let both = words(&[&a.disambiguation, &a.edition], ", ");
    let plain = path("");
    if !taken(&plain) {
        return plain;
    }
    for label in [a.disambiguation.trim(), a.edition.trim(), both.as_str()] {
        if !label.is_empty() {
            let p = path(label);
            if !taken(&p) {
                return p;
            }
        }
    }
    path(a.release_id.trim())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub disc: u32,
    pub discs: u32,
    pub number: Option<u32>,
    pub title: String,
    /// The song's own artist: named in the file on a compilation.
    pub artist: String,
    pub compilation: bool,
    pub ext: String,
}

/// `{track:02} - {title}.{ext}`; several discs `{disc:02}-{track:02}`; a compilation also names
/// the song's artist.
pub fn track_file(t: &Track) -> String {
    let number = match (t.number, t.discs > 1) {
        (Some(n), true) => format!("{:02}-{n:02}", t.disc.max(1)),
        (Some(n), false) => format!("{n:02}"),
        (None, true) => format!("{:02}", t.disc.max(1)),
        (None, false) => String::new(),
    };
    let artist = if t.compilation { t.artist.as_str() } else { "" };
    let title = titled(&t.title, "Unknown track");
    file(&words(&[&number, artist, &title], " - "), &t.ext)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Book {
    pub author: String,
    pub title: String,
    pub year: Option<i32>,
    pub series: String,
    /// "3", "2.5".
    pub series_number: String,
    pub narrator: String,
    pub edition: String,
}

/// `Audiobooks/{author}/{title} ({year})`; in a series
/// `Audiobooks/{author}/{series}/{number} - {title} ({year})`. The narrator, then the edition,
/// separate otherwise equal folders.
pub fn book_folder(b: &Book, taken: &dyn Fn(&str) -> bool) -> String {
    let author = limit(clean(&titled(&b.author, "Unknown author")));
    let title = with_year(&titled(&b.title, "Unknown book"), b.year);
    let name = words(&[&b.series_number, &title], " - ");
    let parent = if b.series.trim().is_empty() {
        format!("Audiobooks/{author}")
    } else {
        format!("Audiobooks/{author}/{}", limit(clean(&b.series)))
    };
    let path = |label: &str| {
        let n = if label.is_empty() {
            name.clone()
        } else {
            format!("{name} [{label}]")
        };
        format!("{parent}/{}", limit(clean(&n)))
    };
    let plain = path("");
    if !taken(&plain) {
        return plain;
    }
    let both = words(&[&b.narrator, &b.edition], ", ");
    for label in [b.narrator.trim(), b.edition.trim()] {
        if !label.is_empty() {
            let p = path(label);
            if !taken(&p) {
                return p;
            }
        }
    }
    path(if both.is_empty() { "2" } else { &both })
}

/// A book's file: one file stays one file under the book's title; a book split into chapters
/// gets `{chapter:02} - {chapter_title}.{ext}`.
pub fn book_file(b: &Book, chapter: Option<(u32, &str)>, ext: &str) -> String {
    match chapter {
        None => file(&titled(&b.title, "Unknown book"), ext),
        Some((n, t)) => file(&words(&[&format!("{n:02}"), t], " - "), ext),
    }
}

/// An example of a copy's name per media type, for Settings → Library.
pub fn example(kind: &str) -> String {
    let free = |_: &str| false;
    match kind {
        "movies" => movie(&Movie {
            title: "Alien".into(),
            year: Some(1979),
            imdb: "tt0078748".into(),
            tech: VideoTech {
                resolution: "1080p".into(),
                vcodec: "H264".into(),
                acodec: "DTS".into(),
                channels: "5.1".into(),
                ..Default::default()
            },
            ext: "mkv".into(),
            ..Default::default()
        }),
        "shows" => episode(&Episode {
            show: "Firefly".into(),
            first_air_year: Some(2002),
            show_imdb: "tt0303461".into(),
            season: 1,
            episode: 1,
            title: "Serenity".into(),
            tech: VideoTech {
                resolution: "1080p".into(),
                vcodec: "H264".into(),
                acodec: "AC3".into(),
                channels: "5.1".into(),
                ..Default::default()
            },
            ext: "mkv".into(),
            ..Default::default()
        }),
        "audiobooks" => {
            let b = Book {
                author: "Jane Austen".into(),
                title: "Pride and Prejudice".into(),
                year: Some(1813),
                ..Default::default()
            };
            format!("{}/{}", book_folder(&b, &free), book_file(&b, None, "m4b"))
        }
        _ => {
            let a = Album {
                album_artist: "Rosalía".into(),
                title: "MOTOMAMI".into(),
                year: Some(2022),
                ..Default::default()
            };
            let t = Track {
                disc: 1,
                discs: 1,
                number: Some(1),
                title: "SAOKO".into(),
                ext: "flac".into(),
                ..Default::default()
            };
            format!("{}/{}", album_folder(&a, &free), track_file(&t))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREE: &dyn Fn(&str) -> bool = &|_| false;

    fn tech() -> VideoTech {
        VideoTech {
            resolution: "2160p".into(),
            hdr: "HDR10".into(),
            vcodec: "HEVC".into(),
            bitdepth: "10bit".into(),
            acodec: "TrueHD".into(),
            atmos: true,
            channels: "7.1".into(),
        }
    }

    #[test]
    fn movies_with_and_without_an_imdb_id() {
        let m = Movie {
            title: "Blade Runner".into(),
            year: Some(1982),
            imdb: "tt0083658".into(),
            tmdb: Some(78),
            edition: "Final Cut".into(),
            tech: tech(),
            part: Some(2),
            ext: "mkv".into(),
        };
        assert_eq!(
            movie(&m),
            "Movies/Blade Runner (1982) {imdb-tt0083658}/Blade Runner (1982) {imdb-tt0083658} - Final Cut - 2160p HDR10 HEVC 10bit - TrueHD Atmos 7.1 - part2.mkv"
        );
        let plain = Movie {
            title: "Alien".into(),
            year: Some(1979),
            tmdb: Some(348),
            ext: "mp4".into(),
            ..Default::default()
        };
        assert_eq!(
            movie(&plain),
            "Movies/Alien (1979) {tmdb-348}/Alien (1979) {tmdb-348}.mp4"
        );
    }

    #[test]
    fn versions_of_a_film_share_its_folder() {
        let a = Movie {
            title: "X".into(),
            year: Some(2000),
            tmdb: Some(1),
            ext: "mkv".into(),
            ..Default::default()
        };
        let mut b = a.clone();
        b.tech.resolution = "720p".into();
        let dir = |s: String| s.rsplit_once('/').unwrap().0.to_string();
        assert_eq!(dir(movie(&a)), dir(movie(&b)));
        assert_ne!(movie(&a), movie(&b));
    }

    #[test]
    fn a_special_lands_in_season_00() {
        let e = Episode {
            show: "Doctor Who".into(),
            first_air_year: Some(2005),
            show_imdb: "tt0436992".into(),
            season: 0,
            episode: 3,
            title: "The Runaway Bride".into(),
            imdb: "tt0957301".into(),
            tech: VideoTech {
                resolution: "1080p".into(),
                hdr: "HDR10".into(),
                vcodec: "HEVC".into(),
                bitdepth: "10bit".into(),
                acodec: "AAC".into(),
                channels: "2.0".into(),
                ..Default::default()
            },
            ext: "mkv".into(),
            ..Default::default()
        };
        assert_eq!(
            episode(&e),
            "TV Shows/Doctor Who (2005) {imdb-tt0436992}/Season 00/Doctor Who (2005) - S00E03 - The Runaway Bride {imdb-tt0957301} - 1080p HDR10 HEVC - AAC 2.0.mkv"
        );
    }

    #[test]
    fn several_episodes_in_one_file() {
        let e = Episode {
            show: "S".into(),
            season: 2,
            episode: 1,
            last_episode: Some(2),
            ext: "mkv".into(),
            ..Default::default()
        };
        assert_eq!(episode(&e), "TV Shows/S/Season 02/S - S02E01-E02.mkv");
    }

    #[test]
    fn a_multi_disc_compilation_names_disc_and_artist() {
        let t = Track {
            disc: 2,
            discs: 2,
            number: Some(7),
            title: "SAOKO".into(),
            artist: "Rosalía".into(),
            compilation: true,
            ext: "flac".into(),
        };
        assert_eq!(track_file(&t), "02-07 - Rosalía - SAOKO.flac");
        let single = Track {
            disc: 1,
            discs: 1,
            number: Some(1),
            ..t.clone()
        };
        assert_eq!(track_file(&single), "01 - Rosalía - SAOKO.flac");
        let plain = Track {
            compilation: false,
            ..single
        };
        assert_eq!(track_file(&plain), "01 - SAOKO.flac");
    }

    #[test]
    fn colliding_albums_get_a_label_and_last_the_mbid() {
        let a = Album {
            album_artist: "Rosalía".into(),
            title: "El mal querer".into(),
            year: Some(2018),
            disambiguation: String::new(),
            edition: "Vinyl".into(),
            release_id: "abc-1".into(),
        };
        let plain = "Music/Rosalía/El mal querer (2018)";
        assert_eq!(album_folder(&a, FREE), plain);
        let one = |p: &str| p == plain;
        assert_eq!(
            album_folder(&a, &one),
            "Music/Rosalía/El mal querer (2018) [Vinyl]"
        );
        let both = |p: &str| p == plain || p.ends_with("[Vinyl]");
        assert_eq!(
            album_folder(&a, &both),
            "Music/Rosalía/El mal querer (2018) [abc-1]"
        );
        let d = Album {
            disambiguation: "Japan".into(),
            ..a
        };
        assert_eq!(
            album_folder(&d, &one),
            "Music/Rosalía/El mal querer (2018) [Japan]"
        );
    }

    #[test]
    fn audiobooks_in_a_series_and_as_one_file() {
        let b = Book {
            author: "Terry Pratchett".into(),
            title: "Mort".into(),
            year: Some(1987),
            series: "Discworld".into(),
            series_number: "4".into(),
            narrator: "Nigel Planer".into(),
            ..Default::default()
        };
        assert_eq!(
            book_folder(&b, FREE),
            "Audiobooks/Terry Pratchett/Discworld/4 - Mort (1987)"
        );
        assert_eq!(book_file(&b, None, "m4b"), "Mort.m4b");
        assert_eq!(
            book_file(&b, Some((3, "Death's Domain")), "mp3"),
            "03 - Death's Domain.mp3"
        );
        let plain = "Audiobooks/Terry Pratchett/Discworld/4 - Mort (1987)";
        let taken = |p: &str| p == plain;
        assert!(book_folder(&b, &taken).ends_with("[Nigel Planer]"));
    }

    #[test]
    fn forbidden_characters_and_edges() {
        assert_eq!(
            clean(r#"AC/DC: Back\In*Black?"<>|"#),
            "AC-DC- Back-In-Black-----"
        );
        assert_eq!(clean("  Trailing dots...  "), "Trailing dots");
        assert_eq!(clean("two  spaces\tand\nlines"), "two spaces and lines");
        assert_eq!(clean("CON"), "CON_");
        assert_eq!(clean("com1.txt"), "com1.txt_");
        assert_eq!(clean("Console"), "Console");
        let long = "é".repeat(400);
        let name = file(&long, "flac");
        assert!(
            name.len() <= MAX_NAME && name.ends_with(".flac"),
            "{}",
            name.len()
        );
    }

    #[test]
    fn missing_fields_leave_no_separator() {
        let m = Movie {
            title: "Only".into(),
            ext: "mkv".into(),
            ..Default::default()
        };
        assert_eq!(movie(&m), "Movies/Only/Only.mkv");
        let t = Track {
            title: "Song".into(),
            ext: "mp3".into(),
            discs: 1,
            ..Default::default()
        };
        assert_eq!(track_file(&t), "Song.mp3");
        let a = Album {
            title: "A".into(),
            ..Default::default()
        };
        assert_eq!(album_folder(&a, FREE), "Music/Unknown artist/A");
    }

    #[test]
    fn the_same_item_gets_the_same_name() {
        for k in ["movies", "shows", "music", "audiobooks"] {
            assert_eq!(example(k), example(k));
        }
        assert_eq!(
            example("music"),
            "Music/Rosalía/MOTOMAMI (2022)/01 - SAOKO.flac"
        );
    }
}
