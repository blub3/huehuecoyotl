//! Merging duplicate items (issue #32). A show or movie that exists twice (split by an older
//! scanner, or matched to the same TMDB id from two folders) becomes one item: seasons and
//! episodes with the same number are merged, files move, and the newer watch state wins.
//!
//! Also the alias table: when an item is merged away, its grouping key keeps pointing at the
//! surviving item, so the next scan of those files finds it instead of creating a new one.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{Result, jobs};

/// Grouping keys of merged items, pointing at the surviving item (`kind`, `key`) -> item.
const ALIAS_TABLE: &str = "
CREATE TABLE group_alias(kind TEXT NOT NULL, key TEXT NOT NULL,
                         item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         PRIMARY KEY (kind, key)) WITHOUT ROWID;
CREATE INDEX group_alias_item ON group_alias(item_id);";

/// At most one top-level movie and one show per TMDB id.
const UNIQUE_TMDB: &str = "
CREATE UNIQUE INDEX item_tmdb ON item(kind, tmdb_id)
  WHERE tmdb_id IS NOT NULL AND parent_id IS NULL AND kind IN ('movie', 'show');";

/// Migration 4: the alias table, the TMDB duplicates merged, the unique index, and the
/// offline `regroup` job that re-keys shows with the current parser.
pub(crate) fn migration_4(tx: &Transaction) -> Result<()> {
    tx.execute_batch(ALIAS_TABLE)?;
    let merged = merge_tmdb_duplicates(tx)?;
    tx.execute_batch(UNIQUE_TMDB)?;
    // Only a library with shows has anything to regroup (a new database has none).
    let shows: i64 = tx.query_row("SELECT count(*) FROM item WHERE kind = 'show'", [], |r| {
        r.get(0)
    })?;
    if shows > 0 {
        jobs::enqueue(
            tx,
            "regroup",
            "{}",
            jobs::priority::BACKGROUND,
            Some("regroup"),
        )?;
    }
    tracing::info!(merged, "duplicate shows and movies merged");
    Ok(())
}

