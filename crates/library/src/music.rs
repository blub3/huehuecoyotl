//! Music matching in the database (issue #11, SPEC 7 "Providers"): what an album looks like to a
//! provider, and what a matched release adds. A release fills in what the tags lack (ids, release
//! type, date) and never overwrites a tag; a locked album is left alone.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The `setting` key that turns online music information off ("0"); on by default (v1.9).
pub const ONLINE_SETTING: &str = "music_online";
/// An album without a match is tried again after a week, like video.
const RETRY_AFTER_MS: i64 = 7 * 24 * 3600 * 1000;

pub fn online(c: &Connection) -> Result<bool> {
    Ok(crate::copies::setting(c, ONLINE_SETTING)?.as_deref() != Some("0"))
}

pub fn set_online(c: &Connection, on: bool) -> Result<()> {
    crate::copies::set_setting(c, ONLINE_SETTING, Some(if on { "1" } else { "0" }))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumTarget {
    pub id: i64,
    pub title: String,
    /// The album artist's name ("Various Artists" for compilations).
    pub artist: String,
    pub tracks: u32,
    pub release_id: Option<String>,
    pub group_id: Option<String>,
    pub locked: bool,
    pub match_state: String,
    /// It has a cover in the grid tier already.
    pub has_cover: bool,
}

fn external(c: &Connection, item: i64, kind: &str) -> Result<Option<String>> {
    Ok(c
        .query_row(
            "SELECT value FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = ?2",
            params![item, kind],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn album_target(c: &Connection, album: i64) -> Result<Option<AlbumTarget>> {
    let row = c
        .query_row(
            "SELECT a.title, coalesce(p.title, ''), a.locked, coalesce(a.match_state, 'unmatched'),
                    (SELECT count(*) FROM item t WHERE t.parent_id = a.id AND t.kind = 'track'),
                    EXISTS (SELECT 1 FROM artwork w WHERE w.item_id = a.id AND w.role = 'poster_grid' AND w.local_path IS NOT NULL)
             FROM item a LEFT JOIN item p ON p.id = a.parent_id WHERE a.id = ?1 AND a.kind = 'album'",
            [album],
            |r| {
                Ok(AlbumTarget {
                    id: album,
                    title: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    artist: r.get(1)?,
                    locked: r.get::<_, i64>(2)? != 0,
                    match_state: r.get(3)?,
                    tracks: r.get::<_, i64>(4)? as u32,
                    has_cover: r.get(5)?,
                    ..Default::default()
                })
            },
        )
        .optional()?;
    let Some(mut t) = row else {
        return Ok(None);
    };
    t.release_id = external(c, album, "release")?;
    t.group_id = external(c, album, "release_group")?;
    Ok(Some(t))
}

/// Albums to look up: not matched yet (or for review) and not tried for a week, not locked.
pub fn due(c: &Connection) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT id FROM item WHERE kind = 'album' AND locked = 0
           AND coalesce(match_state, 'unmatched') IN ('unmatched', 'needs_review')
           AND (meta_fetched_at IS NULL OR meta_fetched_at < ?1)",
    )?;
    let rows = st.query_map([now_ms() - RETRY_AFTER_MS], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// What a matched release says, without the provider's types.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleaseFacts {
    pub id: String,
    pub group_id: Option<String>,
    pub release_type: Option<String>,
    pub compilation: bool,
    pub date: Option<String>,
    /// (name, id) of the release's artists.
    pub artists: Vec<(String, String)>,
    /// (disc, position, title, recording id)
    pub tracks: Vec<(u32, u32, String, String)>,
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn set_external(c: &Connection, item: i64, kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let has: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = ?2)",
        params![item, kind],
        |r| r.get(0),
    )?;
    if !has {
        c.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', ?2, ?3)",
            params![item, kind, value.to_lowercase()],
        )?;
    }
    Ok(())
}

/// A matched release: ids, release type and date where the tags had none; the album artists'
/// and the tracks' ids (tracks by disc and position, or by title when the counts differ).
pub fn apply_release(c: &mut Connection, album: i64, r: &ReleaseFacts) -> Result<()> {
    let tx = c.transaction()?;
    set_external(&tx, album, "release", &r.id)?;
    if let Some(g) = &r.group_id {
        set_external(&tx, album, "release_group", g)?;
    }
    tx.execute(
        "INSERT INTO music(item_id, release_type, compilation, release_date) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(item_id) DO UPDATE SET
           release_type = coalesce(music.release_type, excluded.release_type),
           compilation = max(music.compilation, excluded.compilation),
           release_date = coalesce(music.release_date, excluded.release_date)",
        params![album, r.release_type, r.compilation, r.date],
    )?;
    let year: Option<i64> = r
        .date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok());
    tx.execute(
        "UPDATE item SET year = coalesce(year, ?2) WHERE id = ?1",
        params![album, year],
    )?;
    // Album artists by name.
    let artists: Vec<(i64, String)> = {
        let mut st = tx.prepare(
            "SELECT a.id, a.title FROM credit c JOIN item a ON a.id = c.artist_id WHERE c.item_id = ?1
             UNION SELECT p.id, p.title FROM item al JOIN item p ON p.id = al.parent_id WHERE al.id = ?1",
        )?;
        st.query_map([album], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    for (id, name) in &artists {
        if let Some((_, mb)) = r.artists.iter().find(|(n, _)| norm(n) == norm(name)) {
            set_external(&tx, *id, "artist", mb)?;
        }
    }
    // Tracks.
    let local: Vec<(i64, u32, u32, String)> = {
        let mut st = tx.prepare(
            "SELECT id, coalesce(season_no, 1), coalesce(episode_no, 0), coalesce(title, '') FROM item WHERE parent_id = ?1 AND kind = 'track'",
        )?;
        st.query_map([album], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let by_position = local.len() == r.tracks.len();
    for (id, disc, pos, title) in &local {
        let hit = r.tracks.iter().find(|(d, p, t, _)| {
            if by_position {
                d == disc && p == pos
            } else {
                norm(t) == norm(title)
            }
        });
        if let Some((_, _, _, rec)) = hit {
            set_external(&tx, *id, "recording", rec)?;
        }
    }
    tx.execute(
        "UPDATE item SET match_state = 'matched', meta_fetched_at = ?2 WHERE id = ?1",
        params![album, now_ms()],
    )?;
    tx.commit()?;
    Ok(())
}

/// One candidate for "Review matches": the release id, its title, artist, year, score and track
/// count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u16>,
    pub score: f64,
    pub tracks: Option<u32>,
}

/// Not certain: the album waits for the user's choice, with these candidates. Its tags stay.
pub fn set_review(c: &mut Connection, album: i64, cands: &[Candidate]) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute(
        "UPDATE item SET match_state = 'needs_review', meta_fetched_at = ?2 WHERE id = ?1",
        params![album, now_ms()],
    )?;
    tx.execute("DELETE FROM match_candidate WHERE item_id = ?1", [album])?;
    for k in cands {
        let about = match k.tracks {
            Some(n) => format!("{} · {n} tracks", k.artist),
            None => k.artist.clone(),
        };
        tx.execute(
            "INSERT INTO match_candidate(item_id, provider_id, title, year, score, overview) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![album, k.id, k.title, k.year, k.score, about],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Nothing found: tried again after a week.
pub fn set_unmatched(c: &Connection, album: i64) -> Result<()> {
    c.execute(
        "UPDATE item SET match_state = 'unmatched', meta_fetched_at = ?2 WHERE id = ?1",
        params![album, now_ms()],
    )?;
    Ok(())
}
