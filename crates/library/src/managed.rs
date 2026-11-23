//! The managed library (3.0, issue #150, SPEC 6): a folder of the user's own copies, on this
//! computer or on a share. It is four ordinary sources, one per media type (`source.managed`), so
//! scans, playback and matching work as for any folder. An item stays while any of its files is
//! in the library, also when its original is deleted or its source removed.

use std::collections::{BTreeSet, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Result, catalog, copies, shares};

/// (key, folder in the library, content type of the source)
pub const TYPES: [(&str, &str, &str); 4] = [
    ("movies", "Movies", "video"),
    ("shows", "TV Shows", "video"),
    ("music", "Music", "music"),
    ("audiobooks", "Audiobooks", "audiobooks"),
];

const TYPES_SETTING: &str = "library_types";

pub const SCHEMA: &str = r#"
ALTER TABLE source ADD COLUMN managed TEXT;
ALTER TABLE source ADD COLUMN moving_to INTEGER;
CREATE UNIQUE INDEX source_managed ON source(managed) WHERE managed IS NOT NULL;
CREATE TABLE library_place(id INTEGER PRIMARY KEY CHECK (id = 1),
                           share_id INTEGER REFERENCES share(id),
                           root TEXT NOT NULL);
CREATE TABLE gone_original(id INTEGER PRIMARY KEY,
                           source_id INTEGER NOT NULL,
                           source_name TEXT NOT NULL,
                           item_id INTEGER REFERENCES item(id) ON DELETE SET NULL,
                           kind TEXT NOT NULL,
                           title TEXT NOT NULL,
                           gone_at INTEGER NOT NULL,
                           kept INTEGER NOT NULL);
CREATE INDEX gone_original_source ON gone_original(source_id);
CREATE INDEX gone_original_item ON gone_original(item_id);
"#;

/// Where the library lives: a folder here (`share` is `None`) or on a share.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub share: Option<i64>,
    pub root: String,
}

pub fn place(c: &Connection) -> Result<Option<Place>> {
    Ok(
        c.query_row("SELECT share_id, root FROM library_place", [], |r| {
            Ok(Place {
                share: r.get(0)?,
                root: r.get(1)?,
            })
        })
        .optional()?,
    )
}

/// `root`/`folder`, for a share's `/`-rooted paths and local ones alike.
pub fn join(root: &str, folder: &str) -> String {
    format!("{}/{folder}", root.trim_end_matches('/'))
}

/// A source (other than the library's own) in, around or at this place: two folders scanning the
/// same files would make every item twice.
pub fn overlapping(c: &Connection, p: &Place) -> Result<Option<String>> {
    let mut st = c.prepare(
        "SELECT name, coalesce(root, ''), share_id FROM source
         WHERE root IS NOT NULL AND managed IS NULL AND moving_to IS NULL",
    )?;
    let rows: Vec<(String, String, Option<i64>)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let root = p.root.trim_end_matches('/');
    let within = |a: &str, b: &str| a == b || a.starts_with(&format!("{b}/"));
    Ok(rows
        .into_iter()
        .find(|(_, r, s)| {
            let r = r.trim_end_matches('/');
            *s == p.share && (within(root, r) || within(r, root))
        })
        .map(|(n, ..)| n))
}

