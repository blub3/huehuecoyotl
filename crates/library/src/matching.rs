//! Matching again (2.7, issue #92, SPEC 7 "Matching again"): what each library holds and how
//! much of it is matched, which items a library rematch covers, the state of the rematch runs
//! (kept in `setting`, so a run survives a restart), and the id and lock helpers that the
//! per-kind matching in the core shares.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::Result;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The libraries that can be matched again, as Settings → Matching lists them.
pub const LIBRARIES: &[&str] = &["movies", "shows", "music", "books"];

/// The item kind a library's rematch walks: movies, shows, albums, audiobooks.
pub fn item_kind(library: &str) -> Option<&'static str> {
    match library {
        "movies" => Some("movie"),
        "shows" => Some("show"),
        "music" => Some("album"),
        "books" => Some("audiobook"),
        _ => None,
    }
}

/// The library an item belongs to, for the kinds that can be matched.
pub fn library_of(item_kind: &str) -> Option<&'static str> {
    match item_kind {
        "movie" => Some("movies"),
        "show" | "season" | "episode" => Some("shows"),
        "album" | "track" | "artist" => Some("music"),
        "audiobook" | "person" => Some("books"),
        _ => None,
    }
}

/// One library's row in Settings → Matching.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub library: String,
    pub total: i64,
    pub matched: i64,
    pub review: i64,
    pub none: i64,
    /// Matches the user chose (locked).
    pub locked: i64,
    /// What else the library holds: episodes; songs and artists; authors.
    pub episodes: i64,
    pub songs: i64,
    pub artists: i64,
    pub authors: i64,
}

pub fn counts(c: &Connection, library: &str) -> Result<Counts> {
    let Some(kind) = item_kind(library) else {
        return Ok(Counts::default());
    };
    let mut n = c.query_row(
        "SELECT count(*), coalesce(sum(match_state = 'matched'), 0),
                coalesce(sum(match_state = 'needs_review'), 0),
                coalesce(sum(coalesce(match_state, 'unmatched') NOT IN ('matched', 'needs_review')), 0),
                coalesce(sum(locked <> 0), 0)
         FROM item WHERE kind = ?1 AND virtual = 0",
        [kind],
        |r| {
            Ok(Counts {
                library: library.to_string(),
                total: r.get(0)?,
                matched: r.get(1)?,
                review: r.get(2)?,
                none: r.get(3)?,
                locked: r.get(4)?,
                ..Default::default()
            })
        },
    )?;
    let count = |sql: &str| -> Result<i64> { Ok(c.query_row(sql, [], |r| r.get(0))?) };
    match library {
        "shows" => {
            n.episodes = count("SELECT count(*) FROM item WHERE kind = 'episode' AND virtual = 0")?
        }
        "music" => {
            n.songs = count("SELECT count(*) FROM item WHERE kind = 'track'")?;
            n.artists = count("SELECT count(*) FROM item WHERE kind = 'artist'")?;
        }
        "books" => {
            n.authors =
                count("SELECT count(DISTINCT person_id) FROM book_credit WHERE role = 'author'")?
        }
        _ => {}
    }
    Ok(n)
}

