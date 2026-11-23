//! What a managed copy carries (3.0, issue #153, SPEC 7): the canonical metadata of a track or
//! book as tags, and the `.nfo` next to a video. Only managed copies are written automatically.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// Tags for one file. An empty field is not touched in the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyTags {
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artists: Vec<String>,
    pub date: String,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub disc_total: Option<u32>,
    pub genre: String,
    pub compilation: bool,
    pub mb_artists: Vec<String>,
    pub mb_album_artists: Vec<String>,
    pub mb_release_group: String,
    pub mb_release: String,
    pub mb_recording: String,
    pub mb_track: String,
    pub isrc: Vec<String>,
    pub barcode: String,
    pub narrators: Vec<String>,
    pub series: String,
    pub series_part: String,
}

fn external(c: &Connection, item: i64, kind: &str) -> Result<Vec<String>> {
    let mut st = c.prepare_cached(
        "SELECT value FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = ?2 ORDER BY value",
    )?;
    let rows = st.query_map(params![item, kind], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn one(c: &Connection, item: i64, kind: &str) -> Result<String> {
    Ok(external(c, item, kind)?
        .into_iter()
        .next()
        .unwrap_or_default())
}

/// (names, MusicBrainz ids) of a credit, in order.
fn credits(c: &Connection, item: i64, role: &str) -> Result<(Vec<String>, Vec<String>)> {
    let mut st = c.prepare_cached(
        "SELECT coalesce(a.title, ''), (SELECT e.value FROM external_id e
             WHERE e.item_id = a.id AND e.provider = 'musicbrainz' AND e.kind = 'artist' LIMIT 1)
         FROM credit cr JOIN item a ON a.id = cr.artist_id
         WHERE cr.item_id = ?1 AND cr.role = ?2 ORDER BY cr.position",
    )?;
    let rows: Vec<(String, Option<String>)> = st
        .query_map(params![item, role], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let ids = if rows.iter().all(|(_, i)| i.is_some()) {
        rows.iter().filter_map(|(_, i)| i.clone()).collect()
    } else {
        Vec::new() // Ids pair with names only when every artist has one.
    };
    Ok((rows.into_iter().map(|(n, _)| n).collect(), ids))
}

/// The tags of one track, from the library's canonical data.
pub fn track_tags(c: &Connection, track: i64) -> Result<Option<CopyTags>> {
    let head = c
        .query_row(
            "SELECT coalesce(t.title, ''), t.parent_id, coalesce(a.title, ''),
                    coalesce(m.track_no, t.episode_no), m.track_total, coalesce(m.disc_no, t.season_no),
                    coalesce(am.disc_total, 0), coalesce(am.release_date, m.release_date, a.year, ''),
                    coalesce(m.genre, am.genre, ''), coalesce(am.compilation, 0)
             FROM item t JOIN item a ON a.id = t.parent_id
             LEFT JOIN music m ON m.item_id = t.id LEFT JOIN music am ON am.item_id = a.id
             WHERE t.id = ?1 AND t.kind = 'track'",
            [track],
            |r| {
                let date: rusqlite::types::Value = r.get(7)?;
                let date = match date {
                    rusqlite::types::Value::Text(s) => s,
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    _ => String::new(),
                };
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<u32>>(3)?,
                    r.get::<_, Option<u32>>(4)?,
                    r.get::<_, Option<u32>>(5)?,
                    r.get::<_, u32>(6)?,
                    date,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)? != 0,
                ))
            },
        )
        .optional()?;
    let Some((title, album, album_title, no, total, disc, discs, date, genre, compilation)) = head
    else {
        return Ok(None);
    };
    let (artists, mb_artists) = credits(c, track, "artist")?;
    let (album_artists, mb_album_artists) = credits(c, album, "album_artist")?;
    Ok(Some(CopyTags {
        title,
        artists,
        album: album_title,
        album_artists,
        date,
        track: no.filter(|n| *n > 0),
        track_total: total.filter(|n| *n > 0),
        disc: disc.filter(|n| *n > 0),
        disc_total: (discs > 0).then_some(discs),
        genre,
        compilation,
        mb_artists,
        mb_album_artists,
        mb_release_group: one(c, album, "release_group")?,
        mb_release: one(c, album, "release")?,
        mb_recording: one(c, track, "recording")?,
        mb_track: one(c, track, "track")?,
        isrc: external(c, track, "isrc")?,
        barcode: one(c, album, "barcode")?,
        ..Default::default()
    }))
}

/// The tags of an audiobook. Fields the user locked (`book.locked`) stay as they are in the file.
pub fn book_tags(c: &Connection, book: i64) -> Result<Option<CopyTags>> {
    book_fields(c, book, true)
}

/// All of a book's fields, for its name: a locked title still names the folder.
pub fn book_tags_unlocked(c: &Connection, book: i64) -> Result<Option<CopyTags>> {
    book_fields(c, book, false)
}

