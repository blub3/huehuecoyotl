//! Issue #92: what the user pasted into a match search, when it is an id or a link rather than a
//! title. Recognised: TMDB links (`themoviedb.org/movie/603-the-matrix`, `/tv/1399`), IMDb ids
//! and links (`tt0133093`), MusicBrainz links and bare MBIDs (release, release group, recording,
//! artist), Open Library links and ids (`OL66554W` works, `OL23919A` authors) and ISBN-10/13.

/// A reference to one record at a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ref {
    /// A TMDB movie (`show` false) or show (`show` true).
    Tmdb {
        show: bool,
        id: i64,
    },
    Imdb(String),
    MbRelease(String),
    MbReleaseGroup(String),
    MbRecording(String),
    MbArtist(String),
    /// A bare MBID: the caller knows from the context what it names.
    Mbid(String),
    OlWork(String),
    OlAuthor(String),
    Isbn(String),
}

/// A MusicBrainz id: 8-4-4-4-12 hexadecimal digits.
fn is_mbid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// "tt0133093": two letters and seven or more digits.
fn imdb_id(s: &str) -> Option<String> {
    let s = s.to_ascii_lowercase();
    let digits = s.strip_prefix("tt")?;
    (digits.len() >= 7 && digits.chars().all(|c| c.is_ascii_digit())).then_some(s)
}

/// "OL66554W" → ("OL66554W", 'W').
fn ol_id(s: &str) -> Option<(String, char)> {
    let up = s.to_ascii_uppercase();
    let rest = up.strip_prefix("OL")?;
    let kind = rest.chars().last()?;
    let digits = &rest[..rest.len() - 1];
    (matches!(kind, 'W' | 'A' | 'M')
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit()))
    .then_some((up, kind))
}

/// ISBN-10 or ISBN-13 with a valid check digit; hyphens and spaces do not count.
fn isbn(s: &str) -> Option<String> {
    let clean: String = s
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .collect::<String>()
        .to_ascii_uppercase();
    let clean = clean
        .strip_prefix("ISBN")
        .unwrap_or(&clean)
        .trim_start_matches(':')
        .to_string();
    let digits: Vec<u32> = clean
        .chars()
        .map(|c| if c == 'X' { Some(10) } else { c.to_digit(10) })
        .collect::<Option<_>>()?;
    let ok = match digits.len() {
        10 => {
            digits[..9].iter().all(|d| *d < 10)
                && digits
                    .iter()
                    .enumerate()
                    .map(|(i, d)| (10 - i as u32) * d)
                    .sum::<u32>()
                    % 11
                    == 0
        }
        13 => {
            digits.iter().all(|d| *d < 10)
                && digits
                    .iter()
                    .enumerate()
                    .map(|(i, d)| if i % 2 == 0 { *d } else { 3 * d })
                    .sum::<u32>()
                    % 10
                    == 0
        }
        _ => false,
    };
    ok.then_some(clean)
}

/// The path segments of a link on `host` (or one of its subdomains), without query or fragment.
fn segments<'a>(text: &'a str, host: &str) -> Option<Vec<&'a str>> {
    let rest = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    let (h, path) = rest.split_once('/')?;
    let h = h.to_ascii_lowercase();
    if h != host && !h.ends_with(&format!(".{host}")) {
        return None;
    }
    let path = path.split(['?', '#']).next().unwrap_or_default();
    Some(path.split('/').filter(|s| !s.is_empty()).collect())
}