/// Make `p` the library. Its four folders become the managed sources. With `move_copies` the old
/// ones wait to be emptied (`moving_to`); otherwise they stay as ordinary folders. Returns the
/// new sources.
pub fn install(c: &mut Connection, p: &Place, move_copies: bool) -> Result<Vec<i64>> {
    let tx = c.transaction()?;
    let old: Vec<(i64, String)> = {
        let mut st = tx.prepare("SELECT id, managed FROM source WHERE managed IS NOT NULL")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    tx.execute(
        "UPDATE source SET managed = NULL WHERE managed IS NOT NULL",
        [],
    )?;
    let mut ids = Vec::new();
    for (key, folder, content) in TYPES {
        let root = join(&p.root, folder);
        let name = format!("Your library: {folder}");
        let id = match p.share {
            Some(s) => shares::add_folder(&tx, s, &root, content, &name)?.0,
            None => catalog::add_library(&tx, &name, &root, content)?,
        };
        tx.execute(
            "UPDATE source SET managed = ?2, name = ?3 WHERE id = ?1",
            params![id, key, name],
        )?;
        if move_copies && let Some((o, _)) = old.iter().find(|(_, k)| k == key) {
            tx.execute(
                "UPDATE source SET moving_to = ?2 WHERE id = ?1",
                params![o, id],
            )?;
        }
        ids.push(id);
    }
    tx.execute("DELETE FROM library_place", [])?;
    tx.execute(
        "INSERT INTO library_place(id, share_id, root) VALUES (1, ?1, ?2)",
        params![p.share, p.root],
    )?;
    tx.commit()?;
    Ok(ids)
}

/// The library's copies, also those still on their way: (files, bytes).
pub fn copies(c: &Connection) -> Result<(i64, i64)> {
    Ok(c.query_row(
        "SELECT count(*), coalesce(sum(f.size), 0) FROM file f JOIN source s ON s.id = f.source_id
         WHERE s.managed IS NOT NULL OR s.moving_to IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// The library's four sources.
pub fn sources(c: &Connection) -> Result<Vec<i64>> {
    let mut st = c.prepare("SELECT id FROM source WHERE managed IS NOT NULL ORDER BY id")?;
    let rows = st.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ---- settings per media type ----------------------------------------------------------------

pub fn type_settings(c: &Connection) -> Result<Value> {
    let stored: Value = copies::setting(c, TYPES_SETTING)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut out = serde_json::Map::new();
    for (key, ..) in TYPES {
        let s = &stored[key];
        out.insert(
            key.to_string(),
            json!({
                "enabled": s["enabled"].as_bool().unwrap_or(true),
                "import": if s["import"] == "auto" { "auto" } else { "manual" },
                "watch": s["watch"].as_array().cloned().unwrap_or_default(),
                "writeTags": s["writeTags"].as_bool().unwrap_or(true),
            }),
        );
    }
    Ok(Value::Object(out))
}

/// Change one type's settings; fields not named stay.
pub fn set_type(c: &Connection, key: &str, changes: &Value) -> Result<()> {
    let mut all = type_settings(c)?;
    if let (Some(t), Some(ch)) = (all.get_mut(key), changes.as_object()) {
        for f in ["enabled", "import", "watch", "writeTags"] {
            if let Some(v) = ch.get(f) {
                t[f] = v.clone();
            }
        }
    }
    copies::set_setting(c, TYPES_SETTING, Some(&all.to_string()))
}

// ---- moving the copies ----------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveFile {
    pub file: i64,
    pub from: i64,
    pub to: i64,
    pub rel: String,
    pub size: i64,
}

pub fn next_move(c: &Connection) -> Result<Option<MoveFile>> {
    Ok(c.query_row(
        "SELECT f.id, s.id, s.moving_to, f.path, coalesce(f.size, 0) FROM file f
         JOIN source s ON s.id = f.source_id WHERE s.moving_to IS NOT NULL ORDER BY f.id LIMIT 1",
        [],
        |r| {
            Ok(MoveFile {
                file: r.get(0)?,
                from: r.get(1)?,
                to: r.get(2)?,
                rel: r.get(3)?,
                size: r.get(4)?,
            })
        },
    )
    .optional()?)
}

/// (files still to move, their bytes).
pub fn move_left(c: &Connection) -> Result<(i64, i64)> {
    Ok(c.query_row(
        "SELECT count(*), coalesce(sum(f.size), 0) FROM file f JOIN source s ON s.id = f.source_id
         WHERE s.moving_to IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// A checked copy is at the new place: the file row (with its item, play state and saved copies)
/// now points there.
pub fn file_moved(c: &Connection, m: &MoveFile) -> Result<()> {
    c.execute(
        "UPDATE file SET source_id = ?2 WHERE id = ?1",
        params![m.file, m.to],
    )?;
    c.execute(
        "UPDATE local_copy SET source_id = ?2 WHERE file_id = ?1",
        params![m.file, m.to],
    )?;
    Ok(())
}

/// Old library folders with nothing left hand their album versions over and go. Returns the
/// sources that went.
pub fn finish_moves(c: &mut Connection) -> Result<Vec<i64>> {
    let tx = c.transaction()?;
    let done: Vec<(i64, i64)> = {
        let mut st = tx.prepare(
            "SELECT id, moving_to FROM source s WHERE moving_to IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM file f WHERE f.source_id = s.id)",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (from, to) in &done {
        tx.execute(
            "UPDATE album_version SET source_id = ?2,
                key = 'v:' || ?2 || substr(key, length('v:' || ?1) + 1)
             WHERE source_id = ?1",
            params![from, to],
        )?;
        tx.execute("DELETE FROM source WHERE id = ?1", [from])?;
    }
    tx.commit()?;
    Ok(done.into_iter().map(|(f, _)| f).collect())
}

/// Scans of a folder wait while copies are arriving in it: they would add the files twice.
pub fn is_move_target(c: &Connection, source: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS (SELECT 1 FROM source WHERE moving_to = ?1)",
        [source],
        |r| r.get(0),
    )?)
}

/// A library folder, or one still being emptied: not removed like other folders.
pub fn is_library_source(c: &Connection, source: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS (SELECT 1 FROM source WHERE id = ?1 AND (managed IS NOT NULL OR moving_to IS NOT NULL))",
        [source],
        |r| r.get(0),
    )?)
}

// ---- originals that went --------------------------------------------------------------------

/// What the user thinks of as one thing: the album of a track, the show of an episode.
fn top_item(c: &Connection, item: i64) -> Result<i64> {
    let mut id = item;
    loop {
        let (kind, parent): (String, Option<i64>) = c.query_row(
            "SELECT kind, parent_id FROM item WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        match (kind.as_str(), parent) {
            ("track" | "episode" | "season", Some(p)) => id = p,
            _ => return Ok(id),
        }
    }
}

const FILES_OF: &str = "WITH RECURSIVE t(id) AS (SELECT ?1 UNION ALL SELECT i.id FROM item i JOIN t ON i.parent_id = t.id)
    SELECT DISTINCT f.id, f.source_id, s.managed IS NOT NULL OR s.moving_to IS NOT NULL
    FROM file f JOIN source s ON s.id = f.source_id
    WHERE f.item_id IN t OR f.id IN (SELECT file_id FROM book_file WHERE book_id IN t)";

/// (file, its source, in the library) for everything under an album, show, movie or book.
fn files_of(c: &Connection, top: i64) -> Result<Vec<(i64, i64, bool)>> {
    let mut st = c.prepare_cached(FILES_OF)?;
    let rows = st.query_map([top], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn tops_of(c: &Connection, files: &[i64]) -> Result<BTreeSet<i64>> {
    let mut out = BTreeSet::new();
    for f in files {
        let item: Option<i64> = c
            .query_row(
                "SELECT coalesce(f.item_id, (SELECT book_id FROM book_file b WHERE b.file_id = f.id))
                 FROM file f WHERE f.id = ?1",
                [f],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if let Some(i) = item {
            out.insert(top_item(c, i)?);
        }
    }
    Ok(out)
}

/// Before `gone` files of a source are deleted: remember the albums, shows, movies and books that
/// no longer are in that folder, and whether the library keeps them. Nothing without a library.
pub fn record_gone(c: &Connection, source: i64, gone: &[i64], now: i64) -> Result<()> {
    if gone.is_empty() || place(c)?.is_none() || is_library_source(c, source)? {
        return Ok(());
    }
    let source_name: String =
        c.query_row("SELECT name FROM source WHERE id = ?1", [source], |r| {
            r.get(0)
        })?;
    let set: HashSet<i64> = gone.iter().copied().collect();
    for top in tops_of(c, gone)? {
        let files = files_of(c, top)?;
        // Only part of it went: it is still in this folder.
        if files
            .iter()
            .any(|(f, s, _)| *s == source && !set.contains(f))
        {
            continue;
        }
        let kept = files.iter().any(|(_, _, lib)| *lib);
        let (kind, title): (String, String) = c.query_row(
            "SELECT kind, coalesce(title, '') FROM item WHERE id = ?1",
            [top],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        c.execute(
            "DELETE FROM gone_original WHERE source_id = ?1 AND item_id = ?2",
            params![source, top],
        )?;
        c.execute(
            "INSERT INTO gone_original(source_id, source_name, item_id, kind, title, gone_at, kept)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![source, source_name, top, kind, title, now, kept],
        )?;
    }
    Ok(())
}

/// What left a folder: (total, kept in the library, gone from the app).
pub fn gone_counts(c: &Connection, source: i64) -> Result<(i64, i64, i64)> {
    Ok(c.query_row(
        "SELECT count(*), coalesce(sum(kept), 0), coalesce(sum(kept = 0), 0)
         FROM gone_original WHERE source_id = ?1",
        [source],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?)
}

pub fn gone_list(c: &Connection, source: i64) -> Result<Vec<Value>> {
    let mut st = c.prepare(
        "SELECT item_id, kind, title, gone_at, kept FROM gone_original
         WHERE source_id = ?1 ORDER BY kept DESC, title",
    )?;
    let rows = st.query_map([source], |r| {
        Ok(json!({
            "id": r.get::<_, Option<i64>>(0)?,
            "kind": r.get::<_, String>(1)?,
            "title": r.get::<_, String>(2)?,
            "goneAt": r.get::<_, i64>(3)?,
            "kept": r.get::<_, bool>(4)?,
        }))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// What removing a source would do: (stay, because the library has them; leave the app).
pub fn remove_preview(c: &Connection, source: i64) -> Result<(i64, i64)> {
    let files: Vec<i64> = {
        let mut st = c.prepare("SELECT id FROM file WHERE source_id = ?1")?;
        st.query_map([source], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let (mut stay, mut leave) = (0, 0);
    for top in tops_of(c, &files)? {
        let rest = files_of(c, top)?;
        if rest.iter().any(|(_, s, lib)| *lib && *s != source) {
            stay += 1;
        } else if !rest.iter().any(|(_, s, _)| *s != source) {
            leave += 1;
        }
    }
    Ok((stay, leave))
}

/// "In your library only": the item's files are all in the library and an original went.
pub fn item_origin(c: &Connection, item: i64) -> Result<Value> {
    let top = match c
        .query_row("SELECT id FROM item WHERE id = ?1", [item], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
    {
        Some(_) => top_item(c, item)?,
        None => return Ok(json!({ "libraryOnly": false })),
    };
    let files = files_of(c, top)?;
    if files.is_empty() || files.iter().any(|(_, _, lib)| !*lib) {
        return Ok(json!({ "libraryOnly": false }));
    }
    let gone = c
        .query_row(
            "SELECT source_name, gone_at FROM gone_original WHERE item_id = ?1 AND kept = 1
             ORDER BY gone_at DESC LIMIT 1",
            [top],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;
    Ok(match gone {
        Some((from, at)) => json!({ "libraryOnly": true, "from": from, "goneAt": at }),
        None => json!({ "libraryOnly": false }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    fn album(c: &Connection, id: i64, source: i64, file: i64) {
        c.execute_batch(&format!(
            "INSERT OR IGNORE INTO item(id, kind, title, sort_title) VALUES (1, 'artist', 'Band', 'band');
             INSERT OR IGNORE INTO item(id, kind, title, sort_title, parent_id) VALUES ({id}, 'album', 'Album {id}', 'a', 1);
             INSERT OR IGNORE INTO item(id, kind, title, sort_title, parent_id) VALUES ({id}0, 'track', 'T', 't', {id});
             INSERT INTO file(id, source_id, path, size, item_id) VALUES ({file}, {source}, 'f{file}.flac', 10, {id}0);"
        ))
        .unwrap();
    }

    fn library(c: &mut Connection) -> Vec<i64> {
        install(
            c,
            &Place {
                share: None,
                root: "/lib".into(),
            },
            false,
        )
        .unwrap()
    }

    #[test]
    fn a_library_is_four_managed_folders() {
        let mut c = db();
        let ids = library(&mut c);
        assert_eq!(ids.len(), 4);
        let rows: Vec<(String, String, String)> = c
            .prepare(
                "SELECT managed, root, content FROM source WHERE managed IS NOT NULL ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows[1],
            ("shows".into(), "/lib/TV Shows".into(), "video".into())
        );
        assert_eq!(rows[3].2, "audiobooks");
        set_type(&c, "music", &json!({ "import": "auto", "watch": ["/in"] })).unwrap();
        let t = type_settings(&c).unwrap();
        assert_eq!(t["music"]["import"], "auto");
        assert_eq!(t["movies"]["import"], "manual");
        assert_eq!(t["music"]["writeTags"], true);
    }

    #[test]
    fn folders_that_overlap_are_found() {
        let c = db();
        catalog::add_library(&c, "Films", "/data/films", "video").unwrap();
        let at = |root: &str| Place {
            share: None,
            root: root.into(),
        };
        assert!(overlapping(&c, &at("/data/films")).unwrap().is_some());
        assert!(overlapping(&c, &at("/data/films/lib")).unwrap().is_some());
        assert!(overlapping(&c, &at("/data")).unwrap().is_some());
        assert!(overlapping(&c, &at("/data/films2")).unwrap().is_none());
    }

    #[test]
    fn moving_keeps_the_rows_and_ends_with_the_old_folder_gone() {
        let mut c = db();
        let old = library(&mut c);
        album(&c, 5, old[2], 50);
        c.execute(
            "INSERT INTO album_version(id, album_id, key, source_id) VALUES (9, 5, ?1, ?2)",
            params![format!("v:{}:dir:x", old[2]), old[2]],
        )
        .unwrap();
        let new = install(
            &mut c,
            &Place {
                share: None,
                root: "/other".into(),
            },
            true,
        )
        .unwrap();
        assert!(is_move_target(&c, new[2]).unwrap());
        assert_eq!(move_left(&c).unwrap(), (1, 10));
        let m = next_move(&c).unwrap().unwrap();
        assert_eq!((m.from, m.to), (old[2], new[2]));
        // Not finished while a file is left.
        assert!(!finish_moves(&mut c).unwrap().contains(&old[2]));
        file_moved(&c, &m).unwrap();
        assert!(finish_moves(&mut c).unwrap().contains(&old[2]));
        assert!(next_move(&c).unwrap().is_none());
        let (src, key): (i64, String) = c
            .query_row(
                "SELECT source_id, key FROM album_version WHERE id = 9",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((src, key), (new[2], format!("v:{}:dir:x", new[2])));
    }

    #[test]
    fn starting_empty_keeps_the_old_folders_as_sources() {
        let mut c = db();
        let old = library(&mut c);
        album(&c, 5, old[2], 50);
        install(
            &mut c,
            &Place {
                share: None,
                root: "/other".into(),
            },
            false,
        )
        .unwrap();
        let (managed, moving): (Option<String>, Option<i64>) = c
            .query_row(
                "SELECT managed, moving_to FROM source WHERE id = ?1",
                [old[2]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((managed, moving), (None, None));
    }

    #[test]
    fn originals_go_and_library_copies_stay() {
        let mut c = db();
        c.execute(
            "INSERT INTO source(id, kind, name, root, content) VALUES (7, 'local', 'NAS', '/nas', 'music')",
            [],
        )
        .unwrap();
        album(&c, 5, 7, 50);
        album(&c, 6, 7, 60);
        let lib = library(&mut c);
        // Album 5 also has a library copy.
        c.execute(
            "INSERT INTO file(id, source_id, path, size, item_id) VALUES (51, ?1, 'a.flac', 10, 50)",
            [lib[2]],
        )
        .unwrap();
        assert_eq!(remove_preview(&c, 7).unwrap(), (1, 1));
        record_gone(&c, 7, &[50, 60], 1000).unwrap();
        assert_eq!(gone_counts(&c, 7).unwrap(), (2, 1, 1));
        c.execute("DELETE FROM file WHERE id IN (50, 60)", [])
            .unwrap();
        let o = item_origin(&c, 50).unwrap();
        assert_eq!(o["libraryOnly"], true);
        assert_eq!(o["from"], "NAS");
        assert_eq!(o["goneAt"], 1000);
        assert_eq!(item_origin(&c, 6).unwrap()["libraryOnly"], false);
    }

    #[test]
    fn nothing_is_recorded_without_a_library() {
        let c = db();
        c.execute(
            "INSERT INTO source(id, kind, name, root, content) VALUES (7, 'local', 'NAS', '/nas', 'music')",
            [],
        )
        .unwrap();
        album(&c, 5, 7, 50);
        record_gone(&c, 7, &[50], 1).unwrap();
        assert_eq!(gone_counts(&c, 7).unwrap(), (0, 0, 0));
    }
}
