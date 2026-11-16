//! Offline copies (issue #3): one `local_copy` row per copied file. "Save" copies live in the
//! app's managed storage and are tracked until deleted; "Save as…" exports are tracked only
//! while they run and until the user dismisses them. Sidecar subtitles and the other parts of
//! a multi-part file are rows of their own whose `main_id` points at the main file's row.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Result;
use crate::catalog::ABS_PATH_SQL;

pub const SAVED: &str = "saved";
pub const EXPORT: &str = "export";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Copy {
    pub id: i64,
    /// `saved` (managed storage) or `export` (Save as…).
    pub kind: String,
    pub main_id: Option<i64>,
    pub file_id: Option<i64>,
    pub item_id: Option<i64>,
    pub source_id: i64,
    /// Absolute path inside the source.
    pub source_path: String,
    pub src_mtime: Option<i64>,
    /// The local file (without `.part`).
    pub path: String,
    pub size: u64,
    pub bytes_done: u64,
    /// queued, running, paused, done, failed, outdated
    pub state: String,
    pub error: String,
    /// The error category (auth, unreachable, not_found, unsupported, internal, no_space).
    pub error_kind: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

const COLS: &str = "id, kind, main_id, file_id, item_id, source_id, source_path, src_mtime, path, size, \
                    bytes_done, state, coalesce(error, ''), coalesce(error_kind, ''), created_at, completed_at";

fn row(r: &Row<'_>) -> rusqlite::Result<Copy> {
    Ok(Copy {
        id: r.get(0)?,
        kind: r.get(1)?,
        main_id: r.get(2)?,
        file_id: r.get(3)?,
        item_id: r.get(4)?,
        source_id: r.get(5)?,
        source_path: r.get(6)?,
        src_mtime: r.get(7)?,
        path: r.get(8)?,
        size: r.get::<_, i64>(9)?.max(0) as u64,
        bytes_done: r.get::<_, i64>(10)?.max(0) as u64,
        state: r.get(11)?,
        error: r.get(12)?,
        error_kind: r.get(13)?,
        created_at: r.get(14)?,
        completed_at: r.get(15)?,
    })
}

fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A new row, queued.
#[derive(Clone, Debug, Default)]
pub struct NewCopy {
    pub kind: String,
    pub main_id: Option<i64>,
    pub file_id: Option<i64>,
    pub item_id: Option<i64>,
    pub source_id: i64,
    pub source_path: String,
    pub src_mtime: Option<i64>,
    pub path: String,
    pub size: u64,
}

pub fn insert(c: &Connection, n: &NewCopy) -> Result<i64> {
    c.execute(
        "INSERT INTO local_copy(kind, main_id, file_id, item_id, source_id, source_path, src_mtime, path, size, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            n.kind,
            n.main_id,
            n.file_id,
            n.item_id,
            n.source_id,
            n.source_path,
            n.src_mtime,
            n.path,
            n.size as i64,
            now_s()
        ],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn get(c: &Connection, id: i64) -> Result<Option<Copy>> {
    Ok(c.query_row(
        &format!("SELECT {COLS} FROM local_copy WHERE id = ?1"),
        [id],
        row,
    )
    .optional()?)
}

fn query(c: &Connection, filter: &str, args: impl rusqlite::Params) -> Result<Vec<Copy>> {
    let mut st = c.prepare(&format!(
        "SELECT {COLS} FROM local_copy {filter} ORDER BY id"
    ))?;
    let rows = st.query_map(args, row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Every row, main files and their companions.
pub fn all(c: &Connection) -> Result<Vec<Copy>> {
    query(c, "", [])
}

/// The companions (sidecars, other parts) of a main row.
pub fn companions(c: &Connection, main_id: i64) -> Result<Vec<Copy>> {
    query(c, "WHERE main_id = ?1", [main_id])
}

/// Main rows of saved copies for these items (movies or episodes).
pub fn saved_for_item(c: &Connection, item_id: i64) -> Result<Vec<Copy>> {
    query(
        c,
        "WHERE kind = 'saved' AND main_id IS NULL AND item_id = ?1",
        [item_id],
    )
}

/// Rows that still have work to do (after a restart: queue them again).
pub fn unfinished(c: &Connection) -> Result<Vec<Copy>> {
    query(c, "WHERE state IN ('queued','running')", [])
}

pub fn set_state(c: &Connection, id: i64, state: &str) -> Result<()> {
    c.execute(
        "UPDATE local_copy SET state = ?2, error = CASE WHEN ?2 IN ('queued','running') THEN NULL ELSE error END,
                error_kind = CASE WHEN ?2 IN ('queued','running') THEN NULL ELSE error_kind END
         WHERE id = ?1",
        params![id, state],
    )?;
    Ok(())
}

pub fn set_failed(c: &Connection, id: i64, kind: &str, error: &str) -> Result<()> {
    c.execute(
        "UPDATE local_copy SET state = 'failed', error = ?2, error_kind = ?3 WHERE id = ?1",
        params![id, error, kind],
    )?;
    Ok(())
}

pub fn set_progress(c: &Connection, id: i64, bytes_done: u64) -> Result<()> {
    c.execute(
        "UPDATE local_copy SET bytes_done = ?2 WHERE id = ?1",
        params![id, bytes_done as i64],
    )?;
    Ok(())
}

/// The source file is not the one the row was made for: start again with its new size.
pub fn restart(c: &Connection, id: i64, size: u64, mtime: Option<i64>) -> Result<()> {
    c.execute(
        "UPDATE local_copy SET size = ?2, src_mtime = ?3, bytes_done = 0 WHERE id = ?1",
        params![id, size as i64, mtime],
    )?;
    Ok(())
}

pub fn set_done(c: &Connection, id: i64) -> Result<()> {
    c.execute(
        "UPDATE local_copy SET state = 'done', bytes_done = size, error = NULL, error_kind = NULL,
                completed_at = ?2 WHERE id = ?1",
        params![id, now_s()],
    )?;
    Ok(())
}

/// Remove a row and its companions (the files are the caller's business).
pub fn delete(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM local_copy WHERE id = ?1", [id])?;
    Ok(())
}

/// The local file of a complete, current saved copy of a library file, if there is one.
pub fn usable_path(c: &Connection, file_id: i64) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT path FROM local_copy WHERE file_id = ?1 AND kind = 'saved' AND state = 'done'
             ORDER BY id DESC LIMIT 1",
        [file_id],
        |r| r.get(0),
    )
    .optional()?)
}

/// The saved row (any state) for a library file, if one exists.
pub fn saved_for_file(c: &Connection, file_id: i64) -> Result<Option<Copy>> {
    Ok(c
        .query_row(
            &format!(
                "SELECT {COLS} FROM local_copy WHERE file_id = ?1 AND kind = 'saved' ORDER BY id DESC LIMIT 1"
            ),
            [file_id],
            row,
        )
        .optional()?)
}

/// Is a local path already taken by another row?
pub fn path_taken(c: &Connection, path: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM local_copy WHERE path = ?1 LIMIT 1",
        [path],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

// ---- what to copy -------------------------------------------------------------------------

/// A library file as the engine needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub file_id: i64,
    pub item_id: i64,
    pub source_id: i64,
    pub source_kind: String,
    /// Absolute path inside the source.
    pub path: String,
    pub size: u64,
    pub mtime: Option<i64>,
    pub fingerprint: Option<Vec<u8>>,
}

pub fn target(c: &Connection, file_id: i64) -> Result<Option<Target>> {
    Ok(c
        .query_row(
            &format!(
                "SELECT f.id, f.item_id, f.source_id, s.kind, {ABS_PATH_SQL}, coalesce(f.size, 0), f.mtime, f.fingerprint
                 FROM file f JOIN source s ON s.id = f.source_id WHERE f.id = ?1 AND f.item_id IS NOT NULL"
            ),
            [file_id],
            |r| {
                Ok(Target {
                    file_id: r.get(0)?,
                    item_id: r.get(1)?,
                    source_id: r.get(2)?,
                    source_kind: r.get(3)?,
                    path: r.get(4)?,
                    size: r.get::<_, i64>(5)?.max(0) as u64,
                    mtime: r.get(6)?,
                    fingerprint: r.get(7)?,
                })
            },
        )
        .optional()?)
}

/// The network files of an item, largest first (the version a season or show save takes).
pub fn network_files(c: &Connection, item_id: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT f.id FROM file f JOIN source s ON s.id = f.source_id
         WHERE f.item_id = ?1 AND s.kind != 'local' ORDER BY f.size DESC, f.id",
    )?;
    let rows = st.query_map([item_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Movies and episodes under an item: itself, a season's episodes, a show's episodes (in
/// season and episode order).
pub fn playable_items(c: &Connection, item_id: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "WITH RECURSIVE t(id) AS (SELECT ?1 UNION ALL SELECT i.id FROM item i JOIN t ON i.parent_id = t.id)
         SELECT i.id FROM item i JOIN t ON t.id = i.id
         WHERE i.kind IN ('movie','episode','video')
           AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id)
         ORDER BY coalesce((SELECT season_no FROM item p WHERE p.id = i.parent_id), 0), coalesce(i.episode_no, 0), i.id",
    )?;
    let rows = st.query_map([item_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The folder a saved copy goes into, relative to the storage directory:
/// `Title (Year)` for movies, `Show (Year)/Season 01` for episodes.
pub fn layout_dir(c: &Connection, item_id: i64) -> Result<Vec<String>> {
    let (kind, title, year, season, parent): (
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    ) = c
        .query_row(
            "SELECT kind, title, year, season_no, parent_id FROM item WHERE id = ?1",
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .unwrap_or_else(|| ("video".into(), None, None, None, None));
    let named = |t: Option<String>, y: Option<i64>| {
        let t = t.unwrap_or_default();
        let t = if t.trim().is_empty() {
            "Unknown".to_string()
        } else {
            t
        };
        match y {
            Some(y) if y > 0 => format!("{t} ({y})"),
            _ => t,
        }
    };
    if kind != "episode" {
        return Ok(vec![named(title, year)]);
    }
    // episode -> season -> show
    let (season_no, show) = match parent {
        Some(p) => c
            .query_row(
                "SELECT s.season_no, s.parent_id FROM item s WHERE s.id = ?1",
                [p],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .unwrap_or((season, None)),
        None => (season, None),
    };
    let show_name = match show {
        Some(s) => c
            .query_row("SELECT title, year FROM item WHERE id = ?1", [s], |r| {
                Ok(named(r.get(0)?, r.get(1)?))
            })
            .optional()?
            .unwrap_or_else(|| "Unknown".into()),
        None => named(title, year),
    };
    Ok(vec![
        show_name,
        format!("Season {:02}", season_no.unwrap_or(0)),
    ])
}

// ---- the state of an item's saved copies (issue #4) ----------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveInfo {
    /// none, queued, running, paused, done, failed or outdated. For a season or show: the
    /// busiest state of its episodes' copies, `done` once every episode that can be saved is.
    pub state: String,
    /// Movies and episodes: the main row of the saved copy (0 = none).
    pub copy_id: i64,
    /// Bytes copied and total, subtitles and parts included (seasons, shows: all copies).
    pub bytes_done: u64,
    pub size: u64,
    pub error: String,
    pub error_kind: String,
    /// Episodes (or the movie) with a complete saved copy, and those that can be saved at all
    /// (they have a file on a network source).
    pub saved: i64,
    pub saveable: i64,
}

/// The state of one main row, its companions included.
fn family_state(c: &Connection, main: &Copy) -> Result<(String, u64, u64, String, String)> {
    let mut state = main.state.clone();
    let (mut done, mut size) = (main.bytes_done, main.size);
    let (mut error, mut kind) = (main.error.clone(), main.error_kind.clone());
    for o in companions(c, main.id)? {
        done += o.bytes_done;
        size += o.size;
        if state == "done" && matches!(o.state.as_str(), "queued" | "running" | "failed") {
            state = o.state.clone();
        }
        if error.is_empty() && !o.error.is_empty() {
            error = o.error;
            kind = o.error_kind;
        }
    }
    Ok((state, done, size, error, kind))
}

pub fn save_info(c: &Connection, item_id: i64) -> Result<SaveInfo> {
    let items = playable_items(c, item_id)?;
    let single = items.len() == 1 && items[0] == item_id;
    let mut info = SaveInfo {
        state: "none".into(),
        ..Default::default()
    };
    let mut states = Vec::new();
    for it in items {
        if !network_files(c, it)?.is_empty() {
            info.saveable += 1;
        }
        // The newest saved copy of the item (another version may have been saved before).
        let Some(main) = saved_for_item(c, it)?.into_iter().last() else {
            continue;
        };
        let (state, done, size, error, kind) = family_state(c, &main)?;
        if single {
            info.copy_id = main.id;
        }
        if state == "done" {
            info.saved += 1;
        }
        info.bytes_done += done;
        info.size += size;
        if info.error.is_empty() && !error.is_empty() && state == "failed" {
            info.error = error;
            info.error_kind = kind;
        }
        states.push(state);
    }
    let has = |s: &str| states.iter().any(|x| x == s);
    info.state = if has("running") {
        "running"
    } else if has("queued") {
        "queued"
    } else if has("paused") {
        "paused"
    } else if has("failed") {
        "failed"
    } else if has("outdated") {
        "outdated"
    } else if info.saved > 0 && info.saved >= info.saveable {
        "done"
    } else {
        "none"
    }
    .into();
    Ok(info)
}

// ---- settings -----------------------------------------------------------------------------

pub fn setting(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(
        c.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?,
    )
}

pub fn set_setting(c: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => c.execute(
            "INSERT INTO setting(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, v],
        )?,
        None => c.execute("DELETE FROM setting WHERE key = ?1", [key])?,
    };
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
            "INSERT INTO source(id, kind, name, root) VALUES (2, 'smb', 'NAS', '/Movies');
             INSERT INTO item(id, kind, title, year) VALUES (1, 'movie', 'Alien', 1979);
             INSERT INTO item(id, kind, title, year) VALUES (10, 'show', 'Firefly', 2002);
             INSERT INTO item(id, kind, parent_id, season_no) VALUES (11, 'season', 10, 1);
             INSERT INTO item(id, kind, parent_id, episode_no, title) VALUES (13, 'episode', 11, 2, 'The Train Job');
             INSERT INTO item(id, kind, parent_id, episode_no, title) VALUES (12, 'episode', 11, 1, 'Serenity');
             INSERT INTO file(id, source_id, path, size, mtime, item_id) VALUES
               (100, 2, 'Alien (1979)/Alien.mkv', 50, 7, 1),
               (101, 2, 'Firefly/S01E01.mkv', 10, 7, 12),
               (102, 2, 'Firefly/S01E02.mkv', 10, 7, 13);",
        )
        .unwrap();
        c
    }

    fn saved(c: &Connection, file: i64, item: i64) -> i64 {
        let id = insert(
            c,
            &NewCopy {
                kind: SAVED.into(),
                file_id: Some(file),
                item_id: Some(item),
                source_id: 2,
                source_path: "/x".into(),
                path: format!("/store/{file}.mkv"),
                size: 50,
                ..Default::default()
            },
        )
        .unwrap();
        set_done(c, id).unwrap();
        id
    }

    #[test]
    fn layout_and_items() {
        let c = db();
        assert_eq!(layout_dir(&c, 1).unwrap(), ["Alien (1979)"]);
        assert_eq!(layout_dir(&c, 12).unwrap(), ["Firefly (2002)", "Season 01"]);
        assert_eq!(playable_items(&c, 10).unwrap(), [12, 13]);
        assert_eq!(playable_items(&c, 1).unwrap(), [1]);
        let t = target(&c, 100).unwrap().unwrap();
        assert_eq!(
            (t.path.as_str(), t.size),
            ("/Movies/Alien (1979)/Alien.mkv", 50)
        );
        assert_eq!(network_files(&c, 1).unwrap(), [100]);
    }

    #[test]
    fn changed_or_removed_source_files_make_copies_outdated() {
        let c = db();
        let a = saved(&c, 100, 1);
        let b = saved(&c, 101, 12);
        let sub = insert(
            &c,
            &NewCopy {
                kind: SAVED.into(),
                main_id: Some(a),
                source_id: 2,
                source_path: "/x.srt".into(),
                path: "/store/x.srt".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            usable_path(&c, 100).unwrap().as_deref(),
            Some("/store/100.mkv")
        );
        // a move keeps the copy; a change in place does not
        c.execute(
            "UPDATE file SET path = 'Alien/Alien.mkv', mtime = 9 WHERE id = 100",
            [],
        )
        .unwrap();
        assert_eq!(get(&c, a).unwrap().unwrap().state, "done");
        c.execute("UPDATE file SET size = 51 WHERE id = 100", [])
            .unwrap();
        assert_eq!(get(&c, a).unwrap().unwrap().state, "outdated");
        assert_eq!(usable_path(&c, 100).unwrap(), None);
        // the source file is gone: outdated, and the row stays
        c.execute("DELETE FROM file WHERE id = 101", []).unwrap();
        let gone = get(&c, b).unwrap().unwrap();
        assert_eq!((gone.state.as_str(), gone.file_id), ("outdated", None));
        // deleting a main row takes its companions along
        assert_eq!(companions(&c, a).unwrap().len(), 1);
        delete(&c, a).unwrap();
        assert!(get(&c, sub).unwrap().is_none());
    }

    #[test]
    fn save_info_sums_up_seasons_and_shows() {
        let c = db();
        assert_eq!(save_info(&c, 1).unwrap().state, "none");
        assert_eq!(save_info(&c, 1).unwrap().saveable, 1);
        let a = saved(&c, 101, 12);
        let s = save_info(&c, 11).unwrap();
        assert_eq!((s.state.as_str(), s.saved, s.saveable), ("none", 1, 2));
        let b = insert(
            &c,
            &NewCopy {
                kind: SAVED.into(),
                file_id: Some(102),
                item_id: Some(13),
                source_id: 2,
                source_path: "/y".into(),
                path: "/store/y.mkv".into(),
                size: 10,
                ..Default::default()
            },
        )
        .unwrap();
        set_state(&c, b, "running").unwrap();
        set_progress(&c, b, 4).unwrap();
        let s = save_info(&c, 10).unwrap();
        assert_eq!(
            (s.state.as_str(), s.bytes_done, s.size),
            ("running", 54, 60)
        );
        set_done(&c, b).unwrap();
        assert_eq!(save_info(&c, 10).unwrap().state, "done");
        let e = save_info(&c, 12).unwrap();
        assert_eq!((e.state.as_str(), e.copy_id), ("done", a));
        // the grid's mark and filter
        let q = crate::catalog::Query {
            kind: "show".into(),
            parent: None,
            sort: crate::catalog::Sort::Title,
            unwatched_only: false,
            saved_only: true,
            offset: 0,
            limit: 10,
        };
        let (n, items) = crate::catalog::browse(&c, &q).unwrap();
        assert_eq!((n, items[0].saved), (1, true));
        let q = crate::catalog::Query {
            kind: "movie".into(),
            ..q
        };
        assert_eq!(crate::catalog::browse(&c, &q).unwrap().0, 0);
    }

    #[test]
    fn settings_round_trip() {
        let c = db();
        assert_eq!(setting(&c, "storage_dir").unwrap(), None);
        set_setting(&c, "storage_dir", Some("/a")).unwrap();
        set_setting(&c, "storage_dir", Some("/b")).unwrap();
        assert_eq!(setting(&c, "storage_dir").unwrap().as_deref(), Some("/b"));
        set_setting(&c, "storage_dir", None).unwrap();
        assert_eq!(setting(&c, "storage_dir").unwrap(), None);
    }
}
