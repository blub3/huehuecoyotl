//! File and folder name parsing (SPEC 7, scan step 2). Pure functions, no I/O.
//!
//! The input is a path relative to a library root. The parser looks at the file name first and
//! falls back to the folders above it for the show name, the season and the movie year.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Movie,
    Episode,
    /// Trailers, samples, featurettes: never become items.
    Extra,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub kind: Kind,
    pub title: String,
    pub year: Option<u16>,
    pub season: Option<u16>,
    pub episodes: Vec<u16>,
    /// cd1 / part 2 / disc 1 of a multi-file movie.
    pub part: Option<u16>,
    /// Episodes: the show's name and year, from the show folder above any season, release or
    /// per-episode folder, else from such a folder, else from the file name (issue #32).
    /// Grouping uses this, never a season or release folder itself.
    pub show_title: Option<String>,
    pub show_year: Option<u16>,
    pub resolution: Option<String>,
    pub hdr: bool,
}

const EXTRA_FOLDERS: &[&str] = &[
    "extras",
    "extra",
    "featurettes",
    "featurette",
    "behind the scenes",
    "deleted scenes",
    "interviews",
    "scenes",
    "shorts",
    "trailers",
    "trailer",
    "sample",
    "samples",
    "other",
    "bonus",
    "specials features",
];
const GENERIC_FOLDERS: &[&str] = &[
    "movies",
    "movie",
    "films",
    "film",
    "filme",
    "shows",
    "tv",
    "tv shows",
    "series",
    "serien",
    "videos",
    "video",
    "media",
    "downloads",
    "complete",
    "",
];
/// Tokens that end a title: quality, source, codec, audio, language and release words.
const STOP_TOKENS: &[&str] = &[
    "2160p",
    "1080p",
    "1080i",
    "720p",
    "576p",
    "480p",
    "4k",
    "uhd",
    "hdr",
    "hdr10",
    "dv",
    "sdr",
    "bluray",
    "blu-ray",
    "bdrip",
    "brrip",
    "bd",
    "remux",
    "web",
    "web-dl",
    "webdl",
    "webrip",
    "hdtv",
    "dvdrip",
    "dvd",
    "hdrip",
    "amzn",
    "nf",
    "dsnp",
    "hmax",
    "atvp",
    "x264",
    "x265",
    "h264",
    "h265",
    "h.264",
    "h.265",
    "hevc",
    "avc",
    "xvid",
    "divx",
    "10bit",
    "8bit",
    "aac",
    "ac3",
    "dts",
    "dts-hd",
    "truehd",
    "atmos",
    "ddp5",
    "dd5",
    "flac",
    "mp3",
    "proper",
    "repack",
    "extended",
    "remastered",
    "unrated",
    "directors",
    "imax",
    "internal",
    "limited",
    "multi",
    "german",
    "dl",
    "french",
    "korean",
    "japanese",
    "english",
    "ita",
    "spanish",
    "dubbed",
    "subbed",
    "cd1",
    "cd2",
    "cd3",
    "part1",
    "part2",
    "part3",
];

fn current_year_limit() -> u16 {
    static Y: OnceLock<u16> = OnceLock::new();
    *Y.get_or_init(|| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        (1970 + secs / 31_556_952) as u16 + 1
    })
}

fn as_year(tok: &str) -> Option<u16> {
    let t = tok.trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']');
    if t.len() != 4 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: u16 = t.parse().ok()?;
    (1900..=current_year_limit()).contains(&y).then_some(y)
}

