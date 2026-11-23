//! A movie that is really a show, or a show that is really a movie (2.7, issues #99 and #100):
//! files move between a movie and a show's episodes, with their watch state; items left without
//! a file go; the result is locked to the user's choice, so scans and matching keep it.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// One file of a movie or show, as the kind change lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeFile {
    pub file_id: i64,
    /// The movie or episode it belongs to now.
    pub item_id: i64,
    pub path: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    /// The movie's or show's title.
    pub owner: String,
}

/// The files of a movie, an episode, a season or a whole show.
pub fn files_of(c: &Connection, item: i64) -> Result<Vec<TreeFile>> {
    let mut st = c.prepare(
        "WITH RECURSIVE tree(id) AS (SELECT ?1 UNION ALL SELECT i.id FROM item i JOIN tree t ON i.parent_id = t.id)
         SELECT f.id, f.item_id, f.path, e.season_no, e.episode_no,
                coalesce(sh.title, se.title, e.title, '')
         FROM file f JOIN item e ON e.id = f.item_id
         LEFT JOIN item se ON se.id = e.parent_id LEFT JOIN item sh ON sh.id = se.parent_id
         WHERE f.item_id IN (SELECT id FROM tree)
         ORDER BY e.season_no, e.episode_no, f.path",
    )?;
    Ok(st
        .query_map([item], |r| {
            Ok(TreeFile {
                file_id: r.get(0)?,
                item_id: r.get(1)?,
                path: r.get(2)?,
                season: r.get(3)?,
                episode: r.get(4)?,
                owner: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Other movies with a file in one of these folders: likely more episodes of the same show.
pub fn movies_in_folders(c: &Connection, folders: &[String], except: i64) -> Result<Vec<TreeFile>> {
    let mut out = Vec::new();
    let mut st = c.prepare(
        "SELECT f.id, f.item_id, f.path, i.title FROM file f JOIN item i ON i.id = f.item_id
         WHERE i.kind = 'movie' AND i.id <> ?1 ORDER BY f.path",
    )?;
    let rows = st.query_map([except], |r| {
        Ok(TreeFile {
            file_id: r.get(0)?,
            item_id: r.get(1)?,
            path: r.get(2)?,
            owner: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ..Default::default()
        })
    })?;
    for f in rows {
        let f = f?;
        let folder = f.path.rsplit_once('/').map_or("", |(d, _)| d);
        if folders.iter().any(|d| d == folder) {
            out.push(f);
        }
    }
    Ok(out)
}

/// A top-level item of a kind with this TMDB id.
pub fn by_tmdb(c: &Connection, kind: &str, tmdb: i64) -> Result<Option<(i64, String)>> {
    Ok(c.query_row(
        "SELECT id, coalesce(title, '') FROM item WHERE kind = ?1 AND tmdb_id = ?2 AND parent_id IS NULL",
        params![kind, tmdb],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?)
}

/// The target of a kind change: the matched movie or show, created when the library has none.
fn target(
    c: &Connection,
    kind: &str,
    tmdb: i64,
    title: &str,
    year: Option<i64>,
    group_key: &str,
) -> Result<i64> {
    let id = match by_tmdb(c, kind, tmdb)? {
        Some((id, _)) => id,
        None => {
            c.execute(
                "INSERT INTO item(kind, title, sort_title, year, tmdb_id, match_state, locked, added_at, group_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'matched', 1, ?6, ?7)",
                params![kind, title, crate::text::sort_title(title), year, tmdb, now_ms(), group_key],
            )?;
            let id = c.last_insert_rowid();
            c.execute(
                "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
                params![id, title],
            )?;
            id
        }
    };
    c.execute(
        "UPDATE item SET locked = 1, match_state = 'matched', virtual = 0 WHERE id = ?1",
        [id],
    )?;
    c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [id])?;
    Ok(id)
}

/// Move a file to another item, with the watch state when the target has none.
fn move_file(c: &Connection, file: i64, to: i64) -> Result<i64> {
    let from: i64 = c.query_row("SELECT item_id FROM file WHERE id = ?1", [file], |r| {
        r.get(0)
    })?;
    c.execute(
        "UPDATE file SET item_id = ?2 WHERE id = ?1",
        params![file, to],
    )?;
    c.execute(
        "INSERT OR IGNORE INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, last_file_id)
         SELECT ?2, profile_id, position_ms, duration_ms, watched, updated_at, last_file_id FROM playstate WHERE item_id = ?1",
        params![from, to],
    )?;
    Ok(from)
}

/// Issue #99: these files of movies become episodes (`files`: file, season, episode) of the TMDB
/// show `tmdb`. Movies left without a file go. Returns the show.
pub fn movies_to_show(
    c: &mut Connection,
    files: &[(i64, i64, i64)],
    tmdb: i64,
    title: &str,
    year: Option<i64>,
    group_key: &str,
) -> Result<i64> {
    let tx = c.transaction()?;
    let show = target(&tx, "show", tmdb, title, year, group_key)?;
    let mut old = Vec::new();
    for &(file, season_no, episode_no) in files {
        let season = match tx
            .query_row(
                "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'season' AND season_no = ?2",
                params![show, season_no],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            Some(id) => id,
            None => {
                let name = format!("Season {season_no}");
                tx.execute(
                    "INSERT INTO item(kind, parent_id, title, sort_title, season_no, match_state, added_at)
                     VALUES ('season', ?1, ?2, ?3, ?4, 'matched', ?5)",
                    params![show, name, name.to_lowercase(), season_no, now_ms()],
                )?;
                tx.last_insert_rowid()
            }
        };
        let episode = match tx
            .query_row(
                "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'episode' AND episode_no = ?2",
                params![season, episode_no],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            Some(id) => id,
            None => {
                let name = format!("Episode {episode_no}");
                tx.execute(
                    "INSERT INTO item(kind, parent_id, title, sort_title, season_no, episode_no, match_state, added_at)
                     VALUES ('episode', ?1, ?2, ?3, ?4, ?5, 'matched', ?6)",
                    params![season, name, name.to_lowercase(), season_no, episode_no, now_ms()],
                )?;
                tx.last_insert_rowid()
            }
        };
        tx.execute(
            "UPDATE item SET virtual = 0 WHERE id IN (?1, ?2)",
            params![season, episode],
        )?;
        old.push(move_file(&tx, file, episode)?);
    }
    for id in old {
        tx.execute(
            "DELETE FROM item WHERE id = ?1 AND kind = 'movie' AND NOT EXISTS (SELECT 1 FROM file WHERE item_id = ?1)",
            [id],
        )?;
    }
    tx.commit()?;
    Ok(show)
}

/// Issue #100: these files of a show's episodes become the TMDB movie `tmdb` (several files are
/// its versions). Episodes left without a file become ones only TMDB knows; a show left without
/// any file goes. Returns the movie.
pub fn episodes_to_movie(
    c: &mut Connection,
    files: &[i64],
    tmdb: i64,
    title: &str,
    year: Option<i64>,
    group_key: &str,
) -> Result<i64> {
    let tx = c.transaction()?;
    let movie = target(&tx, "movie", tmdb, title, year, group_key)?;
    let mut shows = Vec::new();
    for &file in files {
        let from = move_file(&tx, file, movie)?;
        let show: Option<i64> = tx
            .query_row(
                "SELECT se.parent_id FROM item e JOIN item se ON se.id = e.parent_id WHERE e.id = ?1 AND e.kind = 'episode'",
                [from],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "UPDATE item SET virtual = 1 WHERE id = ?1 AND kind = 'episode' AND NOT EXISTS (SELECT 1 FROM file WHERE item_id = ?1)",
            [from],
        )?;
        if let Some(s) = show
            && !shows.contains(&s)
        {
            shows.push(s);
        }
    }
    for show in shows {
        let left: i64 = tx.query_row(
            "SELECT count(*) FROM file f JOIN item e ON e.id = f.item_id JOIN item se ON se.id = e.parent_id WHERE se.parent_id = ?1",
            [show],
            |r| r.get(0),
        )?;
        if left == 0 {
            tx.execute("DELETE FROM item WHERE id = ?1", [show])?;
        } else {
            tx.execute(
                "UPDATE item SET virtual = 1 WHERE parent_id = ?1 AND kind = 'season'
                   AND NOT EXISTS (SELECT 1 FROM item e WHERE e.parent_id = item.id AND e.virtual = 0)",
                [show],
            )?;
        }
    }
    tx.commit()?;
    Ok(movie)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    fn item(
        c: &Connection,
        kind: &str,
        parent: Option<i64>,
        title: &str,
        s: Option<i64>,
        e: Option<i64>,
    ) -> i64 {
        c.execute(
            "INSERT INTO item(kind, parent_id, title, season_no, episode_no, match_state) VALUES (?1, ?2, ?3, ?4, ?5, 'matched')",
            params![kind, parent, title, s, e],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    fn file(c: &Connection, item: i64, path: &str) -> i64 {
        c.execute(
            "INSERT INTO file(source_id, path, item_id) VALUES (1, ?1, ?2)",
            params![path, item],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    fn watched(c: &Connection, item: i64) {
        c.execute(
            "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at) VALUES (?1, 1, 0, 100, 1, 1)",
            [item],
        )
        .unwrap();
    }

    #[test]
    fn movies_become_episodes_with_their_watch_state() {
        let mut c = db();
        let m1 = item(&c, "movie", None, "The Long Harbour", None, None);
        let m2 = item(&c, "movie", None, "The Long Harbour 2", None, None);
        let f1 = file(&c, m1, "/tv/Long Harbour/ep1.mkv");
        let f2 = file(&c, m2, "/tv/Long Harbour/ep2.mkv");
        watched(&c, m1);
        let found = movies_in_folders(&c, &["/tv/Long Harbour".into()], m1).unwrap();
        assert_eq!(found.iter().map(|f| f.file_id).collect::<Vec<_>>(), [f2]);
        let show = movies_to_show(
            &mut c,
            &[(f1, 1, 1), (f2, 1, 2)],
            77,
            "The Long Harbour",
            Some(2019),
            "long harbour",
        )
        .unwrap();
        let files = files_of(&c, show).unwrap();
        assert_eq!(
            files
                .iter()
                .map(|f| (f.file_id, f.season, f.episode))
                .collect::<Vec<_>>(),
            [(f1, Some(1), Some(1)), (f2, Some(1), Some(2))]
        );
        let movies: i64 = c
            .query_row("SELECT count(*) FROM item WHERE kind = 'movie'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(movies, 0, "the emptied movies are gone");
        let w: i64 = c
            .query_row(
                "SELECT watched FROM playstate WHERE item_id = ?1",
                [files[0].item_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(w, 1);
        let (locked, tmdb): (i64, i64) = c
            .query_row(
                "SELECT locked, tmdb_id FROM item WHERE id = ?1",
                [show],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((locked, tmdb), (1, 77));
        // An existing show with that id takes the files.
        let m3 = item(&c, "movie", None, "Episode three", None, None);
        let f3 = file(&c, m3, "/tv/Long Harbour/ep3.mkv");
        assert_eq!(
            movies_to_show(&mut c, &[(f3, 1, 3)], 77, "x", None, "x").unwrap(),
            show
        );
        assert_eq!(files_of(&c, show).unwrap().len(), 3);
    }

    #[test]
    fn episodes_become_a_movie_and_an_empty_show_goes() {
        let mut c = db();
        let show = item(&c, "show", None, "Night Shift", None, None);
        let s1 = item(&c, "season", Some(show), "Season 1", Some(1), None);
        let e1 = item(&c, "episode", Some(s1), "Pilot", Some(1), Some(1));
        let e2 = item(&c, "episode", Some(s1), "Two", Some(1), Some(2));
        let f1 = file(&c, e1, "/m/Night Shift (2019)/Night.Shift.2019.mkv");
        let f2 = file(
            &c,
            e2,
            "/m/Night Shift (2019)/Night.Shift.2019.Directors.Cut.mkv",
        );
        watched(&c, e1);
        // One episode only: the show keeps the other, the moved one is TMDB's only now.
        let movie = episodes_to_movie(
            &mut c,
            &[f1],
            9,
            "Night Shift",
            Some(2019),
            "night shift|2019",
        )
        .unwrap();
        let v: i64 = c
            .query_row("SELECT virtual FROM item WHERE id = ?1", [e1], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
        assert_eq!(files_of(&c, show).unwrap().len(), 1);
        let w: i64 = c
            .query_row(
                "SELECT watched FROM playstate WHERE item_id = ?1",
                [movie],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(w, 1);
        // The other one joins the movie as a version; the show has no file left and goes.
        assert_eq!(
            episodes_to_movie(&mut c, &[f2], 9, "x", None, "x").unwrap(),
            movie
        );
        assert_eq!(files_of(&c, movie).unwrap().len(), 2);
        let shows: i64 = c
            .query_row(
                "SELECT count(*) FROM item WHERE kind IN ('show', 'season', 'episode')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(shows, 0);
    }
}
