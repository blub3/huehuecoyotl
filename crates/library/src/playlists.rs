//! Playlists (2.6, issue #81): smart playlists that fill themselves from the library, and the
//! user's own. An own playlist keeps, like the play queue, each song with the file chosen for
//! it; a file that is gone leaves the song, which then plays another version.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::queue::Track;

/// The smart playlists, in the order the Playlists screen shows them.
pub const SMART: [&str; 6] = [
    "hires",
    "most_played",
    "recently_added",
    "not_played",
    "saved",
    "forgotten",
];

const DAY_MS: i64 = 24 * 3600 * 1000;

/// Songs with a file somewhere (what `browse` shows as tracks).
const PLAYABLE: &str = "i.kind = 'track' AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id AND f.source_id != 1)";
const QUALITY: &str = "(SELECT max(CASE WHEN fa.codec LIKE 'dsd%' THEN 3000000000 + coalesce(fa.sample_rate, 0)
    WHEN fa.lossless = 1 THEN 2000000000 + coalesce(fa.sample_rate, 0) * 100 + coalesce(fa.bit_depth, 0)
    ELSE coalesce(fa.bitrate, 0) END) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = i.id)";
const HIRES: &str = "EXISTS (SELECT 1 FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = i.id
    AND fa.lossless = 1 AND (coalesce(fa.bit_depth, 0) > 16 OR coalesce(fa.sample_rate, 0) > 48000))";

/// The WHERE and ORDER BY of a smart playlist, and how many songs it holds at most.
fn smart_sql(key: &str, now: i64) -> Option<(String, String, u32)> {
    let album_added = "(SELECT a.added_at FROM item a WHERE a.id = i.parent_id)";
    Some(match key {
        "hires" => (HIRES.to_string(), format!("{QUALITY} DESC, i.sort_title"), 1000),
        "most_played" => (
            "coalesce(p.play_count, 0) > 0".into(),
            "p.play_count DESC, p.last_played_at DESC".into(),
            50,
        ),
        "recently_added" => (
            format!("{album_added} >= {}", now - 30 * DAY_MS),
            format!("{album_added} DESC, i.parent_id, i.id"),
            1000,
        ),
        "not_played" => (
            "coalesce(p.play_count, 0) = 0".into(),
            "i.sort_title".into(),
            5000,
        ),
        "saved" => (
            "EXISTS (SELECT 1 FROM local_copy lc WHERE lc.item_id = i.id AND lc.kind = 'saved' AND lc.state = 'done')".into(),
            "i.parent_id, i.id".into(),
            5000,
        ),
        "forgotten" => (
            format!(
                "coalesce(p.play_count, 0) >= 5 AND coalesce(p.last_played_at, 0) < {}",
                now - 182 * DAY_MS
            ),
            "p.play_count DESC".into(),
            100,
        ),
        _ => return None,
    })
}

/// A smart playlist's songs, in order (no chosen files: each plays its preferred version).
pub fn smart_tracks(c: &Connection, key: &str, now: i64) -> Result<Vec<i64>> {
    let Some((filter, order, cap)) = smart_sql(key, now) else {
        return Ok(vec![]);
    };
    let mut st = c.prepare(&format!(
        "SELECT i.id FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = 1
         WHERE {PLAYABLE} AND {filter} ORDER BY {order} LIMIT {cap}"
    ))?;
    let rows = st.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A playlist as the Playlists screen lists it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// "smart:<key>" or the own playlist's id as text.
    pub key: String,
    pub id: i64,
    /// Own playlists only; smart ones are named by the UI from `smart`.
    pub name: String,
    pub smart: String,
    pub count: i64,
    pub duration_ms: i64,
    /// Up to four album covers (grid tier), for the mosaic.
    pub covers: Vec<String>,
}

/// One song of a playlist.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub position: i64,
    pub item_id: i64,
    pub file_id: Option<i64>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_id: i64,
    pub duration_ms: i64,
    pub format: String,
    pub hires: bool,
}