/// Merge every group of top-level movies or shows that share a TMDB id. Returns how many
/// items were merged away.
pub fn merge_tmdb_duplicates(c: &Connection) -> Result<usize> {
    let groups: Vec<(String, i64)> = {
        let mut st = c.prepare(
            "SELECT kind, tmdb_id FROM item
             WHERE tmdb_id IS NOT NULL AND parent_id IS NULL AND kind IN ('movie', 'show')
             GROUP BY kind, tmdb_id HAVING count(*) > 1",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut merged = 0;
    for (kind, tmdb) in groups {
        let ids: Vec<i64> = {
            let mut st = c.prepare(&format!(
                "SELECT id FROM item WHERE kind = ?1 AND tmdb_id = ?2 AND parent_id IS NULL
                 ORDER BY {PREFERENCE}"
            ))?;
            let rows = st.query_map(params![kind, tmdb], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let (keep, rest) = ids.split_first().expect("a group has at least two items");
        for drop in rest {
            merge_into(c, *keep, *drop)?;
            merged += 1;
        }
    }
    Ok(merged)
}

/// Which of several duplicates survives: the user's locked choice, then a matched item, then the
/// item with watch history, then the one with the most episodes, then the oldest.
const PREFERENCE: &str = "locked DESC, tmdb_id IS NOT NULL DESC,
    (SELECT count(*) FROM playstate p WHERE p.item_id = item.id
        OR p.item_id IN (SELECT e.id FROM item s JOIN item e ON e.parent_id = s.id WHERE s.parent_id = item.id)) DESC,
    (SELECT count(*) FROM item s JOIN item e ON e.parent_id = s.id WHERE s.parent_id = item.id) DESC,
    id";

/// The surviving item among `a` and `b` (see [`PREFERENCE`]).
pub fn preferred(c: &Connection, a: i64, b: i64) -> Result<i64> {
    Ok(c.query_row(
        &format!("SELECT id FROM item WHERE id IN (?1, ?2) ORDER BY {PREFERENCE} LIMIT 1"),
        params![a, b],
        |r| r.get(0),
    )?)
}

/// Before `item` takes `tmdb_id`: if another top-level item of the same kind already has it,
/// merge the two. Returns the surviving item, which then carries the id.
pub fn claim_tmdb(c: &Connection, item: i64, tmdb_id: i64) -> Result<i64> {
    let other: Option<i64> = c
        .query_row(
            "SELECT o.id FROM item o JOIN item i ON i.id = ?1
             WHERE o.kind = i.kind AND o.tmdb_id = ?2 AND o.parent_id IS NULL AND o.id != ?1",
            params![item, tmdb_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(other) = other else {
        return Ok(item);
    };
    let keep = preferred(c, item, other)?;
    let drop = if keep == item { other } else { item };
    merge_into(c, keep, drop)?;
    Ok(keep)
}

/// Merge item `drop` into `keep` (same kind) and delete `drop`. Children with the same season
/// or episode number are merged recursively; the others move under `keep`.
pub fn merge_into(c: &Connection, keep: i64, drop: i64) -> Result<()> {
    if keep == drop {
        return Ok(());
    }
    let children: Vec<(i64, String, Option<i64>, Option<i64>)> = {
        let mut st =
            c.prepare("SELECT id, kind, season_no, episode_no FROM item WHERE parent_id = ?1")?;
        let rows = st.query_map([drop], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (child, kind, season, episode) in children {
        let twin: Option<i64> = match kind.as_str() {
            "season" => c
                .query_row(
                    "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'season' AND season_no IS ?2",
                    params![keep, season],
                    |r| r.get(0),
                )
                .optional()?,
            "episode" => c
                .query_row(
                    "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'episode' AND episode_no IS ?2",
                    params![keep, episode],
                    |r| r.get(0),
                )
                .optional()?,
            _ => None,
        };
        match twin {
            Some(t) => merge_into(c, t, child)?,
            None => {
                c.execute(
                    "UPDATE item SET parent_id = ?1 WHERE id = ?2",
                    params![keep, child],
                )?;
            }
        }
    }
    c.execute(
        "UPDATE file SET item_id = ?1 WHERE item_id = ?2",
        params![keep, drop],
    )?;
    // Watch state: per profile, the newer of the two.
    c.execute(
        "UPDATE playstate SET position_ms = d.position_ms, duration_ms = d.duration_ms,
                watched = d.watched, updated_at = d.updated_at
         FROM (SELECT * FROM playstate WHERE item_id = ?2) AS d
         WHERE playstate.item_id = ?1 AND playstate.profile_id = d.profile_id
           AND d.updated_at > playstate.updated_at",
        params![keep, drop],
    )?;
    c.execute(
        "UPDATE OR IGNORE playstate SET item_id = ?1 WHERE item_id = ?2",
        params![keep, drop],
    )?;
    c.execute(
        "UPDATE OR IGNORE track_pref SET item_id = ?1 WHERE item_id = ?2",
        params![keep, drop],
    )?;
    // Artwork the survivor lacks moves over; the rest goes with the merged item.
    c.execute(
        "UPDATE artwork SET item_id = ?1 WHERE item_id = ?2
           AND role NOT IN (SELECT role FROM artwork WHERE item_id = ?1)",
        params![keep, drop],
    )?;
    // A match the survivor lacks moves over with everything TMDB said (the id is unique, so
    // the merged item gives it up first).
    let moved_match: Option<i64> = c
        .query_row(
            "SELECT d.tmdb_id FROM item d JOIN item k ON k.id = ?1
             WHERE d.id = ?2 AND d.tmdb_id IS NOT NULL AND k.tmdb_id IS NULL",
            params![keep, drop],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(tmdb) = moved_match {
        c.execute("UPDATE item SET tmdb_id = NULL WHERE id = ?1", [drop])?;
        c.execute(
            "UPDATE item SET (tmdb_id, imdb_id, title, sort_title, original_title, overview, rating,
                              runtime_min, meta_json, match_state, meta_fetched_at) =
                 (SELECT ?3, imdb_id, title, sort_title, original_title, overview, rating,
                         runtime_min, meta_json, match_state, meta_fetched_at FROM item WHERE id = ?2)
             WHERE id = ?1",
            params![keep, drop, tmdb],
        )?;
        c.execute(
            "DELETE FROM artwork WHERE item_id = ?1 AND role IN (SELECT role FROM artwork WHERE item_id = ?2)",
            params![keep, drop],
        )?;
    }
    // Facts the survivor lacks; a lock or a match on either side is kept.
    c.execute(
        "UPDATE item SET
           year = coalesce(year, (SELECT year FROM item WHERE id = ?2)),
           overview = coalesce(overview, (SELECT overview FROM item WHERE id = ?2)),
           locked = max(locked, (SELECT locked FROM item WHERE id = ?2)),
           added_at = min(coalesce(added_at, 9e18), coalesce((SELECT added_at FROM item WHERE id = ?2), 9e18))
         WHERE id = ?1",
        params![keep, drop],
    )?;
    // The merged item's grouping keys now lead to the survivor.
    c.execute(
        "UPDATE OR IGNORE group_alias SET item_id = ?1 WHERE item_id = ?2",
        params![keep, drop],
    )?;
    c.execute(
        "INSERT OR IGNORE INTO group_alias(kind, key, item_id)
         SELECT kind, group_key, ?1 FROM item
         WHERE id = ?2 AND parent_id IS NULL AND group_key IS NOT NULL
           AND group_key IS NOT (SELECT group_key FROM item WHERE id = ?1)",
        params![keep, drop],
    )?;
    c.execute(
        "DELETE FROM job WHERE state = 'queued' AND json_extract(payload_json, '$.item_id') = ?1",
        [drop],
    )?;
    c.execute("DELETE FROM item_fts WHERE rowid = ?1", [drop])?;
    c.execute("DELETE FROM item WHERE id = ?1", [drop])?;
    Ok(())
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

    /// A database written by 1.1.0 (schema 3) with the duplicates of issue #32: the upgrade
    /// merges them, keeps the watch state, makes the TMDB id unique and queues the regroup.
    #[test]
    fn upgrade_from_1_1_0_repairs_duplicates() {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        for m in &crate::MIGRATIONS[..3] {
            match m {
                crate::Migration::Sql(sql) => c.execute_batch(sql).unwrap(),
                crate::Migration::Rust(_) => unreachable!("1.1.0 had SQL migrations only"),
            }
        }
        c.pragma_update(None, "user_version", 3).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root) VALUES (2, 'local', 'M', '/m');
             -- one show matched twice (per season release folder), one movie matched twice
             INSERT INTO item(id, kind, title, group_key, tmdb_id, match_state) VALUES
               (10, 'show', 'Show', 'show s01 web', 7, 'matched'), (20, 'show', 'Show', 'show s02 web', 7, 'matched'),
               (30, 'movie', 'Alien', 'alien|1979', 348, 'matched'), (40, 'movie', 'Alien', 'alien|', 348, 'matched');
             INSERT INTO item(id, kind, parent_id, season_no) VALUES (11, 'season', 10, 1), (21, 'season', 20, 2);
             INSERT INTO item(id, kind, parent_id, episode_no) VALUES (12, 'episode', 11, 1), (22, 'episode', 21, 1);
             INSERT INTO file(source_id, path, item_id) VALUES (2, 's1/e1', 12), (2, 's2/e1', 22), (2, 'a.mkv', 30), (2, 'b.mkv', 40);
             INSERT INTO playstate(item_id, position_ms, watched, updated_at) VALUES (22, 90000, 0, 5), (40, 0, 1, 9);",
        )
        .unwrap();
        assert_eq!(crate::migrate(&mut c).unwrap(), crate::MIGRATIONS.len());
        let q = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(q("SELECT count(*) FROM item WHERE kind = 'show'"), 1);
        assert_eq!(q("SELECT count(*) FROM item WHERE kind = 'movie'"), 1);
        assert_eq!(q("SELECT count(*) FROM item WHERE kind = 'season'"), 2);
        assert_eq!(q("SELECT count(*) FROM file WHERE item_id IS NOT NULL"), 4);
        assert_eq!(q("SELECT count(*) FROM playstate"), 2, "watch state intact");
        assert_eq!(q("SELECT count(*) FROM job WHERE kind = 'regroup'"), 1);
        assert!(
            c.execute("UPDATE item SET tmdb_id = 7 WHERE kind = 'movie'", [])
                .is_ok(),
            "a movie may share a number with a show"
        );
        assert!(
            c.execute(
                "INSERT INTO item(kind, title, tmdb_id) VALUES ('show', 'Show again', 7)",
                []
            )
            .is_err(),
            "but no second show with the same TMDB id"
        );
    }

    #[test]
    fn merging_keeps_episodes_files_and_the_newer_watch_state() {
        let c = db();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root) VALUES (2, 'local', 'M', '/m');
             -- show 10: S01 with E1, E2; show 20: S01 with E2 (watched later), S02 with E1
             INSERT INTO item(id, kind, title, group_key, tmdb_id) VALUES (10, 'show', 'Show', 'show', 7), (20, 'show', 'Show S01 2160p', 'show s01 2160p', NULL);
             INSERT INTO item(id, kind, parent_id, season_no) VALUES (11, 'season', 10, 1), (21, 'season', 20, 1), (22, 'season', 20, 2);
             INSERT INTO item(id, kind, parent_id, episode_no) VALUES (12, 'episode', 11, 1), (13, 'episode', 11, 2), (23, 'episode', 21, 2), (24, 'episode', 22, 1);
             INSERT INTO file(source_id, path, item_id) VALUES (2, 'a/e1', 12), (2, 'a/e2', 13), (2, 'b/e2', 23), (2, 'b/s2e1', 24);
             INSERT INTO playstate(item_id, position_ms, watched, updated_at) VALUES (13, 1000, 0, 100), (23, 0, 1, 200), (24, 5000, 0, 50);",
        )
        .unwrap();
        merge_into(&c, 10, 20).unwrap();
        let q = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(q("SELECT count(*) FROM item WHERE kind = 'show'"), 1);
        assert_eq!(
            q("SELECT count(*) FROM item WHERE kind = 'season' AND parent_id = 10"),
            2
        );
        assert_eq!(q("SELECT count(*) FROM item WHERE kind = 'episode'"), 3);
        assert_eq!(
            q("SELECT count(*) FROM file WHERE item_id = 13"),
            2,
            "both versions of E2"
        );
        assert_eq!(
            q("SELECT watched FROM playstate WHERE item_id = 13"),
            1,
            "newer state wins"
        );
        assert_eq!(
            q("SELECT position_ms FROM playstate WHERE item_id = 24"),
            5000,
            "moved along"
        );
        assert_eq!(
            q("SELECT item_id FROM group_alias WHERE kind = 'show' AND key = 'show s01 2160p'"),
            10
        );
    }

    #[test]
    fn tmdb_duplicates_merge_into_the_preferred_item() {
        let c = db();
        c.execute_batch(
            "DROP INDEX item_tmdb;
             INSERT INTO item(id, kind, title, tmdb_id, locked) VALUES (1, 'movie', 'Alien', 348, 0), (2, 'movie', 'Alien', 348, 1), (3, 'movie', 'Aliens', 679, 0);",
        )
        .unwrap();
        assert_eq!(merge_tmdb_duplicates(&c).unwrap(), 1);
        let ids: Vec<i64> = c
            .prepare("SELECT id FROM item ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(ids, [2, 3], "the locked one survives");
        c.execute_batch(UNIQUE_TMDB).unwrap();
        assert!(
            c.execute(
                "INSERT INTO item(kind, title, tmdb_id) VALUES ('movie', 'x', 679)",
                []
            )
            .is_err(),
            "unique TMDB id"
        );
        c.execute(
            "INSERT INTO item(id, kind, title) VALUES (4, 'movie', 'Aliens (dup)')",
            [],
        )
        .unwrap();
        assert_eq!(claim_tmdb(&c, 4, 679).unwrap(), 3);
        assert_eq!(
            c.query_row("SELECT count(*) FROM item WHERE id = 4", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
