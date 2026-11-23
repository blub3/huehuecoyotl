//! Editing the tags of existing music files (3.0, issue #157, SPEC 7): which files a scope holds,
//! and how the library follows a successful write. Files are never renamed or moved.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::Result;
use crate::text::{normalize, sort_title};

/// One file of the scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditFile {
    pub file: i64,
    pub track: i64,
    pub album: i64,
    pub source: i64,
    pub path: String,
    pub size: i64,
    pub mtime: i64,
}

/// What a scope is called, for its switch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scopes {
    pub track: Option<i64>,
    pub track_title: String,
    pub album: Option<i64>,
    pub album_title: String,
    pub album_files: i64,
    pub artist: Option<i64>,
    pub artist_name: String,
    pub artist_albums: i64,
}

fn title(c: &Connection, id: i64) -> Result<String> {
    Ok(c.query_row(
        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?)
}

fn albums_of(c: &Connection, artist: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT DISTINCT a.id FROM item a LEFT JOIN credit cr ON cr.item_id = a.id AND cr.role = 'album_artist'
         WHERE a.kind = 'album' AND (a.parent_id = ?1 OR cr.artist_id = ?1) ORDER BY a.id",
    )?;
    let rows = st.query_map([artist], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The scopes around an item: its song, album and artist.
pub fn scopes(c: &Connection, item: i64) -> Result<Scopes> {
    let (kind, parent): (String, Option<i64>) = c.query_row(
        "SELECT kind, parent_id FROM item WHERE id = ?1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (track, album, artist) = match kind.as_str() {
        "track" => {
            let a = parent;
            let ar = match a {
                Some(a) => c.query_row("SELECT parent_id FROM item WHERE id = ?1", [a], |r| {
                    r.get(0)
                })?,
                None => None,
            };
            (Some(item), a, ar)
        }
        "album" => (None, Some(item), parent),
        "artist" => (None, None, Some(item)),
        _ => (None, None, None),
    };
    let mut s = Scopes {
        track,
        album,
        artist,
        ..Default::default()
    };
    if let Some(t) = track {
        s.track_title = title(c, t)?;
    }
    if let Some(a) = album {
        s.album_title = title(c, a)?;
        s.album_files = files(c, "album", a)?.len() as i64;
    }
    if let Some(a) = artist {
        s.artist_name = title(c, a)?;
        s.artist_albums = albums_of(c, a)?.len() as i64;
    }
    Ok(s)
}

/// Every file of a song, an album or all of an artist.
pub fn files(c: &Connection, scope: &str, id: i64) -> Result<Vec<EditFile>> {
    let tracks: Vec<i64> = match scope {
        "track" => vec![id],
        "album" => tracks_of(c, id)?,
        "artist" => {
            let mut out = Vec::new();
            for a in albums_of(c, id)? {
                out.extend(tracks_of(c, a)?);
            }
            out
        }
        _ => vec![],
    };
    let mut st = c.prepare_cached(
        "SELECT f.id, t.id, t.parent_id, f.source_id, f.path, coalesce(f.size, 0), coalesce(f.mtime, 0)
         FROM file f JOIN item t ON t.id = f.item_id WHERE t.id = ?1 ORDER BY f.path",
    )?;
    let mut out = Vec::new();
    for t in tracks {
        let rows = st.query_map([t], |r| {
            Ok(EditFile {
                file: r.get(0)?,
                track: r.get(1)?,
                album: r.get(2)?,
                source: r.get(3)?,
                path: r.get(4)?,
                size: r.get(5)?,
                mtime: r.get(6)?,
            })
        })?;
        out.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(out)
}

fn tracks_of(c: &Connection, album: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare_cached(
        "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'track'
         ORDER BY coalesce(season_no, 1), coalesce(episode_no, 0), id",
    )?;
    let rows = st.query_map([album], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The fields a scope may change. A song's album fields belong to its album, and a whole artist
/// has no single album or disc.
pub fn allowed(scope: &str) -> &'static [&'static str] {
    match scope {
        "track" => &["title", "artist", "genre"],
        "album" => &["albumArtist", "artist", "album", "year", "genre", "disc"],
        _ => &["albumArtist", "artist", "genre"],
    }
}

/// The artist with this name: found by its folded key, or a new one.
fn artist_named(c: &Connection, name: &str, now: i64) -> Result<i64> {
    let key = crate::artists::key(name);
    let found: Option<i64> = c
        .query_row(
            "SELECT id FROM item WHERE kind = 'artist' AND group_key = ?1 ORDER BY id LIMIT 1",
            [&key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        // The same artist, spelled anew in the files. A name the user chose, or MusicBrainz's
        // spelling of a matched artist, stays what the app shows (issue #148).
        let (choice, mb): (Option<String>, Option<String>) = c.query_row(
            "SELECT name_choice, mb_title FROM item WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        c.execute(
            "UPDATE item SET files_title = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        let shows_files = choice.as_deref() == Some("files") || (choice.is_none() && mb.is_none());
        if shows_files {
            rename(c, id, name)?;
        }
        return Ok(id);
    }
    c.execute(
        "INSERT INTO item(kind, title, sort_title, match_state, added_at, group_key, files_title)
         VALUES ('artist', ?1, ?2, 'unmatched', ?3, ?4, ?1)",
        params![name, sort_title(name), now, key],
    )?;
    let id = c.last_insert_rowid();
    c.execute(
        "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
        params![id, name],
    )?;
    Ok(id)
}

fn set_credit(c: &Connection, item: i64, role: &str, artists: &[i64]) -> Result<()> {
    c.execute(
        "DELETE FROM credit WHERE item_id = ?1 AND role = ?2",
        params![item, role],
    )?;
    let n = artists.len();
    for (i, a) in artists.iter().enumerate() {
        let join = if i + 1 == n {
            ""
        } else if i + 2 == n {
            " & "
        } else {
            ", "
        };
        c.execute(
            "INSERT OR REPLACE INTO credit(item_id, artist_id, role, position, join_phrase) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![item, a, role, i as i64, join],
        )?;
    }
    Ok(())
}

fn rename(c: &Connection, item: i64, name: &str) -> Result<()> {
    c.execute(
        "UPDATE item SET title = ?2, sort_title = ?3 WHERE id = ?1",
        params![item, name, sort_title(name)],
    )?;
    c.execute(
        "UPDATE item_fts SET title = ?2 WHERE rowid = ?1",
        params![item, name],
    )?;
    Ok(())
}

fn rekey(c: &Connection, track: i64) -> Result<()> {
    let (disc, name, no): (i64, String, Option<i64>) = c.query_row(
        "SELECT coalesce(season_no, 1), coalesce(title, ''), episode_no FROM item WHERE id = ?1",
        [track],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    c.execute(
        "UPDATE item SET group_key = ?2 WHERE id = ?1",
        params![track, crate::versions::track_key(disc, &name, no)],
    )?;
    Ok(())
}

/// What the user changed, field by field; `None` stays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub title: Option<String>,
    pub artist: Option<Vec<String>>,
    pub album_artist: Option<Vec<String>>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub disc: Option<u32>,
}

/// The library follows the files that were written: the same items, with their play counts and
/// playlists, under the new values. `written` holds the files that succeeded, each with its new
/// size and time so the next scan sees them as unchanged.
pub fn apply(
    c: &mut Connection,
    written: &[(EditFile, i64, i64)],
    ch: &Changes,
    now: i64,
) -> Result<()> {
    let tx = c.transaction()?;
    let mut tracks = BTreeSet::new();
    let mut albums = BTreeSet::new();
    for (f, size, mtime) in written {
        tx.execute(
            "UPDATE file SET size = ?2, mtime = ?3, fingerprint = NULL WHERE id = ?1",
            params![f.file, size, mtime],
        )?;
        tracks.insert(f.track);
        albums.insert(f.album);
    }
    let artists = match &ch.artist {
        Some(names) => names
            .iter()
            .map(|n| artist_named(&tx, n, now))
            .collect::<Result<Vec<_>>>()?,
        None => vec![],
    };
    let album_artists = match &ch.album_artist {
        Some(names) => names
            .iter()
            .map(|n| artist_named(&tx, n, now))
            .collect::<Result<Vec<_>>>()?,
        None => vec![],
    };
    for t in &tracks {
        if let Some(name) = &ch.title {
            rename(&tx, *t, name)?;
        }
        if !artists.is_empty() {
            set_credit(&tx, *t, "artist", &artists)?;
        }
        if let Some(g) = &ch.genre {
            tx.execute(
                "UPDATE music SET genre = ?2 WHERE item_id = ?1",
                params![t, g],
            )?;
        }
        if let Some(d) = ch.disc {
            tx.execute(
                "UPDATE item SET season_no = ?2 WHERE id = ?1",
                params![t, d],
            )?;
            tx.execute(
                "UPDATE music SET disc_no = ?2 WHERE item_id = ?1",
                params![t, d],
            )?;
        }
        if ch.title.is_some() || ch.disc.is_some() {
            rekey(&tx, *t)?;
        }
    }
    for a in &albums {
        if let Some(name) = &ch.album {
            if normalize(name) != normalize(&title(&tx, *a)?) || !name.is_empty() {
                rename(&tx, *a, name)?;
            }
            tx.execute(
                "UPDATE album_version SET title = ?2 WHERE album_id = ?1",
                params![a, name],
            )?;
        }
        if let Some(y) = &ch.year {
            let year: Option<i64> = y.get(..4).and_then(|v| v.parse().ok());
            tx.execute("UPDATE item SET year = ?2 WHERE id = ?1", params![a, year])?;
            tx.execute(
                "UPDATE album_version SET year = ?2 WHERE album_id = ?1",
                params![a, year],
            )?;
            tx.execute(
                "UPDATE music SET release_date = ?2 WHERE item_id = ?1",
                params![a, y],
            )?;
        }
        if let Some(g) = &ch.genre {
            tx.execute(
                "UPDATE music SET genre = ?2 WHERE item_id = ?1",
                params![a, g],
            )?;
        }
        if !album_artists.is_empty() {
            set_credit(&tx, *a, "album_artist", &album_artists)?;
            tx.execute(
                "UPDATE item SET parent_id = ?2 WHERE id = ?1",
                params![a, album_artists[0]],
            )?;
        }
    }
    // An artist nothing refers to any more leaves.
    tx.execute_batch(
        "DELETE FROM item WHERE kind = 'artist'
           AND NOT EXISTS (SELECT 1 FROM item a WHERE a.parent_id = item.id)
           AND NOT EXISTS (SELECT 1 FROM credit c WHERE c.artist_id = item.id);
         DELETE FROM item_fts WHERE rowid NOT IN (SELECT id FROM item);",
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::migrate(&mut c).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'M', '/m', 'music');
             INSERT INTO item(id, kind, title, sort_title, group_key) VALUES (1, 'artist', 'Rosalia', 'r', 'artist:rosalia');
             INSERT INTO item(id, kind, title, sort_title, parent_id, year) VALUES
               (2, 'album', 'Lux', 'l', 1, 2025), (3, 'album', 'Other', 'o', 1, 2020);
             INSERT INTO item(id, kind, title, sort_title, parent_id, season_no, episode_no, group_key) VALUES
               (20, 'track', 'One', 'o', 2, 1, 1, 't:1|one'), (21, 'track', 'Two', 't', 2, 1, 2, 't:1|two'),
               (30, 'track', 'Else', 'e', 3, 1, 1, 't:1|else');
             INSERT INTO music(item_id, disc_no, track_no, genre) VALUES (2, NULL, NULL, 'Pop'), (20, 1, 1, 'Pop'), (21, 1, 2, 'Pop'), (30, 1, 1, 'Rock');
             INSERT INTO credit(item_id, artist_id, role, position) VALUES (2, 1, 'album_artist', 0), (3, 1, 'album_artist', 0), (20, 1, 'artist', 0), (21, 1, 'artist', 0), (30, 1, 'artist', 0);
             INSERT INTO file(id, source_id, path, size, mtime, item_id) VALUES
               (200, 2, 'l/1.flac', 10, 5, 20), (201, 2, 'l/2.flac', 10, 5, 21), (300, 2, 'o/1.flac', 10, 5, 30);
             INSERT INTO playstate(item_id, profile_id, updated_at, play_count) VALUES (20, 1, 1, 9);",
        )
        .unwrap();
        c
    }

    #[test]
    fn scopes_and_their_files() {
        let c = db();
        let s = scopes(&c, 20).unwrap();
        assert_eq!((s.track, s.album, s.artist), (Some(20), Some(2), Some(1)));
        assert_eq!((s.album_files, s.artist_albums), (2, 2));
        assert_eq!(files(&c, "track", 20).unwrap().len(), 1);
        assert_eq!(files(&c, "album", 2).unwrap().len(), 2);
        assert_eq!(files(&c, "artist", 1).unwrap().len(), 3);
        assert!(!allowed("track").contains(&"album"));
        assert!(!allowed("artist").contains(&"album"));
    }

    #[test]
    fn the_library_follows_with_the_same_items() {
        let mut c = db();
        let f = files(&c, "album", 2).unwrap();
        let written: Vec<_> = f.iter().map(|x| (x.clone(), 11, 6)).collect();
        let ch = Changes {
            album: Some("LUX".into()),
            album_artist: Some(vec!["Rosalía Vila".into()]),
            artist: Some(vec!["Rosalía Vila".into()]),
            year: Some("2025-11-07".into()),
            ..Default::default()
        };
        apply(&mut c, &written, &ch, 100).unwrap();
        let (t, y, p): (String, i64, i64) = c
            .query_row(
                "SELECT title, year, parent_id FROM item WHERE id = 2",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((t.as_str(), y), ("LUX", 2025));
        assert_ne!(p, 1);
        let name: String = c
            .query_row("SELECT title FROM item WHERE id = ?1", [p], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Rosalía Vila");
        // The other album keeps the old artist, so it stays.
        assert_eq!(title(&c, 1).unwrap(), "Rosalia");
        let plays: i64 = c
            .query_row(
                "SELECT play_count FROM playstate WHERE item_id = 20",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(plays, 9);
        let (size, path): (i64, String) = c
            .query_row("SELECT size, path FROM file WHERE id = 200", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((size, path.as_str()), (11, "l/1.flac"), "never renamed");
    }

    #[test]
    fn a_new_song_title_keeps_the_track() {
        let mut c = db();
        let f = files(&c, "track", 20).unwrap();
        let ch = Changes {
            title: Some("Uno".into()),
            ..Default::default()
        };
        apply(&mut c, &[(f[0].clone(), 10, 7)], &ch, 1).unwrap();
        let key: String = c
            .query_row("SELECT group_key FROM item WHERE id = 20", [], |r| r.get(0))
            .unwrap();
        assert_eq!(key, "t:1|uno");
        assert_eq!(title(&c, 21).unwrap(), "Two");
    }
}