/// The items a library rematch covers: only the unmatched and uncertain ones, or all; matches
/// the user chose only with `include_locked`.
pub fn targets(c: &Connection, library: &str, all: bool, include_locked: bool) -> Result<Vec<i64>> {
    let Some(kind) = item_kind(library) else {
        return Ok(vec![]);
    };
    let mut sql = String::from("SELECT id FROM item WHERE kind = ?1 AND virtual = 0");
    if !all {
        sql.push_str(" AND coalesce(match_state, 'unmatched') <> 'matched'");
    }
    if !include_locked {
        sql.push_str(" AND locked = 0");
    }
    sql.push_str(" ORDER BY id");
    let mut st = c.prepare(&sql)?;
    Ok(st
        .query_map([kind], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

// ---- rematch runs ------------------------------------------------------------------------------

/// The job kind that rematches one item of a library, by the worker that runs it.
pub fn job_kind(library: &str) -> &'static str {
    match library {
        "music" => "rematch_music",
        "books" => "rematch_book",
        _ => "rematch_video",
    }
}

/// A running run whose jobs are all gone (the last ones failed for good, or were removed) is
/// done. Returns whether a run changed.
pub fn reconcile(c: &Connection) -> Result<bool> {
    let mut list = runs(c)?;
    let mut changed = false;
    for r in list.iter_mut().filter(|r| r.state == "running") {
        let left: i64 = c.query_row(
            "SELECT count(*) FROM job WHERE kind = ?1 AND state IN ('queued', 'running')
               AND json_extract(payload_json, '$.library') = ?2",
            params![job_kind(&r.library), r.library],
            |x| x.get(0),
        )?;
        if left == 0 {
            r.state = "done".into();
            r.finished_at = now_ms();
            changed = true;
        }
    }
    if changed {
        save_runs(c, &list)?;
    }
    Ok(changed)
}

const RUNS_KEY: &str = "rematch_runs";

/// One library rematch. Runs wait in line: one library at a time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Run {
    pub library: String,
    pub all: bool,
    pub include_locked: bool,
    /// waiting | running | done | stopped
    pub state: String,
    pub total: i64,
    pub done: i64,
    /// Matched now and not before; matched to something else; waiting for a choice; nothing
    /// sure found.
    pub newly: i64,
    pub changed: i64,
    pub review: i64,
    pub not_found: i64,
    pub started_at: i64,
    pub finished_at: i64,
}

pub fn runs(c: &Connection) -> Result<Vec<Run>> {
    Ok(crate::copies::setting(c, RUNS_KEY)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

pub fn save_runs(c: &Connection, runs: &[Run]) -> Result<()> {
    let json = serde_json::to_string(runs).unwrap_or_else(|_| "[]".into());
    crate::copies::set_setting(c, RUNS_KEY, Some(&json))
}

/// Queue a run for a library, replacing an earlier one of it. Returns whether it may start now
/// (no other library is running).
pub fn add_run(c: &Connection, library: &str, all: bool, include_locked: bool) -> Result<bool> {
    let mut list = runs(c)?;
    list.retain(|r| r.library != library);
    let busy = list
        .iter()
        .any(|r| r.state == "running" || r.state == "waiting");
    list.push(Run {
        library: library.to_string(),
        all,
        include_locked,
        state: "waiting".into(),
        ..Default::default()
    });
    save_runs(c, &list)?;
    Ok(!busy)
}

/// The run that should start next: the oldest waiting one, when none is running.
pub fn next_run(c: &Connection) -> Result<Option<Run>> {
    let list = runs(c)?;
    if list.iter().any(|r| r.state == "running") {
        return Ok(None);
    }
    Ok(list.into_iter().find(|r| r.state == "waiting"))
}

/// Change a library's run in place.
pub fn update_run(c: &Connection, library: &str, f: impl FnOnce(&mut Run)) -> Result<Option<Run>> {
    let mut list = runs(c)?;
    let Some(r) = list.iter_mut().find(|r| r.library == library) else {
        return Ok(None);
    };
    f(r);
    let out = r.clone();
    save_runs(c, &list)?;
    Ok(Some(out))
}

/// Start a waiting run with its targets: (run, items to queue).
pub fn start_run(c: &Connection, library: &str) -> Result<Option<(Run, Vec<i64>)>> {
    let Some(r) = runs(c)?.into_iter().find(|r| r.library == library) else {
        return Ok(None);
    };
    let items = targets(c, library, r.all, r.include_locked)?;
    let n = items.len() as i64;
    let run = update_run(c, library, |r| {
        r.state = if n == 0 {
            "done".into()
        } else {
            "running".into()
        };
        r.total = n;
        r.started_at = now_ms();
        if n == 0 {
            r.finished_at = now_ms();
        }
    })?;
    Ok(run.map(|r| (r, items)))
}

/// Count one item of a run; the run is done after its last item. Returns the run.
pub fn count_result(c: &Connection, library: &str, outcome: &str) -> Result<Option<Run>> {
    update_run(c, library, |r| {
        if r.state != "running" {
            return;
        }
        r.done += 1;
        match outcome {
            "newly" => r.newly += 1,
            "changed" => r.changed += 1,
            "review" => r.review += 1,
            "notFound" => r.not_found += 1,
            _ => {}
        }
        if r.done >= r.total {
            r.state = "done".into();
            r.finished_at = now_ms();
        }
    })
}

// ---- ids and locks ----------------------------------------------------------------------------

pub fn external(c: &Connection, item: i64, provider: &str, kind: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT value FROM external_id WHERE item_id = ?1 AND provider = ?2 AND kind = ?3",
        params![item, provider, kind],
        |r| r.get(0),
    )
    .optional()?)
}

/// Set an item's id at a provider, replacing the one it had (`None` removes it).
pub fn set_external(
    c: &Connection,
    item: i64,
    provider: &str,
    kind: &str,
    value: Option<&str>,
) -> Result<()> {
    c.execute(
        "DELETE FROM external_id WHERE item_id = ?1 AND provider = ?2 AND kind = ?3",
        params![item, provider, kind],
    )?;
    if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
        c.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, ?2, ?3, ?4)",
            params![item, provider, kind, v],
        )?;
    }
    Ok(())
}