/// Turn `The.Matrix_1999` into space-separated words, dropping leading `[Group]` tags.
fn words(name: &str) -> String {
    let mut s = name.trim().to_string();
    while s.starts_with('[') {
        match s.find(']') {
            Some(i) => s = s[i + 1..].trim_start().to_string(),
            None => break,
        }
    }
    // '.' and '_' separate words, except in audio channel counts like "5.1" (one digit on each
    // side), so "2022.1080p" still splits. '-' is never a separator ("9-1-1", "Spider-Man").
    let chars: Vec<char> = s.chars().collect();
    let digit = |i: usize| chars.get(i).is_some_and(char::is_ascii_digit);
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        let between_digits = c == '.'
            && i >= 1
            && digit(i - 1)
            && (i < 2 || !digit(i - 2))
            && digit(i + 1)
            && !digit(i + 2);
        if (c == '.' || c == '_') && !between_digits {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_title(s: &str) -> String {
    let mut t = s
        .replace(['[', ']', '(', ')', '{', '}'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    loop {
        let before = t.len();
        t = t
            .trim()
            .trim_end_matches(['-', ',', ':', '–'])
            .trim()
            .to_string();
        if t.len() == before {
            break;
        }
    }
    t
}

fn is_stop(tok: &str) -> bool {
    let t = tok
        .trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']')
        .to_ascii_lowercase();
    STOP_TOKENS.contains(&t.as_str())
        || t.starts_with("ddp")
        || t.starts_with("dts")
        || (t.ends_with('p')
            && t.len() >= 4
            && t[..t.len() - 1].bytes().all(|b| b.is_ascii_digit()))
}

/// Split "Title 2019 1080p ..." into (title, year). The year is the last year-like token that
/// is not the first word (so "1984 1984" and "2012 (2009)" keep their titles).
fn title_and_year(w: &str) -> (String, Option<u16>) {
    let toks: Vec<&str> = w.split(' ').collect();
    let mut year_at = None;
    for (i, t) in toks.iter().enumerate() {
        if i > 0 && as_year(t).is_some() {
            year_at = Some(i);
        }
        if i > 0 && is_stop(t) {
            break;
        }
    }
    let end = match year_at {
        Some(i) => i,
        None => toks
            .iter()
            .skip(1)
            .position(|t| is_stop(t))
            .map_or(toks.len(), |p| p + 1),
    };
    let year = year_at.and_then(|i| as_year(toks[i]));
    (clean_title(&toks[..end].join(" ")), year)
}

struct EpisodeMatch {
    start: usize, // byte offset where the pattern starts in the words string
    season: Option<u16>,
    episodes: Vec<u16>,
}

fn num(s: &str) -> Option<u16> {
    s.parse().ok()
}

/// SxxEyy[Eyy|-Eyy|-yy], "Sxx Eyy", NxNN, "Season N Episode M".
fn find_episode(w: &str) -> Option<EpisodeMatch> {
    let lower = w.to_ascii_lowercase();
    let b = lower.as_bytes();
    let digits = |mut i: usize| {
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        (s, i)
    };
    let boundary = |i: usize| i == 0 || !b[i - 1].is_ascii_alphanumeric();
    for i in 0..b.len() {
        // S01E02 / S01 E02
        if b[i] == b's' && boundary(i) {
            let (s0, s1) = digits(i + 1);
            if s1 > s0 && s1 - s0 <= 3 {
                let mut j = s1;
                if j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                if j < b.len() && b[j] == b'e' {
                    let (e0, e1) = digits(j + 1);
                    if e1 > e0 && e1 - e0 <= 4 {
                        let season = num(&lower[s0..s1]);
                        let mut eps = vec![num(&lower[e0..e1])?];
                        let mut k = e1;
                        // more episodes: E03, -E03, -03
                        loop {
                            let mut m = k;
                            if m < b.len() && b[m] == b'-' {
                                m += 1;
                            }
                            if m < b.len() && b[m] == b'e' {
                                m += 1;
                            } else if m == k {
                                break;
                            }
                            let (n0, n1) = digits(m);
                            if n1 == n0 || n1 - n0 > 4 {
                                break;
                            }
                            let last = *eps.last().unwrap();
                            let n = num(&lower[n0..n1])?;
                            if n <= last {
                                break;
                            }
                            // a range "E17-E19" means 17, 18, 19
                            let is_range = b[k] == b'-';
                            if is_range {
                                eps.extend(last + 1..=n);
                            } else {
                                eps.push(n);
                            }
                            k = n1;
                        }
                        if k == b.len() || !b[k].is_ascii_alphanumeric() {
                            return Some(EpisodeMatch {
                                start: i,
                                season,
                                episodes: eps,
                            });
                        }
                    }
                }
            }
        }
        // 4x11 / 04x12
        if b[i].is_ascii_digit() && boundary(i) {
            let (s0, s1) = digits(i);
            if s1 - s0 <= 2 && s1 < b.len() && b[s1] == b'x' {
                let (e0, e1) = digits(s1 + 1);
                if e1 > e0 && e1 - e0 <= 3 && (e1 == b.len() || !b[e1].is_ascii_alphanumeric()) {
                    return Some(EpisodeMatch {
                        start: i,
                        season: num(&lower[s0..s1]),
                        episodes: vec![num(&lower[e0..e1])?],
                    });
                }
            }
        }
    }
    // "Season 4 Episode 9"
    if let Some(p) = lower.find("season ") {
        let rest = &lower[p + 7..];
        let sn: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Some(q) = rest.find("episode ") {
            let en: String = rest[q + 8..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if !sn.is_empty() && !en.is_empty() {
                return Some(EpisodeMatch {
                    start: p,
                    season: num(&sn),
                    episodes: vec![num(&en)?],
                });
            }
        }
    }
    None
}

/// "Season 1", "Season 01", "S01", "Staffel 2", "Specials" -> season number.
fn season_folder(name: &str) -> Option<u16> {
    let l = words(name).to_ascii_lowercase();
    if l == "specials" || l == "special" {
        return Some(0);
    }
    for prefix in ["season ", "staffel ", "series ", "saison ", "temporada "] {
        if let Some(n) = l.strip_prefix(prefix) {
            return num(n.trim());
        }
    }
    let n = l.strip_prefix('s')?;
    (!n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
        .then(|| num(n))
        .flatten()
}

/// A folder that holds one season or one episode rather than a whole show: "Season 1",
/// "Specials", a release folder ("Show.S01.2160p.WEB-DL", "Show Season 2 1080p") or a
/// per-episode folder ("Show.S02E01.Title.1080p"). Returns the title and year in front of the
/// season or episode marker (empty when there is none) and the season number.
fn season_or_episode_folder(name: &str) -> Option<(String, Option<u16>, Option<u16>)> {
    if let Some(s) = season_folder(name) {
        return Some((String::new(), None, Some(s)));
    }
    let w = words(name);
    if let Some(m) = find_episode(&w) {
        let (t, y) = title_and_year(&w[..m.start]);
        return Some((t, y, m.season));
    }
    let toks: Vec<&str> = w.split(' ').collect();
    let short_season = |t: &str| {
        let n = t.strip_prefix(['s', 'S'])?;
        (!n.is_empty() && n.len() <= 2 && n.bytes().all(|b| b.is_ascii_digit()))
            .then(|| num(n))
            .flatten()
    };
    for (i, t) in toks.iter().enumerate() {
        let l = t.to_ascii_lowercase();
        // "S01", "S01-S03", "S01-03"
        let season = short_season(&l)
            .or_else(|| short_season(l.split_once('-')?.0))
            .or_else(|| {
                matches!(l.as_str(), "season" | "staffel" | "saison" | "temporada")
                    .then(|| toks.get(i + 1).and_then(|n| num(n)))
                    .flatten()
            });
        if let Some(s) = season {
            let (t, y) = if i == 0 {
                (String::new(), None)
            } else {
                title_and_year(&toks[..i].join(" "))
            };
            return Some((t, y, Some(s)));
        }
    }
    None
}

/// Two titles share a word (ignoring articles): "Star Wars Andor" and "Andor" do, a category
/// folder "Kids" and "Example Show" do not.
fn related(a: &str, b: &str) -> bool {
    let set = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty() && !matches!(*w, "the" | "a" | "an" | "der" | "die" | "das"))
            .map(str::to_string)
            .collect()
    };
    let b = set(b);
    set(a).iter().any(|w| b.contains(w))
}

/// An episode number on its own: "01 - Serenity", "Episode 3", "E04", "Dark - 05", "Part 8".
fn bare_episode(w: &str) -> Option<u16> {
    let toks: Vec<&str> = w.split(' ').collect();
    let first = toks.first()?.to_ascii_lowercase();
    if first.len() <= 3 && first.bytes().all(|b| b.is_ascii_digit()) {
        return num(&first);
    }
    if let Some(n) = first.strip_prefix('e') {
        if !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()) {
            return num(n);
        }
    }
    for (i, t) in toks.iter().enumerate() {
        let l = t.to_ascii_lowercase();
        if (l == "episode" || l == "ep" || l == "part" || l == "folge") && i + 1 < toks.len() {
            if let Some(n) = num(toks[i + 1]) {
                return Some(n);
            }
        }
        if l == "-" && i + 1 < toks.len() && toks[i + 1].len() <= 3 {
            if let Some(n) = num(toks[i + 1]) {
                return Some(n);
            }
        }
    }
    None
}

/// cd1, part2, pt 3, disc 1 near the end of a movie name.
fn movie_part(w: &str) -> Option<u16> {
    let toks: Vec<String> = w.split(' ').map(str::to_ascii_lowercase).collect();
    for (i, t) in toks.iter().enumerate().rev() {
        for p in ["cd", "part", "pt", "disc", "disk", "dvd"] {
            if let Some(n) = t.strip_prefix(p) {
                if !n.is_empty() && n.len() <= 2 && n.bytes().all(|b| b.is_ascii_digit()) {
                    return num(n);
                }
                if n.is_empty() && i + 1 < toks.len() && toks[i + 1].len() <= 2 {
                    if let Some(v) = num(&toks[i + 1]) {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

fn resolution(w: &str) -> Option<String> {
    let l = w.to_ascii_lowercase();
    for r in ["2160p", "1080p", "1080i", "720p", "576p", "480p"] {
        if l.contains(r) {
            return Some(r.to_string());
        }
    }
    (l.contains(" 4k") || l.contains(" uhd")).then(|| "2160p".to_string())
}

/// "Extras", "Featurettes", but also release-style "The.Big.Short.Extras-Grym".
fn is_extra_folder(name: &str) -> bool {
    let l = words(name).to_ascii_lowercase();
    if EXTRA_FOLDERS.contains(&l.as_str()) {
        return true;
    }
    let toks: Vec<&str> = l
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    toks.iter().any(|t| {
        matches!(
            *t,
            "extras" | "featurettes" | "bonus" | "sample" | "samples" | "trailers"
        )
    }) || l.contains("behind the scenes")
        || l.contains("deleted scenes")
}

fn is_extra_name(stem_words: &str) -> bool {
    let l = stem_words.to_ascii_lowercase();
    l == "sample"
        || l.ends_with(" sample")
        || l.ends_with("-sample")
        || l.ends_with("-trailer")
        || l.ends_with(" trailer")
        || l.ends_with("-featurette")
        || l.ends_with("-deleted")
        || l.ends_with("-behindthescenes")
}

fn strip_ext(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((s, e))
            if !s.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            s
        }
        _ => name,
    }
}

/// Parse a path relative to a library root. Both `/` and `\` separate components.
pub fn parse(relative_path: &str) -> Parsed {
    let comps: Vec<&str> = relative_path
        .split(['/', '\\'])
        .filter(|c| !c.is_empty())
        .collect();
    let file = comps.last().copied().unwrap_or("");
    let folders = &comps[..comps.len().saturating_sub(1)];
    let stem = strip_ext(file);
    let w = words(stem);

    let mut p = Parsed {
        kind: Kind::Movie,
        title: String::new(),
        year: None,
        season: None,
        episodes: vec![],
        part: None,
        show_title: None,
        show_year: None,
        resolution: resolution(&w),
        hdr: {
            let l = w.to_ascii_lowercase();
            l.contains("hdr") || l.split(' ').any(|t| t == "dv" || t == "dovi")
        },
    };

    // Disc structures: a Blu-ray (BDMV/STREAM/*.m2ts) or DVD (VIDEO_TS/*.vob) folder is one
    // movie named after the folder holding BDMV or VIDEO_TS. Its streams are versions of that
    // item; playback picks the largest, which is the main feature.
    if let Some(i) = folders
        .iter()
        .position(|f| f.eq_ignore_ascii_case("BDMV") || f.eq_ignore_ascii_case("VIDEO_TS"))
    {
        let in_streams = folders[i].eq_ignore_ascii_case("VIDEO_TS")
            || folders
                .get(i + 1)
                .is_some_and(|f| f.eq_ignore_ascii_case("STREAM"));
        let disc = if i > 0 { Some(folders[i - 1]) } else { None }
            .filter(|f| !GENERIC_FOLDERS.contains(&words(f).to_ascii_lowercase().as_str()));
        match (in_streams, disc) {
            (true, Some(d)) => {
                let (t, y) = title_and_year(&words(d));
                p.title = t;
                p.year = y;
                p.resolution = resolution(&words(d));
                return p;
            }
            _ => {
                p.kind = Kind::Extra; // menus, backups, or a disc without a named folder
                return p;
            }
        }
    }

    if folders.iter().any(|f| is_extra_folder(f)) || is_extra_name(&w) {
        p.kind = Kind::Extra;
        return p;
    }

    // Folder layout, from the file upwards: season, release and per-episode folders
    // ("Season 1", "Show.S01.2160p.WEB", "Show.S02E01.Title.1080p"), then the show folder.
    let mut folder_season = None;
    let mut release: Option<(String, Option<u16>)> = None;
    let mut show_folder: Option<&str> = None;
    for f in folders.iter().rev() {
        if let Some((t, y, s)) = season_or_episode_folder(f) {
            folder_season = folder_season.or(s);
            if release.is_none() && !t.is_empty() {
                release = Some((t, y));
            }
            continue;
        }
        if !GENERIC_FOLDERS.contains(&words(f).to_ascii_lowercase().as_str()) {
            show_folder = Some(f);
        }
        break;
    }

    if let Some(m) = find_episode(&w) {
        p.kind = Kind::Episode;
        p.season = m.season;
        p.episodes = m.episodes;
        let (t, y) = title_and_year(&w[..m.start]);
        p.title = t;
        p.year = y;
    } else if let (Some(s), Some(e)) = (folder_season, bare_episode(&w)) {
        p.kind = Kind::Episode;
        p.season = Some(s);
        p.episodes = vec![e];
    }

    if p.kind == Kind::Episode {
        let folder = show_folder.map(|f| title_and_year(&words(f)));
        // A folder that shares no word with the episode's own title is a category ("Kids").
        let named = (!p.title.is_empty())
            .then(|| p.title.clone())
            .or_else(|| release.as_ref().map(|r| r.0.clone()));
        let folder =
            folder.filter(|(t, _)| !t.is_empty() && named.as_deref().is_none_or(|n| related(t, n)));
        let (show, year) = match (&folder, &release) {
            (Some((t, y)), r) => (t.clone(), y.or(r.as_ref().and_then(|r| r.1))),
            (None, Some((t, y))) => (t.clone(), *y),
            (None, None) => (p.title.clone(), p.year),
        };
        if !show.is_empty() {
            p.show_title = Some(show);
            p.show_year = year.or(p.year);
        }
        if p.season.is_none() {
            p.season = folder_season;
        }
        if p.title.is_empty() {
            // "Show/Season 1/01 - Pilot.mkv": the file has no show name of its own.
            if let Some(t) = &p.show_title {
                p.title = t.clone();
                p.year = p.year.or(p.show_year);
            }
        } else if p.year.is_none() {
            // "Doctor Who (2005)/Season 1/S01E01.mkv" style years live on the show folder
            if let Some(f) = show_folder {
                let (t, y) = title_and_year(&words(f));
                if t.eq_ignore_ascii_case(&p.title) {
                    p.year = y;
                }
            }
        }
        return p;
    }

    // Movie
    let (t, y) = title_and_year(&w);
    p.title = t;
    p.year = y;
    p.part = movie_part(&w);
    if p.year.is_none() {
        if let Some(f) = folders
            .last()
            .filter(|f| !GENERIC_FOLDERS.contains(&words(f).to_ascii_lowercase().as_str()))
        {
            let (ft, fy) = title_and_year(&words(f));
            if fy.is_some() {
                p.title = ft;
                p.year = fy;
            }
        }
    }
    p
}

/// The edition a file name names (issue #34): "{edition-Director's Cut}" as other media
/// servers write it, or words like "Director's Cut", "Extended", "Theatrical", "IMAX".
pub fn edition(name: &str) -> Option<String> {
    if let Some(start) = name.find("{edition-") {
        let rest = &name[start + 9..];
        if let Some(end) = rest.find('}') {
            let e = rest[..end].trim();
            if !e.is_empty() {
                return Some(e.to_string());
            }
        }
    }
    // Only after the title ("Uncut Gems" is a title, not an edition).
    let all = words(strip_ext(name));
    let (title, _) = title_and_year(&all);
    let rest = all
        .get(all.find(title.as_str()).map_or(0, |i| i + title.len())..)
        .unwrap_or("");
    let w = format!(" {} ", rest.to_lowercase().replace(['\'', '’'], ""));
    for (pattern, label) in [
        (" directors cut ", "Director's Cut"),
        (" director cut ", "Director's Cut"),
        (" final cut ", "Final Cut"),
        (" extended ", "Extended"),
        (" theatrical ", "Theatrical"),
        (" imax ", "IMAX"),
        (" unrated ", "Unrated"),
        (" uncut ", "Uncut"),
    ] {
        if w.contains(pattern) {
            return Some(label.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editions() {
        assert_eq!(
            edition("Blade Runner (1982) {edition-Final Cut}.mkv").as_deref(),
            Some("Final Cut")
        );
        assert_eq!(
            edition("Alien.1979.Directors.Cut.2160p.mkv").as_deref(),
            Some("Director's Cut")
        );
        assert_eq!(
            edition("Aliens (1986) Extended Edition.mkv").as_deref(),
            Some("Extended")
        );
        assert_eq!(edition("Dune.2021.IMAX.2160p.mkv").as_deref(), Some("IMAX"));
        assert_eq!(
            edition("Alien (1979) - Theatrical.mp4").as_deref(),
            Some("Theatrical")
        );
        assert_eq!(edition("Alien.1979.1080p.mkv"), None);
        assert_eq!(
            edition("Uncut Gems (2019).mkv"),
            None,
            "a title word is not an edition"
        );
        assert_eq!(edition("Extended.Family.2020.mkv"), None);
    }

    #[test]
    fn words_and_years() {
        assert_eq!(
            words("[Grp] The.Matrix_1999.DD5.1"),
            "The Matrix 1999 DD5.1"
        );
        assert_eq!(as_year("(1999)"), Some(1999));
        assert_eq!(as_year("2049"), None);
        assert_eq!(season_folder("Season 01"), Some(1));
        assert_eq!(season_folder("S2"), Some(2));
        assert_eq!(season_folder("Staffel 3"), Some(3));
        assert_eq!(season_folder("Specials"), Some(0));
        assert_eq!(season_folder("Severance"), None);
    }
}