/// Details of songs, in the order of `tracks` (positions from 0).
fn entries_for(c: &Connection, tracks: &[Track]) -> Result<Vec<Entry>> {
    let mut st = c.prepare_cached(
        "SELECT coalesce(i.title, ''), coalesce(al.title, ''), coalesce(i.parent_id, 0),
           (SELECT group_concat(a.title, ', ') FROM credit cr JOIN item a ON a.id = cr.artist_id
             WHERE cr.item_id = i.id AND cr.role = 'artist'),
           (SELECT coalesce(fa.codec, '') || '|' || coalesce(fa.bit_depth, 0) || '|' || coalesce(fa.sample_rate, 0)
              || '|' || coalesce(fa.bitrate, 0) || '|' || coalesce(fa.lossless, 0) || '|' || coalesce(fa.duration_ms, 0)
            FROM file f JOIN file_audio fa ON fa.file_id = f.id
            WHERE f.item_id = i.id AND (?2 IS NULL OR f.id = ?2)
            ORDER BY fa.lossless DESC, coalesce(fa.bit_depth, 0) DESC, coalesce(fa.sample_rate, 0) DESC LIMIT 1)
         FROM item i LEFT JOIN item al ON al.id = i.parent_id WHERE i.id = ?1",
    )?;
    let mut out = Vec::with_capacity(tracks.len());
    for (pos, (item, file)) in tracks.iter().enumerate() {
        let row = st
            .query_row(params![item, file], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .optional()?;
        let Some((title, album, album_id, artist, raw)) = row else {
            continue;
        };
        let raw = raw.unwrap_or_default();
        let (format, hires) = crate::catalog::format_label(&raw);
        let duration_ms = raw
            .rsplit('|')
            .next()
            .and_then(|d| d.parse().ok())
            .unwrap_or(0);
        out.push(Entry {
            position: pos as i64,
            item_id: *item,
            file_id: *file,
            title,
            artist: artist.unwrap_or_default(),
            album,
            album_id,
            duration_ms,
            format,
            hires,
        });
    }
    Ok(out)
}

/// An own playlist's songs with their chosen files, in order.
pub fn tracks(c: &Connection, playlist: i64) -> Result<Vec<Track>> {
    let mut st = c.prepare(
        "SELECT item_id, file_id FROM playlist_entry WHERE playlist_id = ?1 ORDER BY position",
    )?;
    let rows = st.query_map([playlist], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The songs of a playlist ("smart:<key>" or an id), each with its file or none.
pub fn resolve(c: &Connection, key: &str, now: i64) -> Result<Vec<Track>> {
    if let Some(k) = key.strip_prefix("smart:") {
        return Ok(smart_tracks(c, k, now)?
            .into_iter()
            .map(|t| (t, None))
            .collect());
    }
    match key.parse::<i64>() {
        Ok(id) => tracks(c, id),
        Err(_) => Ok(vec![]),
    }
}

/// A playlist's songs with details; `limit` caps the list (the preview shows a few).
pub fn entries(c: &Connection, key: &str, now: i64, limit: usize) -> Result<Vec<Entry>> {
    let t = resolve(c, key, now)?;
    let n = t.len().min(limit);
    entries_for(c, &t[..n])
}

fn card(c: &Connection, key: &str, now: i64) -> Result<Card> {
    let t = resolve(c, key, now)?;
    let ids: Vec<i64> = t.iter().map(|(i, _)| *i).collect();
    let mut duration = 0i64;
    let mut covers = Vec::new();
    let mut st = c.prepare_cached(
        "SELECT coalesce((SELECT max(coalesce(fa.duration_ms, 0)) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = i.id), 0),
                (SELECT local_path FROM artwork w WHERE w.item_id = i.parent_id AND w.role = 'poster_grid' AND w.local_path IS NOT NULL)
         FROM item i WHERE i.id = ?1",
    )?;
    for id in &ids {
        let (d, cover): (i64, Option<String>) =
            st.query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        duration += d;
        if let Some(cv) = cover
            && covers.len() < 4
            && !covers.contains(&cv)
        {
            covers.push(cv);
        }
    }
    let (id, name, smart) = match key.strip_prefix("smart:") {
        Some(k) => (0, String::new(), k.to_string()),
        None => {
            let id: i64 = key.parse().unwrap_or(0);
            let name: String = c
                .query_row("SELECT name FROM playlist WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?
                .unwrap_or_default();
            (id, name, String::new())
        }
    };
    Ok(Card {
        key: key.to_string(),
        id,
        name,
        smart,
        count: ids.len() as i64,
        duration_ms: duration,
        covers,
    })
}

/// Every playlist: the smart ones in their order, then the user's by name.
pub fn list(c: &Connection, now: i64) -> Result<(Vec<Card>, Vec<Card>)> {
    let mut smart = Vec::new();
    for k in SMART {
        smart.push(card(c, &format!("smart:{k}"), now)?);
    }
    let ids: Vec<i64> = {
        let mut st = c.prepare("SELECT id FROM playlist ORDER BY lower(name), id")?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut mine = Vec::new();
    for id in ids {
        mine.push(card(c, &id.to_string(), now)?);
    }
    Ok((smart, mine))
}

pub fn create(c: &Connection, name: &str, now: i64) -> Result<i64> {
    let name = name.trim();
    let name = if name.is_empty() { "Playlist" } else { name };
    c.execute(
        "INSERT INTO playlist(name, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![name, now],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn rename(c: &Connection, id: i64, name: &str, now: i64) -> Result<()> {
    let name = name.trim();
    if !name.is_empty() {
        c.execute(
            "UPDATE playlist SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now],
        )?;
    }
    Ok(())
}

pub fn delete(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM playlist WHERE id = ?1", [id])?;
    Ok(())
}

/// Append songs with their files.
pub fn add(c: &mut Connection, id: i64, tracks: &[Track], now: i64) -> Result<()> {
    let tx = c.transaction()?;
    let next: i64 = tx.query_row(
        "SELECT coalesce(max(position) + 1, 0) FROM playlist_entry WHERE playlist_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    for (i, (item, file)) in tracks.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlist_entry(playlist_id, position, item_id, file_id) VALUES (?1, ?2, ?3, ?4)",
            params![id, next + i as i64, item, file],
        )?;
    }
    tx.execute(
        "UPDATE playlist SET updated_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    tx.commit()?;
    Ok(())
}

/// Rewrite a playlist's entries in a new order (positions from 0).
fn rewrite(c: &mut Connection, id: i64, tracks: &[Track], now: i64) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute("DELETE FROM playlist_entry WHERE playlist_id = ?1", [id])?;
    for (i, (item, file)) in tracks.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlist_entry(playlist_id, position, item_id, file_id) VALUES (?1, ?2, ?3, ?4)",
            params![id, i as i64, item, file],
        )?;
    }
    tx.execute(
        "UPDATE playlist SET updated_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn remove(c: &mut Connection, id: i64, index: usize, now: i64) -> Result<()> {
    let mut t = tracks(c, id)?;
    if index < t.len() {
        t.remove(index);
        rewrite(c, id, &t, now)?;
    }
    Ok(())
}

pub fn move_entry(c: &mut Connection, id: i64, from: usize, to: usize, now: i64) -> Result<()> {
    let mut t = tracks(c, id)?;
    if from < t.len() && to < t.len() && from != to {
        let e = t.remove(from);
        t.insert(to, e);
        rewrite(c, id, &t, now)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'Music', '/m', 'music');
             INSERT INTO item(id, kind, title, sort_title, added_at) VALUES (10, 'album', 'Hi', 'hi', 1000), (20, 'album', 'Lo', 'lo', 0);
             INSERT INTO item(id, kind, title, sort_title, parent_id) VALUES
               (11, 'track', 'Hi One', 'hi one', 10), (12, 'track', 'Hi Two', 'hi two', 10), (21, 'track', 'Lo One', 'lo one', 20);
             INSERT INTO file(id, source_id, path, size, item_id) VALUES (110, 2, 'a', 1, 11), (111, 2, 'a2', 1, 11), (120, 2, 'b', 1, 12), (210, 2, 'c', 1, 21);
             INSERT INTO file_audio(file_id, codec, sample_rate, bit_depth, lossless, bitrate, duration_ms) VALUES
               (110, 'flac', 192000, 24, 1, 5000, 1000), (111, 'mp3', 44100, 0, 0, 320, 1000), (120, 'flac', 96000, 24, 1, 3000, 2000), (210, 'mp3', 44100, 0, 0, 256, 3000);
             INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, play_count, last_played_at)
               VALUES (21, 1, 0, 0, 0, 1, 7, 1), (12, 1, 0, 0, 0, 1, 2, 50000000000);",
        )
        .unwrap();
        c
    }

    #[test]
    fn smart_playlists() {
        let c = db();
        let now = 50_000_000_000;
        assert_eq!(smart_tracks(&c, "hires", now).unwrap(), [11, 12]);
        assert_eq!(smart_tracks(&c, "most_played", now).unwrap(), [21, 12]);
        assert_eq!(smart_tracks(&c, "not_played", now).unwrap(), [11]);
        // Lo One: played 7 times, last long ago.
        assert_eq!(smart_tracks(&c, "forgotten", now).unwrap(), [21]);
        assert!(smart_tracks(&c, "saved", now).unwrap().is_empty());
        assert!(smart_tracks(&c, "recently_added", now).unwrap().is_empty());
        let (smart, mine) = list(&c, now).unwrap();
        assert_eq!(smart.len(), 6);
        assert_eq!(
            (
                smart[0].smart.as_str(),
                smart[0].count,
                smart[0].duration_ms
            ),
            ("hires", 2, 3000)
        );
        assert!(mine.is_empty());
    }

    #[test]
    fn own_playlists_keep_their_files() {
        let mut c = db();
        let id = create(&c, "  Late drive ", 1).unwrap();
        add(&mut c, id, &[(21, None), (11, Some(111)), (12, None)], 2).unwrap();
        let e = entries(&c, &id.to_string(), 3, 10).unwrap();
        let t: Vec<(&str, &str)> = e
            .iter()
            .map(|e| (e.title.as_str(), e.format.as_str()))
            .collect();
        // The MP3 copy of Hi One was chosen and stays.
        assert_eq!(
            t,
            [
                ("Lo One", "MP3 256k"),
                ("Hi One", "MP3 320k"),
                ("Hi Two", "FLAC 24/96")
            ]
        );
        move_entry(&mut c, id, 2, 0, 4).unwrap();
        remove(&mut c, id, 1, 5).unwrap();
        assert_eq!(tracks(&c, id).unwrap(), [(12, None), (11, Some(111))]);
        rename(&c, id, "Night", 6).unwrap();
        let (_, mine) = list(&c, 7).unwrap();
        assert_eq!((mine[0].name.as_str(), mine[0].count), ("Night", 2));
        // The chosen file goes away: the song stays, without a file.
        c.execute("DELETE FROM file WHERE id = 111", []).unwrap();
        assert_eq!(tracks(&c, id).unwrap(), [(12, None), (11, None)]);
        delete(&c, id).unwrap();
        assert!(list(&c, 8).unwrap().1.is_empty());
    }
}
