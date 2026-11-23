//! Typed queries. Functions taking `&mut Connection` belong on the writer; `&Connection` ones
//! are read-only and run on the pool.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{LOCAL_SOURCE_ID, Result};

pub const DEFAULT_PROFILE: i64 = 1;
/// Below this position nothing is worth resuming.
pub const RESUME_MIN_MS: u64 = 30_000;
/// Past this fraction of the duration an item counts as watched.
pub const WATCHED_FRACTION: f64 = 0.92;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayState {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub watched: bool,
}

impl PlayState {
    /// 0.0 to 1.0 for progress bars; watched items count as complete.
    pub fn progress(&self) -> f32 {
        if self.watched {
            1.0
        } else if self.duration_ms > 0 {
            (self.position_ms as f64 / self.duration_ms as f64).min(1.0) as f32
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackPref {
    pub audio_lang: String,
    pub sub_lang: String,
    pub sub_off: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecentRow {
    pub item_id: i64,
    pub title: String,
    pub path: String,
    pub state: PlayState,
}

fn title_from_path(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(s, _)| s).to_string()
}

/// Make sure a local file has a `file` row and a logical `item` (kind 'video' until the
/// scanner in M2 knows better). Returns the item id.
pub fn ensure_local_video(c: &mut Connection, path: &str, size: u64, mtime: i64) -> Result<i64> {
    let tx = c.transaction()?;
    let existing: Option<(i64, Option<i64>)> = tx
        .query_row(
            "SELECT id, item_id FROM file WHERE source_id = ?1 AND path = ?2",
            params![LOCAL_SOURCE_ID, path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let item_id = match existing {
        Some((file_id, Some(item))) => {
            tx.execute(
                "UPDATE file SET size = ?2, mtime = ?3 WHERE id = ?1",
                params![file_id, size as i64, mtime],
            )?;
            item
        }
        other => {
            let title = title_from_path(path);
            tx.execute(
                "INSERT INTO item(kind, title, sort_title, match_state) VALUES ('video', ?1, lower(?1), 'unmatched')",
                params![title],
            )?;
            let item = tx.last_insert_rowid();
            match other {
                Some((file_id, None)) => {
                    tx.execute(
                        "UPDATE file SET item_id = ?2, size = ?3, mtime = ?4 WHERE id = ?1",
                        params![file_id, item, size as i64, mtime],
                    )?;
                }
                _ => {
                    tx.execute(
                        "INSERT INTO file(source_id, path, size, mtime, item_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![LOCAL_SOURCE_ID, path, size as i64, mtime, item],
                    )?;
                }
            }
            item
        }
    };
    tx.commit()?;
    Ok(item_id)
}

/// Title for the player: episodes read "Show · S1E2 · Episode title".
pub fn item_title(c: &Connection, item_id: i64) -> Result<Option<String>> {
    Ok(c
        .query_row(
            "SELECT CASE WHEN i.kind = 'episode'
                    THEN coalesce(sh.title || ' · ', '') || 'S' || coalesce(i.season_no, 0) || 'E' || coalesce(i.episode_no, 0) || ' · ' || i.title
                    ELSE i.title END
             FROM item i LEFT JOIN item se ON se.id = i.parent_id LEFT JOIN item sh ON sh.id = se.parent_id
             WHERE i.id = ?1",
            [item_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn playstate(c: &Connection, item_id: i64) -> Result<PlayState> {
    Ok(c
        .query_row(
            "SELECT position_ms, duration_ms, watched FROM playstate WHERE item_id = ?1 AND profile_id = ?2",
            params![item_id, DEFAULT_PROFILE],
            |r| Ok(PlayState { position_ms: r.get::<_, i64>(0)? as u64, duration_ms: r.get::<_, i64>(1)? as u64, watched: r.get::<_, i64>(2)? != 0 }),
        )
        .optional()?
        .unwrap_or_default())
}

/// Record progress. `ended` means playback reached the end of the file.
/// Returns the state that was stored.
pub fn save_progress(
    c: &mut Connection,
    item_id: i64,
    position_ms: u64,
    duration_ms: u64,
    ended: bool,
) -> Result<PlayState> {
    let prev = playstate(c, item_id)?;
    let finished =
        ended || (duration_ms > 0 && position_ms as f64 >= duration_ms as f64 * WATCHED_FRACTION);
    let state = if finished {
        PlayState {
            position_ms: 0,
            duration_ms,
            watched: true,
        }
    } else {
        PlayState {
            position_ms: if position_ms < RESUME_MIN_MS {
                0
            } else {
                position_ms
            },
            duration_ms,
            // Re-watching a watched item keeps the flag; only an explicit unmark clears it.
            watched: prev.watched,
        }
    };
    c.execute(
        "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(item_id, profile_id) DO UPDATE SET position_ms = excluded.position_ms,
           duration_ms = excluded.duration_ms, watched = excluded.watched, updated_at = excluded.updated_at",
        params![item_id, DEFAULT_PROFILE, state.position_ms as i64, state.duration_ms as i64, state.watched as i64, now_ms()],
    )?;
    Ok(state)
}

// ---- versions (issue #34) -----------------------------------------------------------------------

/// Two versions are the same cut when their durations differ by less than 2 %; then they share
/// the movie's resume position. An unknown duration counts as the same.
pub fn same_cut(a_ms: u64, b_ms: u64) -> bool {
    if a_ms == 0 || b_ms == 0 {
        return true;
    }
    (a_ms as f64 - b_ms as f64).abs() / (a_ms.max(b_ms) as f64) < 0.02
}

/// Where playing `file` (of `item`, `file_duration_ms` long, 0 when unknown) starts: the item's
/// position for the same cut, else that version's own position.
pub fn resume_for(
    c: &Connection,
    item_id: i64,
    file_id: i64,
    file_duration_ms: u64,
) -> Result<u64> {
    let st = playstate(c, item_id)?;
    if same_cut(st.duration_ms, file_duration_ms) {
        return Ok(if st.watched { 0 } else { st.position_ms });
    }
    Ok(c.query_row(
        "SELECT position_ms FROM file_state WHERE file_id = ?1 AND profile_id = ?2",
        params![file_id, DEFAULT_PROFILE],
        |r| r.get::<_, i64>(0),
    )
    .optional()?
    .unwrap_or(0) as u64)
}

/// Progress of one version. The same cut as the item's last one updates the item's position;
/// a different cut keeps its own. Watched always belongs to the item, and the version becomes
/// the item's last played one.
pub fn save_version_progress(
    c: &mut Connection,
    item_id: i64,
    file_id: i64,
    position_ms: u64,
    duration_ms: u64,
    ended: bool,
) -> Result<PlayState> {
    let prev = playstate(c, item_id)?;
    let state = if same_cut(prev.duration_ms, duration_ms) {
        save_progress(c, item_id, position_ms, duration_ms, ended)?
    } else {
        let finished = ended
            || (duration_ms > 0 && position_ms as f64 >= duration_ms as f64 * WATCHED_FRACTION);
        let pos = if finished || position_ms < RESUME_MIN_MS {
            0
        } else {
            position_ms
        };
        c.execute(
            "INSERT INTO file_state(file_id, profile_id, position_ms, duration_ms, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(file_id, profile_id) DO UPDATE SET position_ms = excluded.position_ms,
               duration_ms = excluded.duration_ms, updated_at = excluded.updated_at",
            params![
                file_id,
                DEFAULT_PROFILE,
                pos as i64,
                duration_ms as i64,
                now_ms()
            ],
        )?;
        if finished && !prev.watched {
            c.execute(
                "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at)
                 VALUES (?1, ?2, 0, 0, 1, ?3)
                 ON CONFLICT(item_id, profile_id) DO UPDATE SET watched = 1, updated_at = excluded.updated_at",
                params![item_id, DEFAULT_PROFILE, now_ms()],
            )?;
        }
        playstate(c, item_id)?
    };
    c.execute(
        "UPDATE playstate SET last_file_id = ?3 WHERE item_id = ?1 AND profile_id = ?2",
        params![item_id, DEFAULT_PROFILE, file_id],
    )?;
    Ok(state)
}

pub fn set_watched(c: &mut Connection, item_id: i64, watched: bool) -> Result<()> {
    c.execute(
        "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at)
         VALUES (?1, ?2, 0, 0, ?3, ?4)
         ON CONFLICT(item_id, profile_id) DO UPDATE SET position_ms = 0, watched = excluded.watched,
           updated_at = excluded.updated_at",
        params![item_id, DEFAULT_PROFILE, watched as i64, now_ms()],
    )?;
    Ok(())
}

/// Started but unfinished items, most recent first.
pub fn continue_watching(c: &Connection, limit: u32) -> Result<Vec<RecentRow>> {
    let mut st = c.prepare(&format!(
        "SELECT i.id, CASE WHEN i.kind = 'episode' THEN coalesce(sh.title || ' · ', '') || 'S' || i.season_no || 'E' || i.episode_no || ' ' || i.title ELSE i.title END,
                {abs}, p.position_ms, p.duration_ms
         FROM playstate p JOIN item i ON i.id = p.item_id
         JOIN file f ON f.id = (SELECT min(id) FROM file WHERE item_id = i.id)
         JOIN source s ON s.id = f.source_id
         LEFT JOIN item se ON se.id = i.parent_id LEFT JOIN item sh ON sh.id = se.parent_id
         WHERE p.profile_id = ?1 AND p.watched = 0 AND p.position_ms > 0
         ORDER BY p.updated_at DESC LIMIT ?2",
        abs = crate::catalog::ABS_PATH_SQL
    ))?;
    let rows = st.query_map(params![DEFAULT_PROFILE, limit], |r| {
        Ok(RecentRow {
            item_id: r.get(0)?,
            title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            path: r.get(2)?,
            state: PlayState {
                position_ms: r.get::<_, i64>(3)? as u64,
                duration_ms: r.get::<_, i64>(4)? as u64,
                watched: false,
            },
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Play state of every known local file directly inside `dir`, keyed by path.
pub fn local_states_in_dir(c: &Connection, dir: &str) -> Result<HashMap<String, PlayState>> {
    let prefix = if dir.ends_with('/') {
        dir.to_string()
    } else {
        format!("{dir}/")
    };
    let mut st = c.prepare(
        "SELECT f.path, p.position_ms, p.duration_ms, p.watched
         FROM file f JOIN playstate p ON p.item_id = f.item_id AND p.profile_id = ?1
         WHERE f.source_id = ?2 AND substr(f.path, 1, length(?3)) = ?3",
    )?;
    let rows = st.query_map(params![DEFAULT_PROFILE, LOCAL_SOURCE_ID, prefix], |r| {
        Ok((
            r.get::<_, String>(0)?,
            PlayState {
                position_ms: r.get::<_, i64>(1)? as u64,
                duration_ms: r.get::<_, i64>(2)? as u64,
                watched: r.get::<_, i64>(3)? != 0,
            },
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, s) = row?;
        if !path[prefix.len()..].contains('/') {
            out.insert(path, s);
        }
    }
    Ok(out)
}

/// The item whose track preferences apply: the show for episodes, the item itself otherwise.
pub fn pref_owner(c: &Connection, item_id: i64) -> Result<i64> {
    let mut id = item_id;
    // episode -> season -> show
    for _ in 0..2 {
        let parent: Option<i64> = c
            .query_row("SELECT parent_id FROM item WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        match parent {
            Some(p) => id = p,
            None => break,
        }
    }
    Ok(id)
}

pub fn track_pref(c: &Connection, item_id: i64) -> Result<TrackPref> {
    let owner = pref_owner(c, item_id)?;
    Ok(c
        .query_row(
            "SELECT audio_lang, sub_lang, sub_off FROM track_pref WHERE item_id = ?1 AND profile_id = ?2",
            params![owner, DEFAULT_PROFILE],
            |r| Ok(TrackPref { audio_lang: r.get::<_, Option<String>>(0)?.unwrap_or_default(), sub_lang: r.get::<_, Option<String>>(1)?.unwrap_or_default(), sub_off: r.get::<_, i64>(2)? != 0 }),
        )
        .optional()?
        .unwrap_or_default())
}

pub fn set_track_pref(c: &mut Connection, item_id: i64, p: &TrackPref) -> Result<()> {
    let owner = pref_owner(c, item_id)?;
    let nz = |s: &str| (!s.is_empty()).then(|| s.to_string());
    c.execute(
        "INSERT INTO track_pref(item_id, profile_id, audio_lang, sub_lang, sub_off) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(item_id, profile_id) DO UPDATE SET audio_lang = excluded.audio_lang,
           sub_lang = excluded.sub_lang, sub_off = excluded.sub_off",
        params![owner, DEFAULT_PROFILE, nz(&p.audio_lang), nz(&p.sub_lang), p.sub_off as i64],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    /// Issue #34: versions of one movie share its watched state and, for the same cut, its
    /// resume point; another cut keeps its own; the last played version is remembered.
    #[test]
    fn versions_share_watched_and_same_cut_resume() {
        let mut c = db();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root) VALUES (2, 'local', 'M', '/m'), (3, 'smb', 'NAS', '/n');
             INSERT INTO item(id, kind, title) VALUES (1, 'movie', 'Alien');
             INSERT INTO file(id, source_id, path, item_id, probe_json) VALUES
               (10, 2, 'Alien.2160p.mkv', 1, '{\"duration_ms\": 7020000}'),
               (11, 3, 'Alien.1080p.mkv', 1, '{\"duration_ms\": 7000000}'),
               (12, 3, 'Alien.Directors.Cut.mkv', 1, '{\"duration_ms\": 8280000}');",
        )
        .unwrap();
        assert!(same_cut(7_020_000, 7_000_000));
        assert!(!same_cut(7_020_000, 8_280_000));
        // Watch part of the 4K version, then switch to 1080p: same cut, same position.
        save_version_progress(&mut c, 1, 10, 1_800_000, 7_020_000, false).unwrap();
        assert_eq!(resume_for(&c, 1, 11, 7_000_000).unwrap(), 1_800_000);
        // The director's cut is longer: it starts at its own position (none yet).
        assert_eq!(resume_for(&c, 1, 12, 8_280_000).unwrap(), 0);
        save_version_progress(&mut c, 1, 12, 3_600_000, 8_280_000, false).unwrap();
        assert_eq!(resume_for(&c, 1, 12, 8_280_000).unwrap(), 3_600_000);
        assert_eq!(
            resume_for(&c, 1, 10, 7_020_000).unwrap(),
            1_800_000,
            "the movie's point is kept"
        );
        let last: i64 = c
            .query_row(
                "SELECT last_file_id FROM playstate WHERE item_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(last, 12, "the version played last");
        // Finishing any version marks the movie watched: one shared state.
        save_version_progress(&mut c, 1, 12, 8_280_000, 8_280_000, true).unwrap();
        assert!(playstate(&c, 1).unwrap().watched);
        save_version_progress(&mut c, 1, 11, 60_000, 7_000_000, false).unwrap();
        assert!(
            playstate(&c, 1).unwrap().watched,
            "watching another version keeps it watched"
        );
    }

    #[test]
    fn ensure_is_stable_per_path() {
        let mut c = db();
        let a = ensure_local_video(&mut c, "/m/Alien (1979).mkv", 10, 1).unwrap();
        let b = ensure_local_video(&mut c, "/m/Alien (1979).mkv", 11, 2).unwrap();
        let other = ensure_local_video(&mut c, "/m/Aliens.mkv", 10, 1).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, other);
        assert_eq!(item_title(&c, a).unwrap().as_deref(), Some("Alien (1979)"));
    }

    #[test]
    fn resume_and_watched_rules() {
        let mut c = db();
        let id = ensure_local_video(&mut c, "/m/a.mkv", 1, 1).unwrap();
        assert_eq!(playstate(&c, id).unwrap(), PlayState::default());
        // too early to resume
        assert_eq!(
            save_progress(&mut c, id, 10_000, 600_000, false)
                .unwrap()
                .position_ms,
            0
        );
        // normal resume point
        let s = save_progress(&mut c, id, 120_000, 600_000, false).unwrap();
        assert_eq!((s.position_ms, s.watched), (120_000, false));
        assert!((s.progress() - 0.2).abs() < 1e-6);
        let recent = continue_watching(&c, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].path, "/m/a.mkv");
        // credits count as finished
        let s = save_progress(&mut c, id, 590_000, 600_000, false).unwrap();
        assert_eq!((s.position_ms, s.watched), (0, true));
        assert!(continue_watching(&c, 10).unwrap().is_empty());
        // re-watching keeps the watched flag
        assert!(
            save_progress(&mut c, id, 60_000, 600_000, false)
                .unwrap()
                .watched
        );
        set_watched(&mut c, id, false).unwrap();
        assert!(!playstate(&c, id).unwrap().watched);
    }

    #[test]
    fn dir_states_only_direct_children() {
        let mut c = db();
        let a = ensure_local_video(&mut c, "/m/a.mkv", 1, 1).unwrap();
        let b = ensure_local_video(&mut c, "/m/sub/b.mkv", 1, 1).unwrap();
        save_progress(&mut c, a, 100_000, 200_000, false).unwrap();
        save_progress(&mut c, b, 100_000, 200_000, false).unwrap();
        let m = local_states_in_dir(&c, "/m").unwrap();
        assert_eq!(m.keys().collect::<Vec<_>>(), ["/m/a.mkv"]);
    }

    #[test]
    fn track_prefs_belong_to_the_show() {
        let mut c = db();
        c.execute_batch(
            "INSERT INTO item(id, kind, title) VALUES (10,'show','S'), (11,'season','S1');
             UPDATE item SET parent_id = 10 WHERE id = 11;
             INSERT INTO item(id, kind, title, parent_id) VALUES (12,'episode','E1',11), (13,'episode','E2',11);",
        )
        .unwrap();
        let p = TrackPref {
            audio_lang: "jpn".into(),
            sub_lang: "eng".into(),
            sub_off: false,
        };
        set_track_pref(&mut c, 12, &p).unwrap();
        assert_eq!(track_pref(&c, 13).unwrap(), p);
        assert_eq!(pref_owner(&c, 12).unwrap(), 10);
    }
}
