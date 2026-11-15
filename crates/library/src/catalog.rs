//! Library queries for the UI (SPEC 8, rule 3: paged view models sized for one screen) and the
//! writes the metadata jobs make.

use rusqlite::{Connection, OptionalExtension, params};

use crate::queries::{DEFAULT_PROFILE, PlayState};
use crate::{LOCAL_SOURCE_ID, Result};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// SQL for a file's absolute path: library sources store paths relative to their root.
pub const ABS_PATH_SQL: &str =
    "CASE WHEN s.root IS NULL THEN f.path ELSE s.root || '/' || f.path END";

// ---- library folders ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct LibraryFolder {
    pub id: i64,
    /// `local`, or a network protocol (`smb`, `nfs`, ...).
    pub kind: String,
    pub name: String,
    /// Local: the absolute folder. Network: the folder inside the share ("" = its root).
    pub root: String,
    /// Network sources: how to reach them, without the password.
    pub config_json: String,
    pub files: i64,
    pub last_scan: Option<i64>,
}

pub fn libraries(c: &Connection) -> Result<Vec<LibraryFolder>> {
    let mut st = c.prepare(
        "SELECT s.id, s.name, s.root, (SELECT count(*) FROM file f WHERE f.source_id = s.id), s.last_scan,
                s.kind, s.config_json
         FROM source s WHERE s.root IS NOT NULL ORDER BY s.name",
    )?;
    let rows = st.query_map([], |r| {
        Ok(LibraryFolder {
            id: r.get(0)?,
            name: r.get(1)?,
            root: r.get(2)?,
            files: r.get(3)?,
            last_scan: r.get(4)?,
            kind: r.get(5)?,
            config_json: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Add a library folder. Returns the existing id if the folder is already a library.
pub fn add_library(c: &Connection, name: &str, root: &str) -> Result<i64> {
    let root = root.trim_end_matches('/');
    if let Some(id) = c
        .query_row(
            "SELECT id FROM source WHERE kind = 'local' AND root = ?1",
            [root],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    c.execute(
        "INSERT INTO source(kind, name, root) VALUES ('local', ?1, ?2)",
        params![name, root],
    )?;
    Ok(c.last_insert_rowid())
}

/// Add a network library: `kind` is the protocol, `root` the folder inside the share.
/// Returns the existing id if the same place is already a library.
pub fn add_network_library(
    c: &Connection,
    kind: &str,
    name: &str,
    config_json: &str,
    root: &str,
) -> Result<i64> {
    let root = root.trim_end_matches('/');
    if let Some(id) = c
        .query_row(
            "SELECT id FROM source WHERE kind = ?1 AND config_json = ?2 AND root = ?3",
            params![kind, config_json, root],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    c.execute(
        "INSERT INTO source(kind, name, config_json, root) VALUES (?1, ?2, ?3, ?4)",
        params![kind, name, config_json, root],
    )?;
    let id = c.last_insert_rowid();
    c.execute(
        "UPDATE source SET credential_ref = ?2 WHERE id = ?1",
        params![id, format!("source:{id}")],
    )?;
    Ok(id)
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceRow {
    pub id: i64,
    pub kind: String,
    pub config_json: String,
    pub credential_ref: Option<String>,
    pub root: Option<String>,
}

pub fn source(c: &Connection, id: i64) -> Result<Option<SourceRow>> {
    Ok(c.query_row(
        "SELECT id, kind, config_json, credential_ref, root FROM source WHERE id = ?1",
        [id],
        |r| {
            Ok(SourceRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                config_json: r.get(2)?,
                credential_ref: r.get(3)?,
                root: r.get(4)?,
            })
        },
    )
    .optional()?)
}

/// Store a network source's updated configuration (an SFTP host key seen for the first time).
pub fn set_source_config(c: &Connection, id: i64, config_json: &str) -> Result<()> {
    c.execute(
        "UPDATE source SET config_json = ?2 WHERE id = ?1",
        params![id, config_json],
    )?;
    Ok(())
}

/// The item of a file given by its path inside a source.
pub fn item_for_source_path(c: &Connection, source_id: i64, abs_path: &str) -> Result<Option<i64>> {
    let root: Option<String> = c
        .query_row("SELECT root FROM source WHERE id = ?1", [source_id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    let rel = match &root {
        Some(r) => match abs_path.strip_prefix(&format!("{r}/")) {
            Some(rel) => rel,
            None => return Ok(None),
        },
        None => abs_path,
    };
    Ok(c.query_row(
        "SELECT item_id FROM file WHERE source_id = ?1 AND path = ?2",
        params![source_id, rel],
        |r| r.get(0),
    )
    .optional()?
    .flatten())
}

/// Remove a library folder and everything only it provided. Play state of items that disappear
/// goes with them.
pub fn remove_library(c: &mut Connection, id: i64) -> Result<()> {
    let tx = c.transaction()?;
    // Queued scans of this folder must not run against a later folder that reuses the id.
    tx.execute(
        "DELETE FROM job WHERE kind = 'scan' AND json_extract(payload_json, '$.source_id') = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM file WHERE source_id = ?1", [id])?;
    tx.execute(
        "DELETE FROM source WHERE id = ?1 AND id != ?2",
        params![id, LOCAL_SOURCE_ID],
    )?;
    tx.execute_batch(
        "DELETE FROM item WHERE kind IN ('movie','episode','video') AND NOT EXISTS (SELECT 1 FROM file f WHERE f.item_id = item.id);
         DELETE FROM item WHERE kind = 'season' AND NOT EXISTS (SELECT 1 FROM item e WHERE e.parent_id = item.id);
         DELETE FROM item WHERE kind = 'show' AND NOT EXISTS (SELECT 1 FROM item s WHERE s.parent_id = item.id);
         DELETE FROM item_fts WHERE rowid NOT IN (SELECT id FROM item);",
    )?;
    tx.commit()?;
    Ok(())
}

/// If `abs_path` lies inside a library folder, the file's item (so the folder browser and the
/// library share one play state).
pub fn library_item_for_path(c: &Connection, abs_path: &str) -> Result<Option<i64>> {
    let roots: Vec<(i64, String)> = {
        let mut st =
            c.prepare("SELECT id, root FROM source WHERE kind = 'local' AND root IS NOT NULL")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, root) in roots {
        if let Some(rel) = abs_path.strip_prefix(&format!("{root}/")) {
            let item = c
                .query_row(
                    "SELECT item_id FROM file WHERE source_id = ?1 AND path = ?2",
                    params![id, rel],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if item.is_some() {
                return Ok(item);
            }
        }
    }
    Ok(None)
}

// ---- view models -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub year: Option<i64>,
    pub season_no: Option<i64>,
    pub episode_no: Option<i64>,
    pub poster: String,
    /// The 540 px grid tier (`poster_grid_l`); empty until it exists, then QML may prefer it.
    pub poster_large: String,
    pub progress: f32,
    pub watched: bool,
    /// Shows and seasons: episodes not yet watched.
    pub unwatched: i64,
    pub overview: String,
    /// Episodes: a file is in the library. Seasons: at least one episode is. Episodes and
    /// seasons only TMDB knows are shown dimmed and cannot be played.
    pub on_disk: bool,
    pub runtime_min: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Title,
    Added,
    Year,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// "movie", "show", or a parent's children when `parent` is set.
    pub kind: String,
    pub parent: Option<i64>,
    pub sort: Sort,
    pub unwatched_only: bool,
    pub offset: u32,
    pub limit: u32,
}

const SUMMARY_COLS: &str = "i.id, i.kind, i.title, i.year, i.season_no, i.episode_no,
    (SELECT local_path FROM artwork a WHERE a.item_id = i.id AND a.role IN ('poster_grid','still') AND local_path IS NOT NULL ORDER BY a.role LIMIT 1),
    p.position_ms, p.duration_ms, p.watched,
    CASE WHEN i.kind = 'show' THEN (SELECT count(*) FROM item s JOIN item e ON e.parent_id = s.id
           LEFT JOIN playstate pe ON pe.item_id = e.id AND pe.profile_id = 1
           WHERE s.parent_id = i.id AND s.season_no > 0 AND coalesce(pe.watched, 0) = 0
             AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = e.id))
         WHEN i.kind = 'season' THEN (SELECT count(*) FROM item e
           LEFT JOIN playstate pe ON pe.item_id = e.id AND pe.profile_id = 1
           WHERE e.parent_id = i.id AND coalesce(pe.watched, 0) = 0
             AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = e.id))
         ELSE 0 END,
    coalesce(i.overview, ''),
    (SELECT local_path FROM artwork a WHERE a.item_id = i.id AND a.role = 'poster_grid_l' AND local_path IS NOT NULL LIMIT 1),
    CASE i.kind
      WHEN 'show' THEN 1
      WHEN 'season' THEN EXISTS (SELECT 1 FROM item e JOIN file f ON f.item_id = e.id WHERE e.parent_id = i.id)
      ELSE EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id) END,
    i.runtime_min";

fn summary(r: &rusqlite::Row) -> rusqlite::Result<Summary> {
    let st = PlayState {
        position_ms: r.get::<_, Option<i64>>(7)?.unwrap_or(0) as u64,
        duration_ms: r.get::<_, Option<i64>>(8)?.unwrap_or(0) as u64,
        watched: r.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
    };
    Ok(Summary {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        year: r.get(3)?,
        season_no: r.get(4)?,
        episode_no: r.get(5)?,
        poster: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        poster_large: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
        on_disk: r.get(13)?,
        runtime_min: r.get(14)?,
        progress: st.progress(),
        watched: st.watched,
        unwatched: r.get(10)?,
        overview: r.get(11)?,
    })
}

/// Items that have something playable: movies and episodes with files; shows and seasons with
/// at least one such episode.
const VISIBLE: &str = "(
    (i.kind IN ('movie','episode') AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id AND f.source_id != 1))
 OR (i.kind = 'season' AND EXISTS (SELECT 1 FROM item e JOIN file f ON f.item_id = e.id WHERE e.parent_id = i.id))
 OR (i.kind = 'show' AND EXISTS (SELECT 1 FROM item s JOIN item e ON e.parent_id = s.id JOIN file f ON f.item_id = e.id WHERE s.parent_id = i.id)))";

/// One page of items plus the total count.
pub fn browse(c: &Connection, q: &Query) -> Result<(u32, Vec<Summary>)> {
    let filter = match q.parent {
        Some(_) => "i.parent_id = ?1".to_string(),
        None => "i.kind = ?1 AND i.parent_id IS NULL".to_string(),
    };
    let unwatched = if q.unwatched_only {
        " AND coalesce(p.watched, 0) = 0"
    } else {
        ""
    };
    let order = match (q.parent.is_some(), q.sort) {
        (true, _) => "coalesce(i.season_no, 0) = 0, i.season_no, i.episode_no, i.sort_title",
        (false, Sort::Title) => "i.sort_title, i.year",
        (false, Sort::Added) => "i.added_at DESC, i.sort_title",
        (false, Sort::Year) => "i.year DESC, i.sort_title",
    };
    let from = format!(
        "FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE {filter} AND {VISIBLE}{unwatched}"
    );
    let key: rusqlite::types::Value = match q.parent {
        Some(p) => p.into(),
        None => q.kind.clone().into(),
    };
    let total: u32 = c.query_row(&format!("SELECT count(*) {from}"), [&key], |r| r.get(0))?;
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} {from} ORDER BY {order} LIMIT ?2 OFFSET ?3"
    ))?;
    let rows = st.query_map(params![key, q.limit, q.offset], summary)?;
    Ok((total, rows.collect::<rusqlite::Result<_>>()?))
}

/// Full-text search over movies and shows; every word is a prefix.
pub fn search(c: &Connection, text: &str, limit: u32) -> Result<Vec<Summary>> {
    let words: Vec<String> = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let fts = words.join(" ");
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} FROM item_fts JOIN item i ON i.id = item_fts.rowid
         LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE item_fts MATCH ?1 AND {VISIBLE} ORDER BY rank LIMIT ?2"
    ))?;
    let rows = st.query_map(params![fts, limit], summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn recently_added(c: &Connection, limit: u32) -> Result<Vec<Summary>> {
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.kind IN ('movie','show') AND {VISIBLE} ORDER BY i.added_at DESC LIMIT ?1"
    ))?;
    let rows = st.query_map([limit], summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileInfo {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub source_id: i64,
    pub source_name: String,
    /// local, smb, nfs, webdav, ftp, sftp
    pub source_kind: String,
    /// The media probe's result (issue #33), "" until the file is analysed.
    pub probe_json: String,
    /// Set by the core: whether the file's source can be reached right now.
    pub online: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    pub item: Summary,
    pub original_title: String,
    pub rating: Option<f64>,
    pub runtime_min: Option<i64>,
    pub meta_json: String,
    pub backdrop: String,
    pub poster_full: String,
    pub match_state: String,
    pub tmdb_id: Option<i64>,
    pub parent_title: String,
    pub files: Vec<FileInfo>,
    pub position_ms: u64,
    pub children: Vec<Summary>,
    /// For shows and seasons: the episode "Play" should start.
    pub next_up: Option<Summary>,
}

fn art(c: &Connection, item: i64, role: &str) -> Result<String> {
    Ok(c
        .query_row("SELECT local_path FROM artwork WHERE item_id = ?1 AND role = ?2 AND local_path IS NOT NULL", params![item, role], |r| r.get::<_, String>(0))
        .optional()?
        .unwrap_or_default())
}

pub fn detail(c: &Connection, id: i64) -> Result<Option<Detail>> {
    let Some(item) = c
        .query_row(
            &format!("SELECT {SUMMARY_COLS} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE} WHERE i.id = ?1"),
            [id],
            summary,
        )
        .optional()?
    else {
        return Ok(None);
    };
    type Extra = (
        Option<String>,
        Option<f64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let (original_title, rating, runtime_min, meta_json, match_state, tmdb_id, parent): Extra =
        c.query_row(
            "SELECT original_title, rating, runtime_min, meta_json, match_state, tmdb_id, parent_id FROM item WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )?;
    // Backdrop and full poster: the item's own, else the show's (for seasons and episodes).
    let show_id: Option<i64> = match item.kind.as_str() {
        "season" => parent,
        "episode" => c
            .query_row(
                "SELECT parent_id FROM item WHERE id = ?1",
                [parent.unwrap_or(0)],
                |r| r.get(0),
            )
            .optional()?
            .flatten(),
        _ => None,
    };
    let mut backdrop = art(c, id, "backdrop")?;
    if backdrop.is_empty() {
        if let Some(s) = show_id {
            backdrop = art(c, s, "backdrop")?;
        }
    }
    let mut poster_full = art(c, id, "poster")?;
    if poster_full.is_empty() {
        if let Some(s) = show_id {
            poster_full = art(c, s, "poster")?;
        }
    }
    let parent_title: String = match show_id {
        Some(s) => c
            .query_row("SELECT title FROM item WHERE id = ?1", [s], |r| {
                r.get::<_, Option<String>>(0)
            })?
            .unwrap_or_default(),
        None => String::new(),
    };
    let files = {
        let mut st = c.prepare(&format!(
            "SELECT {ABS_PATH_SQL}, coalesce(f.size, 0), f.id, s.id, s.name, s.kind, coalesce(f.probe_json, '')
             FROM file f JOIN source s ON s.id = f.source_id WHERE f.item_id = ?1 ORDER BY f.size DESC"
        ))?;
        st.query_map([id], |r| {
            Ok(FileInfo {
                path: r.get(0)?,
                size: r.get(1)?,
                id: r.get(2)?,
                source_id: r.get(3)?,
                source_name: r.get(4)?,
                source_kind: r.get(5)?,
                probe_json: r.get(6)?,
                online: true,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let position_ms = crate::queries::playstate(c, id)?.position_ms;
    let children = match item.kind.as_str() {
        "show" | "season" => children(c, id)?,
        _ => vec![],
    };
    let next_up = match item.kind.as_str() {
        "show" | "season" => next_up(c, id, &item.kind)?,
        _ => None,
    };
    Ok(Some(Detail {
        item,
        original_title: original_title.unwrap_or_default(),
        rating,
        runtime_min,
        meta_json: meta_json.unwrap_or_default(),
        backdrop,
        poster_full,
        match_state: match_state.unwrap_or_default(),
        tmdb_id,
        parent_title,
        files,
        position_ms,
        children,
        next_up,
    }))
}

/// Seasons of a show or episodes of a season for the show page, including the ones only TMDB
/// knows (`on_disk` false), regular seasons first and specials last.
pub fn children(c: &Connection, id: i64) -> Result<Vec<Summary>> {
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} FROM item i
         LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.parent_id = ?1
         ORDER BY coalesce(i.season_no, 0) = 0 AND i.kind = 'season', i.season_no, i.episode_no"
    ))?;
    let rows = st.query_map([id], summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The episode to play next: the most recently started unfinished one, else the first unwatched
/// (regular seasons before specials). When everything on disk is watched, the first episode, so
/// the page offers "Play again" (the summary says `watched`).
fn next_up(c: &Connection, id: i64, kind: &str) -> Result<Option<Summary>> {
    let scope = if kind == "show" {
        "e.parent_id IN (SELECT id FROM item WHERE parent_id = ?1)"
    } else {
        "e.parent_id = ?1"
    };
    let sql = format!(
        "SELECT {cols} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.id IN (SELECT e.id FROM item e WHERE {scope}) AND coalesce(p.watched, 0) = 0
           AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id)
         ORDER BY coalesce(p.position_ms, 0) > 0 DESC, p.updated_at DESC,
                  coalesce(i.season_no, 0) = 0, i.season_no, i.episode_no LIMIT 1",
        cols = SUMMARY_COLS
    );
    if let Some(s) = c.query_row(&sql, [id], summary).optional()? {
        return Ok(Some(s));
    }
    let again = format!(
        "SELECT {cols} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.id IN (SELECT e.id FROM item e WHERE {scope})
           AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id)
         ORDER BY coalesce(i.season_no, 0) = 0, i.season_no, i.episode_no LIMIT 1",
        cols = SUMMARY_COLS
    );
    Ok(c.query_row(&again, [id], summary).optional()?)
}

/// Absolute path of the best file of a playable item (the largest version).
pub fn playable_path(c: &Connection, item_id: i64) -> Result<Option<String>> {
    Ok(playable_files(c, item_id)?
        .into_iter()
        .next()
        .map(|f| f.path))
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayableFile {
    pub source_id: i64,
    pub kind: String,
    /// Absolute: an OS path for local files, the path inside the share for network ones.
    pub path: String,
}

/// Every file of a playable item, best first: local copies, then the largest version.
/// The caller skips sources that are offline.
pub fn playable_files(c: &Connection, item_id: i64) -> Result<Vec<PlayableFile>> {
    let mut st = c.prepare(&format!(
        "SELECT f.source_id, s.kind, {ABS_PATH_SQL} FROM file f JOIN source s ON s.id = f.source_id
         WHERE f.item_id = ?1 ORDER BY s.kind = 'local' DESC, f.size DESC"
    ))?;
    let rows = st.query_map([item_id], |r| {
        Ok(PlayableFile {
            source_id: r.get(0)?,
            kind: r.get(1)?,
            path: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ---- media probe (issue #33) --------------------------------------------------------------------

/// Library files without probe data, or with data of an older schema (re-probed in the
/// background). Changed files lose their data in the scan, so they come back here too.
pub fn files_to_probe(c: &Connection, schema: u32) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT f.id FROM file f JOIN source s ON s.id = f.source_id
         WHERE f.item_id IS NOT NULL AND s.root IS NOT NULL
           AND (f.probe_json IS NULL OR coalesce(json_extract(f.probe_json, '$.schema'), 0) < ?1)
         ORDER BY f.id",
    )?;
    let rows = st.query_map([schema], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Where a file is: (source id, source kind, absolute path in the source, size, item).
pub type ProbeTarget = (i64, String, String, u64, i64);

pub fn probe_target(c: &Connection, file_id: i64) -> Result<Option<ProbeTarget>> {
    Ok(c.query_row(
        &format!(
            "SELECT f.source_id, s.kind, {ABS_PATH_SQL}, coalesce(f.size, 0), f.item_id
             FROM file f JOIN source s ON s.id = f.source_id WHERE f.id = ?1 AND f.item_id IS NOT NULL"
        ),
        [file_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? as u64, r.get(4)?)),
    )
    .optional()?)
}

/// The library file at an absolute path (in a given source, or in any).
pub fn file_for_path(c: &Connection, source: Option<i64>, abs_path: &str) -> Result<Option<i64>> {
    Ok(c.query_row(
        &format!(
            "SELECT f.id FROM file f JOIN source s ON s.id = f.source_id
             WHERE {ABS_PATH_SQL} = ?2 AND (?1 IS NULL OR f.source_id = ?1) LIMIT 1"
        ),
        params![source, abs_path],
        |r| r.get(0),
    )
    .optional()?)
}

/// A file's probe JSON, if any.
pub fn probe_json(c: &Connection, file_id: i64) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT probe_json FROM file WHERE id = ?1",
        [file_id],
        |r| r.get(0),
    )
    .optional()?
    .flatten())
}

/// Store a probe result. Returns the file's item.
pub fn set_probe(c: &Connection, file_id: i64, json: &str) -> Result<Option<i64>> {
    c.execute(
        "UPDATE file SET probe_json = ?2 WHERE id = ?1",
        params![file_id, json],
    )?;
    Ok(
        c.query_row("SELECT item_id FROM file WHERE id = ?1", [file_id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten(),
    )
}

// ---- matching and metadata writes ---------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchTarget {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub year: Option<u16>,
    pub locked: bool,
    pub tmdb_id: Option<i64>,
}

pub fn match_target(c: &Connection, id: i64) -> Result<Option<MatchTarget>> {
    Ok(c.query_row(
        "SELECT id, kind, title, year, locked, tmdb_id FROM item WHERE id = ?1",
        [id],
        |r| {
            Ok(MatchTarget {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                year: r.get::<_, Option<i64>>(3)?.map(|y| y as u16),
                locked: r.get::<_, i64>(4)? != 0,
                tmdb_id: r.get(5)?,
            })
        },
    )
    .optional()?)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CandidateRow {
    pub provider_id: i64,
    pub title: String,
    pub year: Option<u16>,
    pub score: f64,
    pub poster_path: Option<String>,
    pub overview: String,
}

pub fn set_match_state(
    c: &mut Connection,
    id: i64,
    state: &str,
    candidates: &[CandidateRow],
) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute(
        "UPDATE item SET match_state = ?2, meta_fetched_at = ?3 WHERE id = ?1",
        params![id, state, now_ms()],
    )?;
    tx.execute("DELETE FROM match_candidate WHERE item_id = ?1", [id])?;
    for k in candidates {
        tx.execute(
            "INSERT INTO match_candidate(item_id, provider_id, title, year, score, poster_path, overview) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, k.provider_id, k.title, k.year, k.score, k.poster_path, k.overview],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// When the matching rules last changed. Items whose last attempt is older are matched again.
pub const MATCHER_EPOCH_MS: i64 = 1_794_341_019_000; // 2026-11-10 20:03 UTC; never in the future
const RETRY_AFTER_MS: i64 = 7 * 24 * 3600 * 1000;

/// Movies and shows that are not (confidently) matched and are due for another attempt:
/// never tried, tried before the matcher last improved, or tried more than a week ago.
pub fn due_for_matching(c: &Connection) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT id FROM item WHERE kind IN ('movie','show') AND locked = 0
           AND coalesce(match_state, 'unmatched') IN ('unmatched', 'needs_review')
           AND (meta_fetched_at IS NULL OR meta_fetched_at < ?1 OR meta_fetched_at < ?2)",
    )?;
    let rows = st.query_map(params![MATCHER_EPOCH_MS, now_ms() - RETRY_AFTER_MS], |r| {
        r.get(0)
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn candidates(c: &Connection, id: i64) -> Result<Vec<CandidateRow>> {
    let mut st = c.prepare("SELECT provider_id, title, year, score, poster_path, overview FROM match_candidate WHERE item_id = ?1 ORDER BY score DESC")?;
    let rows = st.query_map([id], |r| {
        Ok(CandidateRow {
            provider_id: r.get(0)?,
            title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            year: r.get::<_, Option<i64>>(2)?.map(|y| y as u16),
            score: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
            poster_path: r.get(4)?,
            overview: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Movies and shows waiting for the user to pick a match.
pub fn review_list(c: &Connection) -> Result<Vec<(Summary, Vec<CandidateRow>)>> {
    let ids: Vec<i64> = {
        let mut st = c.prepare("SELECT i.id FROM item i WHERE i.match_state = 'needs_review' AND i.kind IN ('movie','show') ORDER BY i.sort_title")?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::new();
    for id in ids {
        if let Some(d) = detail(c, id)? {
            out.push((d.item, candidates(c, id)?));
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetaUpdate {
    pub tmdb_id: Option<i64>,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub overview: Option<String>,
    pub rating: Option<f64>,
    pub runtime_min: Option<u32>,
    pub imdb_id: Option<String>,
    pub meta_json: Option<String>,
    pub match_state: Option<&'static str>,
}

/// Write provider metadata onto an item. Locked items keep their user-set title and year.
pub fn apply_meta(c: &Connection, id: i64, m: &MetaUpdate) -> Result<()> {
    // A lock protects the user's choice: updates for the chosen TMDB id apply in full, anything
    // else (a different id, or no id) keeps the title and year.
    let (locked, current): (bool, Option<i64>) = c
        .query_row(
            "SELECT locked, tmdb_id FROM item WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, i64>(0)? != 0, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((false, None));
    let locked = locked && (m.tmdb_id.is_none() || m.tmdb_id != current);
    c.execute(
        "UPDATE item SET
           tmdb_id = coalesce(?2, tmdb_id),
           title = CASE WHEN ?11 THEN title ELSE coalesce(nullif(?3, ''), title) END,
           sort_title = CASE WHEN ?11 THEN sort_title ELSE coalesce(?12, sort_title) END,
           original_title = coalesce(?4, original_title),
           year = CASE WHEN ?11 THEN year ELSE coalesce(?5, year) END,
           overview = coalesce(nullif(?6, ''), overview),
           rating = coalesce(?7, rating), runtime_min = coalesce(?8, runtime_min),
           imdb_id = coalesce(?9, imdb_id), meta_json = coalesce(?10, meta_json),
           match_state = coalesce(?13, match_state), meta_fetched_at = ?14
         WHERE id = ?1",
        params![
            id,
            m.tmdb_id,
            m.title,
            m.original_title,
            m.year,
            m.overview,
            m.rating,
            m.runtime_min,
            m.imdb_id,
            m.meta_json,
            locked,
            m.title.as_deref().map(sort_key),
            m.match_state,
            now_ms()
        ],
    )?;
    let (kind, title, original, overview): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = c.query_row(
        "SELECT kind, title, original_title, overview FROM item WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if kind == "movie" || kind == "show" {
        c.execute("DELETE FROM item_fts WHERE rowid = ?1", [id])?;
        c.execute(
            "INSERT INTO item_fts(rowid, title, original_title, overview) VALUES (?1, ?2, ?3, ?4)",
            params![id, title, original, overview],
        )?;
    }
    Ok(())
}

fn sort_key(t: &str) -> String {
    let l = t.trim().to_lowercase();
    for a in [
        "the ", "a ", "an ", "der ", "die ", "das ", "le ", "la ", "les ", "el ",
    ] {
        if let Some(rest) = l.strip_prefix(a) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    l
}

/// The user picked a match (or typed a title): pin it so rescans never overwrite it.
/// The user's choice: lock the item to `tmdb_id`. If another item already has that id, the two
/// are merged first (issue #32). Returns the item that now carries the choice.
pub fn lock(c: &Connection, id: i64, tmdb_id: Option<i64>) -> Result<i64> {
    let id = match tmdb_id {
        Some(t) => crate::merge::claim_tmdb(c, id, t)?,
        None => id,
    };
    c.execute("UPDATE item SET locked = 1, tmdb_id = coalesce(?2, tmdb_id), match_state = 'matched' WHERE id = ?1", params![id, tmdb_id])?;
    c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [id])?;
    Ok(id)
}

/// Record a remote image for an item; the artwork job downloads it.
pub fn set_artwork(c: &Connection, item: i64, role: &str, remote: Option<&str>) -> Result<()> {
    let current: Option<Option<String>> = c
        .query_row(
            "SELECT remote_path FROM artwork WHERE item_id = ?1 AND role = ?2",
            params![item, role],
            |r| r.get(0),
        )
        .optional()?;
    match (current, remote) {
        (Some(cur), Some(r)) if cur.as_deref() == Some(r) => {}
        (Some(_), r) => {
            c.execute("UPDATE artwork SET remote_path = ?3, local_path = NULL WHERE item_id = ?1 AND role = ?2", params![item, role, r])?;
            if role == "poster_grid" {
                // The larger tier was made from the old image.
                c.execute(
                    "DELETE FROM artwork WHERE item_id = ?1 AND role = 'poster_grid_l'",
                    [item],
                )?;
            }
        }
        (None, Some(r)) => {
            c.execute(
                "INSERT INTO artwork(item_id, role, remote_path) VALUES (?1, ?2, ?3)",
                params![item, role, r],
            )?;
        }
        (None, None) => {}
    }
    Ok(())
}

/// Images still to download for an item and its children.
pub fn pending_artwork(c: &Connection, item: i64) -> Result<Vec<(i64, String, String)>> {
    let mut st = c.prepare(
        "SELECT a.item_id, a.role, a.remote_path FROM artwork a
         WHERE a.local_path IS NULL AND a.remote_path IS NOT NULL
           AND (a.item_id = ?1 OR a.item_id IN (SELECT id FROM item WHERE parent_id = ?1)
                OR a.item_id IN (SELECT e.id FROM item e JOIN item s ON e.parent_id = s.id WHERE s.parent_id = ?1))",
    )?;
    let rows = st.query_map([item], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn set_artwork_local(
    c: &Connection,
    item: i64,
    role: &str,
    local: &str,
    w: u32,
    h: u32,
) -> Result<()> {
    c.execute(
        "UPDATE artwork SET local_path = ?3, width = ?4, height = ?5 WHERE item_id = ?1 AND role = ?2",
        params![item, role, local, w, h],
    )?;
    Ok(())
}

/// Record a locally generated image tier (no remote of its own), replacing an older one.
pub fn set_artwork_tier(
    c: &Connection,
    item: i64,
    role: &str,
    local: &str,
    w: u32,
    h: u32,
) -> Result<()> {
    c.execute(
        "DELETE FROM artwork WHERE item_id = ?1 AND role = ?2",
        params![item, role],
    )?;
    c.execute(
        "INSERT INTO artwork(item_id, role, local_path, width, height) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![item, role, local, w, h],
    )?;
    Ok(())
}

/// Items with a grid thumbnail but no `poster_grid_l` tier yet, and a downloaded full-size
/// poster to make it from (offline): (item, local path of the full-size poster).
pub fn missing_large_tiers(c: &Connection, limit: u32) -> Result<Vec<(i64, String)>> {
    let mut st = c.prepare(
        "SELECT g.item_id, p.local_path FROM artwork g
         JOIN artwork p ON p.item_id = g.item_id AND p.role = 'poster' AND p.local_path IS NOT NULL
         WHERE g.role = 'poster_grid' AND g.local_path IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM artwork l WHERE l.item_id = g.item_id AND l.role = 'poster_grid_l')
         ORDER BY g.item_id LIMIT ?1",
    )?;
    let rows = st.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A season TMDB knows but the library has no file of: created as a virtual item (shown dimmed
/// on the show page). Returns the existing season when there is one.
pub fn ensure_season(c: &Connection, show: i64, season_no: i64, title: &str) -> Result<i64> {
    if let Some(id) = c
        .query_row(
            "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'season' AND season_no = ?2",
            params![show, season_no],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    c.execute(
        "INSERT INTO item(kind, parent_id, title, sort_title, season_no, match_state, virtual)
         VALUES ('season', ?1, ?2, ?3, ?4, 'matched', 1)",
        params![show, title, sort_key(title), season_no],
    )?;
    Ok(c.last_insert_rowid())
}

/// An episode TMDB knows that is not on disk, as a virtual item. Returns the existing one.
pub fn ensure_episode(c: &Connection, season: i64, episode_no: i64, title: &str) -> Result<i64> {
    if let Some(id) = c
        .query_row(
            "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'episode' AND episode_no = ?2",
            params![season, episode_no],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let season_no: Option<i64> =
        c.query_row("SELECT season_no FROM item WHERE id = ?1", [season], |r| {
            r.get(0)
        })?;
    c.execute(
        "INSERT INTO item(kind, parent_id, title, sort_title, season_no, episode_no, match_state, virtual)
         VALUES ('episode', ?1, ?2, ?3, ?4, ?5, 'matched', 1)",
        params![season, title, sort_key(title), season_no, episode_no],
    )?;
    Ok(c.last_insert_rowid())
}

/// Seasons and episodes of a show, for applying season details: (season item, season_no,
/// [(episode item, episode_no)]).
/// (season item, season_no, [(episode item, episode_no)])
pub type ShowStructure = Vec<(i64, i64, Vec<(i64, i64)>)>;

/// `on_disk_only`: only seasons and episodes from the scanner, not the ones only TMDB knows
/// (matching compares those with a candidate's seasons).
pub fn show_structure(c: &Connection, show: i64, on_disk_only: bool) -> Result<ShowStructure> {
    let real = if on_disk_only { " AND virtual = 0" } else { "" };
    let seasons: Vec<(i64, i64)> = {
        let mut st = c.prepare(&format!("SELECT id, coalesce(season_no, 0) FROM item WHERE parent_id = ?1 AND kind = 'season'{real} ORDER BY season_no"))?;
        st.query_map([show], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::new();
    for (sid, no) in seasons {
        let mut st = c.prepare(&format!("SELECT id, coalesce(episode_no, 0) FROM item WHERE parent_id = ?1 AND kind = 'episode'{real}"))?;
        let eps = st
            .query_map([sid], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        out.push((sid, no, eps));
    }
    Ok(out)
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
    fn library_items_browse_search_detail() {
        let mut c = db();
        let lib = add_library(&c, "Media", "/m").unwrap();
        assert_eq!(add_library(&c, "Again", "/m/").unwrap(), lib);
        c.execute_batch(&format!(
            "INSERT INTO item(id, kind, title, sort_title, year, added_at) VALUES (1,'movie','The Matrix','matrix',1999,10), (2,'movie','Alien','alien',1979,20);
             INSERT INTO item(id, kind, title, sort_title, added_at) VALUES (3,'show','Firefly','firefly',30);
             INSERT INTO item(id, kind, title, parent_id, season_no) VALUES (4,'season','Season 1',3,1);
             INSERT INTO item(id, kind, title, parent_id, season_no, episode_no) VALUES (5,'episode','Serenity',4,1,1), (6,'episode','Train Job',4,1,2);
             INSERT INTO file(source_id, path, size, item_id) VALUES ({lib},'Matrix.mkv',5,1), ({lib},'Alien.mkv',5,2), ({lib},'F/S01E01.mkv',1,5), ({lib},'F/S01E02.mkv',1,6);
             INSERT INTO item_fts(rowid, title) VALUES (1,'The Matrix'), (2,'Alien'), (3,'Firefly');"
        ))
        .unwrap();
        crate::queries::set_watched(&mut c, 5, true).unwrap();

        let q = |sort| Query {
            kind: "movie".into(),
            parent: None,
            sort,
            unwatched_only: false,
            offset: 0,
            limit: 50,
        };
        let (n, page) = browse(&c, &q(Sort::Title)).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            page.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            ["Alien", "The Matrix"]
        );
        assert_eq!(browse(&c, &q(Sort::Added)).unwrap().1[0].title, "Alien");
        let (_, shows) = browse(
            &c,
            &Query {
                kind: "show".into(),
                ..q(Sort::Title)
            },
        )
        .unwrap();
        assert_eq!(
            (shows[0].title.as_str(), shows[0].unwatched),
            ("Firefly", 1)
        );

        assert_eq!(search(&c, "matr", 10).unwrap()[0].id, 1);
        assert_eq!(
            search(&c, "\"firef", 10).unwrap()[0].id,
            3,
            "stray quotes do not break FTS"
        );
        assert!(search(&c, "  ", 10).unwrap().is_empty());

        let d = detail(&c, 3).unwrap().unwrap();
        assert_eq!(d.children.len(), 1);
        assert_eq!(d.next_up.unwrap().id, 6, "first unwatched episode");
        let e = detail(&c, 6).unwrap().unwrap();
        assert_eq!(e.parent_title, "Firefly");
        assert_eq!(e.files[0].path, "/m/F/S01E02.mkv");
        assert_eq!(playable_path(&c, 1).unwrap().unwrap(), "/m/Matrix.mkv");
        assert_eq!(library_item_for_path(&c, "/m/Alien.mkv").unwrap(), Some(2));
        assert_eq!(
            library_item_for_path(&c, "/elsewhere/Alien.mkv").unwrap(),
            None
        );

        remove_library(&mut c, lib).unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM item", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn large_grid_tier_is_listed_made_and_dropped_with_its_image() {
        let mut c = db();
        let lib = add_library(&c, "Media", "/m").unwrap();
        c.execute_batch(&format!(
            "INSERT INTO item(id, kind, title, sort_title, added_at) VALUES (1,'movie','Alien','alien',1), (2,'season','Season 1','s',2);
             INSERT INTO file(source_id, path, size, item_id) VALUES ({lib},'Alien.mkv',5,1);"
        ))
        .unwrap();
        for (item, role) in [(1, "poster"), (1, "poster_grid"), (2, "poster_grid")] {
            set_artwork(&c, item, role, Some("/a.jpg")).unwrap();
            set_artwork_local(&c, item, role, &format!("/d/{item}/{role}.jpg"), 360, 540).unwrap();
        }
        // The season has no full-size original, so it cannot get the tier offline.
        assert_eq!(
            missing_large_tiers(&c, 10).unwrap(),
            vec![(1, "/d/1/poster.jpg".to_string())]
        );

        let one = |c: &mut Connection| {
            let q = Query {
                kind: "movie".into(),
                parent: None,
                sort: Sort::Title,
                unwatched_only: false,
                offset: 0,
                limit: 10,
            };
            browse(c, &q).unwrap().1.remove(0)
        };
        assert_eq!(one(&mut c).poster_large, "");
        set_artwork_tier(&c, 1, "poster_grid_l", "/d/1/poster_grid_l.jpg", 540, 810).unwrap();
        set_artwork_tier(&c, 1, "poster_grid_l", "/d/1/poster_grid_l.jpg", 540, 810).unwrap();
        let s = one(&mut c);
        assert_eq!(
            (s.poster.as_str(), s.poster_large.as_str()),
            ("/d/1/poster_grid.jpg", "/d/1/poster_grid_l.jpg")
        );
        assert!(missing_large_tiers(&c, 10).unwrap().is_empty());

        // A new poster drops the tier made from the old one.
        set_artwork(&c, 1, "poster_grid", Some("/b.jpg")).unwrap();
        assert_eq!(one(&mut c).poster_large, "");
    }

    /// Issue #32, show page: seasons with specials last, episodes TMDB knows but the library
    /// has not (`on_disk` false), and the main button in its four cases.
    #[test]
    fn show_page_children_and_next_episode() {
        let mut c = db();
        let lib = add_library(&c, "Media", "/m").unwrap();
        c.execute_batch(&format!(
            "INSERT INTO item(id, kind, title) VALUES (1, 'show', 'Show');
             INSERT INTO item(id, kind, parent_id, title, season_no) VALUES (10, 'season', 1, 'Specials', 0), (11, 'season', 1, 'Season 1', 1), (12, 'season', 1, 'Season 2', 2);
             INSERT INTO item(id, kind, parent_id, title, season_no, episode_no, runtime_min) VALUES
               (100, 'episode', 10, 'Special', 0, 1, 30),
               (111, 'episode', 11, 'One', 1, 1, 44), (112, 'episode', 11, 'Two', 1, 2, 45),
               (121, 'episode', 12, 'Three', 2, 1, 46);
             INSERT INTO file(source_id, path, size, item_id) VALUES ({lib}, 's0e1', 1, 100), ({lib}, 's1e1', 1, 111), ({lib}, 's1e2', 1, 112), ({lib}, 's2e1', 1, 121);"
        ))
        .unwrap();
        // TMDB knows S02E02 and a third season the library does not have.
        let s3 = ensure_season(&c, 1, 3, "Season 3").unwrap();
        ensure_episode(&c, s3, 1, "Later").unwrap();
        ensure_episode(&c, 12, 2, "Four").unwrap();
        assert_eq!(ensure_season(&c, 1, 3, "again").unwrap(), s3, "idempotent");

        let seasons = children(&c, 1).unwrap();
        let order: Vec<_> = seasons.iter().map(|s| (s.season_no, s.on_disk)).collect();
        assert_eq!(
            order,
            [
                (Some(1), true),
                (Some(2), true),
                (Some(3), false),
                (Some(0), true)
            ],
            "specials last, season 3 not on disk"
        );
        let eps = children(&c, 12).unwrap();
        let rows: Vec<_> = eps
            .iter()
            .map(|e| (e.episode_no, e.on_disk, e.runtime_min))
            .collect();
        assert_eq!(rows, [(Some(1), true, Some(46)), (Some(2), false, None)]);

        let next = |c: &Connection| {
            let d = detail(c, 1).unwrap().unwrap();
            let n = d.next_up.unwrap();
            (n.id, n.watched)
        };
        // Nothing watched: S01E01.
        assert_eq!(next(&c), (111, false));
        // One episode half watched: resume it.
        crate::queries::save_progress(&mut c, 112, 600_000, 2_700_000, false).unwrap();
        assert_eq!(next(&c), (112, false));
        // Season 1 finished: the next season.
        crate::queries::set_watched(&mut c, 111, true).unwrap();
        crate::queries::set_watched(&mut c, 112, true).unwrap();
        assert_eq!(next(&c), (121, false));
        // Everything on disk watched: start again at S01E01 (the page says "Play again").
        crate::queries::set_watched(&mut c, 121, true).unwrap();
        crate::queries::set_watched(&mut c, 100, true).unwrap();
        assert_eq!(next(&c), (111, true));
    }

    #[test]
    fn metadata_respects_locks_and_updates_search() {
        let c = db();
        c.execute_batch("INSERT INTO item(id, kind, title, sort_title, year) VALUES (1,'movie','the thing','the thing',NULL); INSERT INTO item_fts(rowid,title) VALUES (1,'the thing');").unwrap();
        let m = MetaUpdate {
            tmdb_id: Some(1091),
            title: Some("The Thing".into()),
            year: Some(1982),
            overview: Some("Antarctica".into()),
            match_state: Some("matched"),
            ..Default::default()
        };
        apply_meta(&c, 1, &m).unwrap();
        let t = match_target(&c, 1).unwrap().unwrap();
        assert_eq!(
            (t.title.as_str(), t.year, t.tmdb_id),
            ("The Thing", Some(1982), Some(1091))
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'antarctica'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        lock(&c, 1, Some(1091)).unwrap();
        apply_meta(
            &c,
            1,
            &MetaUpdate {
                title: Some("Something Else".into()),
                year: Some(2011),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            match_target(&c, 1).unwrap().unwrap().title,
            "The Thing",
            "locked title kept"
        );
        set_artwork(&c, 1, "poster", Some("/p.jpg")).unwrap();
        assert_eq!(pending_artwork(&c, 1).unwrap().len(), 1);
        set_artwork_local(&c, 1, "poster", "/cache/p.jpg", 780, 1170).unwrap();
        assert!(pending_artwork(&c, 1).unwrap().is_empty());
        set_artwork(&c, 1, "poster", Some("/p.jpg")).unwrap();
        assert!(
            pending_artwork(&c, 1).unwrap().is_empty(),
            "same remote image is not refetched"
        );
    }
}