pub fn locked(c: &Connection, item: i64) -> Result<bool> {
    Ok(
        c.query_row("SELECT locked FROM item WHERE id = ?1", [item], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
        .unwrap_or(0)
            != 0,
    )
}

/// The user's choice: kept by automatic matching from now on.
pub fn set_locked(c: &Connection, item: i64, on: bool) -> Result<()> {
    c.execute(
        "UPDATE item SET locked = ?2 WHERE id = ?1",
        params![item, on as i64],
    )?;
    Ok(())
}

/// The kind and title of an item, for messages.
pub fn kind_title(c: &Connection, item: i64) -> Result<Option<(String, String)>> {
    Ok(c.query_row(
        "SELECT kind, coalesce(title, '') FROM item WHERE id = ?1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?)
}

/// Issue #106: how an artist or author got its match: through an album, a song or a book
/// (`via` "album:12"); none when the user or a lookup of its own matched it.
pub fn set_matched_via(c: &Connection, item: i64, via: Option<&str>) -> Result<()> {
    set_external(c, item, "coyotl", "matched_via", via)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    fn item(c: &Connection, kind: &str, state: &str, locked: bool) -> i64 {
        c.execute(
            "INSERT INTO item(kind, title, match_state, locked) VALUES (?1, 'x', ?2, ?3)",
            params![kind, state, locked as i64],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    #[test]
    fn counts_and_targets() {
        let c = db();
        let a = item(&c, "movie", "matched", false);
        let b = item(&c, "movie", "needs_review", false);
        let d = item(&c, "movie", "unmatched", false);
        let l = item(&c, "movie", "matched", true);
        let u = item(&c, "movie", "unmatched", true);
        item(&c, "show", "matched", false);
        let n = counts(&c, "movies").unwrap();
        assert_eq!(
            (n.total, n.matched, n.review, n.none, n.locked),
            (5, 2, 1, 2, 2)
        );
        assert_eq!(targets(&c, "movies", false, false).unwrap(), vec![b, d]);
        assert_eq!(targets(&c, "movies", false, true).unwrap(), vec![b, d, u]);
        assert_eq!(targets(&c, "movies", true, false).unwrap(), vec![a, b, d]);
        assert_eq!(
            targets(&c, "movies", true, true).unwrap(),
            vec![a, b, d, l, u]
        );
        assert!(targets(&c, "nothing", true, true).unwrap().is_empty());
    }

    #[test]
    fn runs_wait_in_line_and_count() {
        let c = db();
        item(&c, "movie", "unmatched", false);
        item(&c, "movie", "unmatched", false);
        assert!(add_run(&c, "movies", false, false).unwrap());
        let (run, items) = start_run(&c, "movies").unwrap().unwrap();
        assert_eq!(
            (run.state.as_str(), run.total, items.len()),
            ("running", 2, 2)
        );
        // Another library waits while movies run.
        assert!(!add_run(&c, "books", false, false).unwrap());
        assert_eq!(next_run(&c).unwrap(), None);
        count_result(&c, "movies", "newly").unwrap();
        let r = count_result(&c, "movies", "notFound").unwrap().unwrap();
        assert_eq!((r.state.as_str(), r.newly, r.not_found), ("done", 1, 1));
        assert_eq!(next_run(&c).unwrap().unwrap().library, "books");
        // Nothing to do: done at once.
        let (b, items) = start_run(&c, "books").unwrap().unwrap();
        assert_eq!((b.state.as_str(), items.len()), ("done", 0));
    }

    #[test]
    fn ids_replace_and_locks() {
        let c = db();
        let a = item(&c, "artist", "unmatched", false);
        set_external(&c, a, "musicbrainz", "artist", Some("one")).unwrap();
        set_external(&c, a, "musicbrainz", "artist", Some("two")).unwrap();
        assert_eq!(
            external(&c, a, "musicbrainz", "artist").unwrap().as_deref(),
            Some("two")
        );
        set_external(&c, a, "musicbrainz", "artist", None).unwrap();
        assert_eq!(external(&c, a, "musicbrainz", "artist").unwrap(), None);
        set_locked(&c, a, true).unwrap();
        assert!(locked(&c, a).unwrap());
    }
}