fn book_fields(c: &Connection, book: i64, honour_locks: bool) -> Result<Option<CopyTags>> {
    let head = c
        .query_row(
            "SELECT coalesce(i.title, ''), coalesce(b.published, i.year, ''), b.series_position, b.locked,
                    coalesce((SELECT s.title FROM item s WHERE s.id = i.parent_id AND s.kind = 'book_series'), '')
             FROM item i JOIN book b ON b.item_id = i.id WHERE i.id = ?1",
            [book],
            |r| {
                let date: rusqlite::types::Value = r.get(1)?;
                let date = match date {
                    rusqlite::types::Value::Text(s) => s,
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    _ => String::new(),
                };
                Ok((
                    r.get::<_, String>(0)?,
                    date,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((title, date, position, locked, series)) = head else {
        return Ok(None);
    };
    let people = |role: &str| -> Result<Vec<String>> {
        let mut st = c.prepare_cached(
            "SELECT coalesce(p.title, '') FROM book_credit bc JOIN item p ON p.id = bc.person_id
             WHERE bc.book_id = ?1 AND bc.role = ?2 ORDER BY bc.position",
        )?;
        let rows = st.query_map(params![book, role], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    let authors = people("author")?;
    let mut tags = CopyTags {
        album: title.clone(),
        title,
        artists: authors.clone(),
        album_artists: authors,
        narrators: people("narrator")?,
        date,
        series,
        series_part: position.map(|p| p.to_string()).unwrap_or_default(),
        ..Default::default()
    };
    if !honour_locks {
        return Ok(Some(tags));
    }
    let locked: BTreeSet<String> = serde_json::from_str(&locked).unwrap_or_default();
    let is = |f: &str| locked.contains(f);
    if is("title") {
        tags.title.clear();
        tags.album.clear();
    }
    if is("authors") {
        tags.artists.clear();
        tags.album_artists.clear();
    }
    if is("narrators") {
        tags.narrators.clear();
    }
    if is("series") {
        tags.series.clear();
        tags.series_part.clear();
    }
    if is("published") {
        tags.date.clear();
    }
    Ok(Some(tags))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn ids(tmdb: Option<i64>, imdb: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(t) = tmdb {
        out.push_str(&format!(
            "  <uniqueid type=\"tmdb\" default=\"true\">{t}</uniqueid>\n"
        ));
    }
    if let Some(i) = imdb.filter(|i| !i.is_empty()) {
        let default = if tmdb.is_none() {
            " default=\"true\""
        } else {
            ""
        };
        out.push_str(&format!(
            "  <uniqueid type=\"imdb\"{default}>{}</uniqueid>\n",
            xml(i)
        ));
    }
    out
}

/// The `.nfo` beside a movie or episode: title, year, TMDB and IMDb IDs, and for an episode its
/// show, season and number. The video file itself is not rewritten.
pub fn video_nfo(c: &Connection, item: i64) -> Result<Option<String>> {
    type Row = (
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    );
    let row: Option<Row> = c
        .query_row(
            "SELECT kind, coalesce(title, ''), year, tmdb_id, imdb_id, season_no, episode_no, parent_id
             FROM item WHERE id = ?1",
            [item],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, title, year, tmdb, imdb, season, episode, parent)) = row else {
        return Ok(None);
    };
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
    let year_line = year
        .map(|y| format!("  <year>{y}</year>\n"))
        .unwrap_or_default();
    Ok(match kind.as_str() {
        "movie" | "video" => Some(format!(
            "{head}<movie>\n  <title>{}</title>\n{year_line}{}</movie>\n",
            xml(&title),
            ids(tmdb, imdb.as_deref())
        )),
        "episode" => {
            // Episode → season → show.
            let season_parent: Option<i64> = match parent {
                Some(p) => c
                    .query_row("SELECT parent_id FROM item WHERE id = ?1", [p], |r| {
                        r.get(0)
                    })
                    .optional()?
                    .flatten(),
                None => None,
            };
            let show: String = match season_parent {
                Some(s) => c
                    .query_row(
                        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
                        [s],
                        |r| r.get(0),
                    )
                    .optional()?
                    .unwrap_or_default(),
                None => String::new(),
            };
            Some(format!(
                "{head}<episodedetails>\n  <title>{}</title>\n  <showtitle>{}</showtitle>\n  <season>{}</season>\n  <episode>{}</episode>\n{year_line}{}</episodedetails>\n",
                xml(&title),
                xml(&show),
                season.unwrap_or(0),
                episode.unwrap_or(0),
                ids(tmdb, imdb.as_deref())
            ))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    #[test]
    fn a_track_with_every_id() {
        let c = db();
        c.execute_batch(
            "INSERT INTO item(id, kind, title, sort_title) VALUES (1, 'artist', 'Rosalía', 'r');
             INSERT INTO item(id, kind, title, sort_title, parent_id, year) VALUES (2, 'album', 'MOTOMAMI', 'm', 1, 2022);
             INSERT INTO item(id, kind, title, sort_title, parent_id, season_no, episode_no) VALUES (3, 'track', 'SAOKO', 's', 2, 1, 1);
             INSERT INTO music(item_id, release_date, disc_total, genre) VALUES (2, '2022-03-18', 1, 'Pop');
             INSERT INTO music(item_id, disc_no, track_no, track_total) VALUES (3, 1, 1, 16);
             INSERT INTO credit(item_id, artist_id, role, position) VALUES (2, 1, 'album_artist', 0), (3, 1, 'artist', 0);
             INSERT INTO external_id(item_id, provider, kind, value) VALUES
               (1, 'musicbrainz', 'artist', 'art'), (2, 'musicbrainz', 'release', 'rel'),
               (2, 'musicbrainz', 'release_group', 'rg'), (2, 'musicbrainz', 'barcode', '0194'),
               (3, 'musicbrainz', 'recording', 'rec'), (3, 'musicbrainz', 'track', 'trk'),
               (3, 'musicbrainz', 'isrc', 'USRC17607839');",
        )
        .unwrap();
        let t = track_tags(&c, 3).unwrap().unwrap();
        assert_eq!((t.title.as_str(), t.album.as_str()), ("SAOKO", "MOTOMAMI"));
        assert_eq!(t.date, "2022-03-18");
        assert_eq!(
            (t.track, t.track_total, t.disc, t.disc_total),
            (Some(1), Some(16), Some(1), Some(1))
        );
        assert_eq!(t.artists, ["Rosalía"]);
        assert_eq!(t.mb_artists, ["art"]);
        assert_eq!(
            (t.mb_release.as_str(), t.mb_release_group.as_str()),
            ("rel", "rg")
        );
        assert_eq!(
            (t.mb_recording.as_str(), t.mb_track.as_str()),
            ("rec", "trk")
        );
        assert_eq!(t.isrc, ["USRC17607839"]);
        assert_eq!(t.barcode, "0194");
    }

    #[test]
    fn locked_book_fields_are_left_out() {
        let c = db();
        c.execute_batch(
            r#"INSERT INTO item(id, kind, title, sort_title) VALUES (5, 'person', 'Jane', 'j'), (6, 'person', 'Reader', 'r');
               INSERT INTO item(id, kind, title, sort_title) VALUES (7, 'audiobook', 'Emma', 'e');
               INSERT INTO book(item_id, key, published, locked) VALUES (7, 'k', '1815', '["title"]');
               INSERT INTO book_credit(book_id, person_id, role, position) VALUES (7, 5, 'author', 0), (7, 6, 'narrator', 0);"#,
        )
        .unwrap();
        let t = book_tags(&c, 7).unwrap().unwrap();
        assert_eq!(t.title, "", "locked");
        assert_eq!(t.artists, ["Jane"]);
        assert_eq!(t.narrators, ["Reader"]);
        assert_eq!(t.date, "1815");
    }

    #[test]
    fn nfo_for_a_movie_and_an_episode() {
        let c = db();
        c.execute_batch(
            "INSERT INTO item(id, kind, title, sort_title, year, tmdb_id, imdb_id) VALUES (1, 'movie', 'Tom & Jerry <1>', 't', 1992, 348, 'tt1');
             INSERT INTO item(id, kind, title, sort_title, year) VALUES (2, 'show', 'Firefly', 'f', 2002);
             INSERT INTO item(id, kind, title, sort_title, parent_id, season_no) VALUES (3, 'season', 'Season 1', 's', 2, 1);
             INSERT INTO item(id, kind, title, sort_title, parent_id, season_no, episode_no, imdb_id) VALUES (4, 'episode', 'Serenity', 's', 3, 1, 1, 'tt2');",
        )
        .unwrap();
        let m = video_nfo(&c, 1).unwrap().unwrap();
        assert!(
            m.contains("<title>Tom &amp; Jerry &lt;1&gt;</title>"),
            "{m}"
        );
        assert!(m.contains("<year>1992</year>"));
        assert!(m.contains(r#"<uniqueid type="tmdb" default="true">348</uniqueid>"#));
        assert!(m.contains(r#"<uniqueid type="imdb">tt1</uniqueid>"#));
        let e = video_nfo(&c, 4).unwrap().unwrap();
        assert!(
            e.contains("<episodedetails>") && e.contains("<showtitle>Firefly</showtitle>"),
            "{e}"
        );
        assert!(e.contains("<season>1</season>") && e.contains("<episode>1</episode>"));
        assert!(e.contains(r#"<uniqueid type="imdb" default="true">tt2</uniqueid>"#));
        assert!(video_nfo(&c, 2).unwrap().is_none());
    }
}