/// What `text` refers to, or `None` when it is an ordinary search.
pub fn parse(text: &str) -> Option<Ref> {
    let t = text.trim();
    if t.is_empty()
        || t.contains(char::is_whitespace) && !t.to_ascii_uppercase().starts_with("ISBN")
    {
        return isbn(t).map(Ref::Isbn);
    }
    if let Some(seg) = segments(t, "themoviedb.org") {
        let kind = seg.iter().position(|s| *s == "movie" || *s == "tv")?;
        let id: i64 = seg.get(kind + 1)?.split('-').next()?.parse().ok()?;
        return Some(Ref::Tmdb {
            show: seg[kind] == "tv",
            id,
        });
    }
    if let Some(seg) = segments(t, "imdb.com") {
        return seg.iter().find_map(|s| imdb_id(s)).map(Ref::Imdb);
    }
    if let Some(seg) = segments(t, "musicbrainz.org") {
        let (kind, id) = (seg.first()?, seg.get(1)?);
        if !is_mbid(id) {
            return None;
        }
        let id = id.to_ascii_lowercase();
        return match *kind {
            "release" => Some(Ref::MbRelease(id)),
            "release-group" => Some(Ref::MbReleaseGroup(id)),
            "recording" => Some(Ref::MbRecording(id)),
            "artist" => Some(Ref::MbArtist(id)),
            _ => None,
        };
    }
    if let Some(seg) = segments(t, "openlibrary.org") {
        return seg.iter().find_map(|s| match ol_id(s) {
            Some((id, 'W')) => Some(Ref::OlWork(id)),
            Some((id, 'A')) => Some(Ref::OlAuthor(id)),
            _ => None,
        });
    }
    if is_mbid(t) {
        return Some(Ref::Mbid(t.to_ascii_lowercase()));
    }
    if let Some(id) = imdb_id(t) {
        return Some(Ref::Imdb(id));
    }
    match ol_id(t) {
        Some((id, 'W')) => return Some(Ref::OlWork(id)),
        Some((id, 'A')) => return Some(Ref::OlAuthor(id)),
        _ => {}
    }
    isbn(t).map(Ref::Isbn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_ids() {
        assert_eq!(
            parse("https://www.themoviedb.org/movie/603-the-matrix?language=de"),
            Some(Ref::Tmdb {
                show: false,
                id: 603
            })
        );
        assert_eq!(
            parse("themoviedb.org/tv/1399"),
            Some(Ref::Tmdb {
                show: true,
                id: 1399
            })
        );
        assert_eq!(
            parse("https://www.imdb.com/title/tt0133093/"),
            Some(Ref::Imdb("tt0133093".into()))
        );
        assert_eq!(parse("TT0133093"), Some(Ref::Imdb("tt0133093".into())));
        let mbid = "b84ee12a-09ef-421b-82de-0441a926375b";
        assert_eq!(
            parse(&format!("https://musicbrainz.org/release/{mbid}")),
            Some(Ref::MbRelease(mbid.into()))
        );
        assert_eq!(
            parse(&format!(
                "https://musicbrainz.org/artist/{}",
                mbid.to_uppercase()
            )),
            Some(Ref::MbArtist(mbid.into()))
        );
        assert_eq!(
            parse(&format!("musicbrainz.org/recording/{mbid}")),
            Some(Ref::MbRecording(mbid.into()))
        );
        assert_eq!(
            parse(&format!("https://musicbrainz.org/release-group/{mbid}")),
            Some(Ref::MbReleaseGroup(mbid.into()))
        );
        assert_eq!(parse(mbid), Some(Ref::Mbid(mbid.into())));
        assert_eq!(
            parse("https://openlibrary.org/works/OL66554W/Pride_and_Prejudice"),
            Some(Ref::OlWork("OL66554W".into()))
        );
        assert_eq!(
            parse("https://openlibrary.org/authors/OL21594A"),
            Some(Ref::OlAuthor("OL21594A".into()))
        );
        assert_eq!(parse("ol21594a"), Some(Ref::OlAuthor("OL21594A".into())));
        assert_eq!(
            parse("978-3-16-148410-0"),
            Some(Ref::Isbn("9783161484100".into()))
        );
        assert_eq!(
            parse("ISBN 0-306-40615-2"),
            Some(Ref::Isbn("0306406152".into()))
        );
        assert_eq!(parse("080442957X"), Some(Ref::Isbn("080442957X".into())));
    }

    #[test]
    fn titles_stay_titles() {
        for t in [
            "The Matrix",
            "1984",
            "9783161484101",
            "Blade Runner 2049",
            "https://example.org/movie/603",
            "https://musicbrainz.org/release/not-an-id",
            "",
        ] {
            assert_eq!(parse(t), None, "{t}");
        }
    }
}
